//! `Engine::load(CompiledProgram)` returns the module the engine evaluates: the
//! entry with the program's user templates. The engine resolves exactly that
//! module in the loaded program's `DefEnv` over its own prelude, and any other
//! module in that module's own environment (PRD
//! docs/prds/v0_6/resolution-unification.md §3.2, I1).

use std::fs;
use std::path::Path;
use std::sync::Arc;

use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_core::{Diagnostic, Severity, Type, ValueCellId};
use reify_eval::{DefEnv, Engine, EvalResult, SolverPosture};
use reify_ir::{CompiledFunction, EnumDef, Satisfaction, Value};
use reify_test_support::helpers::make_engine;
use reify_test_support::mocks::MockConstraintChecker;

/// A template only the stdlib declares (std.io).
const STDLIB_ONLY: &str = "STLOutput";

fn compile_in(dir: &Path, modules: &[(&str, &str)], main_src: &str) -> Arc<CompiledProgram> {
    for (stem, src) in modules {
        fs::write(dir.join(format!("{stem}.ri")), src).unwrap();
    }
    let entry_path = dir.join("main.ri");
    fs::write(&entry_path, main_src).unwrap();
    let program = compile_program(
        &entry_path,
        main_src,
        &ModuleResolver::new(dir, dir.join("stdlib")),
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );
    let errors: Vec<&Diagnostic> = program
        .entry()
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "precondition: no Error: {errors:#?}");
    Arc::new(program)
}

/// `a` and `b` both declare `twice(x: Real)`; `a` also declares a pub enum.
fn compile_colliding(dir: &Path) -> Arc<CompiledProgram> {
    compile_in(
        dir,
        &[
            (
                "a",
                "module a\n\
                 pub fn twice(x: Real) -> Real { x * 2 }\n\
                 pub enum Pick { Left, Right }\n",
            ),
            ("b", "module b\npub fn twice(x: Real) -> Real { x + x }\n"),
        ],
        "module main\nimport a\nimport b\n",
    )
}

fn compile_other(dir: &Path) -> Arc<CompiledProgram> {
    compile_in(
        dir,
        &[("c", "module c\npub fn thrice(x: Real) -> Real { x * 3 }\n")],
        "module main\nimport c\n",
    )
}

fn function_in<'m>(
    program: &'m CompiledProgram,
    import: usize,
    name: &str,
) -> &'m CompiledFunction {
    program.imports[import]
        .module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("import {import} declares `{name}`"))
}

fn param_types(function: &CompiledFunction) -> Vec<Type> {
    function.params.iter().map(|(_, ty)| ty.clone()).collect()
}

fn same<D>(found: Option<&D>, expected: &D) -> bool {
    found.is_some_and(|d| std::ptr::eq(d, expected))
}

fn enum_in<'m>(program: &'m CompiledProgram, import: usize, name: &str) -> &'m EnumDef {
    program.imports[import]
        .module
        .enum_defs
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("import {import} declares `{name}`"))
}

/// `main` imports `a`, whose pub surface is `Pulley`, `twice` and `Pick`;
/// `Hidden` is not pub. `Rig` uses `Pulley` and `twice`.
fn compile_rig(dir: &Path) -> Arc<CompiledProgram> {
    compile_in(
        dir,
        &[(
            "a",
            "module a\n\
             pub structure def Pulley {\n    param d : Length = 40mm\n}\n\
             structure def Hidden {\n    param h : Real = 0\n}\n\
             pub fn twice(x: Real) -> Real { x * 2 }\n\
             pub enum Pick { L, R }\n",
        )],
        "module main\nimport a\n\
         structure def Rig {\n    sub p = Pulley()\n    let y = twice(21.0)\n}\n",
    )
}

fn template_names(module: &CompiledModule) -> Vec<&str> {
    module.templates.iter().map(|t| t.name.as_str()).collect()
}

fn value_of<'r>(result: &'r EvalResult, entity: &str, member: &str) -> Option<&'r Value> {
    result.values.get(&ValueCellId::new(entity, member))
}

fn is_close(value: Option<&Value>, expected: f64) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|v| (v - expected).abs() < 1e-12)
}

#[test]
fn load_returns_the_entry_with_the_programs_user_templates() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_rig(dir.path());
    let mut engine = make_engine();

    let module = engine.load(Arc::clone(&program));

    assert_eq!(template_names(&module), ["Rig", "Pulley"]);
    let without_templates = |module: &CompiledModule| {
        format!(
            "{:?}",
            CompiledModule {
                templates: Vec::new(),
                ..module.clone()
            }
        )
    };
    assert_eq!(
        without_templates(&module),
        without_templates(program.entry()),
        "every field but the templates is the entry's"
    );
}

#[test]
fn the_loaded_module_evaluates_through_the_loaded_program() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_rig(dir.path());
    let mut engine = make_engine();
    let module = engine.load(Arc::clone(&program));

    let result = engine.eval(&module);

    assert!(
        is_close(value_of(&result, "Rig", "y"), 42.0),
        "`twice` dispatches to a's pub fn, got {:?}",
        value_of(&result, "Rig", "y")
    );
    assert!(
        is_close(value_of(&result, "Rig.p", "d"), 0.04),
        "Rig.p is a's Pulley, got {:?}",
        value_of(&result, "Rig.p", "d")
    );
}

#[test]
fn the_loaded_module_resolves_in_the_loaded_programs_environment() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_rig(dir.path());
    let mut engine = make_engine();
    let module = engine.load(Arc::clone(&program));

    let scope = engine.env_of(&module);
    let env = scope.env();

    assert!(same(env.resolve_enum("Pick"), enum_in(&program, 0, "Pick")));
    assert!(std::ptr::eq(env.declared(), &*module));
}

/// A copy of the loaded module is not the module the engine loaded, so it
/// resolves in its own environment: it imports nothing.
#[test]
fn a_copy_of_the_loaded_module_resolves_in_its_own_environment() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_rig(dir.path());
    let mut engine = make_engine();
    let module = engine.load(Arc::clone(&program));
    let copy = CompiledModule::clone(&module);

    assert!(engine.env_of(&copy).env().resolve_enum("Pick").is_none());
    let result = engine.eval(&copy);
    assert!(
        matches!(value_of(&result, "Rig", "y"), None | Some(Value::Undef)),
        "the copy's environment has no `twice`, got {:?}",
        value_of(&result, "Rig", "y")
    );
}

/// Before any load, a module resolves in its own environment.
#[test]
fn an_unloaded_engine_resolves_a_module_in_its_own_environment() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_colliding(dir.path());
    let twice_types = param_types(function_in(&program, 0, "twice"));
    let engine = make_engine();

    let scope = engine.env_of(program.entry());
    let env = scope.env();

    assert!(env.resolve_function("twice", &twice_types).is_none());
    assert!(env.resolve_enum("Pick").is_none());
    assert!(std::ptr::eq(env.declared(), program.entry()));
}

#[test]
fn the_loaded_environment_resolves_through_the_loaded_program() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_colliding(dir.path());
    let mut engine = make_engine();
    let module = engine.load(Arc::clone(&program));
    let scope = engine.env_of(&module);
    let env = scope.env();

    let twice = function_in(&program, 0, "twice");
    assert!(same(
        env.resolve_function("twice", &param_types(twice)),
        twice
    ));
    assert!(same(env.resolve_enum("Pick"), enum_in(&program, 0, "Pick")));
    let stdlib_template = load_stdlib()
        .iter()
        .flat_map(|m| m.templates.iter())
        .find(|t| t.name == STDLIB_ONLY)
        .unwrap_or_else(|| panic!("precondition: the stdlib declares `{STDLIB_ONLY}`"));
    assert!(same(env.resolve_template(STDLIB_ONLY), stdlib_template));
    assert!(std::ptr::eq(env.declared(), &*module));
    assert_eq!(env.declared().path, program.entry().path);
}

/// The environment's ambient tier is the engine's own prelude, not a hard-wired
/// stdlib.
#[test]
fn an_engine_with_an_empty_prelude_resolves_no_stdlib_name() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_colliding(dir.path());
    assert!(
        DefEnv::from_program(&program)
            .resolve_template(STDLIB_ONLY)
            .is_some(),
        "precondition: over the stdlib, `{STDLIB_ONLY}` resolves"
    );
    let mut engine = Engine::with_prelude(
        SolverPosture::SolverFree,
        Box::new(MockConstraintChecker::new()),
        None,
        &[],
    );
    let module = engine.load(Arc::clone(&program));
    let scope = engine.env_of(&module);
    let env = scope.env();
    assert!(env.resolve_template(STDLIB_ONLY).is_none());
    let twice = function_in(&program, 0, "twice");
    assert!(same(
        env.resolve_function("twice", &param_types(twice)),
        twice
    ));
}

/// After a second load only the second program's module resolves through a
/// loaded program; the first's falls back to its own environment.
#[test]
fn a_second_load_replaces_the_first() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first = compile_colliding(first_dir.path());
    let second = compile_other(second_dir.path());
    let twice_types = param_types(function_in(&first, 0, "twice"));
    let mut engine = make_engine();
    let first_module = engine.load(Arc::clone(&first));
    let second_module = engine.load(Arc::clone(&second));

    let second_scope = engine.env_of(&second_module);
    let env = second_scope.env();
    assert!(
        env.resolve_function("twice", &twice_types).is_none(),
        "only the first program's import defined `twice`"
    );
    let thrice = function_in(&second, 0, "thrice");
    assert!(same(
        env.resolve_function("thrice", &param_types(thrice)),
        thrice
    ));
    assert!(std::ptr::eq(env.declared(), &*second_module));

    let first_scope = engine.env_of(&first_module);
    let stale = first_scope.env();
    assert!(
        stale.resolve_function("twice", &twice_types).is_none(),
        "the first module is no longer the loaded one"
    );
    assert!(std::ptr::eq(stale.declared(), &*first_module));
}

/// `parts`' pub trait `Supplied` refines the stdlib's `Buy`, and its pub `Bolt`
/// conforms to `Supplied`. `Kit` uses `Bolt` and queries its descendants by
/// the stdlib trait.
fn compile_kit(dir: &Path) -> Arc<CompiledProgram> {
    compile_in(
        dir,
        &[(
            "parts",
            "module parts\n\
             pub trait Supplied : Buy {}\n\
             pub structure def Bolt : Supplied {\n\
             \x20   param supplier : String = \"Fastenal\"\n\
             \x20   param part_number : String = \"BOLT-M6-20\"\n\
             \x20   param unit_cost : Money = 0.50USD\n\
             \x20   param lead_time : Time = 24h\n\
             }\n",
        )],
        "module main\nimport parts\n\
         structure def Kit {\n\
         \x20   sub bolt = Bolt()\n\
         \x20   let bought = filter(self.descendants, Buy)\n\
         }\n",
    )
}

/// The BOM rollup classifies a sub through the loaded program's trait
/// lattice: `Bolt : Supplied` is a `Buy` only through `parts`' `Supplied`.
#[test]
fn an_imported_trait_refinement_makes_a_bom_line() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_kit(dir.path());
    let mut engine = make_engine();
    let module = engine.load(Arc::clone(&program));
    let result = engine.eval(&module);

    let report = engine.build_bom_report(&module, &result.values);

    let subs: Vec<&str> = report.lines.iter().map(|line| line.sub.as_str()).collect();
    assert_eq!(subs, ["bolt"], "Bolt : Supplied : Buy is a Buy line item");
}

/// A structural query's trait filter walks the same lattice.
#[test]
fn an_imported_trait_refinement_passes_a_structural_query_filter() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_kit(dir.path());
    let mut engine = make_engine();
    let module = engine.load(Arc::clone(&program));

    let result = engine.eval(&module);

    assert_eq!(
        value_of(&result, "Kit", "bought"),
        Some(&Value::List(vec![Value::String("Kit.bolt".to_string())])),
        "filter(self.descendants, Buy) keeps the Bolt"
    );
}
