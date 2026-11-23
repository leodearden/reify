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
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::name_env::Definition;
use reify_compiler::program_scope::ProgramDefinition;
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, TopologyTemplate,
};
use reify_core::{Diagnostic, DiagnosticCode, Severity, Type};
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
    assert!(same(
        env.resolve_template("Rig"),
        template_in(entry_own, "Rig")
    ));
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
    assert!(
        !merged.is_empty(),
        "precondition: the I5 merge copied a template"
    );
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
    /// Who reports a collision between `a`'s and `b`'s definitions.
    import_pairs: Reporter,
}

/// The one side that reports a collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reporter {
    /// `DefEnv::collision_warnings`.
    DefEnv,
    /// The entry's own compile, whose prelude-only bind sees `a` and `b`.
    EntryCompile,
}

fn rows() -> Vec<KindRow> {
    vec![
        KindRow {
            kind: "template",
            names: ["SharedT", "PeerT", "HiddenT", "VisT"],
            declare: |n| format!("structure def {n} {{\n    param x : Real = 1\n}}\n"),
            std_name: Some("Color"),
            entry_redeclares: true,
            import_pairs: Reporter::DefEnv,
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
            import_pairs: Reporter::DefEnv,
        },
        KindRow {
            kind: "enum",
            names: ["SharedE", "PeerE", "HiddenE", "VisE"],
            declare: |n| format!("enum {n} {{ Foo, Bar }}\n"),
            std_name: Some("SignalKind"),
            entry_redeclares: true,
            import_pairs: Reporter::DefEnv,
        },
        KindRow {
            kind: "unit",
            names: ["sharedu", "peeru", "hiddenu", "visu"],
            declare: |n| format!("unit {n} : Length = 1.0\n"),
            std_name: None,
            entry_redeclares: false,
            import_pairs: Reporter::EntryCompile,
        },
        KindRow {
            kind: "type alias",
            names: ["SharedA", "PeerA", "HiddenA", "VisA"],
            declare: |n| format!("type {n} = Length\n"),
            std_name: Some("ArealCostRate"),
            entry_redeclares: true,
            import_pairs: Reporter::EntryCompile,
        },
        KindRow {
            kind: "trait",
            names: ["SharedTr", "PeerTr", "HiddenTr", "VisTr"],
            declare: |n| format!("trait {n} {{\n    param k : Real\n}}\n"),
            std_name: Some("Rigid"),
            entry_redeclares: true,
            import_pairs: Reporter::DefEnv,
        },
        KindRow {
            kind: "constraint def",
            names: ["SharedC", "PeerC", "HiddenC", "VisC"],
            declare: |n| format!("constraint def {n} {{\n    param t : Length\n    t > 0mm\n}}\n"),
            std_name: Some("Conforms"),
            entry_redeclares: true,
            import_pairs: Reporter::EntryCompile,
        },
        KindRow {
            kind: "purpose",
            names: [
                "shared_purpose",
                "peer_purpose",
                "hidden_purpose",
                "vis_purpose",
            ],
            declare: |n| {
                format!(
                    "purpose {n}(subject : Structure) {{\n    \
                     constraint forall p in subject.params: determined(p)\n}}\n"
                )
            },
            std_name: Some("design_review"),
            entry_redeclares: true,
            import_pairs: Reporter::DefEnv,
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
        assert_eq!(
            errors_of(&program),
            Vec::<&str>::new(),
            "precondition: no Error"
        );
        let keys: Vec<&str> = program
            .imports
            .iter()
            .map(|u| u.import_path.as_str())
            .collect();
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
trait Kind: ProgramDefinition + Sized {
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
        assert_binds(
            &env,
            named::<D>(a, shared),
            own,
            what("the entry beats the first import"),
        );
        assert_qualified(&env, "main", shared, Some(own));
    } else {
        let first: &D = named(a, shared);
        assert_binds(
            &env,
            named::<D>(b, shared),
            first,
            what("the first import binds"),
        );
    }
    let first_peer: &D = named(a, peer);
    assert_binds(
        &env,
        named::<D>(b, peer),
        first_peer,
        what("the first import beats the second"),
    );
    if let Some(std_name) = row.std_name {
        let (_, std_def) = std_home::<D>(std_name);
        let redeclared: &D = named(b, std_name);
        assert_binds(
            &env,
            std_def,
            redeclared,
            what("the second import beats the stdlib"),
        );
    }
    let hidden_def: &D = named(a, hidden);
    assert!(
        D::resolve(&env, hidden_def).is_none(),
        "{}",
        what("a non-pub import definition is invisible")
    );
    let visible_def: &D = named(a, visible);
    assert_binds(
        &env,
        visible_def,
        visible_def,
        what("its pub sibling is visible"),
    );
    let std_def = std_only::<D>(&row);
    assert_binds(&env, std_def, std_def, what("a stdlib-only name resolves"));

    assert_qualified(&env, "a", visible, Some(visible_def));
    assert_qualified::<D>(&env, "a", hidden, None);
    assert_qualified(&env, "b", peer, Some(named::<D>(b, peer)));
    let (std_module, _) = std_home::<D>(std_def.name());
    assert_qualified(
        &env,
        &std_module.path.to_dotted(),
        std_def.name(),
        Some(std_def),
    );
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
    assert!(same(
        env.resolve_function(&peer.name, &param_types(peer)),
        peer
    ));
    assert!(
        env.resolve_function(&peer.name, &[Type::Bool]).is_none(),
        "no `{}` takes one Bool",
        peer.name
    );
}

// ─── D-6 collision report and the §10 Q4 function composition ─────────────

/// Whether `diagnostic` is the D-6 report of `kind` `name` bound by `winner`
/// over `shadowed`: a coded Warning naming both origins, winner first, that
/// says first-wins.
fn reports_collision(
    diagnostic: &Diagnostic,
    kind: &str,
    name: &str,
    winner: &str,
    shadowed: &str,
) -> bool {
    names_collision(diagnostic, kind, name, winner, shadowed)
        && diagnostic.code == Some(DiagnosticCode::DefinitionCollision)
        && diagnostic.message.contains("first-wins")
}

fn collisions_of_kind<'d>(warnings: &'d [Diagnostic], kind: &str) -> Vec<&'d Diagnostic> {
    warnings
        .iter()
        .filter(|d| d.message.starts_with(&format!("{kind} '")))
        .collect()
}

/// `(name, winner, shadowed, reporter)` of every collision a row's fixture
/// makes whose loser is `a`'s or `b`'s. When the entry redeclares `shared`,
/// the entry compile's prelude-only bind still sees `a`'s `shared` shadow
/// `b`'s; the program's binding does not, since the entry shadows both.
fn import_tier_collisions(
    row: &KindRow,
) -> Vec<(&'static str, &'static str, &'static str, Reporter)> {
    let [shared, peer, ..] = row.names;
    let mut expected = vec![(peer, "a", "b", row.import_pairs)];
    if row.entry_redeclares {
        expected.push((shared, "main", "a", Reporter::DefEnv));
        expected.push((shared, "main", "b", Reporter::DefEnv));
    }
    if !row.entry_redeclares || row.import_pairs == Reporter::EntryCompile {
        expected.push((shared, "a", "b", row.import_pairs));
    }
    expected
}

/// Per kind: one Warning per entry-vs-import shadowing, one per
/// import-vs-import shadowing of a kind the entry's compile does not report,
/// and none for `b`'s redeclaration of a stdlib name (an import-vs-std
/// shadowing, which `b`'s own compile reports).
fn assert_kind_collisions<D: Kind>(kind: &str) {
    let row = row(kind);
    let fx = KindsFixture::new();
    let warnings = DefEnv::from_program(&fx.program).collision_warnings();
    let kind_word = named::<D>(fx.a(), row.names[0]).kind_word();
    let expected: Vec<(&str, &str, &str)> = import_tier_collisions(&row)
        .into_iter()
        .filter(|&(.., reporter)| reporter == Reporter::DefEnv)
        .map(|(name, winner, shadowed, _)| (name, winner, shadowed))
        .collect();
    let of_kind = collisions_of_kind(&warnings, kind_word);
    for (name, winner, shadowed) in &expected {
        let reports = of_kind
            .iter()
            .filter(|d| reports_collision(d, kind_word, name, winner, shadowed))
            .count();
        assert_eq!(
            reports, 1,
            "{kind}: one report of '{name}' ({winner} over {shadowed}), got {of_kind:#?}"
        );
    }
    assert_eq!(
        of_kind.len(),
        expected.len(),
        "{kind}: no other collision is reported, got {of_kind:#?}"
    );
}

#[test]
fn template_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<TopologyTemplate>("template");
}

#[test]
fn function_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<CompiledFunction>("function");
}

#[test]
fn enum_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<EnumDef>("enum");
}

#[test]
fn unit_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<CompiledUnit>("unit");
}

#[test]
fn type_alias_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<CompiledTypeAlias>("type alias");
}

#[test]
fn trait_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<CompiledTrait>("trait");
}

#[test]
fn constraint_def_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<CompiledConstraintDef>("constraint def");
}

#[test]
fn purpose_collisions_are_reported_once_per_import_tier_shadowing() {
    assert_kind_collisions::<CompiledPurpose>("purpose");
}

#[test]
fn the_kinds_fixture_reports_exactly_the_import_tier_shadowings() {
    let fx = KindsFixture::new();
    let warnings = DefEnv::from_program(&fx.program).collision_warnings();
    let expected = rows()
        .iter()
        .flat_map(import_tier_collisions)
        .filter(|&(.., reporter)| reporter == Reporter::DefEnv)
        .count();
    assert_eq!(warnings.len(), expected, "got {warnings:#?}");
}

/// Whether `diagnostic` is a Warning reporting `kind` `name` bound by
/// `winner` over `shadowed`, in any collision wording: the constraint-def
/// prelude seed still has its own.
fn names_collision(
    diagnostic: &Diagnostic,
    kind: &str,
    name: &str,
    winner: &str,
    shadowed: &str,
) -> bool {
    let message = &diagnostic.message;
    let position = |origin: &str| message.find(&format!("'{origin}'"));
    diagnostic.severity == Severity::Warning
        && message.starts_with(&format!("{kind} '{name}' "))
        && matches!((position(winner), position(shadowed)), (Some(w), Some(s)) if w < s)
}

/// Every collision whose loser is a user import's definition has exactly one
/// reporter, across the entry's own compile and DefEnv's report, and the
/// reporter each kind's import-vs-import collisions go to is fixed: units,
/// type aliases and constraint defs are the entry compile's prelude seeds',
/// every other kind DefEnv's.
#[test]
fn each_import_tier_collision_has_exactly_one_reporter() {
    let fx = KindsFixture::new();
    let def_env = DefEnv::from_program(&fx.program).collision_warnings();
    let entry_compile: Vec<&Diagnostic> = fx.program.entry_own_diagnostics().collect();
    for row in rows() {
        let kind_word = match row.kind {
            "template" => named::<TopologyTemplate>(fx.a(), row.names[0]).kind_word(),
            _ => row.kind,
        };
        for (name, winner, shadowed, reporter) in import_tier_collisions(&row) {
            let reports = |diagnostics: &[&Diagnostic]| {
                diagnostics
                    .iter()
                    .filter(|d| names_collision(d, kind_word, name, winner, shadowed))
                    .count()
            };
            let counts = (
                reports(&entry_compile),
                reports(&def_env.iter().collect::<Vec<_>>()),
            );
            let expected = match reporter {
                Reporter::EntryCompile => (1, 0),
                Reporter::DefEnv => (0, 1),
            };
            assert_eq!(
                counts, expected,
                "{} '{name}' ({winner} over {shadowed}): (entry compile, DefEnv) report \
                 counts, expected only {reporter:?}",
                row.kind
            );
        }
    }
}

/// A shadowed STD definition is not DefEnv's to report: the entry's own
/// compile reports entry-vs-std as `StdlibNameShadowed`, and the import's own
/// compile reports import-vs-std the same way.
#[test]
fn a_shadowed_stdlib_definition_is_not_in_the_collision_report() {
    const STD_NAME: &str = "Color";
    stdlib_module_declaring(STD_NAME);
    let dir = tempfile::tempdir().unwrap();
    let declare =
        |marker: u32| format!("structure def {STD_NAME} {{\n    param zz : Real = {marker}\n}}\n");
    let program = compile_in(
        dir.path(),
        &[("a", &format!("module a\npub {}", declare(1)))],
        &format!("module main\nimport a\n{}", declare(2)),
    );
    assert_eq!(
        errors_of(&program),
        Vec::<&str>::new(),
        "precondition: no Error"
    );
    let is_shadow = |d: &&Diagnostic| {
        d.code == Some(DiagnosticCode::StdlibNameShadowed) && d.message.contains(STD_NAME)
    };
    assert!(
        program.entry_own_diagnostics().any(|d| is_shadow(&d)),
        "the entry's own compile reports entry-vs-std, got {:#?}",
        program.entry().diagnostics
    );
    assert!(
        program.rehomed_from("a").any(|d| is_shadow(&d)),
        "a's own compile reports import-vs-std"
    );

    let warnings = DefEnv::from_program(&program).collision_warnings();
    assert_eq!(
        warnings.len(),
        1,
        "only main-vs-a is DefEnv's, got {warnings:#?}"
    );
    assert!(reports_collision(
        &warnings[0],
        "structure",
        STD_NAME,
        "main",
        "a"
    ));
}

#[test]
fn a_program_without_import_tier_shadowing_reports_no_collision() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_fixture(dir.path());
    let warnings = DefEnv::from_program(&program).collision_warnings();
    assert!(warnings.is_empty(), "got {warnings:#?}");
}

/// `a` and `b` both declare `twice(x: Real)`; the entry declares `scale(Real)`
/// beside `a`'s `scale(Length)`.
fn compile_function_fixture(dir: &Path) -> CompiledProgram {
    let program = compile_in(
        dir,
        &[
            (
                "a",
                "module a\n\
                 pub fn twice(x: Real) -> Real { x * 2 }\n\
                 pub fn scale(x: Length) -> Length { x }\n",
            ),
            ("b", "module b\npub fn twice(x: Real) -> Real { x + x }\n"),
        ],
        "module main\nimport a\nimport b\nfn scale(x: Real) -> Real { x }\n",
    );
    assert_eq!(
        errors_of(&program),
        Vec::<&str>::new(),
        "precondition: no Error"
    );
    program
}

/// Functions gain the collision warning they never had.
#[test]
fn an_import_vs_import_function_collision_warns_and_the_first_import_binds() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_function_fixture(dir.path());
    let env = DefEnv::from_program(&program);
    let twice_in_a: &CompiledFunction = named(&program.imports[0].module, "twice");
    assert!(same(
        env.resolve_function("twice", &param_types(twice_in_a)),
        twice_in_a
    ));
    let warnings = env.collision_warnings();
    assert_eq!(warnings.len(), 1, "got {warnings:#?}");
    assert!(reports_collision(
        &warnings[0],
        "function",
        "twice",
        "a",
        "b"
    ));
}

/// §10 Q4: distinct signatures under one name coexist as overloads.
#[test]
fn distinct_signatures_coexist_as_overloads_in_precedence_order() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_function_fixture(dir.path());
    let env = DefEnv::from_program(&program);
    let own: &CompiledFunction = named(program.entry(), "scale");
    let imported: &CompiledFunction = named(&program.imports[0].module, "scale");
    let overloads = env.overloads("scale");
    assert_eq!(overloads.len(), 2, "got {overloads:#?}");
    assert!(
        std::ptr::eq(overloads[0], own),
        "the entry's overload first"
    );
    assert!(std::ptr::eq(overloads[1], imported));
    assert!(
        !env.collision_warnings()
            .iter()
            .any(|d| d.message.contains("'scale'")),
        "overloads are not collisions"
    );
}

/// A function's dispatch key: its name and parameter types.
fn signature(function: &CompiledFunction) -> (&str, Vec<&Type>) {
    (
        function.name.as_str(),
        function.params.iter().map(|(_, ty)| ty).collect(),
    )
}

/// §10 Q4 against the `eval_is_idempotent_for_prelude_functions` source
/// (tests/stdlib_prelude_tests.rs): the entry's `symmetric_tolerance` shadows
/// the stdlib's, and `function_table()` dispatches exactly as the engine's
/// table does, the entry's functions followed by every stdlib module's in load
/// order, first match per signature.
#[test]
fn the_function_table_dispatches_as_the_engine_table_does() {
    let source = "module main\n\
         fn symmetric_tolerance(nominal: Length, deviation: Length) -> Length {\n\
         \x20   nominal - deviation\n\
         }\n\
         \n\
         structure S {\n\
         \x20   let v : Length = symmetric_tolerance(5mm, 2mm)\n\
         }\n";
    let dir = tempfile::tempdir().unwrap();
    let program = compile_in(dir.path(), &[], source);
    assert_eq!(
        errors_of(&program),
        Vec::<&str>::new(),
        "precondition: no Error"
    );
    let env = DefEnv::from_program(&program);
    let own: &CompiledFunction = named(env.declared(), "symmetric_tolerance");
    assert!(
        load_stdlib()
            .iter()
            .flat_map(|m| m.functions.iter())
            .any(|f| signature(f) == signature(own)),
        "precondition: the stdlib declares symmetric_tolerance with the entry's signature"
    );
    assert!(same(
        env.resolve_function("symmetric_tolerance", &param_types(own)),
        own
    ));

    let table = env.function_table();
    let reference: Vec<&CompiledFunction> = env
        .declared()
        .functions
        .iter()
        .chain(load_stdlib().iter().flat_map(|m| m.functions.iter()))
        .collect();
    let first_in_table = |function: &CompiledFunction| {
        table
            .iter()
            .find(|f| signature(f) == signature(function))
            .unwrap_or_else(|| panic!("the table dispatches {:?}", signature(function)))
    };
    let first_in_reference = |function: &CompiledFunction| {
        *reference
            .iter()
            .find(|f| signature(f) == signature(function))
            .expect("the function is in the reference order")
    };
    assert_eq!(
        table
            .iter()
            .filter(|f| signature(f) == signature(own))
            .count(),
        1,
        "the table holds one symmetric_tolerance(Length, Length)"
    );
    assert_eq!(first_in_table(own).content_hash, own.content_hash);
    for function in &reference {
        assert_eq!(
            first_in_table(function).content_hash,
            first_in_reference(function).content_hash,
            "dispatch of {:?} differs from the engine's first match",
            signature(function)
        );
    }
    for (i, function) in table.iter().enumerate() {
        assert_eq!(
            function.content_hash,
            first_in_reference(function).content_hash,
            "the table entry {:?} is the engine's first match",
            signature(function)
        );
        assert!(
            table[..i]
                .iter()
                .all(|f| signature(f) != signature(function)),
            "the table holds one entry per signature, {:?} twice",
            signature(function)
        );
    }
}
