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

// ---------------------------------------------------------------------------
// 1D NearestNeighbor
// ---------------------------------------------------------------------------

/// Knot-exact: querying at every grid point reproduces the corresponding
/// sample value exactly.
#[test]
fn nearest_1d_knot_exact_reproduction() {
    let grid = [0.0f64, 1.0, 2.5, 4.0];
    let values = [3.0f64, 7.0, 11.0, 13.0];
    for (i, &x) in grid.iter().enumerate() {
        let r = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, x);
        assert!(
            approx_eq(r.value, values[i], TOL),
            "knot {} got {}",
            i,
            r.value
        );
        assert!(r.diagnostics.is_empty());
    }
}

/// Cell-midpoint exact tie is broken by `round_ties_even` — choose the
/// endpoint with the even index.
///
/// Grid `[0.0, 1.0, 2.0]` values `[10.0, 20.0, 30.0]`:
/// - query `0.5` is exactly between indices 0 and 1 → even index wins → 0 → `10.0`.
/// - query `1.5` is exactly between indices 1 and 2 → even index wins → 2 → `30.0`.
#[test]
fn nearest_1d_midpoint_tie_breaks_even() {
    let grid = [0.0f64, 1.0, 2.0];
    let values = [10.0f64, 20.0, 30.0];

    let r_low = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 0.5);
    assert!(approx_eq(r_low.value, 10.0, TOL), "0.5 → {}", r_low.value);

    let r_high = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 1.5);
    assert!(approx_eq(r_high.value, 30.0, TOL), "1.5 → {}", r_high.value);
}

/// Sub-midpoint queries snap to the closer sample.
#[test]
fn nearest_1d_sub_midpoint_picks_closer() {
    let grid = [0.0f64, 1.0, 2.0];
    let values = [10.0f64, 20.0, 30.0];

    // 0.4 is closer to 0.0 than to 1.0 → 10.0
    let r1 = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 0.4);
    assert!(approx_eq(r1.value, 10.0, TOL), "0.4 → {}", r1.value);

    // 0.6 is closer to 1.0 than to 0.0 → 20.0
    let r2 = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 0.6);
    assert!(approx_eq(r2.value, 20.0, TOL), "0.6 → {}", r2.value);

    // 1.4 is closer to 1.0 than to 2.0 → 20.0
    let r3 = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 1.4);
    assert!(approx_eq(r3.value, 20.0, TOL), "1.4 → {}", r3.value);

    // 1.7 is closer to 2.0 than to 1.0 → 30.0
    let r4 = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 1.7);
    assert!(approx_eq(r4.value, 30.0, TOL), "1.7 → {}", r4.value);
}

/// Out-of-range queries clamp to the nearest endpoint sample.
#[test]
fn nearest_1d_out_of_range_clamps_to_endpoint() {
    let grid = [0.0f64, 1.0, 2.0];
    let values = [10.0f64, 20.0, 30.0];

    let r_below =
        interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, -10.0);
    assert!(approx_eq(r_below.value, 10.0, TOL));

    let r_above =
        interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 99.0);
    assert!(approx_eq(r_above.value, 30.0, TOL));
}

// ---------------------------------------------------------------------------
// 1D Cubic (Catmull-Rom)
// ---------------------------------------------------------------------------

const CUBIC_TOL: f64 = 1e-10;

/// Knot-exact: querying at every grid point reproduces the corresponding
/// sample value exactly on a 6-point uniform grid.
#[test]
fn cubic_1d_knot_exact_reproduction_uniform() {
    let grid: Vec<f64> = (0..6).map(|i| i as f64).collect();
    let values: Vec<f64> = vec![1.0, 5.0, -2.0, 3.5, 0.0, 7.25];
    for (i, &x) in grid.iter().enumerate() {
        let r = interpolate_1d(InterpolationMethod::Cubic, &grid, &values, x);
        assert!(
            approx_eq(r.value, values[i], CUBIC_TOL),
            "knot {} got {}, expected {}",
            i,
            r.value,
            values[i]
        );
        assert!(r.diagnostics.is_empty());
    }
}

/// Catmull-Rom reproduces a synthetic cubic polynomial exactly within
/// interior cells (where the 4-point stencil is fully available).
///
/// Polynomial: f(x) = 2 - 3*x + 1.5*x^2 - 0.4*x^3.
#[test]
fn cubic_1d_reproduces_cubic_polynomial_in_interior() {
    let f = |x: f64| 2.0 - 3.0 * x + 1.5 * x * x - 0.4 * x * x * x;
    let grid: Vec<f64> = (0..8).map(|i| i as f64).collect();
    let values: Vec<f64> = grid.iter().copied().map(f).collect();

    // Interior cells: i=1..=5 are fully bracketed by valid stencil neighbours.
    // Sample several non-knot positions inside each.
    for cell in 1..=5 {
        let xs = [
            grid[cell] + 0.1,
            grid[cell] + 0.25,
            grid[cell] + 0.5,
            grid[cell] + 0.75,
            grid[cell] + 0.9,
        ];
        for &x in &xs {
            let r = interpolate_1d(InterpolationMethod::Cubic, &grid, &values, x);
            let expected = f(x);
            assert!(
                approx_eq(r.value, expected, CUBIC_TOL),
                "cell {} x={} got {}, expected {}",
                cell,
                x,
                r.value,
                expected
            );
        }
    }
}

/// Edge-cell behaviour with linear-extrapolated ghost points: on a 4-point
/// grid the first and last cells still reproduce both endpoint sample values
/// exactly when queried at the knots.
#[test]
fn cubic_1d_edge_cell_endpoints_reproduce_samples() {
    let grid = [0.0f64, 1.0, 2.0, 3.0];
    let values = [1.0f64, 4.0, 9.0, 16.0];
    for (i, &x) in grid.iter().enumerate() {
        let r = interpolate_1d(InterpolationMethod::Cubic, &grid, &values, x);
        assert!(
            approx_eq(r.value, values[i], CUBIC_TOL),
            "knot {} got {}",
            i,
            r.value
        );
    }
}

/// Degenerate 2-point grid: Cubic collapses to Linear because both ghost
/// points are linear extrapolations of the only cell's endpoints.
#[test]
fn cubic_1d_two_point_grid_matches_linear() {
    let grid = [0.0f64, 1.0];
    let values = [10.0f64, 30.0];
    for &x in &[0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
        let cubic = interpolate_1d(InterpolationMethod::Cubic, &grid, &values, x);
        let linear = interpolate_1d(InterpolationMethod::Linear, &grid, &values, x);
        assert!(
            approx_eq(cubic.value, linear.value, CUBIC_TOL),
            "x={} cubic={} linear={}",
            x,
            cubic.value,
            linear.value
        );
    }
}
