//! Tests for the W_TRAIT_USER_ASSERTED specialization escape hatch warning.
//!
//! When a structure (or occurrence) explicitly declares one of the seven stdlib
//! geometry-conformance marker traits as a trait bound, the compiler emits a
//! `W_TRAIT_USER_ASSERTED` warning (PRD `docs/prds/geometry-traits.md` task 6,
//! `DiagnosticCode::TraitUserAsserted`). The declaration is treated as a user
//! assertion that bypasses any future runtime conformance check.
//!
//! Test coverage:
//! - Basic emission: single geometry marker bound emits one warning.
//! - Non-geometry trait: no warning emitted.
//! (Additional multi-bound, mixed, and parametric coverage in step 7.)

use reify_test_support::{compile_source_with_stdlib, warnings_only};
use reify_types::{DiagnosticCode, Severity};

/// A structure with a single Watertight bound must emit exactly one
/// `W_TRAIT_USER_ASSERTED` warning with the correct code, severity, message
/// content, and a non-empty label span.
#[test]
fn single_watertight_marker_emits_one_warning_with_correct_code_and_label() {
    let source = r#"
structure def Foo : Watertight {
    param x : Real = 1.0
}
"#;
    let module = compile_source_with_stdlib(source);
    let warnings = warnings_only(&module);
    let asserted: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitUserAsserted))
        .collect();

    assert_eq!(
        asserted.len(),
        1,
        "expected exactly 1 W_TRAIT_USER_ASSERTED warning, got {}: {:#?}",
        asserted.len(),
        asserted
    );

    let w = asserted[0];
    assert_eq!(w.severity, Severity::Warning, "expected Warning severity");

    assert!(
        w.message.contains("Foo"),
        "message should name the entity 'Foo', got: {:?}",
        w.message
    );
    assert!(
        w.message.contains("Watertight"),
        "message should name the trait 'Watertight', got: {:?}",
        w.message
    );

    assert_eq!(
        w.labels.len(),
        1,
        "expected exactly 1 label (the bound span), got: {:#?}",
        w.labels
    );
    let label = &w.labels[0];
    assert!(
        !label.span.is_empty(),
        "label span must be non-empty (should point at the trait bound), got: {:?}",
        label.span
    );
}

/// A structure with only a non-geometry trait bound must emit zero
/// `W_TRAIT_USER_ASSERTED` warnings.  `Elastic` is a stdlib mechanical
/// material trait — not one of the seven geometry markers.
#[test]
fn non_geometry_trait_emits_no_user_asserted_warning() {
    let source = r#"
structure def MyMaterial : Elastic {
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
}
"#;
    let module = compile_source_with_stdlib(source);
    let warnings = warnings_only(&module);
    let asserted: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitUserAsserted))
        .collect();

    assert_eq!(
        asserted.len(),
        0,
        "expected 0 W_TRAIT_USER_ASSERTED warnings for non-geometry trait, got {}: {:#?}",
        asserted.len(),
        asserted
    );
}
