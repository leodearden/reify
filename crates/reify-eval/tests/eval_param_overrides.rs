//! Integration tests for `Engine::eval` honoring `self.param_overrides`.
//!
//! Task 2017: teach `Engine::eval()` to consult `self.param_overrides` for
//! Param cells (with the same validation `edit_param` performs) so the CLI
//! can drop its shadow `user_overrides` Vec and `reapply_user_overrides()`
//! helper. These tests drive the engine-side refactor; the value-level CLI
//! tests in `crates/reify-cli/src/mcp_context.rs` lock the outward behavior.

use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::journal::EventKind;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

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

/// Build a single-structure source with a guarded group `where active { param x : T = D }`
/// and compile it. The shape the task-2154 tests need varies in `active` (Bool default),
/// `x_type` (the guarded-member type), and `x_default` (its default expression); everything
/// else is invariant. Centralising the format string here defeats the drift risk that
/// would otherwise compound as more guarded-group tests are added.
fn guarded_module(active: bool, x_type: &str, x_default: &str) -> CompiledModule {
    compile_source(&format!(
        "structure S {{ param active : Bool = {active}\n where active {{ param x : {x_type} = {x_default} }} }}"
    ))
}

/// Sibling of `guarded_module` for shape-2 tests that exercise the `else { ... }` branch.
/// `active = false` is hardcoded — every test that exercises the else-branch needs the
/// guard inactive so the `else_members` loop runs. When a test genuinely needs `active = true`
/// it should use `guarded_module` (members-branch) or inline its source.
fn guarded_module_with_else(y_type: &str, y_default: &str) -> CompiledModule {
    compile_source(&format!(
        "structure S {{ param active : Bool = false\n where active {{ param x : Scalar = 5mm }} else {{ param y : {y_type} = {y_default} }} }}"
    ))
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
    let warnings: Vec<&reify_core::Diagnostic> = result_b
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
        wmsg.contains("override") || wmsg.contains("mismatch") || wmsg.contains("type-kind"),
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
    let warnings: Vec<&reify_core::Diagnostic> = result_b
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
    let module_b =
        compile_source("structure S { param width: Int = 80\n param thickness: Scalar = 5mm }");
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
    let warnings: Vec<&reify_core::Diagnostic> = result_b
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

// ──────────────────────────────────────────────────────────────────────────────
// step-11: clear_param_overrides() empties the map so eval reverts to defaults
// ──────────────────────────────────────────────────────────────────────────────

/// After `engine.clear_param_overrides()` the next `eval()` must read back
/// the module defaults rather than any previously-set override.  This is
/// the engine-level primitive the CLI's `load_file`/`open_file` will call
/// instead of the current `CliState::clear_overrides` shadow-copy reset.
///
/// Currently FAILS TO COMPILE because `Engine::clear_param_overrides()`
/// does not yet exist — step-12 adds it.
#[test]
fn clear_param_overrides_empties_map_and_subsequent_eval_uses_defaults() {
    let mut engine = fresh_engine();
    let width_id = ValueCellId::new("S", "width");

    let module_a = compile_source("structure S { param width: Scalar = 100mm }");
    let _ = engine.eval(&module_a);

    // Establish the override, confirm it takes effect.
    engine.set_param_and_invalidate(&width_id, length_scalar(0.12));
    let with_override = engine.eval(&module_a);
    assert_eq!(
        with_override.values.get(&width_id),
        Some(&length_scalar(0.12)),
        "sanity: override should flow through before clear"
    );

    // Clear the override map, re-eval: module default wins again.
    engine.clear_param_overrides();
    let after_clear = engine.eval(&module_a);
    assert_eq!(
        after_clear.values.get(&width_id),
        Some(&length_scalar(0.1)),
        "clear_param_overrides must wipe the override so eval falls back to the 100mm default"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// step-13: no-override baseline — behavior unchanged for untouched engines
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock for the "behavior is unchanged for the common case where
/// param_overrides was never populated" invariant.  Critical because
/// hundreds of existing tests across the crate rely on eval() reading
/// module defaults and not emitting spurious diagnostics when no override
/// has been set — this must not flip after steps 2/4/6/8/12.
#[test]
fn eval_on_fresh_engine_with_no_overrides_uses_defaults_and_emits_no_diagnostics() {
    let mut engine = fresh_engine();
    let width_id = ValueCellId::new("S", "width");
    let height_id = ValueCellId::new("S", "height");

    let module = compile_source(
        "structure S { param width: Scalar = 100mm\n param height: Scalar = 200mm }",
    );
    let result = engine.eval(&module);

    // (a) Both params read back their module defaults.
    assert_eq!(
        result.values.get(&width_id),
        Some(&length_scalar(0.1)),
        "width must read its 100mm default when no override was set"
    );
    assert_eq!(
        result.values.get(&height_id),
        Some(&length_scalar(0.2)),
        "height must read its 200mm default when no override was set"
    );

    // (b) No diagnostics — none of the override-mismatch warnings should
    //     fire for a cell with no override in the map.
    assert!(
        result.diagnostics.is_empty(),
        "untouched engine must not emit diagnostics on a vanilla eval, got: {:?}",
        result.diagnostics
    );

    // (c) clear_param_overrides on an already-empty map is a no-op: must
    //     not panic, and a subsequent eval still yields the defaults.
    engine.clear_param_overrides();
    let after_noop_clear = engine.eval(&module);
    assert_eq!(
        after_noop_clear.values.get(&width_id),
        Some(&length_scalar(0.1)),
        "clear on empty map must leave defaults intact"
    );
    assert!(
        after_noop_clear.diagnostics.is_empty(),
        "eval after no-op clear must still not emit diagnostics, got: {:?}",
        after_noop_clear.diagnostics
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2154 step-1: guarded-group active-member (members loop) override survival
// ──────────────────────────────────────────────────────────────────────────────

/// After an initial `eval()`, writing an override via `set_param_and_invalidate`
/// on a guarded-group Param (inside a `where guard { param x ... }` block) and
/// calling `eval()` again must surface the overridden value — NOT the compiled
/// module default.  Pre-task-2154 baseline: FAILED because the cold-eval
/// third-pass `members` loop in `Engine::eval` evaluated `cell.default_expr`
/// directly without consulting `self.param_overrides`.  Task-2154 consolidated
/// the resolution into `eval_guarded_group_param_cell`, which now backs both
/// call sites.
#[test]
fn eval_honors_override_on_guarded_group_active_member_param() {
    let mut engine = fresh_engine();
    let module = guarded_module(true, "Scalar", "5mm");
    let x_id = ValueCellId::new("S", "x");

    // Initial eval: x is inside the active branch, default should be 5mm = 0.005m.
    let first = engine.eval(&module);
    assert_eq!(
        first.values.get(&x_id),
        Some(&length_scalar(0.005)),
        "pre-override eval should yield the 5mm default for the guarded-group member"
    );

    // Establish override on the guarded-group Param.
    engine.set_param_and_invalidate(&x_id, length_scalar(0.012));

    // Re-evaluate — the cold-eval third pass must now consult param_overrides.
    let second = engine.eval(&module);
    assert_eq!(
        second.values.get(&x_id),
        Some(&length_scalar(0.012)),
        "post-override eval must surface the 0.012m override for the guarded-group member, not the 5mm default"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2154 step-3: guarded-group else-member (else_members loop) override survival
// ──────────────────────────────────────────────────────────────────────────────

/// After an initial `eval()`, writing an override via `set_param_and_invalidate`
/// on a guarded-group Param inside the `else { ... }` branch and calling `eval()`
/// again must surface the overridden value — NOT the compiled module default.
/// Pre-task-2154 baseline: FAILED because the cold-eval third-pass
/// `else_members` loop in `Engine::eval` evaluated `cell.default_expr` directly
/// without consulting `self.param_overrides`.  Task-2154 consolidated the
/// resolution into `eval_guarded_group_param_cell`, which now backs both call
/// sites.
#[test]
fn eval_honors_override_on_guarded_group_else_member_param() {
    let mut engine = fresh_engine();
    let module = guarded_module_with_else("Scalar", "10mm");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // Initial eval: guard is false so x is inactive (Undef), y is active (10mm = 0.01m).
    let first = engine.eval(&module);
    assert_eq!(
        first.values.get(&x_id),
        Some(&Value::Undef),
        "pre-override eval: x must be Undef (guard is false, member inactive)"
    );
    assert_eq!(
        first.values.get(&y_id),
        Some(&length_scalar(0.01)),
        "pre-override eval: y must be 10mm default (else_member active)"
    );

    // Establish override on the else-branch Param.
    engine.set_param_and_invalidate(&y_id, length_scalar(0.012));

    // Re-evaluate — the cold-eval else_members loop must now consult param_overrides.
    let second = engine.eval(&module);
    assert_eq!(
        second.values.get(&y_id),
        Some(&length_scalar(0.012)),
        "post-override eval must surface the 0.012m override for the else-branch Param, not the 10mm default"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2154 step-5: type-kind mismatch on guarded-group member emits warning
// ──────────────────────────────────────────────────────────────────────────────

/// When the user source is edited so that a guarded-group Param's type-kind no
/// longer matches the override value (here: Scalar[LENGTH] override against an
/// Int cell inside a `where active { ... }` block), eval() must:
/// - fall back to the module default (the Int default wins),
/// - emit a Warning diagnostic naming the cell + "type-kind",
/// - RETAIN the override in `param_overrides` so that reverting the edit
///   resurfaces it.
///
/// Mirrors eval_param_overrides.rs:139-191 but on a guarded-group Param.
#[test]
fn eval_skips_type_kind_mismatched_override_on_guarded_group_member_with_warning() {
    let mut engine = fresh_engine();
    let x_id = ValueCellId::new("S", "x");

    // Module A: x is Scalar[LENGTH] inside a guarded group. Set a matching override.
    let module_a = guarded_module(true, "Scalar", "5mm");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&x_id, length_scalar(0.12));

    // Module B: x is now an Int inside the same guard. The Scalar override is
    // type-kind incompatible.
    let module_b = guarded_module(true, "Int", "7");
    let result_b = engine.eval(&module_b);

    // (a) The Int default wins, not the (now-mismatched) Scalar override.
    assert_eq!(
        result_b.values.get(&x_id),
        Some(&Value::Int(7)),
        "type-kind mismatched override must be skipped in favour of Int default on guarded-group Param"
    );

    // (b) Exactly one Warning diagnostic calls out the mismatched cell.
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.x"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.x, got: {:?}",
        result_b.diagnostics
    );
    let wmsg = warnings[0].message.as_str();
    assert!(
        wmsg.contains("type-kind"),
        "warning should mention 'type-kind', got: {wmsg:?}"
    );

    // (c) The override is RETAINED — re-eval Module A: the Scalar override resurfaces.
    let module_a_again = guarded_module(true, "Scalar", "5mm");
    let result_a2 = engine.eval(&module_a_again);
    assert_eq!(
        result_a2.values.get(&x_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient type-kind mismatch eval on guarded-group Param"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2154 step-7: dimension mismatch on guarded-group member emits warning
// ──────────────────────────────────────────────────────────────────────────────

/// A guarded-group Param whose Scalar override carries a different SI dimension
/// (here: LENGTH override against a Mass/MASS cell) is type-kind compatible —
/// both sides are Scalar — so this path exercises ScalarDimensionMismatch, not
/// TypeKindMismatch. eval() must detect the dimension drift, skip the override,
/// emit a Warning mentioning "dimension", retain the override, fall back to the
/// Mass default.
///
/// Mirrors eval_param_overrides.rs:208-260 but on a guarded-group Param.
#[test]
fn eval_skips_dimension_mismatched_override_on_guarded_group_member_with_warning() {
    let mut engine = fresh_engine();
    let x_id = ValueCellId::new("S", "x");

    // Module A: x is Scalar[LENGTH] inside a guarded group. Set a LENGTH override.
    let module_a = guarded_module(true, "Scalar", "5mm");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&x_id, length_scalar(0.12));

    // Module B: x is now Mass (still Scalar kind; type-kind guard passes,
    // ScalarDimensionMismatch fires).
    let module_b = guarded_module(true, "Mass", "2kg");
    let result_b = engine.eval(&module_b);

    // (a) The Mass default wins (2kg → 2.0 SI).
    let expected_mass_default = Value::Scalar {
        si_value: 2.0,
        dimension: DimensionVector::MASS,
    };
    assert_eq!(
        result_b.values.get(&x_id),
        Some(&expected_mass_default),
        "dimension-mismatched override must be skipped in favour of the Mass default on guarded-group Param"
    );

    // (b) Exactly one Warning mentions the cell + the word "dimension".
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.x"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.x, got: {:?}",
        result_b.diagnostics
    );
    assert!(
        warnings[0].message.contains("dimension"),
        "dimension-mismatch warning should mention 'dimension', got: {:?}",
        warnings[0].message
    );
    // Also lock total warning count so spurious unrelated warnings are caught.
    assert_eq!(
        result_b
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count(),
        1,
        "expected exactly one Warning diagnostic in total, got: {:?}",
        result_b.diagnostics
    );

    // (c) Override is RETAINED — re-eval Module A: LENGTH override resurfaces.
    let module_a_again = guarded_module(true, "Scalar", "5mm");
    let result_a2 = engine.eval(&module_a_again);
    assert_eq!(
        result_a2.values.get(&x_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient dimension-mismatch eval on guarded-group Param"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2154 amend: type-kind mismatch on guarded-group ELSE-member emits warning
// ──────────────────────────────────────────────────────────────────────────────

/// Mirror of `eval_skips_type_kind_mismatched_override_on_guarded_group_member_with_warning`
/// but for the `else { ... }` branch. When the user source is edited so that an
/// else-branch Param's type-kind no longer matches the override value (here:
/// Scalar[LENGTH] override against an Int cell inside `else { param y ... }`),
/// eval() must:
/// - fall back to the module default (the Int default wins),
/// - emit a Warning diagnostic naming the cell + "type-kind",
/// - RETAIN the override in `param_overrides` so that reverting the edit
///   resurfaces it.
///
/// This test closes the symmetry gap left by the main task: the else_members
/// Param branch is a separately-compiled copy in engine_eval.rs and could
/// diverge independently from the members branch. Adding this test ensures the
/// helper `eval_guarded_group_param_cell` (task 2154 amend) correctly handles
/// the rejection path on both branches.
#[test]
fn eval_skips_type_kind_mismatched_override_on_guarded_group_else_member_with_warning() {
    let mut engine = fresh_engine();
    let y_id = ValueCellId::new("S", "y");

    // Module A: y is Scalar[LENGTH] in the else-branch (guard = false). Set a
    // matching LENGTH override.
    let module_a = guarded_module_with_else("Scalar", "10mm");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&y_id, length_scalar(0.12));

    // Module B: y is now an Int inside the else-branch. The Scalar override is
    // type-kind incompatible.
    let module_b = guarded_module_with_else("Int", "7");
    let result_b = engine.eval(&module_b);

    // (a) The Int default wins, not the (now-mismatched) Scalar override.
    assert_eq!(
        result_b.values.get(&y_id),
        Some(&Value::Int(7)),
        "type-kind mismatched override must be skipped in favour of Int default on guarded-group else-member Param"
    );

    // (b) Exactly one Warning diagnostic calls out the mismatched cell.
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.y"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.y, got: {:?}",
        result_b.diagnostics
    );
    let wmsg = warnings[0].message.as_str();
    assert!(
        wmsg.contains("type-kind"),
        "warning should mention 'type-kind', got: {wmsg:?}"
    );

    // (c) The override is RETAINED — re-eval Module A: the Scalar override resurfaces.
    let module_a_again = guarded_module_with_else("Scalar", "10mm");
    let result_a2 = engine.eval(&module_a_again);
    assert_eq!(
        result_a2.values.get(&y_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient type-kind mismatch eval on guarded-group else-member Param"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2234: dimension mismatch on guarded-group ELSE-member emits warning
// ──────────────────────────────────────────────────────────────────────────────

/// A guarded-group Param in the `else { ... }` branch whose Scalar override carries
/// a different SI dimension (here: LENGTH override against a Mass/MASS cell) is
/// type-kind compatible — both sides are Scalar — so this path exercises
/// ScalarDimensionMismatch, not TypeKindMismatch. eval() must detect the dimension
/// drift, skip the override, emit a Warning mentioning "dimension", retain the
/// override, fall back to the Mass default.
///
/// Closes the symmetry gap left by task-2154: the members-branch has [type-kind,
/// dimension, survival] tests; the else_members-branch had [type-kind, survival]
/// — this test adds the missing dimension test for the else-branch.
///
/// `eval_guarded_group_param_cell` (engine_eval.rs:216) is invoked from BOTH the
/// `members` loop and the `else_members` loop. A future change special-casing the
/// branches inside the helper would silently regress this path without the test.
///
/// Mirrors `eval_skips_dimension_mismatched_override_on_guarded_group_member_with_warning`
/// (the members-branch sibling) — only the cell name (`y` vs `x`), the active flag
/// (`false` vs `true`), and the source skeleton (else vs members) differ.
#[test]
fn eval_skips_dimension_mismatched_override_on_guarded_group_else_member_with_warning() {
    let mut engine = fresh_engine();
    let y_id = ValueCellId::new("S", "y");

    // Module A: y is Scalar[LENGTH] in the else-branch (guard = false). Set a
    // LENGTH override.
    let module_a = guarded_module_with_else("Scalar", "10mm");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&y_id, length_scalar(0.12));

    // Module B: y is now Mass (still Scalar kind; type-kind guard passes,
    // ScalarDimensionMismatch fires on the else-branch helper invocation).
    let module_b = guarded_module_with_else("Mass", "2kg");
    let result_b = engine.eval(&module_b);

    // (a) The Mass default wins (2kg → 2.0 SI).
    let expected_mass_default = Value::Scalar {
        si_value: 2.0,
        dimension: DimensionVector::MASS,
    };
    assert_eq!(
        result_b.values.get(&y_id),
        Some(&expected_mass_default),
        "dimension-mismatched override must be skipped in favour of Mass default on guarded-group else-member Param"
    );

    // (b) Exactly one Warning mentions the cell + the word "dimension".
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.y"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.y, got: {:?}",
        result_b.diagnostics
    );
    assert!(
        warnings[0].message.contains("dimension"),
        "dimension-mismatch warning should mention 'dimension', got: {:?}",
        warnings[0].message
    );
    // Also lock total warning count so spurious unrelated warnings are caught.
    assert_eq!(
        result_b
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count(),
        1,
        "expected exactly one Warning diagnostic in total, got: {:?}",
        result_b.diagnostics
    );

    // (c) Override is RETAINED — re-eval Module A: LENGTH override resurfaces.
    let module_a_again = guarded_module_with_else("Scalar", "10mm");
    let result_a2 = engine.eval(&module_a_again);
    assert_eq!(
        result_a2.values.get(&y_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient dimension-mismatch eval on guarded-group else-member Param"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2179 S4: rejected-override-with-no-default inserts Undef into result.values
// ──────────────────────────────────────────────────────────────────────────────

/// When an override is rejected (type-kind mismatch) AND the cell has no
/// default_expr, `result.values` must contain `Value::Undef` for the cell
/// rather than a missing key.  Without the S4 fix, `result.values.get(&p_id)`
/// returns `None`, which would panic any caller that does `.get().unwrap()`.
///
/// Three-phase setup:
///   A) Module with `param p: Scalar = 1mm` + `let q: Scalar = p` — set a
///      Scalar[LENGTH] override (0.5 m) so it is stored in param_overrides.
///   B) Module with `param p: Int` (NO default) + `let q: Int = p` — the
///      stored Scalar override is now type-kind incompatible.
///
/// Assertions on result from evaluating module B:
///   (a) `result.values.get(&p_id) == Some(&Value::Undef)` (the S4 discriminator;
///       currently `None` — the test is expected to FAIL before the fix).
///   (b) `result.values.get(&q_id) == Some(&Value::Undef)` — downstream Let
///       propagates Undef without panic.
///   (c) Exactly one Warning diagnostic mentions "S.p" and "type-kind".
///   (d) `engine.snapshot().values[p] == (Value::Undef, Undetermined)` — orthogonal
///       to (a): `EvalResult.values` and the persistent `Snapshot` are separate maps.
#[test]
fn eval_inserts_undef_for_no_default_param_with_rejected_override() {
    let mut engine = fresh_engine();
    let p_id = ValueCellId::new("S", "p");
    let q_id = ValueCellId::new("S", "q");

    // Phase A: module with p: Scalar = 1mm. Set a valid Scalar[LENGTH] override.
    let module_a = compile_source("structure S { param p: Scalar = 1mm\n let q: Scalar = p }");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&p_id, length_scalar(0.5));

    // Phase B: module with p: Int (no default). Scalar override is incompatible.
    let module_b = compile_source("structure S { param p: Int\n let q: Int = p }");
    let result_b = engine.eval(&module_b);

    // (a) S4 assertion: rejected-override-no-default cell must be Undef, not absent.
    assert_eq!(
        result_b.values.get(&p_id),
        Some(&Value::Undef),
        "rejected-override-with-no-default param must be Undef in result.values, not missing (got None)"
    );

    // (b) Downstream Let q = p must propagate Undef without panic.
    assert_eq!(
        result_b.values.get(&q_id),
        Some(&Value::Undef),
        "let q = p must propagate Undef when p is Undef"
    );

    // (c) Exactly one Warning mentioning S.p and the word "type-kind".
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.p"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.p, got: {:?}",
        result_b.diagnostics
    );
    assert!(
        warnings[0].message.contains("type-kind"),
        "warning should mention 'type-kind', got: {:?}",
        warnings[0].message
    );

    // (d) Snapshot half: the persistent snapshot entry must also be
    // (Undef, Undetermined).  Orthogonal to (a): `EvalResult.values` and the
    // persistent `Snapshot` are separate maps.  See engine_eval.rs:469-479.
    assert_eq!(
        engine.snapshot().unwrap().values.get(&p_id),
        Some(&(Value::Undef, DeterminacyState::Undetermined)),
        "rejected-override-no-default path (engine_eval.rs:469-479): \
         snapshot entry for p must be (Undef, Undetermined)"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2179 amend: dimension-mismatch path also inserts Undef
// ──────────────────────────────────────────────────────────────────────────────

/// Sibling of `eval_inserts_undef_for_no_default_param_with_rejected_override`
/// that exercises the `ScalarDimensionMismatch` arm rather than `TypeKindMismatch`.
///
/// When an override is rejected via dimension mismatch AND the cell has no
/// `default_expr`, `result.values` must contain `Value::Undef` for the cell
/// rather than a missing key — the same S4 guarantee as the type-kind path.
///
/// Three-phase setup:
///   A) Module with `param p: Scalar = 1mm` + `let q: Scalar = p` — set a
///      Scalar[LENGTH] override (0.5 m).
///   B) Module with `param p: Mass` (NO default) + `let q: Mass = p` — the
///      stored LENGTH override is now dimension-incompatible with MASS.
///
/// Assertions on result from evaluating module B:
///   (a) `result.values.get(&p_id) == Some(&Value::Undef)` (S4 discriminator).
///   (b) `result.values.get(&q_id) == Some(&Value::Undef)` — downstream Let
///       propagates Undef without panic.
///   (c) Exactly one Warning diagnostic mentions "S.p" and "dimension".
///   (d) `engine.snapshot().values[p] == (Value::Undef, Undetermined)` — orthogonal
///       to (a): `EvalResult.values` and the persistent `Snapshot` are separate maps.
#[test]
fn eval_inserts_undef_for_no_default_param_with_dimension_rejected_override() {
    let mut engine = fresh_engine();
    let p_id = ValueCellId::new("S", "p");
    let q_id = ValueCellId::new("S", "q");

    // Phase A: module with p: Scalar = 1mm. Set a valid Scalar[LENGTH] override.
    let module_a = compile_source("structure S { param p: Scalar = 1mm\n let q: Scalar = p }");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&p_id, length_scalar(0.5));

    // Phase B: module with p: Mass (no default). LENGTH override is dimension-
    // incompatible with MASS (same type-kind — both Scalar — so TypeKindMismatch
    // does not fire, only ScalarDimensionMismatch).
    let module_b = compile_source("structure S { param p: Mass\n let q: Mass = p }");
    let result_b = engine.eval(&module_b);

    // (a) S4 assertion: dimension-rejected-override-no-default must be Undef.
    assert_eq!(
        result_b.values.get(&p_id),
        Some(&Value::Undef),
        "dimension-rejected-override-with-no-default param must be Undef in result.values, not missing"
    );

    // (b) Downstream Let q = p must propagate Undef without panic.
    assert_eq!(
        result_b.values.get(&q_id),
        Some(&Value::Undef),
        "let q = p must propagate Undef when p is Undef (dimension-mismatch path)"
    );

    // (c) Exactly one Warning mentioning S.p and the word "dimension".
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.p"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.p, got: {:?}",
        result_b.diagnostics
    );
    assert!(
        warnings[0].message.contains("dimension"),
        "warning should mention 'dimension', got: {:?}",
        warnings[0].message
    );

    // (d) Snapshot half: the persistent snapshot entry must also be
    // (Undef, Undetermined).  Orthogonal to (a): EvalResult.values and the
    // persistent Snapshot are separate maps.  See engine_eval.rs:469-479.
    assert_eq!(
        engine.snapshot().unwrap().values.get(&p_id),
        Some(&(Value::Undef, DeterminacyState::Undetermined)),
        "rejected-override-no-default path (engine_eval.rs:469-479): \
         snapshot entry for p must be (Undef, Undetermined) after dimension-mismatch rejection"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2194 S2: partial-map invariant — no-override-no-default cells are absent
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock for the partial-map shape of `EvalResult.values`: when a
/// Param cell has NO override stored AND NO `default_expr`, the cell is
/// intentionally OMITTED from `EvalResult.values` (preserves pre-task-2017
/// silent-skip semantics; engine_eval.rs:494-518). A sibling Param cell that
/// HAS a default still appears in the map — the omission is per-cell, not
/// per-module.
///
/// This test would FAIL against any future "unify (a)" refactor that switches
/// the no-override-no-default arm from `continue` to `(Undef, Undetermined)`
/// insert. Such a change MUST be intentional and accompanied by an explicit
/// behavioural-contract update — this test is the drift detector.
///
/// Orthogonal counterpart to `eval_inserts_undef_for_no_default_param_with_rejected_override`
/// (line 752): that test pins PRESENT for the rejected-override-no-default
/// branch; this test pins ABSENT for the no-override-no-default branch.
#[test]
fn eval_omits_no_default_no_override_param_cell_from_result_values() {
    let mut engine = fresh_engine();
    let p_id = ValueCellId::new("S", "p");
    let other_id = ValueCellId::new("S", "other");

    // p has neither override (fresh engine) nor default_expr.
    // other has a default — exercises the NOT-omitted path as a positive control.
    let module = compile_source("structure S { param p: Int\n param other: Int = 42 }");
    let result = engine.eval(&module);

    // (a) Partial-map invariant: p must be ABSENT from result.values.
    assert!(
        result.values.get(&p_id).is_none(),
        "no-override-no-default Param cell `p` must be absent from EvalResult.values \
         (partial-map invariant; engine_eval.rs:494-518), got: {:?}",
        result.values.get(&p_id)
    );

    // (b) Positive control: a sibling Param with a default IS present.
    //     Sharpens the lock — proves the omission is per-cell, not a regression
    //     where eval starts producing empty value maps.
    assert_eq!(
        result.values.get(&other_id),
        Some(&Value::Int(42)),
        "sibling Param cell `other` (with default) must be present in result.values"
    );

    // (c) Snapshot pre-seed survives: Snapshot::from_compiled_module pre-seeds
    //     every cell with (Undef, Undetermined); the no-override-no-default
    //     `continue` does NOT touch snapshot.values, so the pre-seed value
    //     remains. This is the orthogonal half of the task-2179 snapshot
    //     assertion (line 801) — together they cover both branches.
    assert_eq!(
        engine.snapshot().unwrap().values.get(&p_id),
        Some(&(Value::Undef, DeterminacyState::Undetermined)),
        "snapshot pre-seed for no-override-no-default cell must remain (Undef, Undetermined)"
    );

    // (d) No diagnostics: silent-skip means no warning fires for the omission.
    assert!(
        result.diagnostics.is_empty(),
        "no-override-no-default Param cell must not emit any diagnostics, got: {:?}",
        result.diagnostics
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2195 step-1: S4 top-level path records cache entry alongside journal pair
// ──────────────────────────────────────────────────────────────────────────────

/// Before task-2195, the S4 top-level Param branch (rejected-override-no-default)
/// intentionally omitted `cache.record_evaluation` — the deferred-cache rationale
/// comment (engine_eval.rs:584-593) documents that deferral. Task-2195 resolves it.
///
/// Setup mirrors `eval_inserts_undef_for_no_default_param_with_rejected_override`:
///   A) Param `p: Scalar = 1mm` — set a Scalar[LENGTH] override.
///   B) Param `p: Int` (NO default) — override is type-kind incompatible → S4 arm fires.
///
/// Assertions (on state after evaluating module B):
///   (a) `engine.cache_store().get(&NodeId::Value(p_id))` is Some with
///       `entry.result == CachedResult::Value(Value::Undef, Undetermined)`.
///       Currently FAILS — S4 arm skips cache.record_evaluation.
///   (b) Journal for `p` has both a `Started` and a `Completed` event (sanity check).
#[test]
fn eval_records_cache_entry_alongside_journal_pair_for_top_level_s4_path() {
    let mut engine = fresh_engine();
    let p_id = ValueCellId::new("S", "p");

    // Phase A: set a valid Scalar override so it's stored.
    let module_a = compile_source("structure S { param p: Scalar = 1mm\n let q: Scalar = p }");
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&p_id, length_scalar(0.5));

    // Phase B: Int param with no default — S4 arm fires.
    let module_b = compile_source("structure S { param p: Int\n let q: Int = p }");
    let _ = engine.eval(&module_b);

    let node_id = NodeId::Value(p_id.clone());

    // (a) Cache entry must exist for p — FAILS before step-2 impl.
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("S4 path must write a cache entry for p (task-2195 resolves deferred-cache)");
    match &entry.result {
        CachedResult::Value(val, det) => {
            assert_eq!(*val, Value::Undef, "S4 cache entry value must be Undef");
            assert_eq!(
                *det,
                DeterminacyState::Undetermined,
                "S4 cache entry determinacy must be Undetermined"
            );
        }
        other => panic!("expected CachedResult::Value for S4 path, got {:?}", other),
    }

    // (b) Journal sanity check: Started + Completed pair present.
    let events = engine.journal().events_for_node(&node_id);
    assert!(
        events.iter().any(|e| matches!(e.kind, EventKind::Started)),
        "journal must have a Started event for p (S4 path)"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Completed { .. })),
        "journal must have a Completed event for p (S4 path)"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2195 step-3: guarded-group active-branch Param records journal + cache
// ──────────────────────────────────────────────────────────────────────────────

/// `eval_guarded_group_param_cell` (engine_eval.rs:180-254) currently emits
/// neither journal events nor cache writes for any of its four value-write paths.
/// Task-2195 fixes that. This test drives the implementation.
///
/// Setup: `guarded_module(true, "Scalar", "5mm")` — guard is `active: Bool = true`
/// so the `members` loop runs `eval_guarded_group_param_cell` on `S.x`. The
/// override bucket is empty on a fresh engine, so the default-eval path fires
/// and produces `Value::Scalar { si_value: 0.005, dimension: LENGTH }`.
///
/// Assertions after `engine.eval(&module)`:
///   (a) `engine.journal().events_for_node(&NodeId::Value(x_id))` has at least
///       one `EventKind::Started` AND one `EventKind::Completed { .. }`.
///       Currently FAILS — the helper emits no journal events.
///   (b) `engine.cache_store().get(&NodeId::Value(x_id))` is Some with
///       `CachedResult::Value(Value::Scalar { 0.005m LENGTH }, Determined)`.
///       Currently FAILS — the helper writes no cache entries.
#[test]
fn eval_records_journal_pair_and_cache_entry_for_guarded_group_active_branch_param() {
    let mut engine = fresh_engine();
    let module = guarded_module(true, "Scalar", "5mm");
    engine.eval(&module);

    let x_id = ValueCellId::new("S", "x");
    let node_id = NodeId::Value(x_id.clone());

    // (a) Journal: exactly one Started+Completed pair, in order — FAILS before step-4 impl.
    let events = engine.journal().events_for_node(&node_id);
    assert_eq!(
        events.len(),
        2,
        "guarded-group active-branch Param must emit exactly one Started+Completed pair (task-2195)"
    );
    assert!(
        matches!(events[0].kind, EventKind::Started),
        "first journal event for x must be Started (task-2195)"
    );
    assert!(
        matches!(events[1].kind, EventKind::Completed { .. }),
        "second journal event for x must be Completed (task-2195)"
    );

    // (b) Cache entry: CachedResult::Value(5mm as SI, Determined) — FAILS before step-4 impl.
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("guarded-group active-branch Param must write a cache entry (task-2195)");
    match &entry.result {
        CachedResult::Value(val, det) => {
            assert_eq!(
                *val,
                length_scalar(0.005),
                "guarded-group x cache value must be 5mm (0.005 m SI)"
            );
            assert_eq!(
                *det,
                DeterminacyState::Determined,
                "guarded-group x cache determinacy must be Determined (has default)"
            );
        }
        other => panic!(
            "expected CachedResult::Value for guarded-group active-branch x, got {:?}",
            other
        ),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2195 step-5: guarded-group else-branch Param regression lock
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock for the `else_members` call site of `eval_guarded_group_param_cell`
/// (engine_eval.rs else_members loop). Step-4 updated BOTH call sites; this test
/// would catch a partial fix that only updated the `members` call site.
///
/// Setup: `guarded_module_with_else("Scalar", "10mm")` — guard is
/// `active: Bool = false`, so the `else_members` loop runs the helper on `S.y`.
/// With a fresh engine (no override), the default-eval path fires and produces
/// `Value::Scalar { si_value: 0.01, dimension: LENGTH }` (10mm = 0.01 m SI).
///
/// Assertions after `engine.eval(&module)`:
///   (a) Journal for `y` has both Started and Completed events.
///   (b) Cache has `CachedResult::Value(0.01m LENGTH, Determined)` for `y`.
#[test]
fn eval_records_journal_pair_and_cache_entry_for_guarded_group_else_branch_param() {
    let mut engine = fresh_engine();
    let module = guarded_module_with_else("Scalar", "10mm");
    engine.eval(&module);

    let y_id = ValueCellId::new("S", "y");
    let node_id = NodeId::Value(y_id.clone());

    // (a) Journal: exactly one Started+Completed pair, in order (else_members call site).
    let events = engine.journal().events_for_node(&node_id);
    assert_eq!(
        events.len(),
        2,
        "else-branch Param y must emit exactly one Started+Completed pair (else_members call site)"
    );
    assert!(
        matches!(events[0].kind, EventKind::Started),
        "first journal event for y must be Started (else_members call site)"
    );
    assert!(
        matches!(events[1].kind, EventKind::Completed { .. }),
        "second journal event for y must be Completed (else_members call site)"
    );

    // (b) Cache: CachedResult::Value(10mm SI, Determined) for y.
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("else-branch Param y must have a cache entry (else_members call site)");
    match &entry.result {
        CachedResult::Value(val, det) => {
            assert_eq!(
                *val,
                length_scalar(0.01),
                "else-branch y cache value must be 10mm (0.01 m SI)"
            );
            assert_eq!(
                *det,
                DeterminacyState::Determined,
                "else-branch y cache determinacy must be Determined"
            );
        }
        other => panic!(
            "expected CachedResult::Value for else-branch y, got {:?}",
            other
        ),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2195 step-6: helper rejected-override-no-default arm records journal+cache
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock for the rejected-override-no-default early-return path inside
/// `eval_guarded_group_param_cell` (the helper's S4-equivalent). Step-4 must have
/// covered ALL FOUR paths including this one; this test catches a partial fix that
/// only instrumented the success paths.
///
/// Two-phase setup triggering the else-branch helper's rejected-no-default arm:
///   A) `structure S { param active : Bool = false\n where active { param x : Scalar = 5mm } else { param y : Scalar = 10mm } }`
///      — set a Scalar[LENGTH] override on `S.y` (valid override stored).
///   B) `structure S { param active : Bool = false\n where active { param x : Scalar = 5mm } else { param y : Int } }`
///      — `y` is now `Int` with NO default; the Scalar override is type-kind
///        incompatible → helper's rejected-override-no-default arm fires.
///
/// Assertions after evaluating module B:
///   (a) Journal for `y` has Started + Completed.
///   (b) Cache for `y` has `CachedResult::Value(Undef, Undetermined)`.
///   (c) Exactly one Warning mentioning "S.y" and "type-kind" (existing contract).
///   (d) Override is RETAINED — re-eval with module A resurfaces the override.
#[test]
fn eval_records_journal_pair_and_cache_entry_for_guarded_group_rejected_override_no_default_param()
{
    let mut engine = fresh_engine();
    let y_id = ValueCellId::new("S", "y");
    let node_id = NodeId::Value(y_id.clone());

    // Phase A: compile module with y: Scalar = 10mm. Set a valid Scalar override.
    let module_a = compile_source(
        "structure S { param active : Bool = false\n where active { param x : Scalar = 5mm } else { param y : Scalar = 10mm } }",
    );
    let _ = engine.eval(&module_a);
    engine.set_param_and_invalidate(&y_id, length_scalar(0.12));

    // Phase B: y is now Int (no default) — Scalar override is type-kind incompatible.
    let module_b = compile_source(
        "structure S { param active : Bool = false\n where active { param x : Scalar = 5mm } else { param y : Int } }",
    );
    // Snapshot journal length before phase B so we can check the delta for phase B alone
    // (phase A already contributed a Started+Completed pair for y).
    let journal_len_before_b = engine.journal().events_for_node(&node_id).len();
    let result_b = engine.eval(&module_b);

    // (a) Journal: exactly one Started+Completed pair added by phase B, in order.
    let events = engine.journal().events_for_node(&node_id);
    let phase_b_events = &events[journal_len_before_b..];
    assert_eq!(
        phase_b_events.len(),
        2,
        "helper rejected-no-default arm must emit exactly one Started+Completed pair"
    );
    assert!(
        matches!(phase_b_events[0].kind, EventKind::Started),
        "first phase-B journal event for y must be Started"
    );
    assert!(
        matches!(phase_b_events[1].kind, EventKind::Completed { .. }),
        "second phase-B journal event for y must be Completed"
    );

    // (b) Cache: CachedResult::Value(Undef, Undetermined).
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("helper rejected-no-default arm must write a cache entry");
    match &entry.result {
        CachedResult::Value(val, det) => {
            assert_eq!(
                *val,
                Value::Undef,
                "rejected-no-default cache value must be Undef"
            );
            assert_eq!(
                *det,
                DeterminacyState::Undetermined,
                "rejected-no-default cache determinacy must be Undetermined"
            );
        }
        other => panic!(
            "expected CachedResult::Value(Undef, Undetermined) for rejected-no-default y, got {:?}",
            other
        ),
    }

    // (c) Exactly one Warning mentioning S.y and "type-kind".
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.y"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.y, got: {:?}",
        result_b.diagnostics
    );
    assert!(
        warnings[0].message.contains("type-kind"),
        "warning must mention 'type-kind', got: {:?}",
        warnings[0].message
    );

    // (d) Override is retained — re-eval module A resurfaces the 0.12m override.
    let result_a2 = engine.eval(&module_a);
    assert_eq!(
        result_a2.values.get(&y_id),
        Some(&length_scalar(0.12)),
        "override must survive a transient type-kind mismatch in the helper's rejected-no-default arm"
    );
    // (d2) Cache must now reflect the resurfaced Determined override value, not the stale Undef.
    // This locks in the correctness of the deferred-cache deferral resolution from task-2195:
    // a cached Undef must not mask a recovered override via the incremental fast-path.
    let entry_after = engine
        .cache_store()
        .get(&node_id)
        .expect("cache entry must exist for y after module A re-eval");
    match &entry_after.result {
        CachedResult::Value(val, det) => {
            assert_eq!(
                *val,
                length_scalar(0.12),
                "cache must hold the resurfaced override value (0.12m) after module A re-eval"
            );
            assert_eq!(
                *det,
                DeterminacyState::Determined,
                "cache determinacy must be Determined for resurfaced override"
            );
        }
        other => panic!(
            "expected CachedResult::Value(0.12m, Determined) after module A re-eval, got {:?}",
            other
        ),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2195 step-7: helper no-override-no-default arm records journal+cache
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock for the no-override-no-default early-return path inside
/// `eval_guarded_group_param_cell` (lines 200-206 in the original). Step-4 must
/// have covered ALL FOUR paths; this test catches a partial fix that only
/// instrumented the success and rejected-override paths.
///
/// The helper's no-override-no-default arm intentionally writes Undef (diverging
/// from the top-level Param branch which bare-continues — task-2154 design
/// decision). Recording in journal+cache makes this write visible to tooling.
///
/// Setup: `structure S { param active : Bool = true\n where active { param x : Int } }`
///   — active=true so the `members` loop runs the helper on `S.x`.
///   — `x` has no `default_expr` and the fresh engine has no override for it.
///   — The no-override-no-default arm fires.
///
/// Assertions after `engine.eval(&module)`:
///   (a) Journal for `x` has Started + Completed.
///   (b) Cache for `x` has `CachedResult::Value(Undef, Undetermined)`.
///   (c) `result.values.get(&x_id) == Some(&Value::Undef)` — the intentional Undef
///       write contract from task-2154: guarded-group cells always appear in the map.
///   (d) No diagnostics — the no-override-no-default path is silent.
#[test]
fn eval_records_journal_pair_and_cache_entry_for_guarded_group_no_override_no_default_param() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure S { param active : Bool = true\n where active { param x : Int } }",
    );
    let result = engine.eval(&module);

    let x_id = ValueCellId::new("S", "x");
    let node_id = NodeId::Value(x_id.clone());

    // (a) Journal: exactly one Started+Completed pair, in order (no-override-no-default path).
    let events = engine.journal().events_for_node(&node_id);
    assert_eq!(
        events.len(),
        2,
        "helper no-override-no-default arm must emit exactly one Started+Completed pair"
    );
    assert!(
        matches!(events[0].kind, EventKind::Started),
        "first journal event for x must be Started (no-override-no-default)"
    );
    assert!(
        matches!(events[1].kind, EventKind::Completed { .. }),
        "second journal event for x must be Completed (no-override-no-default)"
    );

    // (b) Cache: CachedResult::Value(Undef, Undetermined).
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("helper no-override-no-default arm must write a cache entry");
    match &entry.result {
        CachedResult::Value(val, det) => {
            assert_eq!(
                *val,
                Value::Undef,
                "no-override-no-default cache value must be Undef"
            );
            assert_eq!(
                *det,
                DeterminacyState::Undetermined,
                "no-override-no-default cache determinacy must be Undetermined"
            );
        }
        other => panic!(
            "expected CachedResult::Value(Undef, Undetermined) for no-override-no-default x, got {:?}",
            other
        ),
    }

    // (c) EvalResult.values has Undef for x — intentional Undef write (task-2154 contract).
    assert_eq!(
        result.values.get(&x_id),
        Some(&Value::Undef),
        "guarded-group no-override-no-default cell must be present as Undef in result.values \
         (task-2154 contract: all guarded cells appear in the map)"
    );

    // (d) No diagnostics — silent write, unlike the rejected-override path which warns.
    assert!(
        result.diagnostics.is_empty(),
        "no-override-no-default guarded-group Param must not emit diagnostics, got: {:?}",
        result.diagnostics
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2270 step-1: version-threading regression lock — top-level Param branch
// ──────────────────────────────────────────────────────────────────────────────

/// Regression-lock test: every `EvalEvent` emitted for a top-level Param cell
/// must carry `event.version == engine.snapshot().unwrap().version`.
///
/// This pins the version-threading invariant for the top-level Param branch in
/// `Engine::eval`. A future regression where the hoisted `version` binding drifts
/// from the snapshot's `VersionId` (e.g., reading a stale value, off-by-one
/// increment) would break this test.
///
/// Setup: `structure S { param width: Scalar = 100mm }` — single top-level Param
/// cell. After `engine.eval(&module)`, the journal for `S.width` must hold a
/// Started+Completed pair, and both events must carry the engine's snapshot version.
#[test]
fn eval_threads_snapshot_version_through_top_level_param_journal_events() {
    let mut engine = fresh_engine();
    let module = compile_source("structure S { param width: Scalar = 100mm }");
    engine.eval(&module);

    let width_id = ValueCellId::new("S", "width");
    let node_id = NodeId::Value(width_id);

    let snapshot_version = engine.snapshot().unwrap().version;
    let events = engine.journal().events_for_node(&node_id);

    assert!(
        !events.is_empty(),
        "journal must have at least one event for S.width after eval"
    );

    for event in &events {
        assert_eq!(
            event.version, snapshot_version,
            "every journal event for S.width must carry the engine snapshot version \
             (event.version={:?}, snapshot.version={:?})",
            event.version, snapshot_version,
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// task-2270 step-3: version-threading regression lock — guarded-group both arms
// ──────────────────────────────────────────────────────────────────────────────

/// Regression-lock test: every `EvalEvent` emitted for a guarded-group Param
/// cell must carry `event.version == engine.snapshot().unwrap().version`, for
/// BOTH the members (active-branch) and else_members (else-branch) call sites
/// of `eval_guarded_group_param_cell`.
///
/// Uses two separate engine instances so each arm can be exercised independently
/// (a single module's guard is either true or false, not both at once):
///   (1) `engine_a` with `guarded_module(true, "Scalar", "5mm")` — members call site.
///   (2) `engine_b` with `guarded_module_with_else("Scalar", "10mm")` — else_members call site.
///
/// If either call site threads a different or stale version (e.g., a future
/// refactor moves the `param_ctx` initialization into the wrong scope and captures
/// an outdated `version_id`), this test breaks.
#[test]
fn eval_threads_snapshot_version_through_guarded_group_param_journal_events_on_both_call_sites() {
    // (1) members call site — guard is true, x is evaluated via the members loop.
    let mut engine_a = fresh_engine();
    let module_a = guarded_module(true, "Scalar", "5mm");
    engine_a.eval(&module_a);

    let x_id = ValueCellId::new("S", "x");
    let node_id_x = NodeId::Value(x_id);
    let snap_version_a = engine_a.snapshot().unwrap().version;
    let events_a = engine_a.journal().events_for_node(&node_id_x);

    assert!(
        !events_a.is_empty(),
        "journal must have at least one event for S.x (members call site)"
    );
    for event in &events_a {
        assert_eq!(
            event.version, snap_version_a,
            "members-branch journal event for S.x must carry the engine snapshot version \
             (event.version={:?}, snapshot.version={:?})",
            event.version, snap_version_a,
        );
    }

    // (2) else_members call site — guard is false, y is evaluated via the else_members loop.
    let mut engine_b = fresh_engine();
    let module_b = guarded_module_with_else("Scalar", "10mm");
    engine_b.eval(&module_b);

    let y_id = ValueCellId::new("S", "y");
    let node_id_y = NodeId::Value(y_id);
    let snap_version_b = engine_b.snapshot().unwrap().version;
    let events_b = engine_b.journal().events_for_node(&node_id_y);

    assert!(
        !events_b.is_empty(),
        "journal must have at least one event for S.y (else_members call site)"
    );
    for event in &events_b {
        assert_eq!(
            event.version, snap_version_b,
            "else-branch journal event for S.y must carry the engine snapshot version \
             (event.version={:?}, snapshot.version={:?})",
            event.version, snap_version_b,
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// step-5 / step-6: Money source-form rendering in dimension-mismatch warnings
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock: when a param-override dimension mismatch fires for a
/// Money-typed param, the warning message must render the dimensions in
/// source form (e.g. "USD", "kg") rather than Debug format
/// (e.g. "DimensionVector([Rational{…},…])").
///
/// Setup:
///   Module A  — `param p: Mass = 5kg`     (MASS Param cell; override stored as MASS)
///   Module B  — `param p: Money = 0.0`    (MONEY Param cell; MASS override mismatches)
///
/// Note: `auto` is intentionally NOT used for the Money param default because
/// `param p: Money = auto` compiles to `ValueCellKind::Auto`, which is pruned by
/// `prune_param_overrides_against` (it only retains `ValueCellKind::Param` cells).
/// A concrete default (`0.0`) keeps the cell as `Param` kind so the override
/// survives pruning and reaches `validate_param_override`.
///
/// Expected warning: "dimension mismatch (expected USD, got kg)" — both units
/// in source form.  Today this FAILS because engine_eval.rs:443 still uses
/// `{:?}`, which produces the raw Rational vector instead of human-readable units.
#[test]
fn param_override_dimension_mismatch_warning_renders_money_in_source_form() {
    let mut engine = fresh_engine();
    let p_id = ValueCellId::new("S", "p");

    // Module A: param p is Mass (Param kind).  Register a MASS-dimensioned override.
    let module_a = compile_source("structure S { param p: Mass = 5kg }");
    let _ = engine.eval(&module_a);
    let mass_override = Value::Scalar {
        si_value: 5.0,
        dimension: DimensionVector::MASS,
    };
    engine.set_param_and_invalidate(&p_id, mass_override);

    // Module B: param p is Money (Param kind — non-auto default preserves Param kind
    // so the override survives prune_param_overrides_against).
    // The MASS override mismatches the MONEY cell_type → ScalarDimensionMismatch warning.
    let module_b = compile_source("structure S { param p: Money = 0.0 }");
    let result_b = engine.eval(&module_b);

    // Exactly one Warning mentioning S.p must be emitted.
    let warnings: Vec<&reify_core::Diagnostic> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("S.p"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning mentioning S.p, got: {:?}",
        result_b.diagnostics
    );
    let msg = &warnings[0].message;

    // Source-form rendering: both dimensions must appear as unit names.
    assert!(
        msg.contains("USD"),
        "warning must contain 'USD' (Money source form), got: {:?}",
        msg
    );
    assert!(
        msg.contains("kg"),
        "warning must contain 'kg' (Mass source form), got: {:?}",
        msg
    );

    // Debug-format artefacts must NOT appear.
    assert!(
        !msg.contains("Rational"),
        "warning must not contain 'Rational' (debug format), got: {:?}",
        msg
    );
    assert!(
        !msg.contains("DimensionVector("),
        "warning must not contain 'DimensionVector(' (debug format), got: {:?}",
        msg
    );
}
