//! NS-P1/P3 at the policy point itself (`reify_compiler::name_env`), driven
//! over SYNTHETIC module sets.
//!
//! Post-β the real stdlib has no same-kind collision, so first-wins among
//! peers has no live stdlib witness: it is pinned here instead. Every
//! fixture prelude is ordered `[std.p1, std.p2, a, b]` — the `stdlib ++
//! imports` shape `program::compile_program` builds — so the tiering
//! (local, then imports in slice order, then std modules in slice order) is
//! observable rather than an accident of slice order.

use std::collections::HashSet;

use reify_compiler::name_env::{
    Bindings, Definition, FunctionSignature, Origin, Tier, bind, resolve,
};
use reify_compiler::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    TopologyTemplate,
};
use reify_core::{ContentHash, ModulePath, SourceSpan, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledFnBody, CompiledFunction, EnumDef};
use reify_test_support::{
    CompiledModuleBuilder, CompiledPurposeBuilder, CompiledTraitBuilder, TopologyTemplateBuilder,
};

/// Which names each prelude module declares, for EVERY kind. The expected
/// outcome per name (prelude order `[std.p1, std.p2, a, b]`):
/// - `Shared`: local beats a, b, std.p1, std.p2
/// - `ImportStd`: a beats its import peer b and both std modules
/// - `StdOnly`: std.p1 beats its std peer std.p2
/// - `LocalImport`: local beats b
/// - `LocalStd`: local beats std.p2
/// - `Solo`, `LocalSolo`: no collision
const PRELUDE_PLAN: &[(&str, &[&str])] = &[
    ("std.p1", &["Shared", "ImportStd", "StdOnly"]),
    ("std.p2", &["Shared", "ImportStd", "StdOnly", "LocalStd"]),
    ("a", &["Shared", "ImportStd", "Solo"]),
    ("b", &["Shared", "ImportStd", "LocalImport"]),
];
const LOCAL_PLAN: &[&str] = &["Shared", "LocalImport", "LocalStd", "LocalSolo"];

/// `(name, winner origin, shadowed origin)` for every losing definition.
const EXPECTED_SHADOWINGS: &[(&str, &str, &str)] = &[
    ("ImportStd", "a", "b"),
    ("ImportStd", "a", "std.p1"),
    ("ImportStd", "a", "std.p2"),
    ("LocalImport", "local", "b"),
    ("LocalStd", "local", "std.p2"),
    ("Shared", "local", "a"),
    ("Shared", "local", "b"),
    ("Shared", "local", "std.p1"),
    ("Shared", "local", "std.p2"),
    ("StdOnly", "std.p1", "std.p2"),
];

/// A name declared non-pub in module `a` for every kind that carries a
/// prelude-visibility filter (aliases, constraint defs, purposes).
const HIDDEN: &str = "Hidden";

fn module_path(label: &str) -> ModulePath {
    ModulePath::from_dotted(label).expect("valid dotted module label")
}

fn origin_label(origin: Origin<'_>) -> String {
    match origin {
        Origin::Local => "local".to_string(),
        Origin::Module(path) => path.0.join("."),
    }
}

fn expected_tier(label: &str) -> Tier {
    match label {
        "local" => Tier::Local,
        "a" | "b" => Tier::Import,
        "std.p1" | "std.p2" => Tier::Stdlib,
        other => panic!("no expected tier for origin label {other}"),
    }
}

// ─── per-kind synthetic definitions ────────────────────────────────────────

fn template(name: &str) -> TopologyTemplate {
    TopologyTemplateBuilder::new(name).build()
}

fn enum_def(name: &str) -> EnumDef {
    EnumDef {
        name: name.to_string(),
        variants: vec!["V".into()],
        doc: None,
        type_params: vec![],
        is_pub: true,
    }
}

fn trait_def(name: &str) -> CompiledTrait {
    CompiledTraitBuilder::new(name).build()
}

fn function(name: &str, param: &str, param_ty: Type, return_ty: Type) -> CompiledFunction {
    let params = vec![(param.to_string(), param_ty)];
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: return_ty.clone(),
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::value_ref(ValueCellId::new(name, param), return_ty),
        },
        content_hash: ContentHash::of_str(name),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    }
}

fn int_fn(name: &str) -> CompiledFunction {
    function(name, "x", Type::Int, Type::Int)
}

fn type_alias(name: &str, is_pub: bool) -> CompiledTypeAlias {
    CompiledTypeAlias {
        name: name.to_string(),
        resolved_type: Some(Type::length()),
        type_params: vec![],
        type_expr: None,
        body_scope: None,
        is_pub,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str(name),
    }
}

fn constraint_def(name: &str, is_pub: bool) -> CompiledConstraintDef {
    CompiledConstraintDef {
        name: name.to_string(),
        is_pub,
        type_params: vec![],
        params: vec![],
        predicates: vec![],
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str(name),
        pragmas: vec![],
        annotations: vec![],
        annotations_optimized_target: None,
    }
}

fn purpose(name: &str, is_pub: bool) -> CompiledPurpose {
    let builder = CompiledPurposeBuilder::new(name);
    if is_pub { builder.public() } else { builder }.build()
}

// ─── the fixture ───────────────────────────────────────────────────────────

/// One synthetic name environment: a local surface per kind plus the prelude
/// `[std.p1, std.p2, a, b]`.
struct Fixture {
    prelude: Vec<CompiledModule>,
    templates: Vec<TopologyTemplate>,
    enums: Vec<EnumDef>,
    traits: Vec<CompiledTrait>,
    functions: Vec<CompiledFunction>,
    aliases: Vec<CompiledTypeAlias>,
    constraint_defs: Vec<CompiledConstraintDef>,
    purposes: Vec<CompiledPurpose>,
}

/// Overloads layered on top of the uniform plan: `a` declares `over(Int)`,
/// `b` declares `over(Length)` (a different signature — both must bind), and
/// `std.p1` declares `over(Int)` again under a different PARAM NAME and
/// RETURN TYPE (the same key as `a`'s — it must lose to `a`).
fn overloads_for(label: &str) -> Vec<CompiledFunction> {
    match label {
        "a" => vec![function("over", "x", Type::Int, Type::Int)],
        "b" => vec![function("over", "x", Type::length(), Type::Int)],
        "std.p1" => vec![function("over", "renamed", Type::Int, Type::length())],
        _ => vec![],
    }
}

fn build_module(label: &str, names: &[&str]) -> CompiledModule {
    let mut builder = CompiledModuleBuilder::new(module_path(label));
    for name in names {
        builder = builder
            .template(template(name))
            .enum_def(enum_def(name))
            .trait_def(trait_def(name))
            .function(int_fn(name))
            .type_alias(type_alias(name, true))
            .compiled_purpose(purpose(name, true));
    }
    for f in overloads_for(label) {
        builder = builder.function(f);
    }
    if label == "a" {
        builder = builder
            .type_alias(type_alias(HIDDEN, false))
            .compiled_purpose(purpose(HIDDEN, false));
    }
    let mut module = builder.build();
    module.constraint_defs = names.iter().map(|n| constraint_def(n, true)).collect();
    if label == "a" {
        module.constraint_defs.push(constraint_def(HIDDEN, false));
    }
    module
}

/// Local definitions are deliberately NON-pub: the local tier is never
/// visibility-filtered.
fn fixture() -> Fixture {
    Fixture {
        prelude: PRELUDE_PLAN
            .iter()
            .map(|(label, names)| build_module(label, names))
            .collect(),
        templates: LOCAL_PLAN.iter().map(|n| template(n)).collect(),
        enums: LOCAL_PLAN.iter().map(|n| enum_def(n)).collect(),
        traits: LOCAL_PLAN.iter().map(|n| trait_def(n)).collect(),
        functions: LOCAL_PLAN.iter().map(|n| int_fn(n)).collect(),
        aliases: LOCAL_PLAN.iter().map(|n| type_alias(n, false)).collect(),
        constraint_defs: LOCAL_PLAN
            .iter()
            .map(|n| constraint_def(n, false))
            .collect(),
        purposes: LOCAL_PLAN.iter().map(|n| purpose(n, false)).collect(),
    }
}

impl Fixture {
    fn module(&self, label: &str) -> &CompiledModule {
        self.prelude
            .iter()
            .find(|m| origin_label(Origin::Module(&m.path)) == label)
            .unwrap_or_else(|| panic!("fixture has no module {label}"))
    }
}

// ─── (a) precedence, templates ─────────────────────────────────────────────

fn template_in<'a>(templates: &'a [TopologyTemplate], name: &str) -> &'a TopologyTemplate {
    templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no template {name}"))
}

#[test]
fn local_beats_every_prelude_definition() {
    let fx = fixture();
    let bound = resolve(&fx.templates, &fx.prelude, "Shared").expect("Shared resolves");
    assert!(
        std::ptr::eq(bound, template_in(&fx.templates, "Shared")),
        "the local Shared must win over a, b, std.p1 and std.p2"
    );
}

#[test]
fn import_tier_beats_std_tier_even_though_std_comes_first_in_the_slice() {
    let fx = fixture();
    let bound = resolve::<TopologyTemplate, _, _>(&[], &fx.prelude, "Shared").expect("resolves");
    assert!(
        std::ptr::eq(bound, template_in(&fx.module("a").templates, "Shared")),
        "with no local Shared, import `a` must win although std.p1 precedes it in the slice"
    );
}

#[test]
fn first_wins_among_import_peers_and_among_std_peers() {
    let fx = fixture();
    let import_winner = resolve(&fx.templates, &fx.prelude, "ImportStd").expect("resolves");
    assert!(
        std::ptr::eq(
            import_winner,
            template_in(&fx.module("a").templates, "ImportStd")
        ),
        "a precedes b in the slice, so a's ImportStd wins"
    );
    let std_winner = resolve(&fx.templates, &fx.prelude, "StdOnly").expect("resolves");
    assert!(
        std::ptr::eq(
            std_winner,
            template_in(&fx.module("std.p1").templates, "StdOnly")
        ),
        "std.p1 precedes std.p2 in the slice, so std.p1's StdOnly wins"
    );
}

#[test]
fn winners_are_listed_in_precedence_order() {
    let fx = fixture();
    let bindings = bind(&fx.templates, &fx.prelude);
    let order: Vec<&str> = bindings.winners().map(|t| t.name.as_str()).collect();
    assert_eq!(
        order,
        vec![
            "Shared",
            "LocalImport",
            "LocalStd",
            "LocalSolo",
            "ImportStd",
            "Solo",
            "StdOnly"
        ],
        "locals first, then import winners in slice order, then std winners in slice order"
    );
    let map = bindings.into_map();
    assert_eq!(map.len(), order.len(), "into_map holds exactly the winners");
}

// ─── (b) lookup / table agreement property ─────────────────────────────────

/// For every key, the lazy lookup and the bound table return the SAME
/// reference — or both return `None`.
fn assert_lookup_agrees_with_table<'a, D: Definition>(
    kind: &str,
    local: &'a [D],
    prelude: &'a [CompiledModule],
    keys: &[D::Key<'a>],
) {
    let bindings = bind(local, prelude);
    for key in keys {
        let looked_up = resolve(local, prelude, key);
        let tabled = bindings.get(key);
        match (looked_up, tabled) {
            (None, None) => {}
            (Some(l), Some(t)) => assert!(
                std::ptr::eq(l, t),
                "{kind} {key:?}: resolve and bind disagree on the winner"
            ),
            (l, t) => panic!(
                "{kind} {key:?}: resolve found {} but bind found {}",
                l.is_some(),
                t.is_some()
            ),
        }
    }
}

/// Every key declared anywhere — local, any prelude module, pub or not —
/// plus one absent key.
fn all_keys<'a, D: Definition>(
    local: &'a [D],
    prelude: &'a [CompiledModule],
    raw: impl Fn(&'a CompiledModule) -> &'a [D],
    absent: D::Key<'a>,
) -> Vec<D::Key<'a>> {
    local
        .iter()
        .chain(prelude.iter().flat_map(raw))
        .map(Definition::key)
        .chain(std::iter::once(absent))
        .collect()
}

#[test]
fn resolve_and_bind_agree_for_every_key_of_every_kind() {
    let fx = fixture();
    let p = &fx.prelude;
    let absent_fn = int_fn("Absent");

    let keys = all_keys(&fx.templates, p, |m| m.templates.as_slice(), "Absent");
    assert_lookup_agrees_with_table("template", &fx.templates, p, &keys);
    let keys = all_keys(&fx.enums, p, |m| m.enum_defs.as_slice(), "Absent");
    assert_lookup_agrees_with_table("enum", &fx.enums, p, &keys);
    let keys = all_keys(&fx.traits, p, |m| m.trait_defs.as_slice(), "Absent");
    assert_lookup_agrees_with_table("trait", &fx.traits, p, &keys);
    let keys = all_keys(
        &fx.functions,
        p,
        |m| m.functions.as_slice(),
        absent_fn.key(),
    );
    assert_lookup_agrees_with_table("function", &fx.functions, p, &keys);
    let keys = all_keys(&fx.aliases, p, |m| m.type_aliases.as_slice(), "Absent");
    assert_lookup_agrees_with_table("type alias", &fx.aliases, p, &keys);
    let keys = all_keys(
        &fx.constraint_defs,
        p,
        |m| m.constraint_defs.as_slice(),
        "Absent",
    );
    assert_lookup_agrees_with_table("constraint def", &fx.constraint_defs, p, &keys);
    let keys = all_keys(
        &fx.purposes,
        p,
        |m| m.compiled_purposes.as_slice(),
        "Absent",
    );
    assert_lookup_agrees_with_table("purpose", &fx.purposes, p, &keys);
}

// ─── (c) shadowings ────────────────────────────────────────────────────────

/// `(name, winner origin, shadowed origin)` rows for every recorded
/// shadowing, after checking each record is internally consistent.
fn shadowing_rows<'a, D: Definition>(
    kind: &str,
    bindings: &Bindings<'a, D>,
    name_of: impl Fn(&D) -> &str,
) -> Vec<(String, String, String)> {
    let mut rows: Vec<(String, String, String)> = bindings
        .shadowings()
        .iter()
        .map(|s| {
            let winner = origin_label(s.winner.origin);
            let shadowed = origin_label(s.shadowed.origin);
            assert_eq!(
                s.winner.origin.tier(),
                expected_tier(&winner),
                "{kind}: winner tier"
            );
            assert_eq!(
                s.shadowed.origin.tier(),
                expected_tier(&shadowed),
                "{kind}: shadowed tier"
            );
            assert!(
                s.winner.definition.key() == s.key && s.shadowed.definition.key() == s.key,
                "{kind}: a shadowing pairs two definitions of its own key"
            );
            assert!(
                bindings
                    .get(&s.key)
                    .is_some_and(|w| std::ptr::eq(w, s.winner.definition)),
                "{kind}: the recorded winner is the bound definition"
            );
            (name_of(s.shadowed.definition).to_string(), winner, shadowed)
        })
        .collect();
    rows.sort();
    rows
}

fn expected_rows(extra: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
    let mut rows: Vec<(String, String, String)> = EXPECTED_SHADOWINGS
        .iter()
        .chain(extra)
        .map(|(n, w, s)| (n.to_string(), w.to_string(), s.to_string()))
        .collect();
    rows.sort();
    rows
}

#[test]
fn one_shadowing_per_losing_definition_for_every_kind() {
    let fx = fixture();
    let p = &fx.prelude;
    let expected = expected_rows(&[]);

    let rows = shadowing_rows("template", &bind(&fx.templates, p), |t| &t.name);
    assert_eq!(rows, expected, "template shadowings");
    let rows = shadowing_rows("enum", &bind(&fx.enums, p), |e| &e.name);
    assert_eq!(rows, expected, "enum shadowings");
    let rows = shadowing_rows("trait", &bind(&fx.traits, p), |t| &t.name);
    assert_eq!(rows, expected, "trait shadowings");
    let rows = shadowing_rows("type alias", &bind(&fx.aliases, p), |a| &a.name);
    assert_eq!(rows, expected, "type alias shadowings");
    let rows = shadowing_rows("constraint def", &bind(&fx.constraint_defs, p), |c| &c.name);
    assert_eq!(rows, expected, "constraint def shadowings");
    let rows = shadowing_rows("purpose", &bind(&fx.purposes, p), |q| &q.name);
    assert_eq!(rows, expected, "purpose shadowings");

    let rows = shadowing_rows("function", &bind(&fx.functions, p), |f| &f.name);
    assert_eq!(
        rows,
        expected_rows(&[("over", "a", "std.p1")]),
        "function shadowings: the uniform plan plus std.p1's over(Int) losing to a's"
    );
}

#[test]
fn collision_free_keys_record_no_shadowing() {
    let fx = fixture();
    let bindings = bind(&fx.templates, &fx.prelude);
    let shadowed_names: HashSet<&str> = bindings
        .shadowings()
        .iter()
        .map(|s| s.winner.definition.name.as_str())
        .collect();
    for name in ["Solo", "LocalSolo"] {
        assert!(bindings.get(name).is_some(), "{name} is bound");
        assert!(
            !shadowed_names.contains(name),
            "{name} collides with nothing"
        );
    }
}

// ─── (d) per-kind keys and visibility ──────────────────────────────────────

fn function_in<'a>(m: &'a CompiledModule, name: &str, param_ty: &Type) -> &'a CompiledFunction {
    m.functions
        .iter()
        .find(|f| f.name == name && f.params[0].1 == *param_ty)
        .unwrap_or_else(|| panic!("no fn {name}({param_ty:?})"))
}

#[test]
fn functions_are_keyed_by_name_and_param_types_only() {
    let fx = fixture();
    let a_over = function_in(fx.module("a"), "over", &Type::Int);
    let b_over = function_in(fx.module("b"), "over", &Type::length());
    let std_over = function_in(fx.module("std.p1"), "over", &Type::Int);

    assert!(
        std_over.key() == a_over.key(),
        "param names and return type are not part of a function's key"
    );
    assert!(
        b_over.key() != a_over.key(),
        "different param types are different keys"
    );
    assert_eq!(
        a_over.key(),
        FunctionSignature::new("over", &a_over.params),
        "FunctionSignature::new builds the same key a definition reports"
    );

    let bindings = bind(&fx.functions, &fx.prelude);
    assert!(
        bindings
            .get(&a_over.key())
            .is_some_and(|f| std::ptr::eq(f, a_over)),
        "same signature: the import-tier over(Int) wins over std.p1's"
    );
    assert!(
        bindings
            .get(&b_over.key())
            .is_some_and(|f| std::ptr::eq(f, b_over)),
        "a different signature is bound alongside, not shadowed"
    );
}

#[test]
fn templates_enums_and_traits_are_separate_name_keyed_namespaces() {
    let fx = fixture();
    let p = &fx.prelude;
    let t = bind(&fx.templates, p)
        .get("ImportStd")
        .expect("template bound");
    let e = bind(&fx.enums, p).get("ImportStd").expect("enum bound");
    let r = bind(&fx.traits, p).get("ImportStd").expect("trait bound");
    let a = fx.module("a");
    assert!(std::ptr::eq(t, template_in(&a.templates, "ImportStd")));
    let a_enum = a.enum_defs.iter().find(|x| x.name == "ImportStd").unwrap();
    let a_trait = a.trait_defs.iter().find(|x| x.name == "ImportStd").unwrap();
    assert!(std::ptr::eq(e, a_enum), "a's enum ImportStd wins");
    assert!(std::ptr::eq(r, a_trait), "a's trait ImportStd wins");
}

#[test]
fn non_pub_prelude_aliases_constraint_defs_and_purposes_are_not_bound() {
    let fx = fixture();
    let p = &fx.prelude;
    assert!(
        bind(&fx.aliases, p).get(HIDDEN).is_none(),
        "non-pub prelude alias"
    );
    assert!(
        resolve(&fx.aliases, p, HIDDEN).is_none(),
        "non-pub prelude alias"
    );
    assert!(
        bind(&fx.constraint_defs, p).get(HIDDEN).is_none(),
        "non-pub prelude constraint def"
    );
    assert!(
        resolve(&fx.constraint_defs, p, HIDDEN).is_none(),
        "non-pub prelude constraint def"
    );
    assert!(
        bind(&fx.purposes, p).get(HIDDEN).is_none(),
        "non-pub prelude purpose"
    );
    assert!(
        resolve(&fx.purposes, p, HIDDEN).is_none(),
        "non-pub prelude purpose"
    );
}

#[test]
fn pub_prelude_aliases_constraint_defs_and_purposes_are_bound() {
    let fx = fixture();
    let p = &fx.prelude;
    let a = fx.module("a");
    let alias = bind(&fx.aliases, p)
        .get("Solo")
        .expect("pub prelude alias bound");
    assert!(std::ptr::eq(
        alias,
        a.type_aliases.iter().find(|x| x.name == "Solo").unwrap()
    ));
    let cd = bind(&fx.constraint_defs, p)
        .get("Solo")
        .expect("pub prelude constraint def bound");
    assert!(std::ptr::eq(
        cd,
        a.constraint_defs.iter().find(|x| x.name == "Solo").unwrap()
    ));
    let pp = bind(&fx.purposes, p)
        .get("Solo")
        .expect("pub prelude purpose bound");
    assert!(std::ptr::eq(
        pp,
        a.compiled_purposes
            .iter()
            .find(|x| x.name == "Solo")
            .unwrap()
    ));
}

#[test]
fn non_pub_local_definitions_are_still_bound() {
    let fx = fixture();
    let p = &fx.prelude;
    let alias = bind(&fx.aliases, p)
        .get("LocalSolo")
        .expect("local alias bound");
    assert!(std::ptr::eq(alias, &fx.aliases[3]));
    let cd = bind(&fx.constraint_defs, p)
        .get("LocalSolo")
        .expect("local constraint def bound");
    assert!(std::ptr::eq(cd, &fx.constraint_defs[3]));
    let pp = bind(&fx.purposes, p)
        .get("LocalSolo")
        .expect("local purpose bound");
    assert!(std::ptr::eq(pp, &fx.purposes[3]));
}

#[test]
fn origin_tiers_classify_by_first_path_segment() {
    let std_path = module_path("std.ports.electrical");
    let import_path = module_path("vendor.std");
    assert_eq!(Origin::Local.tier(), Tier::Local);
    assert_eq!(Origin::Module(&std_path).tier(), Tier::Stdlib);
    assert_eq!(
        Origin::Module(&import_path).tier(),
        Tier::Import,
        "only the FIRST segment decides the std tier"
    );
}

#[test]
fn a_compiled_enum_records_whether_it_was_declared_pub() {
    let module = reify_test_support::compile_source_named(
        "pub enum Shown { A }\nenum Kept { B }\n",
        "enum_visibility",
    );
    let visibility = |name: &str| {
        module
            .enum_defs
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("enum {name} compiled"))
            .is_pub
    };
    assert!(visibility("Shown"), "`pub enum Shown` is pub");
    assert!(!visibility("Kept"), "`enum Kept` is not pub");
}

// ─── program-level binding: bind_program / resolve_in_program (task 5522) ──
//
// Over a REAL tempdir program: entry `main` imports `a` then `b`. Every
// expectation is a pointer into the program or the stdlib, so the test pins
// WHICH definition binds, not just that one does.

mod program_binding {
    use std::fs;
    use std::path::Path;

    use reify_compiler::cfg::CfgSet;
    use reify_compiler::module_dag::ModuleResolver;
    use reify_compiler::name_env::{
        Definition, FunctionSignature, bind_program, resolve_in_program,
    };
    use reify_compiler::program::{CompiledProgram, compile_program};
    use reify_compiler::stdlib_loader::load_stdlib;
    use reify_compiler::{CompiledModule, CompiledPurpose, CompiledUnit, TopologyTemplate};
    use reify_core::Severity;
    use reify_ir::{CompiledFunction, EnumDef, Satisfaction};
    use reify_test_support::mocks::MockConstraintChecker;

    /// A std template an import redeclares (import beats std), and one only
    /// the stdlib declares.
    const STD_TEMPLATE: &str = "DisplayStyle";
    const STD_ONLY_TEMPLATE: &str = "STLOutput";
    /// A std enum an import redeclares, and one only the stdlib declares.
    const STD_ENUM: &str = "InterpolationMethod";
    const STD_ONLY_ENUM: &str = "JointPosition";
    /// A std function signature an import redeclares.
    const STD_FN: &str = "symmetric_tolerance";
    const STD_ONLY_UNIT: &str = "cm";

    fn a_source() -> String {
        format!(
            "module a\n\
             pub structure def Shared {{\n    param x : Real = 1\n}}\n\
             pub structure def ImportPeer {{\n    param x : Real = 1\n}}\n\
             pub structure def {STD_TEMPLATE} {{\n    param x : Real = 1\n}}\n\
             structure def HiddenT {{\n    param x : Real = 1\n}}\n\
             pub structure def VisT {{\n    param x : Real = 1\n}}\n\
             pub fn shared_fn(x: Real) -> Real {{ x }}\n\
             pub fn peer_fn(x: Real) -> Real {{ x }}\n\
             pub fn {STD_FN}(nominal: Length, deviation: Length) -> Length {{ nominal }}\n\
             fn hidden_fn(x: Real) -> Real {{ x }}\n\
             pub fn vis_fn(x: Real) -> Real {{ x }}\n\
             pub enum SharedE {{ A }}\n\
             pub enum PeerE {{ A }}\n\
             pub enum {STD_ENUM} {{ Mine }}\n\
             enum HiddenE {{ H }}\n\
             pub enum VisE {{ V }}\n\
             pub unit peeru : Length = 1.0\n\
             unit hiddenu : Length = 1.0\n\
             pub unit visu : Length = 1.0\n"
        )
    }

    const B_SOURCE: &str = "module b\n\
         pub structure def Shared {\n    param y : Real = 2\n}\n\
         pub structure def ImportPeer {\n    param y : Real = 2\n}\n\
         pub fn peer_fn(x: Real) -> Real { x + 1 }\n\
         pub enum PeerE { B }\n\
         pub unit peeru : Length = 2.0\n";

    const MAIN_SOURCE: &str = "module main\nimport a\nimport b\n\
         structure def Shared {\n    param z : Real = 3\n}\n\
         fn shared_fn(x: Real) -> Real { x * 2 }\n\
         enum SharedE { Z }\n";

    fn compile_fixture(dir: &Path) -> CompiledProgram {
        fs::write(dir.join("a.ri"), a_source()).unwrap();
        fs::write(dir.join("b.ri"), B_SOURCE).unwrap();
        let entry_path = dir.join("main.ri");
        fs::write(&entry_path, MAIN_SOURCE).unwrap();
        let program = compile_program(
            &entry_path,
            MAIN_SOURCE,
            &ModuleResolver::new(dir, dir.join("stdlib")),
            &CfgSet::default(),
            &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
        );
        let errors: Vec<_> = program
            .entry()
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "precondition: fixture compiles: {errors:?}");
        let keys: Vec<&str> = program.imports.iter().map(|u| u.import_path.as_str()).collect();
        assert_eq!(keys, ["a", "b"], "precondition: a is imported before b");
        program
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        program: CompiledProgram,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let program = compile_fixture(dir.path());
            Fixture { _dir: dir, program }
        }

        fn a(&self) -> &CompiledModule {
            &self.program.imports[0].module
        }

        fn b(&self) -> &CompiledModule {
            &self.program.imports[1].module
        }
    }

    fn prelude() -> &'static [CompiledModule] {
        load_stdlib()
    }

    fn std_home<D: Definition>(name: &str) -> &'static D {
        prelude()
            .iter()
            .find_map(|m| D::exported_by(m).find(|d| d.name() == name))
            .unwrap_or_else(|| panic!("precondition: some std module exports `{name}`"))
    }

    fn named<'a, D: Definition>(items: impl IntoIterator<Item = &'a D>, name: &str) -> &'a D {
        items
            .into_iter()
            .find(|d| d.name() == name)
            .unwrap_or_else(|| panic!("fixture declares `{name}`"))
    }

    fn assert_binds<'a, D: Definition>(found: Option<&'a D>, expected: &'a D, what: &str) {
        let found = found.unwrap_or_else(|| panic!("{what}: nothing bound"));
        assert!(std::ptr::eq(found, expected), "{what}: bound {found:?}");
    }

    fn by_name<'a, D>(program: &'a CompiledProgram, name: &str) -> Option<&'a D>
    where
        D: Definition,
        D::Key<'a>: std::borrow::Borrow<str>,
    {
        let bound = bind_program::<D>(program, prelude()).get(name);
        let resolved = resolve_in_program::<D, str>(program, prelude(), name);
        assert_eq!(
            bound.map(|d| d as *const D),
            resolved.map(|d| d as *const D),
            "bind_program and resolve_in_program disagree on `{name}`"
        );
        bound
    }

    #[test]
    fn templates_bind_entry_then_imports_in_order_then_std() {
        let fx = Fixture::new();
        let p = &fx.program;
        let entry_own = p.entry_declared_templates();
        let t = |name| by_name::<TopologyTemplate>(p, name);
        assert_binds(t("Shared"), named(entry_own, "Shared"), "entry beats import");
        assert_binds(t("ImportPeer"), named(&fx.a().templates, "ImportPeer"), "a beats b");
        assert_binds(t(STD_TEMPLATE), named(&fx.a().templates, STD_TEMPLATE), "import beats std");
        assert_binds(
            t(STD_ONLY_TEMPLATE),
            std_home::<TopologyTemplate>(STD_ONLY_TEMPLATE),
            "std-only resolves",
        );
        assert!(t("HiddenT").is_none(), "a non-pub import template is invisible");
        assert_binds(t("VisT"), named(&fx.a().templates, "VisT"), "pub sibling visible");
    }

    #[test]
    fn functions_bind_by_signature_entry_then_imports_in_order_then_std() {
        let fx = Fixture::new();
        let p = &fx.program;
        let f = |module: &CompiledModule, name: &str| -> *const CompiledFunction {
            let declared = named(&module.functions, name);
            let key = FunctionSignature::new(name, &declared.params);
            let bound = bind_program::<CompiledFunction>(p, prelude()).get(&key);
            let resolved = resolve_in_program::<CompiledFunction, _>(p, prelude(), &key);
            assert_eq!(
                bound.map(|d| d as *const _),
                resolved.map(|d| d as *const _),
                "bind_program and resolve_in_program disagree on `{name}`"
            );
            bound.map_or(std::ptr::null(), |d| d as *const _)
        };
        let entry = p.entry();
        assert_eq!(f(fx.a(), "shared_fn"), named(&entry.functions, "shared_fn") as *const _);
        assert_eq!(f(fx.b(), "peer_fn"), named(&fx.a().functions, "peer_fn") as *const _);
        let std_fn = prelude()
            .iter()
            .flat_map(|m| m.functions.iter())
            .find(|g| {
                g.key() == named(&fx.a().functions, STD_FN).key()
            });
        assert!(std_fn.is_some(), "precondition: the stdlib declares {STD_FN} with a's signature");
        assert_eq!(f(fx.a(), STD_FN), named(&fx.a().functions, STD_FN) as *const _);
        let std_only = prelude()
            .iter()
            .flat_map(|m| m.functions.iter())
            .find(|g| ![STD_FN, "shared_fn", "peer_fn", "vis_fn", "hidden_fn"].contains(&g.name.as_str()))
            .expect("precondition: the stdlib declares some function");
        let std_only_bound = resolve_in_program::<CompiledFunction, _>(p, prelude(), &std_only.key());
        assert_binds(std_only_bound, std_only, "std-only function resolves");
        assert!(f(fx.a(), "hidden_fn").is_null(), "a non-pub import fn is invisible");
        assert_eq!(f(fx.a(), "vis_fn"), named(&fx.a().functions, "vis_fn") as *const _);
    }

    #[test]
    fn enums_bind_entry_then_imports_in_order_then_std() {
        let fx = Fixture::new();
        let p = &fx.program;
        let e = |name| by_name::<EnumDef>(p, name);
        assert_binds(e("SharedE"), named(&p.entry().enum_defs, "SharedE"), "entry beats import");
        assert_binds(e("PeerE"), named(&fx.a().enum_defs, "PeerE"), "a beats b");
        assert_binds(e(STD_ENUM), named(&fx.a().enum_defs, STD_ENUM), "import beats std");
        assert_binds(e(STD_ONLY_ENUM), std_home::<EnumDef>(STD_ONLY_ENUM), "std-only resolves");
        assert!(e("HiddenE").is_none(), "a non-pub import enum is invisible");
        assert_binds(e("VisE"), named(&fx.a().enum_defs, "VisE"), "pub sibling visible");
    }

    /// An entry re-declaring an import's unit, or an import re-declaring a std
    /// unit, is a compile Error that drops the later unit, so those two
    /// precedence pairs never reach a compiled program.
    #[test]
    fn units_bind_first_import_then_std() {
        let fx = Fixture::new();
        let p = &fx.program;
        let u = |name| by_name::<CompiledUnit>(p, name);
        assert_binds(u("peeru"), named(&fx.a().units, "peeru"), "a beats b");
        assert_binds(u(STD_ONLY_UNIT), std_home::<CompiledUnit>(STD_ONLY_UNIT), "std-only resolves");
        assert!(u("hiddenu").is_none(), "a non-pub import unit is invisible");
        assert_binds(u("visu"), named(&fx.a().units, "visu"), "pub sibling visible");
    }

    /// The entry's own templates are `entry_declared_templates()`: a pub
    /// template the I5 merge copied into `entry.templates` binds the IMPORT
    /// UNIT's template, and no Local-vs-Import shadowing is recorded for it.
    #[test]
    fn the_entry_scope_excludes_templates_merged_from_imports() {
        let fx = Fixture::new();
        let p = &fx.program;
        assert!(
            p.entry().templates.iter().any(|t| t.name == "VisT")
                && !p.entry_declared_templates().iter().any(|t| t.name == "VisT"),
            "precondition: VisT is an I5-merged copy, not an entry declaration"
        );
        let bindings = bind_program::<TopologyTemplate>(p, prelude());
        assert_binds(bindings.get("VisT"), named(&fx.a().templates, "VisT"), "merged name");
        assert!(
            !bindings
                .shadowings()
                .iter()
                .any(|s| s.key == "VisT"),
            "a merged copy is not a second candidate for its own name"
        );
    }

    /// `merge_prelude_purposes` copies std purposes into every module's
    /// `compiled_purposes` with prelude-sentinel spans; those copies are not
    /// part of any module's surface, so with no user purposes nothing collides.
    #[test]
    fn merged_prelude_purposes_are_not_part_of_a_modules_surface() {
        let fx = Fixture::new();
        let p = &fx.program;
        let merged = |m: &CompiledModule| {
            m.compiled_purposes
                .iter()
                .any(|purpose| purpose.declaration_span.is_prelude())
        };
        assert!(
            merged(p.entry()) && merged(fx.a()) && merged(fx.b()),
            "precondition: std purposes are merged into the entry and each import"
        );
        let bindings = bind_program::<CompiledPurpose>(p, prelude());
        assert!(
            bindings.shadowings().is_empty(),
            "no purpose collides: {:?}",
            bindings
                .shadowings()
                .iter()
                .map(|s| (s.key, s.winner.origin, s.shadowed.origin))
                .collect::<Vec<_>>()
        );
    }

    /// `resolve_in_program` agrees with `bind_program(..).get(key)` for every
    /// name bound in the fixture, of every kind it declares.
    #[test]
    fn resolve_in_program_agrees_with_bind_program_for_every_bound_key() {
        fn agree<D: Definition>(program: &CompiledProgram) {
            let bindings = bind_program::<D>(program, prelude());
            for candidate in bindings.winning_candidates() {
                let key = candidate.definition.key();
                let resolved = resolve_in_program::<D, _>(program, prelude(), &key)
                    .unwrap_or_else(|| panic!("{key:?} resolves"));
                let bound = bindings.get(&key).unwrap_or_else(|| panic!("{key:?} is bound"));
                assert!(
                    std::ptr::eq(resolved, bound) && std::ptr::eq(bound, candidate.definition),
                    "{key:?}: resolve_in_program and bind_program disagree"
                );
            }
        }
        let fx = Fixture::new();
        agree::<TopologyTemplate>(&fx.program);
        agree::<CompiledFunction>(&fx.program);
        agree::<EnumDef>(&fx.program);
        agree::<CompiledUnit>(&fx.program);
        agree::<CompiledPurpose>(&fx.program);
    }
}
