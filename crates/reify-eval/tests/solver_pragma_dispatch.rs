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
use reify_core::{ModulePath, Severity, Type};
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

/// Build a two-template CompiledModule, each with one auto-param. Used to pin
/// the "warning emitted at most once per eval call" invariant: even when the
/// solver runs once per template, the "not registered" warning must surface
/// exactly once, because `resolve_solver_for_module` is called before the
/// template loop.
fn make_module_with_two_auto_param_templates() -> CompiledModule {
    let template_s = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();
    let template_t = TopologyTemplateBuilder::new("T")
        .auto_param("T", "width", Type::length())
        .constraint("T", 0, None, gt(value_ref("T", "width"), literal(mm(1.0))))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_s)
        .template(template_t)
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
    use reify_core::VersionId;

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

/// When `module.solver_pragma.name` does NOT match any registered solver,
/// the engine falls back to the default `with_solver` solver and emits a
/// single "named back-end '<name>' is not registered; falling back" warning
/// per `eval()` call — even when the module contains multiple auto-param
/// templates that would otherwise iterate the solver in lock-step.
#[test]
fn eval_falls_back_to_default_solver_with_warning_when_named_solver_not_registered() {
    let mut module = make_module_with_two_auto_param_templates();
    module.solver_pragma = Some(SolverPragma {
        name: "libslvs".to_string(),
        options: BTreeMap::new(),
    });

    let default_spy = SpyConstraintSolver::new_solved(HashMap::new());
    let default_captured = default_spy.captured_problem();

    // NB: no `register_solver("libslvs", …)` call — the named back-end is
    // intentionally absent so the resolver must fall back to the default.
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(default_spy));

    let result = engine.eval(&module);

    assert!(
        default_captured.lock().unwrap().is_some(),
        "expected default solver to be invoked when named back-end 'libslvs' is not registered"
    );

    let fallback_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| {
            d.message.contains("libslvs")
                && (d.message.contains("not registered") || d.message.contains("falling back"))
        })
        .collect();

    assert_eq!(
        fallback_warnings.len(),
        1,
        "expected exactly one 'named back-end not registered / falling back' warning per eval call \
         (regardless of template count); got {} warnings: {:?}",
        fallback_warnings.len(),
        fallback_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Same fallback policy as
/// `eval_falls_back_to_default_solver_with_warning_when_named_solver_not_registered`,
/// but exercising `eval_cached` (the LSP / on-keystroke path) instead of
/// `eval`. Pins the at-most-once-warning invariant for the cached-eval path:
/// because the entire point of routing both invocation sites through the
/// shared `resolve_solver_for_module` helper is to keep behaviour identical,
/// the eval_cached side must surface exactly one fallback warning per call —
/// never zero (silent fallback hiding a misconfiguration), never N (one per
/// auto-param template). Without this test, a refactor that moves the
/// `resolve_solver_for_module` call inside the template loop in
/// `eval_cached` could regress silently.
#[test]
fn eval_cached_falls_back_to_default_solver_with_warning_when_named_solver_not_registered() {
    use reify_core::VersionId;

    let mut module = make_module_with_two_auto_param_templates();
    module.solver_pragma = Some(SolverPragma {
        name: "libslvs".to_string(),
        options: BTreeMap::new(),
    });

    let default_spy = SpyConstraintSolver::new_solved(HashMap::new());

    // NB: no `register_solver("libslvs", …)` call — the named back-end is
    // intentionally absent so the resolver must fall back to the default.
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(default_spy));

    // Prime the cache via a regular eval so eval_cached has state to consult.
    // The eval call itself emits one fallback warning, but those diagnostics
    // are returned in `engine.eval(&module)`'s result and do NOT carry over
    // to the eval_cached return value (each call returns its own
    // diagnostics vec).
    let _ = engine.eval(&module);

    let result = engine.eval_cached(&module, VersionId(0));

    let fallback_warnings: Vec<&reify_core::Diagnostic> = result
        .eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| {
            d.message.contains("libslvs")
                && (d.message.contains("not registered") || d.message.contains("falling back"))
        })
        .collect();

    assert_eq!(
        fallback_warnings.len(),
        1,
        "expected exactly one 'named back-end not registered / falling back' warning per \
         eval_cached call (regardless of template count); got {} warnings: {:?}",
        fallback_warnings.len(),
        fallback_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
