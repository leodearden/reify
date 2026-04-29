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
//! - Multi-bound: each geometry marker bound emits its own warning with a distinct span.
//! - Mixed bounds: only geometry marker bounds trip the lint.
//! - Parametric: every stdlib geometry marker name triggers exactly one warning.

use reify_test_support::{compile_source_with_stdlib, errors_only, warnings_only, EXPECTED_GEOMETRY_TRAITS};
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
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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

/// An *occurrence* with a single Watertight bound must emit exactly one
/// `W_TRAIT_USER_ASSERTED` warning.  The lint fires for both `structure def` and
/// `occurrence def` because both reach `compile_entity` via `EntityDefRef`, which
/// shares the same trait_bound iteration loop.  A future refactor that moves
/// warning emission into a structure-only branch would silently regress this test.
#[test]
fn occurrence_with_geometry_marker_bound_emits_one_user_asserted_warning() {
    let source = r#"
occurrence def Joint : Watertight {
    param x : Real = 1.0
}
"#;
    let module = compile_source_with_stdlib(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let warnings = warnings_only(&module);
    let asserted: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitUserAsserted))
        .collect();

    assert_eq!(
        asserted.len(),
        1,
        "expected exactly 1 W_TRAIT_USER_ASSERTED warning for occurrence def, got {}: {:#?}",
        asserted.len(),
        asserted
    );

    let w = asserted[0];
    assert_eq!(w.severity, Severity::Warning, "expected Warning severity");
    assert!(
        w.message.contains("Joint"),
        "message should name the entity 'Joint', got: {:?}",
        w.message
    );
    assert!(
        w.message.contains("Watertight"),
        "message should name the trait 'Watertight', got: {:?}",
        w.message
    );
}

/// A structure with two geometry marker bounds (`Closed + Manifold`) must emit
/// exactly 2 `W_TRAIT_USER_ASSERTED` warnings, one per marker, with distinct
/// label spans (each pinned to its own trait bound, not to the entity span).
#[test]
fn multi_geometry_marker_bound_emits_one_warning_per_marker_with_distinct_label_spans() {
    let source = r#"
structure def Shell : Closed + Manifold {
    param x : Real = 1.0
}
"#;
    let module = compile_source_with_stdlib(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let warnings = warnings_only(&module);
    let asserted: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitUserAsserted))
        .collect();

    assert_eq!(
        asserted.len(),
        2,
        "expected exactly 2 W_TRAIT_USER_ASSERTED warnings (one per bound), got {}: {:#?}",
        asserted.len(),
        asserted
    );

    // Confirm both trait names appear across the two warnings.
    assert!(
        asserted.iter().any(|d| d.message.contains("Closed")),
        "expected one warning to name 'Closed'"
    );
    assert!(
        asserted.iter().any(|d| d.message.contains("Manifold")),
        "expected one warning to name 'Manifold'"
    );

    // Each warning must have exactly one label with a non-empty span.
    for w in &asserted {
        assert_eq!(w.labels.len(), 1, "each warning must have 1 label, got: {:#?}", w.labels);
        assert!(
            !w.labels[0].span.is_empty(),
            "label span must be non-empty, got: {:?}",
            w.labels[0].span
        );
    }

    // The two label spans must be distinct (each bound has its own source position).
    assert_ne!(
        asserted[0].labels[0].span,
        asserted[1].labels[0].span,
        "Closed and Manifold bounds must have distinct label spans"
    );
}

/// A structure with mixed geometry and non-geometry bounds (`Watertight + Elastic`)
/// must emit exactly 1 `W_TRAIT_USER_ASSERTED` warning — only for `Watertight`.
/// `Elastic` is a stdlib mechanical material trait and must not trip the lint.
#[test]
fn mixed_geometry_and_non_geometry_bounds_emit_one_warning_for_geometry_only() {
    let source = r#"
structure def Hybrid : Watertight + Elastic {
    param x : Real = 1.0
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
}
"#;
    let module = compile_source_with_stdlib(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let warnings = warnings_only(&module);
    let asserted: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitUserAsserted))
        .collect();

    assert_eq!(
        asserted.len(),
        1,
        "expected exactly 1 W_TRAIT_USER_ASSERTED warning (for Watertight only), got {}: {:#?}",
        asserted.len(),
        asserted
    );

    assert!(
        asserted[0].message.contains("Watertight"),
        "the single warning must name 'Watertight', got: {:?}",
        asserted[0].message
    );
}

/// Parametric coverage: for every name in `EXPECTED_GEOMETRY_TRAITS`, a
/// structure with that single trait bound must emit exactly one
/// `W_TRAIT_USER_ASSERTED` warning whose message names the trait.
///
/// This pins the helper's full alphabet against drift — if `GEOMETRY_MARKER_TRAITS`
/// in `crates/reify-compiler/src/geometry_traits.rs` diverges from `EXPECTED_GEOMETRY_TRAITS` in the test fixture,
/// one of these sub-tests will fail.
#[test]
fn every_stdlib_geometry_marker_emits_one_user_asserted_warning_when_declared_explicitly() {
    for trait_name in EXPECTED_GEOMETRY_TRAITS {
        let source = format!(
            r#"
structure def Foo_{trait_name} : {trait_name} {{
    param x : Real = 1.0
}}
"#
        );
        let module = compile_source_with_stdlib(&source);
        assert!(
            errors_only(&module).is_empty(),
            "trait '{trait_name}': expected no compile errors, got: {:#?}",
            errors_only(&module)
        );
        let warnings = warnings_only(&module);
        let asserted: Vec<_> = warnings
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitUserAsserted))
            .collect();

        assert_eq!(
            asserted.len(),
            1,
            "trait '{trait_name}': expected exactly 1 W_TRAIT_USER_ASSERTED warning, got {}: {:#?}",
            asserted.len(),
            asserted
        );

        assert!(
            asserted[0].message.contains(trait_name),
            "trait '{trait_name}': warning message should name the trait, got: {:?}",
            asserted[0].message
        );

        assert_eq!(
            asserted[0].severity,
            Severity::Warning,
            "trait '{trait_name}': expected Warning severity"
        );
    }
}
