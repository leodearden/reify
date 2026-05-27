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
    lower_to_sampled, read_vdb_file,
};
use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};
use reify_ir::{InterpolationKind, SampledGridKind};

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
        other => panic!("expected Err(IngestError::UnitMismatch {{ … }}), got {other:?}"),
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
        other => panic!("expected Err(IngestError::UnsupportedCodomain {{ … }}), got {other:?}"),
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

// ---------------------------------------------------------------------------
// Step-9 RED: grid-shape invariant tests
// ---------------------------------------------------------------------------

/// Step-9(a): empty data buffer → `IngestError::EmptyGrid`. Defends the
/// downstream `interp::interpolate_Nd` `assert!` on non-empty data — pinned
/// in the same shape as `engine_eval::build_sampled_field`'s pre-flight
/// guards.
#[test]
fn lower_to_sampled_empty_data_returns_empty_grid() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "empty", &Type::length());
    match result {
        Err(IngestError::EmptyGrid) => {}
        other => panic!("expected Err(IngestError::EmptyGrid), got {other:?}"),
    }
}

/// Step-9(b): 1D grid with mismatched data length →
/// `IngestError::DataShapeMismatch` carrying the expected node count, the
/// actual data length, and a `"4"` shape rendering for single-axis cases.
#[test]
fn lower_to_sampled_data_shape_mismatch_returns_data_shape_mismatch() {
    // 4 nodes expected (bounds [0,3], spacing 1.0), but data has 5 elements.
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0, 4.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "mismatch1d", &Type::length());
    match result {
        Err(IngestError::DataShapeMismatch {
            expected,
            actual,
            shape,
        }) => {
            assert_eq!(expected, 4);
            assert_eq!(actual, 5);
            assert_eq!(shape, "4");
        }
        other => panic!("expected Err(IngestError::DataShapeMismatch {{ … }}), got {other:?}"),
    }
}

/// Step-9(c): 2D 3×4 grid → 12 expected nodes, but data has 10 → shape
/// rendering uses the multi-axis `"3×4"` form.
#[test]
fn lower_to_sampled_2d_data_shape_mismatch_renders_axis_count() {
    // bounds_min=[0,0], bounds_max=[2,3], spacing=[1,1] →
    // axis 0 = 3 nodes (0,1,2), axis 1 = 4 nodes (0,1,2,3); 12 cells expected.
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular2D,
        bounds_min: vec![0.0, 0.0],
        bounds_max: vec![2.0, 3.0],
        spacing: vec![1.0, 1.0],
        data: vec![0.0; 10],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "mismatch2d", &Type::length());
    match result {
        Err(IngestError::DataShapeMismatch {
            expected,
            actual,
            shape,
        }) => {
            assert_eq!(expected, 12);
            assert_eq!(actual, 10);
            assert_eq!(shape, "3×4");
        }
        other => panic!("expected Err(IngestError::DataShapeMismatch {{ … }}), got {other:?}"),
    }
}

/// Step-9(d): non-positive spacing on any axis →
/// `IngestError::InvalidSpacing` carrying the offending axis index and
/// value. Defends the downstream linspace / interp math which assumes
/// strictly-positive finite spacing per axis.
#[test]
fn lower_to_sampled_non_positive_spacing_returns_invalid_spacing() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![0.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "zerospace", &Type::length());
    match result {
        Err(IngestError::InvalidSpacing { axis, value }) => {
            assert_eq!(axis, 0);
            assert_eq!(value, 0.0);
        }
        other => panic!("expected Err(IngestError::InvalidSpacing {{ … }}), got {other:?}"),
    }
}

/// Amendment: `kind = Regular3D` paired with single-element axis vectors
/// is a reachable caller construction mistake (since `OpenVdbGridSource`
/// has `pub` fields). Returning a structured `AxisLengthMismatch` instead
/// of panicking on `bounds_min[i]` indexing is the contract.
#[test]
fn lower_to_sampled_axis_length_mismatch_returns_structured_error() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular3D,
        bounds_min: vec![0.0],
        bounds_max: vec![1.0],
        spacing: vec![1.0],
        data: vec![0.0; 8],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "axisbad", &Type::length());
    match result {
        Err(IngestError::AxisLengthMismatch {
            axis_count,
            bounds_min_len,
            bounds_max_len,
            spacing_len,
        }) => {
            assert_eq!(axis_count, 3);
            assert_eq!(bounds_min_len, 1);
            assert_eq!(bounds_max_len, 1);
            assert_eq!(spacing_len, 1);
        }
        other => panic!("expected Err(IngestError::AxisLengthMismatch {{ … }}), got {other:?}"),
    }
}

/// Amendment: `bounds_max < bounds_min` is an inversion mistake; without a
/// dedicated check, `linspace_inclusive` silently collapses to `[start]`
/// and the user sees an unhelpful `DataShapeMismatch`. Pin the structured
/// `InvalidBounds` error so the failure mode is reported in the user's
/// own terms.
#[test]
fn lower_to_sampled_inverted_bounds_returns_invalid_bounds() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![3.0],
        bounds_max: vec![0.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "invbounds", &Type::length());
    match result {
        Err(IngestError::InvalidBounds { axis, min, max }) => {
            assert_eq!(axis, 0);
            assert_eq!(min, 3.0);
            assert_eq!(max, 0.0);
        }
        other => panic!("expected Err(IngestError::InvalidBounds {{ … }}), got {other:?}"),
    }
}

/// Amendment: a non-finite bound (NaN / Inf) on any axis must surface as
/// `InvalidBounds` rather than silently producing a 1-node axis from
/// linspace.
#[test]
fn lower_to_sampled_non_finite_bounds_returns_invalid_bounds() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![f64::NAN],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "nanbounds", &Type::length());
    match result {
        Err(IngestError::InvalidBounds { axis, .. }) => {
            assert_eq!(axis, 0);
        }
        other => panic!("expected Err(IngestError::InvalidBounds {{ … }}), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Step-3058 RED: degenerate-axis invariant
// ---------------------------------------------------------------------------

/// Step-3058: `bounds_min == bounds_max` with positive spacing collapses
/// `linspace_inclusive` to a 1-node axis (`[0.0]`), which the existing
/// `InvalidBounds` check does NOT reject (it uses `max < min`). The new
/// `DegenerateAxis` guard fires AFTER `axis_grids` is computed and catches
/// any axis with fewer than 2 nodes — defending `interp::interpolate_Nd`'s
/// `assert!(grid.len() >= 2)`.
///
/// Canonical degenerate input from the task description:
///   `bounds_min=[0.0]`, `bounds_max=[0.0]`, `spacing=[1.0]`, `data=[42.0]`
/// Pre-flight pipeline:
///   - AxisLengthMismatch: 1 == 1 ✓
///   - EmptyGrid: 1 element ≠ 0 ✓
///   - InvalidSpacing: 1.0 > 0 and finite ✓
///   - InvalidBounds: 0.0 finite, 0.0 finite, 0.0 < 0.0 is false ✓
///   - axis_grids: linspace_inclusive(0.0, 0.0, 1.0) → [0.0] (1 node)
///     → NEW DegenerateAxis guard fires here
#[test]
fn lower_to_sampled_degenerate_axis_returns_degenerate_axis() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![0.0],
        spacing: vec![1.0],
        data: vec![42.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "degenerate", &Type::length());
    match result {
        Err(IngestError::DegenerateAxis {
            axis,
            node_count,
            bounds_min,
            bounds_max,
            spacing,
        }) => {
            assert_eq!(axis, 0);
            assert_eq!(node_count, 1);
            assert_eq!(bounds_min, 0.0);
            assert_eq!(bounds_max, 0.0);
            assert_eq!(spacing, 1.0);
        }
        other => panic!("expected Err(IngestError::DegenerateAxis {{ … }}), got {other:?}"),
    }
}

/// Step-3058: `lower_to_sampled` rejects an axis grid with fewer than 2 nodes
/// when spacing is larger than the bounds span (second distinct degenerate case).
///
/// Pre-flight pipeline for `bounds_min=[0.0]`, `bounds_max=[0.4]`, `spacing=[1.0]`:
///   - AxisLengthMismatch: 1 == 1 ✓
///   - EmptyGrid: 1 element ≠ 0 ✓
///   - InvalidSpacing: 1.0 > 0 and finite ✓
///   - InvalidBounds: 0.0 finite, 0.4 finite, 0.4 < 0.0 is false ✓
///   - axis_grids: linspace_inclusive(0.0, 0.4, 1.0) → round(0.4/1.0) = 0 intervals → [0.0] (1 node)
///     → NEW DegenerateAxis guard fires here
#[test]
fn lower_to_sampled_spacing_exceeds_span_returns_degenerate_axis() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![0.4],
        spacing: vec![1.0],
        data: vec![1.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "degenerate_spacing", &Type::length());
    match result {
        Err(IngestError::DegenerateAxis {
            axis,
            node_count,
            bounds_min,
            bounds_max,
            spacing,
        }) => {
            assert_eq!(axis, 0);
            assert_eq!(node_count, 1);
            assert_eq!(bounds_min, 0.0);
            assert_eq!(bounds_max, 0.4);
            assert_eq!(spacing, 1.0);
        }
        other => panic!("expected Err(IngestError::DegenerateAxis {{ … }}), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Step-3060: ExcessiveAxisLength cap guard
// ---------------------------------------------------------------------------

/// Task 3060 / 3187: `lower_to_sampled` rejects an axis whose interval count is
/// **finite but exceeds** [`reify_types::sampled::LINSPACE_MAX_INTERVALS`].
///
/// This test covers the **finite-too-large** case: `bounds_max = LINSPACE_MAX_INTERVALS + 1`,
/// `spacing = 1.0` → `n_intervals = LINSPACE_MAX_INTERVALS + 1` exactly.
/// The `ExcessiveAxisLength` payload carries the precise count (not a saturated sentinel),
/// in parity with `LinspaceError::Excessive { n_intervals }` in `reify-types`.
///
/// Pre-flight pipeline for `bounds_min=[0.0]`, `bounds_max=[(LINSPACE_MAX_INTERVALS+1) as f64]`,
/// `spacing=[1.0]`:
///   - AxisLengthMismatch: 1 == 1 ✓
///   - EmptyGrid: 4 elements ≠ 0 ✓
///   - InvalidSpacing: 1.0 > 0 and finite ✓
///   - InvalidBounds: both finite, not inverted ✓
///   - axis_grids: linspace_inclusive → Err(Excessive { n_intervals }) → ExcessiveAxisLength
#[test]
fn lower_to_sampled_excessive_axis_returns_excessive_axis_length() {
    use reify_ir::sampled::LINSPACE_MAX_INTERVALS;
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        // Just over the production cap (10_000_001) — finite interval count,
        // triggers LinspaceError::Excessive (not Overflow).
        bounds_max: vec![(LINSPACE_MAX_INTERVALS + 1) as f64],
        spacing: vec![1.0],
        data: vec![0.0; 4],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "huge", &Type::length());
    match result {
        Err(IngestError::ExcessiveAxisLength { axis, n_intervals }) => {
            assert_eq!(axis, 0);
            assert_eq!(
                n_intervals,
                LINSPACE_MAX_INTERVALS + 1,
                "expected n_intervals == {} (exact finite count), got {}",
                LINSPACE_MAX_INTERVALS + 1,
                n_intervals
            );
        }
        other => panic!("expected Err(IngestError::ExcessiveAxisLength {{ … }}), got {other:?}"),
    }
}

/// Task 3187 step-4 RED: `lower_to_sampled` with a truly overflowing axis
/// (`bounds_max = 1e308`, `spacing = 1.0` → ratio ≈ 1e308 ≫ usize::MAX as f64)
/// must emit an error whose Display says "more intervals than usize can represent",
/// NOT the saturated `usize::MAX` count (18446744073709551615).
///
/// Currently RED: step-1 maps `LinspaceError::Overflow` to
/// `ExcessiveAxisLength { n_intervals: usize::MAX }`, so the Display message
/// leaks "18446744073709551615" and does not say "more intervals than…".
/// Step-5 adds `IngestError::OverflowingAxisLength { axis }` to fix this.
#[test]
fn lower_to_sampled_overflow_axis_display_says_more_intervals_than_usize() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![1e308],
        spacing: vec![1.0],
        data: vec![0.0; 4],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let err = lower_to_sampled(&grid, "huge", &Type::length())
        .expect_err("expected Err for overflow input, got Ok");
    let msg = err.to_string();
    // Distinct overflow phrasing — not a numeric "requires N intervals" message.
    assert!(
        msg.contains("more intervals than usize can represent"),
        "overflow Display should say 'more intervals than usize can represent', got: {msg:?}"
    );
    // Must NOT leak the saturated usize::MAX sentinel value.
    assert!(
        !msg.contains("18446744073709551615"),
        "overflow Display must not contain the saturated usize::MAX sentinel, got: {msg:?}"
    );
    // Axis identification preserved across the two ExcessiveAxisLength siblings.
    assert!(
        msg.contains("axis 0"),
        "overflow Display should identify the offending axis (axis 0), got: {msg:?}"
    );
}

// ---------------------------------------------------------------------------
// Step-11 RED: read_vdb_file v0.2 stub contract
// ---------------------------------------------------------------------------

/// Step-11: `read_vdb_file` is the v0.2 stub for the file-read entry point.
/// Pins the FfiNotImplemented variant + the Display contract that a
/// follow-up FFI implementation must preserve (so consumers' error parsing
/// continues to work after the body is swapped in).
///
/// Display contract: the path payload is the structural part — operators
/// must be able to identify the offending file in a multi-import workflow.
/// The surrounding prose ("OpenVDB", task ID, etc.) is incidental and
/// intentionally not pinned, so future rewording (or the FFI body landing)
/// doesn't have to update test assertions.
///
/// This test is gated `cfg(not(has_openvdb))` because when the real FFI is
/// present the stub body is replaced by the real read path (see task 3095
/// step-8). The parallel `cfg(has_openvdb)` test is added in step-9.
#[cfg(not(has_openvdb))]
#[test]
fn read_vdb_file_returns_ffi_not_implemented_with_path() {
    let result = read_vdb_file("path/to/example.vdb", "voxel_grid", &Type::length());
    let err = match result {
        Err(IngestError::FfiNotImplemented { path }) => {
            assert_eq!(path, "path/to/example.vdb");
            IngestError::FfiNotImplemented { path }
        }
        other => panic!("expected Err(IngestError::FfiNotImplemented {{ path }}), got {other:?}"),
    };

    // Pin only the structural payload of the Display: the path. Prose is
    // incidental and not part of the contract.
    let msg = format!("{err}");
    assert!(
        msg.contains("path/to/example.vdb"),
        "Display message must include the path; got {msg:?}"
    );
}

/// Step-9 (`cfg(has_openvdb)`): `read_vdb_file` with a non-existent path must
/// return `IngestError::FileReadError { path, .. }` — NOT `FfiNotImplemented`.
///
/// Pins the new error contract that the real FFI body introduces: the OpenVDB
/// I/O layer raises a C++ exception (`std::runtime_error`) which the cxx bridge
/// maps to a Rust `cxx::Exception`; `read_vdb_file` converts it to
/// `IngestError::FileReadError { path: <the caller's path>, detail: <ex.what()> }`.
///
/// The `detail` field is intentionally not asserted — its prose comes from the
/// OpenVDB C++ layer and may differ across library versions.
#[cfg(has_openvdb)]
#[test]
fn read_vdb_file_missing_path_returns_file_read_error() {
    let missing = "/nonexistent/path/does-not-exist.vdb";
    let result = read_vdb_file(missing, "any_grid", &Type::Real);
    match result {
        Err(IngestError::FileReadError { path, detail: _ }) => {
            assert_eq!(
                path, missing,
                "FileReadError must carry the requested path; got path={path:?}"
            );
        }
        other => panic!(
            "expected Err(IngestError::FileReadError {{ path: {:?}, .. }}), got {other:?}",
            missing
        ),
    }
}

// ---------------------------------------------------------------------------
// Amendment: extra coverage for documented edge cases
// ---------------------------------------------------------------------------

/// Amendment: the documented "caller-managed contract" path — when a grid
/// declares no units (`units = None`), `validate_grid_units` short-circuits
/// to `Ok(())` and the lowering proceeds. Pins the early-return that the
/// `sampled { … }` source path also relies on (no metadata, no validation).
#[test]
fn lower_to_sampled_no_units_skips_dimension_check() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: None,
        interpolation: OpenVdbInterpolation::Linear,
    };
    // Even though codomain = Pressure, units = None means the caller takes
    // responsibility for the dimensional contract.
    let outcome = lower_to_sampled(&grid, "nounits", &pressure_type())
        .expect("missing units must skip the dimension check");
    assert_eq!(outcome.field.kind, SampledGridKind::Regular1D);
    assert!(outcome.warnings.is_empty());
}

/// Amendment: the PRD's worked example codomain `Tensor<2, 3, Pressure>`
/// paired with grid units `MPa` (also Pressure) must lower successfully
/// end-to-end through the public API. Complements the
/// `extract_codomain_dimension` internal tests which only assert the
/// recursion in isolation.
#[test]
fn lower_to_sampled_tensor_pressure_with_mpa_grid_succeeds() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("MPa".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let codomain = Type::tensor(
        2,
        3,
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        },
    );
    let outcome = lower_to_sampled(&grid, "stress", &codomain)
        .expect("Tensor<2,3,Pressure> + MPa must lower successfully");
    assert_eq!(outcome.field.kind, SampledGridKind::Regular1D);
    assert!(outcome.warnings.is_empty());
}

/// Amendment: a `Type::Real` codomain (dimensionless) paired with a
/// unit-bearing grid (e.g. `m`) is a common caller mistake. It must
/// surface as `UnitMismatch` (LENGTH vs DIMENSIONLESS) rather than
/// silently succeeding.
#[test]
fn lower_to_sampled_real_codomain_with_meter_grid_returns_unit_mismatch() {
    let grid = OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        data: vec![0.0, 1.0, 2.0, 3.0],
        units: Some("m".to_string()),
        interpolation: OpenVdbInterpolation::Linear,
    };
    let result = lower_to_sampled(&grid, "real", &Type::Real);
    match result {
        Err(IngestError::UnitMismatch {
            expected_dimension,
            found_dimension,
            found_unit,
        }) => {
            assert_eq!(expected_dimension, DimensionVector::DIMENSIONLESS);
            assert_eq!(found_dimension, DimensionVector::LENGTH);
            assert_eq!(found_unit, "m");
        }
        other => panic!("expected Err(IngestError::UnitMismatch {{ … }}), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Step-15 RED: API-surface contract test
// ---------------------------------------------------------------------------

/// Step-15: pins the v0.2 ingestion-API public surface that the eventual
/// task-5 wiring layer (and any future FFI follow-up) will consume.
/// Compile-RED if any name is not `pub` from the `ingest` module;
/// runtime-RED if `KNOWN_UNITS` is below the v0.2 minimum size (m / Pa /
/// MPa / K / kg + at least one prefixed length variant).
#[test]
fn ingest_module_public_api_surface_compiles() {
    use reify_kernel_openvdb::ingest::{
        IngestError, IngestOutcome, KNOWN_UNITS, OpenVdbGridKind, OpenVdbGridSource,
        OpenVdbInterpolation, lower_to_sampled, read_vdb_file, validate_grid_units,
    };
    // Reference each name once so the use-import is not unused.
    let _: fn(_, _, _) -> Result<IngestOutcome, IngestError> = lower_to_sampled;
    let _: fn(_, _, _) -> Result<IngestOutcome, IngestError> = read_vdb_file;
    let _: fn(_, _) -> Result<(), IngestError> = validate_grid_units;
    let _: OpenVdbGridKind = OpenVdbGridKind::Regular1D;
    let _: OpenVdbInterpolation = OpenVdbInterpolation::Linear;
    let _grid: Option<OpenVdbGridSource> = None;
    assert!(
        KNOWN_UNITS.len() >= 6,
        "v0.2 KNOWN_UNITS must cover at least {{m, Pa, MPa, K, kg}} + ≥1 prefixed length \
         (≥6 entries); got {}",
        KNOWN_UNITS.len()
    );
}
