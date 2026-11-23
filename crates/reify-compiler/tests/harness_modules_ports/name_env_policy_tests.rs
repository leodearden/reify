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
    Bindings, Definition, FunctionSignature, Origin, Shadowing, Surface, Tier, bind, bind_scopes,
    collision_warning, resolve, resolve_in_scopes, surface_of,
};
use reify_compiler::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    EntityKind, TopologyTemplate,
};
use reify_core::{
    ContentHash, DiagnosticCode, ModulePath, Severity, SourceSpan, Type, ValueCellId,
};
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

// ─── scope-level core: surface_of / bind_scopes / resolve_in_scopes ───────
//
// Three scopes: a `local` surface, direct imports `[a, b]` (import tier, `pub`
// surface only), and the ambient prelude `[std.p1, c]` (`c` import tier). The
// expected precedence is local > direct imports in order > import-tier prelude
// modules > std, whatever the slice order.

/// `(name, is_pub)` per scope module. `a`'s non-pub `Secret` must not bind
/// through the direct-import tier; the ambient `c`'s does.
const SCOPE_PLAN: &[(&str, &[(&str, bool)])] = &[
    (
        "a",
        &[
            ("Everywhere", true),
            ("DirectPeer", true),
            ("OverAmbient", true),
            ("Secret", false),
            ("Visible", true),
        ],
    ),
    ("b", &[("Everywhere", true), ("DirectPeer", true)]),
    (
        "c",
        &[
            ("Everywhere", true),
            ("OverAmbient", true),
            ("Secret", false),
            ("PreludeImport", true),
        ],
    ),
    (
        "std.p1",
        &[
            ("Everywhere", true),
            ("OverAmbient", true),
            ("PreludeImport", true),
            ("StdOnly", true),
            ("Secret", true),
        ],
    ),
];
const SCOPE_LOCAL: &[&str] = &["Everywhere", "Mine"];

/// `(name, origin)` of every winner, in precedence order.
const SCOPE_WINNERS: &[(&str, &str)] = &[
    ("Everywhere", "local"),
    ("Mine", "local"),
    ("DirectPeer", "a"),
    ("OverAmbient", "a"),
    ("Visible", "a"),
    ("Secret", "c"),
    ("PreludeImport", "c"),
    ("StdOnly", "std.p1"),
];

/// `(name, winner origin, shadowed origin)` for every losing definition.
const SCOPE_SHADOWINGS: &[(&str, &str, &str)] = &[
    ("DirectPeer", "a", "b"),
    ("Everywhere", "local", "a"),
    ("Everywhere", "local", "b"),
    ("Everywhere", "local", "c"),
    ("Everywhere", "local", "std.p1"),
    ("OverAmbient", "a", "c"),
    ("OverAmbient", "a", "std.p1"),
    ("PreludeImport", "c", "std.p1"),
    ("Secret", "c", "std.p1"),
];

fn scope_module(label: &str, declarations: &[(&str, bool)]) -> CompiledModule {
    let mut builder = CompiledModuleBuilder::new(module_path(label));
    for &(name, is_pub) in declarations {
        builder = builder
            .enum_def(EnumDef {
                is_pub,
                ..enum_def(name)
            })
            .function(CompiledFunction {
                is_pub,
                ..int_fn(name)
            });
    }
    builder.build()
}

struct ScopesFixture {
    modules: Vec<CompiledModule>,
    enums: Vec<EnumDef>,
    functions: Vec<CompiledFunction>,
}

impl ScopesFixture {
    fn new() -> Self {
        ScopesFixture {
            modules: SCOPE_PLAN
                .iter()
                .map(|(label, declarations)| scope_module(label, declarations))
                .collect(),
            enums: SCOPE_LOCAL.iter().map(|n| enum_def(n)).collect(),
            functions: SCOPE_LOCAL.iter().map(|n| int_fn(n)).collect(),
        }
    }

    fn module(&self, label: &str) -> &CompiledModule {
        self.modules
            .iter()
            .find(|m| origin_label(Origin::Module(&m.path)) == label)
            .unwrap_or_else(|| panic!("scope fixture has no module {label}"))
    }

    fn direct_imports(&self) -> [&CompiledModule; 2] {
        [self.module("a"), self.module("b")]
    }

    fn prelude(&self) -> [&CompiledModule; 2] {
        [self.module("std.p1"), self.module("c")]
    }
}

/// A kind the scope fixture declares in every module and locally.
trait ScopeKind: Definition + Sized {
    fn declared_in(module: &CompiledModule) -> &[Self];
    fn local(fx: &ScopesFixture) -> &[Self];
}

impl ScopeKind for EnumDef {
    fn declared_in(module: &CompiledModule) -> &[Self] {
        &module.enum_defs
    }
    fn local(fx: &ScopesFixture) -> &[Self] {
        &fx.enums
    }
}

impl ScopeKind for CompiledFunction {
    fn declared_in(module: &CompiledModule) -> &[Self] {
        &module.functions
    }
    fn local(fx: &ScopesFixture) -> &[Self] {
        &fx.functions
    }
}

fn pointers<'a, D: 'a>(definitions: impl IntoIterator<Item = &'a D>) -> Vec<*const D> {
    definitions.into_iter().map(|d| d as *const D).collect()
}

fn assert_surfaces<D: ScopeKind>(kind: &str) {
    let fx = ScopesFixture::new();
    let a = fx.module("a");
    let pub_only: Vec<&D> = D::declared_in(a).iter().filter(|d| d.is_pub()).collect();
    assert!(
        pub_only.len() < D::declared_in(a).len(),
        "precondition: a declares a non-pub {kind}"
    );
    assert_eq!(
        pointers(surface_of::<D>(a, Surface::DirectImport)),
        pointers(pub_only),
        "{kind}: a direct import offers only its pub definitions"
    );
    assert_eq!(
        pointers(surface_of::<D>(a, Surface::Ambient)),
        pointers(D::exported_by(a)),
        "{kind}: a prelude module offers everything it exports"
    );
}

#[test]
fn a_direct_import_offers_its_pub_surface_and_a_prelude_module_its_exports() {
    assert_surfaces::<EnumDef>("enum");
    assert_surfaces::<CompiledFunction>("function");
}

fn assert_scope_precedence<D: ScopeKind>(kind: &str) {
    let fx = ScopesFixture::new();
    let bindings = bind_scopes(D::local(&fx), fx.direct_imports(), fx.prelude());
    let winners: Vec<(&str, String)> = bindings
        .winning_candidates()
        .iter()
        .map(|c| (c.definition.name(), origin_label(c.origin)))
        .collect();
    let expected: Vec<(&str, String)> = SCOPE_WINNERS
        .iter()
        .map(|(name, origin)| (*name, origin.to_string()))
        .collect();
    assert_eq!(winners, expected, "{kind}: winners and their origins");

    let mut shadowings: Vec<(&str, String, String)> = bindings
        .shadowings()
        .iter()
        .map(|s| {
            (
                s.shadowed.definition.name(),
                origin_label(s.winner.origin),
                origin_label(s.shadowed.origin),
            )
        })
        .collect();
    shadowings.sort();
    let expected: Vec<(&str, String, String)> = SCOPE_SHADOWINGS
        .iter()
        .map(|(n, w, s)| (*n, w.to_string(), s.to_string()))
        .collect();
    assert_eq!(shadowings, expected, "{kind}: shadowings");

    let a_secret = D::declared_in(fx.module("a"))
        .iter()
        .find(|d| d.name() == "Secret")
        .expect("a declares Secret");
    assert!(
        bindings
            .winners()
            .chain(bindings.shadowings().iter().map(|s| s.shadowed.definition))
            .all(|d| !std::ptr::eq(d, a_secret)),
        "{kind}: a's non-pub Secret is no candidate through the direct-import tier"
    );
}

#[test]
fn bind_scopes_ranks_local_then_direct_imports_then_prelude_imports_then_std() {
    assert_scope_precedence::<EnumDef>("enum");
    assert_scope_precedence::<CompiledFunction>("function");
}

fn assert_scope_lookup_agrees<'a, D: ScopeKind>(
    kind: &str,
    fx: &'a ScopesFixture,
    absent: D::Key<'a>,
) {
    let local = D::local(fx);
    let bindings = bind_scopes(local, fx.direct_imports(), fx.prelude());
    for candidate in bindings.winning_candidates() {
        let key = candidate.definition.key();
        let resolved = resolve_in_scopes(local, fx.direct_imports(), fx.prelude(), &key)
            .unwrap_or_else(|| panic!("{kind} {key:?} resolves"));
        assert!(
            bindings
                .get(&key)
                .is_some_and(|b| std::ptr::eq(b, resolved)),
            "{kind} {key:?}: resolve_in_scopes and bind_scopes disagree"
        );
    }
    assert!(
        resolve_in_scopes(local, fx.direct_imports(), fx.prelude(), &absent).is_none(),
        "{kind}: an absent key resolves to nothing"
    );
}

#[test]
fn resolve_in_scopes_agrees_with_bind_scopes_for_every_bound_key() {
    let fx = ScopesFixture::new();
    assert_scope_lookup_agrees::<EnumDef>("enum", &fx, "Absent");
    let absent_fn = int_fn("Absent");
    assert_scope_lookup_agrees::<CompiledFunction>("function", &fx, absent_fn.key());
}

/// `bind` is `bind_scopes` with no direct imports: the same winners, by
/// pointer and in order, and the same shadowings.
fn assert_bind_is_bind_scopes_without_direct_imports<'a, D: Definition>(
    kind: &str,
    local: &'a [D],
    prelude: &'a [CompiledModule],
) {
    let one = bind(local, prelude);
    let scoped = bind_scopes(local, std::iter::empty::<&CompiledModule>(), prelude);
    assert_eq!(
        pointers(one.winners()),
        pointers(scoped.winners()),
        "{kind}: winners"
    );
    let rows = |b: &Bindings<'a, D>| -> Vec<(D::Key<'a>, Origin<'a>, Origin<'a>)> {
        b.shadowings()
            .iter()
            .map(|s| (s.key.clone(), s.winner.origin, s.shadowed.origin))
            .collect()
    };
    assert_eq!(rows(&one), rows(&scoped), "{kind}: shadowings");
}

#[test]
fn bind_is_bind_scopes_without_direct_imports_for_every_kind() {
    let fx = fixture();
    let p = &fx.prelude;
    assert_bind_is_bind_scopes_without_direct_imports("template", &fx.templates, p);
    assert_bind_is_bind_scopes_without_direct_imports("enum", &fx.enums, p);
    assert_bind_is_bind_scopes_without_direct_imports("trait", &fx.traits, p);
    assert_bind_is_bind_scopes_without_direct_imports("function", &fx.functions, p);
    assert_bind_is_bind_scopes_without_direct_imports("type alias", &fx.aliases, p);
    assert_bind_is_bind_scopes_without_direct_imports("constraint def", &fx.constraint_defs, p);
    assert_bind_is_bind_scopes_without_direct_imports("purpose", &fx.purposes, p);
}

// ─── collision_warning: the one wording of a first-wins collision ─────────

/// The shadowing of `name` whose loser comes from `shadowed`.
fn shadowing_of<'b, 'a, D: Definition>(
    bindings: &'b Bindings<'a, D>,
    name: &str,
    shadowed: &str,
) -> &'b Shadowing<'a, D> {
    bindings
        .shadowings()
        .iter()
        .find(|s| {
            s.shadowed.definition.name() == name && origin_label(s.shadowed.origin) == shadowed
        })
        .unwrap_or_else(|| panic!("{name} from {shadowed} is shadowed"))
}

/// A `DefinitionCollision` Warning reading `expected_message`.
fn assert_collision_warning(warning: &reify_core::Diagnostic, expected_message: &str) {
    assert_eq!(warning.severity, Severity::Warning, "{warning:?}");
    assert_eq!(
        warning.code,
        Some(DiagnosticCode::DefinitionCollision),
        "{warning:?}"
    );
    assert_eq!(warning.message, expected_message);
}

#[test]
fn a_collision_warning_names_a_local_winner_by_the_binding_module() {
    let fx = fixture();
    let bindings = bind(&fx.aliases, &fx.prelude);
    let shadowing = shadowing_of(&bindings, "LocalImport", "b");
    assert_eq!(shadowing.winner.origin, Origin::Local, "precondition");
    assert_collision_warning(
        &collision_warning(shadowing, Some(&module_path("user.main"))),
        "type alias 'LocalImport' declared in both 'user.main' and 'b'; \
         first-wins: 'user.main' binds",
    );
}

#[test]
fn a_collision_warning_from_a_bind_without_local_scope_names_modules_by_dotted_path() {
    let fx = fixture();
    let bindings = bind::<CompiledTrait, _>(&[], &fx.prelude);
    let shadowing = shadowing_of(&bindings, "StdOnly", "std.p2");
    assert_collision_warning(
        &collision_warning(shadowing, None),
        "trait 'StdOnly' declared in both 'std.p1' and 'std.p2'; first-wins: 'std.p1' binds",
    );
}

#[test]
fn a_template_collision_warning_names_the_winners_entity_kind() {
    let fx = fixture();
    let mut occurrence = template("LocalImport");
    occurrence.entity_kind = EntityKind::Occurrence;
    let local = [occurrence];
    let bindings = bind(&local, &fx.prelude);
    let shadowing = shadowing_of(&bindings, "LocalImport", "b");
    assert_eq!(
        shadowing.shadowed.definition.entity_kind,
        EntityKind::Structure,
        "precondition: the shadowed template is a structure"
    );
    assert_collision_warning(
        &collision_warning(shadowing, Some(&module_path("main"))),
        "occurrence 'LocalImport' declared in both 'main' and 'b'; first-wins: 'main' binds",
    );
}
