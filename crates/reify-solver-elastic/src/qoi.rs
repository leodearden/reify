//! Bounded linear functionals of the P1 displacement field, and their dual
//! loads.
//!
//! PRD reference: `docs/prds/v0_6/goal-oriented-error-estimation.md` §5.1
//! (QoI surface — ball means, coordinate-addressed, parametric, linear), §5.3
//! (dual solve), and §6 contract items C2–C6.
//!
//! # Purpose
//!
//! A *quantity of interest* is a scalar the designer actually cares about —
//! "the downward tip deflection near this mount", "the normal stress across
//! this plane" — rather than the global energy norm the Z-Z estimator
//! ([`crate::error_estimator`]) minimises. Goal-oriented (dual-weighted
//! residual) error estimation needs two things from such a functional:
//!
//! - `J(u_h)`, its value on the current discrete solution, and
//! - `g` with `J(v) = gᵀv` — the **dual load** whose solve `K z_h = g`
//!   produces the adjoint field that weights the primal residual.
//!
//! Both are re-derived from coordinates on *every* mesh the refinement loop
//! produces (C6): nothing here is an index into a mesh that a remesh will
//! throw away.
//!
//! # Module boundary
//!
//! This module owns the functionals, their dual loads, and the dual-solve
//! seam. The per-element dual-weighted *indicator* lives in
//! [`crate::error_estimator`] instead, because the recovery-form contraction
//! it shares with `compute_zz_indicator` is built on that module's private
//! compliance helpers.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;

    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// f64 node coordinates of the standard 5-node, 2-tet fan fixture shared
    /// with `crate::error_estimator`'s `two_tet_fan_mesh`.
    ///
    /// Topology: tet0 = [0,1,2,3] (the canonical unit tet), tet1 = [1,2,3,4]
    /// (shares face {1,2,3} with tet0). Both tets have volume 1/6.
    ///
    /// Element centroids — the coordinates every ball test is placed against:
    ///   tet0 → (0.25, 0.25, 0.25)
    ///   tet1 → (0.50, 0.50, 0.50)
    fn two_tet_fan_coords() -> Vec<[f64; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ]
    }

    fn two_tet_fan_tets() -> Vec<[usize; 4]> {
        vec![[0, 1, 2, 3], [1, 2, 3, 4]]
    }

    /// A non-trivial nodal displacement field over the 2-tet fan (15 entries,
    /// 3 per node). Deliberately NOT a rigid-body or zero field, so an error
    /// case cannot pass for the wrong reason.
    fn two_tet_fan_u() -> Vec<f64> {
        vec![
            0.00, 0.00, 0.00, // node 0
            0.07, -0.02, 0.01, // node 1
            -0.03, 0.05, 0.04, // node 2
            0.02, 0.06, -0.05, // node 3
            0.09, 0.03, 0.08, // node 4
        ]
    }

    /// Assert that BOTH `evaluate` and `dual_load` reject `qoi` with an error
    /// satisfying `is_expected`, on the shared 2-tet fan fixture.
    ///
    /// C4 requires the typed error from *both* directions — a QoI that
    /// validated only in `evaluate` would hand the solver a silently zero `g`,
    /// which is the failure mode the contract names explicitly. Returning
    /// `Err` is what makes "never a zero `g`, never `NaN`" structural: there
    /// is no `Vec<f64>` to be wrong.
    ///
    /// The expectation is a PREDICATE rather than a `QoiError` compared with
    /// `assert_eq!`, because one of the cases this helper must express is a
    /// NaN `radius`: `QoiError`'s derived `PartialEq` inherits `f64`'s, and
    /// `NaN != NaN`, so an equality assertion against
    /// `NonPositiveRadius { radius: NaN }` could never hold however correct
    /// the implementation. Callers match the variant and compare the payload
    /// bitwise where it matters.
    fn assert_both_directions_reject(
        qoi: &dyn QuantityOfInterest,
        case: &str,
        is_expected: impl Fn(&QoiError) -> bool,
    ) {
        let coords = two_tet_fan_coords();
        let tets = two_tet_fan_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = two_tet_fan_u();

        match qoi.evaluate(mesh, &mat, &u) {
            Err(e) => assert!(
                is_expected(&e),
                "{case}: evaluate returned the wrong QoiError variant: {e:?}",
            ),
            Ok(j) => panic!(
                "{case}: evaluate must return a typed error, got Ok({j}) — C4 \
                 forbids resolving an unresolvable QoI",
            ),
        }
        match qoi.dual_load(mesh, &mat, &u) {
            Err(e) => assert!(
                is_expected(&e),
                "{case}: dual_load returned the wrong QoiError variant: {e:?}",
            ),
            Ok(g) => panic!(
                "{case}: dual_load must return a typed error, got Ok(g) with \
                 {} entries — C4 forbids a silently zero (or any) g here",
                g.len(),
            ),
        }
    }

    /// BT4 / C4 — `LocalDisplacementQoi` returns a TYPED [`QoiError`] from
    /// both `evaluate` and `dual_load` for every unresolvable input, never a
    /// panic and never a silently zero `g`.
    ///
    /// Cases, per PRD §6 C4:
    ///
    /// * `radius = 0.0`, `radius = -1.0`, `radius` non-finite (NaN, +inf) →
    ///   [`QoiError::NonPositiveRadius`]. §5.1 makes `radius` a *required,
    ///   positive* payload field; a default "small" radius was rejected
    ///   because it silently reintroduces the point delta whose divergence
    ///   §3 measured.
    /// * `direction = [0,0,0]` → [`QoiError::ZeroDirection`].
    /// * `at` outside every element, with a radius too small to catch any
    ///   centroid → [`QoiError::PointOutsideBody`].
    ///
    /// # The zero-direction case runs without `#[should_panic]` deliberately
    ///
    /// Directions are normalized by the extractor, and `resolve` asserts
    /// unit length as a caller precondition — but that assertion is a
    /// `debug_assert!`, and this test runs with `debug_assertions` on. A zero
    /// vector must therefore reach the typed error *before* the unit-length
    /// assertion fires. If the check order were reversed, this test would
    /// panic rather than fail an assertion, which is why the ordering inside
    /// `resolve` is fixed and documented rather than incidental.
    ///
    /// # TDD red→green
    ///
    /// **RED** (step-1): `QoiError`, `QuantityOfInterest`, `P1TetMeshRef` and
    /// `LocalDisplacementQoi` do not exist, so this fails to COMPILE — the
    /// crate's established RED convention for a new type surface.
    ///
    /// **GREEN** (step-2): the `src/qoi.rs` core lands with one private
    /// `resolve` chokepoint shared by both methods.
    #[test]
    fn local_displacement_qoi_returns_typed_error_from_both_directions_for_every_unresolvable_input()
    {
        let good_at = [0.25, 0.25, 0.25];
        let good_dir = [0.0, -1.0, 0.0];

        for bad_radius in [0.0_f64, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_both_directions_reject(
                &LocalDisplacementQoi {
                    at: good_at,
                    radius: bad_radius,
                    direction: good_dir,
                },
                &format!("radius = {bad_radius}"),
                // Bitwise payload comparison, so the NaN case pins the
                // offending value just as tightly as the finite ones.
                |e| {
                    matches!(e, QoiError::NonPositiveRadius { radius }
                             if radius.to_bits() == bad_radius.to_bits())
                },
            );
        }

        assert_both_directions_reject(
            &LocalDisplacementQoi {
                at: good_at,
                radius: 0.1,
                direction: [0.0, 0.0, 0.0],
            },
            "direction = [0,0,0]",
            |e| *e == QoiError::ZeroDirection,
        );

        let far_outside = [100.0, 100.0, 100.0];
        assert_both_directions_reject(
            &LocalDisplacementQoi {
                at: far_outside,
                radius: 0.1,
                direction: good_dir,
            },
            "at far outside the body",
            |e| *e == QoiError::PointOutsideBody { at: far_outside },
        );
    }

    /// The BT4 error cases above are not vacuous: a well-formed
    /// `LocalDisplacementQoi` on the same fixture resolves, and its dual load
    /// is non-zero.
    ///
    /// Without this control, an implementation whose `resolve` unconditionally
    /// returned `Err` would pass every assertion in the test above.
    #[test]
    fn local_displacement_qoi_resolves_and_produces_a_non_zero_dual_load_on_the_same_fixture() {
        let coords = two_tet_fan_coords();
        let tets = two_tet_fan_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = two_tet_fan_u();

        // Centred on tet0's centroid with a radius that catches it alone.
        let qoi = LocalDisplacementQoi {
            at: [0.25, 0.25, 0.25],
            radius: 0.1,
            direction: [0.0, -1.0, 0.0],
        };

        let j = qoi
            .evaluate(mesh, &mat, &u)
            .expect("a well-formed QoI centred on a real element centroid must resolve");
        assert!(j.is_finite(), "J(u_h) must be finite, got {j}");

        let g = qoi
            .dual_load(mesh, &mat, &u)
            .expect("a well-formed QoI must produce a dual load");
        assert_eq!(
            g.len(),
            3 * coords.len(),
            "dual load must be one entry per DOF",
        );
        assert!(
            g.iter().all(|x| x.is_finite()),
            "C4: the dual load must never contain NaN or infinity",
        );
        assert!(
            g.iter().any(|&x| x != 0.0),
            "C4: g is never zero for a non-zero direction and a non-empty \
             contributing set; got an all-zero g",
        );
    }

    /// `QoiKind` ships exactly the two §5.1 variants, and
    /// `LocalDisplacementQoi` reports the displacement one.
    ///
    /// "Exactly two" is asserted structurally by the wildcard-free `match`
    /// below: adding a third variant makes this function fail to compile.
    /// `kind()` is what tells the result layer which `QoIEstimate` variant —
    /// hence which dimension — `evaluate` returned.
    #[test]
    fn qoi_kind_has_exactly_the_two_shipped_variants_and_local_displacement_reports_its_own() {
        fn label(k: QoiKind) -> &'static str {
            match k {
                QoiKind::LocalDisplacement => "LocalDisplacement",
                QoiKind::LocalNormalStress => "LocalNormalStress",
            }
        }

        assert_eq!(label(QoiKind::LocalDisplacement), "LocalDisplacement");
        assert_eq!(label(QoiKind::LocalNormalStress), "LocalNormalStress");

        let qoi = LocalDisplacementQoi {
            at: [0.25, 0.25, 0.25],
            radius: 0.1,
            direction: [0.0, -1.0, 0.0],
        };
        assert_eq!(qoi.kind(), QoiKind::LocalDisplacement);
    }
}
