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
    // Scaffold only (prerequisite pre-1): the explicit-mode and group-ratio
    // pipelines are filled in incrementally across the T1b steps. Binding the
    // parameters to `_` keeps the real names in place for those steps while
    // avoiding unused-variable warnings in the meantime.
    let _ = (nodes_guess, members, kinds, spec);
    unimplemented!("form_find_free: implemented incrementally across Tensegrity T1b steps")
}

// ---------------------------------------------------------------------------
// Crate-internal numeric helpers (D assembly + spectral nullity classification)
// ---------------------------------------------------------------------------

/// Relative tolerance for nullity classification: an eigenvalue counts as a null
/// direction when its magnitude is below this fraction of the largest-magnitude
/// eigenvalue. The prism golden has a wide spectral gap (fifth eigenvalue 6 vs
/// fourth ≈ 2.5e-15), so 1e-8 separates the null space from the rest of the
/// spectrum without a brittle absolute threshold.
#[allow(dead_code)] // wired into form_find_free's validation/recovery at steps 4/6/8
const NULLITY_REL_TOL: f64 = 1e-8;

/// Spectral classification of the force-density matrix `D`.
#[allow(dead_code)] // fields consumed by validation (step 4) / recovery (step 6)
struct SpectrumClassification {
    /// Eigenvalues of `D`, sorted ascending by magnitude.
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
#[allow(dead_code)] // wired into form_find_free's pipelines at steps 4/8/10
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
#[allow(dead_code)] // wired into form_find_free's pipelines at steps 4/8/10
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
        kinds.extend(std::iter::repeat(MemberKind::Cable).take(9));
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
}
