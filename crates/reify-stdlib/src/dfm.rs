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
use reify_core::Diagnostic;

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
    // The optional 3rd arg (DFMSeverity / DFMRule) tags the violation severity for
    // `diagnose`; it does not affect the fit. It is validated in step-4.
    let fits = (0..3).all(|i| part[i] <= envelope[i]);
    Value::Bool(fits)
}

/// Extract the per-axis extents `[x, y, z] = max - min` from a `Value::BoundingBox`.
///
/// Returns `None` for a non-`BoundingBox`, or a box whose `min`/`max` are not
/// 3-component Point/Vector/Tensor numerics (via [`tensor_components_f64`], which
/// also rejects mixed-dimension corners). LENGTH-dimension and finiteness guards
/// are added in step-4.
fn parse_bbox_extents(v: &Value) -> Option<[f64; 3]> {
    let (min, max) = match v {
        Value::BoundingBox { min, max } => (min, max),
        _ => return None,
    };
    let (min_vals, _min_dim) = tensor_components_f64(min)?;
    let (max_vals, _max_dim) = tensor_components_f64(max)?;
    if min_vals.len() != 3 || max_vals.len() != 3 {
        return None;
    }
    Some([
        max_vals[0] - min_vals[0],
        max_vals[1] - min_vals[1],
        max_vals[2] - min_vals[2],
    ])
}

/// Pure post-call DFM diagnostic classifier (the `DFMSeverity` bridge).
///
/// Mirrors [`crate::flexures::flexure_diagnose`]: returns a `Vec<Diagnostic>`, fires on
/// BOTH the success and `Value::Undef` paths, and short-circuits to an empty `Vec` for
/// any non-DFM `name`.
pub fn diagnose(_name: &str, _args: &[Value], _result: &Value) -> Vec<Diagnostic> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

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
}
