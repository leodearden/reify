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
