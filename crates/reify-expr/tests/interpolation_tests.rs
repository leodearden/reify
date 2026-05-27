//! Interpolation module tests.
//!
//! Pure-function tests for `reify_expr::interp` covering the v0.1 interpolation
//! methods (Linear, NearestNeighbor, Cubic) against analytic interpolation
//! references on regular grids. RBF and Kriging are deferred and exercised via
//! their fall-back-with-warning path.

use reify_expr::interp::{
    InterpolationMethod, InterpolationResult, interpolate_1d, interpolate_2d, interpolate_3d,
};
use reify_core::{DiagnosticCode, Severity};

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
    let r: InterpolationResult = interpolate_1d(InterpolationMethod::Linear, &grid, &values, 0.5);
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
    assert!(
        approx_eq(r_below.value, 10.0, TOL),
        "below got {}",
        r_below.value
    );

    let r_above = interpolate_1d(InterpolationMethod::Linear, &grid, &values, 99.0);
    assert!(
        approx_eq(r_above.value, 30.0, TOL),
        "above got {}",
        r_above.value
    );
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

/// Cell-midpoint exact tie is broken by the even-grid-index tie-breaker — between
/// the two bracketing samples, the endpoint whose grid index is even wins. This is
/// grid-offset-dependent (not `round_ties_even` / banker's rounding on the value).
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

    let r_below = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, -10.0);
    assert!(approx_eq(r_below.value, 10.0, TOL));

    let r_above = interpolate_1d(InterpolationMethod::NearestNeighbor, &grid, &values, 99.0);
    assert!(approx_eq(r_above.value, 30.0, TOL));
}

// ---------------------------------------------------------------------------
// 1D Cubic (4-point Lagrange)
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

/// The 4-point Lagrange cubic reproduces a synthetic cubic polynomial exactly
/// within interior cells (where the full 4-point stencil is available).
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

// ---------------------------------------------------------------------------
// 2D Linear (bilinear)
// ---------------------------------------------------------------------------

/// Build a row-major 2D values vector with shape `(nx, ny)` from a closure.
fn build_2d(grid_x: &[f64], grid_y: &[f64], f: impl Fn(f64, f64) -> f64) -> Vec<f64> {
    let mut v = Vec::with_capacity(grid_x.len() * grid_y.len());
    for &x in grid_x {
        for &y in grid_y {
            v.push(f(x, y));
        }
    }
    v
}

/// Corners of a 2x2 grid reproduce their sample values exactly.
#[test]
fn linear_2d_corners_reproduce_samples() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let values = vec![10.0, 20.0, 30.0, 40.0]; // (0,0)=10 (0,1)=20 (1,0)=30 (1,1)=40

    for (i, &x) in gx.iter().enumerate() {
        for (j, &y) in gy.iter().enumerate() {
            let r = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (x, y));
            let expected = values[i * gy.len() + j];
            assert!(
                approx_eq(r.value, expected, TOL),
                "corner ({},{}) got {}, expected {}",
                i,
                j,
                r.value,
                expected
            );
            assert!(r.diagnostics.is_empty());
        }
    }
}

/// Center of the unit cell `(0.5, 0.5)` is the arithmetic mean of the four
/// corner values.
#[test]
fn linear_2d_unit_cell_center_is_mean() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let values = vec![10.0, 20.0, 30.0, 40.0];
    let r = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (0.5, 0.5));
    let expected = (10.0 + 20.0 + 30.0 + 40.0) / 4.0;
    assert!(approx_eq(r.value, expected, TOL), "got {}", r.value);
}

/// Separability: bilinear at fixed `y` reduces to 1D linear in `x` over the
/// row interpolated to that `y`.
#[test]
fn linear_2d_separable_against_1d_linear() {
    let gx = vec![0.0f64, 1.0, 3.0, 6.0];
    let gy = vec![0.0f64, 2.0, 5.0];
    let f = |x: f64, y: f64| 1.0 + 2.0 * x - 0.5 * y + 0.3 * x * y;
    let values = build_2d(&gx, &gy, f);

    let xs = [0.4, 1.7, 4.0];
    let ys = [0.5, 3.0];
    for &qy in &ys {
        // Build a 1D row by interpolating each grid_x's column at qy.
        let row: Vec<f64> = (0..gx.len())
            .map(|i| {
                let col: Vec<f64> = (0..gy.len()).map(|j| values[i * gy.len() + j]).collect();
                interpolate_1d(InterpolationMethod::Linear, &gy, &col, qy).value
            })
            .collect();

        for &qx in &xs {
            let r2 = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (qx, qy));
            let r1 = interpolate_1d(InterpolationMethod::Linear, &gx, &row, qx).value;
            assert!(
                approx_eq(r2.value, r1, 1e-9),
                "({},{}): 2D={} vs 1D={}",
                qx,
                qy,
                r2.value,
                r1
            );
        }
    }
}

/// Out-of-range queries clamp each axis independently.
#[test]
fn linear_2d_out_of_range_clamps_each_axis() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let values = vec![10.0, 20.0, 30.0, 40.0]; // (0,0) (0,1) (1,0) (1,1)

    // Below-left corner: clamps to (0,0)
    let r1 = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (-5.0, -2.0));
    assert!(approx_eq(r1.value, 10.0, TOL), "got {}", r1.value);

    // Above-right corner: clamps to (1,1)
    let r2 = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (10.0, 12.0));
    assert!(approx_eq(r2.value, 40.0, TOL), "got {}", r2.value);

    // Mixed: x in range, y above → clamp y to last; lerp in x
    let r3 = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (0.5, 10.0));
    let expected = 0.5 * (20.0 + 40.0); // y=1 row: (0,1)=20, (1,1)=40
    assert!(approx_eq(r3.value, expected, TOL), "got {}", r3.value);
}

// ---------------------------------------------------------------------------
// 2D NearestNeighbor
// ---------------------------------------------------------------------------

/// Knot-exact reproduction at every grid point.
#[test]
fn nearest_2d_knot_exact_reproduction() {
    let gx = vec![0.0f64, 1.0, 2.0];
    let gy = vec![0.0f64, 0.5, 1.0];
    let values = build_2d(&gx, &gy, |x, y| 3.0 * x + 7.0 * y - 1.0);
    for (i, &x) in gx.iter().enumerate() {
        for (j, &y) in gy.iter().enumerate() {
            let r = interpolate_2d(
                InterpolationMethod::NearestNeighbor,
                &gx,
                &gy,
                &values,
                (x, y),
            );
            let expected = values[i * gy.len() + j];
            assert!(approx_eq(r.value, expected, TOL), "({},{})", i, j);
            assert!(r.diagnostics.is_empty());
        }
    }
}

/// Quadrant of nearest cell wins: 2x2 grid, query `(0.3, 0.7)` snaps to
/// corner `(0, 1)` (closer in x to 0, closer in y to 1).
#[test]
fn nearest_2d_quadrant_of_nearest_cell_wins() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let values = vec![10.0, 20.0, 30.0, 40.0]; // (0,0)=10 (0,1)=20 (1,0)=30 (1,1)=40
    let r = interpolate_2d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &values,
        (0.3, 0.7),
    );
    assert!(approx_eq(r.value, 20.0, TOL), "got {}", r.value);
}

/// Exact ties on each axis use the even-grid-index tie-breaker independently
/// (grid-offset-dependent, not `round_ties_even` on the value). With grid
/// `[0.0, 1.0, 2.0]` on each axis, query `(0.5, 1.5)` ties on both: x even
/// index wins → 0; y even index wins → 2. Result is `values[(0, 2)]`.
#[test]
fn nearest_2d_axis_ties_independent() {
    let gx = [0.0f64, 1.0, 2.0];
    let gy = [0.0f64, 1.0, 2.0];
    let f = |x: f64, y: f64| x * 10.0 + y;
    let values = build_2d(&gx, &gy, f);
    let r = interpolate_2d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &values,
        (0.5, 1.5),
    );
    let expected = f(0.0, 2.0);
    assert!(approx_eq(r.value, expected, TOL), "got {}", r.value);
}

/// Out-of-range clamps each axis independently to the nearest sample.
#[test]
fn nearest_2d_out_of_range_clamps_each_axis() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let values = vec![10.0, 20.0, 30.0, 40.0];
    let r1 = interpolate_2d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &values,
        (-3.0, -3.0),
    );
    assert!(approx_eq(r1.value, 10.0, TOL));
    let r2 = interpolate_2d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &values,
        (10.0, 0.2),
    );
    assert!(approx_eq(r2.value, 30.0, TOL), "got {}", r2.value);
}

// ---------------------------------------------------------------------------
// 2D Bicubic
// ---------------------------------------------------------------------------

/// Knot-exact reproduction at every grid point on a 5x5 uniform grid.
#[test]
fn cubic_2d_knot_exact_reproduction() {
    let gx: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let gy: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let f = |x: f64, y: f64| 1.0 + x - 2.0 * y + x * y;
    let values = build_2d(&gx, &gy, f);
    for (i, &x) in gx.iter().enumerate() {
        for (j, &y) in gy.iter().enumerate() {
            let r = interpolate_2d(InterpolationMethod::Cubic, &gx, &gy, &values, (x, y));
            let expected = values[i * gy.len() + j];
            assert!(
                approx_eq(r.value, expected, CUBIC_TOL),
                "({},{}) got {}, expected {}",
                i,
                j,
                r.value,
                expected
            );
            assert!(r.diagnostics.is_empty());
        }
    }
}

/// Bicubic recovers a synthetic polynomial of total degree 3 exactly within
/// interior cells (cell indices in `1..=2` on a 5-point grid have a fully
/// available 4x4 stencil).
///
/// Polynomial: f(x,y) = 1 + 2x + 3y + xy + x^2 y - x y^2 + x^3 - y^3.
#[test]
fn cubic_2d_reproduces_total_degree_three_in_interior() {
    let gx: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let gy: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let f = |x: f64, y: f64| {
        1.0 + 2.0 * x + 3.0 * y + x * y + x * x * y - x * y * y + x * x * x - y * y * y
    };
    let values = build_2d(&gx, &gy, f);

    for ci in 1..=2usize {
        for cj in 1..=2usize {
            let xs = [0.1, 0.5, 0.9];
            let ys = [0.1, 0.5, 0.9];
            for &dx in &xs {
                for &dy in &ys {
                    let qx = gx[ci] + dx;
                    let qy = gy[cj] + dy;
                    let r = interpolate_2d(InterpolationMethod::Cubic, &gx, &gy, &values, (qx, qy));
                    let expected = f(qx, qy);
                    assert!(
                        approx_eq(r.value, expected, 1e-9),
                        "cell ({},{}) ({},{}) got {}, expected {}",
                        ci,
                        cj,
                        qx,
                        qy,
                        r.value,
                        expected
                    );
                }
            }
        }
    }
}

/// Separability: bicubic equals tensor product of 1D Cubic — for any
/// query, computing 4 1D-cubics along x (one per bracketing y-row) and
/// then a 1D-cubic in y over those four intermediates must match
/// `interpolate_2d(Cubic, ...)`.
#[test]
fn cubic_2d_separable_against_1d_cubic_tensor_product() {
    let gx: Vec<f64> = (0..6).map(|i| i as f64).collect();
    let gy: Vec<f64> = (0..6).map(|i| i as f64).collect();
    let f = |x: f64, y: f64| (x - 1.0).sin() + (y * 0.5).cos();
    let values = build_2d(&gx, &gy, f);

    let qs = [(2.3f64, 2.7f64), (2.5, 3.5), (1.1, 4.4)];
    for &(qx, qy) in &qs {
        let r2 = interpolate_2d(InterpolationMethod::Cubic, &gx, &gy, &values, (qx, qy));

        // Manual tensor product: for each i, compute a 1D cubic along y of
        // column i; this gives a row of length grid_x.len(); then 1D cubic
        // along x to evaluate at qx.
        let row: Vec<f64> = (0..gx.len())
            .map(|i| {
                let col: Vec<f64> = (0..gy.len()).map(|j| values[i * gy.len() + j]).collect();
                interpolate_1d(InterpolationMethod::Cubic, &gy, &col, qy).value
            })
            .collect();
        let r_tensor = interpolate_1d(InterpolationMethod::Cubic, &gx, &row, qx).value;

        assert!(
            approx_eq(r2.value, r_tensor, 1e-9),
            "({},{}) 2D={} tensor={}",
            qx,
            qy,
            r2.value,
            r_tensor
        );
    }
}

// ---------------------------------------------------------------------------
// 3D Trilinear (Linear method)
// ---------------------------------------------------------------------------

/// Build a row-major 3D values vector with shape `(nx, ny, nz)` from a closure
/// using the layout `values[i * ny * nz + j * nz + k]`.
fn build_3d(
    grid_x: &[f64],
    grid_y: &[f64],
    grid_z: &[f64],
    f: impl Fn(f64, f64, f64) -> f64,
) -> Vec<f64> {
    let mut v = Vec::with_capacity(grid_x.len() * grid_y.len() * grid_z.len());
    for &x in grid_x {
        for &y in grid_y {
            for &z in grid_z {
                v.push(f(x, y, z));
            }
        }
    }
    v
}

/// Corners of a 2x2x2 grid reproduce their sample values exactly.
#[test]
fn linear_3d_corners_reproduce_samples() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let gz = [0.0f64, 1.0];
    let f = |x: f64, y: f64, z: f64| 100.0 * x + 10.0 * y + z;
    let values = build_3d(&gx, &gy, &gz, f);
    let ny = gy.len();
    let nz = gz.len();
    for (i, &x) in gx.iter().enumerate() {
        for (j, &y) in gy.iter().enumerate() {
            for (k, &z) in gz.iter().enumerate() {
                let r = interpolate_3d(
                    InterpolationMethod::Linear,
                    &gx,
                    &gy,
                    &gz,
                    &values,
                    (x, y, z),
                );
                let expected = values[i * ny * nz + j * nz + k];
                assert!(
                    approx_eq(r.value, expected, TOL),
                    "corner ({},{},{}) got {}, expected {}",
                    i,
                    j,
                    k,
                    r.value,
                    expected
                );
                assert!(r.diagnostics.is_empty());
            }
        }
    }
}

/// Center `(0.5, 0.5, 0.5)` of the unit cell is the arithmetic mean of the
/// eight corner values.
#[test]
fn linear_3d_unit_cell_center_is_mean() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let gz = [0.0f64, 1.0];
    // Hand-picked distinct corner values.
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let r = interpolate_3d(
        InterpolationMethod::Linear,
        &gx,
        &gy,
        &gz,
        &values,
        (0.5, 0.5, 0.5),
    );
    let expected = (1.0 + 2.0 + 3.0 + 4.0 + 5.0 + 6.0 + 7.0 + 8.0) / 8.0;
    assert!(approx_eq(r.value, expected, TOL), "got {}", r.value);
}

/// Separability against `interpolate_2d(Linear, ...)` along constant z: build
/// a 2D slice of values at z=qz by interpolating each (i,j) z-column at qz,
/// then evaluate that 2D slice at (qx, qy); the result must match the direct
/// 3D evaluation.
#[test]
fn linear_3d_separable_against_2d_linear_at_constant_z() {
    let gx = vec![0.0f64, 1.0, 3.0];
    let gy = vec![0.0f64, 2.0, 5.0];
    let gz = vec![0.0f64, 1.0, 4.0];
    let f = |x: f64, y: f64, z: f64| 1.0 + 2.0 * x - 0.5 * y + 0.7 * z + 0.1 * x * y - 0.3 * y * z;
    let values = build_3d(&gx, &gy, &gz, f);
    let nx = gx.len();
    let ny = gy.len();
    let nz = gz.len();

    let qz = 0.6;
    // Build a 2D slice of size (nx, ny) at z=qz by interpolating each z-column.
    let mut slice2d = Vec::with_capacity(nx * ny);
    for i in 0..nx {
        for j in 0..ny {
            let col: Vec<f64> = (0..nz).map(|k| values[i * ny * nz + j * nz + k]).collect();
            let v = interpolate_1d(InterpolationMethod::Linear, &gz, &col, qz).value;
            slice2d.push(v);
        }
    }

    let xs = [0.4, 1.7];
    let ys = [0.5, 3.0];
    for &qx in &xs {
        for &qy in &ys {
            let r3 = interpolate_3d(
                InterpolationMethod::Linear,
                &gx,
                &gy,
                &gz,
                &values,
                (qx, qy, qz),
            );
            let r2 =
                interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &slice2d, (qx, qy)).value;
            assert!(
                approx_eq(r3.value, r2, 1e-9),
                "({},{},{}): 3D={} vs 2D-on-slice={}",
                qx,
                qy,
                qz,
                r3.value,
                r2
            );
        }
    }
}

/// Out-of-range queries clamp each axis independently.
#[test]
fn linear_3d_out_of_range_clamps_each_axis() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let gz = [0.0f64, 1.0];
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

    // All three below: clamp to corner (0,0,0)
    let r1 = interpolate_3d(
        InterpolationMethod::Linear,
        &gx,
        &gy,
        &gz,
        &values,
        (-1.0, -1.0, -1.0),
    );
    assert!(approx_eq(r1.value, 1.0, TOL), "below got {}", r1.value);

    // All three above: clamp to corner (1,1,1)
    let r2 = interpolate_3d(
        InterpolationMethod::Linear,
        &gx,
        &gy,
        &gz,
        &values,
        (10.0, 10.0, 10.0),
    );
    assert!(approx_eq(r2.value, 8.0, TOL), "above got {}", r2.value);

    // Mixed: x in range, y above, z below — clamp y→1 and z→0; lerp in x.
    // Face at (y=1, z=0) has corners values[(0,1,0)]=3 and values[(1,1,0)]=7.
    let r3 = interpolate_3d(
        InterpolationMethod::Linear,
        &gx,
        &gy,
        &gz,
        &values,
        (0.5, 10.0, -2.0),
    );
    let expected = 0.5 * (3.0 + 7.0);
    assert!(approx_eq(r3.value, expected, TOL), "got {}", r3.value);
}

// ---------------------------------------------------------------------------
// 3D NearestNeighbor
// ---------------------------------------------------------------------------

/// Knot-exact reproduction at every grid point on a 3x3x3 grid.
#[test]
fn nearest_3d_knot_exact_reproduction() {
    let gx = vec![0.0f64, 1.0, 2.0];
    let gy = vec![0.0f64, 0.5, 1.0];
    let gz = vec![0.0f64, 1.0, 3.0];
    let f = |x: f64, y: f64, z: f64| 7.0 * x - 2.0 * y + 3.0 * z + 1.0;
    let values = build_3d(&gx, &gy, &gz, f);
    let ny = gy.len();
    let nz = gz.len();
    for (i, &x) in gx.iter().enumerate() {
        for (j, &y) in gy.iter().enumerate() {
            for (k, &z) in gz.iter().enumerate() {
                let r = interpolate_3d(
                    InterpolationMethod::NearestNeighbor,
                    &gx,
                    &gy,
                    &gz,
                    &values,
                    (x, y, z),
                );
                let expected = values[i * ny * nz + j * nz + k];
                assert!(approx_eq(r.value, expected, TOL), "({},{},{})", i, j, k);
                assert!(r.diagnostics.is_empty());
            }
        }
    }
}

/// Octant of nearest cell wins: 2x2x2 grid, query `(0.3, 0.7, 0.4)` snaps to
/// corner `(0, 1, 0)` (closer in x to 0, closer in y to 1, closer in z to 0).
#[test]
fn nearest_3d_octant_of_nearest_cell_wins() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let gz = [0.0f64, 1.0];
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    // Layout: (i, j, k) → values[i*4 + j*2 + k].
    // (0, 1, 0) → values[0 + 2 + 0] = 3.0
    let r = interpolate_3d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &gz,
        &values,
        (0.3, 0.7, 0.4),
    );
    assert!(approx_eq(r.value, 3.0, TOL), "got {}", r.value);
}

/// Exact ties on each axis use the even-grid-index tie-breaker independently
/// (grid-offset-dependent, not `round_ties_even` on the value). With grid
/// `[0.0, 1.0, 2.0]` on each axis, query `(0.5, 1.5, 0.5)` ties on all three:
/// x even index wins → 0; y even index wins → 2; z even index wins → 0.
/// Result is `values[(0, 2, 0)]`.
#[test]
fn nearest_3d_axis_ties_independent() {
    let gx = [0.0f64, 1.0, 2.0];
    let gy = [0.0f64, 1.0, 2.0];
    let gz = [0.0f64, 1.0, 2.0];
    let f = |x: f64, y: f64, z: f64| x * 100.0 + y * 10.0 + z;
    let values = build_3d(&gx, &gy, &gz, f);
    let r = interpolate_3d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &gz,
        &values,
        (0.5, 1.5, 0.5),
    );
    let expected = f(0.0, 2.0, 0.0);
    assert!(approx_eq(r.value, expected, TOL), "got {}", r.value);
}

/// Out-of-range queries clamp each axis independently.
#[test]
fn nearest_3d_out_of_range_clamps_each_axis() {
    let gx = [0.0f64, 1.0];
    let gy = [0.0f64, 1.0];
    let gz = [0.0f64, 1.0];
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    // All below: corner (0, 0, 0) → 1.0
    let r1 = interpolate_3d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &gz,
        &values,
        (-5.0, -5.0, -5.0),
    );
    assert!(approx_eq(r1.value, 1.0, TOL));
    // x above, y in-range (closer to 1), z below → corner (1, 1, 0) → values[1*4+1*2+0] = 7.0
    let r2 = interpolate_3d(
        InterpolationMethod::NearestNeighbor,
        &gx,
        &gy,
        &gz,
        &values,
        (10.0, 0.7, -2.0),
    );
    assert!(approx_eq(r2.value, 7.0, TOL), "got {}", r2.value);
}

// ---------------------------------------------------------------------------
// 3D Tricubic
// ---------------------------------------------------------------------------

/// Knot-exact reproduction at every grid point on a 5x5x5 uniform grid.
#[test]
fn cubic_3d_knot_exact_reproduction() {
    let gx: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let gy: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let gz: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let f = |x: f64, y: f64, z: f64| 1.0 + x - 2.0 * y + 0.5 * z + x * y - y * z;
    let values = build_3d(&gx, &gy, &gz, f);
    let ny = gy.len();
    let nz = gz.len();
    for (i, &x) in gx.iter().enumerate() {
        for (j, &y) in gy.iter().enumerate() {
            for (k, &z) in gz.iter().enumerate() {
                let r = interpolate_3d(
                    InterpolationMethod::Cubic,
                    &gx,
                    &gy,
                    &gz,
                    &values,
                    (x, y, z),
                );
                let expected = values[i * ny * nz + j * nz + k];
                assert!(
                    approx_eq(r.value, expected, CUBIC_TOL),
                    "({},{},{}) got {}, expected {}",
                    i,
                    j,
                    k,
                    r.value,
                    expected
                );
                assert!(r.diagnostics.is_empty());
            }
        }
    }
}

/// Tricubic recovers a synthetic polynomial of total degree 3 exactly within
/// the interior 2x2x2 block of cells on a 5x5x5 grid (cells indexed by
/// `1..=2` on every axis have a fully available 4x4x4 stencil).
///
/// Polynomial: f(x,y,z) = 1 + 2x - y + 3z + x*y + y*z + x*z + x^2 - y^3 + z^2*x.
///
/// **Separability guard.** The `cubic_3d` algorithm is separable: it first
/// collapses the (y,z)-plane with a 2D cubic kernel, then applies a 1D cubic
/// along x. Exact reproduction of a degree-3 polynomial (guaranteed by
/// 4-point Lagrange exactness) is only achievable if the tensor-product
/// composition is correctly wired — any breakage in the separable structure
/// (wrong axis order, mismatched stencil offsets, etc.) surfaces here as a
/// reproducible numeric mismatch, without the need for a tautological test
/// that mirrors the implementation's own factoring.
#[test]
fn cubic_3d_reproduces_total_degree_three_in_interior() {
    let gx: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let gy: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let gz: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let f = |x: f64, y: f64, z: f64| {
        1.0 + 2.0 * x - y + 3.0 * z + x * y + y * z + x * z + x * x - y * y * y + z * z * x
    };
    let values = build_3d(&gx, &gy, &gz, f);

    for ci in 1..=2usize {
        for cj in 1..=2usize {
            for ck in 1..=2usize {
                for &dx in &[0.1, 0.5, 0.9] {
                    for &dy in &[0.2, 0.5, 0.8] {
                        for &dz in &[0.25, 0.5, 0.75] {
                            let qx = gx[ci] + dx;
                            let qy = gy[cj] + dy;
                            let qz = gz[ck] + dz;
                            let r = interpolate_3d(
                                InterpolationMethod::Cubic,
                                &gx,
                                &gy,
                                &gz,
                                &values,
                                (qx, qy, qz),
                            );
                            let expected = f(qx, qy, qz);
                            assert!(
                                approx_eq(r.value, expected, 1e-9),
                                "cell ({},{},{}) ({},{},{}) got {}, expected {}",
                                ci,
                                cj,
                                ck,
                                qx,
                                qy,
                                qz,
                                r.value,
                                expected
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Larger 4x4 monotone grid: the midpoint of any cell equals the mean of its
/// four corner values.
#[test]
fn linear_2d_4x4_cell_midpoint_is_corner_mean() {
    let gx = vec![0.0f64, 1.0, 2.0, 3.0];
    let gy = vec![0.0f64, 1.0, 2.0, 3.0];
    let f = |x: f64, y: f64| x + 2.0 * y + 0.1 * x * y;
    let values = build_2d(&gx, &gy, f);
    for i in 0..gx.len() - 1 {
        for j in 0..gy.len() - 1 {
            let qx = 0.5 * (gx[i] + gx[i + 1]);
            let qy = 0.5 * (gy[j] + gy[j + 1]);
            let v00 = values[i * gy.len() + j];
            let v01 = values[i * gy.len() + (j + 1)];
            let v10 = values[(i + 1) * gy.len() + j];
            let v11 = values[(i + 1) * gy.len() + (j + 1)];
            let expected = 0.25 * (v00 + v01 + v10 + v11);
            let r = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, (qx, qy));
            assert!(
                approx_eq(r.value, expected, TOL),
                "cell ({},{}) got {}, expected {}",
                i,
                j,
                r.value,
                expected
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Deferred methods (Rbf / Kriging) — fall back to Linear with a single warning
// ---------------------------------------------------------------------------

/// Helper: assert a single deferred-method warning whose message mentions both
/// the method name and "Linear".
fn assert_deferred_warning(diags: &[reify_core::Diagnostic], method_name: &str) {
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic, got {}",
        diags.len()
    );
    let d = &diags[0];
    assert_eq!(d.severity, Severity::Warning, "expected Warning severity");
    assert_eq!(
        d.code,
        Some(DiagnosticCode::InterpolationDeferred),
        "expected InterpolationDeferred code"
    );
    let msg_lower = d.message.to_lowercase();
    let needle_lower = method_name.to_lowercase();
    assert!(
        msg_lower.contains(&needle_lower),
        "message {:?} does not mention {:?}",
        d.message,
        method_name
    );
    assert!(
        d.message.contains("Linear"),
        "message {:?} does not mention 'Linear'",
        d.message
    );
}

/// 1D: `Rbf` and `Kriging` produce the same value as `Linear`, plus a single
/// deferred-method warning that references the chosen method and "Linear".
#[test]
fn deferred_methods_1d_match_linear_and_warn() {
    let grid = [0.0f64, 1.0, 3.0, 6.0];
    let values = [0.0f64, 10.0, 30.0, 90.0];
    let q = 1.7;
    let expected = interpolate_1d(InterpolationMethod::Linear, &grid, &values, q).value;
    for (method, name) in [
        (InterpolationMethod::Rbf, "RBF"),
        (InterpolationMethod::Kriging, "Kriging"),
    ] {
        let r = interpolate_1d(method, &grid, &values, q);
        assert_eq!(r.value, expected, "{name} 1D value must equal Linear");
        assert_deferred_warning(&r.diagnostics, name);
    }
}

/// 2D: `Rbf` and `Kriging` produce the same value as `Linear`, plus a single
/// deferred-method warning.
#[test]
fn deferred_methods_2d_match_linear_and_warn() {
    let gx = vec![0.0f64, 1.0, 3.0];
    let gy = vec![0.0f64, 2.0, 5.0];
    let f = |x: f64, y: f64| 1.0 + 2.0 * x - 0.5 * y + 0.3 * x * y;
    let values = build_2d(&gx, &gy, f);
    let q = (1.4f64, 3.7f64);
    let expected = interpolate_2d(InterpolationMethod::Linear, &gx, &gy, &values, q).value;
    for (method, name) in [
        (InterpolationMethod::Rbf, "RBF"),
        (InterpolationMethod::Kriging, "Kriging"),
    ] {
        let r = interpolate_2d(method, &gx, &gy, &values, q);
        assert_eq!(r.value, expected, "{name} 2D value must equal Linear");
        assert_deferred_warning(&r.diagnostics, name);
    }
}

/// 3D: `Rbf` and `Kriging` produce the same value as `Linear`, plus a single
/// deferred-method warning.
#[test]
fn deferred_methods_3d_match_linear_and_warn() {
    let gx = vec![0.0f64, 1.0, 3.0];
    let gy = vec![0.0f64, 2.0, 5.0];
    let gz = vec![0.0f64, 1.0, 4.0];
    let f = |x: f64, y: f64, z: f64| 1.0 + 2.0 * x - 0.5 * y + 0.7 * z + 0.1 * x * y - 0.3 * y * z;
    let values = build_3d(&gx, &gy, &gz, f);
    let q = (1.4f64, 3.7f64, 2.0f64);
    let expected = interpolate_3d(InterpolationMethod::Linear, &gx, &gy, &gz, &values, q).value;
    for (method, name) in [
        (InterpolationMethod::Rbf, "RBF"),
        (InterpolationMethod::Kriging, "Kriging"),
    ] {
        let r = interpolate_3d(method, &gx, &gy, &gz, &values, q);
        assert_eq!(r.value, expected, "{name} 3D value must equal Linear");
        assert_deferred_warning(&r.diagnostics, name);
    }
}

/// Non-deferred methods (Linear, NearestNeighbor, Cubic) produce empty
/// `diagnostics` vectors across all three dimensions — sanity-check that
/// the deferred-method warning is the *only* diagnostic source.
#[test]
fn non_deferred_methods_produce_no_diagnostics() {
    // 1D
    let g1 = [0.0f64, 1.0, 2.0];
    let v1 = [10.0f64, 20.0, 30.0];
    for m in [
        InterpolationMethod::Linear,
        InterpolationMethod::NearestNeighbor,
        InterpolationMethod::Cubic,
    ] {
        let r = interpolate_1d(m, &g1, &v1, 0.5);
        assert!(r.diagnostics.is_empty(), "1D {:?} emitted diagnostics", m);
    }

    // 2D
    let g2x = [0.0f64, 1.0, 2.0];
    let g2y = [0.0f64, 1.0, 2.0];
    let v2 = build_2d(&g2x, &g2y, |x, y| x + y);
    for m in [
        InterpolationMethod::Linear,
        InterpolationMethod::NearestNeighbor,
        InterpolationMethod::Cubic,
    ] {
        let r = interpolate_2d(m, &g2x, &g2y, &v2, (0.5, 0.5));
        assert!(r.diagnostics.is_empty(), "2D {:?} emitted diagnostics", m);
    }

    // 3D
    let g3x = [0.0f64, 1.0, 2.0];
    let g3y = [0.0f64, 1.0, 2.0];
    let g3z = [0.0f64, 1.0, 2.0];
    let v3 = build_3d(&g3x, &g3y, &g3z, |x, y, z| x + y + z);
    for m in [
        InterpolationMethod::Linear,
        InterpolationMethod::NearestNeighbor,
        InterpolationMethod::Cubic,
    ] {
        let r = interpolate_3d(m, &g3x, &g3y, &g3z, &v3, (0.5, 0.5, 0.5));
        assert!(r.diagnostics.is_empty(), "3D {:?} emitted diagnostics", m);
    }
}

// ---------------------------------------------------------------------------
// NaN-propagation tests
// ---------------------------------------------------------------------------

/// A NaN query in 3D must produce a NaN value with no diagnostics, for all
/// three v0.1 methods. Any component being NaN poisons the result (IEEE 754
/// NaN-poisoning convention).
#[test]
fn nan_query_3d_returns_nan_with_no_diagnostics() {
    let gx = [0.0f64, 1.0, 2.0];
    let gy = [0.0f64, 1.0, 2.0];
    let gz = [0.0f64, 1.0, 2.0];
    let values = build_3d(&gx, &gy, &gz, |x, y, z| x + y + z);
    let queries: &[(f64, f64, f64)] = &[
        (f64::NAN, 0.5, 0.5),
        (0.5, f64::NAN, 0.5),
        (0.5, 0.5, f64::NAN),
        (f64::NAN, f64::NAN, f64::NAN),
    ];
    for m in [
        InterpolationMethod::Linear,
        InterpolationMethod::NearestNeighbor,
        InterpolationMethod::Cubic,
    ] {
        for &q in queries {
            let r = interpolate_3d(m, &gx, &gy, &gz, &values, q);
            assert!(
                r.value.is_nan(),
                "3D {:?} query {:?}: expected NaN, got {}",
                m,
                q,
                r.value
            );
            assert!(
                r.diagnostics.is_empty(),
                "3D {:?} query {:?}: expected empty diagnostics, got {:?}",
                m,
                q,
                r.diagnostics
            );
        }
    }
}

/// A NaN query in 2D must produce a NaN value with no diagnostics, for all
/// three v0.1 methods. Any component being NaN poisons the result (IEEE 754
/// NaN-poisoning convention).
#[test]
fn nan_query_2d_returns_nan_with_no_diagnostics() {
    let gx = [0.0f64, 1.0, 2.0];
    let gy = [0.0f64, 1.0, 2.0];
    let values = build_2d(&gx, &gy, |x, y| x + y);
    let queries: &[(f64, f64)] = &[(f64::NAN, 0.5), (0.5, f64::NAN), (f64::NAN, f64::NAN)];
    for m in [
        InterpolationMethod::Linear,
        InterpolationMethod::NearestNeighbor,
        InterpolationMethod::Cubic,
    ] {
        for &q in queries {
            let r = interpolate_2d(m, &gx, &gy, &values, q);
            assert!(
                r.value.is_nan(),
                "2D {:?} query {:?}: expected NaN, got {}",
                m,
                q,
                r.value
            );
            assert!(
                r.diagnostics.is_empty(),
                "2D {:?} query {:?}: expected empty diagnostics, got {:?}",
                m,
                q,
                r.diagnostics
            );
        }
    }
}

/// A NaN query in 1D must produce a NaN value with no diagnostics, for all
/// three v0.1 methods (Linear, NearestNeighbor, Cubic).
///
/// IEEE 754 NaN-poisoning convention: the result is NaN when the query is NaN.
/// No diagnostic is emitted (consistent with the silent constant-extrapolation
/// policy for out-of-range queries).
#[test]
fn nan_query_1d_returns_nan_with_no_diagnostics() {
    let grid = [0.0f64, 1.0, 3.0, 6.0];
    let values = [0.0f64, 10.0, 30.0, 90.0];
    for m in [
        InterpolationMethod::Linear,
        InterpolationMethod::NearestNeighbor,
        InterpolationMethod::Cubic,
    ] {
        let r = interpolate_1d(m, &grid, &values, f64::NAN);
        assert!(
            r.value.is_nan(),
            "1D {:?}: expected NaN, got {}",
            m,
            r.value
        );
        assert!(
            r.diagnostics.is_empty(),
            "1D {:?}: expected empty diagnostics, got {:?}",
            m,
            r.diagnostics
        );
    }
}

/// Deferred methods (Rbf, Kriging) with a NaN query return NaN with *empty*
/// diagnostics — the NaN short-circuit fires before deferred-method resolution,
/// so no deferred-method warning is emitted. This locks down the chosen
/// behaviour described in the `interpolate_1d` doc comment.
#[test]
fn nan_query_1d_deferred_method_returns_nan_no_warning() {
    let grid = [0.0f64, 1.0, 3.0, 6.0];
    let values = [0.0f64, 10.0, 30.0, 90.0];
    for m in [InterpolationMethod::Rbf, InterpolationMethod::Kriging] {
        let r = interpolate_1d(m, &grid, &values, f64::NAN);
        assert!(
            r.value.is_nan(),
            "1D deferred {:?}: expected NaN, got {}",
            m,
            r.value
        );
        assert!(
            r.diagnostics.is_empty(),
            "1D deferred {:?}: expected empty diagnostics (no deferred warning), got {:?}",
            m,
            r.diagnostics
        );
    }
}
