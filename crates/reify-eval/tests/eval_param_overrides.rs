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
use reify_types::{DimensionVector, Severity, Value, ValueCellId};

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

// ──────────────────────────────────────────────────────────────────────────────
// step-3: orphaned-override purge across topology edits
// ──────────────────────────────────────────────────────────────────────────────

/// If a Param cell is removed from the module and later re-added with the
/// same `ValueCellId`, the stale override must NOT zombie-resurrect — the
/// intermediate module that lacks the cell should have purged the entry
/// from `self.param_overrides`.  Mirrors the purge `edit_source` already
/// performs after a structural edit.  Currently FAILS because step-2
/// carries the override forward through the intermediate eval.
#[test]
fn eval_purges_override_for_cell_absent_from_new_graph() {
    let mut engine = fresh_engine();

    let module_a = compile_source(
        "structure S { param width: Scalar = 100mm\n param height: Scalar = 200mm }",
    );
    let width_id = ValueCellId::new("S", "width");

    // Eval A, then set an override on `width`.
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&width_id, length_scalar(0.12));

    // Eval a variant B that REMOVES `width` entirely. After this eval, the
    // override entry for `width` must have been purged — the cell no longer
    // exists in the graph, so a dormant entry would zombie-resurrect on a
    // future edit that re-adds `width`.
    let module_b = compile_source("structure S { param height: Scalar = 200mm }");
    let result_b = engine.eval(&module_b);
    assert!(
        result_b.values.get(&width_id).is_none(),
        "width cell is absent from module B so no value should be present, got {:?}",
        result_b.values.get(&width_id)
    );
    assert!(
        result_b.diagnostics.is_empty(),
        "silent purge must not emit spurious warnings, got {:?}",
        result_b.diagnostics
    );

    // Eval module C (identical topology to A: same name, same Scalar type,
    // same 100mm default). The re-added `width` cell must resolve to the
    // MODULE DEFAULT (0.1m), NOT the zombie 0.12m override.
    let module_c = compile_source(
        "structure S { param width: Scalar = 100mm\n param height: Scalar = 200mm }",
    );
    let result_c = engine.eval(&module_c);
    assert_eq!(
        result_c.values.get(&width_id),
        Some(&length_scalar(0.1)),
        "re-added cell must resolve to module default, not zombie override"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// step-5: type-kind mismatch skips the override with a Warning diagnostic
// ──────────────────────────────────────────────────────────────────────────────

/// When the user source is edited so that a Param's type-kind no longer
/// matches the override value (here: Scalar[LENGTH] override against an Int
/// cell), eval() must:
/// - fall back to the module default,
/// - emit a Warning diagnostic naming the cell + the mismatch,
/// - RETAIN the override in `param_overrides` so that reverting the edit
///   resurrects the override.
///
/// Currently FAILS because step-2's override path has no validation — the
/// Scalar value would be wrongly inserted into an Int-typed cell.
#[test]
fn eval_skips_type_kind_mismatched_override_and_emits_warning_diagnostic() {
    let mut engine = fresh_engine();
    let width_id = ValueCellId::new("S", "width");

    // Module A: width is Scalar[LENGTH]. Set a matching override.
    let module_a = compile_source("structure S { param width: Scalar = 100mm }");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&width_id, length_scalar(0.12));

    // Module B: width is now an Int. The Scalar override is type-kind
    // incompatible.
    let module_b = compile_source("structure S { param width: Int = 80 }");
    let result_b = engine.eval(&module_b);

    // (a) The default wins, not the (now-mismatched) override.
    assert_eq!(
        result_b.values.get(&width_id),
        Some(&Value::Int(80)),
        "type-kind mismatched override must be skipped in favour of Int default"
    );

    // (b) Exactly one Warning diagnostic calls out the mismatched cell.
    let warnings: Vec<&reify_types::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.width"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.width, got: {:?}",
        result_b.diagnostics
    );
    let wmsg = warnings[0].message.as_str();
    assert!(
        wmsg.contains("override")
            || wmsg.contains("mismatch")
            || wmsg.contains("type-kind"),
        "warning should mention override/mismatch/type-kind, got: {wmsg:?}"
    );

    // (c) The override is RETAINED (behavioural check: re-compile module A
    //     and eval; the Scalar override must reappear because the mismatch
    //     eval did not remove it from param_overrides).
    let module_a_again = compile_source("structure S { param width: Scalar = 100mm }");
    let result_a2 = engine.eval(&module_a_again);
    assert_eq!(
        result_a2.values.get(&width_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient type-kind mismatch eval"
    );
}
