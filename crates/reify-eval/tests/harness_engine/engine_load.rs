//! `Engine::load(CompiledProgram)`: the engine's resolution environment is the
//! loaded program's `DefEnv` over the engine's own prelude (PRD
//! docs/prds/v0_6/resolution-unification.md §3.2).

use std::fs;
use std::path::Path;
use std::sync::Arc;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_core::{Diagnostic, Severity, Type};
use reify_eval::{DefEnv, Engine, SolverPosture};
use reify_ir::{CompiledFunction, Satisfaction};
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

#[test]
fn an_engine_has_no_definition_environment_before_a_load() {
    assert!(make_engine().def_env().is_none());
}

#[test]
fn the_loaded_environment_resolves_through_the_loaded_program() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_colliding(dir.path());
    let mut engine = make_engine();
    engine.load(Arc::clone(&program));
    let env = engine
        .def_env()
        .expect("a loaded engine has an environment");

    let twice = function_in(&program, 0, "twice");
    assert!(same(
        env.resolve_function("twice", &param_types(twice)),
        twice
    ));
    let pick = program.imports[0]
        .module
        .enum_defs
        .iter()
        .find(|e| e.name == "Pick")
        .expect("a declares Pick");
    assert!(same(env.resolve_enum("Pick"), pick));
    let stdlib_template = load_stdlib()
        .iter()
        .flat_map(|m| m.templates.iter())
        .find(|t| t.name == STDLIB_ONLY)
        .unwrap_or_else(|| panic!("precondition: the stdlib declares `{STDLIB_ONLY}`"));
    assert!(same(env.resolve_template(STDLIB_ONLY), stdlib_template));
    assert!(std::ptr::eq(env.declared(), program.entry()));
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
    engine.load(Arc::clone(&program));
    let env = engine
        .def_env()
        .expect("a loaded engine has an environment");
    assert!(env.resolve_template(STDLIB_ONLY).is_none());
    let twice = function_in(&program, 0, "twice");
    assert!(same(
        env.resolve_function("twice", &param_types(twice)),
        twice
    ));
}

#[test]
fn a_second_load_replaces_the_first() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first = compile_colliding(first_dir.path());
    let second = compile_other(second_dir.path());
    let twice_types = param_types(function_in(&first, 0, "twice"));
    let mut engine = make_engine();
    engine.load(Arc::clone(&first));
    engine.load(Arc::clone(&second));
    let env = engine
        .def_env()
        .expect("a loaded engine has an environment");
    assert!(
        env.resolve_function("twice", &twice_types).is_none(),
        "only the first program's import defined `twice`"
    );
    let thrice = function_in(&second, 0, "thrice");
    assert!(same(
        env.resolve_function("thrice", &param_types(thrice)),
        thrice
    ));
    assert!(std::ptr::eq(env.declared(), second.entry()));
}
