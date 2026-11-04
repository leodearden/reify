//! The single toleranced Scalar-equality policy (task #6653).
//!
//! Shared by the language's `==` / `!=` verdict (`reify_expr::eval_eq`) and
//! by the DimensionalSolver's acceptance of equality rows, so a solved model
//! never reports its own equalities violated. Two SI magnitudes are equal
//! when they agree within a relative tolerance of the larger magnitude or
//! within a dimension-aware absolute floor, whichever is looser. Ordering
//! comparisons are NOT toleranced. [`equality_band`] states the same policy
//! as a range, for reify-constraints' interval refutation (`hc4`).

use crate::dimension::DimensionVector;

const RELATIVE_TOLERANCE: f64 = 1e-9;

/// The `==` verdict's absolute length floor, in metres.
pub const LENGTH_FLOOR_M: f64 = 1e-6;

/// The radius, in metres, at which an angle is read as a length.
pub const ANGULAR_REFERENCE_RADIUS_M: f64 = 1.0;

/// The solver's metre-row tolerance floor, in metres. Distinct from
/// [`LENGTH_FLOOR_M`]: it only keeps a scale-relative solver bar off zero for
/// sub-millimetre clusters (PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md`
/// §7.3).
pub const SOLVER_LENGTH_FLOOR_M: f64 = 1e-9;

/// Absolute floor for every dimension without a curated resolution, and the
/// minimum floor of every dimension.
pub const DEFAULT_ABSOLUTE_FLOOR: f64 = 1e-12;

fn absolute_floor(dimension: DimensionVector) -> f64 {
    if dimension == DimensionVector::LENGTH {
        LENGTH_FLOOR_M
    } else if dimension == DimensionVector::ANGLE {
        LENGTH_FLOOR_M / ANGULAR_REFERENCE_RADIUS_M
    } else {
        DEFAULT_ABSOLUTE_FLOOR
    }
}

/// The largest difference the policy accepts between two magnitudes whose
/// larger absolute value is `magnitude`.
fn tolerance(magnitude: f64, dimension: DimensionVector) -> f64 {
    (RELATIVE_TOLERANCE * magnitude).max(absolute_floor(dimension))
}

/// Whether two SI magnitudes of the given dimension are equal under the
/// policy. Non-finite operands compare exactly.
pub fn scalars_equal(a: f64, b: f64, dimension: DimensionVector) -> bool {
    if a == b {
        return true;
    }
    if !(a.is_finite() && b.is_finite()) {
        return false;
    }
    (a - b).abs() <= tolerance(a.abs().max(b.abs()), dimension)
}

/// A range holding every `a` that [`scalars_equal`] accepts against SOME
/// `b ∈ [lo, hi]`; an infinite endpoint passes through.
///
/// A partner `a` accepted against `b` has `|a| <= (|b| + floor) / (1 − rel)`,
/// which bounds the tolerance it was accepted under. `b` plus that tolerance
/// is increasing in `b`, so each edge of the band is set by its own endpoint.
/// The tolerance and the edge are each widened one ulp outward, covering the
/// rounding of both this computation and the comparison it encloses.
pub fn equality_band(lo: f64, hi: f64, dimension: DimensionVector) -> (f64, f64) {
    let edge_tolerance = |b: f64| {
        let partner = (b.abs() + absolute_floor(dimension)) / (1.0 - RELATIVE_TOLERANCE);
        tolerance(partner, dimension).next_up()
    };
    let lower = if lo.is_finite() {
        (lo - edge_tolerance(lo)).next_down()
    } else {
        lo
    };
    let upper = if hi.is_finite() {
        (hi + edge_tolerance(hi)).next_up()
    } else {
        hi
    };
    (lower, upper)
}
