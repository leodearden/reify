// Integration tests for W_UNDERDETERMINED detection (task κ #4019).
//
// These tests exercise `detect_underdetermined` (wired in Engine::eval) through
// the engine-level builder harness — the same approach as scope_coupling.rs.
// Engine-level tests are preferred because the builder controls ValueCellIds
// exactly; `engine.check()` is the literal `reify check` entry point.

use reify_core::{DiagnosticCode, ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{ObjectiveSense, ObjectiveSet};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder, gt, literal, mm,
    value_ref,
};

// ---------------------------------------------------------------------------
// Helper: build a no-solver engine (mirrors `reify check` entry point).
// ---------------------------------------------------------------------------
fn no_solver_engine() -> Engine {
    Engine::new(Box::new(MockConstraintChecker::new()), None)
}

// ---------------------------------------------------------------------------
// Test A — positive: single unconstrained auto param emits W_UNDERDETERMINED
// (step-3 RED — no detection pass exists yet)
// ---------------------------------------------------------------------------

/// A single-template module with one `auto` param "FreeBar.gap" and NO
/// constraint or objective referencing it.  With no solver attached,
/// `engine.eval` must emit exactly one `W_UNDERDETERMINED` diagnostic naming
/// the free param ("FreeBar.gap") and indicating no touching constraints.
///
/// RED until step-4 adds `detect_underdetermined` to Engine::eval.
#[test]
fn eval_emits_underdetermined_for_unconstrained_auto_param() {
    let template = TopologyTemplateBuilder::new("FreeBar")
        .auto_param("FreeBar", "gap", Type::length())
        // No constraints — gap is entirely unconstrained.
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let under_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .collect();

    assert_eq!(
        under_diags.len(),
        1,
        "expected exactly 1 W_UNDERDETERMINED diagnostic for unconstrained auto param, \
         got {}: {:?}",
        under_diags.len(),
        result.diagnostics,
    );

    let msg = &under_diags[0].message;
    assert!(
        msg.contains("W_UNDERDETERMINED"),
        "diagnostic message should contain 'W_UNDERDETERMINED'; got: {msg}"
    );
    assert!(
        msg.contains("FreeBar.gap"),
        "diagnostic message should name the free cell 'FreeBar.gap'; got: {msg}"
    );
    assert!(
        msg.contains("touching constraints: none")
            || msg.contains("none")
                && msg.contains("touching"),
        "diagnostic message should mention empty touching constraints; got: {msg}"
    );
}
