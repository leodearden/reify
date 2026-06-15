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

use reify_ir::sampled::{LINSPACE_MAX_INTERVALS, LinspaceError, linspace_inclusive};
use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, Type};
use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

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
    /// The OpenVDB grid declared a unit whose dimension does not match the
    /// codomain type's dimension (e.g. grid units = `m` (Length) but
    /// codomain = Pressure).
    UnitMismatch {
        /// Dimension extracted from the field declaration's codomain type.
        expected_dimension: DimensionVector,
        /// Dimension looked up from the OpenVDB grid's units string.
        found_dimension: DimensionVector,
        /// The grid units string that was looked up.
        found_unit: String,
    },
    /// The OpenVDB grid declared a units string not present in the v0.2
    /// [`KNOWN_UNITS`] table. The follow-up FFI task may extend the table
    /// if a wider corpus of `.vdb` files surfaces missing units.
    UnknownUnit {
        /// The unrecognised unit string.
        unit: String,
    },
    /// The codomain type passed to [`lower_to_sampled`] is not a meaningful
    /// numeric field codomain (e.g. `Type::Bool`, `Type::String`,
    /// `Type::Geometry`).
    UnsupportedCodomain {
        /// String representation of the offending codomain type.
        type_repr: String,
    },
    /// The grid carries no data values. Defends downstream
    /// `interp::interpolate_Nd` which assumes a non-empty data buffer.
    EmptyGrid,
    /// The flat data buffer's length does not match the product of
    /// per-axis node counts (row-major, axis-0 outermost). Defends the
    /// length-equality `assert!` inside `interp::interpolate_Nd`.
    DataShapeMismatch {
        /// Number of data elements the grid shape would require.
        expected: usize,
        /// Number of data elements actually supplied.
        actual: usize,
        /// `'×'`-joined per-axis node-count rendering (e.g. `"4"` for 1D,
        /// `"3×4"` for 2D, `"2×2×2"` for 3D).
        shape: String,
    },
    /// A per-axis spacing is non-positive or non-finite. Defends the
    /// downstream `linspace_inclusive` / `interp::interpolate_Nd` math
    /// which assumes strictly-positive finite spacing per axis.
    InvalidSpacing {
        /// Index of the offending axis (0 = outermost).
        axis: usize,
        /// The offending spacing value.
        value: f64,
    },
    /// One or more of `bounds_min` / `bounds_max` / `spacing` does not have
    /// length equal to the axis count implied by `kind`. Surfaces a
    /// caller-side construction mistake (e.g. `kind = Regular3D` paired with
    /// a 1-element `bounds_min`) as a structured error rather than a panic
    /// on a downstream `bounds_min[i]` index.
    AxisLengthMismatch {
        /// Axis count implied by [`OpenVdbGridSource::kind`] (1/2/3).
        axis_count: usize,
        /// Length of the supplied `bounds_min` vector.
        bounds_min_len: usize,
        /// Length of the supplied `bounds_max` vector.
        bounds_max_len: usize,
        /// Length of the supplied `spacing` vector.
        spacing_len: usize,
    },
    /// A per-axis `bounds_max` is below `bounds_min`, or one of the bounds
    /// is non-finite. Defends the linspace builder which would otherwise
    /// silently collapse to a 1-node axis and surface as a confusing
    /// `DataShapeMismatch` downstream.
    InvalidBounds {
        /// Index of the offending axis (0 = outermost).
        axis: usize,
        /// The offending `bounds_min[axis]` value.
        min: f64,
        /// The offending `bounds_max[axis]` value.
        max: f64,
    },
    /// An axis grid produced by `linspace_inclusive` has fewer than 2 nodes.
    /// Defends `interp::interpolate_Nd`'s `assert!(grid.len() >= 2)` which
    /// requires at least two nodes per axis to perform any interpolation.
    ///
    /// This fires AFTER `axis_grids` is built so the check uses the same
    /// rounding arithmetic as `linspace_inclusive` itself — catching both
    /// `bounds_min == bounds_max` and spacing-larger-than-span cases that
    /// the earlier `InvalidBounds` guard (`max < min`) does not reject.
    DegenerateAxis {
        /// Index of the offending axis (0 = outermost).
        axis: usize,
        /// Actual number of nodes produced (always 1 in current `linspace_inclusive`;
        /// the guard is `< 2` for defense-in-depth).
        node_count: usize,
        /// `bounds_min[axis]` that produced the collapse.
        bounds_min: f64,
        /// `bounds_max[axis]` that produced the collapse.
        bounds_max: f64,
        /// `spacing[axis]` that produced the collapse.
        spacing: f64,
    },
    /// An axis interval count exceeds [`reify_types::sampled::LINSPACE_MAX_INTERVALS`].
    ///
    /// A legitimately-finite but enormous combination such as
    /// `bounds_min=0.0`, `bounds_max=1e308`, `spacing=1.0` would make
    /// `(span / spacing).round() as usize` saturate on overflow and attempt
    /// to allocate an astronomically large `Vec`.  This variant surfaces
    /// before any allocation when [`linspace_inclusive`] returns `None`.
    ///
    /// Distinct from [`IngestError::DegenerateAxis`]: the cap rejects axes
    /// that are too **long**, `DegenerateAxis` rejects axes that are too
    /// **short** (collapsed to 1 node).
    ExcessiveAxisLength {
        /// Index of the offending axis (0 = outermost).
        axis: usize,
        /// Computed interval count that exceeded the cap.  Always a finite,
        /// representable `usize` — when the ratio overflows `usize`, the
        /// `OverflowingAxisLength` variant is used instead.
        n_intervals: usize,
    },
    /// An axis interval count exceeds `usize::MAX` (i.e. `(span/spacing) > usize::MAX as f64`).
    ///
    /// Distinct from [`IngestError::ExcessiveAxisLength`]: this variant indicates
    /// the count cannot be meaningfully represented in `usize`, so no `n_intervals`
    /// payload is carried — embedding the saturated `usize::MAX` value in a
    /// user-facing message would falsely imply a precise (though absurd) count.
    OverflowingAxisLength {
        /// Index of the offending axis (0 = outermost).
        axis: usize,
    },
    /// Returned by [`read_vdb_file`] (cfg(has_openvdb) mode) when the
    /// underlying `openvdb::io::File` layer fails — file not found, wrong
    /// grid type, missing grid name, etc.
    ///
    /// The `path` payload names the file the caller asked to read so the
    /// error message can identify it concretely in a multi-import workflow.
    FileReadError {
        /// The path the caller asked to read.
        path: String,
        /// Detail string from the underlying FFI exception.
        detail: String,
    },
}

/// v0.2 OpenVDB units → [`DimensionVector`] lookup table.
///
/// Intentionally small: covers the units the PRD's worked examples and the
/// common engineering-OpenVDB grid metadata use (m / mm / cm / km;
/// Pa / kPa / MPa / GPa; K; kg; kg/m^3). Unrecognised unit strings yield
/// [`IngestError::UnknownUnit`].
///
/// # Why not `reify-compiler`'s unit registry?
///
/// `reify-kernel-openvdb` is a peer adapter crate that deliberately does
/// NOT depend on `reify-compiler` (the dependency direction is inverted at
/// the workspace level — see `Cargo.toml` comment block). Pulling in the
/// full unit registry would form a cycle. A small static slice is
/// sufficient for v0.2; the follow-up FFI task can revisit if a wider
/// corpus of real `.vdb` files surfaces missing units.
pub static KNOWN_UNITS: &[(&str, DimensionVector)] = &[
    // Length and prefixed variants — all map to the LENGTH dimension.
    ("m", DimensionVector::LENGTH),
    ("mm", DimensionVector::LENGTH),
    ("cm", DimensionVector::LENGTH),
    ("km", DimensionVector::LENGTH),
    // Pressure and prefixed variants.
    ("Pa", DimensionVector::PRESSURE),
    ("kPa", DimensionVector::PRESSURE),
    ("MPa", DimensionVector::PRESSURE),
    ("GPa", DimensionVector::PRESSURE),
    // Temperature.
    ("K", DimensionVector::TEMPERATURE),
    // Mass.
    ("kg", DimensionVector::MASS),
    // Mass density.
    ("kg/m^3", DimensionVector::MASS_DENSITY),
];

/// Successful ingestion result: the lowered field plus any non-fatal
/// warnings (e.g. interpolation deferrals).
#[derive(Debug)]
pub struct IngestOutcome {
    /// The lowered sampled-field runtime value.
    pub field: SampledField,
    /// Non-fatal warnings emitted during lowering. Currently used for
    /// interpolation-deferral diagnostics; will be expanded in
    /// follow-up steps (units / unsupported-codomain warnings).
    pub warnings: Vec<reify_core::Diagnostic>,
}

/// Build an [`OpenVdbGridSource`] from FFI-extracted metadata and the raw f32
/// densified buffer that comes out of `grid_densify_to_buffer`.
///
/// This is the shared construction helper used by BOTH:
/// - [`read_vdb_file`] (file-read path: opens a `.vdb` file → extracts
///   metadata via FFI → densifies → calls this helper).
/// - `OpenVdbKernel::densify_grid_to_sampled` (in-kernel path: starts from
///   an already-registered handle → extracts metadata via FFI → densifies
///   → calls this helper).
///
/// # Invariants captured here (one place, not duplicated)
///
/// - `kind = Regular3D` — both paths work with 3-D SDF grids only.
/// - Axis-0 = X, Axis-1 = Y, Axis-2 = Z (row-major X-outermost) — matches
///   `reify_expr::interp::interpolate_3d`, `engine_eval::build_sampled_field`,
///   and the workspace-wide row-major convention.
/// - `f32 → f64` conversion via **consuming** `into_iter()` so the transient
///   f32 buffer is freed as soon as the f64 `collect()` finishes.
///   At the C++-side cap (~256M voxels = 1 GiB f32) this keeps the peak at
///   ~3 GiB rather than holding both buffers live (esc-3095-97 suggestion 2).
/// - `units_str.is_empty() → None` — an empty units string from
///   `grid_units()` means the grid carries no units metadata; `None` in
///   `OpenVdbGridSource` is the signal that `validate_grid_units` short-circuits
///   without a `UnitMismatch` check.
/// - `interpolation = Linear` — `meshToLevelSet`-built grids do not write
///   interpolation metadata; the correct interpolation for a continuous SDF
///   is linear (box-sampler).
#[cfg(has_openvdb)]
pub(crate) fn build_realized_grid_source(
    voxel_sizes: [f64; 3],
    bbox_min: [f64; 3],
    bbox_max: [f64; 3],
    units_str: &str,
    raw_buffer: Vec<f32>,
) -> OpenVdbGridSource {
    // `into_iter()` (consuming) — not `iter()` (borrowing) — so the f32
    // buffer is freed as soon as the f64 collect finishes.
    let data: Vec<f64> = raw_buffer.into_iter().map(|v| v as f64).collect();
    OpenVdbGridSource {
        kind: OpenVdbGridKind::Regular3D,
        bounds_min: bbox_min.to_vec(),
        bounds_max: bbox_max.to_vec(),
        spacing: voxel_sizes.to_vec(),
        data,
        units: if units_str.is_empty() {
            None
        } else {
            Some(units_str.to_string())
        },
        interpolation: OpenVdbInterpolation::Linear,
    }
}

/// Lower an in-memory OpenVDB grid to a [`SampledField`].
///
/// Handles `Regular1D` / `Regular2D` / `Regular3D` arms uniformly, mapping
/// each to the corresponding [`SampledGridKind`] and constructing per-axis
/// grids via [`linspace_inclusive`].
///
/// # Errors
///
/// Returns [`IngestError`] for any fatal ingestion failure:
///   - [`IngestError::UnitMismatch`] / [`IngestError::UnknownUnit`] /
///     [`IngestError::UnsupportedCodomain`] from
///     [`validate_grid_units`].
///   - Step-10 adds the empty-grid / data-shape-mismatch / invalid-spacing
///     pre-flight guards.
pub fn lower_to_sampled(
    grid: &OpenVdbGridSource,
    name: &str,
    codomain_type: &Type,
) -> Result<IngestOutcome, IngestError> {
    validate_grid_units(grid.units.as_deref(), codomain_type)?;

    let axis_count = match grid.kind {
        OpenVdbGridKind::Regular1D => 1,
        OpenVdbGridKind::Regular2D => 2,
        OpenVdbGridKind::Regular3D => 3,
    };

    // Pre-flight invariant checks — mirrors `engine_eval::build_sampled_field`'s
    // step-24 guards so the lowered SampledField never trips downstream
    // `interp::interpolate_Nd` `assert!`s on malformed input.
    //
    // Order:
    //   (0) reject axis-length mismatch first — `OpenVdbGridSource` is a
    //       `pub` struct with public fields, so a caller-constructed grid
    //       with `kind = Regular3D` but `bounds_min.len() == 1` is a
    //       reachable failure mode that must be surfaced before any
    //       `bounds_min[i]` indexing happens.
    //   (1) reject empty data buffer — `EmptyGrid` is more descriptive than
    //       `DataShapeMismatch { expected: N, actual: 0 }` for the common
    //       "user forgot to populate data" failure mode.
    //   (2) reject non-positive / non-finite spacing per axis — surfaces a
    //       precise per-axis error before linspace collapses to a 1-node
    //       grid.
    //   (3) reject inverted / non-finite bounds per axis — same reasoning,
    //       linspace silently collapses to `[start]` for negative span.
    //   (4) build `axis_grids`, then reject any axis with < 2 nodes —
    //       `bounds_min == bounds_max` and spacing-larger-than-span both
    //       pass checks (0)–(3) but collapse to a 1-node axis after
    //       linspace rounding; checking post-build keeps the guard
    //       lockstep with `linspace_inclusive`'s own arithmetic.
    //   (5) reject `data.len() != product(axis_lengths)` last, after the
    //       axis grids are well-formed enough to compute `expected`.
    if grid.bounds_min.len() != axis_count
        || grid.bounds_max.len() != axis_count
        || grid.spacing.len() != axis_count
    {
        return Err(IngestError::AxisLengthMismatch {
            axis_count,
            bounds_min_len: grid.bounds_min.len(),
            bounds_max_len: grid.bounds_max.len(),
            spacing_len: grid.spacing.len(),
        });
    }
    if grid.data.is_empty() {
        return Err(IngestError::EmptyGrid);
    }
    for (i, s) in grid.spacing.iter().enumerate() {
        if !(*s > 0.0 && s.is_finite()) {
            return Err(IngestError::InvalidSpacing { axis: i, value: *s });
        }
    }
    for i in 0..axis_count {
        let min = grid.bounds_min[i];
        let max = grid.bounds_max[i];
        if !min.is_finite() || !max.is_finite() || max < min {
            return Err(IngestError::InvalidBounds { axis: i, min, max });
        }
    }

    let kind = match grid.kind {
        OpenVdbGridKind::Regular1D => SampledGridKind::Regular1D,
        OpenVdbGridKind::Regular2D => SampledGridKind::Regular2D,
        OpenVdbGridKind::Regular3D => SampledGridKind::Regular3D,
    };

    let mut axis_grids: Vec<Vec<f64>> = Vec::with_capacity(axis_count);
    for i in 0..axis_count {
        match linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], grid.spacing[i]) {
            Ok(g) => axis_grids.push(g),
            Err(LinspaceError::Excessive { n_intervals }) => {
                return Err(IngestError::ExcessiveAxisLength {
                    axis: i,
                    n_intervals,
                });
            }
            Err(LinspaceError::Overflow) => {
                return Err(IngestError::OverflowingAxisLength { axis: i });
            }
        }
    }

    // Guard (4): each axis must have ≥ 2 nodes after linspace construction.
    // Catches bounds_min == bounds_max and spacing-larger-than-span — both
    // pass the earlier InvalidBounds/InvalidSpacing checks but collapse to
    // a 1-node axis due to linspace rounding. Mirrors the engine_eval guard
    // in `build_sampled_field` (engine_eval.rs:858–877).
    for (i, axis) in axis_grids.iter().enumerate() {
        if axis.len() < 2 {
            return Err(IngestError::DegenerateAxis {
                axis: i,
                node_count: axis.len(),
                bounds_min: grid.bounds_min[i],
                bounds_max: grid.bounds_max[i],
                spacing: grid.spacing[i],
            });
        }
    }

    let expected: usize = axis_grids.iter().map(|g| g.len()).product();
    if grid.data.len() != expected {
        let shape = axis_grids
            .iter()
            .map(|g| g.len().to_string())
            .collect::<Vec<_>>()
            .join("×");
        return Err(IngestError::DataShapeMismatch {
            expected,
            actual: grid.data.len(),
            shape,
        });
    }

    let (interpolation, interp_warning) = map_interpolation(name, grid.interpolation);

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

    let mut warnings = Vec::new();
    if let Some(w) = interp_warning {
        warnings.push(w);
    }

    Ok(IngestOutcome { field, warnings })
}

/// Map an [`OpenVdbInterpolation`] to the corresponding
/// [`InterpolationKind`], emitting a deferred-warning diagnostic for
/// modes that lower lossily.
///
/// Mappings:
///   - `Linear`    → `InterpolationKind::Linear` (no warning)
///   - `Quadratic` → `InterpolationKind::Cubic` (warning: deferred to v0.2)
///   - `Staggered` → `InterpolationKind::Linear` (warning: deferred to v0.2)
///
/// Mirrors the existing `W_INTERPOLATION_DEFERRED` precedent in
/// `crates/reify-expr/src/interp.rs` (Rbf/Kriging fallback). A single
/// shared `DiagnosticCode::InterpolationDeferred` keeps consumer filtering
/// simple.
fn map_interpolation(
    grid_name: &str,
    vdb: OpenVdbInterpolation,
) -> (InterpolationKind, Option<Diagnostic>) {
    match vdb {
        OpenVdbInterpolation::Linear => (InterpolationKind::Linear, None),
        OpenVdbInterpolation::Quadratic => {
            let warn = Diagnostic::warning(format!(
                "OpenVDB grid '{grid_name}' declares quadratic interpolation; \
                 mapping to Cubic for v0.2"
            ))
            .with_code(DiagnosticCode::InterpolationDeferred);
            (InterpolationKind::Cubic, Some(warn))
        }
        OpenVdbInterpolation::Staggered => {
            let warn = Diagnostic::warning(format!(
                "OpenVDB grid '{grid_name}' declares staggered interpolation; \
                 mapping to Linear for v0.2"
            ))
            .with_code(DiagnosticCode::InterpolationDeferred);
            (InterpolationKind::Linear, Some(warn))
        }
    }
}

/// Validate that the OpenVDB grid's declared units are dimensionally
/// compatible with the field declaration's codomain type.
///
/// Returns:
///   - `Ok(())` when both sides agree (or when the grid declares no units —
///     interpreted as a caller-managed contract; the codomain is still
///     extracted to surface unsupported-codomain errors regardless).
///   - `Err(IngestError::UnknownUnit)` when the grid's unit string is not
///     in [`KNOWN_UNITS`].
///   - `Err(IngestError::UnitMismatch)` when the grid's unit dimension does
///     not match the codomain's leaf-Scalar dimension.
///   - `Err(IngestError::UnsupportedCodomain)` when the codomain is not a
///     numeric Scalar / Real / Tensor / Vector / Point.
///
/// Used internally by [`lower_to_sampled`]; exposed publicly so the
/// follow-up FFI body and task-5's compiler/eval wiring can pre-validate
/// before invoking the full lowering pipeline.
pub fn validate_grid_units(
    grid_units: Option<&str>,
    codomain_type: &Type,
) -> Result<(), IngestError> {
    let expected_dimension = extract_codomain_dimension(codomain_type)?;
    let Some(unit_str) = grid_units else {
        // Grid has no declared units — codomain extraction succeeded, so the
        // numeric path is at least valid. The caller takes responsibility
        // for the dimensional contract (matches the `sampled { … }` source
        // path which has no unit metadata at all).
        return Ok(());
    };
    let found_dimension =
        lookup_unit_dimension(unit_str).ok_or_else(|| IngestError::UnknownUnit {
            unit: unit_str.to_string(),
        })?;
    if found_dimension != expected_dimension {
        return Err(IngestError::UnitMismatch {
            expected_dimension,
            found_dimension,
            found_unit: unit_str.to_string(),
        });
    }
    Ok(())
}

/// Read a `.vdb` file and lower the named `FloatGrid` to a [`SampledField`].
///
/// When `cfg(has_openvdb)` is set (i.e. `/opt/reify-deps` is present), this
/// function opens the file via the OpenVDB FFI, extracts grid metadata and
/// active-voxel data, and delegates downstream validation/lowering to
/// [`lower_to_sampled`].
///
/// When `cfg(has_openvdb)` is NOT set (stub build), the function returns
/// [`IngestError::FfiNotImplemented`] so callers can distinguish the
/// stub-mode surface-scaffold from a real read failure.
///
/// # Parameters
///
/// - `path`: filesystem path to the `.vdb` file.
/// - `grid_name`: name of the `FloatGrid` inside the `.vdb` file.
/// - `codomain_type`: codomain type declared by the field definition;
///   used to validate grid units via [`lower_to_sampled`].
///
/// # Errors
///
/// - `cfg(has_openvdb)`: returns [`IngestError::FileReadError`] for
///   file-not-found, missing/wrong-type grid, or other FFI-layer failures;
///   propagates all [`IngestError`] variants from [`lower_to_sampled`].
/// - `cfg(not(has_openvdb))`: always returns [`IngestError::FfiNotImplemented`].
#[cfg(not(has_openvdb))]
pub fn read_vdb_file(
    path: &str,
    _grid_name: &str,
    _codomain_type: &Type,
) -> Result<IngestOutcome, IngestError> {
    Err(IngestError::FfiNotImplemented {
        path: path.to_string(),
    })
}

/// Real `read_vdb_file` body — compiled only when `cfg(has_openvdb)` is set.
///
/// Opens the named `FloatGrid` from `path` via the cxx-bridge FFI, densifies
/// active voxels into a row-major f64 buffer over the active bounding box,
/// and calls [`lower_to_sampled`] to produce the final [`IngestOutcome`].
///
/// # Layout
///
/// The densified buffer from `grid_densify_to_buffer` is X-outermost
/// (axis-0 = X, axis-1 = Y, axis-2 = Z, row-major); axis-0 corresponds to
/// `bounds_min/max[0]` (world X). The `OpenVdbGridSource` is constructed
/// accordingly so `lower_to_sampled`'s axis-count × data-shape checks always
/// match. This matches the workspace-wide row-major-axis-0-outermost
/// convention used by `reify_expr::interp::interpolate_3d`,
/// `engine_eval::build_sampled_field`, and `reify-expr`'s
/// `field_reductions`/`sampled`/`interp` modules.
///
/// # Library initialisation
///
/// Calls [`crate::init::ensure_initialized`] before any FFI access so callers
/// that invoke `read_vdb_file` directly — without first instantiating an
/// `OpenVdbKernel` — still register the built-in grid types in OpenVDB's
/// I/O dispatch table. Without this, `gridPtrCast<FloatGrid>` returns null
/// and the read path emits a misleading "is not a FloatGrid" error.
#[cfg(has_openvdb)]
pub fn read_vdb_file(
    path: &str,
    grid_name: &str,
    codomain_type: &Type,
) -> Result<IngestOutcome, IngestError> {
    use crate::ffi::ffi as openvdb_ffi;

    // Ensure OpenVDB's I/O dispatch table is populated before any FFI access.
    // Idempotent — guarded by a OnceLock in `crate::init`.
    crate::init::ensure_initialized();

    // Open and read the named FloatGrid from the .vdb file.
    let grid_handle = openvdb_ffi::read_vdb_grid_ffi(path, grid_name).map_err(|e| {
        IngestError::FileReadError {
            path: path.to_string(),
            detail: e.to_string(),
        }
    })?;

    // Extract grid metadata via FFI accessors.
    //
    // `grid_voxel_sizes` returns the per-axis diagonal of the grid's linear
    // transform. For meshToVolume-built grids these are isotropic (all three
    // equal) but external `.vdb` imports may carry an anisotropic transform;
    // propagating the per-axis values into `SampledField.spacing` keeps the
    // axis grids consistent with `bounds_min/max` regardless. (Earlier
    // revisions called the FFI as `grid_voxel_size` returning a single
    // scalar, which silently replaced Y/Z spacing with X spacing for any
    // anisotropic grid — producing axis grids whose lengths did not match
    // the densified buffer length and yielding `DataShapeMismatch`.)
    let voxel_sizes = openvdb_ffi::grid_voxel_sizes(&grid_handle);
    let bbox_min_arr = openvdb_ffi::grid_bbox_min(&grid_handle);
    let bbox_max_arr = openvdb_ffi::grid_bbox_max(&grid_handle);
    let units_str = openvdb_ffi::grid_units(&grid_handle);

    // Densify all active voxels into a flat f32 buffer (X-outermost,
    // row-major) and convert to f64 for the in-memory model.
    //
    // The C++ side rejects bbox densifications exceeding
    // `GRID_DENSIFY_MAX_VOXELS` (~256M voxels ≈ 1 GiB) by throwing a
    // `std::runtime_error`; cxx maps it to `Err(cxx::Exception)` which we
    // surface as `IngestError::FileReadError`.
    let raw_buffer: Vec<f32> = openvdb_ffi::grid_densify_to_buffer(&grid_handle).map_err(|e| {
        IngestError::FileReadError {
            path: path.to_string(),
            detail: e.to_string(),
        }
    })?;
    // Build the OpenVdbGridSource from the per-axis metadata and the raw
    // f32 buffer.  The drift-prone construction details (Regular3D,
    // X-outermost, f32→f64 conversion, units-empty→None) live in ONE
    // place — `build_realized_grid_source` — so both this file-read path
    // and `densify_grid_to_sampled` share the same axis convention.
    let source = build_realized_grid_source(voxel_sizes, bbox_min_arr, bbox_max_arr, &units_str, raw_buffer);

    lower_to_sampled(&source, grid_name, codomain_type)
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::FfiNotImplemented { path } => write!(
                f,
                "OpenVDB file ingestion is not yet implemented; \
                 reify-kernel-openvdb's read_vdb_file is a v0.2 \
                 ingestion-API surface scaffold for task 2666. \
                 Real OpenVDB FFI is a follow-up task. (path = {path})"
            ),
            IngestError::UnitMismatch {
                expected_dimension,
                found_dimension,
                found_unit,
            } => write!(
                f,
                "OpenVDB grid unit '{found_unit}' has dimension {found_dimension:?} \
                 but field codomain expects dimension {expected_dimension:?}"
            ),
            IngestError::UnknownUnit { unit } => write!(
                f,
                "OpenVDB grid declares unrecognised unit string '{unit}'; \
                 not present in reify-kernel-openvdb's v0.2 KNOWN_UNITS table"
            ),
            IngestError::UnsupportedCodomain { type_repr } => write!(
                f,
                "field codomain type '{type_repr}' is not a meaningful \
                 numeric codomain for OpenVDB-imported data"
            ),
            IngestError::EmptyGrid => {
                write!(f, "OpenVDB grid carries no data values (empty data buffer)")
            }
            IngestError::DataShapeMismatch {
                expected,
                actual,
                shape,
            } => write!(
                f,
                "OpenVDB grid data length {actual} does not match grid shape \
                 ({shape}); expected {expected} elements (row-major, \
                 axis-0 outermost)"
            ),
            IngestError::InvalidSpacing { axis, value } => write!(
                f,
                "OpenVDB grid axis {axis} spacing must be positive and finite, got {value}"
            ),
            IngestError::AxisLengthMismatch {
                axis_count,
                bounds_min_len,
                bounds_max_len,
                spacing_len,
            } => write!(
                f,
                "OpenVDB grid axis-vector length mismatch: kind implies {axis_count} axes \
                 but bounds_min has {bounds_min_len}, bounds_max has {bounds_max_len}, \
                 spacing has {spacing_len}"
            ),
            IngestError::InvalidBounds { axis, min, max } => write!(
                f,
                "OpenVDB grid axis {axis} bounds are invalid: bounds_min={min}, bounds_max={max} \
                 (max must be finite and >= min)"
            ),
            IngestError::DegenerateAxis {
                axis,
                node_count,
                bounds_min,
                bounds_max,
                spacing,
            } => write!(
                f,
                "OpenVDB grid axis {axis} produced only {node_count} node(s); need at least 2 \
                 (check bounds and spacing — bounds_min={bounds_min} bounds_max={bounds_max} \
                 spacing={spacing})"
            ),
            IngestError::ExcessiveAxisLength { axis, n_intervals } => write!(
                f,
                "OpenVDB grid axis {axis} requires {n_intervals} intervals, which exceeds the \
                 maximum of {LINSPACE_MAX_INTERVALS}; reduce the axis span or increase the spacing"
            ),
            IngestError::OverflowingAxisLength { axis } => write!(
                f,
                "OpenVDB grid axis {axis} requires more intervals than usize can represent; \
                 reduce the axis span or increase the spacing"
            ),
            IngestError::FileReadError { path, detail } => {
                write!(f, "OpenVDB file read failed for '{path}': {detail}")
            }
        }
    }
}

impl std::error::Error for IngestError {}

/// Look up a unit string in [`KNOWN_UNITS`].
fn lookup_unit_dimension(unit: &str) -> Option<DimensionVector> {
    KNOWN_UNITS
        .iter()
        .find(|(s, _)| *s == unit)
        .map(|(_, d)| *d)
}

/// Extract the leaf-Scalar dimension from a field codomain type.
///
/// Recurses through composite quantity-bearing variants
/// (`Type::Tensor`/`Vector`/`Point`/`Matrix`) to reach the leaf
/// `Type::Scalar { dimension }`. `Type::dimensionless_scalar()` is treated as
/// [`DimensionVector::DIMENSIONLESS`] for compatibility with the rest of
/// the language. All other variants (Bool, Int, String, Enum, Function,
/// Geometry, etc.) are not meaningful field codomains for OpenVDB-imported
/// numeric data and produce [`IngestError::UnsupportedCodomain`].
fn extract_codomain_dimension(t: &Type) -> Result<DimensionVector, IngestError> {
    match t {
        Type::Scalar { dimension } => Ok(*dimension),
        Type::Tensor { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Point { quantity, .. }
        | Type::Matrix { quantity, .. } => extract_codomain_dimension(quantity),
        other => Err(IngestError::UnsupportedCodomain {
            type_repr: format_type_repr(other),
        }),
    }
}

/// Render a short structural label for an unsupported codomain, sufficient
/// to identify the variant in error messages (e.g. `"Bool"`, `"Int"`,
/// `"Geometry"`, `"Enum"`, `"Function"`, `"List"`, …).
///
/// Uses an exhaustive `match` over every `Type` variant, returning the
/// exact Rust identifier name as a `&'static str`. The exhaustiveness
/// check is the key property: adding a new variant to `Type` in
/// `reify-types` produces a compiler error here, forcing a deliberate
/// label decision rather than silently inheriting whatever `Debug`
/// formatting happens to emit.
///
/// Trade-off: payload data (e.g. the name inside `Type::Enum("Foo")`) is
/// dropped — only the variant identity is preserved. That's the correct
/// granularity for an "unsupported codomain" error message; the variant
/// identity is the actionable contract.
fn format_type_repr(t: &Type) -> String {
    match t {
        Type::Bool => "Bool",
        Type::Int => "Int",
        Type::String => "String",
        Type::Scalar { dimension } if dimension.is_dimensionless() => "Real",
        Type::Scalar { .. } => "Scalar",
        Type::Enum(_) => "Enum",
        Type::List(_) => "List",
        Type::Set(_) => "Set",
        Type::Map(_, _) => "Map",
        Type::Keyed(_) => "Keyed",
        Type::Option(_) => "Option",
        Type::Function { .. } => "Function",
        Type::TypeParam(_) => "TypeParam",
        Type::StructureRef(_) => "StructureRef",
        Type::TraitObject(_) => "TraitObject",
        Type::Field { .. } => "Field",
        Type::Geometry => "Geometry",
        Type::Point { .. } => "Point",
        Type::Vector { .. } => "Vector",
        Type::Tensor { .. } => "Tensor",
        Type::Complex(_) => "Complex",
        Type::Orientation(_) => "Orientation",
        Type::Frame(_) => "Frame",
        Type::Transform(_) => "Transform",
        Type::AffineMap(_) => "AffineMap",
        Type::Selector(_) => "Selector",
        Type::AnySelector => "Selector",
        Type::Range(_) => "Range",
        Type::Plane => "Plane",
        Type::Axis => "Axis",
        Type::Direction => "Direction",
        Type::Relation => "Relation",
        Type::BoundingBox => "BoundingBox",
        Type::Matrix { .. } => "Matrix",
        Type::ScalarParam(_) => "ScalarParam",
        Type::Error => "Error",
        Type::Union(_) => "Union",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    //! Internal-only tests for the codomain-dimension extractor.
    //!
    //! The integration suite (`tests/ingest_tests.rs`) exercises
    //! `extract_codomain_dimension` indirectly through `lower_to_sampled`.
    //! These tests pin the recursion correctness directly so the contract
    //! is locked in independently of the public lowering pipeline.
    use super::*;

    /// Step-13(a): a `Tensor<2, 3, Pressure>` codomain → `Ok(PRESSURE)`.
    /// Pins the `Tensor → quantity` recursion that the PRD's worked
    /// example (`Tensor<2, 3, Pressure>`) relies on.
    #[test]
    fn extract_codomain_dimension_recurses_through_tensor() {
        let t = Type::tensor(
            2,
            3,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        );
        assert_eq!(
            extract_codomain_dimension(&t),
            Ok(DimensionVector::PRESSURE)
        );
    }

    /// Step-13(b): a `Vector<3, Length>` codomain → `Ok(LENGTH)`. Pins the
    /// `Vector → quantity` recursion arm.
    #[test]
    fn extract_codomain_dimension_recurses_through_vector_of_length() {
        let t = Type::vec3(Type::length());
        assert_eq!(extract_codomain_dimension(&t), Ok(DimensionVector::LENGTH));
    }

    /// Step-13(c): `Type::dimensionless_scalar()` codomain → `Ok(DIMENSIONLESS)`. Pins the
    /// `Real` arm, which exists for compatibility with the rest of the
    /// language treating dimensionless numerics as `Type::dimensionless_scalar()`.
    #[test]
    fn extract_codomain_dimension_real_is_dimensionless() {
        assert_eq!(
            extract_codomain_dimension(&Type::dimensionless_scalar()),
            Ok(DimensionVector::DIMENSIONLESS)
        );
    }

    /// Contract table: `format_type_repr` returns the exact Rust variant
    /// identifier name for every `Type` variant.
    ///
    /// This test is the regression net for the exhaustive-match refactor in
    /// step 2: if any arm is misspelled or omitted, the compiler flags the
    /// exhaustiveness failure (for missing arms) or this test catches a
    /// wrong string at runtime.
    #[test]
    fn format_type_repr_returns_variant_identifier_name_for_each_type_variant() {
        // Unit variants (10)
        assert_eq!(format_type_repr(&Type::Bool), "Bool");
        assert_eq!(format_type_repr(&Type::Int), "Int");
        assert_eq!(format_type_repr(&Type::dimensionless_scalar()), "Real");
        assert_eq!(format_type_repr(&Type::String), "String");
        assert_eq!(format_type_repr(&Type::Geometry), "Geometry");
        assert_eq!(format_type_repr(&Type::Plane), "Plane");
        assert_eq!(format_type_repr(&Type::Axis), "Axis");
        assert_eq!(format_type_repr(&Type::Direction), "Direction");
        assert_eq!(format_type_repr(&Type::BoundingBox), "BoundingBox");
        assert_eq!(format_type_repr(&Type::Error), "Error");

        // Newtype-payload variants (13)
        assert_eq!(format_type_repr(&Type::Enum("X".into())), "Enum");
        assert_eq!(format_type_repr(&Type::List(Box::new(Type::Bool))), "List");
        assert_eq!(format_type_repr(&Type::Set(Box::new(Type::Bool))), "Set");
        assert_eq!(
            format_type_repr(&Type::Keyed(Box::new(Type::Bool))),
            "Keyed"
        );
        assert_eq!(
            format_type_repr(&Type::Option(Box::new(Type::Bool))),
            "Option"
        );
        assert_eq!(
            format_type_repr(&Type::Complex(Box::new(Type::dimensionless_scalar()))),
            "Complex"
        );
        assert_eq!(format_type_repr(&Type::Range(Box::new(Type::Int))), "Range");
        assert_eq!(format_type_repr(&Type::Orientation(3)), "Orientation");
        assert_eq!(format_type_repr(&Type::Frame(3)), "Frame");
        assert_eq!(format_type_repr(&Type::Transform(3)), "Transform");
        assert_eq!(format_type_repr(&Type::AffineMap(3)), "AffineMap");
        assert_eq!(
            format_type_repr(&Type::Selector(reify_core::ty::SelectorKind::Face)),
            "Selector"
        );
        assert_eq!(format_type_repr(&Type::TypeParam("T".into())), "TypeParam");
        assert_eq!(
            format_type_repr(&Type::StructureRef("S".into())),
            "StructureRef"
        );
        assert_eq!(
            format_type_repr(&Type::TraitObject("Tr".into())),
            "TraitObject"
        );
        assert_eq!(
            format_type_repr(&Type::Union(vec![Type::Bool, Type::Int])),
            "Union"
        );
        assert_eq!(
            format_type_repr(&Type::ScalarParam("Q".into())),
            "ScalarParam"
        );

        // Two-element tuple variant (1)
        assert_eq!(
            format_type_repr(&Type::Map(Box::new(Type::Bool), Box::new(Type::Int))),
            "Map"
        );

        // Struct-like variants (7)
        assert_eq!(
            format_type_repr(&Type::Scalar {
                dimension: DimensionVector::LENGTH
            }),
            "Scalar"
        );
        assert_eq!(
            format_type_repr(&Type::Function {
                params: vec![],
                return_type: Box::new(Type::Bool),
            }),
            "Function"
        );
        assert_eq!(
            format_type_repr(&Type::Field {
                domain: Box::new(Type::dimensionless_scalar()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "Field"
        );
        assert_eq!(
            format_type_repr(&Type::Point {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }),
            "Point"
        );
        assert_eq!(
            format_type_repr(&Type::Vector {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }),
            "Vector"
        );
        assert_eq!(
            format_type_repr(&Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }),
            "Tensor"
        );
        assert_eq!(
            format_type_repr(&Type::Matrix {
                m: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }),
            "Matrix"
        );
        // β (task 4602): Applied and Projection — new variants; RED until step-2.
        assert_eq!(
            format_type_repr(&Type::applied(
                "Coupling",
                vec![Type::StructureRef("Prismatic".into())]
            )),
            "Applied"
        );
        assert_eq!(
            format_type_repr(&Type::projection(
                Type::StructureRef("Prismatic".into()),
                "MotionValue"
            )),
            "Projection"
        );
    }
}
