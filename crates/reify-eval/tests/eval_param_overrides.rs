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
use reify_types::{DeterminacyState, DimensionVector, Severity, Value, ValueCellId};

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
///   (d) `engine.snapshot().unwrap().values.get(&p_id) ==
///       Some(&(Value::Undef, DeterminacyState::Undetermined))`.
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
    let warnings: Vec<&reify_types::Diagnostic> = result_b
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

    // (d) Snapshot must hold (Undef, Undetermined) for the rejected cell.
    //
    // NOTE: `Snapshot::from_compiled_module` already pre-seeds every value
    // cell with (Undef, Undetermined), so this assertion would pass even
    // without the explicit `snapshot.values.insert` in engine_eval.rs.
    // The explicit insert is defence-in-depth (see design decision in
    // plan.json); to truly verify the overwrite behaviour would require a
    // more complex setup that leaves the snapshot with a Determined value
    // for `p` just before Phase B's eval (e.g. by issuing a successful
    // eval of a compatible module so the snapshot holds the override value
    // as Determined, then calling eval on the incompatible module). That
    // complexity is out of scope here; this assertion validates the correct
    // final shape is present regardless of how it was written.
    let snap_val = engine.snapshot().unwrap().values.get(&p_id).cloned();
    assert_eq!(
        snap_val,
        Some((Value::Undef, DeterminacyState::Undetermined)),
        "snapshot must hold (Undef, Undetermined) for rejected-override-no-default param, got: {:?}",
        snap_val
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
///   (d) `engine.snapshot().unwrap().values.get(&p_id) ==
///       Some(&(Value::Undef, DeterminacyState::Undetermined))`.
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
    let warnings: Vec<&reify_types::Diagnostic> = result_b
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

    // (d) Snapshot must hold (Undef, Undetermined) for the rejected cell.
    let snap_val = engine.snapshot().unwrap().values.get(&p_id).cloned();
    assert_eq!(
        snap_val,
        Some((Value::Undef, DeterminacyState::Undetermined)),
        "snapshot must hold (Undef, Undetermined) for dimension-rejected-override-no-default param, got: {:?}",
        snap_val
    );
}
