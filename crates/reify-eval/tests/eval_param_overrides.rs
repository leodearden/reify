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

// ──────────────────────────────────────────────────────────────────────────────
// step-7: dimension mismatch skips the override with a Warning diagnostic
// ──────────────────────────────────────────────────────────────────────────────

/// A Scalar-typed cell whose override carries a different SI dimension (here:
/// a LENGTH override against a MASS cell) is still type-kind compatible —
/// both sides are `Value::Scalar`/`Type::Scalar` — so this path is distinct
/// from the type-kind mismatch of step-5. eval() must detect the dimension
/// drift, skip the override, emit a Warning mentioning "dimension", retain
/// the override, and fall back to the module default.
///
/// Currently FAILS because step-6 only validates type-kind; the Scalar[LENGTH]
/// override would be accepted into a Scalar[MASS] cell, silently corrupting
/// the snapshot.
#[test]
fn eval_skips_dimension_mismatched_override_and_emits_warning_diagnostic() {
    let mut engine = fresh_engine();
    let width_id = ValueCellId::new("S", "width");

    // Module A: width is Scalar[LENGTH]. Set a LENGTH-dimensioned override.
    let module_a = compile_source("structure S { param width: Scalar = 100mm }");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&width_id, length_scalar(0.12));

    // Module B: width is now a Mass (still Scalar kind, so the type-kind
    // guard passes — this is the dimension-mismatch path).
    let module_b = compile_source("structure S { param width: Mass = 5kg }");
    let result_b = engine.eval(&module_b);

    // (a) The Mass default wins, not the LENGTH override.
    let expected_mass_default = Value::Scalar {
        si_value: 5.0,
        dimension: DimensionVector::MASS,
    };
    assert_eq!(
        result_b.values.get(&width_id),
        Some(&expected_mass_default),
        "dimension-mismatched override must be skipped in favour of the Mass default"
    );

    // (b) Exactly one Warning mentions the cell + the word "dimension".
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
    assert!(
        warnings[0].message.contains("dimension"),
        "dimension-mismatch warning should mention 'dimension', got: {:?}",
        warnings[0].message
    );

    // (c) The override is RETAINED — recompile module A, eval, the LENGTH
    //     override reappears on a matching cell.
    let module_a_again = compile_source("structure S { param width: Scalar = 100mm }");
    let result_a2 = engine.eval(&module_a_again);
    assert_eq!(
        result_a2.values.get(&width_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient dimension-mismatch eval"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// step-9: partial mismatch — compatible overrides survive, mismatched warns
// ──────────────────────────────────────────────────────────────────────────────

/// When two params are overridden and a subsequent module change invalidates
/// one override but leaves the other compatible, eval() must:
/// - honor the still-compatible override (no cross-contamination),
/// - fall back to the default for the mismatched cell,
/// - emit exactly one warning (about the mismatched cell only).
///
/// This locks the per-override independence contract that the deleted CLI-side
/// `reapply_user_overrides_partial_mismatch_preserves_surviving_overrides_and_warns_for_mismatched`
/// used to assert.  Acts as a regression lock: should PASS against the
/// step-6/step-8 implementation because the override path is per-cell.
#[test]
fn eval_partial_mismatch_preserves_compatible_overrides_and_warns_only_for_mismatched() {
    let mut engine = fresh_engine();
    let width_id = ValueCellId::new("S", "width");
    let thickness_id = ValueCellId::new("S", "thickness");

    // Module A: both params are Scalar[LENGTH]. Override both.
    let module_a = compile_source(
        "structure S { param width: Scalar = 100mm\n param thickness: Scalar = 5mm }",
    );
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&width_id, length_scalar(0.12));
    engine.set_param_and_invalidate(&thickness_id, length_scalar(0.004));

    // Module B: width is now an Int (type-kind mismatch for its override);
    //           thickness stays Scalar[LENGTH] (override remains compatible).
    let module_b = compile_source(
        "structure S { param width: Int = 80\n param thickness: Scalar = 5mm }",
    );
    let result_b = engine.eval(&module_b);

    // (a) thickness override survives unchanged.
    assert_eq!(
        result_b.values.get(&thickness_id),
        Some(&length_scalar(0.004)),
        "compatible thickness override must survive a peer-cell mismatch"
    );

    // (b) width falls back to the Int default, override skipped.
    assert_eq!(
        result_b.values.get(&width_id),
        Some(&Value::Int(80)),
        "type-kind mismatched width override must fall back to the Int default"
    );

    // (c) Exactly ONE warning diagnostic: about S.width, not S.thickness.
    let warnings: Vec<&reify_types::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning for the partial mismatch, got: {:?}",
        result_b.diagnostics
    );
    assert!(
        warnings[0].message.contains("S.width"),
        "warning should be about S.width, got: {:?}",
        warnings[0].message
    );
    assert!(
        !warnings[0].message.contains("S.thickness"),
        "warning must not mention the compatible S.thickness cell, got: {:?}",
        warnings[0].message
    );
}
