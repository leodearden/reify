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

use reify_types::{
    Diagnostic, DiagnosticCode, DimensionVector, InterpolationKind, SampledField, SampledGridKind,
    Type,
};

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
    pub warnings: Vec<reify_types::Diagnostic>,
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
    let found_dimension = lookup_unit_dimension(unit_str).ok_or_else(|| {
        IngestError::UnknownUnit {
            unit: unit_str.to_string(),
        }
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
/// `Type::Scalar { dimension }`. `Type::Real` is treated as
/// [`DimensionVector::DIMENSIONLESS`] for compatibility with the rest of
/// the language. All other variants (Bool, Int, String, Enum, Function,
/// Geometry, etc.) are not meaningful field codomains for OpenVDB-imported
/// numeric data and produce [`IngestError::UnsupportedCodomain`].
fn extract_codomain_dimension(t: &Type) -> Result<DimensionVector, IngestError> {
    match t {
        Type::Scalar { dimension } => Ok(*dimension),
        Type::Real => Ok(DimensionVector::DIMENSIONLESS),
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
/// to identify the variant in error messages (e.g. "Bool", "Int",
/// "String", "Geometry", "Enum", "Function", "List", …). Avoids depending
/// on a `Display` impl that may not exist on every variant.
fn format_type_repr(t: &Type) -> String {
    match t {
        Type::Bool => "Bool".to_string(),
        Type::Int => "Int".to_string(),
        Type::String => "String".to_string(),
        Type::Enum(name) => format!("Enum({name})"),
        Type::List(_) => "List".to_string(),
        Type::Set(_) => "Set".to_string(),
        Type::Map(_, _) => "Map".to_string(),
        Type::Option(_) => "Option".to_string(),
        Type::Function { .. } => "Function".to_string(),
        Type::TypeParam(name) => format!("TypeParam({name})"),
        Type::StructureRef(name) => format!("StructureRef({name})"),
        Type::TraitObject(name) => format!("TraitObject({name})"),
        Type::Field { .. } => "Field".to_string(),
        Type::Geometry => "Geometry".to_string(),
        Type::Complex(_) => "Complex".to_string(),
        Type::Orientation(n) => format!("Orientation({n})"),
        Type::Frame(n) => format!("Frame({n})"),
        Type::Transform(n) => format!("Transform({n})"),
        Type::Range(_) => "Range".to_string(),
        Type::Plane => "Plane".to_string(),
        Type::Axis => "Axis".to_string(),
        Type::BoundingBox => "BoundingBox".to_string(),
        Type::Error => "Error".to_string(),
        Type::Union(_) => "Union".to_string(),
        // The Scalar/Real/Tensor/Vector/Point/Matrix arms are handled by
        // extract_codomain_dimension before reaching here, so they should
        // never appear in an UnsupportedCodomain error. Render generically
        // for completeness.
        Type::Scalar { .. } => "Scalar".to_string(),
        Type::Real => "Real".to_string(),
        Type::Tensor { .. } => "Tensor".to_string(),
        Type::Vector { .. } => "Vector".to_string(),
        Type::Point { .. } => "Point".to_string(),
        Type::Matrix { .. } => "Matrix".to_string(),
    }
}
