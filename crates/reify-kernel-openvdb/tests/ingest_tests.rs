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
    IngestOutcome, OpenVdbGridKind, OpenVdbGridSource, OpenVdbInterpolation, lower_to_sampled,
};
use reify_types::{InterpolationKind, SampledGridKind, Type};

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
