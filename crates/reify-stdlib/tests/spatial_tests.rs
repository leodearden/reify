//! Integration tests for the Featherstone 6D spatial-vector core
//! (`reify_stdlib::dynamics::spatial`).
//!
//! Mirrors the `tests/complex_tests.rs` layout: top-of-file `use`, per-behavior
//! `mod` blocks, shared tolerance/entrywise-equality helpers at the top.
//!
//! Convention (Featherstone 2008, §2.4): spatial vectors are ordered
//! `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` — angular first, linear second. 6×6
//! matrices are row-major `[f64; 36]`.

use reify_stdlib::dynamics::spatial::{
    cross_f, cross_m, Frame3, SpatialInertia6, SpatialTransform6, SpatialVector6,
};

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

/// Entrywise approximate-equality assertion for a `SpatialVector6`.
fn assert_vec6_eq(actual: &SpatialVector6, expected: &[f64; 6], tol: f64) {
    let a = actual.as_array();
    for i in 0..6 {
        assert!(
            (a[i] - expected[i]).abs() < tol,
            "component [{i}]: expected {}, got {} (|Δ|={:e}, tol={:e})",
            expected[i],
            a[i],
            (a[i] - expected[i]).abs(),
            tol
        );
    }
}

/// Reference row-major 6×6 · 6 matrix-vector multiply (test oracle).
fn matvec6(m: &[f64; 36], v: &[f64; 6]) -> [f64; 6] {
    let mut out = [0.0; 6];
    for i in 0..6 {
        let mut acc = 0.0;
        for k in 0..6 {
            acc += m[i * 6 + k] * v[k];
        }
        out[i] = acc;
    }
    out
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

    #[test]
    fn rotation_and_translation_bottom_left_is_neg_skew_r_times_e() {
        // Distinguishes the Featherstone bottom-left ordering -r̃·E (this
        // code, Eq. 2.24) from the alternative -E·r̃. The pure-translation
        // case (E=I ⇒ -r̃·E == -E·r̃) and pure-rotation case (r=0 ⇒ BL=0)
        // can't tell the two apart, and the capstone / apply oracles are
        // self-consistent, so a wrong-but-consistent ordering would slip
        // past every other test. Downstream RBD-δ/RBD-ε must interoperate
        // with this exact convention, so pin it with a frame that has BOTH
        // nonzero rotation and nonzero translation.
        //
        // 90° about z: q = (cos π/4, 0, 0, sin π/4) ⇒
        //   E = [[0,-1,0],[1,0,0],[0,0,1]].
        // r = [1,2,3] ⇒ r̃ = [[0,-3,2],[3,0,-1],[-2,1,0]].
        // r̃·E = [[-3,0,2],[0,-3,-1],[1,2,0]] ⇒
        //   BL = -r̃·E = [[3,0,-2],[0,3,1],[-1,-2,0]].
        // (The alternative -E·r̃ = [[3,0,-1],[0,3,-2],[2,-1,0]] is distinct,
        // so this assertion genuinely discriminates the convention.)
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let f = Frame3::new([s, 0.0, 0.0, s], [1.0, 2.0, 3.0]);
        let x = SpatialTransform6::from_frame3(&f);

        let e = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let z3 = [[0.0; 3]; 3];
        let bl = [[3.0, 0.0, -2.0], [0.0, 3.0, 1.0], [-1.0, -2.0, 0.0]];
        let expected = block6(e, z3, bl, e);

        assert_mat6_eq(&x.as_matrix(), &expected, TOL_TIGHT);
    }
}

mod compose {
    use super::*;

    #[test]
    fn identity_compose_identity_is_identity() {
        let i = SpatialTransform6::from_frame3(&Frame3::identity());
        assert_mat6_eq(&i.compose(&i).as_matrix(), &identity6(), TOL_NUMERIC);
    }

    #[test]
    fn translation_only_compose_sums_skew() {
        // F1: t=[1,0,0], F2: t=[0,2,0], both identity rotation.
        // [[I,0];[-r̃₁,I]]·[[I,0];[-r̃₂,I]] = [[I,0];[-(r̃₁+r̃₂),I]]
        // and skew is linear, so -(r̃₁+r̃₂) = -skew([1,2,0]).
        let f1 = Frame3::new([1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let f2 = Frame3::new([1.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0]);
        let composed = SpatialTransform6::from_frame3(&f1)
            .compose(&SpatialTransform6::from_frame3(&f2));

        let i3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let z3 = [[0.0; 3]; 3];
        // -skew([1,2,0]) = -[[0,0,2],[0,0,-1],[-2,1,0]]
        let neg_skew_sum = [[0.0, 0.0, -2.0], [0.0, 0.0, 1.0], [2.0, -1.0, 0.0]];
        let expected = block6(i3, z3, neg_skew_sum, i3);

        assert_mat6_eq(&composed.as_matrix(), &expected, TOL_NUMERIC);
    }

    #[test]
    fn compose_is_associative() {
        let x1 = SpatialTransform6::from_frame3(&Frame3::new(
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ));
        let x2 = SpatialTransform6::from_frame3(&Frame3::new(
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
        ));
        let x3 = SpatialTransform6::from_frame3(&Frame3::new(
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 3.0],
        ));
        let left = x1.compose(&x2).compose(&x3);
        let right = x1.compose(&x2.compose(&x3));
        assert_mat6_eq(&left.as_matrix(), &right.as_matrix(), TOL_NUMERIC);
    }

    #[test]
    fn compose_then_apply_equals_nested_apply() {
        // compose() is documented "apply other first, then self", i.e.
        //   x1.compose(&x2).apply(v) == x1.apply(x2.apply(v)).
        // This is the exact contract the RNEA forward pass relies on. The
        // associativity/block tests and the self-consistent oracles
        // (capstone derives inverse from the same convention;
        // apply_matches_dense_matvec checks apply against as_matrix) would
        // all pass under an operand-order or row/column-major mismatch
        // between compose and apply — only the cross-relation catches it.
        // Use a nontrivial pair with both rotation and translation.
        let s = std::f64::consts::FRAC_1_SQRT_2; // 90° about z
        let half = std::f64::consts::PI / 12.0; // 30° about x
        let (c, sx) = (half.cos(), half.sin());
        let x1 = SpatialTransform6::from_frame3(&Frame3::new(
            [s, 0.0, 0.0, s],
            [1.0, 2.0, 3.0],
        ));
        let x2 = SpatialTransform6::from_frame3(&Frame3::new(
            [c, sx, 0.0, 0.0],
            [-2.0, 0.5, 1.0],
        ));
        let v = SpatialVector6::from_array([0.5, -1.5, 2.0, -3.0, 4.0, -0.25]);

        let lhs = x1.compose(&x2).apply(&v);
        let rhs = x1.apply(&x2.apply(&v));
        assert_vec6_eq(&lhs, &rhs.as_array(), TOL_NUMERIC);
    }
}

mod inverse {
    use super::*;

    #[test]
    fn identity_inverse_is_identity() {
        let x = SpatialTransform6::from_frame3(&Frame3::identity());
        assert_mat6_eq(&x.inverse().as_matrix(), &identity6(), TOL_TIGHT);
    }

    #[test]
    fn pure_translation_inverse_negates_translation() {
        // Featherstone closed-form: X(r, E)⁻¹ = X(−Eᵀr, Eᵀ).
        // identity rotation ⇒ X(F(t))⁻¹ == X(F(−t)).
        let t = [1.0, 2.0, 3.0];
        let x = SpatialTransform6::from_frame3(&Frame3::new([1.0, 0.0, 0.0, 0.0], t));
        let expected = SpatialTransform6::from_frame3(&Frame3::new(
            [1.0, 0.0, 0.0, 0.0],
            [-t[0], -t[1], -t[2]],
        ));
        assert_mat6_eq(&x.inverse().as_matrix(), &expected.as_matrix(), TOL_TIGHT);
    }

    #[test]
    fn pure_rotation_inverse_conjugates_rotation() {
        // 30° about x: q = (cos π/12, sin π/12, 0, 0); conj = (cos, −sin, 0, 0).
        // Zero translation ⇒ X(F(q))⁻¹ == X(F(conj q)).
        let half = std::f64::consts::PI / 12.0;
        let (c, s) = (half.cos(), half.sin());
        let q = [c, s, 0.0, 0.0];
        let q_conj = [c, -s, 0.0, 0.0];
        let x = SpatialTransform6::from_frame3(&Frame3::new(q, [0.0, 0.0, 0.0]));
        let expected =
            SpatialTransform6::from_frame3(&Frame3::new(q_conj, [0.0, 0.0, 0.0]));
        assert_mat6_eq(&x.inverse().as_matrix(), &expected.as_matrix(), TOL_NUMERIC);
    }
}

mod capstone {
    use super::*;

    /// Minimal deterministic xorshift64 PRNG (seed must be nonzero). No `rand`
    /// dev-dependency: the workspace has zero `rand` references and adding one
    /// for a single test inflates every downstream consumer's dep graph.
    struct Xorshift64(u64);

    impl Xorshift64 {
        fn new(seed: u64) -> Self {
            Xorshift64(seed)
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        /// Uniform in `[0, 1)` using the top 53 bits.
        fn next_unit(&mut self) -> f64 {
            (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
        }
        /// Uniform in `[lo, hi]`.
        fn range(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (hi - lo) * self.next_unit()
        }
    }

    /// User-observable RBD-γ signal: for 50 deterministic random `Frame3`
    /// samples, `X(f).compose(X(f).inverse())` equals the 6×6 identity
    /// entrywise within the Featherstone-canonical 1e-12 tolerance.
    #[test]
    fn from_frame3_compose_inverse_is_identity_50_random_samples() {
        let mut rng = Xorshift64::new(0xDEAD_BEEF_CAFE_BABE);
        let id = identity6();

        for sample in 0..50 {
            // (a) random unit quaternion, rejecting near-zero magnitudes.
            let q = loop {
                let raw = [
                    rng.range(-1.0, 1.0),
                    rng.range(-1.0, 1.0),
                    rng.range(-1.0, 1.0),
                    rng.range(-1.0, 1.0),
                ];
                let norm =
                    (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2] + raw[3] * raw[3])
                        .sqrt();
                if norm > 1e-6 {
                    break [
                        raw[0] / norm,
                        raw[1] / norm,
                        raw[2] / norm,
                        raw[3] / norm,
                    ];
                }
            };
            // (b) random translation in [-10, 10] meters.
            let t = [
                rng.range(-10.0, 10.0),
                rng.range(-10.0, 10.0),
                rng.range(-10.0, 10.0),
            ];
            // (c)–(d)
            let f = Frame3::new(q, t);
            let x = SpatialTransform6::from_frame3(&f);
            let prod = x.compose(&x.inverse()).as_matrix();

            // (e) entrywise within 1e-12 of I₆, with a sample diagnostic.
            for i in 0..6 {
                for j in 0..6 {
                    let got = prod[i * 6 + j];
                    let want = id[i * 6 + j];
                    assert!(
                        (got - want).abs() < TOL_NUMERIC,
                        "sample {sample}: X·X⁻¹ ≠ I at [{i},{j}]: got {got}, want {want} \
                         (|Δ|={:e}, tol={:e})\n  Frame3 {{ rotation: {:?}, translation: {:?} }}",
                        (got - want).abs(),
                        TOL_NUMERIC,
                        q,
                        t,
                    );
                }
            }
        }
    }
}

mod spatial_inertia6 {
    use super::*;

    #[test]
    fn point_mass_at_origin_is_block_diagonal() {
        // Featherstone Eq. 2.63 with c = 0:
        //   I_6 = [[Ī, 0]; [0, m·I_3]].
        let i6 = SpatialInertia6::from_mass_com_inertia(
            2.0,
            [0.0, 0.0, 0.0],
            [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]],
        );
        let tl = [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]];
        let z3 = [[0.0; 3]; 3];
        let br = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let expected = block6(tl, z3, z3, br);
        assert_mat6_eq(&i6.as_matrix(), &expected, TOL_TIGHT);
    }

    #[test]
    fn is_symmetric() {
        // Featherstone (2008) §2.13: the spatial inertia is symmetric. With
        // Ī symmetric, c̃·c̃ᵀ symmetric, and (m·c̃)ᵀ = m·c̃ᵀ (the BL block),
        // every entry should mirror across the diagonal.
        let i6 = SpatialInertia6::from_mass_com_inertia(
            2.5,
            [1.0, -0.5, 0.25],
            [[1.0, 0.1, 0.0], [0.1, 2.0, 0.05], [0.0, 0.05, 3.0]],
        );
        let m = i6.as_matrix();
        for i in 0..6 {
            for j in 0..6 {
                assert!(
                    (m[i * 6 + j] - m[j * 6 + i]).abs() < TOL_TIGHT,
                    "asymmetry at [{i},{j}]/[{j},{i}]: {} vs {}",
                    m[i * 6 + j],
                    m[j * 6 + i]
                );
            }
        }
    }

    #[test]
    fn identity_inertia_at_origin_apply_returns_input() {
        // mass = 1, com = 0, Ī = I_3 ⇒ I_6 = I_6×6, so apply is the identity.
        let i6 = SpatialInertia6::from_mass_com_inertia(
            1.0,
            [0.0, 0.0, 0.0],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        );
        let v = SpatialVector6::from_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_vec6_eq(&i6.apply(&v), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], TOL_TIGHT);
    }

    #[test]
    fn parallel_axis_offset_com_matches_featherstone_eq_2_63() {
        // c = [1, 0, 0], m = 1, Ī = I_3. Featherstone Eq. 2.63:
        //   c̃        = [[0,0,0],[0,0,-1],[0,1,0]]
        //   c̃·c̃ᵀ   = [[0,0,0],[0,1,0],[0,0,1]]
        //   TL = Ī + m·c̃·c̃ᵀ = [[1,0,0],[0,2,0],[0,0,2]]
        //   TR = m·c̃,  BL = m·c̃ᵀ = -m·c̃,  BR = m·I_3.
        let i6 = SpatialInertia6::from_mass_com_inertia(
            1.0,
            [1.0, 0.0, 0.0],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        );
        let tl = [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let tr = [[0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
        let bl = [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
        let br = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let expected = block6(tl, tr, bl, br);
        assert_mat6_eq(&i6.as_matrix(), &expected, TOL_TIGHT);

        // Apply to v_lin = [1,0,0] (parallel to c): the m·c̃ contribution to
        // angular momentum is c × v_lin = 0, so the result matches the dense
        // matvec via as_matrix() and collapses to [0,0,0, 1,0,0].
        let v = SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let expected_apply = matvec6(&i6.as_matrix(), &v.as_array());
        assert_vec6_eq(&i6.apply(&v), &expected_apply, TOL_TIGHT);
        assert_vec6_eq(&i6.apply(&v), &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0], TOL_TIGHT);

        // Apply to ω_z = [0,0,1, 0,0,0]: the m·c̃ off-diagonal block is
        // exercised and the dense matvec is the oracle.
        let w = SpatialVector6::from_array([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        let expected_w = matvec6(&i6.as_matrix(), &w.as_array());
        assert_vec6_eq(&i6.apply(&w), &expected_w, TOL_TIGHT);
    }
}

mod cross_m_module {
    use super::*;

    /// Featherstone (2008) Eq. 2.31, motion-on-motion cross product:
    ///   cross_m(v, w) = [ω_v × ω_w, ω_v × v_w + v_v × ω_w].

    #[test]
    fn zero_cross_zero_is_zero() {
        let z = SpatialVector6::zero();
        assert_vec6_eq(&cross_m(&z, &z), &[0.0; 6], TOL_TIGHT);
    }

    #[test]
    fn angular_cross_angular_is_omega_cross_omega() {
        // ω_v = e_x, ω_w = e_y, v_v = v_w = 0 ⇒ [e_x × e_y, 0] = [e_z, 0].
        let v = SpatialVector6::from_angular_linear([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let w = SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]);
        assert_vec6_eq(
            &cross_m(&v, &w),
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            TOL_TIGHT,
        );
    }

    #[test]
    fn angular_cross_linear_is_in_linear_slot() {
        // ω_v = e_x, v_w = e_y, rest zero ⇒ [0, ω_v × v_w] = [0, e_x × e_y] = [0, e_z].
        let v = SpatialVector6::from_angular_linear([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let w = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_vec6_eq(
            &cross_m(&v, &w),
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            TOL_TIGHT,
        );
    }

    #[test]
    fn linear_cross_angular_is_in_linear_slot() {
        // v_v = e_x, ω_w = e_y, rest zero ⇒ [0, v_v × ω_w] = [0, e_x × e_y] = [0, e_z].
        let v = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let w = SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]);
        assert_vec6_eq(
            &cross_m(&v, &w),
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            TOL_TIGHT,
        );
    }

    #[test]
    fn linear_cross_linear_vanishes() {
        // v_v = e_x, v_w = e_y, ω_v = ω_w = 0 ⇒ both terms vanish per Eq. 2.31.
        let v = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let w = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_vec6_eq(&cross_m(&v, &w), &[0.0; 6], TOL_TIGHT);
    }
}

mod cross_f_module {
    use super::*;

    /// Featherstone (2008) Eq. 2.32, motion-on-force cross product:
    ///   cross_f(v, f) = [ω_v × τ_f + v_v × F_f,  ω_v × F_f]
    /// where the spatial-force layout reuses SpatialVector6 storage but
    /// interprets `[0..3]` as torque τ and `[3..6]` as force F.

    #[test]
    fn zero_cross_zero_is_zero() {
        let z = SpatialVector6::zero();
        assert_vec6_eq(&cross_f(&z, &z), &[0.0; 6], TOL_TIGHT);
    }

    #[test]
    fn angular_on_torque_is_in_torque_slot() {
        // ω_v = e_x, τ_f = e_y, rest zero ⇒ [e_x × e_y, 0] = [e_z, 0].
        let v = SpatialVector6::from_angular_linear([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let f = SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]);
        assert_vec6_eq(
            &cross_f(&v, &f),
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            TOL_TIGHT,
        );
    }

    #[test]
    fn linear_on_force_is_in_torque_slot() {
        // v_v = e_x, F_f = e_y, rest zero ⇒ [v_v × F_f, 0] = [e_z, 0].
        let v = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let f = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_vec6_eq(
            &cross_f(&v, &f),
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            TOL_TIGHT,
        );
    }

    #[test]
    fn angular_on_force_is_in_force_slot() {
        // ω_v = e_x, F_f = e_y, rest zero ⇒ [0, ω_v × F_f] = [0, e_z].
        let v = SpatialVector6::from_angular_linear([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let f = SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_vec6_eq(
            &cross_f(&v, &f),
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            TOL_TIGHT,
        );
    }

    #[test]
    fn motion_force_duality_holds() {
        // Featherstone (2008) §2.11: for any motion v, motion w, force f,
        //   <cross_m(v, w), f> = −<w, cross_f(v, f)>
        // where <·, ·> is the standard 6-vector dot product (ω·τ + v·F).
        let v = SpatialVector6::from_angular_linear([1.0, -0.5, 2.0], [0.25, 1.5, -1.0]);
        let w = SpatialVector6::from_angular_linear([-0.75, 2.0, 0.5], [1.0, -1.25, 0.5]);
        let f = SpatialVector6::from_angular_linear([0.5, 1.0, -2.0], [-1.5, 0.25, 1.75]);

        let dot = |a: &SpatialVector6, b: &SpatialVector6| -> f64 {
            let a = a.as_array();
            let b = b.as_array();
            (0..6).map(|i| a[i] * b[i]).sum()
        };

        let lhs = dot(&cross_m(&v, &w), &f);
        let rhs = -dot(&w, &cross_f(&v, &f));
        assert!(
            (lhs - rhs).abs() < TOL_NUMERIC,
            "duality failure: <cross_m(v,w), f> = {lhs}, -<w, cross_f(v,f)> = {rhs} \
             (|Δ|={:e}, tol={:e})",
            (lhs - rhs).abs(),
            TOL_NUMERIC
        );
    }
}

mod transform_apply {
    use super::*;

    #[test]
    fn identity_apply_is_identity() {
        let v = SpatialVector6::from_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let x = SpatialTransform6::from_frame3(&Frame3::identity());
        assert_eq!(x.apply(&v).as_array(), v.as_array());
    }

    #[test]
    fn pure_translation_apply_picks_up_cross_term() {
        // X(F([1,0,0])) · [ω=(0,0,1); v=(0,0,0)] = [E·ω; -r̃E·ω + E·v]
        // = [(0,0,1); -[1,0,0]×[0,0,1]] = [(0,0,1); (0,1,0)].
        let x = SpatialTransform6::from_frame3(&Frame3::new(
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ));
        let v = SpatialVector6::from_array([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert_vec6_eq(&x.apply(&v), &[0.0, 0.0, 1.0, 0.0, 1.0, 0.0], TOL_TIGHT);
    }

    #[test]
    fn apply_matches_dense_matvec_for_nontrivial_frame() {
        // 30° about x + translation [1,2,3]; apply must equal the explicit
        // dense 6×6·6 multiply over as_matrix().
        let half = std::f64::consts::PI / 12.0;
        let (c, s) = (half.cos(), half.sin());
        let f = Frame3::new([c, s, 0.0, 0.0], [1.0, 2.0, 3.0]);
        let x = SpatialTransform6::from_frame3(&f);
        let v = SpatialVector6::from_array([0.5, -1.5, 2.0, -3.0, 4.0, -0.25]);
        let expected = matvec6(&x.as_matrix(), &v.as_array());
        assert_vec6_eq(&x.apply(&v), &expected, TOL_NUMERIC);
    }
}
