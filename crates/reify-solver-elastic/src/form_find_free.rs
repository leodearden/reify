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

use crate::fd_criteria::{RowSet, StiffnessScale};
use crate::fd_matrix::{NULLITY_REL_TOL, SpectrumError, SymmetricSpectrum, assemble_d};
use crate::form_find::{
    ForceDensityInputError, ForceDensityProblem, FreeSpec, IsotropicStencil, MemberKind,
    ZeroAreaTriangle, member_axial_forces, scatter_surface_stencils,
};
use crate::group_spec::{GroupSpec, MIN_UNIT_MAGNITUDE};
use faer::Mat;
use faer::mat::MatRef;

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
    let free = free_spec(spec, members.len())?;
    let problem = ForceDensityProblem::new(nodes_guess, members, kinds, free, &[], &[], &[])?;
    form_find_line_only(&problem)
}

/// The densities `spec` leaves free to vary, validated: an explicit `q` as
/// given, or a [`ForceDensitySpec::GroupRatios`] spec that can seed the search.
fn free_spec(
    spec: &ForceDensitySpec,
    n_members: usize,
) -> Result<FreeSpec<'_>, ForceDensityInputError> {
    match spec {
        ForceDensitySpec::Explicit(q) => Ok(FreeSpec::Explicit(q)),
        ForceDensitySpec::GroupRatios {
            group_ids,
            seed_ratios,
            reference_group,
        } => Ok(FreeSpec::GroupRatios(GroupSpec::new(
            group_ids,
            seed_ratios,
            *reference_group,
            n_members,
        )?)),
    }
}

/// The line-only free-standing solve of a validated problem; its surfaces, if
/// any, do not enter `D`.
fn form_find_line_only(problem: &ForceDensityProblem<'_>) -> Result<FreeFormResult, FreeFormError> {
    match problem.spec() {
        // Deterministic foundation: assemble D, classify nullity, recover
        // coordinates, compute forces.
        FreeSpec::Explicit(_) => form_find_explicit(problem),
        // Adaptive eigenvalue-minimisation search over relative group densities;
        // on success it produces an admissible q and delegates to the explicit
        // path.
        FreeSpec::GroupRatios(groups) => form_find_group_ratios(problem, groups),
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
    // against the caller's own geometry — before the combined fixed point's
    // bootstrap moves it to a (non-degenerate) line-only equilibrium.
    let problem = ForceDensityProblem::new(
        nodes_guess,
        members,
        kinds,
        free_spec(spec, members.len())?,
        &[],
        surfaces,
        surface_stresses,
    )?;

    // Empty surfaces delegate to the line-only path with an empty echo.
    if surfaces.is_empty() {
        let mut result = form_find_line_only(&problem)?;
        result.surface_stresses = Vec::new();
        return Ok(result);
    }

    certify_combined_stop(&problem, combined_fixed_point(&problem)?)
}

/// Where the combined fixed point stopped.
enum CombinedStop {
    /// An iterate met the stop test.
    Equilibrium(CombinedIterate),
    /// The budget or the stall guard ended the loop: the densities of the
    /// last iteration whose relax step was kept, with the geometry it moved to
    /// (the problem's own densities at the starting geometry until one is).
    ShortOfEquilibrium { q: Vec<f64>, nodes: Vec<[f64; 3]> },
}

/// The combined cotangent fixed point: at each iterate assemble the surface
/// term, take the densities there and stop at equilibrium, else relax the
/// geometry, until the budget or the stall guard ends it.
///
/// It starts from the line-only equilibrium rather than the caller's guess: a
/// perturbed guess breaks the 3-fold symmetry of the surface term, leaving the
/// combined `D` at nullity 2, while the line-only form is the symmetric prism,
/// where the combined `D` can reach nullity 4. When the line-only solve fails
/// (e.g. an Explicit `q` with no line-only equilibrium) it starts from the
/// guess.
fn combined_fixed_point(problem: &ForceDensityProblem<'_>) -> Result<CombinedStop, FreeFormError> {
    let boot = form_find_line_only(problem).ok();
    let mut q_source = QSource::new(problem, boot.as_ref());
    let mut current = boot.map_or_else(|| problem.nodes().to_vec(), |boot| boot.nodes);
    let mut relaxed_q = problem.q().to_vec();
    let mut relax = RelaxStep::new();
    let mut stall = StallGuard::new();

    for _ in 0..MAX_FREE_SURFACE_ITERS {
        let surface_mat = assemble_surface_matrix(
            current.len(),
            problem.surfaces(),
            problem.surface_stresses(),
            &current,
        )?;
        let q = q_source.densities_at(problem, &current, &surface_mat)?;
        let iterate = CombinedIterate::new(problem, q, current, surface_mat);
        if iterate.is_equilibrium() {
            return Ok(CombinedStop::Equilibrium(iterate));
        }
        let next = relax.relax(problem, &iterate, q_source.admits_projection_trial());
        if stall.stalled_after(&iterate.nodes, &next, iterate.residual) {
            return Ok(CombinedStop::ShortOfEquilibrium {
                q: relaxed_q,
                nodes: iterate.nodes,
            });
        }
        relaxed_q = iterate.q;
        current = next;
    }
    Ok(CombinedStop::ShortOfEquilibrium {
        q: relaxed_q,
        nodes: current,
    })
}

/// The result `stop` certifies to. At equilibrium the iterate's own `D` must
/// certify as a form. Short of equilibrium it is always an error, and
/// [`FreeFormError::SearchDidNotConverge`] names the stopped pair's
/// [`FormDefect`] when its `D` is not a form, so a model that is not one
/// connected nullity-4 form names its defect even when its residual never
/// settled (an isolated node's zero row keeps it unsettled). It is never `Ok`
/// with `converged = false`, which a consumer reading only the value would
/// take for an equilibrium.
fn certify_combined_stop(
    problem: &ForceDensityProblem<'_>,
    stop: CombinedStop,
) -> Result<FreeFormResult, FreeFormError> {
    match stop {
        CombinedStop::Equilibrium(iterate) => {
            let nullity = iterate.certify()?.nullity();
            Ok(iterate.into_result(problem, nullity))
        }
        CombinedStop::ShortOfEquilibrium { q, nodes } => {
            let surface_mat = assemble_surface_matrix(
                nodes.len(),
                problem.surfaces(),
                problem.surface_stresses(),
                &nodes,
            )?;
            let d = combined_force_density_matrix(problem.members(), &q, &surface_mat);
            Err(match FreeStandingForm::certify(&d) {
                Ok(_) => FreeFormError::SearchDidNotConverge { defect: None },
                Err(FreeFormError::NotAForm(defect)) => FreeFormError::SearchDidNotConverge {
                    defect: Some(defect),
                },
                Err(other) => other,
            })
        }
    }
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

/// Trips once [`GEO_STALL_ITERS`] consecutive iterations each left the
/// geometry where it ENTERED the iteration and shrank the residual by less
/// than [`GEO_STALL_RESID_THRESHOLD`]. `entering` is the geometry before the
/// relax step, so a projection trial the relax step accepted counts as
/// movement even when the descent half then stalled.
struct StallGuard {
    previous_residual: f64,
    stuck_iterations: usize,
}

impl StallGuard {
    /// A guard with no history. Its `+∞` previous residual makes any finite
    /// first residual an improvement and an infinite one flat.
    fn new() -> Self {
        StallGuard {
            previous_residual: f64::INFINITY,
            stuck_iterations: 0,
        }
    }

    /// Records one iteration; true once the guard has tripped.
    fn stalled_after(
        &mut self,
        entering: &[[f64; 3]],
        leaving: &[[f64; 3]],
        residual: f64,
    ) -> bool {
        let unmoved = leaving == entering;
        let flat = residual >= self.previous_residual * (1.0 - GEO_STALL_RESID_THRESHOLD);
        self.stuck_iterations = if unmoved && flat {
            self.stuck_iterations + 1
        } else {
            0
        };
        self.previous_residual = residual;
        self.stuck_iterations >= GEO_STALL_ITERS
    }
}

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
/// the [`StiffnessScale`] criterion over [`RowSet::All`], since every node is
/// free here. That one definition carries the per-row normalisation (exactly
/// gauge-invariant) and the zero/NaN-row rejection.
fn all_node_equilibrium_residual_relative(d: &Mat<f64>, nodes: &[[f64; 3]]) -> f64 {
    StiffnessScale::of(d, RowSet::All).relative_residual(nodes)
}

/// One outer iterate of the combined fixed point: densities `q` at geometry
/// `nodes`, the surface term there, their combined `D` and its all-node
/// equilibrium residual.
struct CombinedIterate {
    q: Vec<f64>,
    nodes: Vec<[f64; 3]>,
    surface_mat: Mat<f64>,
    d: Mat<f64>,
    residual: f64,
}

impl CombinedIterate {
    fn new(
        problem: &ForceDensityProblem<'_>,
        q: Vec<f64>,
        nodes: Vec<[f64; 3]>,
        surface_mat: Mat<f64>,
    ) -> Self {
        let d = combined_force_density_matrix(problem.members(), &q, &surface_mat);
        let residual = all_node_equilibrium_residual_relative(&d, &nodes);
        CombinedIterate {
            q,
            nodes,
            surface_mat,
            d,
            residual,
        }
    }

    /// Whether the residual meets the fixed point's stop test.
    fn is_equilibrium(&self) -> bool {
        self.residual <= FREE_SURFACE_EQUILIBRIUM_REL_TOL
    }

    /// The residual of `trial_nodes` at this iterate's densities, given the
    /// surface term assembled there.
    fn residual_at(
        &self,
        problem: &ForceDensityProblem<'_>,
        trial_nodes: &[[f64; 3]],
        trial_surface_mat: &Mat<f64>,
    ) -> f64 {
        let d = combined_force_density_matrix(problem.members(), &self.q, trial_surface_mat);
        all_node_equilibrium_residual_relative(&d, trial_nodes)
    }

    fn certify(&self) -> Result<FreeStandingForm, FreeFormError> {
        FreeStandingForm::certify(&self.d)
    }

    /// This iterate as a converged result whose certified `D` has `nullity`.
    fn into_result(self, problem: &ForceDensityProblem<'_>, nullity: usize) -> FreeFormResult {
        FreeFormResult {
            member_forces: member_axial_forces(problem.members(), &self.q, &self.nodes),
            force_densities: self.q,
            nodes: self.nodes,
            nullity,
            converged: true,
            surface_stresses: problem.surface_stresses().to_vec(),
        }
    }
}

/// Where each outer iteration's densities come from: the one place the
/// combined fixed point branches on the force-density spec.
enum QSource<'p> {
    /// The explicit `q`, the same at every iterate.
    FixedQ(&'p [f64]),
    /// A group search on the combined `D` at each iterate, warm-started from
    /// the magnitudes the previous search found. `warm_group_mag` is in units
    /// of the reference group's seed magnitude.
    SearchedQ {
        groups: &'p GroupSpec,
        warm_group_mag: Vec<f64>,
    },
}

impl<'p> QSource<'p> {
    /// The source `problem`'s spec names. A searched source starts from the
    /// line-only bootstrap's magnitudes when `boot` solved, not from the seeds:
    /// at seeds (-1, 1, 1) the Z-equilibrium constraint (q_strut + q_vert = 0)
    /// lands each free group's 1-D minimum back at its seed, so coordinate
    /// descent stalls before the combined objective drives the nullity to 4,
    /// while the bootstrap's q* ≈ (-√3, 1, +√3) lies in the basin of the
    /// combined target (-√3·(1+w), 1, +√3·(1+w)).
    fn new(problem: &'p ForceDensityProblem<'_>, boot: Option<&FreeFormResult>) -> Self {
        match problem.spec() {
            FreeSpec::Explicit(q) => QSource::FixedQ(q),
            FreeSpec::GroupRatios(groups) => {
                let mut warm_group_mag = groups.unit_seed_magnitudes();
                if let Some(boot) = boot {
                    groups.refresh_unit_magnitudes(&mut warm_group_mag, &boot.force_densities);
                }
                QSource::SearchedQ {
                    groups,
                    warm_group_mag,
                }
            }
        }
    }

    /// The densities at the iterate `nodes`, whose surface term is
    /// `surface_mat`.
    fn densities_at(
        &mut self,
        problem: &ForceDensityProblem<'_>,
        nodes: &[[f64; 3]],
        surface_mat: &Mat<f64>,
    ) -> Result<Vec<f64>, FreeFormError> {
        match self {
            QSource::FixedQ(q) => Ok(q.to_vec()),
            QSource::SearchedQ {
                groups,
                warm_group_mag,
            } => {
                let q = form_find_group_ratios_combined(
                    problem,
                    groups,
                    nodes,
                    warm_group_mag,
                    surface_mat,
                )?;
                groups.refresh_unit_magnitudes(warm_group_mag, &q);
                Ok(q)
            }
        }
    }

    /// Whether the relax step may first try projecting the iterate into the
    /// null space of its `D`. Only a fixed `q` admits it: a searched source
    /// keeps only the densities its search found and discards the geometry
    /// that search projected.
    fn admits_projection_trial(&self) -> bool {
        matches!(self, QSource::FixedQ(_))
    }
}

/// The geometry move of one outer iteration at the iterate's fixed `q`: an
/// optional [`RelaxStep::projection_trial`], then one [`RelaxStep::descend`]
/// step on the eigenvalue-gap objective [`RelaxStep::eig_gap`] from wherever
/// the trial left the geometry. Projection moves `x`
/// into null₄(D); descent drives `D` toward nullity 4 without moving `x` into
/// null(D); the fixed point needs `x ∈ null(D(x))`. The densities alone cannot
/// get there: only the symmetric realisation lets the geometry-dependent
/// membrane term bring the combined `D` to nullity 4, and a force group shares
/// one magnitude across its members, so it cannot cancel the per-edge
/// cotangent asymmetry. The geometry must move.
struct RelaxStep {
    descent_step: f64,
}

impl RelaxStep {
    const INITIAL_DESCENT_STEP: f64 = 1e-2;

    fn new() -> Self {
        RelaxStep {
            descent_step: Self::INITIAL_DESCENT_STEP,
        }
    }

    /// The geometry leaving the iteration. The descent step adapts (grows on
    /// an accepted step, shrinks on backtracks) and carries to the next call.
    fn relax(
        &mut self,
        problem: &ForceDensityProblem<'_>,
        iterate: &CombinedIterate,
        try_projection: bool,
    ) -> Vec<[f64; 3]> {
        let projected = if try_projection {
            Self::projection_trial(problem, iterate)
        } else {
            None
        };
        let start = projected.as_deref().unwrap_or(iterate.nodes.as_slice());
        self.descend(problem, &iterate.q, start)
    }

    /// The iterate projected onto the [`FREE_FORM_NULLITY`] smallest-|λ|
    /// eigenvectors of its `D`, kept only if it recovers a 3-D geometry whose
    /// surface term assembles and whose residual at the iterate's `q` does not
    /// exceed the iterate's own. A failed trial is `None`, never an error: an
    /// intermediate iterate's [`FreeFormError::SingularRecovery`] or
    /// [`FreeFormError::DegenerateTriangle`] must not replace the contractual
    /// [`FreeFormError::SearchDidNotConverge`]. The residual guard keeps a
    /// regressing trial from becoming the descent's start.
    fn projection_trial(
        problem: &ForceDensityProblem<'_>,
        iterate: &CombinedIterate,
    ) -> Option<Vec<[f64; 3]>> {
        let trial = form_find_explicit_combined_relaxed(
            problem.members(),
            &iterate.q,
            &iterate.nodes,
            &iterate.surface_mat,
        )
        .ok()?;
        let trial_surface_mat = assemble_surface_matrix(
            trial.len(),
            problem.surfaces(),
            problem.surface_stresses(),
            &trial,
        )
        .ok()?;
        (iterate.residual_at(problem, &trial, &trial_surface_mat) <= iterate.residual)
            .then_some(trial)
    }

    /// One backtracking gradient-descent step on [`RelaxStep::eig_gap`] over
    /// the node geometry at fixed force densities `q`, from `start`. Returns
    /// the stepped geometry; the descent step grows on the accepted step and
    /// shrinks on each rejection, and carries to the next call. If no
    /// downhill step is found within the backtracking budget the geometry is
    /// returned unchanged (the outer loop then either has already converged
    /// or stops).
    ///
    /// The gradient is a central finite difference over the `3n` coordinates.
    fn descend(
        &mut self,
        problem: &ForceDensityProblem<'_>,
        q: &[f64],
        start: &[[f64; 3]],
    ) -> Vec<[f64; 3]> {
        let n = start.len();
        let obj = |x: &[[f64; 3]]| Self::eig_gap(problem, q, x);
        let g0 = obj(start);

        // Central finite-difference gradient over the 3n coordinates.
        const FD_H: f64 = 1e-6;
        let mut grad = vec![[0.0_f64; 3]; n];
        for r in 0..n {
            for a in 0..3 {
                let mut xp = start.to_vec();
                xp[r][a] += FD_H;
                let mut xm = start.to_vec();
                xm[r][a] -= FD_H;
                grad[r][a] = (obj(&xp) - obj(&xm)) / (2.0 * FD_H);
            }
        }

        // Backtracking line search: accept the first step that strictly
        // decreases the objective, growing the step on success and halving it
        // on each rejection.
        const STEP_GROW: f64 = 1.3;
        const STEP_SHRINK: f64 = 0.5;
        const MAX_BACKTRACK: usize = 40;
        for _ in 0..MAX_BACKTRACK {
            let mut trial = start.to_vec();
            for r in 0..n {
                for a in 0..3 {
                    trial[r][a] -= self.descent_step * grad[r][a];
                }
            }
            if obj(&trial) < g0 {
                self.descent_step *= STEP_GROW;
                return trial;
            }
            self.descent_step *= STEP_SHRINK;
        }
        start.to_vec()
    }

    /// The descent objective: the sum of squares of the `FREE_FORM_NULLITY`
    /// smallest-|λ| eigenvalues of `D_combined = CᵀQC + Σ_T σ_T·L_T` at force
    /// densities `q` and geometry `nodes`, normalised via
    /// [`normalised_eig_gap`] so the objective is GAUGE-invariant: under a
    /// uniform `q → λ·q`, `σ → λ·σ` rescaling every eigenvalue of `D` scales
    /// by `λ`, so an un-normalised sum-of-squares would scale by `λ²` and drive
    /// [`RelaxStep::descend`]'s finite-difference gradient — and therefore its
    /// backtracking line search — to different accept/reject decisions
    /// depending on the force-density/stress scale.
    ///
    /// The line-only `D` is geometry-independent and rank-deficient by 4 for a
    /// whole *affine family* of realisations. The membrane cotangent weights DO
    /// depend on geometry, so the combined `D` only reaches nullity 4 at the
    /// specific symmetric realisation. Driving this objective to zero over the
    /// node coordinates selects that realisation. A degenerate trial geometry
    /// (zero-area triangle), or a combined `D` with no spectrum, returns `+∞`
    /// so the line search rejects it.
    fn eig_gap(problem: &ForceDensityProblem<'_>, q: &[f64], nodes: &[[f64; 3]]) -> f64 {
        match assemble_surface_matrix(
            nodes.len(),
            problem.surfaces(),
            problem.surface_stresses(),
            nodes,
        ) {
            Ok(surface_mat) => {
                let d = combined_force_density_matrix(problem.members(), q, &surface_mat);
                SymmetricSpectrum::of(&d)
                    .map_or(f64::INFINITY, |spectrum| normalised_eig_gap(&spectrum))
            }
            Err(_) => f64::INFINITY,
        }
    }
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
/// `q` are the iterate's densities and `nodes_guess` its geometry, which
/// gauge-fixes the recovery.
fn form_find_explicit_combined_relaxed(
    members: &[(usize, usize)],
    q: &[f64],
    nodes_guess: &[[f64; 3]],
    surface_mat: &Mat<f64>,
) -> Result<Vec<[f64; 3]>, FreeFormError> {
    let d = combined_force_density_matrix(members, q, surface_mat);
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
/// `initial_group_mag` are warm-start magnitudes in reference units (e.g. from a
/// line-only bootstrap) that replace the group spec's seed magnitudes as the starting point AND as the
/// centre of the search bracket. Starting near the correct combined q avoids the
/// stall-guard pitfall where coordinate descent at the wrong starting point can
/// reach a fixed point before the objective hits the nullity-4 minimum.
///
/// `problem` holds the validated seed densities, `groups` its group spec and
/// `nodes_guess` the current iterate.
fn form_find_group_ratios_combined(
    problem: &ForceDensityProblem<'_>,
    groups: &GroupSpec,
    nodes_guess: &[[f64; 3]],
    initial_group_mag: &[f64],
    surface_mat: &Mat<f64>,
) -> Result<Vec<f64>, FreeFormError> {
    let members = problem.members();

    // Warm-start: use the provided initial magnitudes (e.g. from the line-only
    // bootstrap) rather than the seed magnitudes. This ensures the search
    // starts near the correct combined q and avoids the coordinate-descent stall
    // at the original seeds that causes the stall guard to fire prematurely.
    let mut group_mag: Vec<f64> = initial_group_mag.to_vec();
    let free_groups = groups.free_groups();
    let unit = groups.reference_magnitude();
    let unit_surface = Mat::<f64>::from_fn(surface_mat.nrows(), surface_mat.ncols(), |i, j| {
        surface_mat[(i, j)] / unit
    });

    // Objective: Σ λ² over the FREE_FORM_NULLITY smallest eigenvalues of the
    // COMBINED D = line_D(unit_densities) + unit_surface, where unit_surface is
    // a fixed additive term. Un-normalised, unlike RelaxStep::eig_gap's
    // normalised_eig_gap: the search runs in units of the reference group's
    // seed magnitude (both terms of D divided by it), so its objective,
    // OBJ_TOL, stall guard, log grid and floors are dimensionless, and its
    // verdict does not depend on the overall (seed, σ) scale. A `D` with no spectrum scores +∞, rejecting the
    // trial.
    let objective = |group_mag: &[f64]| -> f64 {
        let d = combined_force_density_matrix(
            members,
            &groups.unit_densities(group_mag),
            &unit_surface,
        );
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
                    trial[g] = (baseline[g] * s).max(MIN_UNIT_MAGNITUDE);
                }
                objective(&trial)
            },
            1.0 / SEARCH_BRACKET_FACTOR,
            SEARCH_BRACKET_FACTOR,
        );
        for &g in &free_groups {
            group_mag[g] = (group_mag[g] * best_s).max(MIN_UNIT_MAGNITUDE);
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
            // Bracket centred on the current (warm) magnitude — not on the seed.
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

    let q = groups.densities(&group_mag);

    // The outer loop moves the geometry itself, so the recovered nodes are not
    // returned; a found `q` whose forced-nullity recovery cannot span 3-D is
    // still this search's error.
    let checked = problem.with_densities(&q)?;
    form_find_explicit_combined_relaxed(checked.members(), checked.q(), nodes_guess, surface_mat)?;
    Ok(q)
}

/// Sum of `(λ/max_mag)²` over the `FREE_FORM_NULLITY` smallest-|λ|
/// eigenvalues of `spectrum` — the gauge-invariant score
/// [`RelaxStep::eig_gap`] reports. Dividing by its `max_mag` (which
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
/// search, which keeps the un-normalised `Σλ²` form: it runs in units of the
/// reference group's seed magnitude, so that form is already gauge-free.
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
/// of `q` is nullity-invariant, so only relative ratios matter), and the whole
/// search runs in units of that seed magnitude, so a whole-gauge rescale of the
/// seeds leaves its path unchanged.
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
    groups: &GroupSpec,
) -> Result<FreeFormResult, FreeFormError> {
    let members = problem.members();
    let n = problem.nodes().len();

    // Current per-group magnitude in reference units (each group's sign is
    // fixed by its seed). The reference group's magnitude stays at its seed;
    // free groups start there and are refined by the search.
    let unit_seed = groups.unit_seed_magnitudes();
    let mut group_mag = unit_seed.clone();
    let free_groups = groups.free_groups();

    // Objective: Σ λ² over the FREE_FORM_NULLITY smallest-|λ| eigenvalues of
    // `D(group_mag)`. [`SymmetricSpectrum`] orders eigenvalues ascending by |λ|,
    // so `take(4)` is exactly those. The smooth sum-of-squares (rather than the
    // bare 4th eigenvalue, which is V-shaped/kinked) keeps coordinate descent
    // from stalling at the tolerance crossing. A `D` with no spectrum scores
    // +∞, rejecting the trial.
    let objective = |group_mag: &[f64]| -> f64 {
        let d = assemble_d(n, members, &groups.unit_densities(group_mag));
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
            let lo = unit_seed[g] / SEARCH_BRACKET_FACTOR;
            let hi = unit_seed[g] * SEARCH_BRACKET_FACTOR;
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
    let q = groups.densities(&group_mag);
    match form_find_explicit(&problem.with_densities(&q)?) {
        Err(FreeFormError::NotAForm(defect)) => Err(FreeFormError::SearchDidNotConverge {
            defect: Some(defect),
        }),
        other => other,
    }
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

    // ---- Null-space coordinate recovery (through the explicit entry) ----

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
        // A mildly perturbed symmetric-prism guess (recovery should gauge-fix
        // to it). The explicit `D` does not depend on the guess.
        let x = form_find_free(
            &perturbed_prism_guess(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q.clone()),
        )
        .expect("perturbed symmetric-prism guess must recover a 3-D realisation")
        .nodes;

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

    // ── δ (task 4415): surface-aware form_find_free_surfaces ──────────────────

    // Top {0,1,2} and bottom {3,4,5} triangle surfaces used in the δ tests.
    // These are the two face-triangles of the triplex prism.
    fn prism_surfaces() -> Vec<(usize, usize, usize)> {
        vec![(0, 1, 2), (3, 4, 5)]
    }

    // ── δ combined golden: triplex + isotropic membrane on top+bottom faces ──

    /// Combined golden: triplex (3 struts + 9 cables) + isotropic membrane σ=0.2
    /// on the top {0,1,2} and bottom {3,4,5} triangles. These are exactly
    /// equilateral at the canonical prism, so the cotangent-Laplacian adds a
    /// uniform weight w = σ/(2√3) to each horizontal edge — identical to boosting
    /// the horizontal cable densities. Combined D has nullity 4 for any modest σ>0
    /// (scale-invariant closed form), so the search finds it and the cotangent
    /// fixed point converges with combined-D equilibrium residual ~machine-zero.
    #[test]
    fn surfaces_free_combined_prism_membrane_golden() {
        let sigma = 0.2_f64;
        let membrane = PrismMembrane::with_sigma(sigma);
        let guess = perturbed_prism_guess();

        let spec = ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: vec![-1.0, 1.0, 1.0],
            reference_group: 1,
        };

        let result = membrane
            .form_find(&guess, &spec)
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
        for (idx, (&kind, &n_i)) in membrane
            .kinds
            .iter()
            .zip(result.member_forces.iter())
            .enumerate()
        {
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

        // Primary honest signal: the combined all-node equilibrium residual at
        // the SOLVED geometry and densities.
        let resid = membrane.residual_of(&result);
        assert!(
            resid < 1e-9,
            "combined per-row relative residual max|(D(x)·x)_i|/Σ_j|D_ij| over 1 + scale \
             = {resid:.3e}, expected < 1e-9",
        );
    }

    /// Unit-level companion to the integration file's `Explicit` combined
    /// coverage (task 6537): `Explicit` combined free-standing form-finding,
    /// from the perturbed guess, must converge to the closed-form combined
    /// self-stress, judged by the fixed point's own [`CombinedIterate`]
    /// residual. The independent faer-free verification path
    /// (`reassemble_d_combined` / `free_residual_scaled`) lives in
    /// tests/tensegrity_delta_combined_form_find.rs, whose regression table
    /// (`combined_explicit_analytic_q_converges_across_sigma_and_perturbation`)
    /// also covers σ=1.0. Shape (equilateral-triangle) and
    /// `surface_stresses`-echo assertions live only in that table, to avoid
    /// a third near-verbatim copy of the same assertion block.
    #[test]
    fn surfaces_free_explicit_combined_q_converges_from_perturbed_guess() {
        let sigma = 1.0_f64;
        let membrane = PrismMembrane::with_sigma(sigma);
        let guess = perturbed_prism_guess();
        let spec = ForceDensitySpec::Explicit(closed_form_combined_q(sigma));

        let result = membrane
            .form_find(&guess, &spec)
            .expect("combined explicit q from perturbed guess must form-find");

        assert!(result.converged, "combined solve must converge");
        assert_eq!(result.nullity, 4, "combined D must have nullity 4");

        // Primary honest signal: the combined all-node equilibrium residual at
        // the SOLVED geometry and densities.
        let resid = membrane.residual_of(&result);
        assert!(
            resid < 1e-9,
            "combined per-row relative residual max|(D(x)·x)_i|/Σ_j|D_ij| over 1 + scale \
             = {resid:.3e}, expected < 1e-9",
        );
    }

    // ── gauge lock (task 6413) — defect D2: the eigenvalue-gap DESCENT STEP.
    // Task 6413's outer stop test is the all-node `StiffnessScale` criterion,
    // whose gauge invariance `fd_criteria`'s own tests pin; that fix does NOT
    // touch the relax step's descent objective (`RelaxStep::eig_gap`) or its
    // descent move, which is why this task is not a straight port of task
    // 6119's diff. Unit altitude only: MEASURED, there is no e2e fixture that
    // both converges AND exercises the descent step under a gauge change (the
    // `ForceDensitySpec::Explicit` combined mode never converges from any
    // meaningful geometry perturbation), so claiming an e2e RED here would be
    // a doomed test — see `tensegrity_free_surface_gauge_covariance.rs` for
    // the e2e coverage this task DOES carry (the stop test only).

    /// The triplex membrane at the canonical prism with the closed-form line
    /// `q` and σ = 0.2, both scaled by `lambda`: the descent objective at those
    /// densities and geometry, as the relax step scores a trial.
    fn eig_gap_at_canonical_prism(lambda: f64) -> f64 {
        let membrane = PrismMembrane::with_sigma(PrismMembrane::SIGMA * lambda);
        let q: Vec<f64> = closed_form_q().iter().map(|v| v * lambda).collect();
        let x = canonical_prism();
        let problem = membrane.problem(&x, FreeSpec::Explicit(&q));
        RelaxStep::eig_gap(&problem, &q, &x)
    }

    #[test]
    fn relax_step_eig_gap_is_gauge_invariant_under_uniform_scaling() {
        // The objective is a pure scalar SCORE (sum of squares of the 4
        // smallest-|λ| eigenvalues of D_combined), so its gauge factor must
        // cancel — this is the right contract for a score, distinct from the
        // descent step below (which maps geometry to geometry, and geometry
        // carries no gauge weight, so ITS contract is bit-identical output,
        // not a cancelled scale factor). D_combined scales bit-exactly by λ
        // under q → λ·q, σ → λ·σ (λ a power of two), so every eigenvalue of D
        // scales by λ and the sum-of-4-smallest-squares scales by λ². The
        // pristine (un-normalised) objective is therefore NOT gauge-invariant
        // — checked with assert_eq! (bit-exact, λ a power of two) rather than
        // a tolerance.
        //
        // MEASURED RED after task 6413's step-2, this exact fixture (step-2
        // did not touch the objective): 6.000000000000011e-2 vs
        // 6.597069766656012e10 — ratio ≈ 1.0995e12 ≈ 2^40 = λ², not the
        // expected 1.
        const LAMBDA_UP: f64 = 1_048_576.0; // 2^20
        const LAMBDA_DOWN: f64 = 1.0 / 1_048_576.0; // 2^-20

        let o1 = eig_gap_at_canonical_prism(1.0);
        let o_up = eig_gap_at_canonical_prism(LAMBDA_UP);
        let o_down = eig_gap_at_canonical_prism(LAMBDA_DOWN);

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
    fn relax_step_output_is_gauge_invariant_under_uniform_scaling() {
        // The relax step maps GEOMETRY to GEOMETRY (plus the step size it
        // carries to the next call), and geometry carries no gauge weight
        // (only q/σ do) — so, unlike the scalar objective above, the right
        // contract here is that the OUTPUT is bit-identical across a gauge
        // change, not scaled by any power of λ. It failed before task 6413
        // because the finite-difference gradient was taken of the
        // un-normalised (gauge-covariant, scaling as λ²) objective, so a fixed
        // backtracking step accepted/rejected different trial geometries
        // depending on λ.
        //
        // MEASURED RED after task 6413's step-2 (step-2 did not touch the
        // descent): both the returned geometry and the next step size differed
        // across λ.
        const LAMBDA_UP: f64 = 1_048_576.0; // 2^20

        let relax_at = |lambda: f64| -> (Vec<[f64; 3]>, f64) {
            let membrane = PrismMembrane::with_sigma(PrismMembrane::SIGMA * lambda);
            let q: Vec<f64> = closed_form_q().iter().map(|v| v * lambda).collect();
            let x = canonical_prism();
            let problem = membrane.problem(&x, FreeSpec::Explicit(&q));
            let iterate =
                CombinedIterate::new(&problem, q.clone(), x.clone(), membrane.surface_matrix(&x));
            let mut relax = RelaxStep::new();
            let next = relax.relax(&problem, &iterate, false);
            (next, relax.descent_step)
        };

        let (next1, nstep1) = relax_at(1.0);
        let (next_up, nstep_up) = relax_at(LAMBDA_UP);

        assert_eq!(
            next1, next_up,
            "relax-step geometry must be exactly gauge-invariant (no gauge weight on geometry)",
        );
        assert_eq!(
            nstep1, nstep_up,
            "relax-step next step size must be exactly gauge-invariant: step(1)={nstep1:e} step(λ)={nstep_up:e}",
        );
    }

    #[test]
    fn relax_step_eig_gap_of_an_all_zero_d_is_zero_not_infinity() {
        // The sibling guard on the residual
        // (`fd_criteria::tests::an_all_zero_judged_row_is_not_at_equilibrium`)
        // returns INFINITY because the residual IS the outer loop's
        // convergence gate. This objective is NOT a convergence gate — only
        // the descent step's line-search score — so an identically-zero D
        // (no spectral gap left to close) is honestly a perfect score, and
        // the guarded branch must return 0.0: returning INFINITY here would
        // instead make the line search reject every trial.
        let (members, kinds) = triplex_topology();
        let x = canonical_prism();
        let q = closed_form_q();
        // No surfaces, so at zero densities the combined D is identically zero
        // at every geometry.
        let problem =
            ForceDensityProblem::new(&x, &members, &kinds, FreeSpec::Explicit(&q), &[], &[], &[])
                .expect("closed-form prism q is valid input");
        let zero_q = vec![0.0; members.len()];

        let obj = RelaxStep::eig_gap(&problem, &zero_q, &x);
        assert_eq!(obj, 0.0, "all-zero D must score a perfect 0.0, got {obj}");

        // With g0 == 0.0 the finite-difference gradient is also all-zero (D
        // stays identically zero at every FD-perturbed geometry too, since
        // the densities are zero and no surface acts, regardless of x), so the
        // backtracking search can find no strictly-downhill trial and must
        // return the geometry unchanged — pinning the claim that a premature
        // "perfect" score cannot masquerade as progress.
        let iterate = CombinedIterate::new(&problem, zero_q.clone(), x.clone(), Mat::zeros(6, 6));
        assert_eq!(
            RelaxStep::new().relax(&problem, &iterate, false),
            x,
            "the relax step must leave geometry unchanged when the objective is already 0.0",
        );
    }

    // ── χ-ff (task 9274): the combined fixed point's units ────────────────────

    /// The triplex with its two prism membrane triangles at one σ (0.2 unless
    /// given), held so a test can borrow the validated problem
    /// `form_find_free_surfaces` builds.
    struct PrismMembrane {
        members: Vec<(usize, usize)>,
        kinds: Vec<MemberKind>,
        surfaces: Vec<(usize, usize, usize)>,
        sigmas: Vec<f64>,
    }

    impl PrismMembrane {
        const SIGMA: f64 = 0.2;

        fn new() -> Self {
            Self::with_sigma(Self::SIGMA)
        }

        fn with_sigma(sigma: f64) -> Self {
            let (members, kinds) = triplex_topology();
            let surfaces = prism_surfaces();
            let sigmas = vec![sigma; surfaces.len()];
            PrismMembrane {
                members,
                kinds,
                surfaces,
                sigmas,
            }
        }

        fn problem<'a>(
            &'a self,
            nodes: &'a [[f64; 3]],
            spec: FreeSpec<'a>,
        ) -> ForceDensityProblem<'a> {
            ForceDensityProblem::new(
                nodes,
                &self.members,
                &self.kinds,
                spec,
                &[],
                &self.surfaces,
                &self.sigmas,
            )
            .expect("the prism membrane satisfies the input contract")
        }

        fn surface_matrix(&self, nodes: &[[f64; 3]]) -> Mat<f64> {
            let mut s = Mat::<f64>::zeros(nodes.len(), nodes.len());
            scatter_surface_stencils::<IsotropicStencil>(
                &mut s,
                &self.surfaces,
                &self.sigmas,
                nodes,
            )
            .expect("the prism triangles are non-degenerate at these nodes");
            s
        }

        /// The combined free-standing solve of this membrane from `guess`.
        fn form_find(
            &self,
            guess: &[[f64; 3]],
            spec: &ForceDensitySpec,
        ) -> Result<FreeFormResult, FreeFormError> {
            form_find_free_surfaces(
                guess,
                &self.members,
                &self.kinds,
                &self.surfaces,
                &self.sigmas,
                spec,
            )
        }

        /// `result`'s all-node equilibrium residual at its own densities and
        /// geometry, as the fixed point's [`CombinedIterate`] judges a trial.
        fn residual_of(&self, result: &FreeFormResult) -> f64 {
            let problem = self.problem(&result.nodes, FreeSpec::Explicit(&result.force_densities));
            let surface = self.surface_matrix(&result.nodes);
            CombinedIterate::new(
                &problem,
                result.force_densities.clone(),
                result.nodes.clone(),
                surface.clone(),
            )
            .residual_at(&problem, &result.nodes, &surface)
        }
    }

    /// Signed seed ratio per triplex group: struts compressive, horizontals
    /// (the reference) and verticals tensile.
    const TRIPLEX_SEED_RATIOS: [f64; 3] = [-1.0, 1.0, 1.0];

    fn triplex_group_ratios() -> ForceDensitySpec {
        ForceDensitySpec::GroupRatios {
            group_ids: triplex_group_ids(),
            seed_ratios: TRIPLEX_SEED_RATIOS.to_vec(),
            reference_group: 1,
        }
    }

    /// [`triplex_group_ratios`] as the validated group spec a problem holds.
    fn triplex_group_spec() -> GroupSpec {
        let group_ids = triplex_group_ids();
        GroupSpec::new(&group_ids, &TRIPLEX_SEED_RATIOS, 1, group_ids.len())
            .expect("the triplex group spec seeds the search")
    }

    #[test]
    fn combined_iterate_is_an_equilibrium_at_the_closed_form_prism_only() {
        let membrane = PrismMembrane::new();
        let (golden, guess) = (canonical_prism(), perturbed_prism_guess());
        let q = closed_form_combined_q(PrismMembrane::SIGMA);
        let problem = membrane.problem(&guess, FreeSpec::Explicit(&q));
        let iterate_at = |nodes: &[[f64; 3]]| {
            CombinedIterate::new(
                &problem,
                q.clone(),
                nodes.to_vec(),
                membrane.surface_matrix(nodes),
            )
        };

        let off_golden = iterate_at(&guess);

        assert!(iterate_at(&golden).is_equilibrium());
        assert!(!off_golden.is_equilibrium());
        assert!(
            off_golden.residual_at(&problem, &golden, &membrane.surface_matrix(&golden))
                <= FREE_SURFACE_EQUILIBRIUM_REL_TOL,
            "an iterate judges a trial geometry at its own q, with the trial's surface term",
        );
    }

    #[test]
    fn fixed_q_source_yields_the_explicit_densities_at_every_iterate() {
        let membrane = PrismMembrane::new();
        let guess = perturbed_prism_guess();
        let q = closed_form_combined_q(PrismMembrane::SIGMA);
        let problem = membrane.problem(&guess, FreeSpec::Explicit(&q));
        let mut source = QSource::new(&problem, None);

        for nodes in [perturbed_prism_guess(), canonical_prism()] {
            let surface_mat = membrane.surface_matrix(&nodes);
            assert_eq!(
                source.densities_at(&problem, &nodes, &surface_mat),
                Ok(q.clone())
            );
        }
    }

    #[test]
    fn only_a_fixed_q_source_admits_the_projection_trial() {
        let membrane = PrismMembrane::new();
        let guess = perturbed_prism_guess();
        let q = closed_form_combined_q(PrismMembrane::SIGMA);
        let fixed = membrane.problem(&guess, FreeSpec::Explicit(&q));
        let searched = membrane.problem(&guess, FreeSpec::GroupRatios(triplex_group_spec()));

        assert!(QSource::new(&fixed, None).admits_projection_trial());
        assert!(!QSource::new(&searched, None).admits_projection_trial());
    }

    #[test]
    fn searched_q_source_keeps_each_group_seed_sign() {
        let membrane = PrismMembrane::new();
        let guess = perturbed_prism_guess();
        let problem = membrane.problem(&guess, FreeSpec::GroupRatios(triplex_group_spec()));
        let boot = form_find_free(
            &guess,
            &membrane.members,
            &membrane.kinds,
            &triplex_group_ratios(),
        )
        .expect("the line-only triplex form-finds");
        let mut source = QSource::new(&problem, Some(&boot));

        let q = source
            .densities_at(&problem, &boot.nodes, &membrane.surface_matrix(&boot.nodes))
            .expect("the delta golden's first combined search succeeds");

        assert_eq!(q.len(), membrane.members.len());
        for (member, (&qi, &group)) in q.iter().zip(triplex_group_ids().iter()).enumerate() {
            assert_eq!(
                qi.signum(),
                TRIPLEX_SEED_RATIOS[group].signum(),
                "member {member} (group {group}) density {qi} lost its seed sign",
            );
        }
    }

    #[test]
    fn relax_step_without_a_projection_trial_only_descends_carrying_its_step_size() {
        let membrane = PrismMembrane::new();
        let guess = perturbed_prism_guess();
        let q = closed_form_combined_q(PrismMembrane::SIGMA);
        let problem = membrane.problem(&guess, FreeSpec::Explicit(&q));
        let eig_gap = |nodes: &[[f64; 3]]| RelaxStep::eig_gap(&problem, &q, nodes);
        let first = CombinedIterate::new(
            &problem,
            q.clone(),
            guess.clone(),
            membrane.surface_matrix(&guess),
        );
        let mut relax = RelaxStep::new();

        let next = relax.relax(&problem, &first, false);

        assert!(
            eig_gap(&next) < eig_gap(&guess),
            "the relax step must descend"
        );
        assert_ne!(
            relax.descent_step,
            RelaxStep::INITIAL_DESCENT_STEP,
            "the adapted step must carry to the next call",
        );
        assert!(
            RelaxStep::projection_trial(&problem, &first).is_some(),
            "the guess must admit a projection trial for withholding it to show",
        );
        assert_ne!(next, RelaxStep::new().relax(&problem, &first, true));
    }

    #[test]
    fn relax_step_absorbs_a_projection_trial_that_cannot_span_3d() {
        let membrane = PrismMembrane::new();
        let planar: Vec<[f64; 3]> = canonical_prism()
            .into_iter()
            .map(|[x, y, _]| [x, y, 0.0])
            .collect();
        let q = closed_form_combined_q(PrismMembrane::SIGMA);
        let problem = membrane.problem(&planar, FreeSpec::Explicit(&q));
        let surface_mat = membrane.surface_matrix(&planar);
        let iterate = CombinedIterate::new(&problem, q.clone(), planar.clone(), surface_mat);
        assert_eq!(RelaxStep::projection_trial(&problem, &iterate), None);

        let next = RelaxStep::new().relax(&problem, &iterate, true);

        assert_eq!(next, RelaxStep::new().relax(&problem, &iterate, false));
    }

    /// Drives `guard` through `GEO_STALL_ITERS - 1` iterations that each leave
    /// the geometry at `at` with `residual`, none of which may trip it.
    fn approach_stall(guard: &mut StallGuard, at: &[[f64; 3]], residual: f64) {
        for call in 1..GEO_STALL_ITERS {
            assert!(
                !guard.stalled_after(at, at, residual),
                "unmoved call {call} of {} tripped the guard early",
                GEO_STALL_ITERS - 1,
            );
        }
    }

    #[test]
    fn stall_guard_trips_on_the_geo_stall_iters_th_unmoved_flat_iteration() {
        let x = canonical_prism();
        let mut guard = StallGuard::new();
        assert!(!guard.stalled_after(&x, &x, 1.0));

        approach_stall(&mut guard, &x, 1.0);

        assert!(guard.stalled_after(&x, &x, 1.0));
    }

    #[test]
    fn stall_guard_resets_when_the_geometry_leaves_where_it_entered() {
        let (x, y) = (canonical_prism(), perturbed_prism_guess());
        let mut guard = StallGuard::new();
        assert!(!guard.stalled_after(&x, &x, 1.0));
        approach_stall(&mut guard, &x, 1.0);

        assert!(!guard.stalled_after(&x, &y, 1.0));

        approach_stall(&mut guard, &x, 1.0);
        assert!(guard.stalled_after(&x, &x, 1.0));
    }

    #[test]
    fn stall_guard_resets_when_the_residual_shrinks_past_the_threshold() {
        let x = canonical_prism();
        let shrunk = 1.0 * (1.0 - 2.0 * GEO_STALL_RESID_THRESHOLD);
        let mut guard = StallGuard::new();
        assert!(!guard.stalled_after(&x, &x, 1.0));
        approach_stall(&mut guard, &x, 1.0);

        assert!(!guard.stalled_after(&x, &x, shrunk));

        approach_stall(&mut guard, &x, shrunk);
        assert!(guard.stalled_after(&x, &x, shrunk));
    }

    #[test]
    fn stall_guard_reads_an_infinite_residual_as_flat_from_the_first_iteration() {
        let x = canonical_prism();
        let mut guard = StallGuard::new();

        approach_stall(&mut guard, &x, f64::INFINITY);

        assert!(guard.stalled_after(&x, &x, f64::INFINITY));
    }
}
