//! Integration tests for `Engine::eval` honoring `self.param_overrides`.
//!
//! Task 2017: teach `Engine::eval()` to consult `self.param_overrides` for
//! Param cells (with the same validation `edit_param` performs) so the CLI
//! can drop its shadow `user_overrides` Vec and `reapply_user_overrides()`
//! helper. These tests drive the engine-side refactor; the value-level CLI
//! tests in `crates/reify-cli/src/mcp_context.rs` lock the outward behavior.

use reify_compiler::CompiledModule;
use reify_eval::Engine;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;
use reify_types::{DimensionVector, Value, ValueCellId};

/// Build an Engine with an empty prelude for self-contained param-override tests.
/// Uses `Engine::with_prelude(…, &[])` so the tests do not depend on stdlib state.
fn fresh_engine() -> Engine {
    Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
}

/// Convenience: parse + compile a single-structure source string via the
/// shared test-support helper.  Exposed at the module level so subsequent
/// test steps can reuse it without retyping the import chain.
fn compile_source(source: &str) -> CompiledModule {
    parse_and_compile(source)
}

/// Build a `Value::Scalar` with LENGTH dimension (metres), mirroring the
/// compiler's handling of `mm` literals (`100mm` → `Value::Scalar { si_value: 0.1, dimension: LENGTH }`).
fn length_scalar(si_meters: f64) -> Value {
    Value::Scalar {
        si_value: si_meters,
        dimension: DimensionVector::LENGTH,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// step-1: cold-start honouring a single override
// ──────────────────────────────────────────────────────────────────────────────

/// After an initial `eval()`, writing an override via `set_param_and_invalidate`
/// and calling `eval()` again must surface the overridden value — NOT the
/// compiled-module default.  Currently FAILS because `Engine::eval()` ignores
/// `self.param_overrides` and rebuilds the snapshot from `default_expr`.
#[test]
fn eval_honors_single_param_override_on_cold_start() {
    let mut engine = fresh_engine();
    let compiled = compile_source("structure S { param width: Scalar = 100mm }");

    // Initial eval to register the template + populate snapshot with defaults.
    let first = engine.eval(&compiled);
    let width_id = ValueCellId::new("S", "width");
    assert_eq!(
        first.values.get(&width_id),
        Some(&length_scalar(0.1)),
        "pre-override eval should yield the 100mm default"
    );

    // Establish the override (goes into `Engine::param_overrides`).
    engine.set_param_and_invalidate(&width_id, length_scalar(0.12));

    // Re-evaluate from cold — eval() should now consult param_overrides.
    let second = engine.eval(&compiled);
    assert_eq!(
        second.values.get(&width_id),
        Some(&length_scalar(0.12)),
        "post-override eval must surface the 0.12m override, not the 100mm default"
    );
}
