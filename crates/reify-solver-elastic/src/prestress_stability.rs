//! Self-stress & prestress-stability analysis kernel (Tensegrity T2).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-structures.md` §5 / Tier-2 leaf T2. This is the
//! layer-3 analysis kernel of the v0_6 tensegrity DAG: given a realised
//! geometry (`nodes`), a member topology (`members`), and per-member force
//! densities `q`, it reports the classical self-stress / mechanism / stability
//! verdict of the prestressed framework.
//!
//! # Method
//!
//! 1. **Equilibrium matrix** `A` (`d·N × m`, unit-direction convention
//!    `A·s = f` with `s` the member axial forces): column `i` for member
//!    `(j, k)` carries the unit direction `û = (x_k − x_j)/L` in node-`j`'s rows
//!    and `−û` in node-`k`'s rows, in node-major / axis-minor DOF order
//!    (`3a + α`) so `A`'s rows match `K_G = D ⊗ I₃` and the buckling kernel's
//!    `u[3·node + axis]` ordering.
//! 2. **Self-stress states** `s = nullity(A) = m − rank(A)` — a valid tensegrity
//!    needs `s ≥ 1` (PRD §5).
//! 3. **Infinitesimal mechanisms** `null(Aᵀ)` minus the rigid-body modes
//!    (3 translations + 3 infinitesimal rotations); the reported count is the
//!    rigid-excluded internal mechanism count.
//! 4. **Maxwell number** `m − d·N` (Calladine's identity, reported as the raw
//!    integer field).
//! 5. **Geometric/stress stiffness** `K_G = D ⊗ I₃` with `D = CᵀQC` reused
//!    verbatim from layer-2 ([`crate::form_find_free::assemble_force_density_matrix`]).
//!    No sign flip — `q` already encodes cable(+)/strut(−); this is the prestress
//!    energy Hessian (contrast the buckling kernel's `−K_g`).
//! 6. **Prestress stability**: reduced `K_G^red = Mᵀ K_G M` on the internal
//!    mechanism subspace `M`; prestress-stable iff `K_G^red ≻ 0`, tested by
//!    reusing the buckling dense eigensolver path
//!    ([`crate::eigensolve::solve_eigen_dense`]).
//! 7. **Super-stability** (Connelly): `D` PSD ∧ `rank(D) == N − d − 1`. The
//!    third condition (member directions not on a conic at infinity) is an
//!    intentionally-documented deferral.
//!
//! # Scope
//!
//! Kernel only: this module does not touch the `.ri` `constraint form.stable`
//! surface, the stdlib signature, or the reify-eval trampoline — exactly like
//! the T1a ([`crate::form_find`]) and T1b ([`crate::form_find_free`]) kernels
//! before it. See `plan.json` design_decisions for the scoping rationale.

// TDD scaffolding (Task 3796): this kernel is built bottom-up — helper fns
// (`assemble_equilibrium_matrix`, `count_self_stress_states`, …) land several
// steps before step-14 wires them into the public `analyze_prestress_stability`.
// Until then they are reachable only from `#[cfg(test)]` unit tests, so the
// non-test lib build (clippy `--all-targets -D warnings`) would flag every one
// as dead. This blanket allow keeps each intermediate commit clippy-clean; it is
// REMOVED in step-16, after the public entry point makes every helper live, so
// the final state still gets full dead-code coverage.
#![allow(dead_code)]

use faer::{Mat, Side};

/// Relative tolerance for spectral rank / nullity classification: an eigenvalue
/// of a symmetric Gram matrix counts as nonzero (a rank direction) only when its
/// magnitude exceeds this fraction of the largest-magnitude eigenvalue.
///
/// Same value and rationale as the layer-2 form-finding classifier
/// ([`crate::form_find_free`]'s `NULLITY_REL_TOL`): the exact unit-scale prism
/// has a wide spectral gap (O(1) nonzero singular values vs O(1e-15) numerical
/// zeros), so `1e-8` cleanly separates the null space from the rest of the
/// spectrum without a brittle absolute threshold.
const NULLITY_REL_TOL: f64 = 1e-8;

/// Rank of a symmetric Gram matrix (e.g. `AᵀA` or `AAᵀ`) from its spectrum: the
/// count of eigenvalues whose magnitude exceeds `rel_tol · max|λ|`.
///
/// Reuses the dense self-adjoint eigendecomposition pattern of
/// [`crate::form_find_free`]'s `classify_spectrum` (faer `self_adjoint_eigen`
/// with a relative threshold). A Gram matrix is real-symmetric PSD by
/// construction, so the eigenvalues are the squared singular values of the
/// underlying matrix and the relative threshold is well-scaled.
fn spectral_rank(gram: &Mat<f64>, rel_tol: f64) -> usize {
    let n = gram.nrows();
    if n == 0 {
        return 0;
    }
    // Dense self-adjoint EVD; the Gram matrix is real symmetric, so a failure
    // here is a bug, not infeasible input — panic with a descriptive message
    // (matching the form_find_free / eigensolve `.expect` precedent).
    let eig = gram
        .self_adjoint_eigen(Side::Lower)
        .expect("Gram matrix is real symmetric PSD; self-adjoint EVD must succeed");
    let s = eig.S();
    let eigenvalues: Vec<f64> = (0..n).map(|i| s[i]).collect();
    let max_mag = eigenvalues.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    let threshold = rel_tol * max_mag;
    eigenvalues.iter().filter(|v| v.abs() > threshold).count()
}

/// Form the Gram matrix `AᵀA` (`m × m`) of `a` (`p × m`) by explicit
/// accumulation. The matrices here are tiny (`m = 12`, `p = 18` for the
/// triplex), so the direct triple loop is clear and cheap.
fn gram_transpose_self(a: &Mat<f64>) -> Mat<f64> {
    let p = a.nrows();
    let m = a.ncols();
    let mut gram = Mat::<f64>::zeros(m, m);
    for i in 0..m {
        for j in 0..m {
            let mut acc = 0.0;
            for r in 0..p {
                acc += a[(r, i)] * a[(r, j)];
            }
            gram[(i, j)] = acc;
        }
    }
    gram
}

/// Number of self-stress states `s = nullity(A) = m − rank(A)`, where `m` is the
/// member count (columns of the equilibrium matrix `A`).
///
/// `rank(A) = rank(AᵀA)` (the Gram matrix shares `A`'s rank), computed as the
/// spectral rank of `AᵀA` under [`NULLITY_REL_TOL`]. A valid tensegrity needs
/// `s ≥ 1` — at least one self-equilibrated prestress (PRD §5).
pub(crate) fn count_self_stress_states(a: &Mat<f64>) -> usize {
    let m = a.ncols();
    let gram = gram_transpose_self(a);
    let rank = spectral_rank(&gram, NULLITY_REL_TOL);
    m - rank
}

/// Assemble the Pellegrino–Calladine equilibrium matrix `A` (`d·N × m`) in the
/// unit-direction convention `A·s = f`, where `s` is the vector of member axial
/// forces and `f` the resulting nodal force vector.
///
/// Column `i` for member `(j, k)` carries the unit direction
/// `û = (x_k − x_j) / ‖x_k − x_j‖` in node-`j`'s three rows and `−û` in
/// node-`k`'s three rows; every other entry is zero. Rows are laid out
/// node-major / axis-minor (DOF index `3a + α`), so `A`'s rows align with
/// `K_G = D ⊗ I₃` and the buckling kernel's `u[3·node + axis]` ordering — the
/// reduced-stiffness projection `Mᵀ K_G M` depends on this shared DOF order.
///
/// The unit-vector form has the same rank/nullity as the full force-density
/// form (they differ by a nonsingular diagonal length scaling), so the
/// self-stress and mechanism counts are identical while matching the standard
/// equilibrium-matrix definition.
pub(crate) fn assemble_equilibrium_matrix(
    nodes: &[[f64; 3]],
    members: &[(usize, usize)],
) -> Mat<f64> {
    let n = nodes.len();
    let m = members.len();
    let mut a = Mat::<f64>::zeros(3 * n, m);
    for (i, &(j, k)) in members.iter().enumerate() {
        let d = [
            nodes[k][0] - nodes[j][0],
            nodes[k][1] - nodes[j][1],
            nodes[k][2] - nodes[j][2],
        ];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let u = [d[0] / len, d[1] / len, d[2] / len];
        for (axis, &ua) in u.iter().enumerate() {
            a[(3 * j + axis, i)] = ua;
            a[(3 * k + axis, i)] = -ua;
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The complete 9-cable triplex member list (3 struts + 9 cables) in
    /// struts-then-cables order, mirroring `form_find_free.rs`'s
    /// `triplex_topology`. Self-stress / mechanism / stability counts depend
    /// only on the topology + geometry (and `q`), so the per-member kind tags
    /// are not needed by this kernel.
    fn triplex_members() -> Vec<(usize, usize)> {
        vec![
            // struts
            (0, 4),
            (1, 5),
            (2, 3),
            // top horizontals
            (0, 1),
            (1, 2),
            (2, 0),
            // bottom horizontals
            (3, 4),
            (4, 5),
            (5, 3),
            // verticals
            (0, 3),
            (1, 4),
            (2, 5),
        ]
    }

    /// The canonical symmetric triplex prism (R = 1, height = 1, twist 30°),
    /// identical to `form_find_free.rs`'s fixture: nodes 0,1,2 top (z = 1) at
    /// azimuth 120°·i; 3,4,5 bottom (z = 0) at azimuth 120°·i + 30°. The exact
    /// equilibrium geometry whose self-stress / mechanism goldens this kernel
    /// must reproduce.
    fn canonical_prism() -> Vec<[f64; 3]> {
        let deg = std::f64::consts::PI / 180.0;
        let top = |i: usize| {
            let a = 120.0 * (i as f64) * deg;
            [a.cos(), a.sin(), 1.0]
        };
        let bot = |i: usize| {
            let a = (120.0 * (i as f64) + 30.0) * deg;
            [a.cos(), a.sin(), 0.0]
        };
        vec![top(0), top(1), top(2), bot(0), bot(1), bot(2)]
    }

    /// Re-derive the unit member direction `û = (x_k − x_j)/‖x_k − x_j‖` from the
    /// fixture coordinates — the column-`i` convention the equilibrium matrix
    /// must encode (`û` into node-`j`'s rows, `−û` into node-`k`'s rows).
    fn unit_dir(nodes: &[[f64; 3]], j: usize, k: usize) -> [f64; 3] {
        let d = [
            nodes[k][0] - nodes[j][0],
            nodes[k][1] - nodes[j][1],
            nodes[k][2] - nodes[j][2],
        ];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        [d[0] / l, d[1] / l, d[2] / l]
    }

    #[test]
    fn equilibrium_matrix_has_correct_shape_and_member_columns() {
        let nodes = canonical_prism();
        let members = triplex_members();
        let a = assemble_equilibrium_matrix(&nodes, &members);

        // (1) Shape: d·N rows × m columns = 18 × 12 (A·s = f, s = member forces).
        assert_eq!(a.nrows(), 3 * nodes.len(), "A must have d·N = 18 rows");
        assert_eq!(
            a.ncols(),
            members.len(),
            "A must have one column per member",
        );

        // (2) Column structure for one strut (member 0 = (0,4)) and one cable
        // (member 3 = (0,1)): û in node-j's three rows, −û in node-k's three
        // rows, 0 everywhere else, in node-major / axis-minor row order (3a+α).
        const TOL: f64 = 1e-12;
        for &col in &[0_usize, 3] {
            let (j, k) = members[col];
            let u = unit_dir(&nodes, j, k);
            for row in 0..a.nrows() {
                let node = row / 3;
                let axis = row % 3;
                let expected = if node == j {
                    u[axis]
                } else if node == k {
                    -u[axis]
                } else {
                    0.0
                };
                assert!(
                    (a[(row, col)] - expected).abs() < TOL,
                    "A[{row},{col}] (member {col} = ({j},{k}), node {node} axis {axis}) = {}, expected {expected}",
                    a[(row, col)],
                );
            }
        }
    }

    /// A planar open square: 4 coplanar nodes of the unit square (z = 0). The
    /// floppy reference — with edge-only cables it carries no self-stress (two
    /// perpendicular tensions cannot self-balance), the s = 0 signal that makes
    /// a framework prestress-unstable.
    fn open_square() -> Vec<[f64; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]
    }

    /// The four edge members of the open square, in ring order.
    fn square_members() -> Vec<(usize, usize)> {
        vec![(0, 1), (1, 2), (2, 3), (3, 0)]
    }

    #[test]
    fn self_stress_count_is_one_for_prism_and_zero_for_open_square() {
        // Canonical triplex: rank(A) = 11 over m = 12 members ⇒ exactly one
        // self-stress state (the published prestress) — the s ≥ 1 that a valid
        // tensegrity needs (PRD §5).
        let a_prism = assemble_equilibrium_matrix(&canonical_prism(), &triplex_members());
        assert_eq!(
            count_self_stress_states(&a_prism),
            1,
            "canonical triplex must have exactly one self-stress state",
        );

        // Planar open square: 4 independent edge directions ⇒ rank(A) = 4 = m ⇒
        // nullity(A) = 0, no self-stress.
        let a_square = assemble_equilibrium_matrix(&open_square(), &square_members());
        assert_eq!(
            count_self_stress_states(&a_square),
            0,
            "planar open square has no self-stress state",
        );
    }
}
