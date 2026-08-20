//! Tests for §6.3/§6.5 `= undef` optionality on thermal and optical material traits.
//!
//! This file contains RED→GREEN tests for task #4241 (γ).
//!
//! # Thermal tests (§6.3) — added in step-1/step-2
//! - `thermal_omit_optional_params_compiles_cleanly`: A conformer that supplies only the
//!   three required thermal params (plus MaterialSpec parents) and omits the three optional
//!   ones (melting_point, max_service_temperature, glass_transition) must compile with no
//!   Error-severity diagnostics.
//! - `refractory_omit_max_service_temperature_is_indeterminate`: A Refractory conformer
//!   that omits max_service_temperature must compile cleanly and the inherited
//!   `max_service_temperature >= 1500.0` constraint must evaluate to Indeterminate (not
//!   Violated), reflecting Kleene three-valued logic on undef inputs.
//!
//! RED state: both thermal tests fail today because the params are still required members.
//! GREEN state (after step-2): the `.ri` file carries `= undef` defaults so the params
//! are optional, and these tests pass.
//!
//! # Optical tests (§6.5) — appended in step-3/step-4
//! (See bottom of file.)

use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::{check_source_with_stdlib, compile_source_with_stdlib};

// ─── §6.3 thermal: omit optional params ──────────────────────────────────────

/// A `structure def` conforming to ThermallyCharacterized may omit the three
/// optional temperature params (melting_point, max_service_temperature,
/// glass_transition) when they carry `= undef` defaults, as long as the three
/// required dimensioned params and the parent MaterialSpec params are supplied.
///
/// RED: omitting these params yields "missing required member" diagnostics until
/// the `= undef` defaults are applied in step-2.
#[test]
fn thermal_omit_optional_params_compiles_cleanly() {
    let source = r#"
structure def ThermalOmit : ThermallyCharacterized {
    param density : Density = 3900kg/m^3
    param name : String = "alumina_partial"
    param thermal_conductivity : ThermalConductivity = 30.0 * 1W / (1m * 1K)
    param specific_heat : SpecificHeat = 880.0 * 1J / (1kg * 1K)
    param thermal_expansion : ThermalExpansion = undef
}
"#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "ThermalOmit omitting optional thermal params should compile with no errors, \
         got: {:?}",
        errors
    );
}

// ─── §6.3 refractory: omit max_service_temperature → Indeterminate ───────────

/// A top-level Refractory conformer that omits max_service_temperature (an
/// optional param after step-2) should compile cleanly and the inherited
/// `max_service_temperature >= 1500.0` constraint must evaluate to
/// Indeterminate (Kleene three-valued logic: undef input → Indeterminate,
/// not Violated). Zero Violated constraint results.
///
/// GREEN: the three temperature params carry `= undef` defaults (step-2) and
/// Temperature type (task #3112). The constraint `max_service_temperature >= 1500.0K`
/// evaluates to Indeterminate when max_service_temperature is undef (Kleene logic).
///
/// Uses a top-level structure INSTANCE (no `def` keyword) so Engine::check
/// evaluates the inherited constraint. Precedent: stdlib_prelude_tests.rs and
/// constraint_def_eval.rs tests using the same pattern.
#[test]
fn refractory_omit_max_service_temperature_is_indeterminate() {
    let source = r#"
structure RefractoryOmit : Refractory {
    param density : Density = 3900kg/m^3
    param name : String = "refractory_partial"
    param thermal_conductivity : ThermalConductivity = 30.0 * 1W / (1m * 1K)
    param specific_heat : SpecificHeat = 880.0 * 1J / (1kg * 1K)
    param thermal_expansion : ThermalExpansion = undef
}
"#;

    let check_result = check_source_with_stdlib(source);

    // No Error-severity check diagnostics.
    let check_errors: Vec<_> = check_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        check_errors.is_empty(),
        "RefractoryOmit check should produce no error diagnostics, got: {:?}",
        check_errors
    );

    // Locate all constraint results for the RefractoryOmit entity specifically.
    // RefractoryOmit inherits exactly one constraint from Refractory:
    //   `max_service_temperature >= 1500.0K`
    // Asserting count == 1 pins the test to that specific constraint — if additional
    // inherited constraints are added later the count check will flag the mismatch and
    // prompt a more specific assertion, rather than the test silently passing for the
    // wrong constraint.
    let refractory_omit_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|cr| cr.id.entity == "RefractoryOmit")
        .collect();
    assert_eq!(
        refractory_omit_constraints.len(),
        1,
        "Expected exactly 1 constraint result for RefractoryOmit \
         (the inherited max_service_temperature >= 1500.0), got: {:?}",
        refractory_omit_constraints
    );
    assert_eq!(
        refractory_omit_constraints[0].satisfaction,
        Satisfaction::Indeterminate,
        "RefractoryOmit max_service_temperature >= 1500.0K should be Indeterminate \
         (max_service_temperature omitted → undef input via Kleene), got: {:?}",
        refractory_omit_constraints[0].satisfaction
    );

    // Zero Violated — omitting an optional param must never trigger a constraint violation.
    let violated: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|cr| cr.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "RefractoryOmit should have zero Violated constraint results, got: {:?}",
        violated
    );
}

// ─── §6.5 optical: omit optional params ──────────────────────────────────────

/// A `structure def` conforming to OpticallyCharacterized may omit the three
/// optional optical params (absorption_coefficient, transmittance,
/// reference_thickness) when they carry `= undef` defaults, as long as the
/// required refractive_index and the parent MaterialSpec params are supplied.
///
/// RED: omitting these params yields "missing required member" diagnostics until
/// the `= undef` defaults are applied in step-4.
#[test]
fn optical_omit_optional_params_compiles_cleanly() {
    let source = r#"
structure def OpticalOmit : OpticallyCharacterized {
    param density : Density = 2500kg/m^3
    param name : String = "glass_partial"
    param refractive_index : Real = 1.52
}
"#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "OpticalOmit omitting optional optical params should compile with no errors, \
         got: {:?}",
        errors
    );
}
