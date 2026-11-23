//! `reify_compiler::program_scope`: a `CompiledProgram`'s definitions bound by
//! name_env's rule over the program's three scopes (resolution-unification I6,
//! I7, D-5).
//!
//! Over a REAL tempdir program: entry `main` imports `a` then `b`. Every
//! expectation is a pointer into the program or the stdlib, so the test pins
//! WHICH definition binds, not just that one does.

use std::fs;
use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::name_env::{Definition, FunctionSignature};
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::program_scope::{
    ProgramDefinition, bind_program, module_surface, resolve_in_program,
};
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
    assert!(
        errors.is_empty(),
        "precondition: fixture compiles: {errors:?}"
    );
    let keys: Vec<&str> = program
        .imports
        .iter()
        .map(|u| u.import_path.as_str())
        .collect();
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
    D: ProgramDefinition,
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
    assert_binds(
        t("Shared"),
        named(entry_own, "Shared"),
        "entry beats import",
    );
    assert_binds(
        t("ImportPeer"),
        named(&fx.a().templates, "ImportPeer"),
        "a beats b",
    );
    assert_binds(
        t(STD_TEMPLATE),
        named(&fx.a().templates, STD_TEMPLATE),
        "import beats std",
    );
    assert_binds(
        t(STD_ONLY_TEMPLATE),
        std_home::<TopologyTemplate>(STD_ONLY_TEMPLATE),
        "std-only resolves",
    );
    assert!(
        t("HiddenT").is_none(),
        "a non-pub import template is invisible"
    );
    assert_binds(
        t("VisT"),
        named(&fx.a().templates, "VisT"),
        "pub sibling visible",
    );
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
    assert_eq!(
        f(fx.a(), "shared_fn"),
        named(&entry.functions, "shared_fn") as *const _
    );
    assert_eq!(
        f(fx.b(), "peer_fn"),
        named(&fx.a().functions, "peer_fn") as *const _
    );
    let std_fn = prelude()
        .iter()
        .flat_map(|m| m.functions.iter())
        .find(|g| g.key() == named(&fx.a().functions, STD_FN).key());
    assert!(
        std_fn.is_some(),
        "precondition: the stdlib declares {STD_FN} with a's signature"
    );
    assert_eq!(
        f(fx.a(), STD_FN),
        named(&fx.a().functions, STD_FN) as *const _
    );
    let std_only = prelude()
        .iter()
        .flat_map(|m| m.functions.iter())
        .find(|g| {
            ![STD_FN, "shared_fn", "peer_fn", "vis_fn", "hidden_fn"].contains(&g.name.as_str())
        })
        .expect("precondition: the stdlib declares some function");
    let std_only_bound = resolve_in_program::<CompiledFunction, _>(p, prelude(), &std_only.key());
    assert_binds(std_only_bound, std_only, "std-only function resolves");
    assert!(
        f(fx.a(), "hidden_fn").is_null(),
        "a non-pub import fn is invisible"
    );
    assert_eq!(
        f(fx.a(), "vis_fn"),
        named(&fx.a().functions, "vis_fn") as *const _
    );
}

#[test]
fn enums_bind_entry_then_imports_in_order_then_std() {
    let fx = Fixture::new();
    let p = &fx.program;
    let e = |name| by_name::<EnumDef>(p, name);
    assert_binds(
        e("SharedE"),
        named(&p.entry().enum_defs, "SharedE"),
        "entry beats import",
    );
    assert_binds(e("PeerE"), named(&fx.a().enum_defs, "PeerE"), "a beats b");
    assert_binds(
        e(STD_ENUM),
        named(&fx.a().enum_defs, STD_ENUM),
        "import beats std",
    );
    assert_binds(
        e(STD_ONLY_ENUM),
        std_home::<EnumDef>(STD_ONLY_ENUM),
        "std-only resolves",
    );
    assert!(e("HiddenE").is_none(), "a non-pub import enum is invisible");
    assert_binds(
        e("VisE"),
        named(&fx.a().enum_defs, "VisE"),
        "pub sibling visible",
    );
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
    assert_binds(
        u(STD_ONLY_UNIT),
        std_home::<CompiledUnit>(STD_ONLY_UNIT),
        "std-only resolves",
    );
    assert!(u("hiddenu").is_none(), "a non-pub import unit is invisible");
    assert_binds(
        u("visu"),
        named(&fx.a().units, "visu"),
        "pub sibling visible",
    );
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
            && !p
                .entry_declared_templates()
                .iter()
                .any(|t| t.name == "VisT"),
        "precondition: VisT is an I5-merged copy, not an entry declaration"
    );
    let bindings = bind_program::<TopologyTemplate>(p, prelude());
    assert_binds(
        bindings.get("VisT"),
        named(&fx.a().templates, "VisT"),
        "merged name",
    );
    assert!(
        !bindings.shadowings().iter().any(|s| s.key == "VisT"),
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
    fn agree<D: ProgramDefinition>(program: &CompiledProgram) {
        let bindings = bind_program::<D>(program, prelude());
        for candidate in bindings.winning_candidates() {
            let key = candidate.definition.key();
            let resolved = resolve_in_program::<D, _>(program, prelude(), &key)
                .unwrap_or_else(|| panic!("{key:?} resolves"));
            let bound = bindings
                .get(&key)
                .unwrap_or_else(|| panic!("{key:?} is bound"));
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

fn holds(surface: &[&TopologyTemplate], expected: &TopologyTemplate) -> bool {
    surface.iter().any(|t| std::ptr::eq(*t, expected))
}

fn holds_name(surface: &[&TopologyTemplate], name: &str) -> bool {
    surface.iter().any(|t| t.name == name)
}

/// A qualified lookup sees one module's surface alone (I7), keyed by dotted
/// module path: a direct import's `pub` templates, the entry's own
/// declarations (never an I5-merged copy), or a prelude module's exports.
#[test]
fn module_surface_is_one_modules_surface_keyed_by_dotted_path() {
    let fx = Fixture::new();
    let p = &fx.program;
    let surface = |key: &str| -> Vec<&TopologyTemplate> {
        module_surface::<TopologyTemplate>(p, prelude(), key)
            .unwrap_or_else(|| panic!("`{key}` names a module of the program"))
            .collect()
    };

    let a = surface("a");
    assert!(
        holds(&a, named(&fx.a().templates, "VisT")),
        "an import exposes its pub VisT"
    );
    assert!(
        !holds_name(&a, "HiddenT"),
        "an import hides its non-pub HiddenT"
    );

    let main = surface("main");
    assert!(
        holds(&main, named(p.entry_declared_templates(), "Shared")),
        "the entry exposes its own Shared"
    );
    assert!(
        !holds_name(&main, "VisT"),
        "the entry's surface excludes the I5-merged VisT"
    );

    let std_key = prelude()
        .iter()
        .find(|m| TopologyTemplate::exported_by(m).any(|t| t.name == STD_ONLY_TEMPLATE))
        .unwrap_or_else(|| panic!("precondition: some std module exports {STD_ONLY_TEMPLATE}"))
        .path
        .to_dotted();
    assert!(
        holds(
            &surface(&std_key),
            std_home::<TopologyTemplate>(STD_ONLY_TEMPLATE)
        ),
        "the std module `{std_key}` exposes {STD_ONLY_TEMPLATE}"
    );

    assert!(
        module_surface::<TopologyTemplate>(p, prelude(), "no.such.module").is_none(),
        "an unknown key names no module"
    );
}
