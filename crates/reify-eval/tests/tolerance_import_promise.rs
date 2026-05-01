//! Engine-level integration tests for the imported-geometry tolerance
//! promise contract (per task 2651 / PRD
//! `docs/prds/v0_2/per-purpose-tolerance.md` "Resolved design decisions" →
//! "Imported geometry promise"; arch §10.4 / §14.5).
//!
//! Builds a hand-crafted `STEPInput` template carrying a
//! `param tolerance : Length = X m` declaration whose post-`eval()`
//! value-cell entry is the imported-geometry tolerance promise. Asserts the
//! promise is observable via `Engine::imported_tolerance_promise`, then
//! pairs it with the existing demand-side fixture pattern (manufacturing
//! purpose + STEPOutput template + MyDesign subject) to exercise
//! `Engine::check_imported_tolerance_promise`'s strict-tighter-than-promise
//! warning emission and the four no-op rows of its truth table.

use reify_test_support::builders::CompiledModuleBuilder;
use reify_test_support::{
    make_engine, manufacturing_purpose, my_design_template, step_input_template,
    step_output_template,
};
use reify_types::{DiagnosticCode, ModulePath, Severity};

/// Pinned by the imported-geometry-promise contract: after `eval()`, the
/// `STEPInput` template's `param tolerance : Length = X m` declaration
/// surfaces as a value-cell entry under `(STEPInput, "tolerance")`, and
/// `Engine::imported_tolerance_promise("STEPInput")` returns
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
        "STEPInput's `param tolerance : Length = 50um` default expression \
         must surface in the post-eval snapshot.values map under \
         (STEPInput, \"tolerance\") and be returned as Some(50e-6) by the \
         engine query"
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

/// Pinned by the no-op rows of `check_imported_tolerance_promise`'s truth
/// table. Mirrors the four-block precedent
/// `engine_demanded_tolerance_for_output_handles_partial_inputs` in
/// `tests/tolerance_combine.rs:115-215`. Each scoped sub-block exercises
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
        let module = CompiledModuleBuilder::new(ModulePath::new(vec![
            "test_no_input_template".to_string(),
        ]))
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
