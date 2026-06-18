//! Engine-level integration tests for the imported-geometry tolerance
//! promise contract (per task 2651 / PRD
//! `docs/prds/v0_2/per-purpose-tolerance.md` "Resolved design decisions" →
//! "Imported geometry promise"; arch §10.4 / §14.5).
//!
//! Builds a hand-crafted `STEPInput` template carrying a
//! `param provenance : Provenance` declaration whose post-`eval()`
//! value-cell entry (`provenance.tolerance_guarantee`) is the imported-geometry
//! tolerance promise. Asserts the promise is observable via
//! `Engine::imported_tolerance_promise`, then pairs it with the existing
//! demand-side fixture pattern (manufacturing purpose + STEPOutput template +
//! MyDesign subject) to exercise `Engine::check_imported_tolerance_promise`'s
//! strict-tighter-than-promise warning emission and the four no-op rows of its
//! truth table.

use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_test_support::builders::CompiledModuleBuilder;
use reify_test_support::{
    make_engine, manufacturing_purpose, my_design_template, step_input_template,
    step_output_template,
};

/// Pinned by the imported-geometry-promise contract: after `eval()`, the
/// `STEPInput` template's `param provenance : Provenance` declaration
/// (default `provenance.tolerance_guarantee = X m`) surfaces as a
/// value-cell entry under `(STEPInput, "provenance")` whose
/// `Value::StructureInstance.fields["tolerance_guarantee"]` is the promise,
/// and `Engine::imported_tolerance_promise("STEPInput")` returns
/// `Some(promise_tol_si)`.
#[test]
fn engine_imported_tolerance_promise_returns_si_value_after_eval() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_imported_tolerance_promise_extracted".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);

    assert_eq!(
        engine.imported_tolerance_promise("STEPInput"),
        Some(50e-6),
        "STEPInput's `param provenance : Provenance` declaration \
         (default provenance.tolerance_guarantee = 50um) must surface in the \
         post-eval snapshot.values map under (STEPInput, \"provenance\") as a \
         StructureInstance whose fields[\"tolerance_guarantee\"] is Some(50e-6)"
    );
}

/// Pinned by the warning-emission contract from PRD
/// `docs/prds/v0_2/per-purpose-tolerance.md`: when a downstream demand is
/// strictly tighter than the imported-geometry tolerance promise, the
/// runtime emits a `Severity::Warning` carrying
/// `DiagnosticCode::ImportedTolerancePromiseInsufficient` and the
/// as-imported realization proceeds.
///
/// Setup: STEPInput promise=50µm (loose), STEPOutput body=1µm (tight),
/// manufacturing purpose=1µm (also tight). After `activate_purpose`, the
/// demanded tolerance for STEPOutput is `min(1µm, 1µm) = 1µm` (via
/// `combine_demanded_tolerance`'s min-fold), which is strictly tighter than
/// the 50µm promise — so the warning fires.
#[test]
fn engine_check_imported_tolerance_promise_emits_warning_when_demand_strictly_tighter_than_promise()
{
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_promise_insufficient".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(1e-6))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let diag = engine
        .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
        .expect(
            "with promise=50e-6 and demand=1e-6 (strict tighter), the check must \
             return Some(diagnostic) — not None",
        );

    assert_eq!(
        diag.severity,
        Severity::Warning,
        "diagnostic severity must be Warning (PRD: warn, not error — \
         runtime proceeds with as-imported realization)"
    );
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::ImportedTolerancePromiseInsufficient),
        "diagnostic code must round-trip ImportedTolerancePromiseInsufficient \
         for filter-by-code downstream consumers"
    );
    assert!(
        diag.message.contains("STEPInput"),
        "message must name the input template so authors can locate the \
         import site (got: {:?})",
        diag.message
    );
}

/// Integration pin for the zero-promise lint introduced by task 2833
/// (option-(b continuation)): when the imported-geometry promise is exactly
/// `0.0` AND the downstream demand is strictly positive, the engine must emit
/// `DiagnosticCode::InputTolerancePromiseIsZero` (NOT
/// `ImportedTolerancePromiseInsufficient` — the strict-`<` branch never fires
/// when `promise == 0.0` because `demanded < 0.0` is false for all
/// `demanded >= 0.0`).
///
/// Setup mirrors `engine_check_imported_tolerance_promise_emits_warning_when_demand_strictly_tighter_than_promise`
/// with `step_input_template(0.0)` (zero-promise variant) instead of
/// `step_input_template(50e-6)`. Demand path is identical:
/// STEPOutput(1µm) + manufacturing(1µm) → `min(1µm, 1µm) = 1µm > 0.0`, so
/// the zero-promise guard fires.
#[test]
fn engine_check_imported_tolerance_promise_emits_zero_promise_lint_when_promise_zero_and_demand_positive()
 {
    let module =
        CompiledModuleBuilder::new(ModulePath::new(vec!["test_zero_promise_lint".to_string()]))
            .template(step_input_template(0.0))
            .template(step_output_template(1e-6))
            .template(my_design_template())
            .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
            .build();

    let mut engine = make_engine();
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let diag = engine
        .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
        .expect(
            "with promise=0.0 and demand=1µm (positive), the check must \
             return Some(diagnostic) — the zero-promise lint must fire",
        );

    assert_eq!(
        diag.severity,
        Severity::Warning,
        "diagnostic severity must be Warning (PRD: warn, not error)"
    );
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::InputTolerancePromiseIsZero),
        "code must be InputTolerancePromiseIsZero, NOT \
         ImportedTolerancePromiseInsufficient — proves the new branch \
         fires before the strict-`<` branch (which cannot fire when \
         promise==0.0 anyway)"
    );
    assert!(
        diag.message.contains("STEPInput"),
        "message must name the input template (got: {:?})",
        diag.message
    );
}

/// Degenerate guard: when both the imported-geometry promise AND the downstream
/// demand are exactly `0.0`, neither the zero-promise lint
/// ([`DiagnosticCode::InputTolerancePromiseIsZero`]) nor the insufficient-promise
/// lint ([`DiagnosticCode::ImportedTolerancePromiseInsufficient`]) should fire.
///
/// The zero-promise lint is guarded by `demanded > 0.0` (strict, not `>= 0.0`) —
/// when both are zero there is no real mismatch and the canonical
/// `(0.0, 0.0) -> false` truth-table row in `is_promise_insufficient` rules this
/// sufficient. Loosening the guard to `>= 0.0` would emit the lint in the
/// degenerate (0, 0) case and contradict the locked truth-table row.
///
/// Task 2833 decision: the `demanded > 0.0` strict guard is intentional and must
/// not be widened to `>= 0.0`.
#[test]
fn engine_check_imported_tolerance_promise_returns_none_when_promise_and_demand_both_zero() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_zero_promise_zero_demand".to_string(),
    ]))
    .template(step_input_template(0.0))
    .template(step_output_template(0.0))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 0.0))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    assert!(
        engine
            .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
            .is_none(),
        "with promise=0.0 and demand=0.0 (both zero), neither lint must fire — \
         the zero-promise lint guard is `demanded > 0.0` (strict), not `>= 0.0`, \
         and `is_promise_insufficient(0.0, 0.0)` is false by the strict-`<` rule. \
         Loosening the guard to `>= 0.0` would regress this assertion."
    );
}

/// Fills the `(positive_promise, demand=0)` cell of
/// `Engine::check_imported_tolerance_promise`'s dispatch matrix — the
/// symmetric mirror of the `(zero_promise, positive_demand)` row pinned by
/// `engine_check_imported_tolerance_promise_emits_zero_promise_lint_when_promise_zero_and_demand_positive`.
///
/// Confirms that the zero-promise check at `Engine::check_imported_tolerance_promise in engine_tolerance.rs`
/// (introduced by task 2833) does **NOT** intercept the `(positive, 0.0)`
/// row: the guard `promise == 0.0 && demanded > 0.0` skips because
/// `promise != 0.0`. The strict-`<` branch then fires because
/// `is_promise_insufficient(0.0, 50e-6) == true` — zero is the tightest
/// possible demand (canonical truth-table row `(demand=0.0, promise=1µm) →
/// true`, case (d) of
/// `is_promise_insufficient_returns_true_iff_demanded_strictly_less_than_promise`).
///
/// A regression that swapped the operands of the zero-promise guard
/// (e.g., `demanded == 0.0 && promise > 0.0`) would silently steal this
/// case from the strict-`<` branch, changing `code` from
/// `ImportedTolerancePromiseInsufficient` to `InputTolerancePromiseIsZero`
/// and breaking the dispatch-matrix truth table without a compile error.
/// (Note: widening `&&` to `||` does NOT steal this case for this fixture —
/// `promise == 0.0 || demanded > 0.0` evaluates to `false || false = false`
/// since promise=50µm ≠ 0 and demand=0 is not > 0; the guard still skips.)
///
/// Setup mirrors
/// `engine_check_imported_tolerance_promise_returns_none_when_promise_and_demand_both_zero`
/// with `step_input_template(50e-6)` instead of `step_input_template(0.0)`:
/// STEPOutput(0.0) + manufacturing(0.0) fold to `min(0.0, 0.0) = 0.0` via
/// `combine_demanded_tolerance`, so demanded = 0.0.
#[test]
fn engine_check_imported_tolerance_promise_emits_insufficient_lint_when_promise_positive_and_demand_zero()
 {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_positive_promise_zero_demand".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(0.0))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 0.0))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let diag = engine
        .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
        .expect(
            "promise=50µm, demand=0 — strict-< branch must fire: \
             is_promise_insufficient(0.0, 50e-6) == true, so Some(diagnostic) \
             is required; None would mean the (positive_promise, demand=0) \
             dispatch-matrix cell is broken",
        );

    assert_eq!(
        diag.severity,
        Severity::Warning,
        "diagnostic severity must be Warning (PRD: warn, not error — \
         runtime proceeds with as-imported realization)"
    );
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::ImportedTolerancePromiseInsufficient),
        "code must be ImportedTolerancePromiseInsufficient, NOT \
         InputTolerancePromiseIsZero — proves the zero-promise branch \
         (promise == 0.0 && demanded > 0.0) correctly skipped because \
         promise=50µm != 0.0; a guard with swapped operands \
         (demanded == 0.0 && promise > 0.0) would produce \
         InputTolerancePromiseIsZero here"
    );
    assert!(
        diag.message.contains("STEPInput"),
        "message must name the input template for author-locatability \
         (got: {:?})",
        diag.message
    );
}

/// Pinned by the no-op rows of `check_imported_tolerance_promise`'s truth
/// table. Mirrors the four-block precedent
/// `engine_demanded_tolerance_for_output_handles_partial_inputs` in
/// `tests/tolerance_combine.rs`. Each scoped sub-block exercises
/// a distinct path that must return `None`:
///
/// - (a) Promise absent (no STEPInput template) — silent-skip on the
///   promise-side `?` early-return.
/// - (b) Demand absent (no STEPOutput template, no purpose) — silent-skip
///   on the demand-side `?` early-return.
/// - (c) Demand looser than promise — `is_promise_insufficient` returns
///   false, so the diagnostic does not fire.
/// - (d) Demand equal to promise — strict `<` is false, so the diagnostic
///   does not fire (this branch pins the strict-vs-non-strict design
///   decision; flipping `<` to `<=` would regress this assertion).
#[test]
fn engine_check_imported_tolerance_promise_returns_none_in_no_op_cases() {
    // (a) No Input template — module has only MyDesign, no STEPInput.
    //     The promise contributor is None, so the `?` short-circuits to None.
    {
        let module =
            CompiledModuleBuilder::new(ModulePath::new(vec!["test_no_input_template".to_string()]))
                .template(my_design_template())
                .build();
        let mut engine = make_engine();
        engine.eval(&module);
        assert!(
            engine
                .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
                .is_none(),
            "(a) no STEPInput template ⇒ promise contributor is None ⇒ check \
             must return None (no diagnostic to emit)"
        );
    }

    // (b) No demand — module has STEPInput(50e-6) and MyDesign but no
    //     STEPOutput template and no active purpose. Promise contributor is
    //     Some(50e-6), but the demand contributor is None, so the second `?`
    //     short-circuits to None.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec![
            "test_no_demand_contributor".to_string(),
        ]))
        .template(step_input_template(50e-6))
        .template(my_design_template())
        .build();
        let mut engine = make_engine();
        engine.eval(&module);
        // No `activate_purpose` call — demand-side contributes None.
        assert!(
            engine
                .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
                .is_none(),
            "(b) no demand contributor (no STEPOutput template + no active \
             purpose) ⇒ check must return None even though promise is \
             Some(50e-6)"
        );
    }

    // (c) Demand looser than promise — STEPInput(1e-6 promise) +
    //     STEPOutput(50e-6 output bound) + MyDesign + manufacturing(50e-6).
    //     After activation, demand = min(50e-6, 50e-6) = 50e-6, which is
    //     LOOSER than the 1e-6 promise. The promise's upper-bound guarantee
    //     covers the looser demand → no diagnostic.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec![
            "test_demand_looser_than_promise".to_string(),
        ]))
        .template(step_input_template(1e-6))
        .template(step_output_template(50e-6))
        .template(my_design_template())
        .compiled_purpose(manufacturing_purpose("manufacturing", 50e-6))
        .build();
        let mut engine = make_engine();
        engine.eval(&module);
        engine.activate_purpose("manufacturing", "MyDesign");
        assert!(
            engine
                .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
                .is_none(),
            "(c) demand 50µm looser than promise 1µm ⇒ promise covers it ⇒ \
             check must return None (no diagnostic)"
        );
    }

    // (d) Demand equal to promise — STEPInput(10e-6) + STEPOutput(10e-6) +
    //     MyDesign + manufacturing(10e-6). After activation, demand =
    //     min(10e-6, 10e-6) = 10e-6, which is EQUAL to the 10e-6 promise.
    //     Strict `<` is false → no diagnostic. This is the canonical
    //     strict-vs-non-strict design-decision pin: flipping the comparator
    //     from `<` to `<=` would regress this assertion.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec![
            "test_demand_equal_to_promise".to_string(),
        ]))
        .template(step_input_template(10e-6))
        .template(step_output_template(10e-6))
        .template(my_design_template())
        .compiled_purpose(manufacturing_purpose("manufacturing", 10e-6))
        .build();
        let mut engine = make_engine();
        engine.eval(&module);
        engine.activate_purpose("manufacturing", "MyDesign");
        assert!(
            engine
                .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
                .is_none(),
            "(d) demand 10µm == promise 10µm ⇒ strict `<` rules this \
             sufficient ⇒ check must return None; flipping `<` to `<=` \
             would regress this assertion"
        );
    }
}

/// Leaf-observable regression pin: the promise value reflects the
/// `provenance.tolerance_guarantee` field supplied to `step_input_template`,
/// not any stdlib default. Uses 0.01mm (1e-5 m) — deliberately distinct from
/// the 1µm (1e-6 m) stdlib `STEPInput` default — to prove the fixture value
/// is what surfaces, not some cached or fallback value.
///
/// Also pins the diagnostic path: pairing a 10µm promise with a 1µm demand
/// (STEPOutput + manufacturing purpose both at 1e-6) fires the
/// `ImportedTolerancePromiseInsufficient` warning, confirming the promise
/// read via `provenance.tolerance_guarantee` flows into the full
/// `check_imported_tolerance_promise` pipeline.
#[test]
fn engine_imported_tolerance_promise_reads_provenance_tolerance_guarantee_not_default() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_provenance_tolerance_guarantee_leaf".to_string(),
    ]))
    .template(step_input_template(1e-5))
    .template(step_output_template(1e-6))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);

    assert_eq!(
        engine.imported_tolerance_promise("STEPInput"),
        Some(1e-5),
        "imported_tolerance_promise must return the provenance.tolerance_guarantee \
         value (0.01mm = 1e-5 m) supplied to step_input_template, not a default"
    );

    engine.activate_purpose("manufacturing", "MyDesign");

    let diag = engine
        .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
        .expect(
            "demand=1µm is strictly tighter than promise=10µm (provenance-sourced) \
             — ImportedTolerancePromiseInsufficient must fire",
        );

    assert_eq!(
        diag.code,
        Some(DiagnosticCode::ImportedTolerancePromiseInsufficient),
        "diagnostic code must be ImportedTolerancePromiseInsufficient — proves \
         the provenance-sourced 10µm promise flows into check_imported_tolerance_promise"
    );
    assert_eq!(
        diag.severity,
        Severity::Warning,
        "diagnostic severity must be Warning"
    );
    assert!(
        diag.message.contains("STEPInput"),
        "message must name the input template (got: {:?})",
        diag.message
    );
}

/// E2e gap-closer: the real stdlib `STEPInput` occurrence (io.ri:169-173)
/// declares `provenance: Provenance(... tolerance_guarantee: 0.001mm)`, which
/// is compiled via a struct-constructor expression — NOT the literal
/// `StructureInstance` that `step_input_template` fixture uses. This test
/// closes the fixture-vs-real-eval gap by:
///
/// 1. Loading the compiled stdlib modules (no re-parse needed — they are
///    built once at compile time by `stdlib_loader`).
/// 2. Finding the module that contains the real `STEPInput` template.
/// 3. Evaling that module with a fresh engine.
/// 4. Asserting `imported_tolerance_promise("STEPInput") == Some(1e-6)` —
///    proving that the struct-constructor eval path (GR-001) materialises a
///    `Value::StructureInstance` in the `provenance` cell with the correct
///    `tolerance_guarantee`, matching the recognition shape that
///    `extract_input_tolerance_promise` expects.
///
/// If GR-001 were to regress and the struct-constructor stopped producing a
/// `Value::StructureInstance`, this test would return `None` and fail, while
/// the fixture-based tests would remain green — making the gap visible.
///
/// `0.001mm = 1e-6 m` (io.ri:171).
#[test]
fn stdlib_step_input_provenance_tolerance_guarantee_matches_io_ri_default() {
    let stdlib_modules = reify_compiler::stdlib_loader::load_stdlib();
    let io_module = stdlib_modules
        .iter()
        .find(|m| m.templates.iter().any(|t| t.name == "STEPInput"))
        .expect(
            "stdlib must contain a module with a STEPInput template (io.ri); \
             if STEPInput was renamed or moved, update this test accordingly",
        );

    let mut engine = make_engine();
    engine.eval(io_module);

    assert_eq!(
        engine.imported_tolerance_promise("STEPInput"),
        Some(1e-6),
        "stdlib STEPInput (io.ri:171) defaults tolerance_guarantee to \
         0.001mm = 1e-6 m via a Provenance struct-constructor call; \
         imported_tolerance_promise must return Some(1e-6) after eval, \
         proving the struct-constructor path (GR-001) materialises a \
         Value::StructureInstance in the provenance cell that the \
         six-gate extractor can read"
    );
}
