//! Degenerated (continuum-based) shell substrate: per-node directors and a
//! varying element Jacobian, carrying the MITC3+ assumed transverse-shear field.
//!
//! # References
//!
//! - Ahmad, S., Irons, B. M. & Zienkiewicz, O. C. (1970). "Analysis of thick
//!   and thin shell structures by curved finite elements." *Int. J. Numer.
//!   Methods Eng.*, 2(3), 419–451. — the original *degenerated solid* shell.
//! - Bathe, K.-J. (2014). *Finite Element Procedures*, 2nd ed., §5.4.2 — the
//!   continuum-based (degenerated) shell kinematics used here.
//! - Lee, Y., Lee, P.-S. & Bathe, K.-J. (2014). "The MITC3+ shell element and
//!   its performance." *Computers & Structures*, 138, 12–23. — the assumed
//!   transverse-shear field this substrate *carries* (task 3392 owns it).
//!
//! # Geometry map
//!
//! The element interpolates a mid-surface plus a per-node *director* fibre:
//!
//! ```text
//! X(ξ, η, ζ) = Σ_i N_i(ξ, η) · x_i  +  (ζ / 2) · Σ_i N_i(ξ, η) · t_i · V_i
//! ```
//!
//! where `N_i` are the three linear triangle shape functions
//! ([`crate::elements::mitc3_plus::Mitc3Plus::shape_at`]), `x_i` are the
//! mid-surface vertex positions, `t_i` the nodal thicknesses, `V_i` the
//! per-node **unit directors** (vertex normals), and `ζ ∈ [-1, 1]` the
//! through-thickness natural coordinate (`ζ = +1` top surface, `ζ = -1`
//! bottom).
//!
//! # Why a degenerate substrate (the varying-Jacobian deliverable)
//!
//! On a flat facet with all directors parallel to the facet normal, the 3×3
//! Jacobian `J = ∂X/∂(ξ,η,ζ)` is **invariant** in `ζ` and the element reduces
//! to the flat MITC3+ of task 3392. When the directors tilt (curved geometry),
//! the `(ζ/2) Σ ∇N_i t_i V_i` term makes `J` **vary** across the element —
//! that director-tilt-induced variation IS the varying Jacobian, and it
//! recovers the intra-element membrane–bending coupling a single flat facet
//! cannot represent.
//!
//! # Director provenance (cross-PRD seam G4)
//!
//! The element *consumes* explicit per-node directors (provenance-agnostic).
//! This module additionally ships a neighbour-averaged facet-normal fallback
//! for meshes without extraction-supplied vertex normals; curved benchmarks
//! supply analytic (e.g. radial) directors as the extraction stand-in. Actual
//! voxel-extraction wiring is deferred to integration (tasks 4065 / 4069).
//!
//! # Scope
//!
//! This module owns the *substrate*: directors, the geometry map, the varying
//! Jacobian, the membrane+bending strain–displacement operator, and the
//! covariant→physical re-expression of the carried MITC3+ shear field. The
//! transverse-shear *formulation* itself is task 3392's; ANS-membrane is task
//! 4065's. The element stiffness assembled from these pieces lives beside its
//! flat-facet sibling in [`crate::shell_assembly`].

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Unit-normal of triangle `[p0, p1, p2]` via the `build_shell_frame`
    /// cross-product convention `n = (p1−p0) × (p2−p0)`, normalized. Used by
    /// the tests to independently reproduce expected facet normals.
    fn facet_unit_normal(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3]) -> [f64; 3] {
        let d01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let d02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let n = [
            d01[1] * d02[2] - d01[2] * d02[1],
            d01[2] * d02[0] - d01[0] * d02[2],
            d01[0] * d02[1] - d01[1] * d02[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        [n[0] / len, n[1] / len, n[2] / len]
    }

    fn norm(v: [f64; 3]) -> f64 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    /// (i) Single flat triangle: every node director equals the unit facet
    /// normal. A triangle in the xy-plane has facet normal (0,0,1).
    #[test]
    fn directors_from_facets_single_flat_triangle_all_equal_facet_normal() {
        let nodes = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let connectivity = vec![[0_usize, 1, 2]];
        let dirs: Vec<Director> = directors_from_facets(&nodes, &connectivity);

        assert_eq!(dirs.len(), 3, "one director per node");
        let n = facet_unit_normal(nodes[0], nodes[1], nodes[2]);
        assert!(
            (n[0]).abs() < TOL && (n[1]).abs() < TOL && (n[2] - 1.0).abs() < TOL,
            "facet normal must be +z, got {n:?}",
        );
        for (i, d) in dirs.iter().enumerate() {
            for k in 0..3 {
                assert!(
                    (d[k] - n[k]).abs() < TOL,
                    "director[{i}][{k}] = {}, expected facet normal {}",
                    d[k],
                    n[k],
                );
            }
        }
    }

    /// (ii) Two facets meeting at a shared edge (symmetric tent, 90° fold): the
    /// shared-vertex director is the normalized sum of the two facet normals.
    ///
    /// Layout (ridge along y from node 0 to node 1):
    ///   node 0 = (0,0,0), node 1 = (0,2,0)  — shared ridge
    ///   node 2 = (−1,1,1) — apex of facet A = [0,1,2], unit normal (1,0,1)/√2
    ///   node 3 = (1,1,1)  — apex of facet B = [1,0,3], unit normal (−1,0,1)/√2
    /// Sum of unit normals = (0,0,√2) → normalized (0,0,1) at the shared nodes.
    #[test]
    fn directors_from_facets_shared_edge_is_normalized_sum_of_facet_normals() {
        let nodes = vec![
            [0.0, 0.0, 0.0],  // 0 shared
            [0.0, 2.0, 0.0],  // 1 shared
            [-1.0, 1.0, 1.0], // 2 facet-A apex
            [1.0, 1.0, 1.0],  // 3 facet-B apex
        ];
        // Reverse the shared edge on facet B so both normals point +z (outward).
        let connectivity = vec![[0_usize, 1, 2], [1_usize, 0, 3]];
        let dirs: Vec<Director> = directors_from_facets(&nodes, &connectivity);
        assert_eq!(dirs.len(), 4);

        let n_a = facet_unit_normal(nodes[0], nodes[1], nodes[2]);
        let n_b = facet_unit_normal(nodes[1], nodes[0], nodes[3]);
        // Sanity: the hand-computed unit normals.
        let inv_sqrt2 = 1.0 / 2.0_f64.sqrt();
        assert!((n_a[0] - inv_sqrt2).abs() < TOL && n_a[1].abs() < TOL && (n_a[2] - inv_sqrt2).abs() < TOL);
        assert!((n_b[0] + inv_sqrt2).abs() < TOL && n_b[1].abs() < TOL && (n_b[2] - inv_sqrt2).abs() < TOL);

        // Shared nodes 0 and 1: normalized(n_a + n_b) = (0,0,1).
        for &shared in &[0_usize, 1] {
            let d = dirs[shared];
            assert!(
                d[0].abs() < TOL && d[1].abs() < TOL && (d[2] - 1.0).abs() < TOL,
                "shared director[{shared}] = {d:?}, expected (0,0,1)",
            );
        }
        // Non-shared nodes keep their single facet normal.
        for k in 0..3 {
            assert!((dirs[2][k] - n_a[k]).abs() < TOL, "node 2 dir mismatch");
            assert!((dirs[3][k] - n_b[k]).abs() < TOL, "node 3 dir mismatch");
        }
    }

    /// (iii) Every director is unit-norm, including at shared vertices where
    /// several facet normals are accumulated.
    #[test]
    fn directors_from_facets_are_always_unit_norm() {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [-1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let connectivity = vec![[0_usize, 1, 2], [1_usize, 0, 3]];
        let dirs = directors_from_facets(&nodes, &connectivity);
        for (i, d) in dirs.iter().enumerate() {
            assert!(
                (norm(*d) - 1.0).abs() < TOL,
                "director[{i}] = {d:?} has norm {}, expected 1.0",
                norm(*d),
            );
        }
    }
}
