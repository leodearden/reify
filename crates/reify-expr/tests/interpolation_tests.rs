//! Interpolation module tests.
//!
//! Pure-function tests for `reify_expr::interp` covering the v0.1 interpolation
//! methods (Linear, NearestNeighbor, Cubic) against analytic interpolation
//! references on regular grids. RBF and Kriging are deferred and exercised via
//! their fall-back-with-warning path.

use reify_expr::interp::{InterpolationMethod, InterpolationResult, interpolate_1d};

const TOL: f64 = 1e-12;

fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---------------------------------------------------------------------------
// 1D Linear
// ---------------------------------------------------------------------------

/// Two-point grid `[0.0, 1.0]` with values `[10.0, 20.0]`, query `0.5` returns
/// the arithmetic mean `15.0` — the textbook lerp.
#[test]
fn linear_1d_two_point_midpoint_is_mean() {
    let grid = [0.0f64, 1.0];
    let values = [10.0f64, 20.0];
    let r: InterpolationResult =
        interpolate_1d(InterpolationMethod::Linear, &grid, &values, 0.5);
    assert!(approx_eq(r.value, 15.0, TOL), "got {}", r.value);
    assert!(r.diagnostics.is_empty(), "linear emits no diagnostics");
}

/// Knot-exact: querying at every grid point reproduces the corresponding
/// sample value exactly.
#[test]
fn linear_1d_knot_exact_reproduction() {
    let grid = [0.0f64, 0.25, 0.75, 1.0];
    let values = [3.0f64, 7.0, 11.0, 13.0];
    for (i, &x) in grid.iter().enumerate() {
        let r = interpolate_1d(InterpolationMethod::Linear, &grid, &values, x);
        assert!(
            approx_eq(r.value, values[i], TOL),
            "knot {} (x={}) got {}, expected {}",
            i,
            x,
            r.value,
            values[i]
        );
    }
}

/// Multi-cell linear interior: 4-point grid with monotone values; midpoint of
/// each cell is the arithmetic mean of the two cell endpoints.
#[test]
fn linear_1d_multi_cell_midpoint_is_cell_mean() {
    let grid = [0.0f64, 1.0, 3.0, 6.0];
    let values = [0.0f64, 10.0, 30.0, 90.0];
    for i in 0..grid.len() - 1 {
        let mid = 0.5 * (grid[i] + grid[i + 1]);
        let expected = 0.5 * (values[i] + values[i + 1]);
        let r = interpolate_1d(InterpolationMethod::Linear, &grid, &values, mid);
        assert!(
            approx_eq(r.value, expected, TOL),
            "cell {} mid={} got {}, expected {}",
            i,
            mid,
            r.value,
            expected
        );
    }
}

/// Out-of-range queries clamp to the nearest endpoint sample (constant
/// extrapolation).
#[test]
fn linear_1d_out_of_range_clamps_to_endpoint() {
    let grid = [0.0f64, 1.0, 2.0];
    let values = [10.0f64, 20.0, 30.0];

    let r_below = interpolate_1d(InterpolationMethod::Linear, &grid, &values, -1.5);
    assert!(approx_eq(r_below.value, 10.0, TOL), "below got {}", r_below.value);

    let r_above = interpolate_1d(InterpolationMethod::Linear, &grid, &values, 99.0);
    assert!(approx_eq(r_above.value, 30.0, TOL), "above got {}", r_above.value);
}
