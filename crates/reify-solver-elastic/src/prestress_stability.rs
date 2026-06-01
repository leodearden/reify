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

/// Dot product of two equal-length DOF vectors stored as slices.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Modified Gram–Schmidt orthonormalisation of a set of column vectors.
///
/// Returns an orthonormal basis of `span(vectors)`; any vector whose residual
/// norm (after subtracting its projection onto the already-accepted basis) falls
/// to `drop_tol` is linearly dependent on the accepted set and is dropped — so
/// the returned count is the numerical rank of the input. The inputs here are
/// unit-norm eigenvectors / rigid-body generators, so a residual at `drop_tol`
/// (≈ 1e-8) is numerically zero against the O(1) surviving directions (the same
/// wide-gap rationale as [`NULLITY_REL_TOL`]).
fn orthonormalize_columns(vectors: &[Vec<f64>], drop_tol: f64) -> Vec<Vec<f64>> {
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for v in vectors {
        let mut w = v.clone();
        for b in &basis {
            let proj = dot(&w, b);
            for (wi, bi) in w.iter_mut().zip(b.iter()) {
                *wi -= proj * bi;
            }
        }
        let norm = dot(&w, &w).sqrt();
        if norm > drop_tol {
            for wi in w.iter_mut() {
                *wi /= norm;
            }
            basis.push(w);
        }
    }
    basis
}

/// Form the Gram matrix `AAᵀ` (`p × p`) of `a` (`p × m`) by explicit
/// accumulation. `AAᵀ` is symmetric PSD and shares `A`'s left null space
/// (`AAᵀ v = 0 ⟺ Aᵀ v = 0`, since `vᵀAAᵀv = ‖Aᵀv‖²`), so its zero-eigenvalue
/// eigenvectors span `null(Aᵀ)` — the infinitesimal mechanisms before
/// rigid-body removal.
fn gram_self_transpose(a: &Mat<f64>) -> Mat<f64> {
    let p = a.nrows();
    let m = a.ncols();
    let mut gram = Mat::<f64>::zeros(p, p);
    for i in 0..p {
        for j in 0..p {
            let mut acc = 0.0;
            for c in 0..m {
                acc += a[(i, c)] * a[(j, c)];
            }
            gram[(i, j)] = acc;
        }
    }
    gram
}

/// Orthonormal basis of the null space of a symmetric Gram matrix: its
/// eigenvectors whose eigenvalue magnitude is `≤ rel_tol · max|λ|`.
///
/// Mirrors [`crate::form_find_free`]'s `classify_spectrum` (dense
/// `self_adjoint_eigen`, relative threshold), returning the null eigenvectors as
/// owned columns. The eigenvectors of a real symmetric matrix are orthonormal,
/// so the returned set is already an orthonormal null-space basis.
fn null_space_basis(gram: &Mat<f64>, rel_tol: f64) -> Vec<Vec<f64>> {
    let n = gram.nrows();
    if n == 0 {
        return Vec::new();
    }
    let eig = gram
        .self_adjoint_eigen(Side::Lower)
        .expect("Gram matrix is real symmetric PSD; self-adjoint EVD must succeed");
    let s = eig.S();
    let u = eig.U();
    let eigenvalues: Vec<f64> = (0..n).map(|i| s[i]).collect();
    let max_mag = eigenvalues.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    let threshold = rel_tol * max_mag;
    let mut basis = Vec::new();
    for (i, &lambda) in eigenvalues.iter().enumerate() {
        if lambda.abs() <= threshold {
            basis.push((0..n).map(|r| u[(r, i)]).collect());
        }
    }
    basis
}

/// Assemble the six rigid-body generators of `nodes` as columns of a `3N × 6`
/// matrix, in node-major / axis-minor DOF order (`3r + α`).
///
/// Columns 0–2 are the unit translations along `x`, `y`, `z` (every node moves
/// by `eₐ`); columns 3–5 are the infinitesimal rotations about the `x`, `y`, `z`
/// axes through the centroid — node `r`'s velocity is `ω × (xᵣ − centroid)`.
/// Together they span the motions that carry no internal strain;
/// [`extract_internal_mechanisms`] projects this span out of `null(Aᵀ)` to
/// isolate the *internal* mechanisms. The columns are intentionally NOT
/// pre-orthonormalised — the caller orthonormalises them, which robustly recovers
/// the true rigid rank for degenerate/planar geometry (where it may be < 6).
fn rigid_body_modes(nodes: &[[f64; 3]]) -> Mat<f64> {
    let n = nodes.len();
    let mut centroid = [0.0_f64; 3];
    for node in nodes {
        for (axis, c) in centroid.iter_mut().enumerate() {
            *c += node[axis];
        }
    }
    if n > 0 {
        for c in centroid.iter_mut() {
            *c /= n as f64;
        }
    }

    let mut modes = Mat::<f64>::zeros(3 * n, 6);
    for (r, node) in nodes.iter().enumerate() {
        // Translations: unit displacement of node r along each axis.
        for axis in 0..3 {
            modes[(3 * r + axis, axis)] = 1.0;
        }
        // Infinitesimal rotations: velocity = ω × d, d = node − centroid.
        let d = [
            node[0] - centroid[0],
            node[1] - centroid[1],
            node[2] - centroid[2],
        ];
        // ω = x-axis ⇒ (0, −d_z, d_y)
        modes[(3 * r + 1, 3)] = -d[2];
        modes[(3 * r + 2, 3)] = d[1];
        // ω = y-axis ⇒ (d_z, 0, −d_x)
        modes[(3 * r, 4)] = d[2];
        modes[(3 * r + 2, 4)] = -d[0];
        // ω = z-axis ⇒ (−d_y, d_x, 0)
        modes[(3 * r, 5)] = -d[1];
        modes[(3 * r + 1, 5)] = d[0];
    }
    modes
}

/// Extract an orthonormal basis of the *internal* infinitesimal mechanisms of
/// the framework: `null(Aᵀ)` with the rigid-body span projected out.
///
/// 1. `null(Aᵀ)` = the zero-eigenvalue eigenvectors of `AAᵀ`
///    ([`null_space_basis`] on [`gram_self_transpose`]).
/// 2. Orthonormalise the six rigid-body generators ([`rigid_body_modes`]); the
///    surviving count is the true rigid rank `n_rigid` (6 for a non-degenerate
///    3-D form, fewer for planar/collinear node sets).
/// 3. Subtract the rigid-span projection from each `null(Aᵀ)` vector and
///    re-orthonormalise; vectors that were entirely rigid collapse below the drop
///    tolerance and are removed.
///
/// The returned matrix is `3N × m_count` with orthonormal columns, where
/// `m_count = nullity(Aᵀ) − n_rigid` is the internal mechanism count (1 for the
/// canonical triplex: its single non-affine prism twist).
pub(crate) fn extract_internal_mechanisms(a: &Mat<f64>, nodes: &[[f64; 3]]) -> Mat<f64> {
    let dim = a.nrows();

    // 1. null(Aᵀ): left null space of A via the zero eigenvectors of AAᵀ.
    let gram = gram_self_transpose(a);
    let null_vectors = null_space_basis(&gram, NULLITY_REL_TOL);

    // 2. Orthonormal rigid-body basis (rank = n_rigid).
    let rigid = rigid_body_modes(nodes);
    let rigid_cols: Vec<Vec<f64>> = (0..rigid.ncols())
        .map(|c| (0..dim).map(|r| rigid[(r, c)]).collect())
        .collect();
    let rigid_basis = orthonormalize_columns(&rigid_cols, NULLITY_REL_TOL);

    // 3. Project the rigid span out of each null(Aᵀ) vector.
    let mut projected: Vec<Vec<f64>> = Vec::with_capacity(null_vectors.len());
    for v in &null_vectors {
        let mut w = v.clone();
        for rb in &rigid_basis {
            let proj = dot(&w, rb);
            for (wi, ri) in w.iter_mut().zip(rb.iter()) {
                *wi -= proj * ri;
            }
        }
        projected.push(w);
    }

    // 4. Re-orthonormalise; purely-rigid residuals drop out, leaving the
    //    internal mechanism basis.
    let mechanisms = orthonormalize_columns(&projected, NULLITY_REL_TOL);

    let m_count = mechanisms.len();
    let mut out = Mat::<f64>::zeros(dim, m_count);
    for (c, col) in mechanisms.iter().enumerate() {
        for (r, &val) in col.iter().enumerate() {
            out[(r, c)] = val;
        }
    }
    out
}

/// Assemble the geometric / stress stiffness `K_G = D ⊗ I₃` (`3N × 3N`) from the
/// layer-2 force-density matrix `D = CᵀQC`.
///
/// `D` ([`crate::form_find_free::assemble_force_density_matrix`]) is reused
/// verbatim (PRD §5 "shares layer 2's core"), then expanded by the Kronecker
/// product with the 3×3 identity: `K_G[3a+α, 3b+α] = D[a,b]` for each axis
/// `α ∈ {0,1,2}`, with every off-axis (`α≠β`) entry zero. There is NO sign flip —
/// `q` already encodes cable(+)/strut(−), so `K_G` is the prestress energy
/// Hessian directly (contrast the buckling kernel's `−K_g`).
pub(crate) fn assemble_geometric_stiffness(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
) -> Mat<f64> {
    let d = crate::form_find_free::assemble_force_density_matrix(n, members, q);
    let mut k_g = Mat::<f64>::zeros(3 * n, 3 * n);
    for a in 0..n {
        for b in 0..n {
            let dab = d[(a, b)];
            for axis in 0..3 {
                k_g[(3 * a + axis, 3 * b + axis)] = dab;
            }
        }
    }
    k_g
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

    #[test]
    fn internal_mechanism_subspace_is_rigid_free_and_counts_one_for_prism() {
        let nodes = canonical_prism();
        let a = assemble_equilibrium_matrix(&nodes, &triplex_members());

        // Internal (rigid-body-excluded) mechanism basis of the canonical
        // triplex: nullity(Aᵀ) = 7, minus n_rigid = 6 rigid-body modes ⇒ exactly
        // one internal infinitesimal mechanism (the textbook prism twist).
        let basis = extract_internal_mechanisms(&a, &nodes);
        assert_eq!(
            basis.ncols(),
            1,
            "canonical triplex has exactly one internal mechanism",
        );
        assert_eq!(
            basis.nrows(),
            3 * nodes.len(),
            "mechanism vectors live in the d·N DOF space",
        );

        const TOL: f64 = 1e-9;

        // (1) Columns are orthonormal: BᵀB ≈ I (1×1 ⇒ ≈ 1 for the prism, but
        // checked generally so the property holds for any returned basis width).
        let k = basis.ncols();
        for i in 0..k {
            for j in 0..k {
                let mut dot = 0.0_f64;
                for r in 0..basis.nrows() {
                    dot += basis[(r, i)] * basis[(r, j)];
                }
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (dot - expected).abs() < TOL,
                    "BᵀB[{i},{j}] = {dot}, expected {expected} (mechanism basis must be orthonormal)",
                );
            }
        }

        // (2) Each mechanism column is orthogonal to the full rigid-body span
        // (3 translations + 3 infinitesimal rotations): the reported mechanism is
        // purely internal, carrying no net translation or rotation.
        let rigid = rigid_body_modes(&nodes);
        assert_eq!(
            rigid.ncols(),
            6,
            "6 rigid-body generators (3 translations + 3 rotations)",
        );
        assert_eq!(rigid.nrows(), 3 * nodes.len());
        for c in 0..basis.ncols() {
            for rb in 0..rigid.ncols() {
                let mut dot = 0.0_f64;
                for r in 0..basis.nrows() {
                    dot += basis[(r, c)] * rigid[(r, rb)];
                }
                assert!(
                    dot.abs() < TOL,
                    "mechanism column {c} · rigid mode {rb} = {dot}, must be ~0 (rigid-free)",
                );
            }
        }
    }

    /// Closed-form force densities for the symmetric prism, struts-then-cables
    /// order: struts −√3, the six horizontals +1, verticals +√3 (identical to
    /// `form_find_free.rs`). These make `D` rank-deficient by exactly 4
    /// (D eigenvalues 0,0,0,0,6,6) — the super-stable golden spectrum.
    fn closed_form_q() -> Vec<f64> {
        let s = 3.0_f64.sqrt();
        vec![
            -s, -s, -s, // struts
            1.0, 1.0, 1.0, // top horizontals
            1.0, 1.0, 1.0, // bottom horizontals
            s, s, s, // verticals
        ]
    }

    #[test]
    fn geometric_stiffness_is_force_density_kron_identity() {
        let members = triplex_members();
        let q = closed_form_q();
        let n = 6;

        let k_g = assemble_geometric_stiffness(n, &members, &q);
        assert_eq!(k_g.nrows(), 3 * n, "K_G is 3N×3N");
        assert_eq!(k_g.ncols(), 3 * n);

        // K_G = D ⊗ I₃: the on-axis block (α=β) replicates D[a,b]; every
        // off-axis entry (α≠β) is zero. Compare against the layer-2 D assembly
        // the kernel reuses verbatim.
        let d = crate::form_find_free::assemble_force_density_matrix(n, &members, &q);
        const TOL: f64 = 1e-12;
        for a in 0..n {
            for b in 0..n {
                for alpha in 0..3 {
                    for beta in 0..3 {
                        let got: f64 = k_g[(3 * a + alpha, 3 * b + beta)];
                        let expected: f64 = if alpha == beta { d[(a, b)] } else { 0.0 };
                        assert!(
                            (got - expected).abs() < TOL,
                            "K_G[{},{}] (a={a}, b={b}, α={alpha}, β={beta}) = {got}, expected {expected}",
                            3 * a + alpha,
                            3 * b + beta,
                        );
                    }
                }
            }
        }
    }
}
