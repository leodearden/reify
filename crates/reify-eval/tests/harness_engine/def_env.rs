//! `DefEnv`, the authoritative definition environment of a `CompiledProgram`
//! (PRD docs/prds/v0_6/resolution-unification.md §3.2, I6/I7, D-5).
//!
//! Per kind, a bare name binds the entry's own declaration, then each direct
//! import's `pub` surface in declaration order, then the stdlib (I6); a
//! qualified lookup is keyed by `(module_key, name)` (I7). The entry's own
//! templates are `entry_declared_templates()`, so a name the I5 merge copied
//! into the entry binds the import unit's template. The rule is
//! `reify_compiler::name_env`'s; these pins hold DefEnv to it over real
//! programs built by `reify_compiler::program::compile_program`.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::name_env::Definition;
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, TopologyTemplate,
};
use reify_core::{Severity, Type};
use reify_eval::DefEnv;
use reify_ir::{CompiledFunction, EnumDef, Satisfaction};
use reify_test_support::mocks::MockConstraintChecker;

/// A stdlib-only template name (std.io), resolved through the stdlib scope.
const STDLIB_ONLY: &str = "STLOutput";
/// A stdlib template name the entry also declares, to test shadowing.
const SHADOWED: &str = "DisplayStyle";

fn stdlib_module_declaring(name: &str) -> &'static CompiledModule {
    load_stdlib()
        .iter()
        .find(|m| m.templates.iter().any(|t| t.name == name))
        .unwrap_or_else(|| panic!("precondition: some stdlib module declares `{name}`"))
}

fn template_in<'m>(templates: &'m [TopologyTemplate], name: &str) -> &'m TopologyTemplate {
    templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("`{name}` is declared"))
}

fn compile_in(dir: &Path, modules: &[(&str, &str)], main_src: &str) -> CompiledProgram {
    for (stem, src) in modules {
        fs::write(dir.join(format!("{stem}.ri")), src).unwrap();
    }
    let entry_path = dir.join("main.ri");
    fs::write(&entry_path, main_src).unwrap();
    compile_program(
        &entry_path,
        main_src,
        &ModuleResolver::new(dir, dir.join("stdlib")),
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    )
}

fn errors_of(program: &CompiledProgram) -> Vec<&str> {
    program
        .entry()
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

fn compile_fixture(dir: &Path) -> CompiledProgram {
    let main_src = format!(
        "module main\nimport parts\n\
         structure def Rig {{\n    sub p = Pulley()\n}}\n\
         structure def {SHADOWED} {{\n    param marker : Real = 7\n}}\n"
    );
    compile_in(
        dir,
        &[(
            "parts",
            "module parts\n\
             pub structure def Pulley {\n    param r : Real = 1\n}\n\
             structure def Hidden {\n    param h : Real = 0\n}\n",
        )],
        &main_src,
    )
}

fn same<D>(found: Option<&D>, expected: &D) -> bool {
    found.is_some_and(|t| std::ptr::eq(t, expected))
}

#[test]
fn def_env_resolves_with_entry_then_imports_then_stdlib_precedence() {
    let stdlib_only_home = stdlib_module_declaring(STDLIB_ONLY);
    let shadowed_home = stdlib_module_declaring(SHADOWED);
    let dir = tempfile::tempdir().unwrap();
    let program = compile_fixture(dir.path());
    let env = DefEnv::from_program(&program);
    let entry_own = program.entry_declared_templates();
    let parts_unit = &program.imports[0];
    assert_eq!(parts_unit.import_path, "parts");

    // (a) an entry-declared template binds the entry's own; an import's pub
    // template binds the IMPORT UNIT's, not the copy the I5 merge made.
    assert!(same(env.resolve_template("Rig"), template_in(entry_own, "Rig")));
    assert!(
        program.entry().templates.iter().any(|t| t.name == "Pulley"),
        "precondition: the I5 merge copied Pulley into the entry"
    );
    assert!(
        same(
            env.resolve_template("Pulley"),
            template_in(&parts_unit.module.templates, "Pulley")
        ),
        "`Pulley` must resolve to the import unit's template"
    );

    // (b) a stdlib-only name resolves to the stdlib module's template.
    assert!(same(
        env.resolve_template(STDLIB_ONLY),
        template_in(&stdlib_only_home.templates, STDLIB_ONLY)
    ));

    // (c) the entry's declaration shadows the stdlib one.
    let entry_shadow = template_in(entry_own, SHADOWED);
    assert!(!std::ptr::eq(
        entry_shadow,
        template_in(&shadowed_home.templates, SHADOWED)
    ));
    assert!(same(env.resolve_template(SHADOWED), entry_shadow));

    // (d) unknown and non-pub names do not resolve.
    assert!(env.resolve_template("NoSuchThing").is_none());
    assert!(
        env.resolve_template("Hidden").is_none(),
        "a non-pub import template is neither merged nor visible"
    );

    // (e) qualified lookup is keyed by (module_key, name), pub surface only;
    // the entry's scope is its own declarations.
    assert!(same(
        env.resolve_template_in("parts", "Pulley"),
        template_in(&parts_unit.module.templates, "Pulley")
    ));
    assert!(env.resolve_template_in("parts", "Hidden").is_none());
    assert!(same(
        env.resolve_template_in("main", "Rig"),
        template_in(entry_own, "Rig")
    ));
    assert!(
        env.resolve_template_in("main", "Pulley").is_none(),
        "a merged copy is not part of the entry's own scope"
    );
    let stdlib_key = stdlib_only_home.path.to_dotted();
    assert!(same(
        env.resolve_template_in(&stdlib_key, STDLIB_ONLY),
        template_in(&stdlib_only_home.templates, STDLIB_ONLY)
    ));
    assert!(env.resolve_template_in("nope", "Rig").is_none());

    // (f) the declared module is the entry itself.
    assert!(std::ptr::eq(env.declared(), program.entry()));
}

/// (g) Over every entry-declared and stdlib name, DefEnv agrees with the
/// compile-side rule: `name_env::resolve` over the entry's template list and
/// the stdlib, as eval's `find_template_in_scope` calls it. A name the I5 merge
/// copied binds the import unit's template, which the copy duplicates.
#[test]
fn def_env_agrees_with_the_compile_side_rule_and_binds_merged_names_at_home() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_fixture(dir.path());
    let env = DefEnv::from_program(&program);

    let reference = |name: &str| -> Option<&TopologyTemplate> {
        reify_compiler::name_env::resolve(&program.entry().templates, load_stdlib(), name)
    };

    let declared = program.entry_declared_templates();
    let merged = &program.entry().templates[declared.len()..];
    assert!(!merged.is_empty(), "precondition: the I5 merge copied a template");
    let merged_names: HashSet<&str> = merged.iter().map(|t| t.name.as_str()).collect();

    let names = declared
        .iter()
        .chain(load_stdlib().iter().flat_map(|m| m.templates.iter()))
        .map(|t| t.name.as_str())
        .filter(|name| !merged_names.contains(name));
    let mut checked = 0;
    for name in names {
        let expected = reference(name).expect("every enumerated name resolves");
        assert!(
            same(env.resolve_template(name), expected),
            "compile-side parity broken for `{name}`"
        );
        checked += 1;
    }
    assert!(checked > declared.len(), "stdlib names were enumerated");

    for copy in merged {
        let home = program
            .imports
            .iter()
            .find_map(|unit| unit.module.templates.iter().find(|t| t.name == copy.name))
            .unwrap_or_else(|| panic!("`{}` has a home import", copy.name));
        assert!(same(env.resolve_template(&copy.name), home));
        assert_eq!(home.name, copy.name);
        assert_eq!(home.content_hash, copy.content_hash);
    }
}

// ─── every definition kind: entry > import a > import b > stdlib ───────────
//
// Entry `main` imports `a` then `b`. Per kind, `a` declares four names (pub
// `shared`, pub `peer`, non-pub `hidden`, pub `visible`), `b` redeclares
// `shared` and `peer` and one stdlib name, and the entry redeclares `shared`.

/// One definition kind of the fixture.
struct KindRow {
    kind: &'static str,
    /// `[shared, peer, hidden, visible]`.
    names: [&'static str; 4],
    /// A declaration of `name` of this kind, without a `pub` marker.
    declare: fn(&str) -> String,
    /// A stdlib definition `b` redeclares. `None` for units: an import
    /// redeclaring a stdlib unit is a compile Error that drops it.
    std_name: Option<&'static str>,
    /// Whether the entry redeclares `shared`. Not for units: that is a compile
    /// Error, pinned by its own test below.
    entry_redeclares: bool,
}

fn rows() -> Vec<KindRow> {
    vec![
        KindRow {
            kind: "template",
            names: ["SharedT", "PeerT", "HiddenT", "VisT"],
            declare: |n| format!("structure def {n} {{\n    param x : Real = 1\n}}\n"),
            std_name: Some("Color"),
            entry_redeclares: true,
        },
        KindRow {
            kind: "function",
            names: ["shared_fn", "peer_fn", "hidden_fn", "vis_fn"],
            declare: |n| {
                format!(
                    "fn {n}(joint: Real, from: Real, to: Real, max_accel: Real) -> Real {{ joint }}\n"
                )
            },
            std_name: Some("ramp_profile"),
            entry_redeclares: true,
        },
        KindRow {
            kind: "enum",
            names: ["SharedE", "PeerE", "HiddenE", "VisE"],
            declare: |n| format!("enum {n} {{ Foo, Bar }}\n"),
            std_name: Some("SignalKind"),
            entry_redeclares: true,
        },
        KindRow {
            kind: "unit",
            names: ["sharedu", "peeru", "hiddenu", "visu"],
            declare: |n| format!("unit {n} : Length = 1.0\n"),
            std_name: None,
            entry_redeclares: false,
        },
        KindRow {
            kind: "type alias",
            names: ["SharedA", "PeerA", "HiddenA", "VisA"],
            declare: |n| format!("type {n} = Length\n"),
            std_name: Some("ArealCostRate"),
            entry_redeclares: true,
        },
        KindRow {
            kind: "trait",
            names: ["SharedTr", "PeerTr", "HiddenTr", "VisTr"],
            declare: |n| format!("trait {n} {{\n    param k : Real\n}}\n"),
            std_name: Some("Rigid"),
            entry_redeclares: true,
        },
        KindRow {
            kind: "constraint def",
            names: ["SharedC", "PeerC", "HiddenC", "VisC"],
            declare: |n| format!("constraint def {n} {{\n    param t : Length\n    t > 0mm\n}}\n"),
            std_name: Some("Conforms"),
            entry_redeclares: true,
        },
        KindRow {
            kind: "purpose",
            names: ["shared_purpose", "peer_purpose", "hidden_purpose", "vis_purpose"],
            declare: |n| {
                format!(
                    "purpose {n}(subject : Structure) {{\n    \
                     constraint forall p in subject.params: determined(p)\n}}\n"
                )
            },
            std_name: Some("design_review"),
            entry_redeclares: true,
        },
    ]
}

fn row(kind: &str) -> KindRow {
    rows()
        .into_iter()
        .find(|row| row.kind == kind)
        .unwrap_or_else(|| panic!("no row for {kind}"))
}

fn kinds_sources() -> (String, String, String) {
    let mut a = String::from("module a\n");
    let mut b = String::from("module b\n");
    let mut main = String::from("module main\nimport a\nimport b\n");
    for row in rows() {
        let [shared, peer, hidden, visible] = row.names;
        let pub_decl = |name: &str| format!("pub {}", (row.declare)(name));
        a += &pub_decl(shared);
        a += &pub_decl(peer);
        a += &(row.declare)(hidden);
        a += &pub_decl(visible);
        b += &pub_decl(shared);
        b += &pub_decl(peer);
        if let Some(std_name) = row.std_name {
            b += &pub_decl(std_name);
        }
        if row.entry_redeclares {
            main += &(row.declare)(shared);
        }
    }
    (a, b, main)
}

struct KindsFixture {
    _dir: tempfile::TempDir,
    program: CompiledProgram,
}

impl KindsFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, main) = kinds_sources();
        let program = compile_in(dir.path(), &[("a", &a), ("b", &b)], &main);
        assert_eq!(errors_of(&program), Vec::<&str>::new(), "precondition: no Error");
        let keys: Vec<&str> = program.imports.iter().map(|u| u.import_path.as_str()).collect();
        assert_eq!(keys, ["a", "b"], "precondition: a is imported before b");
        KindsFixture { _dir: dir, program }
    }

    fn a(&self) -> &CompiledModule {
        &self.program.imports[0].module
    }

    fn b(&self) -> &CompiledModule {
        &self.program.imports[1].module
    }
}

/// A definition kind, with its DefEnv resolver.
trait Kind: Definition + Sized {
    /// Every definition of this kind `module` holds, `pub` or not.
    fn all_in(module: &CompiledModule) -> &[Self];

    /// What `env` binds `like`'s key to, through this kind's own resolver.
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self>;
}

impl Kind for TopologyTemplate {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.templates
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_template(&like.name)
    }
}

fn param_types(function: &CompiledFunction) -> Vec<Type> {
    function.params.iter().map(|(_, ty)| ty.clone()).collect()
}

impl Kind for CompiledFunction {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.functions
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_function(&like.name, &param_types(like))
    }
}

impl Kind for EnumDef {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.enum_defs
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_enum(&like.name)
    }
}

impl Kind for CompiledUnit {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.units
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_unit(&like.name)
    }
}

impl Kind for CompiledTypeAlias {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.type_aliases
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_alias(&like.name)
    }
}

impl Kind for CompiledTrait {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.trait_defs
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_trait(&like.name)
    }
}

impl Kind for CompiledConstraintDef {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.constraint_defs
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_constraint_def(&like.name)
    }
}

impl Kind for CompiledPurpose {
    fn all_in(module: &CompiledModule) -> &[Self] {
        &module.compiled_purposes
    }
    fn resolve<'p>(env: &DefEnv<'p>, like: &Self) -> Option<&'p Self> {
        env.resolve_purpose(&like.name)
    }
}

fn named<'m, D: Kind>(module: &'m CompiledModule, name: &str) -> &'m D {
    D::all_in(module)
        .iter()
        .find(|d| d.name() == name)
        .unwrap_or_else(|| panic!("module {} declares `{name}`", module.path))
}

/// A stdlib definition of this kind no fixture module declares, found by
/// lookup rather than assumed.
fn std_only<D: Kind>(row: &KindRow) -> &'static D {
    load_stdlib()
        .iter()
        .flat_map(|m| D::exported_by(m))
        .find(|d| !row.names.contains(&d.name()) && Some(d.name()) != row.std_name)
        .unwrap_or_else(|| panic!("precondition: the stdlib exports some {}", row.kind))
}

fn std_home<D: Kind>(name: &str) -> (&'static CompiledModule, &'static D) {
    load_stdlib()
        .iter()
        .find_map(|m| D::exported_by(m).find(|d| d.name() == name).map(|d| (m, d)))
        .unwrap_or_else(|| panic!("precondition: some stdlib module exports `{name}`"))
}

fn assert_binds<D: Kind>(env: &DefEnv<'_>, like: &D, expected: &D, what: String) {
    let found = D::resolve(env, like).unwrap_or_else(|| panic!("{what}: nothing bound"));
    assert!(std::ptr::eq(found, expected), "{what}: bound {found:?}");
}

fn assert_qualified<D: Kind>(env: &DefEnv<'_>, module_key: &str, name: &str, expected: Option<&D>) {
    let found = env.resolve_in::<D>(module_key, name);
    assert_eq!(
        found.map(|d| d as *const D),
        expected.map(|d| d as *const D),
        "resolve_in({module_key:?}, {name:?}) bound {found:?}"
    );
}

/// entry > first import > second import > stdlib for one kind; a non-pub
/// import definition is invisible; a stdlib-only name resolves; qualified
/// lookup sees one module's surface alone.
fn assert_kind_precedence<D: Kind>(kind: &str) {
    let row = row(kind);
    let fx = KindsFixture::new();
    let program = &fx.program;
    let env = DefEnv::from_program(program);
    let (a, b) = (fx.a(), fx.b());
    let [shared, peer, hidden, visible] = row.names;
    let what = |claim: &str| format!("{kind}: {claim}");

    if row.entry_redeclares {
        let own: &D = named(program.entry(), shared);
        assert_binds(&env, named::<D>(a, shared), own, what("the entry beats the first import"));
        assert_qualified(&env, "main", shared, Some(own));
    } else {
        let first: &D = named(a, shared);
        assert_binds(&env, named::<D>(b, shared), first, what("the first import binds"));
    }
    let first_peer: &D = named(a, peer);
    assert_binds(&env, named::<D>(b, peer), first_peer, what("the first import beats the second"));
    if let Some(std_name) = row.std_name {
        let (_, std_def) = std_home::<D>(std_name);
        let redeclared: &D = named(b, std_name);
        assert_binds(&env, std_def, redeclared, what("the second import beats the stdlib"));
    }
    let hidden_def: &D = named(a, hidden);
    assert!(
        D::resolve(&env, hidden_def).is_none(),
        "{}",
        what("a non-pub import definition is invisible")
    );
    let visible_def: &D = named(a, visible);
    assert_binds(&env, visible_def, visible_def, what("its pub sibling is visible"));
    let std_def = std_only::<D>(&row);
    assert_binds(&env, std_def, std_def, what("a stdlib-only name resolves"));

    assert_qualified(&env, "a", visible, Some(visible_def));
    assert_qualified::<D>(&env, "a", hidden, None);
    assert_qualified(&env, "b", peer, Some(named::<D>(b, peer)));
    let (std_module, _) = std_home::<D>(std_def.name());
    assert_qualified(&env, &std_module.path.to_dotted(), std_def.name(), Some(std_def));
    assert_qualified::<D>(&env, "nope", visible, None);
}

#[test]
fn templates_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<TopologyTemplate>("template");
}

#[test]
fn functions_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<CompiledFunction>("function");
}

#[test]
fn enums_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<EnumDef>("enum");
}

#[test]
fn units_bind_imports_in_order_then_stdlib() {
    assert_kind_precedence::<CompiledUnit>("unit");
}

#[test]
fn type_aliases_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<CompiledTypeAlias>("type alias");
}

#[test]
fn traits_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<CompiledTrait>("trait");
}

#[test]
fn constraint_defs_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<CompiledConstraintDef>("constraint def");
}

#[test]
fn purposes_bind_entry_then_imports_in_order_then_stdlib() {
    assert_kind_precedence::<CompiledPurpose>("purpose");
}

/// An entry re-declaring an imported unit is a compile Error that drops the
/// entry's unit, so the import's unit binds.
#[test]
fn an_entry_unit_redeclaring_an_imported_unit_is_an_error_and_the_import_binds() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_in(
        dir.path(),
        &[("a", "module a\npub unit sharedu : Length = 1.0\n")],
        "module main\nimport a\nunit sharedu : Length = 2.0\n",
    );
    assert!(
        errors_of(&program)
            .iter()
            .any(|m| m.contains("duplicate unit declaration 'sharedu'")),
        "the duplicate-unit Error fires, got {:?}",
        errors_of(&program)
    );
    assert!(
        !program.entry().units.iter().any(|u| u.name == "sharedu"),
        "the entry's unit is dropped"
    );
    let env = DefEnv::from_program(&program);
    let imported: &CompiledUnit = named(&program.imports[0].module, "sharedu");
    assert!(same(env.resolve_unit("sharedu"), imported));
}

#[test]
fn function_resolution_matches_parameter_types_exactly() {
    let row = row("function");
    let fx = KindsFixture::new();
    let env = DefEnv::from_program(&fx.program);
    let peer: &CompiledFunction = named(fx.a(), row.names[1]);
    assert!(same(env.resolve_function(&peer.name, &param_types(peer)), peer));
    assert!(
        env.resolve_function(&peer.name, &[Type::Bool]).is_none(),
        "no `{}` takes one Bool",
        peer.name
    );
}
