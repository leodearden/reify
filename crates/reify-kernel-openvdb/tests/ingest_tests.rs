//! Integration tests for the OpenVDB v0.2 ingestion pipeline.
//!
//! Pins the public API surface of `reify_kernel_openvdb::ingest`:
//!   - `OpenVdbGridSource` / `OpenVdbInterpolation` / `OpenVdbGridKind`
//!   - `IngestError` / `IngestOutcome`
//!   - `lower_to_sampled` / `read_vdb_file` / `validate_grid_units` / `KNOWN_UNITS`
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/imported-field-source.md` "Decomposition plan" task 2 —
//! OpenVDB ingestion (file read, sample buffer, lowering to `sampled`).

use reify_kernel_openvdb::ingest::{
    IngestError, IngestOutcome, OpenVdbGridKind, OpenVdbGridSource, OpenVdbInterpolation,
    lower_to_sampled,
};
use reify_types::{DiagnosticCode, DimensionVector, InterpolationKind, SampledGridKind, Severity, Type};

/// Step-1 happy path: a 1D `Length` grid lowered with linear interpolation
/// produces a `SampledField` whose semantic content (kind, bounds, spacing,
/// axis grids, interpolation, data, name) matches the source exactly, with
/// no warnings emitted.
#[test]
fn lower_to_sampled_1d_length_linear_grid_produces_sampled_field() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };

    let outcome: IngestOutcome =
        lower_to_sampled(&grid, "test_field", &Type::length()).expect("lowering must succeed");

    let field = &outcome.field;
    assert_eq!(field.name, "test_field");
    assert_eq!(field.kind, SampledGridKind::Regular1D);
    assert_eq!(field.bounds_min, vec![0.0]);
    assert_eq!(field.bounds_max, vec![3.0]);
    assert_eq!(field.spacing, vec![1.0]);
    assert_eq!(field.axis_grids, vec![vec![0.0, 1.0, 2.0, 3.0]]);
    assert_eq!(field.interpolation, InterpolationKind::Linear);
    assert_eq!(field.data, vec![0.0, 1.0, 2.0, 3.0]);
    assert!(
        outcome.warnings.is_empty(),
        "linear interpolation must not emit deferred warnings"
    );
}

/// Step-3 happy path: a 2D grid (4×3 nodes) lowers to a `Regular2D`
/// `SampledField` with two axis grids and 12 flat data elements.
#[test]
fn lower_to_sampled_2d_grid_produces_regular2d_field() {
    // 4×3 grid: bounds_min=[0,0], bounds_max=[3,2], spacing=[1,1] →
    // axis 0 has 4 nodes (0,1,2,3); axis 1 has 3 nodes (0,1,2); 12 cells.
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular2D,
        bounds_min: vec![0.0, 0.0],
        bounds_max: vec![3.0, 2.0],
        spacing: vec![1.0, 1.0],
        data: vec![
            0.0, 0.1, 0.2, // row 0 (axis-0 = 0)
            1.0, 1.1, 1.2, // row 1 (axis-0 = 1)
            2.0, 2.1, 2.2, // row 2 (axis-0 = 2)
            3.0, 3.1, 3.2, // row 3 (axis-0 = 3)
        ],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };

    let outcome = lower_to_sampled(&grid, "two_d_field", &Type::length()).unwrap();

    assert_eq!(outcome.field.kind, SampledGridKind::Regular2D);
    assert_eq!(outcome.field.bounds_min.len(), 2);
    assert_eq!(outcome.field.bounds_max.len(), 2);
    assert_eq!(outcome.field.spacing.len(), 2);
    assert_eq!(outcome.field.axis_grids.len(), 2);
    assert_eq!(outcome.field.axis_grids[0].len(), 4);
    assert_eq!(outcome.field.axis_grids[1].len(), 3);
    assert_eq!(outcome.field.data.len(), 12);
    assert_eq!(outcome.field.interpolation, InterpolationKind::Linear);
    assert!(outcome.warnings.is_empty());
}

/// Step-3 happy path: a 3D grid (2×2×2 nodes) lowers to a `Regular3D`
/// `SampledField` with three axis grids and 8 flat data elements.
#[test]
fn lower_to_sampled_3d_grid_produces_regular3d_field() {
    // 2×2×2 grid: bounds_min=[0,0,0], bounds_max=[1,1,1], spacing=[1,1,1] →
    // each axis has 2 nodes (0,1); 8 cells total.
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        data: vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };

    let outcome = lower_to_sampled(&grid, "three_d_field", &Type::length()).unwrap();

    assert_eq!(outcome.field.kind, SampledGridKind::Regular3D);
    assert_eq!(outcome.field.axis_grids.len(), 3);
    assert_eq!(outcome.field.axis_grids[0].len(), 2);
    assert_eq!(outcome.field.axis_grids[1].len(), 2);
    assert_eq!(outcome.field.axis_grids[2].len(), 2);
    assert_eq!(outcome.field.data.len(), 8);
    assert!(outcome.warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Step-5 RED: unit-validation tests
// ---------------------------------------------------------------------------

/// Helper: minimal valid 1D grid with the given units, used by the
/// unit-validation tests so they share a stable shape.
fn unit_test_grid(units: Option<&str>) -> OpenVdbGridSource {
    OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: units.map(|s| s.to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    }
}

/// Pressure scalar codomain: `Type::Scalar { dimension: PRESSURE }`.
fn pressure_type() -> Type {
    Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    }
}

/// Step-5(a): grid declares `MPa` (Pressure), codomain is Pressure → Ok.
#[test]
fn validate_grid_units_matching_dimension_succeeds() {
    let grid = unit_test_grid(Some("MPa"));
    let result = lower_to_sampled(&grid, "p", &pressure_type());
    assert!(
        result.is_ok(),
        "MPa unit on a Pressure codomain must lower successfully, got {result:?}"
    );
}

/// Step-5(b): grid declares `m` (Length), codomain is Pressure → UnitMismatch.
#[test]
fn validate_grid_units_mismatched_dimension_returns_unit_mismatch() {
    let grid = unit_test_grid(Some("m"));
    let result = lower_to_sampled(&grid, "p", &pressure_type());
    match result {
        Err(IngestError::UnitMismatch {
            expected_dimension,
            found_dimension,
            found_unit,
        }) => {
            assert_eq!(expected_dimension, DimensionVector::PRESSURE);
            assert_eq!(found_dimension, DimensionVector::LENGTH);
            assert_eq!(found_unit, "m");
        }
        other => panic!(
            "expected Err(IngestError::UnitMismatch {{ … }}), got {other:?}"
        ),
    }
}

/// Step-5(c): grid declares unrecognised `ZZZ_unknown`, codomain is Length →
/// UnknownUnit.
#[test]
fn validate_grid_units_unknown_string_returns_unknown_unit() {
    let grid = unit_test_grid(Some("ZZZ_unknown"));
    let result = lower_to_sampled(&grid, "p", &Type::length());
    match result {
        Err(IngestError::UnknownUnit { unit }) => {
            assert_eq!(unit, "ZZZ_unknown");
        }
        other => panic!("expected Err(IngestError::UnknownUnit {{ … }}), got {other:?}"),
    }
}

/// Step-5(d): codomain is `Type::Bool` (not a numeric scalar) →
/// UnsupportedCodomain.
#[test]
fn validate_grid_units_unsupported_codomain_returns_error() {
    let grid = unit_test_grid(Some("m"));
    let result = lower_to_sampled(&grid, "p", &Type::Bool);
    match result {
        Err(IngestError::UnsupportedCodomain { type_repr }) => {
            assert_eq!(type_repr, "Bool");
        }
        other => panic!(
            "expected Err(IngestError::UnsupportedCodomain {{ … }}), got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Step-7 RED: interpolation-mapping tests
// ---------------------------------------------------------------------------

/// Helper: minimal valid 1D Length grid with the given interpolation, used
/// by the interpolation-mapping tests.
fn interp_test_grid(interpolation: OpenVdbInterpolation) -> OpenVdbGridSource {
    OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("m".to_string()),
        interpolation,
    }
}

/// Step-7(a): linear interpolation maps to InterpolationKind::Linear with
/// no warnings emitted.
#[test]
fn lower_to_sampled_linear_interpolation_emits_no_warnings() {
    let grid = interp_test_grid(OpenVdbInterpolation::Linear);
    let outcome = lower_to_sampled(&grid, "linf", &Type::length()).unwrap();
    assert_eq!(outcome.field.interpolation, InterpolationKind::Linear);
    assert!(
        outcome.warnings.is_empty(),
        "linear interpolation must not emit deferred warnings"
    );
}

/// Step-7(b): quadratic interpolation maps to InterpolationKind::Cubic with
/// a single InterpolationDeferred warning that names both modes.
#[test]
fn lower_to_sampled_quadratic_interpolation_maps_to_cubic_with_deferred_warning() {
    let grid = interp_test_grid(OpenVdbInterpolation::Quadratic);
    let outcome = lower_to_sampled(&grid, "quadf", &Type::length()).unwrap();
    assert_eq!(outcome.field.interpolation, InterpolationKind::Cubic);
    assert_eq!(
        outcome.warnings.len(),
        1,
        "quadratic mapping must emit exactly one deferred warning"
    );
    let w = &outcome.warnings[0];
    assert_eq!(w.code, Some(DiagnosticCode::InterpolationDeferred));
    assert_eq!(w.severity, Severity::Warning);
    assert!(
        w.message.contains("quadratic"),
        "warning message must name 'quadratic'; got {:?}",
        w.message
    );
    assert!(
        w.message.contains("Cubic"),
        "warning message must name target 'Cubic'; got {:?}",
        w.message
    );
}

/// Step-7(c): staggered interpolation maps to InterpolationKind::Linear with
/// a single InterpolationDeferred warning that names both modes.
#[test]
fn lower_to_sampled_staggered_interpolation_maps_to_linear_with_deferred_warning() {
    let grid = interp_test_grid(OpenVdbInterpolation::Staggered);
    let outcome = lower_to_sampled(&grid, "stagf", &Type::length()).unwrap();
    assert_eq!(outcome.field.interpolation, InterpolationKind::Linear);
    assert_eq!(
        outcome.warnings.len(),
        1,
        "staggered mapping must emit exactly one deferred warning"
    );
    let w = &outcome.warnings[0];
    assert_eq!(w.code, Some(DiagnosticCode::InterpolationDeferred));
    assert_eq!(w.severity, Severity::Warning);
    assert!(
        w.message.contains("staggered"),
        "warning message must name 'staggered'; got {:?}",
        w.message
    );
    assert!(
        w.message.contains("Linear"),
        "warning message must name target 'Linear'; got {:?}",
        w.message
    );
}
