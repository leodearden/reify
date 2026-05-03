//! OpenVDB ingestion pipeline (v0.2 task 2666).
//!
//! This module exposes the structural ingestion path that takes an
//! OpenVDB grid (in-memory `OpenVdbGridSource`) and lowers it to the
//! internal [`reify_types::SampledField`] representation already used by
//! `field def F { source = sampled { … } }`. The compiler/eval wiring
//! (`field def … source = imported { … }`) is wired in task 5.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/imported-field-source.md` "Decomposition plan" task 2.
//!
//! # v0.2 scope
//!
//! Real OpenVDB FFI is deferred to a follow-up task (the file-read entry
//! point [`read_vdb_file`] returns [`IngestError::FfiNotImplemented`] until
//! the FFI lands). This module ships the in-memory ingestion path that
//! the compiler/eval wiring (task 5) and a future FFI body will plug into.
//!
//! # Module layout
//!
//! - [`OpenVdbGridSource`] / [`OpenVdbGridKind`] / [`OpenVdbInterpolation`] —
//!   in-memory model of an OpenVDB grid.
//! - [`IngestError`] — fatal ingestion failures (returned via `Result::Err`).
//! - [`IngestOutcome`] — successful return: `SampledField` + non-fatal warnings.
//! - [`lower_to_sampled`] — orchestrates the in-memory lowering pipeline.
//! - [`read_vdb_file`] — v0.2 stub that returns `FfiNotImplemented`.

use reify_types::{InterpolationKind, SampledField, SampledGridKind, Type};

/// Spatial-grid shape of an OpenVDB source grid.
///
/// Maps 1:1 to [`SampledGridKind`] at lowering time. Mirrors OpenVDB's
/// 1D / 2D / 3D structured-grid kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenVdbGridKind {
    Regular1D,
    Regular2D,
    Regular3D,
}

/// Interpolation modes that an OpenVDB grid may declare on its metadata.
///
/// Per the PRD, the three modes the importer may encounter are
/// `linear`, `quadratic`, and `staggered`. The mapping to
/// [`InterpolationKind`] is intentionally lossy (see [`lower_to_sampled`])
/// because `InterpolationKind` does not have native `Quadratic` /
/// `Staggered` variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenVdbInterpolation {
    Linear,
    Quadratic,
    Staggered,
}

/// In-memory model of an OpenVDB grid ready to be lowered to a
/// [`SampledField`].
///
/// Constructed either by a future OpenVDB FFI layer (file read) or by a
/// caller that already has the grid in memory.
#[derive(Debug, Clone)]
pub struct OpenVdbGridSource {
    /// 1D / 2D / 3D shape selector.
    pub kind: OpenVdbGridKind,
    /// Per-axis lower bound (length 1/2/3 matching `kind`).
    pub bounds_min: Vec<f64>,
    /// Per-axis upper bound.
    pub bounds_max: Vec<f64>,
    /// Per-axis grid spacing.
    pub spacing: Vec<f64>,
    /// Flat row-major data buffer (axis-0 outermost).
    pub data: Vec<f64>,
    /// Optional grid-units string from OpenVDB metadata (e.g. `"m"`, `"MPa"`).
    pub units: Option<String>,
    /// Interpolation mode declared on the grid metadata.
    pub interpolation: OpenVdbInterpolation,
}

/// Fatal ingestion failures.
///
/// Non-fatal interpolation-fallback warnings are surfaced via
/// [`IngestOutcome::warnings`] instead.
#[derive(Debug, Clone, PartialEq)]
pub enum IngestError {
    /// Returned by [`read_vdb_file`] until the OpenVDB FFI lands. Carries
    /// the path that the caller asked to read so the error message can
    /// name it concretely.
    FfiNotImplemented {
        /// The path the caller asked to read.
        path: String,
    },
}

/// Successful ingestion result: the lowered field plus any non-fatal
/// warnings (e.g. interpolation deferrals).
#[derive(Debug)]
pub struct IngestOutcome {
    /// The lowered sampled-field runtime value.
    pub field: SampledField,
    /// Non-fatal warnings emitted during lowering. Currently used for
    /// interpolation-deferral diagnostics; will be expanded in
    /// follow-up steps (units / unsupported-codomain warnings).
    pub warnings: Vec<reify_types::Diagnostic>,
}

/// Lower an in-memory OpenVDB grid to a [`SampledField`].
///
/// Step-2 minimal implementation: handles the `Regular1D` arm only. Step-4
/// extends this to `Regular2D` and `Regular3D`.
///
/// # Errors
///
/// Returns [`IngestError`] for any fatal ingestion failure. For step-2
/// the only failure variant is [`IngestError::FfiNotImplemented`] (not
/// reachable from this entry point — only [`read_vdb_file`] returns it).
///
/// # Codomain type
///
/// `codomain_type` is currently unused in step-2 (unit validation is
/// added in step-6). Marked `_codomain_type` for now.
pub fn lower_to_sampled(
    grid: &OpenVdbGridSource,
    name: &str,
    _codomain_type: &Type,
) -> Result<IngestOutcome, IngestError> {
    let axis_count = match grid.kind {
        OpenVdbGridKind::Regular1D => 1,
        OpenVdbGridKind::Regular2D => 2,
        OpenVdbGridKind::Regular3D => 3,
    };
    debug_assert_eq!(grid.bounds_min.len(), axis_count);
    debug_assert_eq!(grid.bounds_max.len(), axis_count);
    debug_assert_eq!(grid.spacing.len(), axis_count);

    let kind = match grid.kind {
        OpenVdbGridKind::Regular1D => SampledGridKind::Regular1D,
        OpenVdbGridKind::Regular2D => SampledGridKind::Regular2D,
        OpenVdbGridKind::Regular3D => SampledGridKind::Regular3D,
    };

    let axis_grids: Vec<Vec<f64>> = (0..axis_count)
        .map(|i| linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], grid.spacing[i]))
        .collect();

    let interpolation = InterpolationKind::Linear;

    let field = SampledField {
        name: name.to_string(),
        kind,
        bounds_min: grid.bounds_min.clone(),
        bounds_max: grid.bounds_max.clone(),
        spacing: grid.spacing.clone(),
        axis_grids,
        interpolation,
        data: grid.data.clone(),
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    };

    Ok(IngestOutcome {
        field,
        warnings: Vec::new(),
    })
}

/// Inclusive linspace from `start` to `stop` with step `spacing`.
///
/// Produces `[start, start+spacing, …, stop]` (or as close as
/// `floor((stop-start)/spacing)` admits). Returns `[start]` for
/// degenerate-but-valid inputs (non-positive spacing or `stop < start`).
///
/// Mirrors `engine_eval::linspace_inclusive` byte-identically so that
/// OpenVDB-imported and user-supplied sampled fields share one axis-grid
/// layout — keeping all downstream interp assumptions transferable.
fn linspace_inclusive(start: f64, stop: f64, spacing: f64) -> Vec<f64> {
    if spacing <= 0.0 || !spacing.is_finite() || !start.is_finite() || !stop.is_finite() {
        return vec![start];
    }
    let span = stop - start;
    if span < 0.0 {
        return vec![start];
    }
    // Round to nearest integer to avoid floating-point cliff effects on
    // exact-fit cases (e.g. (2.0 - 0.0) / 1.0 → 1.999… instead of 2).
    let n_intervals = (span / spacing).round() as usize;
    let count = n_intervals + 1;
    (0..count).map(|i| start + (i as f64) * spacing).collect()
}
