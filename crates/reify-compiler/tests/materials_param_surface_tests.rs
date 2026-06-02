//! Tests for PRD §6 parameter-surface additions to stdlib/materials_mechanical.ri.
//!
//! Split by mechanism:
//! - `compile_source_with_stdlib` → conformance (no Severity::Error) for
//!   TemperatureDependent (§6.1) and optional-param omissions (§6.2).
//! - `check_source_with_stdlib`   → constraint Satisfied/Violated for the
//!   Elastic poissons_ratio constraint (§6.2).

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

// ── §6.1 TemperatureDependent — conformance (compile-time) ───────────────────

/// Conforming structure that omits reference_temperature.
/// The trait provides a default (293.15K) so the param is optional.
/// Expects: compilation with no Severity::Error diagnostics.
///
/// RED: TemperatureDependent does not yet exist → unresolved-trait error.
#[test]
fn temperature_dependent_omits_reference_temperature_is_clean() {
    let src = r#"
        structure def RoomTemp : TemperatureDependent {
            let marker = 1
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when omitting reference_temperature \
         (should default to 293.15K), got: {:?}",
        errors
    );
}

/// Conforming structure that explicitly supplies reference_temperature = 350.0K.
/// Expects: compilation with no Severity::Error diagnostics.
///
/// RED: TemperatureDependent does not yet exist → unresolved-trait error.
#[test]
fn temperature_dependent_supplies_350k_is_clean() {
    let src = r#"
        structure def HotEnv : TemperatureDependent {
            param reference_temperature : Temperature = 350.0K
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when supplying reference_temperature = 350.0K, \
         got: {:?}",
        errors
    );
}
