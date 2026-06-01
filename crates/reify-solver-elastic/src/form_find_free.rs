//! Force-Density (FD) form-finding kernel — free-standing case (Tensegrity T1b).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-structures.md` Tier-1 leaf T1b. Where the anchored
//! kernel ([`crate::form_find`], T1a) takes the force densities `q` and a set of
//! anchored nodes as given and solves a reduced linear system for the free-node
//! coordinates, the *free-standing* kernel has **no anchors**: the whole
//! structure floats, and a self-stressed equilibrium exists only for special `q`
//! that make the force-density matrix `D = Cᵀ Q C` rank-deficient by exactly
//! `d + 1 = 4` (three coordinate null directions plus the always-present
//! all-ones translation mode).
//!
//! # Method (free-standing case)
//!
//! 1. Assemble the full `N×N` force-density matrix `D = Cᵀ Q C` (the same rank-1
//!    per-member accumulation as the anchored kernel, but with no free/anchor
//!    partition).
//! 2. Classify the nullity of `D` via a dense self-adjoint eigendecomposition.
//!    A 3-D free-standing form requires nullity exactly `4`.
//! 3. Recover node coordinates from the null space, gauge-fixed by least-squares
//!    affine alignment to the caller's initial guess.
//! 4. Member forces are `Nᵢ = qᵢ · Lᵢ` on the recovered geometry.
//!
//! Two force-density specifications are supported (see [`ForceDensitySpec`]):
//! [`ForceDensitySpec::Explicit`] takes per-member `q` directly (pure linear
//! algebra), while [`ForceDensitySpec::GroupRatios`] runs an adaptive
//! eigenvalue-minimisation search that drives the `(d+1)`-th smallest eigenvalue
//! of `D` toward zero over the free relative group densities, then delegates to
//! the explicit path.
//!
//! # Sign convention
//!
//! Identical to the anchored kernel and shared via [`MemberKind`]: cables carry
//! tension (`q > 0`), struts carry compression (`q < 0`).
//!
//! # Scope
//!
//! Kernel only: this module does not touch the `.ri` example, the stdlib
//! `form_find` signature, or the reify-eval trampoline (those remain anchored-T1a
//! wired). See `plan.json` design_decisions for the scoping rationale.

use crate::form_find::MemberKind;
use faer::{Mat, Side};

/// How the per-member force densities `q` are specified for a free-standing
/// form-find.
#[derive(Debug, Clone, PartialEq)]
pub enum ForceDensitySpec {
    /// Explicit per-member force density `q`, in the same struts-then-cables
    /// member order as `members` / `kinds`. The deterministic foundation:
    /// assemble `D`, classify nullity, recover coordinates, compute forces.
    Explicit(Vec<f64>),
    /// Per-group *relative* densities discovered by the adaptive eigenvalue
    /// search. Members sharing a `group_id` move together; `seed_ratios` gives a
    /// signed starting ratio per group (its sign also fixes the group's
    /// tension / compression sense); `reference_group` is held fixed as the
    /// scale gauge (overall scaling of `q` is nullity-invariant, so only relative
    /// ratios vary).
    GroupRatios {
        /// Per-member group id, parallel to `members` / `kinds`.
        group_ids: Vec<usize>,
        /// Signed seed ratio per group, indexed by group id.
        seed_ratios: Vec<f64>,
        /// Group id held fixed as the reference (gauge) during the search.
        reference_group: usize,
    },
}

/// Result of a free-standing Force-Density form-find solve.
#[derive(Debug, Clone)]
pub struct FreeFormResult {
    /// Recovered free-standing node coordinates, in original node order,
    /// gauge-fixed by affine alignment to the caller's initial guess.
    pub nodes: Vec<[f64; 3]>,
    /// Per-member axial force `Nᵢ = qᵢ · Lᵢ` on the recovered geometry, in
    /// struts-then-cables member order (struts compressive, cables tensile).
    pub member_forces: Vec<f64>,
    /// The force densities used for the solve (an echo of the explicit `q`, or
    /// the densities found by the adaptive search), struts-then-cables order.
    pub force_densities: Vec<f64>,
    /// Nullity of `D` at the solution (a valid 3-D form has nullity `4`).
    pub nullity: usize,
    /// Whether the solve produced a valid free-standing equilibrium.
    pub converged: bool,
}

/// Reason a free-standing form-find is infeasible. Mirrors the
/// [`crate::form_find::FormFindError`] diagnostic-enum precedent: infeasible
/// input becomes a clean typed error, never a panic or a silently-wrong result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeFormError {
    /// A member's force density violates its kind's sign contract (a cable with
    /// `q ≤ 0` or a strut with `q ≥ 0`).
    SignViolation,
    /// The force-density matrix `D` is rank-deficient by the wrong amount: a
    /// valid 3-D free-standing form needs nullity exactly `d + 1 = 4`.
    NullityMismatch {
        /// Nullity actually observed in `D`'s spectrum.
        observed: usize,
        /// Nullity required for a valid form (`d + 1 = 4`).
        expected: usize,
    },
    /// Input arrays disagree in length (`members`, `kinds`, and the per-member
    /// `q` / `group_ids`).
    DimensionMismatch,
    /// The adaptive [`ForceDensitySpec::GroupRatios`] search exhausted its
    /// iteration budget without reaching a nullity-`4` configuration.
    SearchDidNotConverge,
    /// Null-space coordinate recovery was rank-deficient (the recovered basis did
    /// not span a 3-D realisation).
    SingularRecovery,
}

/// Solve the free-standing Force-Density form-finding problem.
///
/// `nodes_guess` is the caller's initial node placement (used only to gauge-fix
/// the recovered shape — its metric content is otherwise free). `members` are
/// `(start, end)` index pairs in struts-then-cables order, `kinds` tags each
/// member, and `spec` selects explicit-`q` or adaptive group-ratio form-finding.
///
/// Returns the solved [`FreeFormResult`] on success, or a [`FreeFormError`]
/// describing why the input is infeasible.
pub fn form_find_free(
    nodes_guess: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    spec: &ForceDensitySpec,
) -> Result<FreeFormResult, FreeFormError> {
    match spec {
        // Deterministic foundation: assemble D, classify nullity, recover
        // coordinates, compute forces.
        ForceDensitySpec::Explicit(q) => form_find_explicit(nodes_guess, members, kinds, q),
        // Adaptive eigenvalue-minimisation search over relative group densities;
        // on success it produces an admissible q and delegates to the explicit
        // path. Implemented in T1b step-10.
        ForceDensitySpec::GroupRatios { .. } => {
            unimplemented!("GroupRatios adaptive force-density search: implemented in T1b step-10")
        }
    }
}

/// Explicit-mode pipeline: validate the spec, recover the gauge-fixed
/// free-standing coordinates, and compute member forces — the deterministic core
/// that [`ForceDensitySpec::GroupRatios`] also delegates to once its search finds
/// an admissible `q`.
fn form_find_explicit(
    nodes_guess: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    q: &[f64],
) -> Result<FreeFormResult, FreeFormError> {
    let n = nodes_guess.len();

    // Up-front feasibility guards + the single dense EVD (length / sign / nullity).
    let spectrum = validate_explicit(n, members, kinds, q)?;
    let nullity = spectrum.nullity;

    // Gauge-fixed coordinates from null(D), aligned to the caller's guess.
    let nodes = recover_coordinates(nodes_guess, &spectrum)?;

    // Per-member axial force Nᵢ = qᵢ · Lᵢ on the recovered geometry, in
    // struts-then-cables (input) order — mirrors the anchored kernel's force pass.
    let member_forces: Vec<f64> = members
        .iter()
        .zip(q.iter())
        .map(|(&(j, k), &qi)| {
            let pj = nodes[j];
            let pk = nodes[k];
            let len = ((pj[0] - pk[0]).powi(2)
                + (pj[1] - pk[1]).powi(2)
                + (pj[2] - pk[2]).powi(2))
            .sqrt();
            qi * len
        })
        .collect();

    Ok(FreeFormResult {
        nodes,
        member_forces,
        force_densities: q.to_vec(),
        nullity,
        converged: true,
    })
}

// ---------------------------------------------------------------------------
// Crate-internal numeric helpers (D assembly + spectral nullity classification)
// ---------------------------------------------------------------------------

/// Relative tolerance for nullity classification: an eigenvalue counts as a null
/// direction when its magnitude is below this fraction of the largest-magnitude
/// eigenvalue. The prism golden has a wide spectral gap (fifth eigenvalue 6 vs
/// fourth ≈ 2.5e-15), so 1e-8 separates the null space from the rest of the
/// spectrum without a brittle absolute threshold.
const NULLITY_REL_TOL: f64 = 1e-8;

/// Spectral classification of the force-density matrix `D`.
#[derive(Debug)]
struct SpectrumClassification {
    /// Eigenvalues of `D`, sorted ascending by magnitude. Consumed by the
    /// step-10 adaptive search (which drives the `d+1`-th toward zero) and the
    /// spectral-gap unit tests; the explicit path reads only `nullity` /
    /// `eigenvectors`, so this field is otherwise unused for now.
    #[allow(dead_code)]
    eigenvalues: Vec<f64>,
    /// Eigenvectors as columns, in the same order as `eigenvalues` (column `j`
    /// is the eigenvector for `eigenvalues[j]`). The first `nullity` columns span
    /// the null space used for coordinate recovery.
    eigenvectors: Mat<f64>,
    /// Number of eigenvalues whose magnitude is below the relative tolerance —
    /// the nullity of `D`.
    nullity: usize,
}

/// Assemble the full `N×N` force-density matrix `D = Cᵀ Q C` for the whole
/// (unanchored) structure.
///
/// Reuses the anchored kernel's rank-1 per-member accumulation
/// (`crate::form_find::form_find_anchored`): for a member `(j, k)` with force
/// density `qᵢ`, add `qᵢ` to `D[j,j]` and `D[k,k]` and `−qᵢ` to `D[j,k]` and
/// `D[k,j]`. Unlike the anchored case there is no free/anchor partition — the
/// full `D` is what the eigenvalue / null-space form-finding operates on.
fn assemble_force_density_matrix(n: usize, members: &[(usize, usize)], q: &[f64]) -> Mat<f64> {
    let mut d = Mat::<f64>::zeros(n, n);
    for (&(j, k), &qi) in members.iter().zip(q.iter()) {
        d[(j, j)] += qi;
        d[(k, k)] += qi;
        d[(j, k)] -= qi;
        d[(k, j)] -= qi;
    }
    d
}

/// Classify the spectrum of the symmetric force-density matrix `D` via a dense
/// self-adjoint eigendecomposition.
///
/// Returns the eigenpairs sorted ascending by `|λ|` together with the nullity
/// (count of eigenvalues whose magnitude is below `rel_tol · max|λ|`). `D` is
/// indefinite by construction (struts contribute negative `q`), so the sort is
/// by magnitude, not algebraic value.
fn classify_spectrum(d: &Mat<f64>, rel_tol: f64) -> SpectrumClassification {
    let n = d.nrows();

    // Dense self-adjoint (symmetric standard) eigendecomposition. faer returns
    // eigenvalues in ascending *algebraic* order with eigenvectors as columns of
    // U. D is real symmetric by construction, so a failure here is a bug, not an
    // infeasible-input condition — panic with a descriptive message (matching the
    // eigensolve.rs `.expect` precedent) rather than threading an error.
    let eig = d
        .self_adjoint_eigen(Side::Lower)
        .expect("force-density matrix D is real symmetric; self-adjoint EVD must succeed");
    let s = eig.S();
    let u = eig.U();

    // D is indefinite (struts contribute negative q), so algebraic order is not
    // magnitude order. Reorder the eigenpairs ascending by |λ| — the null
    // directions are the smallest-magnitude ones, and recovery (step 6) takes the
    // leading `nullity` columns.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| s[a].abs().total_cmp(&s[b].abs()));

    let eigenvalues: Vec<f64> = order.iter().map(|&i| s[i]).collect();

    let mut eigenvectors = Mat::<f64>::zeros(n, n);
    for (new_col, &src_col) in order.iter().enumerate() {
        for r in 0..n {
            eigenvectors[(r, new_col)] = u[(r, src_col)];
        }
    }

    // Relative tolerance scaled by the largest-magnitude eigenvalue: a null
    // direction is one whose |λ| sits at or below tol·max|λ|. Scaling by max|λ|
    // keeps the threshold meaningful regardless of the overall force-density
    // scale, and the prism's wide spectral gap (≈0 vs 6) makes the count robust.
    let max_mag = eigenvalues.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    let threshold = rel_tol * max_mag;
    let nullity = eigenvalues.iter().filter(|v| v.abs() <= threshold).count();

    SpectrumClassification {
        eigenvalues,
        eigenvectors,
        nullity,
    }
}

/// Validate an explicit-mode force-density spec and classify `D`'s spectrum.
///
/// Runs the three up-front feasibility guards in order and returns the matching
/// [`FreeFormError`] on the first failure:
///
/// 1. length agreement across `members` / `kinds` / `q` ([`DimensionMismatch`]),
/// 2. the per-member sign contract — cables `q > 0`, struts `q < 0`, reusing
///    [`MemberKind`] ([`SignViolation`]),
/// 3. the nullity-equals-`d + 1` check via [`classify_spectrum`]
///    ([`NullityMismatch`], carrying observed vs expected).
///
/// On success returns the [`SpectrumClassification`] of `D` — computed for the
/// nullity check and reused by coordinate recovery (step 6) so the dense
/// eigendecomposition runs only once.
///
/// [`DimensionMismatch`]: FreeFormError::DimensionMismatch
/// [`SignViolation`]: FreeFormError::SignViolation
/// [`NullityMismatch`]: FreeFormError::NullityMismatch
fn validate_explicit(
    n: usize,
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    q: &[f64],
) -> Result<SpectrumClassification, FreeFormError> {
    // A valid 3-D free-standing form needs `D` rank-deficient by exactly
    // `d + 1 = 4`: three coordinate null directions plus the always-present
    // all-ones translation mode (`D · 𝟙 = 0` for any `q`, since `C · 𝟙 = 0`).
    const SPATIAL_DIM: usize = 3;
    const EXPECTED_NULLITY: usize = SPATIAL_DIM + 1;

    // 1. Length agreement across `members` / `kinds` / `q` (mirrors the anchored
    //    kernel's first guard): disagreeing lengths mean the caller mis-built the
    //    problem, so reject before indexing them together below.
    if members.len() != kinds.len() || members.len() != q.len() {
        return Err(FreeFormError::DimensionMismatch);
    }

    // 2. Per-member sign contract, reusing the shared `MemberKind` vocabulary:
    //    cables carry tension (`q > 0`), struts carry compression (`q < 0`). A
    //    violation is infeasible input — `D` would still assemble, but the
    //    self-stress it encodes would be sign-inconsistent — so surface a clean
    //    diagnostic rather than a silently-wrong form.
    for (&kind, &qi) in kinds.iter().zip(q.iter()) {
        let sign_ok = match kind {
            MemberKind::Cable => qi > 0.0,
            MemberKind::Strut => qi < 0.0,
        };
        if !sign_ok {
            return Err(FreeFormError::SignViolation);
        }
    }

    // 3. Nullity check: assemble `D` and classify its spectrum once. A nullity
    //    other than `d + 1` means this `q` does not admit a 3-D self-stressed
    //    free-standing form (too few null directions ⇒ over-constrained; too many
    //    ⇒ an under-determined / degenerate configuration). The classification is
    //    returned so coordinate recovery (step 6) reuses the single dense EVD.
    let d = assemble_force_density_matrix(n, members, q);
    let spectrum = classify_spectrum(&d, NULLITY_REL_TOL);
    if spectrum.nullity != EXPECTED_NULLITY {
        return Err(FreeFormError::NullityMismatch {
            observed: spectrum.nullity,
            expected: EXPECTED_NULLITY,
        });
    }

    Ok(spectrum)
}

/// Recover gauge-fixed free-standing node coordinates from the null space of `D`.
///
/// `null(D)` is 4-dimensional for a valid form: the three coordinate modes plus
/// the always-present all-ones translation mode. Geometrically it is the space of
/// *affine functions* of the equilibrium shape, so it pins the form only up to an
/// affine transform (a shear changes the apparent twist). We gauge-fix by
/// orthogonally projecting the caller's `nodes_guess` onto `null(D)` per axis —
/// the least-squares affine alignment to the guess, the standard form-finding
/// "refine a guess" convention.
///
/// Because the eigenvectors from [`classify_spectrum`] are orthonormal, the
/// projection is just `X = U₀ (U₀ᵀ G)`, where `U₀` is the leading-`nullity`
/// eigenvector block and `G` the guess. The result lies exactly in `null(D)`, so
/// `D · X = 0` (equilibrium), and is the closest such configuration to the guess
/// (the all-ones direction inside `U₀` absorbs translation).
///
/// Returns [`FreeFormError::SingularRecovery`] if the recovered coordinates fail
/// to span 3-D (a rank-deficient realisation, e.g. a degenerate guess).
fn recover_coordinates(
    nodes_guess: &[[f64; 3]],
    spectrum: &SpectrumClassification,
) -> Result<Vec<[f64; 3]>, FreeFormError> {
    let n = nodes_guess.len();
    let nullity = spectrum.nullity;
    // Leading `nullity` columns of the (ascending-|λ|) eigenvectors span null(D).
    let u0 = &spectrum.eigenvectors;

    // Orthogonal projection of each guess coordinate axis onto null(D):
    //   coeff[k][axis] = Σ_r U₀[r,k] · G[r,axis]   (project onto basis column k)
    //   X[r][axis]     = Σ_k U₀[r,k] · coeff[k][axis]   (reconstruct)
    // U₀'s columns are orthonormal (self-adjoint EVD), so this is the
    // least-squares affine alignment of null(D) to the guess; the all-ones
    // direction inside U₀ absorbs the translation gauge.
    let mut coeff = vec![[0.0_f64; 3]; nullity];
    for (k, ck) in coeff.iter_mut().enumerate() {
        for axis in 0..3 {
            let mut acc = 0.0;
            for (r, gr) in nodes_guess.iter().enumerate() {
                acc += u0[(r, k)] * gr[axis];
            }
            ck[axis] = acc;
        }
    }
    let mut x = vec![[0.0_f64; 3]; n];
    for (r, xr) in x.iter_mut().enumerate() {
        for axis in 0..3 {
            let mut acc = 0.0;
            for (k, ck) in coeff.iter().enumerate() {
                acc += u0[(r, k)] * ck[axis];
            }
            xr[axis] = acc;
        }
    }

    // SingularRecovery guard: a valid realisation must span 3-D. Form the
    // centred coordinate covariance `M = Σ (xᵣ − c)(xᵣ − c)ᵀ` (3×3, SPD) and
    // require it well-conditioned — `det(M)` collapses toward zero when the
    // recovered points are coplanar/collinear/coincident. The test is
    // scale-invariant (compare `det` against the isotropic `(tr/3)³`), so it
    // fires only on genuine rank deficiency, not on overall scale.
    let mut c = [0.0_f64; 3];
    for xr in &x {
        for a in 0..3 {
            c[a] += xr[a] / n as f64;
        }
    }
    let mut m = [[0.0_f64; 3]; 3];
    for xr in &x {
        let dr = [xr[0] - c[0], xr[1] - c[1], xr[2] - c[2]];
        for a in 0..3 {
            for b in 0..3 {
                m[a][b] += dr[a] * dr[b];
            }
        }
    }
    let trace = m[0][0] + m[1][1] + m[2][2];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    // `(tr/3)³` is `det(M)` for an isotropic spread; a healthy 3-D form sits
    // within a small constant of it (≈0.86 for the unit prism), so 1e-9 cleanly
    // separates full rank from a degenerate recovery without a brittle absolute
    // threshold.
    const SINGULAR_REL_TOL: f64 = 1e-9;
    let isotropic = (trace / 3.0).powi(3);
    if trace <= 0.0 || det <= SINGULAR_REL_TOL * isotropic {
        return Err(FreeFormError::SingularRecovery);
    }

    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The complete 9-cable triplex (triangular antiprism / T-prism): 6 nodes,
    /// 3 struts + 9 cables (3 top, 3 bottom, 3 vertical), in struts-then-cables
    /// member order. Struts (0,4)(1,5)(2,3); top (0,1)(1,2)(2,0); bottom
    /// (3,4)(4,5)(5,3); vertical (0,3)(1,4)(2,5).
    fn triplex_topology() -> (Vec<(usize, usize)>, Vec<MemberKind>) {
        let members = vec![
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
        ];
        let mut kinds = vec![MemberKind::Strut; 3];
        kinds.resize(members.len(), MemberKind::Cable);
        (members, kinds)
    }

    /// Closed-form force densities for the symmetric prism, struts-then-cables
    /// order: struts −√3, the six horizontals +1, verticals +√3. These make `D`
    /// rank-deficient by exactly 4 (D eigenvalues 0,0,0,0,6,6).
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
    fn closed_form_prism_q_has_nullity_four_with_spectral_gap() {
        let (members, _kinds) = triplex_topology();
        let q = closed_form_q();
        let d = assemble_force_density_matrix(6, &members, &q);
        let spec = classify_spectrum(&d, NULLITY_REL_TOL);

        assert_eq!(
            spec.nullity, 4,
            "closed-form prism q must give nullity 4 (3 coord modes + translation); eigenvalues = {:?}",
            spec.eigenvalues,
        );

        // Eigenvalues are sorted ascending by magnitude: the fourth-smallest is
        // still in the null space (~0), and there is a wide gap to the fifth,
        // which sits at ~6.
        assert!(
            spec.eigenvalues[3].abs() < 1e-9,
            "fourth-smallest |λ| must be ~0 (still null), got {}",
            spec.eigenvalues[3],
        );
        assert!(
            (spec.eigenvalues[4].abs() - 6.0).abs() < 1e-6,
            "fifth |λ| must be ~6 (spectral gap above the null space), got {}",
            spec.eigenvalues[4],
        );
    }

    /// A *generic* admissible q: distinct per-member magnitudes (struts
    /// negative, cables positive). The uniform all-magnitudes-1 assignment is
    /// **not** generic — it is highly symmetric (C₃ × top/bottom) and, at the
    /// strut/cable ratio −1, accidentally admits a second null mode (D nullity
    /// 2). Distinct magnitudes break that symmetry, leaving only the all-ones
    /// translation mode, so nullity collapses to exactly 1.
    fn generic_admissible_q(kinds: &[MemberKind]) -> Vec<f64> {
        kinds
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let mag = 1.0 + 0.37 * (i as f64);
                match k {
                    MemberKind::Cable => mag,
                    MemberKind::Strut => -mag,
                }
            })
            .collect()
    }

    #[test]
    fn generic_admissible_q_has_translation_only_nullity_one() {
        let (members, kinds) = triplex_topology();
        // A generic admissible q (distinct magnitudes, signs honouring each
        // member kind) leaves only the all-ones translation mode in null(D), so
        // nullity must be exactly 1 — distinguishing it from the special
        // closed-form prism q (nullity 4).
        let q = generic_admissible_q(&kinds);
        let d = assemble_force_density_matrix(6, &members, &q);
        let spec = classify_spectrum(&d, NULLITY_REL_TOL);

        assert_eq!(
            spec.nullity, 1,
            "generic admissible q has only the all-ones translation mode in null(D); eigenvalues = {:?}",
            spec.eigenvalues,
        );
    }

    // ---- Explicit-mode feasibility diagnostics (validate_explicit) ----

    #[test]
    fn explicit_cable_with_nonpositive_q_is_sign_violation() {
        let (members, kinds) = triplex_topology();
        let mut q = closed_form_q();
        // Force a top cable (member 3) to a non-positive density: a cable must
        // carry tension (q > 0), so this is infeasible input.
        q[3] = -1.0;
        assert_eq!(
            validate_explicit(6, &members, &kinds, &q).unwrap_err(),
            FreeFormError::SignViolation,
        );
    }

    #[test]
    fn explicit_strut_with_nonnegative_q_is_sign_violation() {
        let (members, kinds) = triplex_topology();
        let mut q = closed_form_q();
        // Force a strut (member 0) to a non-negative density: a strut must carry
        // compression (q < 0), so this is infeasible input.
        q[0] = 1.0;
        assert_eq!(
            validate_explicit(6, &members, &kinds, &q).unwrap_err(),
            FreeFormError::SignViolation,
        );
    }

    #[test]
    fn explicit_wrong_nullity_q_is_nullity_mismatch_carrying_counts() {
        let (members, kinds) = triplex_topology();
        // Admissible (signs honour kinds) but non-special: nullity 1 (only the
        // translation mode), not the d+1 = 4 a valid 3-D form requires. The error
        // carries observed (1) vs expected (4).
        let q = generic_admissible_q(&kinds);
        assert_eq!(
            validate_explicit(6, &members, &kinds, &q).unwrap_err(),
            FreeFormError::NullityMismatch {
                observed: 1,
                expected: 4,
            },
        );
    }

    #[test]
    fn explicit_length_mismatch_is_dimension_mismatch() {
        let (members, kinds) = triplex_topology();
        // One density short of the 12 members → dimension mismatch, caught up
        // front before any sign or nullity work.
        let q = vec![1.0; members.len() - 1];
        assert_eq!(
            validate_explicit(6, &members, &kinds, &q).unwrap_err(),
            FreeFormError::DimensionMismatch,
        );
    }

    // ---- Null-space coordinate recovery (recover_coordinates) ----

    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn norm(a: [f64; 3]) -> f64 {
        dot(a, a).sqrt()
    }
    fn member_len(nodes: &[[f64; 3]], m: (usize, usize)) -> f64 {
        norm(sub(nodes[m.0], nodes[m.1]))
    }

    /// Assert the members in `group` all have equal length within a relative
    /// `tol` (max−min ≤ tol·mean).
    fn assert_equal_lengths(nodes: &[[f64; 3]], group: &[(usize, usize)], tol: f64, what: &str) {
        let lens: Vec<f64> = group.iter().map(|&m| member_len(nodes, m)).collect();
        let mean = lens.iter().sum::<f64>() / lens.len() as f64;
        let max = lens.iter().copied().fold(f64::MIN, f64::max);
        let min = lens.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            (max - min) <= tol * mean,
            "{what} lengths must be equal within {tol} relative; got {lens:?} (mean {mean:.4})",
        );
    }

    /// The canonical symmetric triplex prism consistent with the closed-form q
    /// (derived from the per-node FD equilibrium): equal top/bottom circumradius
    /// R = 1, unit height, twist α = 30°. Node order matches `triplex_topology`:
    /// 0,1,2 top (z = 1) at azimuth 120°·i; 3,4,5 bottom (z = 0) at 120°·i + 30°.
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

    /// A mildly perturbed (≈1e-3 per coordinate, deterministic) symmetric-prism
    /// guess: the canonical prism plus fixed small offsets. The form-finding
    /// convention refines a guess, so recovery should gauge-fix to it; fixed
    /// offsets (no RNG) keep the dependent tests bit-stable. Shared by the
    /// recovery and explicit-entry-point tests.
    fn perturbed_prism_guess() -> Vec<[f64; 3]> {
        const PERTURB: [[f64; 3]; 6] = [
            [0.0009, -0.0011, 0.0007],
            [-0.0013, 0.0006, 0.0010],
            [0.0012, 0.0008, -0.0009],
            [-0.0007, -0.0012, 0.0011],
            [0.0010, -0.0008, -0.0013],
            [-0.0011, 0.0013, 0.0006],
        ];
        canonical_prism()
            .iter()
            .zip(PERTURB.iter())
            .map(|(p, d)| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
            .collect()
    }

    #[test]
    fn recovers_metric_prism_from_perturbed_guess() {
        let (members, kinds) = triplex_topology();
        let q = closed_form_q();
        // One dense EVD, shared between the nullity check and recovery.
        let spectrum = validate_explicit(6, &members, &kinds, &q)
            .expect("closed-form prism q is feasible (nullity 4)");

        // A mildly perturbed symmetric-prism guess (recovery should gauge-fix
        // to it).
        let guess = perturbed_prism_guess();

        let x = recover_coordinates(&guess, &spectrum)
            .expect("perturbed symmetric-prism guess must recover a 3-D realisation");

        // (1) Equilibrium: X lies in null(D), so the per-axis residual ‖D·X‖∞
        // must vanish to machine precision — the rock-solid correctness signal.
        let d = assemble_force_density_matrix(6, &members, &q);
        let mut resid = 0.0_f64;
        for i in 0..6 {
            // (D·X) row i, a 3-vector across the coordinate axes.
            let mut row = [0.0_f64; 3];
            for (j, xj) in x.iter().enumerate() {
                let dij = d[(i, j)];
                for (row_a, &xja) in row.iter_mut().zip(xj.iter()) {
                    *row_a += dij * xja;
                }
            }
            resid = row.iter().fold(resid, |m, &v| m.max(v.abs()));
        }
        assert!(
            resid < 1e-9,
            "equilibrium residual ‖D·X‖∞ must be ~0, got {resid:.3e}",
        );

        // (2) Metric prism: each member group is equal-length. 5% relative tol —
        // a correct recovery drifts only ~1e-3 from the symmetric prism (the
        // in-null-space part of the ~1e-3 guess perturbation), whereas a broken
        // recovery (raw guess / sheared affine image) spreads O(1).
        const MTOL: f64 = 5e-2;
        assert_equal_lengths(&x, &members[0..3], MTOL, "strut");
        assert_equal_lengths(&x, &members[3..9], MTOL, "horizontal cable");
        assert_equal_lengths(&x, &members[9..12], MTOL, "vertical cable");

        // (3) Top {0,1,2} and bottom {3,4,5} are each equilateral triangles...
        assert_equal_lengths(&x, &[(0, 1), (1, 2), (2, 0)], MTOL, "top triangle edge");
        assert_equal_lengths(&x, &[(3, 4), (4, 5), (5, 3)], MTOL, "bottom triangle edge");

        // (3b) ...lying in parallel planes (the two triangle normals are parallel).
        let n_top = cross(sub(x[1], x[0]), sub(x[2], x[0]));
        let n_bot = cross(sub(x[4], x[3]), sub(x[5], x[3]));
        let cos_planes = dot(n_top, n_bot).abs() / (norm(n_top) * norm(n_bot));
        assert!(
            cos_planes > 1.0 - 1e-3,
            "top/bottom triangle planes must be parallel; |cos| = {cos_planes:.6}",
        );

        // (4) Vertical-pair angular offset ≈ 30°: the twist between the triangles
        // about the prism axis (centroid-to-centroid). Project a top node and its
        // paired bottom node onto the plane ⊥ axis and take their angle.
        let centroid = |g: &[usize]| {
            let mut c = [0.0; 3];
            for &i in g {
                for a in 0..3 {
                    c[a] += x[i][a] / g.len() as f64;
                }
            }
            c
        };
        let c_top = centroid(&[0, 1, 2]);
        let c_bot = centroid(&[3, 4, 5]);
        let axis = {
            let a = sub(c_top, c_bot);
            let n = norm(a);
            [a[0] / n, a[1] / n, a[2] / n]
        };
        let proj = |p: [f64; 3], c: [f64; 3]| {
            let r = sub(p, c);
            let along = dot(r, axis);
            [
                r[0] - along * axis[0],
                r[1] - along * axis[1],
                r[2] - along * axis[2],
            ]
        };
        // Vertical pair (0,3): top node 0 and its bottom partner node 3.
        let u = proj(x[0], c_top);
        let w = proj(x[3], c_bot);
        let twist_deg =
            (dot(u, w) / (norm(u) * norm(w))).acos() * 180.0 / std::f64::consts::PI;
        assert!(
            (twist_deg - 30.0).abs() < 2.0,
            "vertical-pair twist must be ≈30°, got {twist_deg:.3}°",
        );
    }

    // ---- Explicit-mode member forces + form_find_free entry point ----

    #[test]
    fn form_find_free_explicit_populates_result_with_correct_force_signs() {
        let (members, kinds) = triplex_topology();
        let q = closed_form_q();
        let guess = perturbed_prism_guess();

        let result = form_find_free(
            &guess,
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q.clone()),
        )
        .expect("explicit closed-form prism q must form-find");

        // Spectrum / convergence metadata.
        assert_eq!(result.nullity, 4, "a valid 3-D form has nullity d+1 = 4");
        assert!(result.converged, "explicit closed-form solve must converge");
        assert_eq!(result.nodes.len(), 6, "one recovered coordinate per node");

        // force_densities echo the input q exactly.
        assert_eq!(result.force_densities, q, "force_densities must echo input q");

        // member_forces: N_i = q_i · L_i in struts-then-cables (input) order.
        assert_eq!(result.member_forces.len(), members.len());
        // Struts carry compression (N < 0), cables tension (N > 0) — the sign of
        // N_i follows q_i since every recovered length L_i > 0.
        for (idx, (&kind, &n_i)) in kinds.iter().zip(result.member_forces.iter()).enumerate() {
            match kind {
                MemberKind::Strut => assert!(
                    n_i < 0.0,
                    "strut {idx} must be compressive (N < 0), got {n_i}",
                ),
                MemberKind::Cable => assert!(
                    n_i > 0.0,
                    "cable {idx} must be tensile (N > 0), got {n_i}",
                ),
            }
        }

        // Every recovered member length is positive and finite: L_i = N_i / q_i
        // (q_i is non-zero for every member).
        for (&n_i, &qi) in result.member_forces.iter().zip(q.iter()) {
            let len = n_i / qi;
            assert!(
                len > 0.0 && len.is_finite(),
                "member length L = N/q must be positive & finite, got {len}",
            );
        }
    }

    // ---- Adaptive GroupRatios force-density search (GroupRatios mode) ----

    /// Per-member group ids for the triplex, parallel to `triplex_topology`'s
    /// member order: struts → group 0, the six horizontals (top + bottom) →
    /// group 1, verticals → group 2.
    fn triplex_group_ids() -> Vec<usize> {
        vec![
            0, 0, 0, // struts
            1, 1, 1, // top horizontals
            1, 1, 1, // bottom horizontals
            2, 2, 2, // verticals
        ]
    }

    #[test]
    fn group_ratios_search_recovers_closed_form_prism_relative_q() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();

        // Seed near the *signs* of the closed form — struts compressive (−1),
        // horizontals (the reference) and verticals tensile (+1) — but with
        // magnitudes all 1, not the √3 closed form. The adaptive search must
        // discover the relative magnitudes on its own.
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1, // horizontals held fixed as the scale gauge
        };

        let result = form_find_free(&guess, &members, &kinds, &spec)
            .expect("group-ratio search must form-find the prism");

        // Spectrum / convergence metadata: a valid 3-D form has nullity d+1 = 4.
        assert_eq!(result.nullity, 4, "form-found prism must have nullity 4");
        assert!(result.converged, "group-ratio solve must converge");

        // Recovered *relative* densities: struts −√3, verticals +√3, horizontals
        // pinned at the +1 reference. Every member in a group shares its ratio.
        let s = 3.0_f64.sqrt();
        for i in 0..3 {
            assert!(
                (result.force_densities[i] - (-s)).abs() < 1e-6,
                "strut {i} relative q must be ≈ −√3, got {}",
                result.force_densities[i],
            );
        }
        for i in 3..9 {
            assert!(
                (result.force_densities[i] - 1.0).abs() < 1e-12,
                "horizontal {i} is the reference group, must stay = 1, got {}",
                result.force_densities[i],
            );
        }
        for i in 9..12 {
            assert!(
                (result.force_densities[i] - s).abs() < 1e-6,
                "vertical {i} relative q must be ≈ +√3, got {}",
                result.force_densities[i],
            );
        }

        // Force signs follow the recovered q: struts compressive, cables tensile.
        for (idx, (&kind, &n_i)) in kinds.iter().zip(result.member_forces.iter()).enumerate() {
            match kind {
                MemberKind::Strut => assert!(
                    n_i < 0.0,
                    "strut {idx} must be compressive (N < 0), got {n_i}",
                ),
                MemberKind::Cable => assert!(
                    n_i > 0.0,
                    "cable {idx} must be tensile (N > 0), got {n_i}",
                ),
            }
        }
    }

    #[test]
    fn group_ratios_search_all_positive_does_not_converge() {
        let (members, _kinds) = triplex_topology();
        // Treat every member as a cable and seed every group positive. A
        // positive-only force-density assignment keeps D a connected-graph
        // weighted Laplacian, whose nullity is exactly 1 (the all-ones
        // translation mode) for *any* positive ratios — so no nullity-4 q exists
        // in the search space. The search must exhaust its budget and report
        // SearchDidNotConverge, never panic.
        let kinds = vec![MemberKind::Cable; members.len()];
        let guess = perturbed_prism_guess();

        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![1.0, 1.0, 1.0], // all tension → nullity stuck at 1
            reference_group: 1,
        };

        assert_eq!(
            form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
            FreeFormError::SearchDidNotConverge,
        );
    }
}
