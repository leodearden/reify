//! Tests for `#solver(<name>)` runtime dispatch (Task 2300).
//!
//! When a `CompiledModule` carries `solver_pragma = Some(SolverPragma { name, .. })`,
//! the engine routes solver invocations to the named solver registered via
//! `Engine::register_solver`. If the named solver is not registered, the
//! engine falls back to the default `self.solver` (set via `with_solver`)
//! and emits a single "named solver '<name>' not registered, falling back to
//! default" warning per resolution call.
//!
//! Two invocation sites in `engine_eval.rs` (eval @1199, eval_cached @1832)
//! must both consult the registry via the shared `resolve_solver_for_module`
//! helper so the resolution policy stays in one auditable location.

use reify_compiler::{CompiledModule, SolverPragma};
use reify_eval::Engine;
use reify_test_support::mocks::{MockConstraintChecker, SpyConstraintSolver};
use reify_test_support::{
    CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, mm, value_ref,
};
use reify_types::{ModulePath, Type};
use std::collections::{BTreeMap, HashMap};

/// Build a one-template CompiledModule with a single auto-param so the engine
/// will invoke the constraint solver on `eval()`. The module's `solver_pragma`
/// is overwritten by the caller to test dispatch routing.
fn make_module_with_auto_param() -> CompiledModule {
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build()
}

/// When `module.solver_pragma.name == "libslvs"` and a solver is registered as
/// `"libslvs"`, the engine routes to the named solver — not the default
/// `with_solver` solver.
#[test]
fn eval_uses_named_solver_from_registry_when_solver_pragma_matches() {
    let mut module = make_module_with_auto_param();
    module.solver_pragma = Some(SolverPragma {
        name: "libslvs".to_string(),
        options: BTreeMap::new(),
    });

    let named_spy = SpyConstraintSolver::new_solved(HashMap::new());
    let named_captured = named_spy.captured_problem();

    let default_spy = SpyConstraintSolver::new_solved(HashMap::new());
    let default_captured = default_spy.captured_problem();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(default_spy));
    engine.register_solver("libslvs", Box::new(named_spy));

    engine.eval(&module);

    assert!(
        named_captured.lock().unwrap().is_some(),
        "expected named solver 'libslvs' to be invoked when solver_pragma matches"
    );
    assert!(
        default_captured.lock().unwrap().is_none(),
        "default solver must NOT be invoked when solver_pragma routes to a registered named solver"
    );
}

/// Same dispatch policy as `eval_uses_named_solver_from_registry_when_solver_pragma_matches`,
/// but exercising `eval_cached` (the LSP / on-keystroke path) instead of `eval`.
/// Both invocation sites must route through the same lookup helper so the
/// resolution policy is byte-identical between the two paths.
#[test]
fn eval_cached_uses_named_solver_from_registry_when_solver_pragma_matches() {
    use reify_types::VersionId;

    let mut module = make_module_with_auto_param();
    module.solver_pragma = Some(SolverPragma {
        name: "libslvs".to_string(),
        options: BTreeMap::new(),
    });

    let named_spy = SpyConstraintSolver::new_solved(HashMap::new());
    let named_captured = named_spy.captured_problem();

    let default_spy = SpyConstraintSolver::new_solved(HashMap::new());
    let default_captured = default_spy.captured_problem();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(default_spy));
    engine.register_solver("libslvs", Box::new(named_spy));

    // Prime the cache via a regular eval so eval_cached has state to consult.
    engine.eval(&module);
    // Reset the captures so we measure only the eval_cached invocation below.
    *named_captured.lock().unwrap() = None;
    *default_captured.lock().unwrap() = None;

    let _ = engine.eval_cached(&module, VersionId(0));

    assert!(
        named_captured.lock().unwrap().is_some(),
        "expected named solver 'libslvs' to be invoked by eval_cached when solver_pragma matches"
    );
    assert!(
        default_captured.lock().unwrap().is_none(),
        "default solver must NOT be invoked by eval_cached when solver_pragma routes to a registered named solver"
    );
}
