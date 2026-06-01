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

/// The self-stress / mechanism / stability verdict of a prestressed framework,
/// as reported by [`analyze_prestress_stability`] — the five PRD §5 output fields.
#[derive(Debug, Clone, PartialEq)]
pub struct StabilityResult {
    /// Number of independent self-stress states `s = nullity(A)`: the dimension of
    /// the space of self-equilibrated prestresses. A valid tensegrity needs
    /// `s ≥ 1` (PRD §5); `s == 0` forces [`stable`](StabilityResult::stable) false.
    pub self_stress_states: usize,
    /// Number of internal infinitesimal mechanisms `nullity(Aᵀ) − n_rigid` (the
    /// rigid-body-excluded count — 1 for the canonical triplex's prism twist).
    pub mechanisms: usize,
    /// Maxwell number `m − d·N` (the raw integer; `−6` for the triplex). Related to
    /// `s` and the total kinematic indeterminacy by Calladine's identity
    /// `m − d·N = s − m_kin`, with `m_kin = nullity(Aᵀ)` (rigid-inclusive).
    pub maxwell: i64,
    /// Whether the framework is prestress-stable: it has self-stress (`s ≥ 1`) and
    /// the reduced geometric stiffness `Mᵀ K_G M` is positive definite on the
    /// internal-mechanism subspace (the prestress stiffens every mechanism).
    pub stable: bool,
    /// Whether the framework satisfies the algebraic conditions of Connelly
    /// super-stability (`D` PSD ∧ `rank(D) == N − d − 1`). See [`is_super_stable`]
    /// for the intentionally-deferred conic-at-infinity condition.
    pub super_stable: bool,
}

/// Reason a prestress-stability analysis cannot run. Mirrors the
/// [`crate::form_find_free::FreeFormError`] diagnostic-enum precedent: malformed
/// input becomes a clean typed error, never a panic or a silently-wrong result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StabilityError {
    /// The input arrays disagree in length (`members.len() != q.len()`), or a
    /// member references a node index `≥ nodes.len()`.
    DimensionMismatch,
    /// A member is geometrically degenerate: a self-loop (`j == k`) or two
    /// (near-)coincident endpoints. Such a member has zero length, so its unit
    /// direction `û = d/L` is `NaN`/`inf` — left unchecked it would silently
    /// poison the equilibrium matrix, the self-stress count, and the final
    /// verdict, contradicting this enum's clean-typed-error contract.
    DegenerateMember,
}

/// Analyse the self-stress / mechanism / prestress-stability of a prestressed
/// framework (PRD `docs/prds/v0_6/tensegrity-structures.md` §5, task T2).
///
/// Given a realised geometry `nodes`, a member topology `members` (node-index
/// pairs), and per-member force densities `q` (cables `> 0`, struts `< 0`),
/// returns the [`StabilityResult`] with the five PRD §5 fields. The method is the
/// classical Pellegrino–Calladine equilibrium-matrix analysis combined with the
/// force-density geometric stiffness `K_G = D ⊗ I₃`; see the module-level docs for
/// the full derivation and the reuse of layer-2's `D = CᵀQC`.
///
/// # Errors
///
/// Returns [`StabilityError::DimensionMismatch`] if `members.len() != q.len()` or
/// any member references a node index `≥ nodes.len()`; returns
/// [`StabilityError::DegenerateMember`] if any member is a self-loop or joins two
/// (near-)coincident nodes (a zero-length member has no well-defined direction).
pub fn analyze_prestress_stability(
    nodes: &[[f64; 3]],
    members: &[(usize, usize)],
    q: &[f64],
) -> Result<StabilityResult, StabilityError> {
    // Guard 1: exactly one force density per member.
    if members.len() != q.len() {
        return Err(StabilityError::DimensionMismatch);
    }
    // Guard 2: every member references an in-range node — otherwise the
    // equilibrium / force-density assembly would index out of bounds and panic.
    let n = nodes.len();
    if members.iter().any(|&(j, k)| j >= n || k >= n) {
        return Err(StabilityError::DimensionMismatch);
    }
    // Guard 3: every member has a well-defined direction. A self-loop or two
    // (near-)coincident endpoints give a zero-length member, whose unit direction
    // û = d/L is NaN/inf; that would propagate silently through the equilibrium
    // matrix, the spectral counts, and the verdict (a silently-wrong result rather
    // than the clean typed error this module promises). Indices are already
    // in-range (Guard 2), so the coordinate lookups here cannot panic.
    if members
        .iter()
        .any(|&(j, k)| is_degenerate_member(nodes, j, k))
    {
        return Err(StabilityError::DegenerateMember);
    }

    // Equilibrium matrix A; self-stress count s = nullity(A) = m − rank(A).
    let a = assemble_equilibrium_matrix(nodes, members);
    let self_stress_states = count_self_stress_states(&a);

    // Internal (rigid-body-excluded) infinitesimal mechanism subspace.
    let mech_basis = extract_internal_mechanisms(&a, nodes);
    let m_count = mech_basis.ncols();

    // Maxwell number m − d·N (raw integer; −6 for the triplex). d = 3 (3-D).
    let maxwell = members.len() as i64 - 3 * (n as i64);

    // Super-stability: the algebraic Connelly conditions on D (d = 3).
    let super_stable = is_super_stable(n, members, q, 3);

    // Prestress stability, with the PRD §5 short-circuits:
    //   s == 0       ⇒ no prestress to stabilise ⇒ not stable.
    //   m_count == 0 ⇒ already rigid, nothing to destabilise ⇒ stable (s ≥ 1).
    // Otherwise the reduced geometric stiffness Mᵀ K_G M must be positive
    // definite: its algebraic minimum eigenvalue must clear a relative threshold
    // scaled by K_G's magnitude — the same wide-gap rationale as NULLITY_REL_TOL
    // (a genuine positive eigenvalue is O(scale); a numerical zero is O(eps·scale)).
    // The s ≥ 1 ∧ m_count ≥ 1 guard also keeps min_eigenvalue_on_subspace away
    // from an ill-posed 0-mode eigensolve.
    let stable = if self_stress_states == 0 {
        false
    } else if m_count == 0 {
        true
    } else {
        let k_g = assemble_geometric_stiffness(n, members, q);
        let scale = max_abs_entry(&k_g);
        min_eigenvalue_on_subspace(&k_g, &mech_basis) > NULLITY_REL_TOL * scale
    };

    Ok(StabilityResult {
        self_stress_states,
        mechanisms: m_count,
        maxwell,
        stable,
        super_stable,
    })
}

/// Largest-magnitude entry of a matrix — a cheap, eigendecomposition-free proxy
/// for its spectral scale. Used to set the *relative* prestress-stability
/// threshold in [`analyze_prestress_stability`]: `max|entry|` is within an
/// `O(N)` factor of `‖K_G‖₂`, which is ample for separating a genuine `O(scale)`
/// positive eigenvalue from an `O(eps·scale)` numerical zero.
fn max_abs_entry(m: &Mat<f64>) -> f64 {
    let mut s = 0.0_f64;
    for i in 0..m.nrows() {
        for j in 0..m.ncols() {
            s = s.max(m[(i, j)].abs());
        }
    }
    s
}

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

/// Whether member `(j, k)` is geometrically degenerate — a self-loop (`j == k`)
/// or two (near-)coincident endpoints whose separation collapses to zero.
///
/// A degenerate member has no well-defined direction: `û = (x_k − x_j)/L` becomes
/// `NaN`/`inf` once `L → 0`, which would silently corrupt every downstream
/// quantity. The separation is judged *relative* to the endpoints' own coordinate
/// magnitudes (the same scale-free philosophy as [`NULLITY_REL_TOL`]), so the test
/// is invariant to the overall size of the structure: with both endpoints at the
/// origin the scale is zero and the exact-coincidence `0 ≤ 0` still trips, while
/// distant-but-coincident nodes trip via the relative bound. Callers must ensure
/// `j` and `k` are in range (this indexes `nodes`).
fn is_degenerate_member(nodes: &[[f64; 3]], j: usize, k: usize) -> bool {
    if j == k {
        return true;
    }
    let d = [
        nodes[k][0] - nodes[j][0],
        nodes[k][1] - nodes[j][1],
        nodes[k][2] - nodes[j][2],
    ];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let mag = |p: &[f64; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    let scale = mag(&nodes[j]) + mag(&nodes[k]);
    // Non-finite length (overflow / NaN coordinates) is also rejected as degenerate.
    !len.is_finite() || len <= NULLITY_REL_TOL * scale
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

/// Algebraic minimum eigenvalue of the reduced stiffness `Mᵀ K_G M` on the
/// mechanism subspace spanned by the orthonormal columns of `basis`.
///
/// Forms the dense reduced matrix `K_red = basisᵀ · K_G · basis`
/// (`m_count × m_count`), wraps it and an identity of the same size as faer
/// [`SparseRowMat`]s, and reuses the buckling **dense** generalized eigensolver
/// path [`crate::eigensolve::solve_eigen_dense`] on the `(K_red, I)` pair (PRD
/// §5/§7 GR-024 reuse seam). With `B = I` the generalized problem collapses to
/// the standard symmetric spectrum and no degenerate-β filtering occurs, so all
/// `m_count` eigenvalues are returned. `solve_eigen_dense` sorts them by `|λ|`,
/// but prestress stability needs the algebraic sign, so we return the algebraic
/// minimum (`K_red ≻ 0 ⟺ min > 0`).
///
/// The `m_count == 1` case (e.g. the canonical triplex's single mechanism) is
/// returned in closed form: a 1×1 symmetric matrix's only eigenvalue *is* its
/// scalar entry. This is also a hard requirement, not just an optimisation —
/// faer's dense QZ (the engine inside `solve_eigen_dense`) requires `n ≥ 2` for
/// its scratch-buffer allocation and panics on a 1×1 input (documented in
/// `tests/joint_stiffness_modal_frequency.rs`). The general `m_count ≥ 2`
/// reduced problem reuses the dense eigensolver path as prescribed.
///
/// Callers only invoke this with `m_count ≥ 1` (the short-circuits in
/// [`analyze_prestress_stability`] handle the `m_count == 0` case).
fn min_eigenvalue_on_subspace(k_g: &Mat<f64>, basis: &Mat<f64>) -> f64 {
    use crate::eigensolve::{EigenSolverOptions, solve_eigen_dense};
    use faer::sparse::{SparseRowMat, Triplet};

    let dim = basis.nrows();
    let m_count = basis.ncols();

    // W = K_G · basis (dim × m_count), then K_red = basisᵀ · W
    // (m_count × m_count). m_count is tiny (1 for the prism), so the explicit
    // triple loops are clear and cheap.
    let mut w = Mat::<f64>::zeros(dim, m_count);
    for c in 0..m_count {
        for i in 0..dim {
            let mut acc = 0.0;
            for j in 0..dim {
                acc += k_g[(i, j)] * basis[(j, c)];
            }
            w[(i, c)] = acc;
        }
    }
    let mut k_red = Mat::<f64>::zeros(m_count, m_count);
    for a in 0..m_count {
        for b in 0..m_count {
            let mut acc = 0.0;
            for i in 0..dim {
                acc += basis[(i, a)] * w[(i, b)];
            }
            k_red[(a, b)] = acc;
        }
    }

    // A 1×1 reduced matrix has its scalar entry as its sole eigenvalue. Take it
    // directly: faer's dense QZ requires n ≥ 2 and panics on a 1×1 input.
    if m_count == 1 {
        return k_red[(0, 0)];
    }

    // Wrap (K_red, I) as SparseRowMat (skip structural zeros) and reuse the
    // buckling dense eigensolver path.
    let mut k_trips: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(m_count * m_count);
    for a in 0..m_count {
        for b in 0..m_count {
            let v = k_red[(a, b)];
            if v != 0.0 {
                k_trips.push(Triplet::new(a, b, v));
            }
        }
    }
    let k_sp = SparseRowMat::try_new_from_triplets(m_count, m_count, &k_trips)
        .expect("reduced-stiffness triplets are square and in range");
    let id_trips: Vec<Triplet<usize, usize, f64>> =
        (0..m_count).map(|i| Triplet::new(i, i, 1.0)).collect();
    let id_sp = SparseRowMat::try_new_from_triplets(m_count, m_count, &id_trips)
        .expect("identity triplets are square and in range");

    let opts = EigenSolverOptions {
        n_modes: m_count,
        ..Default::default()
    };
    let res = solve_eigen_dense(&k_sp, &id_sp, opts);

    // Algebraic minimum (NOT |λ|-minimum) — the sign is the stability verdict.
    res.eigenvalues
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
}

/// Connelly super-stability verdict (algebraic conditions only): the force-density
/// matrix `D = CᵀQC` is positive-semidefinite **and** has rank exactly `N − d − 1`.
///
/// `D` ([`crate::form_find_free::assemble_force_density_matrix`]) is the same
/// layer-2 stress matrix reused by [`assemble_geometric_stiffness`]; its spectrum
/// is classified with the dense self-adjoint pattern shared across this kernel
/// (faer `self_adjoint_eigen`, [`NULLITY_REL_TOL`] relative threshold):
///
/// * **PSD**: the algebraic minimum eigenvalue exceeds `−rel_tol · max|λ|` — no
///   eigenvalue is meaningfully negative. For the canonical triplex `D`'s spectrum
///   is `{0,0,0,0,6,6}`, so the minimum sits at the numerical-zero floor ⇒ PSD.
/// * **Rank**: the count of eigenvalues with `|λ| > rel_tol · max|λ|` equals
///   `N − d − 1` (= 2 for the prism). Written addition-side (`rank + d + 1 == n`)
///   to avoid `usize` underflow when `d + 1 > n` (a degenerate/under-specified
///   form, which is then trivially not super-stable).
///
/// # Deferred condition
///
/// Connelly's full super-stability theorem adds a third requirement — the member
/// directions must not lie on a conic at infinity. That projective check is a
/// degenerate-geometry guard that does NOT change the verdict for generic
/// non-degenerate forms (including the canonical triplex, which is genuinely
/// super-stable), so it is intentionally NOT implemented here and is recorded as a
/// documented scope boundary / follow-up. `is_super_stable` therefore means
/// precisely "satisfies the algebraic PSD + rank conditions of Connelly
/// super-stability".
fn is_super_stable(n: usize, members: &[(usize, usize)], q: &[f64], d: usize) -> bool {
    if n == 0 {
        return false;
    }
    let d_mat = crate::form_find_free::assemble_force_density_matrix(n, members, q);
    let eig = d_mat
        .self_adjoint_eigen(Side::Lower)
        .expect("force-density matrix D is real symmetric; self-adjoint EVD must succeed");
    let s = eig.S();
    let eigenvalues: Vec<f64> = (0..n).map(|i| s[i]).collect();
    let max_mag = eigenvalues.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    let threshold = NULLITY_REL_TOL * max_mag;

    // PSD: no eigenvalue is meaningfully negative (algebraic min above −threshold).
    let min_lambda = eigenvalues.iter().copied().fold(f64::INFINITY, f64::min);
    let psd = min_lambda > -threshold;

    // rank(D) = count of eigenvalues clear of the relative null threshold.
    let rank = eigenvalues.iter().filter(|v| v.abs() > threshold).count();

    psd && rank + d + 1 == n
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

    #[test]
    fn reduced_subspace_min_eigenvalue_positive_for_prism_negative_for_indefinite() {
        // (a) Canonical prism: its single internal mechanism is non-affine, so it
        // is NOT in null(K_G) and the reduced stiffness Mᵀ K_G M (1×1 here) is
        // strictly positive ⇒ prestress-stable.
        let nodes = canonical_prism();
        let members = triplex_members();
        let a = assemble_equilibrium_matrix(&nodes, &members);
        let basis = extract_internal_mechanisms(&a, &nodes);
        let k_g = assemble_geometric_stiffness(6, &members, &closed_form_q());
        let min_prism: f64 = min_eigenvalue_on_subspace(&k_g, &basis);
        assert!(
            min_prism > 1e-6,
            "prism's non-affine mechanism gives strictly positive reduced stiffness (stable), got {min_prism}",
        );

        // (b) Deterministic indefinite case: a 2-column orthonormal basis selects
        // coordinates 0 and 2 of a diagonal K_G whose entries there are +3 and
        // −0.5 (the +99 on coordinate 1 is outside the subspace). The reduced
        // matrix is diag(3, −0.5), so its algebraic minimum is −0.5 < 0. Pins the
        // negative branch without a physically-realised unstable tensegrity.
        let mut k_indef = Mat::<f64>::zeros(3, 3);
        k_indef[(0, 0)] = 3.0;
        k_indef[(1, 1)] = 99.0;
        k_indef[(2, 2)] = -0.5;
        let mut sel = Mat::<f64>::zeros(3, 2);
        sel[(0, 0)] = 1.0; // column 0 selects coordinate 0 (K_G = +3)
        sel[(2, 1)] = 1.0; // column 1 selects coordinate 2 (K_G = −0.5)
        let min_indef: f64 = min_eigenvalue_on_subspace(&k_indef, &sel);
        assert!(
            min_indef < 0.0,
            "reduced diag(3, −0.5) must have a negative algebraic minimum, got {min_indef}",
        );
    }

    /// A *generic* admissible q in struts-then-cables order: distinct per-member
    /// magnitudes, struts negative and cables positive. Mirrors
    /// `form_find_free.rs`'s `generic_admissible_q` (same `1 + 0.37·i` magnitudes,
    /// signs by member kind) but keyed off the known triplex layout — the first
    /// three members are the struts. Distinct magnitudes break the prism's `C₃ ×
    /// top/bottom` symmetry, so `D` keeps only the all-ones translation mode in its
    /// null space (nullity 1) ⇒ rank 5 ≠ N − d − 1 = 2 — the non-super-stable
    /// discriminator (contrast the closed-form q's rank-2 super-stable spectrum).
    fn generic_admissible_q() -> Vec<f64> {
        (0..triplex_members().len())
            .map(|i| {
                let mag = 1.0 + 0.37 * (i as f64);
                if i < 3 { -mag } else { mag }
            })
            .collect()
    }

    #[test]
    fn super_stable_true_for_prism_false_for_wrong_rank() {
        let members = triplex_members();

        // Canonical prism + closed-form q: D's spectrum is {0,0,0,0,6,6} — PSD
        // (min λ = 0, no negative eigenvalue) and rank 2 == N − d − 1 = 6 − 3 − 1.
        // Both algebraic Connelly super-stability conditions hold ⇒ super_stable.
        assert!(
            is_super_stable(6, &members, &closed_form_q(), 3),
            "canonical triplex with closed-form q is super-stable (D PSD, rank 2 = N−d−1)",
        );

        // Generic admissible q: distinct magnitudes leave only the translation
        // mode in null(D), so rank(D) = 5 ≠ N − d − 1 = 2. The rank condition
        // fails ⇒ NOT super-stable (verdict is false regardless of the PSD test).
        assert!(
            !is_super_stable(6, &members, &generic_admissible_q(), 3),
            "generic admissible q gives D rank 5 ≠ N−d−1 = 2 ⇒ not super-stable",
        );
    }

    #[test]
    fn analyze_prestress_stability_reports_prism_fields_and_guards_dims() {
        let nodes = canonical_prism();
        let members = triplex_members();

        // Canonical triplex + closed-form q is the PRD §5 golden: one self-stress
        // state, one internal mechanism, Maxwell number m − d·N = 12 − 18 = −6,
        // prestress-stable, and super-stable (D PSD with rank N−d−1 = 2).
        let result = analyze_prestress_stability(&nodes, &members, &closed_form_q())
            .expect("canonical prism + closed-form q is a well-formed analysis input");
        assert_eq!(
            result,
            StabilityResult {
                self_stress_states: 1,
                mechanisms: 1,
                maxwell: -6,
                stable: true,
                super_stable: true,
            },
        );

        // Guard: a members / q length disagreement is a clean DimensionMismatch
        // (a typed error through the public entry point, never a panic).
        let short_q = vec![1.0_f64; members.len() - 1];
        assert_eq!(
            analyze_prestress_stability(&nodes, &members, &short_q),
            Err(StabilityError::DimensionMismatch),
            "members.len() != q.len() must be DimensionMismatch",
        );

        // Guard: a member referencing a node index ≥ nodes.len() is out of range
        // ⇒ DimensionMismatch (which would otherwise panic on the coord lookup).
        let bad_members = vec![(0_usize, nodes.len())];
        let bad_q = vec![1.0_f64];
        assert_eq!(
            analyze_prestress_stability(&nodes, &bad_members, &bad_q),
            Err(StabilityError::DimensionMismatch),
            "a member node index ≥ nodes.len() must be DimensionMismatch",
        );
    }

    #[test]
    fn analyze_prestress_stability_rejects_degenerate_members() {
        // A self-loop (j == k) has no direction. It must surface as a clean
        // DegenerateMember error rather than a silently-NaN equilibrium matrix.
        let nodes = canonical_prism();
        let self_loop = vec![(0_usize, 0)];
        assert_eq!(
            analyze_prestress_stability(&nodes, &self_loop, &[1.0]),
            Err(StabilityError::DegenerateMember),
            "a self-loop member (j == k) must be DegenerateMember",
        );

        // Two coincident node coordinates ⇒ zero-length member ⇒ DegenerateMember
        // (û = d/L would be NaN/inf and poison every downstream quantity).
        let coincident = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        assert_eq!(
            analyze_prestress_stability(&coincident, &[(0_usize, 1)], &[1.0]),
            Err(StabilityError::DegenerateMember),
            "a member between coincident nodes must be DegenerateMember",
        );

        // The same coincidence away from the origin trips the *relative* bound,
        // confirming the guard is scale-free rather than keyed to an absolute floor.
        let coincident_offset = vec![[5.0, -2.0, 7.0], [5.0, -2.0, 7.0]];
        assert_eq!(
            analyze_prestress_stability(&coincident_offset, &[(0_usize, 1)], &[1.0]),
            Err(StabilityError::DegenerateMember),
            "coincident nodes far from the origin must still be DegenerateMember",
        );
    }
}
