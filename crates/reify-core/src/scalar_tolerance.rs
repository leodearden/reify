//! The single toleranced Scalar-equality policy (task #6653).
//!
//! Shared by the language's `==` / `!=` verdict (`reify_expr::eval_eq`) and
//! by the DimensionalSolver's acceptance of equality rows, so a solved model
//! never reports its own equalities violated. Two SI magnitudes are equal
//! when they agree within a relative tolerance of the larger magnitude or
//! within a dimension-aware absolute floor, whichever is looser. Ordering
//! comparisons are NOT toleranced.

use crate::dimension::DimensionVector;

const RELATIVE_TOLERANCE: f64 = 1e-9;
const LENGTH_FLOOR_M: f64 = 1e-6;
const ANGULAR_REFERENCE_RADIUS_M: f64 = 1.0;

/// Absolute floor for every dimension without a curated resolution, and the
/// minimum floor of every dimension. The solver aliases it as its inequality
/// acceptance slack.
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

/// Whether two SI magnitudes of the given dimension are equal under the
/// policy. Non-finite operands compare exactly.
pub fn scalars_equal(a: f64, b: f64, dimension: DimensionVector) -> bool {
    if a == b {
        return true;
    }
    if !(a.is_finite() && b.is_finite()) {
        return false;
    }
    let relative = RELATIVE_TOLERANCE * a.abs().max(b.abs());
    (a - b).abs() <= relative.max(absolute_floor(dimension))
}
