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

use crate::fd_matrix::{NULLITY_REL_TOL, SpectrumError, SymmetricSpectrum, assemble_d};
use crate::form_find::{
    ForceDensityInputError, ForceDensityProblem, IsotropicStencil, MemberKind, ZeroAreaTriangle,
    free_equilibrium_residual_relative, member_axial_forces, scatter_surface_stencils,
};
use crate::group_spec::{GroupSpec, GroupSpecDefect};
use faer::Mat;
use faer::mat::MatRef;
use std::borrow::Cow;

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
    /// Per-triangle echo of the prescribed isotropic surface stress σ (in
    /// `surfaces` declaration order); empty on the line-only path. The
    /// equilibrium form was solved holding these σ fixed, so the echo is the
    /// physically-carried per-triangle stress. (δ / combined form-find.)
    pub surface_stresses: Vec<f64>,
}

/// Reason a free-standing form-find is infeasible. Mirrors the
/// [`crate::form_find::FormFindError`] diagnostic-enum precedent: infeasible
/// input becomes a clean typed error, never a panic or a silently-wrong result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeFormError {
    /// The input violates the force-density input contract. A
    /// [`ForceDensitySpec::GroupRatios`] spec that cannot seed the search is
    /// [`ForceDensityInputError::MalformedGroupSpec`]; a well-formed one is
    /// checked at its seed densities.
    Input(ForceDensityInputError),
    /// The `D` the solve settled on is not a free-standing form: the explicit
    /// `q`'s, or the combined fixed point's at the equilibrium it reached.
    NotAForm(FormDefect),
    /// The adaptive [`ForceDensitySpec::GroupRatios`] search, or the combined
    /// fixed point, stopped short of a free-standing form. `defect` is `Some`
    /// when it stopped on a `D` that is not a form, and `None` when the
    /// combined fixed point's budget or stall guard ended a valid-form iterate
    /// short of equilibrium.
    SearchDidNotConverge {
        /// What the final iterate's `D` was observed to be, if not a form.
        defect: Option<FormDefect>,
    },
    /// Null-space coordinate recovery was rank-deficient (the recovered basis did
    /// not span a 3-D realisation).
    SingularRecovery,
    /// A surface triangle valid at input degenerated (collinear / zero-area
    /// corners) at a fixed-point iterate, where its cotangent weights
    /// `cot(θ) = (e_a·e_b)/(2·Area)` diverge. (δ / combined struts+cables+membrane.)
    DegenerateTriangle,
    /// A force-density matrix `D` the solve classifies has no dense spectrum.
    Spectrum(SpectrumError),
}

/// What a force-density matrix `D` that is not a free-standing form was
/// observed to be. A 3-D form is rank-deficient by exactly `d + 1 = 4` on one
/// connected force graph, and each connected piece (an isolated node is one)
/// carries its own translation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormDefect {
    /// Nullity observed in `D`'s spectrum.
    pub nullity: usize,
    /// Connected pieces of `D`'s force graph.
    pub components: usize,
}

impl From<ForceDensityInputError> for FreeFormError {
    fn from(input: ForceDensityInputError) -> Self {
        FreeFormError::Input(input)
    }
}

impl From<GroupSpecDefect> for FreeFormError {
    fn from(defect: GroupSpecDefect) -> Self {
        FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(defect))
    }
}

impl From<SpectrumError> for FreeFormError {
    fn from(e: SpectrumError) -> Self {
        FreeFormError::Spectrum(e)
    }
}

impl From<ZeroAreaTriangle> for FreeFormError {
    fn from(_: ZeroAreaTriangle) -> Self {
        FreeFormError::DegenerateTriangle
    }
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
    let q0 = seed_densities(spec, members.len())?;
    let problem = ForceDensityProblem::new(nodes_guess, members, kinds, &q0, &[], &[], &[])?;
    match spec {
        // Deterministic foundation: assemble D, classify nullity, recover
        // coordinates, compute forces.
        ForceDensitySpec::Explicit(_) => form_find_explicit(&problem),
        // Adaptive eigenvalue-minimisation search over relative group densities;
        // on success it produces an admissible q and delegates to the explicit
        // path.
        ForceDensitySpec::GroupRatios {
            group_ids,
            seed_ratios,
            reference_group,
        } => form_find_group_ratios(&problem, group_ids, seed_ratios, *reference_group),
    }
}

/// The densities `spec` fixes before any search, for the input contract: the
/// explicit `q`, or the [`ForceDensitySpec::GroupRatios`] seed densities, whose
/// signs the search keeps.
fn seed_densities(
    spec: &ForceDensitySpec,
    n_members: usize,
) -> Result<Cow<'_, [f64]>, GroupSpecDefect> {
    match spec {
        ForceDensitySpec::Explicit(q) => Ok(Cow::Borrowed(q)),
        ForceDensitySpec::GroupRatios {
            group_ids,
            seed_ratios,
            reference_group,
        } => {
            let groups = GroupSpec::new(group_ids, seed_ratios, *reference_group, n_members)?;
            Ok(Cow::Owned(groups.seed_densities().to_vec()))
        }
    }
}

/// Solve the free-standing Force-Density form-finding problem WITH isotropic
/// surface (membrane) contributions (PRD §4 M1b / D3 — δ / task 4415).
///
/// This is a sibling to [`form_find_free`]: the landed line-only entry and all
/// its callers (trampoline, tests, `prestress_stability`) are byte-identical.
/// Empty `surfaces` / `surface_stresses` delegate to the line-only path with an
/// empty `surface_stresses` echo — the additive-extension invariant.
///
/// The combined force-density matrix is `D = CᵀQC + Σ_T σ_T·L_T`, where the
/// cotangent-Laplacian `L_T` at each triangle depends on geometry. The
/// [`ForceDensitySpec::GroupRatios`] search drives the COMBINED `D`'s nullity
/// to 4 over the line groups (σ is a FIXED additive term during the search).
/// The search+recovery is wrapped in an outer cotangent fixed point (assemble
/// combined `D` at the current geometry → search/recover → repeat until the
/// combined free-node equilibrium residual `‖D(x)·x‖` settles to machine
/// precision), mirroring γ's `form_find_anchored_surfaces`. The residual is the
/// stop test; the final iterate's combined `D` must then also certify as one
/// connected nullity-4 form.
///
/// [`ForceDensitySpec::Explicit`] admissibility: with `σ > 0` present, the
/// membrane's cotangent weights are part of `D`, so `q` must be a COMBINED
/// self-stress — a line-only self-stress (one satisfying the surface-free `D`)
/// is generally NOT admissible once surfaces contribute. For the triplex +
/// two equilateral membrane triangles worked example, every cotangent in the
/// surface stencil is `cot(60°) = 1/√3`, so the surface term collapses to a
/// uniform extra weight `w = σ·cot(60°)/2 = σ/(2√3)` on the six horizontal
/// cables; the admissible combined closed form is
/// `q_strut = -(√3 + σ/2)`, `q_horiz = 1`, `q_vert = +(√3 + σ/2)`. A `q` with
/// no combined equilibrium at any geometry still returns
/// [`FreeFormError::SearchDidNotConverge`] rather than a wrong answer.
///
/// # Errors
/// - [`FreeFormError::Input`] — the input violates the [`ForceDensityInputError`]
///   contract (lengths, a `GroupRatios` spec that cannot seed the search, node
///   indices, q or seed signs, σ tension, node coordinates, input triangles).
/// - [`FreeFormError::DegenerateTriangle`] — a triangle degenerated at an iterate.
/// - [`FreeFormError::NotAForm`] — the fixed point reached equilibrium, but the
///   combined `D` there is not a form ([`FormDefect`]): a disconnected piece,
///   or a nullity other than 4.
/// - [`FreeFormError::SearchDidNotConverge`] — the GroupRatios search or the
///   combined fixed point stopped short of equilibrium. `defect` names the
///   final iterate's [`FormDefect`] when its combined `D` is not a form — an
///   isolated node (whose zero row never settles), or a wrong-nullity
///   `Explicit` spec (with `σ > 0`), see the negative guard
///   `combined_explicit_line_only_q_is_not_a_combined_self_stress` in
///   `tests/tensegrity_delta_combined_form_find.rs`. `defect` is `None` when
///   the budget or stall guard ended a valid form short of equilibrium.
/// - [`FreeFormError::SingularRecovery`] — null-space basis is not 3-D.
/// - [`FreeFormError::Spectrum`] — a `D` the solve classifies has a non-finite
///   entry (e.g. densities large enough to overflow it), or its dense
///   eigendecomposition did not converge ([`SpectrumError`]).
pub fn form_find_free_surfaces(
    nodes_guess: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    surfaces: &[(usize, usize, usize)],
    surface_stresses: &[f64],
    spec: &ForceDensitySpec,
) -> Result<FreeFormResult, FreeFormError> {
    // The input contract, a degenerate INPUT triangle included, is checked
    // against the caller's own geometry — before the bootstrap below rewrites
    // `current` into a (non-degenerate) line-only equilibrium.
    let q0 = seed_densities(spec, members.len())?;
    let problem = ForceDensityProblem::new(
        nodes_guess,
        members,
        kinds,
        &q0,
        &[],
        surfaces,
        surface_stresses,
    )?;

    // Empty surfaces delegate to the line-only path with an empty echo.
    if surfaces.is_empty() {
        let mut result = form_find_free(nodes_guess, members, kinds, spec)?;
        result.surface_stresses = Vec::new();
        return Ok(result);
    }

    // Non-empty: combined cotangent fixed-point loop.
    // Mirrors γ's form_find_anchored_surfaces loop shape (PRD §4 D3).
    let n = nodes_guess.len();

    // Bootstrap: start the fixed-point loop from the line-only equilibrium rather
    // than the raw perturbed guess.  Starting from the perturbed guess breaks the
    // 3-fold symmetry of the cotangent surface matrix, causing the combined D to
    // have nullity 2 instead of 4 — an unrecoverable state for the outer loop.
    // The line-only form-find converges to the canonical (symmetric, equilateral)
    // prism geometry, where the surface matrix has 3-fold symmetry and the combined
    // D can reach nullity 4.  Falls back to nodes_guess if the line-only solve
    // fails (e.g. infeasible Explicit q that has no line-only equilibrium).
    let boot = form_find_free(nodes_guess, members, kinds, spec);
    let mut current = boot
        .as_ref()
        .map(|r| r.nodes.clone())
        .unwrap_or_else(|_| nodes_guess.to_vec());

    // Warm-start group magnitudes for the combined inner search.  Starting the
    // coordinate-descent from the original spec seeds can stall: at seeds
    // (-1, 1, 1) the Z-equilibrium constraint (q_strut + q_vert = 0) makes the
    // 1-D minimum over each free group land back at the seed, so zero improvement
    // is made and the stall guard fires before the combined objective ever drives
    // the nullity to 4.  Using the line-only bootstrap q* ≈ (-√3, 1, +√3) as the
    // starting magnitudes puts the search within the convergence basin of the
    // correct combined target (-√3·(1+w), 1, +√3·(1+w)), where coordinate
    // descent finds it in O(1) rounds.
    let mut warm_group_mag: Vec<f64> = match spec {
        ForceDensitySpec::GroupRatios { group_ids, seed_ratios, .. } => {
            let n_groups = seed_ratios.len();
            let mut mag: Vec<f64> = seed_ratios.iter().map(|r| r.abs()).collect();
            if let Ok(ref b) = boot {
                for (i, &g) in group_ids.iter().enumerate() {
                    if g < n_groups {
                        mag[g] = b.force_densities[i].abs().max(1e-10);
                    }
                }
            }
            mag
        }
        _ => vec![],
    };

    let mut converged = false;
    let mut last_result: Option<FreeFormResult> = None;
    // Adaptive step size for the geometry-descent relaxation, carried across outer
    // iterations (grows on accepted steps, shrinks on backtracks).
    let mut geo_step = 1e-2_f64;
    // Stall detection: break early when geometry is stuck AND residual is not
    // improving, rather than spinning the full MAX_FREE_SURFACE_ITERS budget
    // doing expensive eigendecompositions on an already-stalled search.
    let mut prev_resid = f64::INFINITY;
    let mut stuck_iters = 0_usize;

    for _iter in 0..MAX_FREE_SURFACE_ITERS {
        // Assemble Σ_T σ_T·L_T at the current geometry.
        let surface_mat = assemble_surface_matrix(n, surfaces, surface_stresses, &current)?;

        // (1) Force densities at the current geometry.  For Explicit, q is fixed
        // (validated up front).  For GroupRatios, re-search q on the combined D at
        // this iteration's cotangent weights so the densities co-adapt with the
        // geometry; warm-start the next search from the magnitudes just found.
        let q: Vec<f64> = match spec {
            ForceDensitySpec::Explicit(_) => problem.q().to_vec(),
            ForceDensitySpec::GroupRatios {
                group_ids,
                seed_ratios,
                reference_group,
            } => {
                let searched_q = form_find_group_ratios_combined(
                    &problem,
                    &current,
                    group_ids,
                    seed_ratios,
                    &warm_group_mag,
                    *reference_group,
                    &surface_mat,
                )?;
                for (i, &g) in group_ids.iter().enumerate() {
                    warm_group_mag[g] = searched_q[i].abs().max(1e-10);
                }
                searched_q
            }
        };

        // (2) Combined equilibrium residual at the current geometry — the
        // free-standing fixed point's stop test. `q` is fixed for the rest of this
        // iteration (only the surface term varies with geometry), so the line-only
        // CᵀQC matrix is hoisted once and each surface term is added to it — the
        // (2b) trial-residual check further down reuses this same `d_line`.
        let d_line = assemble_d(n, members, &q);
        let d_at_current = &d_line + &surface_mat;
        let resid = all_node_equilibrium_residual_relative(&d_at_current, &current);

        if resid <= FREE_SURFACE_EQUILIBRIUM_REL_TOL {
            converged = true;
            last_result = Some(combined_result_at(members, &q, &current));
            break;
        }

        // Snapshot the geometry entering this iteration.  Both the (2b) trial-move
        // guard below and the stall guard at the end of this loop body need the
        // PRE-(2b) geometry as their baseline — distinct from `current`, which (2b)
        // may go on to overwrite.
        let before_relax = current.clone();

        // (2b) Null-space geometry recovery — the Explicit branch's missing half.
        // `combined_geometry_descent_step` below minimises the eigen-gap objective, which
        // drives D_combined toward NULLITY 4; it does NOT drive the coordinate vector x INTO
        // null(D_combined). Those are different conditions, and the residual we test above is
        // the latter. The GroupRatios branch has a second lever Explicit lacks — it re-searches
        // q on the combined D at this geometry each iteration; its own projected geometry is
        // discarded at (1), only the densities are kept. With q FIXED here the only remaining
        // lever is x, so project it onto the 4 smallest-|λ| eigenvectors of D_combined at the
        // current geometry — the free-standing analogue of the anchored kernel's per-iteration
        // `solve_reduced` move.
        //
        // Applied as a TRIAL move, not unconditionally: an intermediate projection iterate is
        // not guaranteed to land on a well-posed geometry (its null-space basis can fail to be
        // 3-D, or it can push a membrane triangle toward zero area) and is not guaranteed to
        // improve the residual. Accept the projected geometry only if it (a) recovers
        // coordinates AND re-assembles a surface matrix at the result without error, and (b)
        // does not increase the combined equilibrium residual; otherwise keep `before_relax` and
        // fall through to the descent step in (3).
        //
        // (a) is load-bearing by construction: the `if let ... && let ...` chain below means a
        // `SingularRecovery` / `DegenerateTriangle` raised by this internal relaxation step is
        // absorbed as a no-op rather than propagated to the caller via `?` — swapping the chain
        // for `?` would let an intermediate-iterate error surface in place of the contractual
        // `SearchDidNotConverge`.
        // (b) (the `trial_resid <= resid` check just below) is defensive rather than measured
        // load-bearing on this fixture: forcing it to accept unconditionally (`if true`) and
        // probing 10 infeasible σ/q/guess combinations plus the full crate suite (759 lib + 5
        // integration tests) left every result unchanged — every rejected trial in those cases
        // still resolved to `SearchDidNotConverge` once the rejection was skipped, so no test
        // today would catch this check being deleted. Kept because accepting a
        // residual-regressing trial changes the iterate (3)'s descent runs from, which could
        // destabilise a case not covered by this fixture even though none tried here do.
        if matches!(spec, ForceDensitySpec::Explicit(_))
            && let Ok(trial_nodes) =
                form_find_explicit_combined_relaxed(&problem, &current, &surface_mat)
            && let Ok(trial_surface_mat) =
                assemble_surface_matrix(n, surfaces, surface_stresses, &trial_nodes)
        {
            let d_trial = &d_line + &trial_surface_mat;
            let trial_resid = all_node_equilibrium_residual_relative(&d_trial, &trial_nodes);
            if trial_resid <= resid {
                current = trial_nodes;
            }
        }

        // (3) Relax the geometry one descent step on the eigenvalue-gap objective at
        // fixed q. When (2b) accepted its trial move above, this is the SECOND half of
        // a two-part relaxation (project into null₄(D_combined), then one eigen-gap
        // descent step); when (2b) was rejected or a no-op, this is the only move made
        // this iteration. Both halves are required and neither suffices alone:
        // MEASURED on σ=0.05/perturbed×1 that projection alone plateaus at a
        // non-equilibrium fixed point of the projection map (residual settles at
        // ~2.66e-5), while descent alone ((2b) absent) never leaves the starting
        // residual (~2.84) because it drives D toward nullity 4 without ever moving x
        // into null(D). Projection-then-descent drops the residual 5 orders of
        // magnitude in one combined iteration and then grinds the rest.  The line-only
        // D is rank-deficient by 4 for a whole affine family of geometries (the
        // bootstrap is a slightly-non-symmetric member); only at the symmetric
        // realisation does the geometry-dependent membrane term let the combined D
        // reach nullity 4.  The force-density search alone cannot get there (a force
        // group shares one magnitude across its members, so it cannot cancel the
        // per-edge cotangent asymmetry) — the geometry must move.  This is the
        // free-standing analogue of γ's anchored `solve_reduced` relaxation, with the
        // rigid/scale gauge left free.
        let (next, next_step) = combined_geometry_descent_step(
            n,
            members,
            &q,
            surfaces,
            surface_stresses,
            &current,
            geo_step,
        );
        geo_step = next_step;

        // Stall guard: if NEITHER half of this iteration's relaxation moved the
        // geometry — the (2b) trial move was rejected or a no-op, AND the (3) descent
        // step found no downhill move within its backtracking budget, so `next` equals
        // the geometry this iteration STARTED with (`before_relax`) — AND the residual
        // is not shrinking appreciably, increment a stuck counter and break early to
        // avoid spending O(MAX_ITERS · n · EVD(n)) doing expensive but fruitless work.
        // Comparing against `before_relax` rather than the post-(2b) `current` matters
        // logically: an ACCEPTED (2b) move followed by a stalled (3) must NOT count as
        // stuck (the geometry did move this iteration, via (2b)) — only a rejected/no-op
        // (2b) followed by a stalled (3) may. This is defensive rather than
        // measured-load-bearing on this fixture: reverting this comparison to the
        // pre-amendment `next == current` and re-running the full crate suite (759 lib +
        // 5 integration tests) left every result unchanged, so no test here currently
        // depends on the distinction. Kept because an under-counted stall guard would risk
        // exiting a genuinely-progressing search early on some case not covered by this
        // fixture, trading a real (if untested) correctness risk for one line of clarity.
        if next == before_relax && resid >= prev_resid * (1.0 - GEO_STALL_RESID_THRESHOLD) {
            stuck_iters += 1;
            if stuck_iters >= GEO_STALL_ITERS {
                break;
            }
        } else {
            stuck_iters = 0;
        }
        prev_resid = resid;

        current = next;
        last_result = Some(combined_result_at(members, &q, &current));
    }

    // Certify the final iterate whether or not the loop converged, so a model
    // that is not one connected nullity-4 form names its defect even when its
    // residual never settled (an isolated node's zero row keeps it unsettled).
    let mut result = last_result.expect("every loop exit follows a stored iterate");
    let surface_mat_final = assemble_surface_matrix(n, surfaces, surface_stresses, &result.nodes)?;
    let certified = FreeStandingForm::certify(&combined_force_density_matrix(
        members,
        &result.force_densities,
        &surface_mat_final,
    ));

    // Non-convergence: match the line-only `form_find_free` contract — return
    // `Err(SearchDidNotConverge)` so `run_free` emits `E_FormFindInfeasible`.
    // Returning `Ok` with `converged=false` would create an asymmetry where
    // downstream consumers that only inspect the result value (not `.converged`)
    // silently treat a failed combined solve as a valid equilibrium.
    if !converged {
        return Err(match certified {
            Ok(_) => FreeFormError::SearchDidNotConverge { defect: None },
            Err(FreeFormError::NotAForm(defect)) => FreeFormError::SearchDidNotConverge {
                defect: Some(defect),
            },
            Err(other) => other,
        });
    }
    // Converged: the equilibrium was reached, so a `D` that fails certification
    // is not a form, whatever spec produced it.
    let form = certified?;

    result.converged = true;
    result.nullity = form.nullity();
    result.surface_stresses = surface_stresses.to_vec();
    Ok(result)
}

/// Equilibrium-residual convergence tolerance for the free-standing cotangent
/// fixed point, applied to [`all_node_equilibrium_residual_relative`]: each
/// row's net force RELATIVE to that row of `D`, so convergence is gauge-free as
/// well as coordinate-scale-free. Row norms are uniform at the δ `GroupRatios`
/// golden's converged state (`tensegrity_delta_combined_form_find.rs`), so the
/// value stays ~100× below that golden's independently-checked
/// `EQUIL_TOL = 1e-9`.
const FREE_SURFACE_EQUILIBRIUM_REL_TOL: f64 = 1e-11;

/// Iteration cap for the free-standing cotangent fixed point. Mirrors γ's
/// MAX_SURFACE_ITERS — a generous backstop; well-posed inputs break out early.
const MAX_FREE_SURFACE_ITERS: usize = 200;

/// Minimum relative residual improvement per outer iteration to consider
/// progress is being made (0.01%). Used by the stall guard to detect when
/// the geometry descent is stuck and the residual is no longer shrinking.
const GEO_STALL_RESID_THRESHOLD: f64 = 1e-4;

/// Consecutive iterations with unchanged geometry AND non-improving residual
/// before the outer fixed-point exits early (avoids burning the full
/// `MAX_FREE_SURFACE_ITERS` budget on an intractable configuration).
const GEO_STALL_ITERS: usize = 5;

/// Assemble the surface cotangent-Laplacian contribution `Σ_T σ_T·L_T` at the
/// given node geometry, producing an `n×n` additive matrix via the anchored
/// kernel's [`scatter_surface_stencils`]; a zero-area triangle is
/// [`FreeFormError::DegenerateTriangle`].
fn assemble_surface_matrix(
    n: usize,
    surfaces: &[(usize, usize, usize)],
    surface_stresses: &[f64],
    nodes: &[[f64; 3]],
) -> Result<Mat<f64>, FreeFormError> {
    let mut s = Mat::<f64>::zeros(n, n);
    scatter_surface_stencils::<IsotropicStencil>(&mut s, surfaces, surface_stresses, nodes)?;
    Ok(s)
}

/// The combined force-density matrix `D = CᵀQC + Σ_T σ_T·L_T`: the line `D` at
/// `q` plus an already-assembled surface term ([`assemble_surface_matrix`]).
fn combined_force_density_matrix(
    members: &[(usize, usize)],
    q: &[f64],
    surface_mat: &Mat<f64>,
) -> Mat<f64> {
    assemble_d(surface_mat.nrows(), members, q) + surface_mat
}

/// ALL-node equilibrium residual — the free-standing fixed point's stop test:
/// the anchored kernel's per-row criterion with every row free, since every
/// node is free here. One definition carries the per-row normalisation (exactly
/// gauge-invariant) and the zero/NaN-row rejection.
fn all_node_equilibrium_residual_relative(d: &Mat<f64>, nodes: &[[f64; 3]]) -> f64 {
    let every_row: Vec<usize> = (0..nodes.len()).collect();
    free_equilibrium_residual_relative(d, nodes, &every_row)
}

/// Combined-D geometry recovery, returning the recovered nodes: it **forces
/// nullity = 4** — the 4 smallest-|λ| eigenvectors are taken as the approximate
/// null space regardless of whether their eigenvalues are truly near zero. At
/// intermediate geometry iterations the combined D may not have exact nullity 4
/// (the cotangent weights depend on geometry and the current iterate is not yet
/// the fixed point); forcing nullity 4 lets the outer fixed-point loop proceed
/// toward `x*∈null(D_combined(x*))`. The outer loop's equilibrium residual
/// (`‖D_combined(x)·x‖`) is the stop test, and certifying its final iterate is
/// the validity gate.
///
/// `nodes_guess` is the current iterate, which gauge-fixes the recovery; the
/// densities are `problem`'s.
fn form_find_explicit_combined_relaxed(
    problem: &ForceDensityProblem<'_>,
    nodes_guess: &[[f64; 3]],
    surface_mat: &Mat<f64>,
) -> Result<Vec<[f64; 3]>, FreeFormError> {
    let d = combined_force_density_matrix(problem.members(), problem.q(), surface_mat);
    let spectrum = SymmetricSpectrum::of(&d)?;
    recover_coordinates(
        nodes_guess,
        spectrum.leading_eigenvectors(FREE_FORM_NULLITY),
    )
}

/// Adaptive GroupRatios search operating on the COMBINED `D = CᵀQC + surface_mat`,
/// returning the searched per-member `q`. Mirrors [`form_find_group_ratios`] but
/// the objective includes the fixed additive surface term so the search drives
/// the COMBINED nullity toward 4. Unlike the line-only search it certifies
/// nothing: the outer loop's residual is the stop test, and certifying its final
/// iterate is the validity gate.
///
/// `initial_group_mag` are warm-start magnitudes (e.g. from a line-only bootstrap)
/// that replace the `seed_ratios` absolute values as the starting point AND as the
/// centre of the search bracket. Starting near the correct combined q avoids the
/// stall-guard pitfall where coordinate descent at the wrong starting point can
/// reach a fixed point before the objective hits the nullity-4 minimum.
///
/// `problem` holds the validated seed densities and `nodes_guess` the current
/// iterate.
fn form_find_group_ratios_combined(
    problem: &ForceDensityProblem<'_>,
    nodes_guess: &[[f64; 3]],
    group_ids: &[usize],
    seed_ratios: &[f64],
    initial_group_mag: &[f64],
    reference_group: usize,
    surface_mat: &Mat<f64>,
) -> Result<Vec<f64>, FreeFormError> {
    let members = problem.members();
    let n_groups = seed_ratios.len();

    let group_sign: Vec<f64> = seed_ratios.iter().map(|r| r.signum()).collect();
    // Warm-start: use the provided initial magnitudes (e.g. from the line-only
    // bootstrap) rather than the seed_ratios abs values. This ensures the search
    // starts near the correct combined q and avoids the coordinate-descent stall
    // at the original seeds that causes the stall guard to fire prematurely.
    let mut group_mag: Vec<f64> = initial_group_mag.to_vec();

    let mut appears = vec![false; n_groups];
    for &g in group_ids {
        appears[g] = true;
    }
    let free_groups: Vec<usize> = (0..n_groups)
        .filter(|&g| g != reference_group && appears[g])
        .collect();

    // Objective: Σ λ² over the FREE_FORM_NULLITY smallest eigenvalues of the
    // COMBINED D = line_D(q) + surface_mat. surface_mat is a fixed additive term.
    // Deliberately UN-normalised, unlike combined_eig_gap_objective's
    // normalised_eig_gap: `reference_group` already gauge-fixes this search, so
    // there is no gauge factor to cancel, and normalising would perturb the
    // OBJ_TOL / coordinate-descent stop thresholds below for no benefit this
    // path needs (task 6413 design decision). A `D` with no spectrum scores +∞,
    // rejecting the trial.
    let objective = |group_mag: &[f64]| -> f64 {
        let q = assemble_group_q(members.len(), group_ids, &group_sign, group_mag);
        let d = combined_force_density_matrix(members, &q, surface_mat);
        SymmetricSpectrum::of(&d).map_or(f64::INFINITY, |spectrum| {
            spectrum
                .eigenvalues()
                .iter()
                .take(FREE_FORM_NULLITY)
                .map(|v| v * v)
                .sum()
        })
    };

    const MAX_ROUNDS: usize = 64;
    const OBJ_TOL: f64 = 1e-20;

    // Phase 1: Diagonal scale sweep — scale ALL free groups by the same factor s.
    //
    // The combined D's objective Σλ² has a ridge along strut_mag ≈ vert_mag (from
    // the Z-equilibrium constraint: strut + vert = 0 → their magnitudes must agree).
    // Coordinate descent along individual axes stalls on this ridge — the 1-D
    // minimum over either strut or vert (with the other fixed) lands back at the
    // starting point when they are already equal.  A uniform scale over all free
    // groups moves ALONG the ridge toward the global minimum without crossing it,
    // placing the search in the basin of attraction for the per-group coordinate
    // descent in Phase 2.
    {
        let baseline = group_mag.clone();
        let best_s = minimize_on_coordinate(
            |s| {
                let mut trial = baseline.clone();
                for &g in &free_groups {
                    trial[g] = (baseline[g] * s).max(1e-10);
                }
                objective(&trial)
            },
            1.0 / SEARCH_BRACKET_FACTOR,
            SEARCH_BRACKET_FACTOR,
        );
        for &g in &free_groups {
            group_mag[g] = (group_mag[g] * best_s).max(1e-10);
        }
    }

    // Phase 2: Coordinate descent over individual free-group magnitudes.
    // With the diagonal sweep already in the basin, this refines any remaining
    // per-group ratio differences from the scale-only approximation.
    let mut obj = objective(&group_mag);
    for _ in 0..MAX_ROUNDS {
        if obj < OBJ_TOL {
            break;
        }
        let before = obj;
        for &g in &free_groups {
            // Bracket centred on the current (warm) magnitude — not on seed_ratios.
            let lo = initial_group_mag[g] / SEARCH_BRACKET_FACTOR;
            let hi = initial_group_mag[g] * SEARCH_BRACKET_FACTOR;
            let best = minimize_on_coordinate(
                |m| {
                    let mut trial = group_mag.clone();
                    trial[g] = m;
                    objective(&trial)
                },
                lo,
                hi,
            );
            group_mag[g] = best;
        }
        obj = objective(&group_mag);
        if before - obj <= 1e-18 * before.max(1.0) {
            break;
        }
    }

    let q = assemble_group_q(members.len(), group_ids, &group_sign, &group_mag);

    // The outer loop moves the geometry itself, so the recovered nodes are not
    // returned; a found `q` whose forced-nullity recovery cannot span 3-D is
    // still this search's error.
    form_find_explicit_combined_relaxed(&problem.with_densities(&q)?, nodes_guess, surface_mat)?;
    Ok(q)
}

/// Eigenvalue-gap objective for the free-standing combined fixed point: the
/// sum of squares of the `FREE_FORM_NULLITY` smallest-|λ| eigenvalues of
/// `D_combined = CᵀQC + Σ_T σ_T·L_T` at force densities `q` and geometry `x`,
/// normalised via [`normalised_eig_gap`] so the objective is GAUGE-invariant:
/// under a uniform `q → λ·q`, `σ → λ·σ` rescaling every eigenvalue of `D`
/// scales by `λ`, so an un-normalised sum-of-squares would scale by `λ²` and
/// drive [`combined_geometry_descent_step`]'s finite-difference gradient —
/// and therefore its backtracking line search — to different accept/reject
/// decisions depending on the force-density/stress scale.
///
/// The line-only `D` is geometry-independent and rank-deficient by 4 for a whole
/// *affine family* of realisations. The membrane cotangent weights DO depend on
/// geometry, so the combined `D` only reaches nullity 4 at the specific symmetric
/// realisation. Driving this objective to zero over the node coordinates selects
/// that realisation. A degenerate trial geometry (zero-area triangle), or a
/// combined `D` with no spectrum, returns `+∞` so the line search rejects it.
fn combined_eig_gap_objective(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    surface_stresses: &[f64],
    x: &[[f64; 3]],
) -> f64 {
    match assemble_surface_matrix(n, surfaces, surface_stresses, x) {
        Ok(surface_mat) => {
            let d = combined_force_density_matrix(members, q, &surface_mat);
            SymmetricSpectrum::of(&d)
                .map_or(f64::INFINITY, |spectrum| normalised_eig_gap(&spectrum))
        }
        Err(_) => f64::INFINITY,
    }
}

/// Sum of `(λ/max_mag)²` over the `FREE_FORM_NULLITY` smallest-|λ|
/// eigenvalues of `spectrum` — the gauge-invariant score
/// [`combined_eig_gap_objective`] reports. Dividing by its `max_mag` (which
/// scales by `λ` exactly as every other eigenvalue does under a uniform
/// gauge change) cancels the `λ²` scaling a raw sum-of-squares would carry,
/// so the finite-difference gradient taken of this score is gauge-invariant
/// too.
///
/// `max_mag <= 0` returns `0.0` rather than dividing by zero: an
/// identically-zero `D` (or an all-zero spectrum) has no spectral gap left
/// to close, so a perfect (zero) score is the honest answer — unlike
/// [`all_node_equilibrium_residual_relative`]'s vacuous-zero hazard, this
/// objective is not itself the outer loop's convergence gate (the residual
/// is), so there is no equivalent risk of prematurely signalling
/// convergence.
///
/// NOT used by `form_find_group_ratios_combined`'s own eigenvalue-gap
/// search, which deliberately keeps the un-normalised `Σλ²` form — see the
/// comment on its `objective` closure for why.
fn normalised_eig_gap(spectrum: &SymmetricSpectrum) -> f64 {
    let max_mag = spectrum.max_mag();
    if max_mag <= 0.0 {
        return 0.0;
    }
    spectrum
        .eigenvalues()
        .iter()
        .take(FREE_FORM_NULLITY)
        .map(|v| (v / max_mag) * (v / max_mag))
        .sum()
}

/// One backtracking gradient-descent step on [`combined_eig_gap_objective`] over
/// the node geometry at fixed force densities `q`. Returns the stepped geometry
/// and the (adaptively grown/shrunk) step size for the next call. If no downhill
/// step is found within the backtracking budget the geometry is returned
/// unchanged (the outer loop then either has already converged or stops).
///
/// The gradient is a central finite difference over the `3n` coordinates.
fn combined_geometry_descent_step(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    surface_stresses: &[f64],
    current: &[[f64; 3]],
    step: f64,
) -> (Vec<[f64; 3]>, f64) {
    let obj =
        |x: &[[f64; 3]]| combined_eig_gap_objective(n, members, q, surfaces, surface_stresses, x);
    let g0 = obj(current);

    // Central finite-difference gradient over the 3n coordinates.
    const FD_H: f64 = 1e-6;
    let mut grad = vec![[0.0_f64; 3]; n];
    for r in 0..n {
        for a in 0..3 {
            let mut xp = current.to_vec();
            xp[r][a] += FD_H;
            let mut xm = current.to_vec();
            xm[r][a] -= FD_H;
            grad[r][a] = (obj(&xp) - obj(&xm)) / (2.0 * FD_H);
        }
    }

    // Backtracking line search: accept the first step that strictly decreases the
    // objective, growing the step on success and halving it on each rejection.
    const STEP_GROW: f64 = 1.3;
    const STEP_SHRINK: f64 = 0.5;
    const MAX_BACKTRACK: usize = 40;
    let mut step = step;
    for _ in 0..MAX_BACKTRACK {
        let mut trial = current.to_vec();
        for r in 0..n {
            for a in 0..3 {
                trial[r][a] -= step * grad[r][a];
            }
        }
        if obj(&trial) < g0 {
            return (trial, step * STEP_GROW);
        }
        step *= STEP_SHRINK;
    }
    (current.to_vec(), step)
}

/// Assemble a [`FreeFormResult`] at a fixed geometry + force-density vector:
/// per-member axial force `Nᵢ = qᵢ·Lᵢ` on `geom`, carrying `q` through as the
/// reported densities. `nullity` / `converged` / `surface_stresses` are filled by
/// the caller (the combined outer loop's tail takes the nullity from the final
/// iterate's certificate and sets the echo on convergence).
fn combined_result_at(
    members: &[(usize, usize)],
    q: &[f64],
    geom: &[[f64; 3]],
) -> FreeFormResult {
    let member_forces = member_axial_forces(members, q, geom);
    FreeFormResult {
        nodes: geom.to_vec(),
        member_forces,
        force_densities: q.to_vec(),
        nullity: 0,
        converged: false,
        surface_stresses: Vec::new(),
    }
}

/// Explicit-mode pipeline: certify the form, recover the gauge-fixed
/// free-standing coordinates, and compute member forces — the deterministic core
/// that [`ForceDensitySpec::GroupRatios`] also delegates to once its search finds
/// an admissible `q`.
fn form_find_explicit(problem: &ForceDensityProblem<'_>) -> Result<FreeFormResult, FreeFormError> {
    let (members, q) = (problem.members(), problem.q());

    // The single dense EVD, shared by the certification and the recovery.
    let form = explicit_form(problem)?;
    let nullity = form.nullity();

    // Gauge-fixed coordinates from null(D), aligned to the caller's guess.
    let nodes = recover_coordinates(problem.nodes(), form.null_basis())?;

    // Per-member axial force Nᵢ = qᵢ · Lᵢ on the recovered geometry, in
    // struts-then-cables (input) order — mirrors the anchored kernel's force pass.
    let member_forces = member_axial_forces(members, q, &nodes);

    Ok(FreeFormResult {
        nodes,
        member_forces,
        force_densities: q.to_vec(),
        nullity,
        converged: true,
        surface_stresses: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Adaptive GroupRatios force-density search
// ---------------------------------------------------------------------------

/// Bounded magnitude bracket for each free group: search within `×/÷ 20` of the
/// seed magnitude. This is the "bounded local search" the plan names — it
/// preserves each group's seed sign and keeps the force-density graph connected,
/// so the search stays in the well-posed region around the seed.
const SEARCH_BRACKET_FACTOR: f64 = 20.0;

/// Adaptive [`ForceDensitySpec::GroupRatios`] form-find: search the free relative
/// group densities for a configuration that makes `D` rank-deficient by `d + 1`,
/// then delegate to the explicit pipeline.
///
/// Each group's *sign* is fixed by its seed sign; the search varies only the
/// *magnitude* of the non-reference groups within a bounded bracket. The
/// reference group is held at its seed value as the scale gauge (overall scaling
/// of `q` is nullity-invariant, so only relative ratios matter).
///
/// The objective is the sum of squares of the `FREE_FORM_NULLITY` smallest
/// eigenvalues of `D` (a smooth surrogate for "nullity ≥ 4"); since `λ_(1) = 0`
/// always (the all-ones translation mode), driving it to zero pushes
/// `λ_(2..4) → 0` ⇒ nullity 4. Minimised by coordinate descent with a
/// log-spaced coarse scan + golden-section line search per free group.
///
/// On a successful search the found `q` is handed to [`form_find_explicit`],
/// whose certification is the validity gate: if the found `D` is not a form
/// (the search did not reach an admissible `q`, or the structure is not one
/// connected piece), its [`FreeFormError::NotAForm`] becomes
/// [`FreeFormError::SearchDidNotConverge`] carrying the same [`FormDefect`].
/// Singular-recovery errors propagate unchanged; `problem` was validated at the
/// seed densities, whose signs the search keeps.
///
/// # Scaling
///
/// Every objective evaluation recomputes a *full* dense self-adjoint EVD of `D`
/// (`O(n³)` in the node count `n`), and the coordinate-descent budget
/// (`MAX_ROUNDS` rounds × the per-coordinate `SCAN` + golden-section line search)
/// can reach `~10⁴` EVDs in the worst case. That is negligible for the `n = 6`
/// triplex this kernel targets, but the per-evaluation cost grows as `O(n³)` and
/// the iteration count is bounded only by the search tolerances, not by problem
/// difficulty. Scaling to larger free-standing structures would want a partial
/// (smallest-`k`) eigensolve and/or a reduced `SCAN` / iteration budget; that is
/// a deliberate follow-up, out of scope for the 6-node target here.
fn form_find_group_ratios(
    problem: &ForceDensityProblem<'_>,
    group_ids: &[usize],
    seed_ratios: &[f64],
    reference_group: usize,
) -> Result<FreeFormResult, FreeFormError> {
    let members = problem.members();
    let n_groups = seed_ratios.len();
    let n = problem.nodes().len();

    // Per-group sign (fixed throughout) and current magnitude. The reference
    // group's magnitude stays at its seed; free groups start there and are
    // refined by the search.
    let group_sign: Vec<f64> = seed_ratios.iter().map(|r| r.signum()).collect();
    let mut group_mag: Vec<f64> = seed_ratios.iter().map(|r| r.abs()).collect();

    // Free groups = every group that actually appears in `group_ids`, except the
    // fixed reference. (Groups absent from `group_ids` do not enter `D`, so
    // searching them would be wasted work.)
    let mut appears = vec![false; n_groups];
    for &g in group_ids {
        appears[g] = true;
    }
    let free_groups: Vec<usize> = (0..n_groups)
        .filter(|&g| g != reference_group && appears[g])
        .collect();

    // Objective: Σ λ² over the FREE_FORM_NULLITY smallest-|λ| eigenvalues of
    // `D(group_mag)`. [`SymmetricSpectrum`] orders eigenvalues ascending by |λ|,
    // so `take(4)` is exactly those. The smooth sum-of-squares (rather than the
    // bare 4th eigenvalue, which is V-shaped/kinked) keeps coordinate descent
    // from stalling at the tolerance crossing. A `D` with no spectrum scores
    // +∞, rejecting the trial.
    let objective = |group_mag: &[f64]| -> f64 {
        let q = assemble_group_q(members.len(), group_ids, &group_sign, group_mag);
        let d = assemble_d(n, members, &q);
        SymmetricSpectrum::of(&d).map_or(f64::INFINITY, |spectrum| {
            spectrum
                .eigenvalues()
                .iter()
                .take(FREE_FORM_NULLITY)
                .map(|v| v * v)
                .sum()
        })
    };

    // Coordinate descent. Eigen-evals are cheap (n×n dense EVD, n = node count),
    // so we afford a generous round budget and run tight — well below
    // `NULLITY_REL_TOL`'s effective threshold so the found `q` survives
    // re-classification in `form_find_explicit`.
    const MAX_ROUNDS: usize = 64;
    // Drive the objective *well* below the nullity-classifier's threshold trap.
    // `form_find_explicit` re-classifies the found `q` with `NULLITY_REL_TOL ·
    // max|λ|` (≈ 6e-8 for the prism); since the objective is Σλ² over the 4
    // smallest, that threshold squared (≈ 3.6e-15) is the floor below which the
    // found eigenvalues clear the trap. `1e-20` leaves several orders of margin
    // (and `obj` bottoms out near 1e-30 at the exact closed form), so the
    // recovered ratios land within the `1e-6` goldens.
    const OBJ_TOL: f64 = 1e-20;

    let mut obj = objective(&group_mag);
    for _ in 0..MAX_ROUNDS {
        if obj < OBJ_TOL {
            break;
        }
        let before = obj;
        for &g in &free_groups {
            let lo = seed_ratios[g].abs() / SEARCH_BRACKET_FACTOR;
            let hi = seed_ratios[g].abs() * SEARCH_BRACKET_FACTOR;
            let best = minimize_on_coordinate(
                |m| {
                    let mut trial = group_mag.clone();
                    trial[g] = m;
                    objective(&trial)
                },
                lo,
                hi,
            );
            group_mag[g] = best;
        }
        obj = objective(&group_mag);
        // Stall guard: the objective is bounded below by 0, so once a full round
        // yields no meaningful improvement coordinate descent has reached a
        // (local) minimum. The threshold is tiny (≈ machine-precision-scaled) so
        // it fires only at the genuine plateau — the infeasible (e.g.
        // all-positive) case settles to an exact fixed point (improvement → 0),
        // which `form_find_explicit` below turns into SearchDidNotConverge, while
        // a feasible search keeps refining deep past the threshold trap.
        if before - obj <= 1e-18 * before.max(1.0) {
            break;
        }
    }

    // Delegate to the explicit pipeline with the found `q`, whose certification
    // is the validity gate: a certified form form-finds, and a `D` that is not a
    // form becomes SearchDidNotConverge naming its defect.
    let q = assemble_group_q(members.len(), group_ids, &group_sign, &group_mag);
    match form_find_explicit(&problem.with_densities(&q)?) {
        Err(FreeFormError::NotAForm(defect)) => Err(FreeFormError::SearchDidNotConverge {
            defect: Some(defect),
        }),
        other => other,
    }
}

/// Build the full per-member force-density vector from per-group magnitudes:
/// member `i`'s `q` is its group's signed ratio `group_sign[g] · group_mag[g]`
/// (sign fixed by the seed, magnitude searched). Struts-then-cables member order
/// follows `group_ids` / `members`.
fn assemble_group_q(
    n_members: usize,
    group_ids: &[usize],
    group_sign: &[f64],
    group_mag: &[f64],
) -> Vec<f64> {
    (0..n_members)
        .map(|i| {
            let g = group_ids[i];
            group_sign[g] * group_mag[g]
        })
        .collect()
}

/// Minimise a single-coordinate objective over the positive interval `[lo, hi]`.
///
/// A log-spaced coarse scan first brackets the global minimum (robust against
/// the objective not being perfectly unimodal across the wide `×/÷ 20` range),
/// then [`golden_section_min`] refines within the bracketing sub-interval. Log
/// spacing matches the multiplicative bracket, giving even relative resolution.
fn minimize_on_coordinate<F: Fn(f64) -> f64>(f: F, lo: f64, hi: f64) -> f64 {
    const SCAN: usize = 48;
    let log_lo = lo.ln();
    let log_hi = hi.ln();
    let grid = |i: usize| (log_lo + (log_hi - log_lo) * (i as f64) / (SCAN as f64)).exp();

    let mut best_i = 0usize;
    let mut best_f = f(grid(0));
    for i in 1..=SCAN {
        let fx = f(grid(i));
        if fx < best_f {
            best_f = fx;
            best_i = i;
        }
    }

    // Bracket the minimum by its log-grid neighbours (clamped to the endpoints).
    let a = grid(best_i.saturating_sub(1));
    let b = grid((best_i + 1).min(SCAN));
    golden_section_min(f, a, b)
}

/// Golden-section minimisation of a unimodal `f` on `[a, b]`. The fixed
/// iteration count drives the bracket to ~machine precision (each step shrinks
/// it by the golden ratio ≈ 0.618; 80 steps ⇒ width × ~1e-17), which is far
/// tighter than the `1e-6` ratio tolerance the goldens assert.
fn golden_section_min<F: Fn(f64) -> f64>(f: F, mut a: f64, mut b: f64) -> f64 {
    const ITERS: usize = 80;
    // 1/φ and 1/φ².
    let inv_phi = (5.0_f64.sqrt() - 1.0) / 2.0;
    let inv_phi2 = (3.0 - 5.0_f64.sqrt()) / 2.0;

    let mut c = a + inv_phi2 * (b - a);
    let mut d = a + inv_phi * (b - a);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..ITERS {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = a + inv_phi2 * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + inv_phi * (b - a);
            fd = f(d);
        }
    }
    (a + b) / 2.0
}

// ---------------------------------------------------------------------------
// Free-standing form certification
// ---------------------------------------------------------------------------

/// Nullity of a 3-D free-standing form's `D`: spatial dimension + 1, i.e. three
/// coordinate null directions plus the all-ones translation mode
/// (`D · 𝟙 = 0` for any `q`, since `C · 𝟙 = 0`).
const FREE_FORM_NULLITY: usize = 4;

/// Connected pieces of `D`'s force graph, where nodes `i ≠ j` are joined
/// wherever `D[i,j] ≠ 0`. A node with an all-zero row is a piece on its own.
fn force_graph_components(d: &Mat<f64>) -> usize {
    let n = d.nrows();
    let mut reached = vec![false; n];
    let mut components = 0;
    for root in 0..n {
        if reached[root] {
            continue;
        }
        components += 1;
        reached[root] = true;
        let mut frontier = vec![root];
        while let Some(i) = frontier.pop() {
            for j in 0..n {
                if !reached[j] && d[(i, j)] != 0.0 {
                    reached[j] = true;
                    frontier.push(j);
                }
            }
        }
    }
    components
}

/// A `D` certified as a 3-D free-standing form, carrying the spectrum its
/// certification classified so coordinate recovery reuses that one dense EVD.
#[derive(Debug)]
struct FreeStandingForm(SymmetricSpectrum);

impl FreeStandingForm {
    /// The single validity rule: `d` is a free-standing form when its force
    /// graph is ONE connected piece and it is rank-deficient by exactly
    /// [`FREE_FORM_NULLITY`]; otherwise [`FreeFormError::NotAForm`] with the
    /// observed [`FormDefect`]. Both conditions are needed: every connected
    /// piece carries its own translation mode, so a disconnected `D` can still
    /// show nullity 4. [`FreeFormError::Spectrum`] when `d` has no spectrum.
    fn certify(d: &Mat<f64>) -> Result<FreeStandingForm, FreeFormError> {
        let spectrum = SymmetricSpectrum::of(d)?;
        let nullity = spectrum.nullity(NULLITY_REL_TOL);
        let components = force_graph_components(d);
        if components == 1 && nullity == FREE_FORM_NULLITY {
            Ok(FreeStandingForm(spectrum))
        } else {
            Err(FreeFormError::NotAForm(FormDefect {
                nullity,
                components,
            }))
        }
    }

    /// Nullity of the certified `D`: [`FREE_FORM_NULLITY`], by certification.
    fn nullity(&self) -> usize {
        self.0.nullity(NULLITY_REL_TOL)
    }

    /// Orthonormal basis of the certified `D`'s null space, as columns.
    fn null_basis(&self) -> MatRef<'_, f64> {
        self.0.null_basis(NULLITY_REL_TOL)
    }
}

/// Assemble a validated problem's `D` and certify it as a free-standing form
/// ([`FreeFormError::NotAForm`] otherwise, [`FreeFormError::Spectrum`] when `D`
/// has no spectrum).
fn explicit_form(problem: &ForceDensityProblem<'_>) -> Result<FreeStandingForm, FreeFormError> {
    let d = assemble_d(problem.nodes().len(), problem.members(), problem.q());
    FreeStandingForm::certify(&d)
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
/// `null_block`'s columns are orthonormal eigenvectors of `D`, so the
/// projection is just `X = U₀ (U₀ᵀ G)`, where `U₀` is that null block and `G`
/// the guess. The result lies exactly in `null(D)`, so
/// `D · X = 0` (equilibrium), and is the closest such configuration to the guess
/// (the all-ones direction inside `U₀` absorbs translation).
///
/// Returns [`FreeFormError::SingularRecovery`] if the recovered coordinates fail
/// to span 3-D (a rank-deficient realisation, e.g. a degenerate guess).
fn recover_coordinates(
    nodes_guess: &[[f64; 3]],
    null_block: MatRef<'_, f64>,
) -> Result<Vec<[f64; 3]>, FreeFormError> {
    let n = nodes_guess.len();
    let nullity = null_block.ncols();
    let u0 = null_block;

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
    use crate::form_find::IndexedInput;

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

    /// Closed-form COMBINED self-stress for the triplex + two equilateral
    /// membrane triangles, struts-then-cables order (σ-aware sibling of
    /// [`closed_form_q`]). At the free-standing equilibrium both membrane
    /// triangles are equilateral, so every cotangent in the surface stencil is
    /// cot(60°) = 1/√3 and `Σ_T σ_T·L_T` collapses to a uniform extra edge
    /// weight w = σ·cot(60°)/2 = σ/(2√3) on exactly the six horizontal cables.
    /// Hence D_combined(q) ≡ D_line(q + w·1{horizontal}), and a valid form
    /// needs q + w·1{horizontal} ∝ the triplex self-stress (-√3, 1, √3).
    /// Pinning the horizontals at 1 gives λ = 1 + σ/(2√3), i.e.
    /// q_strut = -(√3 + σ/2), q_horiz = 1, q_vert = +(√3 + σ/2).
    fn closed_form_combined_q(sigma: f64) -> Vec<f64> {
        let a = 3.0_f64.sqrt() + sigma / 2.0;
        vec![
            -a, -a, -a, // struts
            1.0, 1.0, 1.0, // top horizontals
            1.0, 1.0, 1.0, // bottom horizontals
            a, a, a, // verticals
        ]
    }

    #[test]
    fn closed_form_prism_q_has_nullity_four_with_spectral_gap() {
        let (members, _kinds) = triplex_topology();
        let q = closed_form_q();
        let d = assemble_d(6, &members, &q);
        let spec = SymmetricSpectrum::of(&d).expect("finite D");

        assert_eq!(
            spec.nullity(NULLITY_REL_TOL),
            4,
            "closed-form prism q must give nullity 4 (3 coord modes + translation); eigenvalues = {:?}",
            spec.eigenvalues(),
        );

        // Eigenvalues are sorted ascending by magnitude: the fourth-smallest is
        // still in the null space (~0), and there is a wide gap to the fifth,
        // which sits at ~6.
        assert!(
            spec.eigenvalues()[3].abs() < 1e-9,
            "fourth-smallest |λ| must be ~0 (still null), got {}",
            spec.eigenvalues()[3],
        );
        assert!(
            (spec.eigenvalues()[4].abs() - 6.0).abs() < 1e-6,
            "fifth |λ| must be ~6 (spectral gap above the null space), got {}",
            spec.eigenvalues()[4],
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
        let d = assemble_d(6, &members, &q);
        let spec = SymmetricSpectrum::of(&d).expect("finite D");

        assert_eq!(
            spec.nullity(NULLITY_REL_TOL),
            1,
            "generic admissible q has only the all-ones translation mode in null(D); eigenvalues = {:?}",
            spec.eigenvalues(),
        );
    }

    // ---- Free-standing form certification (FreeStandingForm::certify) ----

    #[test]
    fn certify_accepts_connected_nullity_four_d() {
        let (members, _kinds) = triplex_topology();
        let d = assemble_d(6, &members, &closed_form_q());

        let form = FreeStandingForm::certify(&d).expect("closed-form prism D is a form");
        assert_eq!(form.nullity(), FREE_FORM_NULLITY);
    }

    #[test]
    fn certify_names_the_nullity_of_a_connected_wrong_rank_d() {
        let (members, kinds) = triplex_topology();
        let d = assemble_d(6, &members, &generic_admissible_q(&kinds));

        assert_eq!(
            FreeStandingForm::certify(&d).unwrap_err(),
            FreeFormError::NotAForm(FormDefect {
                nullity: 1,
                components: 1,
            }),
        );
    }

    #[test]
    fn certify_rejects_a_disconnected_d_whose_nullity_is_four() {
        // Four disjoint unit cable pairs: one translation mode per pair makes
        // the nullity exactly 4, so the count alone certifies nothing.
        let members = [(0, 1), (2, 3), (4, 5), (6, 7)];
        let d = assemble_d(8, &members, &[1.0; 4]);

        assert_eq!(
            FreeStandingForm::certify(&d).unwrap_err(),
            FreeFormError::NotAForm(FormDefect {
                nullity: 4,
                components: 4,
            }),
        );
    }

    /// `d` with one isolated node appended: a zero last row and column.
    fn with_isolated_node(d: &Mat<f64>) -> Mat<f64> {
        let n = d.nrows();
        Mat::from_fn(
            n + 1,
            n + 1,
            |i, j| if i < n && j < n { d[(i, j)] } else { 0.0 },
        )
    }

    #[test]
    fn certify_counts_an_isolated_node_as_a_component() {
        let (members, _kinds) = triplex_topology();
        let prism_d = assemble_d(6, &members, &closed_form_q());
        let d = with_isolated_node(&prism_d);

        assert_eq!(
            FreeStandingForm::certify(&d).unwrap_err(),
            FreeFormError::NotAForm(FormDefect {
                nullity: 5,
                components: 2,
            }),
        );
    }

    // ---- Explicit-mode feasibility diagnostics ----

    #[test]
    fn explicit_cable_with_nonpositive_q_is_sign_violation() {
        let (members, kinds) = triplex_topology();
        let mut q = closed_form_q();
        // Force a top cable (member 3) to a non-positive density: a cable must
        // carry tension (q > 0), so this is infeasible input.
        q[3] = -1.0;
        assert_eq!(
            form_find_free(
                &perturbed_prism_guess(),
                &members,
                &kinds,
                &ForceDensitySpec::Explicit(q),
            )
            .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::SignViolation { member: 3 }),
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
            form_find_free(
                &perturbed_prism_guess(),
                &members,
                &kinds,
                &ForceDensitySpec::Explicit(q),
            )
            .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::SignViolation { member: 0 }),
        );
    }

    #[test]
    fn explicit_wrong_nullity_q_is_not_a_form_naming_its_nullity() {
        let (members, kinds) = triplex_topology();
        // Admissible (signs honour kinds) but non-special: nullity 1 (only the
        // translation mode), not the d+1 = 4 a valid 3-D form requires, on one
        // connected structure.
        let q = generic_admissible_q(&kinds);
        let guess = perturbed_prism_guess();
        let problem = ForceDensityProblem::new(&guess, &members, &kinds, &q, &[], &[], &[])
            .expect("admissible q is valid input");
        assert_eq!(
            explicit_form(&problem).unwrap_err(),
            FreeFormError::NotAForm(FormDefect {
                nullity: 1,
                components: 1,
            }),
        );
    }

    #[test]
    fn explicit_length_mismatch_is_dimension_mismatch() {
        let (members, kinds) = triplex_topology();
        // One density short of the 12 members → dimension mismatch, caught up
        // front before any sign or nullity work.
        let q = vec![1.0; members.len() - 1];
        assert_eq!(
            form_find_free(
                &perturbed_prism_guess(),
                &members,
                &kinds,
                &ForceDensitySpec::Explicit(q),
            )
            .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::DimensionMismatch),
        );
    }

    #[test]
    fn explicit_out_of_range_member_index_is_index_out_of_range() {
        let (mut members, kinds) = triplex_topology();
        let q = closed_form_q();
        // Point a member at node 6 — out of range for the 6-node (0..=5) problem.
        // `D` is 6×6, so without the bounds guard this panics on the `d[(j, j)]`
        // index in `assemble_d`; the guard turns it into a
        // clean indexed input error through the public entry point (no panic).
        members[0] = (0, 6);
        assert_eq!(
            form_find_free(
                &perturbed_prism_guess(),
                &members,
                &kinds,
                &ForceDensitySpec::Explicit(q),
            )
            .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::IndexOutOfRange {
                what: IndexedInput::MemberEndpoint { member: 0 },
                index: 6,
                len: 6,
            }),
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
        let prism = canonical_prism();
        let problem = ForceDensityProblem::new(&prism, &members, &kinds, &q, &[], &[], &[])
            .expect("closed-form prism q is valid input");
        let form = explicit_form(&problem).expect("closed-form prism q is a form");

        // A mildly perturbed symmetric-prism guess (recovery should gauge-fix
        // to it).
        let guess = perturbed_prism_guess();

        let x = recover_coordinates(&guess, form.null_basis())
            .expect("perturbed symmetric-prism guess must recover a 3-D realisation");

        // (1) Equilibrium: X lies in null(D), so the per-axis residual ‖D·X‖∞
        // must vanish to machine precision — the rock-solid correctness signal.
        let d = assemble_d(6, &members, &q);
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

    #[test]
    fn recover_coordinates_from_coplanar_guess_is_singular_recovery() {
        let (members, kinds) = triplex_topology();
        let q = closed_form_q();
        // The valid nullity-4 spectrum for the closed-form prism — identical to
        // the healthy-recovery test's input, so *only* the guess is degenerate
        // here, isolating the SingularRecovery guard.
        let prism = canonical_prism();
        let problem = ForceDensityProblem::new(&prism, &members, &kinds, &q, &[], &[], &[])
            .expect("closed-form prism q is valid input");
        let form = explicit_form(&problem).expect("closed-form prism q is a form");

        // A *coplanar* (degenerate) guess: the canonical prism's x/y arrangement
        // flattened into the z = 0 plane. Per-axis projection onto null(D) leaves
        // the all-zero z column at zero (a constant vector lies in null(D) via the
        // all-ones mode), so every recovered node has z = 0 — a strictly planar,
        // rank-deficient realisation. The centred-covariance det-vs-isotropic
        // guard must reject it as SingularRecovery rather than return a flat
        // "3-D" form. (Exercises the det branch; a regression that inverts the
        // condition or mis-scales SINGULAR_REL_TOL would let it through.)
        let coplanar_guess: Vec<[f64; 3]> =
            canonical_prism().iter().map(|p| [p[0], p[1], 0.0]).collect();

        assert_eq!(
            recover_coordinates(&coplanar_guess, form.null_basis()).unwrap_err(),
            FreeFormError::SingularRecovery,
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
            FreeFormError::SearchDidNotConverge {
                defect: Some(FormDefect {
                    nullity: 1,
                    components: 1,
                }),
            },
        );
    }

    /// Whether `result` is the search's non-convergence carrying a defect that
    /// names two connected pieces — the observable a disconnected free-standing
    /// model must produce, whatever nullity its search happened to stop at.
    fn is_two_piece_non_convergence(result: &Result<FreeFormResult, FreeFormError>) -> bool {
        matches!(
            result,
            Err(FreeFormError::SearchDidNotConverge {
                defect: Some(FormDefect { components: 2, .. }),
            })
        )
    }

    #[test]
    fn line_only_group_ratios_with_an_isolated_node_does_not_converge() {
        // The search drives D's 4 smallest eigenvalues to 0. The isolated node
        // contributes a free translation mode, so the triplex itself settles
        // FLAT (nullity 3) and the total is exactly 4; the extra node keeps the
        // recovery 3-D. Only the one-connected-piece condition sees through it.
        let (members, kinds) = triplex_topology();
        let mut guess = perturbed_prism_guess();
        guess.push([0.3, 0.2, 0.4]);
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        let result = form_find_free(&guess, &members, &kinds, &spec);
        assert!(is_two_piece_non_convergence(&result), "{result:?}");
    }

    #[test]
    fn line_only_group_ratios_with_a_disconnected_cable_pair_does_not_converge() {
        let (mut members, mut kinds) = triplex_topology();
        let mut guess = perturbed_prism_guess();
        guess.push([0.3, 0.2, 0.4]);
        guess.push([0.3, 0.2, 0.9]);
        members.push((6, 7));
        kinds.push(MemberKind::Cable);
        let mut group_ids = triplex_group_ids();
        group_ids.push(3);
        let spec = ForceDensitySpec::GroupRatios {
            group_ids,
            seed_ratios: vec![-1.0, 1.0, 1.0, 1.0],
            reference_group: 1,
        };

        let result = form_find_free(&guess, &members, &kinds, &spec);
        assert!(is_two_piece_non_convergence(&result), "{result:?}");
    }

    #[test]
    fn group_ratios_out_of_range_member_index_is_index_out_of_range() {
        let (mut members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // An out-of-range node index (6 ≥ the 6-node problem) must be rejected
        // before the search assembles `D`, not panic mid-search.
        members[0] = (0, 6);
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };
        assert_eq!(
            form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::IndexOutOfRange {
                what: IndexedInput::MemberEndpoint { member: 0 },
                index: 6,
                len: 6,
            }),
        );
    }

    // ---- GroupRatios-mode group-spec guards (GroupSpec::new) ----

    #[test]
    fn group_ratios_zero_seed_ratio_is_a_zero_seed() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // A zero seed has no sign to hold fixed while the search varies its
        // magnitude, so a zero entry in `seed_ratios` is rejected up front.
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 0.0, 1.0],
            reference_group: 1,
        };
        assert_eq!(
            form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
                GroupSpecDefect::ZeroSeed { group: 1 }
            )),
        );
    }

    #[test]
    fn group_ratios_reference_group_out_of_range_is_unseeded() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // `seed_ratios` is indexed by group id (3 groups ⇒ valid ids 0..=2), so a
        // `reference_group` of 3 is out of range.
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 3,
        };
        assert_eq!(
            form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
                GroupSpecDefect::UnseededReferenceGroup
            )),
        );
    }

    #[test]
    fn group_ratios_group_id_out_of_range_is_an_unseeded_group() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // A `group_ids` entry referencing a group with no seed ratio (id 3 with
        // only 3 groups defined) is out of range.
        let mut group_ids = triplex_group_ids();
        group_ids[0] = 3; // no seed_ratios[3]
        let spec = ForceDensitySpec::GroupRatios {
            group_ids,
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };
        assert_eq!(
            form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
                GroupSpecDefect::UnseededGroup { member: 0 }
            )),
        );
    }

    #[test]
    fn group_ratios_group_ids_length_mismatch_is_not_per_member() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // `group_ids` must be parallel to `members`; one short is caught before
        // any search work.
        let mut group_ids = triplex_group_ids();
        group_ids.pop();
        let spec = ForceDensitySpec::GroupRatios {
            group_ids,
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };
        assert_eq!(
            form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
                GroupSpecDefect::GroupIdsNotPerMember
            )),
        );
    }

    // ── δ (task 4415): surface-aware form_find_free_surfaces ──────────────────

    // Top {0,1,2} and bottom {3,4,5} triangle surfaces used in the δ tests.
    // These are the two face-triangles of the triplex prism.
    fn prism_surfaces() -> Vec<(usize, usize, usize)> {
        vec![(0, 1, 2), (3, 4, 5)]
    }

    // (a) `surfaces` and `surface_stresses` must agree in length — each triangle
    // needs exactly one isotropic σ. One stress short ⇒ SurfaceCountMismatch.
    #[test]
    fn surfaces_free_count_mismatch_is_surface_count_mismatch() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        let surfaces = prism_surfaces();
        let sigmas = vec![0.2]; // 2 surfaces, 1 stress → mismatch
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        assert_eq!(
            form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
                .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::SurfaceCountMismatch),
        );
    }

    // (b) A non-positive surface stress σ ≤ 0 is infeasible — the surface
    // analogue of a cable with q ≤ 0. Must return NonTensionSurfaceStress.
    #[test]
    fn surfaces_free_nonpositive_sigma_is_non_tension() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        let surfaces = prism_surfaces();
        let sigmas = vec![0.2, -0.1]; // second triangle σ < 0 → infeasible
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        assert_eq!(
            form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
                .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::NonTensionSurfaceStress { surface: 1 }),
        );
    }

    // (c) A surface triangle corner that indexes past the node array is
    // infeasible input — `assemble_surface_matrix` would panic on its
    // `nodes[gi]` index. The module contract promises infeasible input becomes
    // a clean typed error, never a panic.
    #[test]
    fn surfaces_free_out_of_range_index_is_index_out_of_range() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // Boundary index: 6 is the FIRST invalid index for the 6-node triplex,
        // so this pins the `≥ n` comparison that a `> n` typo would let pass.
        // The predicate ANDs three comparisons, so each sibling test puts the
        // bad index in a different corner — THIRD here, first and second in the
        // two anchored tests in `form_find` — pinning all three between them.
        let surfaces = vec![(0usize, 1usize, 6usize)];
        let sigmas = vec![0.2];
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        assert_eq!(
            form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
                .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::IndexOutOfRange {
                what: IndexedInput::SurfaceCorner { surface: 0 },
                index: 6,
                len: 6,
            }),
        );
    }

    // (d) Empty surfaces: form_find_free_surfaces with empty surfaces/stresses
    // must return a result that matches form_find_free in all line-only fields
    // (nodes / member_forces / force_densities / nullity / converged) and
    // carries an empty (NEVER absent) surface_stresses echo.
    #[test]
    fn surfaces_free_empty_matches_line_only_form_find_free() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        let line = form_find_free(&guess, &members, &kinds, &spec)
            .expect("line-only GroupRatios must form-find the prism");
        let surf = form_find_free_surfaces(&guess, &members, &kinds, &[], &[], &spec)
            .expect("empty-surface path must match the line-only result");

        // surface_stresses is an empty Vec, not absent.
        assert!(
            surf.surface_stresses.is_empty(),
            "empty surfaces ⇒ empty surface_stresses echo (got {:?})",
            surf.surface_stresses,
        );

        // Line-only fields must agree exactly (same D, same solve path).
        assert_eq!(surf.converged, line.converged);
        assert_eq!(surf.nullity, line.nullity);
        assert_eq!(surf.force_densities, line.force_densities);
        assert_eq!(surf.member_forces.len(), line.member_forces.len());
        for (a, b) in surf.member_forces.iter().zip(line.member_forces.iter()) {
            assert!((a - b).abs() < 1e-12, "member force mismatch: {a} vs {b}");
        }
        assert_eq!(surf.nodes.len(), line.nodes.len());
        let sub = |a: [f64; 3], b: [f64; 3]| -> f64 {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| (x - y).abs())
                .fold(0.0_f64, f64::max)
        };
        for (a, b) in surf.nodes.iter().zip(line.nodes.iter()) {
            assert!(sub(*a, *b) < 1e-12, "node mismatch: {a:?} vs {b:?}");
        }
    }

    // ── δ combined golden: triplex + isotropic membrane on top+bottom faces ──

    /// Independent (faer-free) reassembly of the combined force-density matrix
    /// D = CᵀQC (lines) + Σ_T σ_T·L_T (surface cotangent-Laplacians) at the
    /// given geometry, as a dense n×n array indexed as [row][col].
    /// Inlines the cotangent-Laplacian formula to avoid depending on the
    /// private `form_find::triangle_cotangent_laplacian` — this is an
    /// independent verification path (the honest signal), not a reuse test.
    #[allow(clippy::needless_range_loop)]
    fn reassemble_d_free(
        n: usize,
        members: &[(usize, usize)],
        q: &[f64],
        surfaces: &[(usize, usize, usize)],
        sigmas: &[f64],
        nodes: &[[f64; 3]],
    ) -> Vec<Vec<f64>> {
        let mut d = vec![vec![0.0_f64; n]; n];
        // Line members (CᵀQC rank-1 per member).
        for (&(j, k), &qi) in members.iter().zip(q.iter()) {
            d[j][j] += qi;
            d[k][k] += qi;
            d[j][k] -= qi;
            d[k][j] -= qi;
        }
        // Surface cotangent-Laplacian contributions (inlined formula).
        // For triangle (i,j,k), edge weight opposite vertex v is (σ/2)·cot(θ_v)
        // where cot(θ_v) = (e_a · e_b) / (2·Area), e_a/e_b the two edges from v.
        let vsub = |a: [f64; 3], b: [f64; 3]| -> [f64; 3] {
            [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
        };
        let vdot =
            |a: [f64; 3], b: [f64; 3]| -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] };
        let vcross = |a: [f64; 3], b: [f64; 3]| -> [f64; 3] {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        for (&(gi, gj, gk), &s) in surfaces.iter().zip(sigmas.iter()) {
            let pi = nodes[gi];
            let pj = nodes[gj];
            let pk = nodes[gk];
            let eij = vsub(pj, pi);
            let eik = vsub(pk, pi);
            let ejk = vsub(pk, pj);
            let cross = vcross(eij, eik);
            let two_area = vdot(cross, cross).sqrt();
            let cot_i = vdot(eij, eik) / two_area;
            let cot_j = vdot(vsub(pi, pj), ejk) / two_area;
            let cot_k = vdot(vsub(pi, pk), vsub(pj, pk)) / two_area;
            let half_s = 0.5 * s;
            let w_ij = half_s * cot_k;
            let w_jk = half_s * cot_i;
            let w_ki = half_s * cot_j;
            // Scatter each edge weight into the global D.
            let mut add_edge = |a: usize, b: usize, w: f64| {
                d[a][a] += w;
                d[b][b] += w;
                d[a][b] -= w;
                d[b][a] -= w;
            };
            add_edge(gi, gj, w_ij);
            add_edge(gj, gk, w_jk);
            add_edge(gk, gi, w_ki);
        }
        d
    }

    /// Max-norm of the ALL-node equilibrium residual ‖D·x‖∞ / (1+scale).
    /// For the free-standing case every node is free, so this checks the full
    /// combined-D null-space condition x∈null(D(x)) at the fixed point.
    #[allow(clippy::needless_range_loop)]
    fn free_equilibrium_residual_scaled(d: &[Vec<f64>], nodes: &[[f64; 3]]) -> f64 {
        let n = nodes.len();
        let mut resid = 0.0_f64;
        let mut scale = 0.0_f64;
        for i in 0..n {
            for axis in 0..3 {
                let mut net = 0.0;
                for j in 0..n {
                    net += d[i][j] * nodes[j][axis];
                }
                resid = resid.max(net.abs());
            }
        }
        for p in nodes {
            for c in p {
                scale = scale.max(c.abs());
            }
        }
        resid / (1.0 + scale)
    }

    /// Combined golden: triplex (3 struts + 9 cables) + isotropic membrane σ=0.2
    /// on the top {0,1,2} and bottom {3,4,5} triangles. These are exactly
    /// equilateral at the canonical prism, so the cotangent-Laplacian adds a
    /// uniform weight w = σ/(2√3) to each horizontal edge — identical to boosting
    /// the horizontal cable densities. Combined D has nullity 4 for any modest σ>0
    /// (scale-invariant closed form), so the search finds it and the cotangent
    /// fixed point converges with combined-D equilibrium residual ~machine-zero.
    #[test]
    fn surfaces_free_combined_prism_membrane_golden() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        let surfaces = prism_surfaces();
        let sigma = 0.2_f64;
        let sigmas = vec![sigma; 2];

        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
            .expect("combined prism+membrane must form-find");

        // Basic convergence + spectrum.
        assert!(result.converged, "combined solve must converge");
        assert_eq!(result.nullity, 4, "combined D must have nullity 4");

        // surface_stresses echoes σ per triangle (never empty here).
        assert_eq!(
            result.surface_stresses.len(),
            2,
            "one surface_stress echo per triangle",
        );
        for (t, &s) in result.surface_stresses.iter().enumerate() {
            assert!(
                (s - sigma).abs() < 1e-12,
                "surface_stresses[{t}] = {s}, expected σ = {sigma}",
            );
        }

        // Force signs: struts compressive, cables tensile.
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

        // Primary honest signal: combined free-node equilibrium residual at the
        // SOLVED geometry, assembled INDEPENDENTLY (faer-free, via reassemble_d_free).
        let d_combined = reassemble_d_free(
            6,
            &members,
            &result.force_densities,
            &surfaces,
            &sigmas,
            &result.nodes,
        );
        let resid = free_equilibrium_residual_scaled(&d_combined, &result.nodes);
        assert!(
            resid < 1e-9,
            "combined equilibrium residual ‖D(x)·x‖∞/(1+scale) = {resid:.3e}, expected < 1e-9",
        );
    }

    /// Unit-level companion to the integration file's `Explicit` combined
    /// coverage (task 6537): `Explicit` combined free-standing form-finding,
    /// from the perturbed guess, must converge to the closed-form combined
    /// self-stress. Verified through the module's own faer-free helpers
    /// (`reassemble_d_free` / `free_equilibrium_residual_scaled`) — an
    /// independently-coded assembly path distinct from the integration
    /// file's `reassemble_d_combined` / `free_residual_scaled`, so a scatter
    /// bug shared between the kernel and one verification path still
    /// surfaces here even though the integration file's regression table
    /// (`combined_explicit_analytic_q_converges_across_sigma_and_perturbation`)
    /// now also covers σ=1.0. Shape (equilateral-triangle) and
    /// `surface_stresses`-echo assertions live only in that table, to avoid
    /// a third near-verbatim copy of the same assertion block.
    #[test]
    fn surfaces_free_explicit_combined_q_converges_from_perturbed_guess() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        let surfaces = prism_surfaces();
        let sigma = 1.0_f64;
        let sigmas = vec![sigma; 2];
        let q = closed_form_combined_q(sigma);
        let spec = ForceDensitySpec::Explicit(q);

        let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
            .expect("combined explicit q from perturbed guess must form-find");

        assert!(result.converged, "combined solve must converge");
        assert_eq!(result.nullity, 4, "combined D must have nullity 4");

        // Primary honest signal: combined free-node equilibrium residual at
        // the SOLVED geometry, assembled INDEPENDENTLY (faer-free).
        let d_combined = reassemble_d_free(
            6,
            &members,
            &result.force_densities,
            &surfaces,
            &sigmas,
            &result.nodes,
        );
        let resid = free_equilibrium_residual_scaled(&d_combined, &result.nodes);
        assert!(
            resid < 1e-9,
            "combined equilibrium residual ‖D(x)·x‖∞/(1+scale) = {resid:.3e}, expected < 1e-9",
        );
    }

    // A degenerate (collinear / zero-area) INPUT surface triangle is an input
    // error, reported instead of assembling a NaN/∞ stencil.
    #[test]
    fn surfaces_free_degenerate_triangle_is_degenerate_triangle() {
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess();
        // Collinear triangle: all three corners on the x-axis → zero area.
        let degenerate_surfaces = vec![(0, 1, 2)]; // nodes 0,1,2 will be collinear
        // Override the guess so nodes 0,1,2 are collinear on the x-axis.
        let mut linear_guess = guess.clone();
        linear_guess[0] = [0.0, 0.0, 0.0];
        linear_guess[1] = [1.0, 0.0, 0.0];
        linear_guess[2] = [2.0, 0.0, 0.0];
        linear_guess[3] = [0.5, 1.0, 0.0];
        linear_guess[4] = [1.5, 1.0, 0.0];
        linear_guess[5] = [1.0, 2.0, 0.0];

        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        assert_eq!(
            form_find_free_surfaces(
                &linear_guess,
                &members,
                &kinds,
                &degenerate_surfaces,
                &[0.2],
                &spec,
            )
            .unwrap_err(),
            FreeFormError::Input(ForceDensityInputError::DegenerateTriangle { surface: 0 }),
        );
    }

    // ── gauge lock (task 6413): all_node_equilibrium_residual_relative must be
    // GAUGE-independent — the free-standing twin of task 6119's anchored-path
    // fix. D_combined = CᵀQC + Σ_T σ_T·L_T is exactly linear in q and σ, so an
    // absolute stop test on the raw residual makes convergence depend on the
    // overall force-density/stress scale, not just the physical equilibrium.

    /// `D_combined = CᵀQC + Σ_T σ_T·L_T` at the canonical (symmetric) prism
    /// geometry, for the closed-form line q and the top+bottom membrane
    /// triangles at `sigma`, both scaled by `lambda` — the uniform gauge
    /// change this task locks convergence against.
    fn combined_d_at_canonical_prism(sigma: f64, lambda: f64) -> Mat<f64> {
        let (members, _kinds) = triplex_topology();
        let q: Vec<f64> = closed_form_q().iter().map(|v| v * lambda).collect();
        let surfaces = prism_surfaces();
        let sigmas = vec![sigma * lambda; surfaces.len()];
        let nodes = canonical_prism();
        let s = assemble_surface_matrix(6, &surfaces, &sigmas, &nodes)
            .expect("canonical prism triangles are non-degenerate");
        combined_force_density_matrix(&members, &q, &s)
    }

    #[test]
    fn all_node_equilibrium_residual_is_gauge_invariant_under_uniform_scaling() {
        // D_combined is exactly linear in q and σ, so a uniform gauge change
        // q → λ·q, σ → λ·σ at fixed geometry scales D_combined by λ entrywise.
        // λ = 2^20 / 2^-20 are powers of two, so the scaling is exact in
        // IEEE-754 (an exponent-field shift, no mantissa rounding) and every
        // downstream sum/max/divide commutes with it exactly — checked with
        // assert_eq! on the raw f64, not a tolerance (mirrors task 6119's
        // `free_equilibrium_residual_is_invariant_under_uniform_force_density_scaling`
        // in `form_find.rs` on the unmerged `task/6119` branch).
        //
        // MEASURED RED on pristine, this exact fixture: the un-normalised
        // residual scales BY λ instead of staying fixed —
        // 8.660254037844406e-2 vs 9.080934537986736e4 (ratio exactly 2^20).
        const LAMBDA_UP: f64 = 1_048_576.0; // 2^20
        const LAMBDA_DOWN: f64 = 1.0 / 1_048_576.0; // 2^-20
        let nodes = canonical_prism();
        let d1 = combined_d_at_canonical_prism(0.2, 1.0);
        let d_up = combined_d_at_canonical_prism(0.2, LAMBDA_UP);
        let d_down = combined_d_at_canonical_prism(0.2, LAMBDA_DOWN);

        let r1 = all_node_equilibrium_residual_relative(&d1, &nodes);
        let r_up = all_node_equilibrium_residual_relative(&d_up, &nodes);
        let r_down = all_node_equilibrium_residual_relative(&d_down, &nodes);

        assert_eq!(
            r1, r_up,
            "residual must be exactly gauge-invariant: base={r1:e} λ=2^20-scaled={r_up:e}",
        );
        assert_eq!(
            r1, r_down,
            "residual must be exactly gauge-invariant: base={r1:e} λ=2^-20-scaled={r_down:e}",
        );
    }

    #[test]
    fn all_node_equilibrium_residual_of_all_zero_d_is_infinity_not_vacuous_zero() {
        // A node block touched by neither a member nor a triangle makes D
        // identically zero, so the raw numerator ‖D·x‖∞ is vacuously 0 — not
        // because equilibrium was reached, but because nothing acts on the
        // node at all. Returning 0.0 (the pristine behaviour) would break the
        // outer fixed-point loop out at iteration 0 and echo the caller's
        // unsolved guess back as "converged" — the same hole task 6119 closed
        // on the anchored path. Must return f64::INFINITY instead.
        //
        // MEASURED RED on pristine: returns 0.0.
        let nodes = canonical_prism();
        let d = Mat::<f64>::zeros(6, 6);
        let resid = all_node_equilibrium_residual_relative(&d, &nodes);
        assert_eq!(
            resid,
            f64::INFINITY,
            "all-zero D must report INFINITY (vacuous, not converged), got {resid}",
        );
    }

    // ── per-row criterion (task 7801): each node's net force is judged against
    // its OWN row of D, so a healthy row can neither mask a degenerate one nor
    // lend its stiffness to a soft one.

    #[test]
    fn all_node_equilibrium_residual_rejects_a_zero_row_beside_healthy_rows() {
        let d = with_isolated_node(&combined_d_at_canonical_prism(0.2, 1.0));
        let mut nodes = canonical_prism();
        nodes.push([0.3, 0.2, 0.4]);

        let resid = all_node_equilibrium_residual_relative(&d, &nodes);
        assert_eq!(
            resid,
            f64::INFINITY,
            "an isolated node's zero row must reject, got {resid}",
        );
    }

    #[test]
    fn all_node_equilibrium_residual_rejects_a_nan_row_beside_healthy_rows() {
        // Every other row is healthy, so only the NaN branch can reject.
        let nodes = vec![[1.0, 1.0, 1.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let mut d = Mat::<f64>::zeros(3, 3);
        d[(0, 1)] = 8.5;
        d[(1, 0)] = 8.5;
        d[(2, 0)] = f64::NAN;

        let resid = all_node_equilibrium_residual_relative(&d, &nodes);
        assert_eq!(
            resid,
            f64::INFINITY,
            "a NaN-carrying row must reject beside healthy rows, got {resid}",
        );
    }

    #[test]
    fn all_node_equilibrium_residual_judges_each_row_against_its_own_stiffness() {
        // Chain 0 —soft (q = 1)— 1 —stiff (q = 2^20)— 2. The stiff member is
        // balanced (nodes 1 and 2 coincide); the soft one is 2^-20 off, which
        // its own row norm (2) sees but the stiff row norm (2 + 2^21) hides.
        const SOFT: f64 = 1.0;
        const STIFF: f64 = 1_048_576.0; // 2^20
        const OFFSET: f64 = 1.0 / 1_048_576.0; // 2^-20
        let d = assemble_d(3, &[(0, 1), (1, 2)], &[SOFT, STIFF]);
        let nodes = vec![[1.0 + OFFSET, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]];

        let resid = all_node_equilibrium_residual_relative(&d, &nodes);
        assert!(
            resid > FREE_SURFACE_EQUILIBRIUM_REL_TOL,
            "the soft row's imbalance must not read as converged: residual {resid:e} \
             vs tolerance {FREE_SURFACE_EQUILIBRIUM_REL_TOL:e}",
        );
    }

    // ── gauge lock (task 6413), continued — defect D2: the eigenvalue-gap
    // DESCENT STEP. Step-2 (above) fixes only the outer stop test; it does
    // NOT touch `combined_eig_gap_objective` / `combined_geometry_descent_step`,
    // which is why this task is not a straight port of task 6119's diff. Unit
    // altitude only: MEASURED, there is no e2e fixture that both converges
    // AND exercises the descent step under a gauge change (the
    // `ForceDensitySpec::Explicit` combined mode never converges from any
    // meaningful geometry perturbation), so claiming an e2e RED here would be
    // a doomed test — see `tensegrity_free_surface_gauge_covariance.rs` for
    // the e2e coverage this task DOES carry (the stop test only).

    #[test]
    fn combined_eig_gap_objective_is_gauge_invariant_under_uniform_scaling() {
        // The objective is a pure scalar SCORE (sum of squares of the 4
        // smallest-|λ| eigenvalues of D_combined), so its gauge factor must
        // cancel — this is the right contract for a score, distinct from the
        // descent step below (which maps geometry to geometry, and geometry
        // carries no gauge weight, so ITS contract is bit-identical output,
        // not a cancelled scale factor). D_combined scales bit-exactly by λ
        // under q → λ·q, σ → λ·σ (established by the D1 tests above), so
        // every eigenvalue of D scales by λ and the sum-of-4-smallest-squares
        // scales by λ². The pristine (un-normalised) objective is therefore
        // NOT gauge-invariant — checked with assert_eq! (bit-exact, λ a power
        // of two) rather than a tolerance.
        //
        // MEASURED RED after step-2, this exact fixture (step-2 does not
        // touch this function): 6.000000000000011e-2 vs 6.597069766656012e10
        // — ratio ≈ 1.0995e12 ≈ 2^40 = λ², not the expected 1.
        const LAMBDA_UP: f64 = 1_048_576.0; // 2^20
        const LAMBDA_DOWN: f64 = 1.0 / 1_048_576.0; // 2^-20
        let (members, _kinds) = triplex_topology();
        let surfaces = prism_surfaces();
        let x = canonical_prism();
        let q = closed_form_q();
        const SIGMA: f64 = 0.2;

        let obj_at = |lambda: f64| -> f64 {
            let q_scaled: Vec<f64> = q.iter().map(|v| v * lambda).collect();
            let sigmas_scaled = vec![SIGMA * lambda; surfaces.len()];
            combined_eig_gap_objective(6, &members, &q_scaled, &surfaces, &sigmas_scaled, &x)
        };

        let o1 = obj_at(1.0);
        let o_up = obj_at(LAMBDA_UP);
        let o_down = obj_at(LAMBDA_DOWN);

        assert_eq!(
            o1, o_up,
            "objective must be exactly gauge-invariant: base={o1:e} λ=2^20-scaled={o_up:e}",
        );
        assert_eq!(
            o1, o_down,
            "objective must be exactly gauge-invariant: base={o1:e} λ=2^-20-scaled={o_down:e}",
        );
    }

    #[test]
    fn combined_geometry_descent_step_output_is_gauge_invariant_under_uniform_scaling() {
        // The descent step maps GEOMETRY to GEOMETRY (plus a next step
        // size), and geometry carries no gauge weight (only q/σ do) — so,
        // unlike the scalar objective above, the right contract here is that
        // the OUTPUT is bit-identical across a gauge change, not scaled by
        // any power of λ. This currently fails because the finite-difference
        // gradient is taken of the un-normalised (gauge-covariant, scaling as
        // λ²) objective, so a fixed backtracking `step` accepts/rejects
        // different trial geometries depending on λ.
        //
        // MEASURED RED after step-2 (step-2 does not touch this function):
        // both the returned geometry and the returned next step size differ
        // across λ.
        const LAMBDA_UP: f64 = 1_048_576.0; // 2^20
        let (members, _kinds) = triplex_topology();
        let surfaces = prism_surfaces();
        let x = canonical_prism();
        let q = closed_form_q();
        const SIGMA: f64 = 0.2;
        const STEP_IN: f64 = 1e-2; // matches the outer loop's initial geo_step

        let step_at = |lambda: f64| -> (Vec<[f64; 3]>, f64) {
            let q_scaled: Vec<f64> = q.iter().map(|v| v * lambda).collect();
            let sigmas_scaled = vec![SIGMA * lambda; surfaces.len()];
            combined_geometry_descent_step(
                6,
                &members,
                &q_scaled,
                &surfaces,
                &sigmas_scaled,
                &x,
                STEP_IN,
            )
        };

        let (next1, nstep1) = step_at(1.0);
        let (next_up, nstep_up) = step_at(LAMBDA_UP);

        assert_eq!(
            next1, next_up,
            "descent-step geometry must be exactly gauge-invariant (no gauge weight on geometry)",
        );
        assert_eq!(
            nstep1, nstep_up,
            "descent-step next step size must be exactly gauge-invariant: step(1)={nstep1:e} step(λ)={nstep_up:e}",
        );
    }

    #[test]
    fn combined_eig_gap_objective_of_all_zero_d_is_zero_not_infinity() {
        // The sibling guard on the residual
        // (all_node_equilibrium_residual_of_all_zero_d_is_infinity_not_vacuous_zero)
        // returns INFINITY because the residual IS the outer loop's
        // convergence gate. This objective is NOT a convergence gate — only
        // the descent step's line-search score — so an identically-zero D
        // (no spectral gap left to close) is honestly a perfect score, and
        // the guarded branch must return 0.0: returning INFINITY here would
        // instead make the line search reject every trial.
        let (members, _kinds) = triplex_topology();
        let surfaces = prism_surfaces();
        let x = canonical_prism();
        let zero_q = vec![0.0; members.len()];
        let zero_sigmas = vec![0.0; surfaces.len()];

        let obj = combined_eig_gap_objective(6, &members, &zero_q, &surfaces, &zero_sigmas, &x);
        assert_eq!(obj, 0.0, "all-zero D must score a perfect 0.0, got {obj}");

        // With g0 == 0.0 the finite-difference gradient is also all-zero (D
        // stays identically zero at every FD-perturbed geometry too, since
        // both q and sigma are zero regardless of x), so the backtracking
        // search can find no strictly-downhill trial and must return the
        // geometry unchanged — pinning the claim that a premature "perfect"
        // score cannot masquerade as progress.
        let (next, _next_step) =
            combined_geometry_descent_step(6, &members, &zero_q, &surfaces, &zero_sigmas, &x, 1e-2);
        assert_eq!(
            next, x,
            "descent step must leave geometry unchanged when the objective is already 0.0",
        );
    }
}
