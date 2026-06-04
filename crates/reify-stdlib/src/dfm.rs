//! Design-for-Manufacturing (DFM) builtins (PRD v0_6 process-dfm-completion, task α).
//!
//! Two surfaces, mirroring the stackup / flexure modules:
//!
//! - [`eval_dfm`] — the pure builtin dispatcher (sibling of `stackup::eval_stackup`),
//!   wired into `crate::eval_builtin`'s fall-through chain in `lib.rs`. It evaluates
//!   `fits_build_volume(part_bbox, envelope_bbox[, severity_or_rule])`, a pure
//!   bbox-vs-bbox extent comparator (no kernel / `EvalContext` access). The two
//!   `Value::BoundingBox` inputs are resolved from Solids UPSTREAM by the existing
//!   kernel-aware `bounding_box(solid)` builtin, so `fits_build_volume` itself stays
//!   unit-testable and dependency-free (PRD §2.1 / §4 decision 4).
//!
//! - [`diagnose`] — the `DFMSeverity` → diagnostic-severity bridge (sibling of
//!   `flexures::flexure_diagnose`). It is re-exported as `crate::dfm_diagnose` and
//!   called from reify-expr's builtin fall-through on BOTH the success and the
//!   `Value::Undef` paths: a successfully-evaluated `fits_build_volume` that returns
//!   `Bool(false)` is a build-volume VIOLATION whose severity comes from the optional
//!   rule argument; a `Value::Undef` result is a usage error.

use reify_ir::Value;
use reify_core::{Diagnostic, DimensionVector, Severity};

use crate::helpers::tensor_components_f64;

/// Evaluate a DFM builtin by name.
///
/// Returns `Some(value)` if `name` is a recognised DFM function, `None` otherwise
/// (so the dispatch chain in `lib.rs` can fall through). Mirrors
/// [`crate::stackup::eval_stackup`]'s `Option<Value>` fall-through convention.
pub(crate) fn eval_dfm(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "fits_build_volume" => fits_build_volume(args),
        _ => return None,
    })
}

// --- fits_build_volume ---

/// `fits_build_volume(part_bbox, envelope_bbox[, severity_or_rule]) -> Bool`.
///
/// Pure component-wise extent comparator: returns `Value::Bool(true)` iff the
/// part's per-axis extent `<=` the envelope's on every axis (EXACT `<=`, no
/// tolerance — PRD §3 G6, so equal extents fit). Both inputs must be
/// `Value::BoundingBox` whose corners are finite 3-component LENGTH Point3s; the
/// optional 3rd argument carries the rule's `DFMSeverity` for [`diagnose`] and is
/// ignored by the boolean compute. Any malformed input yields `Value::Undef`.
fn fits_build_volume(args: &[Value]) -> Value {
    if !matches!(args.len(), 2 | 3) {
        return Value::Undef;
    }
    let part = match parse_bbox_extents(&args[0]) {
        Some(e) => e,
        None => return Value::Undef,
    };
    let envelope = match parse_bbox_extents(&args[1]) {
        Some(e) => e,
        None => return Value::Undef,
    };
    // The optional 3rd arg tags the violation severity for `diagnose` (a bare
    // DFMSeverity enum or a DFMRule structure-instance). It does NOT affect the
    // fit, but a malformed tag is a usage error → Undef.
    if args.len() == 3 && parse_dfm_severity(&args[2]).is_none() {
        return Value::Undef;
    }
    let fits = (0..3).all(|i| part[i] <= envelope[i]);
    Value::Bool(fits)
}

/// Extract the per-axis extents `[x, y, z] = max - min` from a `Value::BoundingBox`.
///
/// Returns `None` (→ the caller yields `Value::Undef`) unless `v` is a
/// `Value::BoundingBox` whose `min` and `max` corners are each a finite,
/// 3-component LENGTH `Point`/`Vector`/`Tensor`. [`tensor_components_f64`] already
/// rejects non-container, empty, non-numeric, and internally-mixed-dimension
/// corners; on top of that we require exactly 3 components, the LENGTH dimension on
/// BOTH corners (which also rejects a min/max dimension mismatch), and finite
/// coordinates (so a NaN/inf corner cannot corrupt the extent compare).
fn parse_bbox_extents(v: &Value) -> Option<[f64; 3]> {
    let (min, max) = match v {
        Value::BoundingBox { min, max } => (min, max),
        _ => return None,
    };
    let (min_vals, min_dim) = tensor_components_f64(min)?;
    let (max_vals, max_dim) = tensor_components_f64(max)?;
    if min_vals.len() != 3 || max_vals.len() != 3 {
        return None;
    }
    if min_dim != DimensionVector::LENGTH || max_dim != DimensionVector::LENGTH {
        return None;
    }
    if !min_vals.iter().all(|x| x.is_finite()) || !max_vals.iter().all(|x| x.is_finite()) {
        return None;
    }
    Some([
        max_vals[0] - min_vals[0],
        max_vals[1] - min_vals[1],
        max_vals[2] - min_vals[2],
    ])
}

/// Parse the rule's `DFMSeverity` from a single `fits_build_volume` 3rd argument.
///
/// Accepts either a bare `Value::Enum { type_name: "DFMSeverity", variant }` or a
/// DFMRule `Value::StructureInstance` carrying a `severity` field of that enum, and
/// maps the `Info` / `Warning` / `Error` variant to the matching [`Severity`].
/// Returns `None` if `v` is neither shape, or carries an unrecognized variant. This
/// both rejects a malformed 3rd arg in [`fits_build_volume`] and is the shape-reader
/// the [`diagnose`] severity bridge reuses (where absence falls back to a default).
fn parse_dfm_severity(v: &Value) -> Option<Severity> {
    let variant = match v {
        Value::Enum { type_name, variant } if type_name == "DFMSeverity" => variant.as_str(),
        Value::StructureInstance(data) => match data.fields.get("severity") {
            Some(Value::Enum { type_name, variant }) if type_name == "DFMSeverity" => {
                variant.as_str()
            }
            _ => return None,
        },
        _ => return None,
    };
    match variant {
        "Info" => Some(Severity::Info),
        "Warning" => Some(Severity::Warning),
        "Error" => Some(Severity::Error),
        _ => None,
    }
}

/// Resolve the `DFMSeverity` carried by a `fits_build_volume` call for the
/// success-path violation diagnostic.
///
/// Scans the optional 3rd argument via [`parse_dfm_severity`] (a bare
/// `DFMSeverity` enum or a DFMRule structure-instance), DEFAULTING to
/// [`Severity::Warning`] when the tag is absent or unrecognized — a build-volume
/// violation is a Warning unless the rule declares otherwise.
fn dfm_severity(args: &[Value]) -> Severity {
    args.get(2)
        .and_then(parse_dfm_severity)
        .unwrap_or(Severity::Warning)
}

/// Build the code-less build-volume VIOLATION diagnostic at `severity`.
///
/// Mirrors [`crate::geometry::diagnose`]'s code-less convention (no
/// `DiagnosticCode` — adding one would touch reify-core, out of this task's
/// scope); the `{I,W,E}_DFM_BUILD_VOLUME` message prefix preserves the PRD's
/// diagnostic-code naming. The severity-specific constructor
/// (`Diagnostic::info` / `warning` / `error`) sets `Diagnostic.severity`, which is
/// the asserted contract.
fn build_volume_violation(severity: Severity) -> Diagnostic {
    let msg = |prefix: char| {
        format!(
            "{prefix}_DFM_BUILD_VOLUME: part does not fit the build volume — its \
             bounding-box extent exceeds the envelope on at least one axis; shrink or \
             reorient the part, or select a larger build envelope"
        )
    };
    match severity {
        Severity::Info => Diagnostic::info(msg('I')),
        Severity::Warning => Diagnostic::warning(msg('W')),
        Severity::Error => Diagnostic::error(msg('E')),
    }
}

/// Pure post-call DFM diagnostic classifier (the `DFMSeverity` bridge).
///
/// Mirrors [`crate::flexures::flexure_diagnose`]: returns a `Vec<Diagnostic>`, fires on
/// BOTH the success and `Value::Undef` paths, and short-circuits to an empty `Vec` for
/// any non-DFM `name`.
///
/// Success path: a `fits_build_volume` that constructed fine but evaluated to
/// `Value::Bool(false)` is a build-volume VIOLATION (the rule holds, the design
/// breaks it) — one diagnostic at the rule's declared [`dfm_severity`]. A
/// `Bool(true)` design fits and surfaces nothing. (The `Value::Undef` usage-error
/// path is added in step-10.)
pub fn diagnose(name: &str, args: &[Value], result: &Value) -> Vec<Diagnostic> {
    if name != "fits_build_volume" {
        return Vec::new();
    }
    let mut diags = Vec::new();
    if let Value::Bool(false) = result {
        diags.push(build_volume_violation(dfm_severity(args)));
    }
    diags
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use reify_core::DimensionVector;
    use reify_core::identity::RealizationNodeId;
    use reify_ir::geometry::GeometryHandleId;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

    /// A LENGTH scalar of `si` metres.
    fn len(si: f64) -> Value {
        Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH }
    }

    /// A `Value::BoundingBox` from two LENGTH Point3 corners (metres).
    fn bbox(min: [f64; 3], max: [f64; 3]) -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![len(min[0]), len(min[1]), len(min[2])])),
            max: Box::new(Value::Point(vec![len(max[0]), len(max[1]), len(max[2])])),
        }
    }

    // ─── step-1: fits_build_volume happy path ──────────────────────────────

    #[test]
    fn fits_build_volume_part_inside_envelope_true() {
        // 10 mm cube part inside a 20 mm cube envelope → fits.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(true)));
    }

    #[test]
    fn fits_build_volume_part_past_one_axis_false() {
        // Part extent 30 mm on X exceeds the 20 mm envelope (Y/Z fit) → does not fit.
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(false)));
    }

    #[test]
    fn fits_build_volume_equal_extents_true() {
        // Inclusive `<=`: equal extents fit (PRD §3 G6, no tolerance).
        let part = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(true)));
    }

    #[test]
    fn fits_build_volume_extent_is_position_invariant_true() {
        // The compare is over extents (max-min), not absolute position: a 10 mm
        // part offset far from the origin still fits a 20 mm envelope at the origin.
        let part = bbox([0.100, 0.100, 0.100], [0.110, 0.110, 0.110]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(true)));
    }

    // ─── step-3 helpers: malformed-input builders ──────────────────────────

    /// A scalar of `si` in the given dimension.
    fn scalar(si: f64, dim: DimensionVector) -> Value {
        Value::Scalar { si_value: si, dimension: dim }
    }

    /// A `Value::BoundingBox` whose min/max corners are Point3s of `dim`.
    fn bbox_dim(min: [f64; 3], max: [f64; 3], dim: DimensionVector) -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![scalar(min[0], dim), scalar(min[1], dim), scalar(min[2], dim)])),
            max: Box::new(Value::Point(vec![scalar(max[0], dim), scalar(max[1], dim), scalar(max[2], dim)])),
        }
    }

    /// A `Value::BoundingBox` from explicit min/max corner values (malformed corners).
    fn bbox_pts(min: Value, max: Value) -> Value {
        Value::BoundingBox { min: Box::new(min), max: Box::new(max) }
    }

    /// A raw kernel "Solid" handle value — NOT a `Value::BoundingBox`.
    fn solid_handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Part", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: GeometryHandleId(1),
        }
    }

    /// A bare `DFMSeverity` enum value.
    fn dfm_sev(variant: &str) -> Value {
        Value::Enum { type_name: "DFMSeverity".into(), variant: variant.into() }
    }

    /// A `DFMRule` structure-instance carrying a `severity` field.
    fn dfm_rule(sev_variant: &str) -> Value {
        let mut fields = PersistentMap::new();
        fields.insert("severity".to_string(), dfm_sev(sev_variant));
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "DFMRule".into(),
            version: 1,
            fields,
        }))
    }

    // ─── step-3: fits_build_volume validation → Undef ──────────────────────

    #[test]
    fn fits_build_volume_arity_zero_undef() {
        assert_eq!(eval_dfm("fits_build_volume", &[]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_arity_one_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        assert_eq!(eval_dfm("fits_build_volume", &[part]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_arity_four_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, dfm_sev("Warning"), Value::Int(1)]),
            Some(Value::Undef)
        );
    }

    #[test]
    fn fits_build_volume_non_bbox_real_undef() {
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[Value::Real(1.0), env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_non_bbox_map_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let m = Value::Map(BTreeMap::new());
        assert_eq!(eval_dfm("fits_build_volume", &[part, m]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_non_bbox_solid_handle_undef() {
        // A raw Solid (ephemeral kernel handle) is NOT a BoundingBox; it must be
        // resolved via bounding_box(...) UPSTREAM. Passing one directly is a usage error.
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[solid_handle(), env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_non_bbox_undef_arg_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, Value::Undef]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_dimensionless_corners_undef() {
        // Corners must be LENGTH; a DIMENSIONLESS bbox is rejected (drives step-4).
        let part = bbox_dim([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], DimensionVector::DIMENSIONLESS);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_mass_corners_undef() {
        // A MASS-dimensioned bbox is not a spatial extent (drives step-4).
        let part = bbox_dim([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], DimensionVector::MASS);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_mismatched_min_max_dim_undef() {
        // min LENGTH, max MASS — inconsistent corner dimensions (drives step-4).
        let part = bbox_pts(
            Value::Point(vec![len(0.0), len(0.0), len(0.0)]),
            Value::Point(vec![
                scalar(1.0, DimensionVector::MASS),
                scalar(1.0, DimensionVector::MASS),
                scalar(1.0, DimensionVector::MASS),
            ]),
        );
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_two_components_undef() {
        // A 2-component corner is not a 3D extent.
        let part = bbox_pts(
            Value::Point(vec![len(0.0), len(0.0)]),
            Value::Point(vec![len(0.010), len(0.010)]),
        );
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_nan_corner_undef() {
        // A NaN corner must not silently produce a bogus comparison (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [f64::NAN, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_inf_corner_undef() {
        // An infinite corner is not a valid finite extent (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [f64::INFINITY, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_invalid_third_arg_real_undef() {
        // 3rd arg must be a DFMSeverity enum or a DFMRule structure-instance (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, Value::Real(1.0)]),
            Some(Value::Undef)
        );
    }

    #[test]
    fn fits_build_volume_invalid_third_arg_wrong_enum_undef() {
        // An enum of the wrong type is not a DFMSeverity (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let wrong = Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() };
        assert_eq!(eval_dfm("fits_build_volume", &[part, env, wrong]), Some(Value::Undef));
    }

    // Guard: a VALID 3rd-arg severity tag must NOT cause Undef — it is ignored by
    // the boolean compute, so step-4 must not over-reject it.
    #[test]
    fn fits_build_volume_valid_severity_enum_third_arg_computes() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, dfm_sev("Warning")]),
            Some(Value::Bool(true))
        );
    }

    #[test]
    fn fits_build_volume_valid_rule_third_arg_computes() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, dfm_rule("Error")]),
            Some(Value::Bool(true))
        );
    }

    // ─── step-5: anti-orphan — fits_build_volume routes through eval_builtin ─

    #[test]
    fn eval_builtin_routes_fits_build_volume_true() {
        // Routes through the PUBLIC `crate::eval_builtin` (not `eval_dfm`): proves
        // the `dfm::eval_dfm` arm is wired into the dispatch chain at runtime —
        // stronger than a grep. A fitting pair must resolve to `Bool(true)`.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(crate::eval_builtin("fits_build_volume", &[part, env]), Value::Bool(true));
    }

    #[test]
    fn eval_builtin_routes_fits_build_volume_false() {
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(crate::eval_builtin("fits_build_volume", &[part, env]), Value::Bool(false));
    }

    // ─── step-7: diagnose success path (DFMSeverity → Severity bridge) ──────

    /// A non-fitting part/envelope pair: the part's X-extent (30 mm) exceeds the
    /// envelope (20 mm), so `fits_build_volume` evaluates to `Bool(false)` — a
    /// build-volume VIOLATION that `diagnose` classifies on the success path.
    fn nonfitting_pair() -> (Value, Value) {
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        (part, env)
    }

    #[test]
    fn diagnose_violation_warning_severity() {
        // Bool(false) + DFMSeverity.Warning → exactly one Warning diagnostic.
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Warning")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1, "one violation diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("W_DFM"),
            "Warning message carries the W_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_violation_error_severity() {
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Error")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM"),
            "Error message carries the E_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_violation_info_severity() {
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Info")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(
            diags[0].message.contains("I_DFM"),
            "Info message carries the I_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_violation_rule_form_reads_severity_field() {
        // The DFMRule structure-instance form: severity is read from its
        // `severity` field (DFMSeverity.Error) → an Error diagnostic.
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_rule("Error")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("E_DFM"), "msg: {}", diags[0].message);
    }

    #[test]
    fn diagnose_violation_defaults_to_warning_when_severity_absent() {
        // No 3rd arg → default Warning (PRD: default Warning when absent).
        let (part, env) = nonfitting_pair();
        let diags = diagnose("fits_build_volume", &[part, env], &Value::Bool(false));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnose_fits_true_emits_no_diagnostic() {
        // A fitting design (Bool(true)) is NOT a violation → empty Vec.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Warning")],
            &Value::Bool(true),
        );
        assert!(diags.is_empty(), "a fitting design surfaces no diagnostic");
    }

    // ─── step-9: diagnose Undef path + non-DFM name guard ──────────────────

    #[test]
    fn diagnose_undef_emits_error_usage_diagnostic() {
        // A `Value::Undef` result is a usage error (non-BoundingBox args, e.g. a
        // raw Solid not resolved via bounding_box(...)): exactly one Error
        // diagnostic, independent of any severity tag (drives step-10).
        let diags = diagnose(
            "fits_build_volume",
            &[Value::Real(1.0), Value::Real(2.0)],
            &Value::Undef,
        );
        assert_eq!(diags.len(), 1, "one usage-error diagnostic");
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM"),
            "usage error carries the E_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_non_dfm_name_undef_is_empty() {
        // A non-DFM builtin name short-circuits to empty even on Undef.
        assert!(diagnose("box", &[], &Value::Undef).is_empty());
    }

    #[test]
    fn diagnose_non_dfm_name_bool_is_empty() {
        // A non-DFM name short-circuits to empty even with a Bool(false) result —
        // the name guard dispatches before the result is inspected.
        assert!(diagnose("stackup_rss", &[], &Value::Bool(false)).is_empty());
    }
}
