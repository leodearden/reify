//! Integration tests for the Featherstone 6D spatial-vector core
//! (`reify_stdlib::dynamics::spatial`).
//!
//! Mirrors the `tests/complex_tests.rs` layout: top-of-file `use`, per-behavior
//! `mod` blocks, shared tolerance/entrywise-equality helpers at the top.
//!
//! Convention (Featherstone 2008, §2.4): spatial vectors are ordered
//! `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` — angular first, linear second. 6×6
//! matrices are row-major `[f64; 36]`.

use reify_stdlib::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

// ── Shared helpers (modeled on complex_tests.rs::assert_complex_eq) ──────────

/// Tolerance for closed-form / single-op matrix comparisons.
const TOL_TIGHT: f64 = 1e-15;
/// Tolerance for composed / multi-op numeric comparisons (Featherstone-canonical).
const TOL_NUMERIC: f64 = 1e-12;

/// Row-major 6×6 identity.
fn identity6() -> [f64; 36] {
    let mut m = [0.0; 36];
    for i in 0..6 {
        m[i * 6 + i] = 1.0;
    }
    m
}

/// Assemble a row-major 6×6 from four 3×3 blocks `[[tl, tr]; [bl, br]]`.
fn block6(
    tl: [[f64; 3]; 3],
    tr: [[f64; 3]; 3],
    bl: [[f64; 3]; 3],
    br: [[f64; 3]; 3],
) -> [f64; 36] {
    let mut m = [0.0; 36];
    for r in 0..3 {
        for c in 0..3 {
            m[r * 6 + c] = tl[r][c];
            m[r * 6 + (c + 3)] = tr[r][c];
            m[(r + 3) * 6 + c] = bl[r][c];
            m[(r + 3) * 6 + (c + 3)] = br[r][c];
        }
    }
    m
}

/// Entrywise approximate-equality assertion for a row-major 6×6 matrix, with a
/// per-element diagnostic on mismatch (mirrors `assert_complex_eq`'s style).
fn assert_mat6_eq(actual: &[f64; 36], expected: &[f64; 36], tol: f64) {
    for i in 0..6 {
        for j in 0..6 {
            let a = actual[i * 6 + j];
            let e = expected[i * 6 + j];
            assert!(
                (a - e).abs() < tol,
                "entry [{i},{j}]: expected {e}, got {a} (|Δ|={:e}, tol={:e})",
                (a - e).abs(),
                tol
            );
        }
    }
}

mod spatial_vector6 {
    use super::*;

    #[test]
    fn zero_is_six_zeros() {
        let z = SpatialVector6::zero();
        assert_eq!(z.as_array(), [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn from_array_round_trips_via_as_array() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let v = SpatialVector6::from_array(a);
        assert_eq!(v.as_array(), a);
    }

    #[test]
    fn angular_is_first_three_linear_is_last_three() {
        let v = SpatialVector6::from_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(v.angular(), [1.0, 2.0, 3.0]);
        assert_eq!(v.linear(), [4.0, 5.0, 6.0]);
    }

    #[test]
    fn from_angular_linear_round_trips() {
        let v = SpatialVector6::from_angular_linear([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
        assert_eq!(v.as_array(), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(v.angular(), [1.0, 2.0, 3.0]);
        assert_eq!(v.linear(), [4.0, 5.0, 6.0]);
    }
}

mod frame3 {
    use super::*;

    #[test]
    fn identity_is_unit_quat_w_first_and_zero_translation() {
        let f = Frame3::identity();
        assert_eq!(f.rotation(), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(f.translation(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn new_round_trips_through_getters() {
        let rot = [0.5, 0.5, 0.5, 0.5];
        let trans = [1.5, -2.25, 7.0];
        let f = Frame3::new(rot, trans);
        assert_eq!(f.rotation(), rot);
        assert_eq!(f.translation(), trans);
    }

    #[test]
    fn identical_components_compare_equal() {
        let a = Frame3::new([0.0, 1.0, 0.0, 0.0], [3.0, 4.0, 5.0]);
        let b = Frame3::new([0.0, 1.0, 0.0, 0.0], [3.0, 4.0, 5.0]);
        assert_eq!(a, b);
    }
}

mod from_frame3 {
    use super::*;

    #[test]
    fn identity_frame_is_6x6_identity() {
        let x = SpatialTransform6::from_frame3(&Frame3::identity());
        assert_mat6_eq(&x.as_matrix(), &identity6(), TOL_TIGHT);
    }

    #[test]
    fn pure_translation_is_block_lower_triangular() {
        // Featherstone Eq. 2.24: X(r, E) = [[E, 0]; [-r̃·E, E]].
        // identity rotation ⇒ E = I_3, so bottom-left = -r̃.
        // r = [1, 2, 3] ⇒ r̃ = [[0,-3,2],[3,0,-1],[-2,1,0]],
        //               so -r̃ = [[0,3,-2],[-3,0,1],[2,-1,0]].
        let f = Frame3::new([1.0, 0.0, 0.0, 0.0], [1.0, 2.0, 3.0]);
        let x = SpatialTransform6::from_frame3(&f);

        let i3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let z3 = [[0.0; 3]; 3];
        let neg_skew = [[0.0, 3.0, -2.0], [-3.0, 0.0, 1.0], [2.0, -1.0, 0.0]];
        let expected = block6(i3, z3, neg_skew, i3);

        assert_mat6_eq(&x.as_matrix(), &expected, TOL_TIGHT);
    }

    #[test]
    fn pure_rotation_is_block_diagonal() {
        // 90° about z: q = (cos π/4, 0, 0, sin π/4).
        // E = [[0,-1,0],[1,0,0],[0,0,1]] (active rotation x→y).
        // Zero translation ⇒ -r̃·E = 0, so X = [[E,0];[0,E]].
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let f = Frame3::new([s, 0.0, 0.0, s], [0.0, 0.0, 0.0]);
        let x = SpatialTransform6::from_frame3(&f);

        let e = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let z3 = [[0.0; 3]; 3];
        let expected = block6(e, z3, z3, e);

        assert_mat6_eq(&x.as_matrix(), &expected, TOL_TIGHT);
    }
}
