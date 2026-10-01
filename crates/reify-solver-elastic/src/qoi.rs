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

use std::fmt;

use faer::sparse::SparseRowMat;

use crate::assembly::tet::tet_p1_centroid;
use crate::boundary::dirichlet::DirichletBc;
use crate::constitutive::IsotropicElastic;
use crate::interpolation::point_in_tet_p1;
use crate::result::{element_stress_p1, tet_volume_p1};
use crate::solver::{CgResult, CgSolverOptions, SolverMode, solve_cg};

/// Borrowed P1 tet mesh view: the f64 coordinates and connectivity the
/// solve actually ran on.
///
/// Deliberately NOT `reify_ir::VolumeMesh`, which stores `f32` vertices and
/// is the *display* mesh, not the solve mesh. A QoI resolved against rounded
/// coordinates would place its ball in a subtly different spot than the one
/// the stiffness matrix was assembled from.
#[derive(Debug, Clone, Copy)]
pub struct P1TetMeshRef<'a> {
    /// Node positions, one `[x, y, z]` per node. Node `n` owns global DOFs
    /// `3n`, `3n+1`, `3n+2`.
    pub coords: &'a [[f64; 3]],
    /// Element connectivity: four global node indices per P1 tet, in
    /// element-local order.
    pub tets: &'a [[usize; 4]],
}

/// Which quantity — hence which physical dimension —
/// [`QuantityOfInterest::evaluate`] returns.
///
/// Mirrors the two `QoIDescriptor` variants of PRD §5.1 one-for-one, so the
/// eval-side result mapping to `DisplacementEstimate` / `NormalStressEstimate`
/// is a variant rename and nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QoiKind {
    /// Mean of `d · u` over the contributing set — a length.
    LocalDisplacement,
    /// Mean of `n · σ · n` over the contributing set — a pressure.
    LocalNormalStress,
}

/// Why a quantity of interest could not be resolved on a given mesh.
///
/// PRD §6 C4: an unresolvable QoI is a *typed error*, never a zero dual load
/// and never a `NaN`. Each variant names the offending value, mirroring
/// [`crate::volume_refine::RefineError`]'s convention so the eval-side
/// `RefineError | QoiError` union is mechanical.
#[derive(Debug, Clone, PartialEq)]
pub enum QoiError {
    /// `radius` is not finite, or is not strictly positive.
    ///
    /// §5.1 makes `radius` a required, positive payload field. A default
    /// "small" radius was rejected at design time because it silently
    /// reintroduces the point-delta functional whose divergence §3 measured.
    NonPositiveRadius {
        /// The offending radius, as supplied.
        radius: f64,
    },
    /// The direction (or stress normal) cannot be normalized to a unit
    /// vector, so the functional it would define does not exist.
    ///
    /// Two inputs land here, and the name covers both because the check that
    /// rejects them is one question — "does this vector have a unit form?":
    ///
    /// * **Zero length.** `d · u` — and hence the dual load — would be
    ///   identically zero. This is C4's named `direction = vec3(0,0,0)` case.
    /// * **Non-finite, or of non-finite squared length.** A `NaN` or `±inf`
    ///   component (or a magnitude so large that `‖d‖²` overflows) makes the
    ///   normalization itself `NaN`, and a `NaN` direction is exactly the
    ///   value this module's contract says is unrepresentable. Calling that
    ///   "zero length" — as the predecessor variant's name and message both
    ///   did — misdescribes the input a caller then has to debug.
    NonNormalizableDirection {
        /// The offending direction, as supplied.
        direction: [f64; 3],
    },
    /// No element centroid lies within the ball, and `at` itself lies inside
    /// no element of this mesh.
    PointOutsideBody {
        /// The offending query point.
        at: [f64; 3],
    },
    /// The contributing set resolved, but carries no usable total volume to
    /// average over — every ball mean here divides by `Σ_K V_K`.
    ///
    /// [`tet_volume_p1`] returns exactly `0.0` for four coplanar nodes, so a
    /// collapsed tet is not hypothetical. Without this arm the division
    /// yields `NaN`, and an all-`NaN` `g` reaches the solver as plausible
    /// garbage. Unconditional rather than a `debug_assert!`, because the
    /// caller who would be handed that `NaN` is a RELEASE caller.
    DegenerateContributingSet {
        /// The query point whose ball resolved to the degenerate set.
        at: [f64; 3],
        /// `Σ_K V_K` over the contributing set: zero, negative, or
        /// non-finite.
        total_volume: f64,
    },
    /// The contributing set carries usable total volume, but one of its
    /// members is unusable on its own.
    ///
    /// A collapsed tet standing ALONGSIDE healthy ones leaves `Σ_K V_K`
    /// strictly positive, so the set-level arm above passes it through and
    /// the ball mean goes on to index it. [`element_stress_p1`]'s own
    /// degenerate-Jacobian guard is a `debug_assert!`; measured in release
    /// on a good/collapsed pair, `evaluate` returned `Ok(NaN)` and
    /// `dual_load` a `g` with 12 of 15 entries non-finite.
    ///
    /// Rejecting rather than SKIPPING the element is deliberate. `V_K` is
    /// the weight the functional is defined by, so dropping one would
    /// silently report a ball mean over a set the caller never asked for —
    /// and would mask a mesh that has already compromised the primal solve,
    /// since `assembly/tet.rs` guards the same primitive.
    DegenerateElement {
        /// The query point whose ball resolved to the offending set.
        at: [f64; 3],
        /// Index into `P1TetMeshRef::tets` of the offending element.
        element: usize,
        /// That element's `V_K`: zero, negative, or non-finite.
        volume: f64,
    },
}

impl fmt::Display for QoiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QoiError::NonPositiveRadius { radius } => write!(
                f,
                "quantity-of-interest radius must be finite and strictly \
                 positive, got {radius}",
            ),
            QoiError::NonNormalizableDirection { direction } => write!(
                f,
                "quantity-of-interest direction ({}, {}, {}) has no unit \
                 form: it must be finite and of strictly positive length",
                direction[0], direction[1], direction[2],
            ),
            QoiError::PointOutsideBody { at } => write!(
                f,
                "quantity-of-interest point ({}, {}, {}) lies outside every \
                 element of this mesh",
                at[0], at[1], at[2],
            ),
            QoiError::DegenerateContributingSet { at, total_volume } => write!(
                f,
                "quantity-of-interest point ({}, {}, {}) resolved to a \
                 contributing set of total volume {total_volume}; the mesh \
                 carries degenerate (zero-volume) tets there",
                at[0], at[1], at[2],
            ),
            QoiError::DegenerateElement {
                at,
                element,
                volume,
            } => write!(
                f,
                "quantity-of-interest point ({}, {}, {}) resolved to a \
                 contributing set whose element {element} is degenerate \
                 (volume {volume}); the mesh carries a collapsed tet there",
                at[0], at[1], at[2],
            ),
        }
    }
}

impl std::error::Error for QoiError {}

/// A bounded linear functional of the displacement field, re-resolvable on
/// any mesh.
///
/// PRD §6's contract surface. Implementors are *coordinate-addressed*: every
/// method re-derives its contributing elements from `mesh` on each call, so
/// no QoI state survives a refinement (C6). A QoI is never an index into a
/// mesh the refiner is about to throw away.
///
/// # Both methods are fallible, and that is load-bearing
///
/// C4 requires the typed error from `evaluate` *and* `dual_load`. A QoI that
/// validated only on the `evaluate` path would hand the dual solve a
/// silently zero `g`, whose solution is the zero field — an error estimate
/// of exactly zero, reported as convergence. Returning `Result` makes that
/// unrepresentable rather than merely discouraged.
pub trait QuantityOfInterest {
    /// `J(u_h)` on this mesh.
    ///
    /// # Errors
    ///
    /// [`QoiError`] when the QoI cannot be resolved on `mesh` (C4).
    fn evaluate(
        &self,
        mesh: P1TetMeshRef<'_>,
        material: &IsotropicElastic,
        u: &[f64],
    ) -> Result<f64, QoiError>;

    /// The dual load `g` with `J(v) = gᵀv`, of length `3 · n_nodes`.
    ///
    /// **Not** yet zeroed at constrained DOFs — the caller owns the BC set,
    /// and `solve_dual_cg` (step-12) is where that zeroing happens. Takes `u_h` so a
    /// linearized nonlinear functional fits this seam later without a
    /// signature change; both functionals shipped today ignore it, which is
    /// what lets a caller assemble `g` *before* the primal solve.
    ///
    /// # Errors
    ///
    /// [`QoiError`] when the QoI cannot be resolved on `mesh` (C4). Never an
    /// all-zero `g` for a non-zero direction.
    fn dual_load(
        &self,
        mesh: P1TetMeshRef<'_>,
        material: &IsotropicElastic,
        u: &[f64],
    ) -> Result<Vec<f64>, QoiError>;

    /// Which quantity — hence which dimension — [`evaluate`](Self::evaluate)
    /// returns.
    fn kind(&self) -> QoiKind;
}

// ---------------------------------------------------------------------------
// Contributing set — the one validation chokepoint (C4, C6)
// ---------------------------------------------------------------------------

/// One element of a resolved [`ContributingSet`].
struct ContributingElement {
    /// Index into `P1TetMeshRef::tets`.
    index: usize,
    /// `V_K`, from [`tet_volume_p1`].
    volume: f64,
}

/// The elements a QoI's ball mean runs over on one particular mesh.
///
/// Constructed only by [`resolve`], which guarantees the four invariants
/// every consumer relies on: `elements` is non-empty, `total_volume` is
/// finite and strictly positive, EVERY member's `volume` is finite and
/// strictly positive, and `direction` is unit length.
struct ContributingSet {
    /// Contributing elements in ascending element index.
    elements: Vec<ContributingElement>,
    /// `V_E = Σ_K V_K` over `elements`.
    total_volume: f64,
    /// The QoI's direction (or stress normal), NORMALIZED.
    ///
    /// Both functionals project onto this rather than onto the raw vector
    /// the caller supplied, so `‖d‖ = 1` holds by construction at the one
    /// site that can enforce it uniformly, instead of being a precondition
    /// each consumer would have to restate and none could verify.
    direction: [f64; 3],
}

/// Resolve a QoI's contributing set on `mesh`, validating every C4 condition.
///
/// This is the single chokepoint both `evaluate` and `dual_load` of *both*
/// shipped functionals run through, so the C4 invariant is enforced in
/// exactly one place, uniformly — a new QoI variant cannot forget a check.
///
/// # The check order is fixed, not incidental
///
/// 1. `radius` finite and strictly positive, else
///    [`QoiError::NonPositiveRadius`].
/// 2. `direction`'s L2 norm finite and strictly positive, else
///    [`QoiError::NonNormalizableDirection`].
/// 3. NORMALIZE `direction`, and hand that unit vector to every consumer
///    through [`ContributingSet`]. Normalizing here rather than asserting
///    unit length removes the precondition altogether: a caller mapping a
///    DSL `vec3(0, -2, 0)` straight through gets the ball mean along that
///    direction, not one silently scaled by two. It must come AFTER check
///    2, which is what rules out the division by a zero norm.
/// 4. `E = { K : ‖centroid(K) − at‖ ≤ radius }`, in ascending element index.
/// 5. If that `E` is empty, fall back to the single element CONTAINING
///    `at`; only if no element contains it is the QoI unresolvable.
/// 6. `Σ_K V_K` over the resolved set finite and strictly positive, else
///    [`QoiError::DegenerateContributingSet`] — this is the division every
///    ball mean performs.
/// 7. Every resolved `V_K` finite and strictly positive, else
///    [`QoiError::DegenerateElement`].
///
/// # Why the SET-level volume check precedes the per-element one
///
/// They are different signals, and the coarser one dominates. When the ball
/// resolves to nothing usable at all — every member collapsed — singling out
/// one member would imply that member is special, while
/// `DegenerateContributingSet` says what is actually true of the set.
/// `DegenerateElement` is the LOCALIZING signal for the case the sum cannot
/// see: a set viable in aggregate that still carries one broken member,
/// which is exactly the case that used to reach [`element_stress_p1`] and
/// return `Ok(NaN)` to a release caller.
///
/// Both run over the SAME assembled `elements` vector, downstream of both
/// selection arms, so the centroid rule and the containment fallback are
/// covered at one chokepoint rather than by two duplicated call-site checks
/// that could later be half-updated.
///
/// `u` is passed only so its length is validated here too, keeping all three
/// of the trait's arguments checked at one site; the ball rule itself does
/// not depend on it.
///
/// # Panics
///
/// If `u.len() != 3 · mesh.coords.len()` — a caller/mesh desynchronisation,
/// not user data, so it follows the crate's unconditional-`assert!` contract
/// convention rather than becoming a [`QoiError`].
///
/// # Errors
///
/// [`QoiError`] per the check order above.
/// [`QoiError::PointOutsideBody`] when the ball catches no centroid AND
/// `at` lies inside no element.
/// [`QoiError::DegenerateContributingSet`] when the resolved elements carry
/// no usable total volume.
/// [`QoiError::DegenerateElement`] when they do, but one of them is itself
/// degenerate.
fn resolve(
    at: [f64; 3],
    radius: f64,
    direction: [f64; 3],
    mesh: P1TetMeshRef<'_>,
    u: &[f64],
) -> Result<ContributingSet, QoiError> {
    assert_eq!(
        u.len(),
        3 * mesh.coords.len(),
        "displacement vector has {} entries but the mesh has {} nodes \
         (expected {} DOFs)",
        u.len(),
        mesh.coords.len(),
        3 * mesh.coords.len(),
    );

    if !radius.is_finite() || radius <= 0.0 {
        return Err(QoiError::NonPositiveRadius { radius });
    }

    let norm_sq = direction[0] * direction[0]
        + direction[1] * direction[1]
        + direction[2] * direction[2];
    if !norm_sq.is_finite() || norm_sq <= 0.0 {
        return Err(QoiError::NonNormalizableDirection { direction });
    }
    let norm = norm_sq.sqrt();
    let unit = [
        direction[0] / norm,
        direction[1] / norm,
        direction[2] / norm,
    ];

    let radius_sq = radius * radius;
    let mut elements = Vec::new();
    let mut total_volume = 0.0_f64;
    for (index, tet) in mesh.tets.iter().enumerate() {
        let nodes = tet_nodes(mesh, tet);
        let c = tet_p1_centroid(&nodes);
        let d_sq = (c[0] - at[0]) * (c[0] - at[0])
            + (c[1] - at[1]) * (c[1] - at[1])
            + (c[2] - at[2]) * (c[2] - at[2]);
        if d_sq <= radius_sq {
            let volume = tet_volume_p1(&nodes);
            total_volume += volume;
            elements.push(ContributingElement { index, volume });
        }
    }

    if elements.is_empty() {
        let index = locate_containing_element(mesh, at).ok_or(QoiError::PointOutsideBody { at })?;
        let volume = tet_volume_p1(&tet_nodes(mesh, &mesh.tets[index]));
        total_volume = volume;
        elements.push(ContributingElement { index, volume });
    }

    if !total_volume.is_finite() || total_volume <= 0.0 {
        return Err(QoiError::DegenerateContributingSet { at, total_volume });
    }
    if let Some(bad) = elements
        .iter()
        .find(|el| !el.volume.is_finite() || el.volume <= 0.0)
    {
        return Err(QoiError::DegenerateElement {
            at,
            element: bad.index,
            volume: bad.volume,
        });
    }
    Ok(ContributingSet {
        elements,
        total_volume,
        direction: unit,
    })
}

/// The four physical node positions of `tet`, in element-local order.
#[inline]
fn tet_nodes(mesh: P1TetMeshRef<'_>, tet: &[usize; 4]) -> [[f64; 3]; 4] {
    [
        mesh.coords[tet[0]],
        mesh.coords[tet[1]],
        mesh.coords[tet[2]],
        mesh.coords[tet[3]],
    ]
}

/// Barycentric slack for the [`locate_containing_element`] fallback.
///
/// PRD §11 Q2: an ABSOLUTE slack on barycentric coordinates, which live in
/// `[0, 1]` for any non-degenerate tet whatever its physical size — so this
/// is scale-invariant by construction and is NEVER scaled by an edge length.
/// Scaling it would make the same coordinate-addressed query (C6) resolve
/// differently on a millimetre model and a metre one.
const LOCATE_BARYCENTRIC_SLACK: f64 = 1e-9;

/// The LOWEST-indexed element of `mesh` containing `at`, or `None` when `at`
/// lies outside the body.
///
/// On a shared face several elements contain `at`; the lowest index wins,
/// which is [`crate::interpolation::locate_element_p1`]'s documented rule.
/// Any deterministic tie-break would do — what matters is that it IS
/// deterministic, since the alternative is a QoI whose contributing set
/// depends on element ordering.
///
/// # Scanned in place, not through `locate_element_p1`
///
/// That primitive takes a `&[LocatableTet]`, which this mesh representation
/// cannot produce without materializing the whole thing: one `Vec` of owned
/// per-element node arrays (96 B/element) for the views to borrow, plus the
/// `Vec` of views. Calling the same [`point_in_tet_p1`] predicate at the
/// same [`LOCATE_BARYCENTRIC_SLACK`] over the same ascending index order is
/// the identical linear scan — `locate_element_p1` is a `for` loop around
/// exactly this predicate — so the result is bit-identical while the two
/// allocations disappear.
///
/// That matters because this is not a rare path: the containment fallback
/// fires whenever the QoI radius is smaller than the local element spacing
/// (§5.1 supports that as an ordinary configuration), and `resolve` runs at
/// least twice per adaptive iteration — once for `evaluate`, once for
/// `dual_load`. On a 500k-element mesh the discarded form cost ~100 MB of
/// transient allocation per refinement iteration for a query that needs
/// none.
///
/// Still O(n_elements) per query. If a caller ever issues MANY queries
/// against one mesh, the answer is
/// [`crate::interpolation::TetSpatialIndex`] — an O(log n) BVH documented as
/// returning bit-identical results — not a return to the allocating form.
fn locate_containing_element(mesh: P1TetMeshRef<'_>, at: [f64; 3]) -> Option<usize> {
    mesh.tets
        .iter()
        .position(|tet| point_in_tet_p1(&tet_nodes(mesh, tet), at, LOCATE_BARYCENTRIC_SLACK))
}

// ---------------------------------------------------------------------------
// LocalDisplacement
// ---------------------------------------------------------------------------

/// Mean of `d · u` over the ball of radius `radius` centred at `at`
/// (PRD §5.1).
///
/// `J(u_h) = Σ_{K∈E} V_K q_K / Σ_{K∈E} V_K` with `q_K = d · ū_K`, the nodal
/// mean of the four `d · u_i`. The functional is *regularized*, not
/// pointwise: as `h → 0` it converges to the true ball mean, an `L²`
/// functional bounded on `H¹`, so the DWR identity holds and effectivity has
/// a limit. A point delta would not — §3 measured its divergence.
///
/// # How the ball resolves to elements (§11 Q1, Q2)
///
/// **Membership is by CENTROID, and an element straddling the sphere is in
/// or out wholesale.** Sphere–tet clipping was rejected at v1: it makes the
/// contributing set a function of the intersection geometry, so an
/// arbitrarily small mesh perturbation moves `J` discontinuously — and a
/// discontinuous functional has no meaningful dual.
///
/// **A ball that catches no centroid falls back to the element containing
/// `at`.** Otherwise the QoI would only be addressable at radii comparable
/// to the local element size: a radius that resolved fine one refinement
/// iteration ago would start reporting [`QoiError::PointOutsideBody`] for a
/// point manifestly inside the body, purely because the mesh around it
/// changed. With the fallback, "inside the body" is the real precondition.
/// That location test uses a scale-invariant barycentric slack
/// ([`LOCATE_BARYCENTRIC_SLACK`]) that is never scaled by an edge length,
/// and on a shared face the lowest containing element index wins.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalDisplacementQoi {
    /// Ball centre, in the same coordinates as `P1TetMeshRef::coords`.
    pub at: [f64; 3],
    /// Ball radius. Required, finite and strictly positive (C4).
    pub radius: f64,
    /// Direction the displacement is projected onto. NORMALIZED on resolve,
    /// so a non-unit vector states a direction rather than a scale factor; a
    /// vector with no unit form — zero, or non-finite — is
    /// [`QoiError::NonNormalizableDirection`].
    pub direction: [f64; 3],
}

impl QuantityOfInterest for LocalDisplacementQoi {
    fn evaluate(
        &self,
        mesh: P1TetMeshRef<'_>,
        _material: &IsotropicElastic,
        u: &[f64],
    ) -> Result<f64, QoiError> {
        let set = resolve(self.at, self.radius, self.direction, mesh, u)?;
        let mut weighted = 0.0_f64;
        for el in &set.elements {
            let mut q = 0.0_f64;
            for &node in &mesh.tets[el.index] {
                for k in 0..3 {
                    q += set.direction[k] * u[3 * node + k];
                }
            }
            weighted += el.volume * (0.25 * q);
        }
        Ok(weighted / set.total_volume)
    }

    fn dual_load(
        &self,
        mesh: P1TetMeshRef<'_>,
        _material: &IsotropicElastic,
        u: &[f64],
    ) -> Result<Vec<f64>, QoiError> {
        let set = resolve(self.at, self.radius, self.direction, mesh, u)?;
        let mut g = vec![0.0_f64; 3 * mesh.coords.len()];
        for el in &set.elements {
            let w = (el.volume / set.total_volume) * 0.25;
            for &node in &mesh.tets[el.index] {
                for k in 0..3 {
                    g[3 * node + k] += w * set.direction[k];
                }
            }
        }
        Ok(g)
    }

    fn kind(&self) -> QoiKind {
        QoiKind::LocalDisplacement
    }
}

// ---------------------------------------------------------------------------
// LocalNormalStress
// ---------------------------------------------------------------------------

/// `n · σ · n` — the normal component of a Cauchy stress tensor across the
/// plane with unit normal `n`.
#[inline]
fn contract_normal_stress(sigma: &[[f64; 3]; 3], n: [f64; 3]) -> f64 {
    let mut s = 0.0_f64;
    for i in 0..3 {
        for j in 0..3 {
            s += n[i] * sigma[i][j] * n[j];
        }
    }
    s
}

/// Gather an element's twelve DOFs from the global displacement vector.
///
/// Convention `u_e[3·local + axis] = u[3·global + axis]`, matching
/// [`element_stress_p1`]'s expectation and `buckling_kernel`'s gathers.
#[inline]
fn element_displacements(u: &[f64], tet: &[usize; 4]) -> [f64; 12] {
    let mut u_e = [0.0_f64; 12];
    for (local, &global) in tet.iter().enumerate() {
        for axis in 0..3 {
            u_e[3 * local + axis] = u[3 * global + axis];
        }
    }
    u_e
}

/// Mean of `n · σ · n` over the ball of radius `radius` centred at `at`
/// (PRD §5.1) — a pressure.
///
/// `J(u_h) = Σ_{K∈E} V_K (nᵀσ_K n) / Σ_{K∈E} V_K`, with `σ_K` the constant
/// P1 element stress from [`element_stress_p1`]. The ball resolves to
/// elements by exactly the same rules as [`LocalDisplacementQoi`] — see that
/// type's doc block for §11 Q1/Q2 — because both functionals share one
/// private `resolve`.
///
/// # Why a ball mean and not a point value
///
/// P1 stress is element-wise CONSTANT and discontinuous across faces, so a
/// pointwise normal stress is not even well defined on a face or an edge,
/// and on a refining mesh a point value does not converge. Averaging over a
/// fixed physical ball is what makes this a bounded functional with a mesh
/// limit — the same reason the displacement QoI is a ball mean, but here the
/// discontinuity makes it not merely preferable but necessary.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalNormalStressQoi {
    /// Ball centre, in the same coordinates as `P1TetMeshRef::coords`.
    pub at: [f64; 3],
    /// Ball radius. Required, finite and strictly positive (C4).
    pub radius: f64,
    /// Normal of the plane the stress is resolved across. NORMALIZED on
    /// resolve — which matters more here than for a displacement, since
    /// `n·σ·n` scales QUADRATICALLY in `‖n‖`. A vector with no unit form —
    /// zero, or non-finite — is [`QoiError::NonNormalizableDirection`], the
    /// same variant a displacement direction yields, since in both cases it
    /// is the vector the functional projects onto that has no direction to
    /// give.
    pub normal: [f64; 3],
}

impl QuantityOfInterest for LocalNormalStressQoi {
    fn evaluate(
        &self,
        mesh: P1TetMeshRef<'_>,
        material: &IsotropicElastic,
        u: &[f64],
    ) -> Result<f64, QoiError> {
        let set = resolve(self.at, self.radius, self.normal, mesh, u)?;
        let mut weighted = 0.0_f64;
        for el in &set.elements {
            let tet = &mesh.tets[el.index];
            let sigma = element_stress_p1(
                &tet_nodes(mesh, tet),
                material,
                &element_displacements(u, tet),
            );
            weighted += el.volume * contract_normal_stress(&sigma, set.direction);
        }
        Ok(weighted / set.total_volume)
    }

    /// # Implementation: the dual load by twelve unit probes
    ///
    /// The columns of a linear map are the images of the basis vectors, so
    /// this recovers each contributing element's row of `g` by applying
    /// [`element_stress_p1`] to each of the twelve unit element
    /// displacements and contracting the image with `n ⊗ n`.
    ///
    /// That extraction is EXACT, not an approximation:
    /// `element_stress_p1` forms `ε = B·u_e` with `B` independent of `u_e`
    /// and `σ = D·ε`, so it is strictly linear with no affine offset —
    /// `u_e = 0` maps to `σ = 0` exactly, with nothing for the probes to
    /// leave behind.
    ///
    /// Probing rather than exposing a B-matrix is deliberate. The crate has
    /// no public B, and adding one would put the Voigt/engineering-shear
    /// convention (`γ = 2ε`) in a second place that could drift from
    /// `assembly::tet`'s. The twelve probes keep it in exactly one. The
    /// O(12) cost per contributing element is negligible: `E` is a handful
    /// of elements around a point, never the mesh.
    fn dual_load(
        &self,
        mesh: P1TetMeshRef<'_>,
        material: &IsotropicElastic,
        u: &[f64],
    ) -> Result<Vec<f64>, QoiError> {
        let set = resolve(self.at, self.radius, self.normal, mesh, u)?;
        let mut g = vec![0.0_f64; 3 * mesh.coords.len()];
        for el in &set.elements {
            let tet = &mesh.tets[el.index];
            let nodes = tet_nodes(mesh, tet);
            let w = el.volume / set.total_volume;
            let mut probe = [0.0_f64; 12];
            for col in 0..12 {
                probe[col] = 1.0;
                let sigma = element_stress_p1(&nodes, material, &probe);
                probe[col] = 0.0;
                g[3 * tet[col / 3] + col % 3] +=
                    w * contract_normal_stress(&sigma, set.direction);
            }
        }
        Ok(g)
    }

    fn kind(&self) -> QoiKind {
        QoiKind::LocalNormalStress
    }
}

// ---------------------------------------------------------------------------
// The dual-solve seam (§5.3)
// ---------------------------------------------------------------------------

/// Assemble a QoI's dual load, constrain it, and solve `K z_h = g`.
///
/// PRD §5.3. This is the whole adjoint solve behind ONE call, so a caller
/// wiring goal-oriented refinement never has to re-derive the BC treatment or
/// re-assemble anything.
///
/// # `k` is the primal's ALREADY-ELIMINATED matrix, reused verbatim
///
/// The dual system shares the primal's operator — that is what makes the
/// adjoint solve cheap, and it is also a correctness precondition, not just
/// an optimisation. Re-assembling or re-eliminating would give a `K` equal
/// only to within assembly round-off, and BT2's bit-identity (a self-dual
/// load must reproduce the primal solution exactly) would fail. For the same
/// reason the caller passes its `CgSolverOptions` through unchanged: PRD §11
/// Q4 derives BT1's `10 · tolerance` slack and BT2's bit-identity from the
/// two solves sharing one tolerance, so the two must move together.
///
/// # The dual is ALWAYS homogeneously constrained
///
/// `g` is zeroed at every constrained DOF, whatever values the PRIMAL
/// prescribed. The adjoint of a problem with non-homogeneous Dirichlet data
/// still has homogeneous data: prescribed DOFs are not unknowns, so the
/// functional cannot be sensitive to a residual there.
///
/// This is deliberately ONE loop rather than half of
/// [`crate::apply_dirichlet_row_elimination`]. That function's other half — folding
/// the eliminated columns into the right-hand side — must NOT run here: `k`
/// arrives already row-eliminated, so those columns are already gone, and
/// applying the correction a second time would corrupt `g`.
///
/// # A non-converged dual is the caller's diagnostic
///
/// The returned [`CgResult`] carries `converged`; this function does not
/// treat `false` as an error. The estimate built from a partially-converged
/// `z_h` is still worth reporting alongside a warning — refusing to report
/// one would turn a quality signal into a hard failure.
///
/// # No warm start across meshes
///
/// Each mesh gets a cold solve. A remesh preserves no DOF numbering, so a
/// previous `z_h` is not merely a poor initial guess on the new mesh — it is
/// a vector whose entries refer to different nodes entirely.
///
/// # Panics
///
/// Both arms of the stale-state desynchronisation the note above describes —
/// a `(k, bcs)` pair eliminated on the pre-remesh mesh, handed the
/// post-remesh one — checked HERE so the BC-zeroing loop cannot index `g`
/// out of bounds first and report a bare offset naming neither the operator,
/// the BC set, nor the mesh:
///
/// * If `k.nrows() != 3 · mesh.coords.len()`, i.e. the OPERATOR and the mesh
///   disagree. [`crate::apply_dirichlet_row_elimination`] and [`solve_cg`] assert
///   the same shape with the same diagnostic.
/// * If any `bcs[i].dof` is beyond the dual load's last entry, i.e. the BC
///   SET and the mesh disagree. The operator check cannot see this one: `k`
///   and `mesh` can agree perfectly while `bcs` is the set carried over from
///   a finer pre-remesh mesh, and that is exactly the case that used to
///   reach a bare slice-index panic.
///
/// # Errors
///
/// [`QoiError`] if the QoI cannot be resolved on `mesh` (C4). No dual solve
/// is attempted in that case: an unresolvable QoI has no dual load, and
/// solving with a zero `g` would return the zero field and report an error
/// estimate of exactly zero — convergence.
#[allow(clippy::too_many_arguments)]
pub fn solve_dual_cg(
    k: &SparseRowMat<usize, f64>,
    qoi: &dyn QuantityOfInterest,
    mesh: P1TetMeshRef<'_>,
    material: &IsotropicElastic,
    u: &[f64],
    bcs: &[DirichletBc],
    opts: CgSolverOptions,
    mode: SolverMode,
) -> Result<CgResult, QoiError> {
    assert_eq!(
        k.nrows(),
        3 * mesh.coords.len(),
        "k.nrows() = {} but the mesh has {} nodes (expected {} DOFs); the \
         operator, the BC set and the mesh must all come from the SAME \
         solve — see \"No warm start across meshes\" above",
        k.nrows(),
        mesh.coords.len(),
        3 * mesh.coords.len(),
    );
    let mut g = qoi.dual_load(mesh, material, u)?;
    if let Some(bad) = bcs.iter().find(|bc| bc.dof >= g.len()) {
        panic!(
            "Dirichlet BC constrains dof {} but the dual load has only {} \
             entries ({} nodes × 3); the operator, the BC set and the mesh \
             must all come from the SAME solve — see \"No warm start across \
             meshes\" above",
            bad.dof,
            g.len(),
            mesh.coords.len(),
        );
    }
    for bc in bcs {
        g[bc.dof] = 0.0;
    }
    Ok(solve_cg(k, &g, opts, mode))
}

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
    /// (shares face {1,2,3} with tet0).
    ///
    /// The two tets have volumes 1/6 and 1/3 — a 1:2 ratio, NOT the equal
    /// volumes `error_estimator`'s copy of this fixture claims in its doc
    /// comment. That inequality is load-bearing here: it is what lets
    /// `evaluate_is_the_volume_weighted_mean_over_elements_of_unequal_volume`
    /// tell a volume-weighted mean apart from a plain elementwise average.
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

    /// Largest `|u|` entry of [`two_tet_fan_u`] — the fan fixture's own
    /// magnitude, and hence the absolute floor [`assert_close`] uses on it.
    const FAN_U_SCALE: f64 = 0.09;

    /// Stress scale of the Kuhn-cube fixtures: `E · max|u| / L`, with the
    /// dimensionless material's `E = 1`, the unit cube's `L = 1`, and
    /// `max|u| ≤ 0.06` across both cube displacement fields. No `σ` these
    /// fixtures can produce is more than an order of magnitude from it.
    const CUBE_STRESS_SCALE: f64 = 0.06;

    /// Assert `got` matches `want` to `rel` RELATIVE, with an absolute floor
    /// of `rel · scale`.
    ///
    /// The floor is the whole point. A relative-only bound collapses to a
    /// demand for BIT equality wherever `want == 0`, and `want == 0` is not
    /// hypothetical here: with `d = [0, 0, 1]` over tet0 alone, the fan
    /// field's z-components (0.00, 0.01, 0.04, −0.05) cancel EXACTLY in f64
    /// — `0.01 + 0.04` rounds to exactly `double(0.05)`. The two sides of
    /// these comparisons are summed in different orders (`evaluate` element
    /// by element, the expectation node by node), so demanding bit equality
    /// of them would pass only by coincidence of rounding, test nothing
    /// about the tolerance in the meantime, and turn any reordering or
    /// fixture edit into a spurious failure. Flooring at the FIXTURE's own
    /// magnitude keeps the bound meaningful at `want == 0` without loosening
    /// it where `want` is large.
    #[track_caller]
    fn assert_close(got: f64, want: f64, rel: f64, scale: f64, case: &str) {
        let tol = rel * want.abs().max(scale);
        let delta = (got - want).abs();
        assert!(
            delta <= tol,
            "{case}: expected {want}, got {got} (|Δ| = {delta} exceeds the \
             tolerance {tol} = {rel} · max(|expected|, fixture scale {scale}))",
        );
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

    /// Every direction/normal input that has no unit form, and so must reach
    /// [`QoiError::NonNormalizableDirection`].
    ///
    /// Shared by the displacement and normal-stress error matrices, because
    /// the arm they exercise is literally shared — one `resolve`. Listing the
    /// cases once is what keeps the two matrices from drifting into covering
    /// different halves of the same contract.
    ///
    /// The zero vector is C4's named case. The rest are the non-finite ones,
    /// which the predecessor variant routed here under the name
    /// `ZeroDirection`: correct routing, wrong diagnosis. The last entry is
    /// finite componentwise but has `‖d‖²` overflow to `+inf`, so it fails
    /// the same normalizability question by a different route.
    fn unusable_directions() -> [[f64; 3]; 6] {
        [
            [0.0, 0.0, 0.0],
            [f64::NAN, 0.0, 0.0],
            [f64::INFINITY, 0.0, 0.0],
            [f64::NEG_INFINITY, 0.0, 0.0],
            [1.0, 0.0, f64::NAN],
            [1e200, 1e200, 1e200],
        ]
    }

    /// Bit patterns of a direction's three components, so a payload carrying
    /// `NaN` can be compared for exact identity.
    ///
    /// `QoiError`'s derived `PartialEq` inherits `f64`'s, under which
    /// `NaN != NaN` — the same reason `assert_both_directions_reject` takes a
    /// predicate rather than a `QoiError`.
    fn direction_bits(direction: &[f64; 3]) -> [u64; 3] {
        [
            direction[0].to_bits(),
            direction[1].to_bits(),
            direction[2].to_bits(),
        ]
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
    /// * `direction = [0,0,0]`, and `direction` with a non-finite component
    ///   (NaN, ±inf) → [`QoiError::NonNormalizableDirection`], each naming
    ///   the offending vector. Both are covered because both reach the same
    ///   arm and the variant's name has to be true of each: a NaN direction
    ///   reported as "zero length" misdescribes the input a caller is then
    ///   sent to debug.
    /// * `at` outside every element, with a radius too small to catch any
    ///   centroid → [`QoiError::PointOutsideBody`].
    ///
    /// # The non-normalizable cases pin the check ORDER
    ///
    /// `resolve` NORMALIZES the direction, so a vector with no unit form has
    /// to reach the typed error *before* that division. Reversed, C4's named
    /// `direction = vec3(0,0,0)` case would come back as a `NaN`-direction
    /// ball mean instead of [`QoiError::NonNormalizableDirection`] — a `NaN`
    /// the module's contract says is unrepresentable. That is why the ordering
    /// inside `resolve` is fixed and documented rather than incidental.
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

        for bad_direction in unusable_directions() {
            assert_both_directions_reject(
                &LocalDisplacementQoi {
                    at: good_at,
                    radius: 0.1,
                    direction: bad_direction,
                },
                &format!("direction = {bad_direction:?}"),
                // Bitwise payload comparison, so the NaN cases pin the
                // offending vector just as tightly as the zero one.
                |e| {
                    matches!(e, QoiError::NonNormalizableDirection { direction }
                             if direction_bits(direction) == direction_bits(&bad_direction))
                },
            );
        }

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
    /// below: adding a third variant makes `label` fail to compile.
    /// `kind()` is what tells the result layer which `QoIEstimate` variant —
    /// hence which dimension — `evaluate` returned.
    ///
    /// There is deliberately no assertion ON `label`. Checking that a
    /// function defined six lines above returns the string literal its own
    /// body is written to return exercises no production code; `label` earns
    /// its place through the compile-time claim, and by naming the offending
    /// variant when one of the assertions below fails.
    #[test]
    fn qoi_kind_has_exactly_the_two_shipped_variants_and_local_displacement_reports_its_own() {
        fn label(k: QoiKind) -> &'static str {
            match k {
                QoiKind::LocalDisplacement => "LocalDisplacement",
                QoiKind::LocalNormalStress => "LocalNormalStress",
            }
        }

        let displacement = LocalDisplacementQoi {
            at: [0.25, 0.25, 0.25],
            radius: 0.1,
            direction: [0.0, -1.0, 0.0],
        };
        let stress = LocalNormalStressQoi {
            at: [0.25, 0.25, 0.25],
            radius: 0.1,
            normal: [0.0, -1.0, 0.0],
        };
        assert_eq!(
            displacement.kind(),
            QoiKind::LocalDisplacement,
            "LocalDisplacementQoi must report the displacement kind, got {}",
            label(displacement.kind()),
        );
        assert_eq!(
            stress.kind(),
            QoiKind::LocalNormalStress,
            "LocalNormalStressQoi must report the normal-stress kind, got {}",
            label(stress.kind()),
        );
    }

    // ── §5.1 contributing-set semantics, functional/dual algebra (step-3) ──

    /// Node indices with any non-zero DOF entry in `g`, ascending.
    ///
    /// The dual load's *support* is how these tests pin contributing-set
    /// membership without reaching into the private `resolve`: `g` is
    /// accumulated node-by-node over exactly the elements of `E` with
    /// strictly positive weights, so its support is the union of those
    /// elements' nodes and nothing else. Pinning membership through the
    /// public surface this way leaves the internal `ContributingSet` free to
    /// change shape.
    fn dual_load_support(g: &[f64]) -> Vec<usize> {
        assert_eq!(g.len() % 3, 0, "a dual load has 3 entries per node");
        (0..g.len() / 3)
            .filter(|&n| (0..3).any(|k| g[3 * n + k] != 0.0))
            .collect()
    }

    /// `(J(u_h), g)` for `qoi` on the shared 2-tet fan fixture.
    fn eval_and_dual_on_fan(qoi: &dyn QuantityOfInterest, case: &str) -> (f64, Vec<f64>) {
        let coords = two_tet_fan_coords();
        let tets = two_tet_fan_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = two_tet_fan_u();
        let j = qoi
            .evaluate(mesh, &mat, &u)
            .unwrap_or_else(|e| panic!("{case}: evaluate must resolve, got {e}"));
        let g = qoi
            .dual_load(mesh, &mat, &u)
            .unwrap_or_else(|e| panic!("{case}: dual_load must resolve, got {e}"));
        (j, g)
    }

    /// `d · ū_K` over an explicit node list — the closed form, derived from
    /// the fixture arrays alone and sharing no code with `super`.
    fn direction_dot_nodal_mean(nodes: &[usize], u: &[f64], d: [f64; 3]) -> f64 {
        let mut q = 0.0_f64;
        for &n in nodes {
            for k in 0..3 {
                q += d[k] * u[3 * n + k];
            }
        }
        q / nodes.len() as f64
    }

    /// Euclidean distance, for stating each fixture's geometric premises as
    /// assertions rather than as trusted comment arithmetic.
    fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    /// The two fan-fixture element centroids, `[tet0, tet1]`.
    const FAN_CENTROIDS: [[f64; 3]; 2] = [[0.25, 0.25, 0.25], [0.5, 0.5, 0.5]];

    /// §5.1 / §11 Q1 — the contributing set is EXACTLY the elements whose
    /// **centroids** lie within `radius` of `at`, and an element straddling
    /// the sphere is in or out **wholesale**.
    ///
    /// Centroid membership, no sphere–tet clipping: clipping was rejected at
    /// v1 because it makes the contributing set a function of the sphere's
    /// intersection geometry, so an arbitrarily small mesh perturbation moves
    /// `J` discontinuously — and a discontinuous functional has no meaningful
    /// dual.
    ///
    /// The radius-0.2 configuration exercises BOTH halves of that rule at
    /// once, and the test asserts the geometry that makes it do so rather
    /// than asserting it in a comment:
    ///
    /// * tet1 is **excluded** even though the ball reaches across the shared
    ///   face into tet1's interior (the ball's closest approach to the plane
    ///   `x+y+z=1` is ≈0.144 < 0.2, and the nearest point is the shared
    ///   face's own centroid) — its centroid is ≈0.433 away, so out wholesale.
    /// * tet0 is **included** even though most of its volume lies outside the
    ///   ball (its node 0 is ≈0.433 away) — its centroid is in, so in
    ///   wholesale.
    ///
    /// The 0.42/0.44 pair then brackets tet1's centroid distance from both
    /// sides, pinning the membership threshold without an fp-equality
    /// comparison against `0.25·√3`.
    #[test]
    fn contributing_set_is_the_centroid_ball_and_straddling_elements_are_in_or_out_wholesale() {
        let c0 = FAN_CENTROIDS[0];
        let c1 = FAN_CENTROIDS[1];
        let d = [0.0, -1.0, 0.0];

        // Geometric premises of the straddle configuration, asserted.
        let centroid_gap = dist(c0, c1);
        let ball_to_shared_face = dist(c0, [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0]);
        assert!(
            ball_to_shared_face < 0.2 && 0.2 < centroid_gap,
            "fixture premise: a radius of 0.2 about tet0's centroid must \
             reach into tet1 ({ball_to_shared_face}) while excluding tet1's \
             centroid ({centroid_gap})",
        );
        assert!(
            dist(c0, [0.0, 0.0, 0.0]) > 0.2,
            "fixture premise: tet0's node 0 must lie OUTSIDE the 0.2 ball, so \
             including tet0 is genuinely a wholesale inclusion",
        );

        for (radius, expected_support, why) in [
            (0.2, vec![0, 1, 2, 3], "straddle: tet1 out wholesale, tet0 in wholesale"),
            (0.42, vec![0, 1, 2, 3], "just inside tet1's centroid distance"),
            (0.44, vec![0, 1, 2, 3, 4], "just outside it — tet1 joins"),
        ] {
            let (_, g) = eval_and_dual_on_fan(
                &LocalDisplacementQoi {
                    at: c0,
                    radius,
                    direction: d,
                },
                why,
            );
            assert_eq!(
                dual_load_support(&g),
                expected_support,
                "radius {radius} ({why}): the dual load's support must be \
                 exactly the union of the contributing elements' nodes",
            );
        }
    }

    /// §5.1 fallback arm — when NO element centroid lies in the ball, the
    /// contributing set falls back to the single element that CONTAINS `at`.
    ///
    /// Without this arm a QoI is only addressable at radii comparable to the
    /// local element size: on a mesh the refiner has just coarsened, a
    /// user-specified radius that resolved fine one iteration ago suddenly
    /// catches no centroid, and the QoI reports `PointOutsideBody` for a
    /// point that is manifestly inside the body. The fallback makes "inside
    /// the body" the real precondition, independent of mesh density.
    ///
    /// `at = (0.05, 0.05, 0.05)` is strictly inside tet0 and strictly outside
    /// tet1 (every point of tet1 has `x+y+z ≥ 1`), so the expected answer is
    /// unambiguous — both assertions the test makes as premises.
    ///
    /// # TDD red→green
    ///
    /// **RED** (step-3): step-2 implements the ball rule only, so an empty
    /// `E` returns `PointOutsideBody` and both calls below fail.
    /// **GREEN** (step-4): `resolve` gains the `locate_element_p1` arm.
    #[test]
    fn contributing_set_falls_back_to_the_containing_element_when_no_centroid_is_in_the_ball() {
        let coords = two_tet_fan_coords();
        let u = two_tet_fan_u();
        let at = [0.05, 0.05, 0.05];
        let radius = 0.01;
        let d = [0.0, -1.0, 0.0];

        // Premises: no centroid is in the ball, and `at` lies in tet0 alone.
        for (i, c) in FAN_CENTROIDS.iter().enumerate() {
            assert!(
                dist(at, *c) > radius,
                "fixture premise: tet{i}'s centroid must lie OUTSIDE the ball, \
                 or this exercises the ball rule rather than the fallback",
            );
        }
        let tet0_nodes = [coords[0], coords[1], coords[2], coords[3]];
        let tet1_nodes = [coords[1], coords[2], coords[3], coords[4]];
        assert!(
            point_in_tet_p1(&tet0_nodes, at, 1e-9),
            "fixture premise: `at` must lie inside tet0",
        );
        assert!(
            !point_in_tet_p1(&tet1_nodes, at, 1e-9),
            "fixture premise: `at` must lie outside tet1, so the expected \
             fallback element is unambiguous",
        );

        let (j, g) = eval_and_dual_on_fan(
            &LocalDisplacementQoi {
                at,
                radius,
                direction: d,
            },
            "fallback to the containing element",
        );

        assert_eq!(
            dual_load_support(&g),
            vec![0, 1, 2, 3],
            "the fallback set is exactly {{tet0}}, so `g`'s support is tet0's \
             four nodes — node 4 belongs to tet1 alone and must stay zero",
        );
        let expected = direction_dot_nodal_mean(&[0, 1, 2, 3], &u, d);
        assert_close(
            j,
            expected,
            1e-12,
            FAN_U_SCALE,
            "a single-element contributing set makes J the plain nodal mean d·ū_0",
        );
    }

    /// §5.1 / §11 Q2 — on a shared face the LOWEST containing element index
    /// wins, and the location tolerance is a **scale-invariant barycentric
    /// slack** that is never scaled by an edge length.
    ///
    /// `at` is the shared face's centroid, so tet0 and tet1 both contain it
    /// (one barycentric coordinate is zero in each) and the tie-break is what
    /// decides the answer. The support assertion discriminates cleanly:
    /// tet0's nodes are `{0,1,2,3}` and tet1's are `{1,2,3,4}`, so picking
    /// the wrong element swaps node 0 for node 4.
    ///
    /// The 1000× rerun is the scale-invariance half. Barycentric coordinates
    /// live in `[0,1]` for any non-degenerate tet regardless of physical
    /// size, so the SAME `1e-9` must classify the same point the same way on
    /// a metre-scale mesh and a kilometre-scale one. A tolerance scaled by
    /// edge length would silently change the tie-break — or miss the face
    /// entirely — on a rescaled model, which for a coordinate-addressed QoI
    /// (C6) means the same design query answering differently in different
    /// units.
    ///
    /// # TDD red→green
    ///
    /// **RED** (step-3), **GREEN** (step-4) — as for the fallback test above.
    #[test]
    fn shared_face_point_falls_back_to_the_lowest_containing_element_at_a_scale_invariant_slack() {
        let d = [0.0, -1.0, 0.0];
        let base_at = [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0];
        let base_radius = 0.01;

        for scale in [1.0_f64, 1000.0] {
            let coords: Vec<[f64; 3]> = two_tet_fan_coords()
                .iter()
                .map(|c| [c[0] * scale, c[1] * scale, c[2] * scale])
                .collect();
            let tets = two_tet_fan_tets();
            let mesh = P1TetMeshRef {
                coords: &coords,
                tets: &tets,
            };
            let mat = dimensionless_steel_like();
            let u = two_tet_fan_u();
            let at = [base_at[0] * scale, base_at[1] * scale, base_at[2] * scale];
            let radius = base_radius * scale;

            // Premises: both elements contain `at` at the SAME unscaled
            // barycentric slack, and neither centroid is in the ball.
            let tet0_nodes = [coords[0], coords[1], coords[2], coords[3]];
            let tet1_nodes = [coords[1], coords[2], coords[3], coords[4]];
            assert!(
                point_in_tet_p1(&tet0_nodes, at, 1e-9) && point_in_tet_p1(&tet1_nodes, at, 1e-9),
                "scale {scale}: fixture premise — both elements must contain \
                 the shared-face point at the unscaled slack 1e-9, or there \
                 is no tie to break",
            );
            for (i, c) in FAN_CENTROIDS.iter().enumerate() {
                let scaled = [c[0] * scale, c[1] * scale, c[2] * scale];
                assert!(
                    dist(at, scaled) > radius,
                    "scale {scale}: fixture premise — tet{i}'s centroid must \
                     lie outside the ball so the fallback arm is what runs",
                );
            }

            let qoi = LocalDisplacementQoi {
                at,
                radius,
                direction: d,
            };
            let g = qoi
                .dual_load(mesh, &mat, &u)
                .unwrap_or_else(|e| panic!("scale {scale}: dual_load must resolve, got {e}"));
            assert_eq!(
                dual_load_support(&g),
                vec![0, 1, 2, 3],
                "scale {scale}: the LOWEST containing element index wins, so \
                 the support is tet0's nodes — node 4 appearing (or node 0 \
                 missing) means tet1 was chosen",
            );
        }
    }

    /// The algebraic identity the whole dual formulation rests on:
    /// `J(v) = gᵀv` exactly, for every resolvable configuration.
    ///
    /// This is what licenses the DWR derivation to replace `J(u − u_h)` with
    /// `gᵀ(u − u_h)` and hence with the dual-weighted residual. If it held
    /// only approximately, every downstream error estimate would carry an
    /// unquantified modelling error on top of the discretization error it is
    /// trying to measure.
    ///
    /// Both functionals are strictly linear in `u` with no affine offset, so
    /// the only defect is floating-point summation ORDER — `evaluate` sums
    /// element-by-element, `gᵀu` node-by-node. That is `O(n_e · ε)` against a
    /// fixture of magnitude [`FAN_U_SCALE`], so the bound carries many orders
    /// of margin and would still catch any genuine algebraic discrepancy.
    ///
    /// The bound is relative to `J` but FLOORED at that fixture scale (see
    /// [`assert_close`]), because one case in the matrix below —
    /// `d = [0, 0, 1]` over tet0 alone — makes `J` exactly zero. Relative
    /// only, that case would demand bit equality between two summation
    /// orders and assert nothing about the tolerance at all.
    #[test]
    fn evaluate_equals_the_dual_load_contracted_with_the_displacement_field() {
        let u = two_tet_fan_u();
        let third = 1.0 / 3.0;
        let two_thirds = 2.0 / 3.0;

        for (at, radius, label) in [
            (FAN_CENTROIDS[0], 0.2, "tet0 alone"),
            (FAN_CENTROIDS[0], 0.44, "both elements"),
            ([0.375, 0.375, 0.375], 0.25, "both, ball centred between them"),
        ] {
            for direction in [
                [0.0, -1.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [third, two_thirds, two_thirds],
            ] {
                let case = format!("{label}, d = {direction:?}");
                let (j, g) = eval_and_dual_on_fan(
                    &LocalDisplacementQoi {
                        at,
                        radius,
                        direction,
                    },
                    &case,
                );
                let contracted: f64 = g.iter().zip(&u).map(|(gi, ui)| gi * ui).sum();
                assert_close(
                    contracted,
                    j,
                    1e-12,
                    FAN_U_SCALE,
                    &format!(
                        "{case}: J(u_h) = gᵀu_h must hold (evaluate is the \
                         expectation, gᵀu_h the measurement)"
                    ),
                );
            }
        }
    }

    /// C4's positive half — `g` is never all-zero for a non-zero direction on
    /// a non-empty contributing set.
    ///
    /// The negative half (BT4 above) makes an unresolvable QoI a typed error.
    /// This one closes the remaining hole: a *resolvable* QoI must not return
    /// a silently zero `g` either, because `K z_h = 0` solves to the zero
    /// dual field and reports an error estimate of exactly zero — i.e.
    /// convergence — for every mesh.
    #[test]
    fn dual_load_is_never_all_zero_for_a_unit_direction_on_a_non_empty_contributing_set() {
        let third = 1.0 / 3.0;
        let two_thirds = 2.0 / 3.0;
        for (at, radius, label) in [
            (FAN_CENTROIDS[0], 0.2, "tet0 alone"),
            (FAN_CENTROIDS[0], 0.44, "both elements"),
        ] {
            for direction in [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
                [third, two_thirds, two_thirds],
                [-two_thirds, third, -two_thirds],
            ] {
                let case = format!("{label}, d = {direction:?}");
                let (_, g) = eval_and_dual_on_fan(
                    &LocalDisplacementQoi {
                        at,
                        radius,
                        direction,
                    },
                    &case,
                );
                assert!(
                    g.iter().any(|&x| x != 0.0),
                    "{case}: C4 — g is never zero for a non-zero direction",
                );
                assert!(
                    g.iter().all(|x| x.is_finite()),
                    "{case}: C4 — g never contains NaN or infinity",
                );
            }
        }
    }

    /// `evaluate` is the **volume-weighted** mean `Σ V_K q_K / Σ V_K`, not the
    /// plain elementwise average.
    ///
    /// The fan fixture's two tets have volumes 1/6 and 1/3 — a 1:2 ratio — so
    /// this configuration discriminates: the volume-weighted answer is
    /// `(q₀ + 2q₁)/3 = -0.0275` and the unweighted one is
    /// `(q₀ + q₁)/2 = -0.02625`, ~4.5% apart. The test asserts that gap
    /// explicitly, so a future fixture edit that equalised the volumes would
    /// fail here rather than silently making the check vacuous.
    ///
    /// Weighting matters because a refined mesh is a graded one: the elements
    /// near a stress concentration are orders of magnitude smaller than those
    /// away from it, so an unweighted mean over a ball would let a cloud of
    /// tiny elements outvote the large element holding most of the ball's
    /// volume, and `J` would drift with mesh density instead of converging.
    #[test]
    fn evaluate_is_the_volume_weighted_mean_over_elements_of_unequal_volume() {
        let coords = two_tet_fan_coords();
        let u = two_tet_fan_u();
        let d = [0.0, -1.0, 0.0];

        let v0 = tet_volume_p1(&[coords[0], coords[1], coords[2], coords[3]]);
        let v1 = tet_volume_p1(&[coords[1], coords[2], coords[3], coords[4]]);
        assert!(
            (v0 - 1.0 / 6.0).abs() < 1e-15 && (v1 - 1.0 / 3.0).abs() < 1e-15,
            "fixture premise: the fan's tets have volumes 1/6 and 1/3, got \
             {v0} and {v1} — equal volumes would make this test vacuous",
        );

        let q0 = direction_dot_nodal_mean(&[0, 1, 2, 3], &u, d);
        let q1 = direction_dot_nodal_mean(&[1, 2, 3, 4], &u, d);
        let weighted = (v0 * q0 + v1 * q1) / (v0 + v1);
        let unweighted = 0.5 * (q0 + q1);
        assert!(
            (weighted - unweighted).abs() > 1e-3 * weighted.abs(),
            "fixture premise: the weighted ({weighted}) and unweighted \
             ({unweighted}) means must differ materially, or this test \
             cannot detect a missing V_K",
        );

        let (j, _) = eval_and_dual_on_fan(
            &LocalDisplacementQoi {
                at: FAN_CENTROIDS[0],
                radius: 0.44,
                direction: d,
            },
            "volume-weighted mean over both elements",
        );
        assert_close(
            j,
            weighted,
            1e-15,
            FAN_U_SCALE,
            &format!(
                "J must be the volume-weighted mean (= -0.0275 by hand); the \
                 unweighted mean would be {unweighted}"
            ),
        );
    }


    /// A contributing set with no volume is a TYPED error — in release too.
    ///
    /// [`tet_volume_p1`] returns exactly `0.0` for four coplanar nodes, so a
    /// collapsed element is reachable from real mesh data rather than only
    /// from a hand-built pathology. Both ball means divide by `Σ_K V_K`, so
    /// without an unconditional check `evaluate` returns `0.0/0.0 = NaN` and
    /// `dual_load` an all-`NaN` `g` that the dual solve consumes as if it
    /// were a load — and it was open for exactly the callers who cannot see
    /// assertions.
    ///
    /// # Exactly what the volume guards close, and what they leave open
    ///
    /// This test and
    /// `a_collapsed_element_beside_a_good_one_yields_a_typed_error` between
    /// them close the EXACTLY-COLLAPSED case, `V_K == 0.0` — whether the
    /// ball resolves to only such elements (here, the set-level guard) or
    /// to one standing beside healthy ones (there, the per-element guard).
    ///
    /// They do NOT make the module's "typed error, never a `NaN`" contract
    /// (C4) airtight, and two cases MEASURED in `--release` on this
    /// module's own material and displacement fixtures say so concretely,
    /// so the next reader can re-run them rather than trust the claim:
    ///
    /// * **Merely tiny `det J`.** A unit tet squashed to thickness `1e-12`
    ///   has `V_K = 1.6666666666666667e-13`, strictly positive, and so
    ///   passes both volume guards. `LocalNormalStressQoi::evaluate`
    ///   returns `Ok(-28846153846.04615)` on it — against a fixture stress
    ///   scale of `0.07`. Finite, so not a C4 violation; garbage all the
    ///   same. `element_stress_p1`'s debug-only `MIN_JACOBIAN_DET` floor
    ///   does not catch it either: that floor is `1e-30` and this tet's
    ///   `|det J|` is `1e-12`, twelve orders above it.
    /// * **Element INVERSION.** [`tet_volume_p1`] returns `|det| / 6`, so
    ///   an inverted tet reports the same `0.16666666666666666` as its good
    ///   twin and is indistinguishable BY VOLUME — no volume guard can see
    ///   it. Measured `Ok(0.14423076923076922)` against the good twin's
    ///   `Ok(0.07884615384615386)`: finite, plausible-looking, and wrong.
    ///   That is mesh validity (PRD task #21), not C4.
    #[test]
    fn a_collapsed_element_yields_a_typed_error_rather_than_a_nan_ball_mean() {
        // Four COPLANAR nodes (every z = 0), i.e. a zero-volume "tet".
        let coords = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        let tets = vec![[0, 1, 2, 3]];
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = vec![0.01_f64; 12];
        // The ball is centred ON the element centroid, so the centroid rule
        // selects it and the fallback arm never runs.
        let at = [0.5, 0.5, 0.0];

        assert_eq!(
            tet_volume_p1(&[coords[0], coords[1], coords[2], coords[3]]),
            0.0,
            "fixture premise: the element must be EXACTLY zero-volume, or \
             this exercises a merely ill-conditioned tet instead",
        );

        let expected = QoiError::DegenerateContributingSet {
            at,
            total_volume: 0.0,
        };
        let displacement = LocalDisplacementQoi {
            at,
            radius: 0.5,
            direction: [0.0, -1.0, 0.0],
        };
        assert_eq!(
            displacement.evaluate(mesh, &mat, &u),
            Err(expected.clone()),
            "evaluate must report the degenerate set, not a NaN ball mean",
        );
        assert_eq!(
            displacement.dual_load(mesh, &mat, &u),
            Err(expected.clone()),
            "dual_load must report it from its own direction too (C4), not \
             hand the solver an all-NaN g",
        );

        let stress = LocalNormalStressQoi {
            at,
            radius: 0.5,
            normal: [0.0, -1.0, 0.0],
        };
        assert_eq!(
            stress.evaluate(mesh, &mat, &u),
            Err(expected.clone()),
            "the check lives in the shared `resolve`, so the stress \
             functional reports the same error on the same mesh",
        );
        assert_eq!(
            stress.dual_load(mesh, &mat, &u),
            Err(expected),
            "and from its dual-load direction as well",
        );
    }

    /// A collapsed element sitting BESIDE a good one is a typed error too.
    ///
    /// The `Σ_K V_K` guard above is a SET-level check, so a single
    /// zero-volume tet in a ball that *also* catches healthy tets leaves
    /// the sum strictly positive: the set passes, and every consumer then
    /// goes on to index the broken element. [`element_stress_p1`]'s own
    /// degenerate-Jacobian guard is a `debug_assert!`, so in RELEASE
    /// `inverse_transpose_3x3` divides by `det == 0` and the `NaN` escapes
    /// into the dual load.
    ///
    /// # `LocalNormalStressQoi` is the witnessing arm
    ///
    /// MEASURED on this exact fixture in a `--release` build, before the
    /// per-element guard existed:
    ///
    /// | call | result |
    /// |---|---|
    /// | `LocalNormalStressQoi::evaluate` | `Ok(NaN)` |
    /// | `LocalNormalStressQoi::dual_load` | `Ok(g)`, 12 of 15 entries non-finite |
    /// | `LocalDisplacementQoi::evaluate` | `Ok(-0.0225)` |
    /// | `LocalDisplacementQoi::dual_load` | `Ok(g)`, 0 non-finite entries |
    ///
    /// That mostly-`NaN` `g` is what [`solve_dual_cg`] hands straight to
    /// CG. The displacement ball mean inverts no Jacobian, so its
    /// `0.0 · finite` term genuinely vanishes and it never `NaN`s — which
    /// makes a displacement-only version of this test one that would have
    /// passed VACUOUSLY before the fix. It is asserted anyway, labelled as
    /// the non-witnessing direction, because the guard lives in the shared
    /// [`resolve`] and must reject uniformly across both functionals.
    ///
    /// A plain `#[test]` — no `should_panic`, no reliance on a debug
    /// assertion — so the RELEASE leg of the verify pipeline is the one
    /// that pins this; release callers are exactly the ones who cannot see
    /// the `debug_assert!`. (In debug the pre-fix code panicked at
    /// `result.rs`'s guard instead of returning `Ok(NaN)`. After the fix
    /// `resolve` rejects before either primitive runs, so both profiles
    /// take the same path and this assertion holds in both.)
    #[test]
    fn a_collapsed_element_beside_a_good_one_yields_a_typed_error() {
        // tet0 is the canonical unit tet (V = 1/6). tet1 shares its face
        // {0,1,2} and closes onto node 4, whose z = 0 like theirs — four
        // COPLANAR nodes, so V = 0 exactly.
        let coords = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
        ];
        let tets = vec![[0, 1, 2, 3], [0, 1, 2, 4]];
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = two_tet_fan_u();
        // Both centroids — (0.25, 0.25, 0.25) and (0.5, 0.5, 0.0) — sit
        // well inside this ball, so the centroid arm selects BOTH and the
        // `locate_containing_element` fallback never runs.
        let at = [0.4, 0.4, 0.2];
        let radius = 5.0;
        let direction = [0.0, -1.0, 0.0];

        let good = tet_volume_p1(&[coords[0], coords[1], coords[2], coords[3]]);
        let collapsed = tet_volume_p1(&[coords[0], coords[1], coords[2], coords[4]]);
        assert_eq!(
            collapsed, 0.0,
            "fixture premise: element 1 must be EXACTLY zero-volume, or \
             this exercises a merely ill-conditioned tet instead",
        );
        assert!(
            good + collapsed > 0.0,
            "fixture premise: the SET must stay viable (Σ V_K = {} must be \
             > 0), or this exercises the set-level guard rather than the \
             per-element one",
            good + collapsed,
        );

        let expected = QoiError::DegenerateElement {
            at,
            element: 1,
            volume: 0.0,
        };
        let reject = |qoi: &dyn QuantityOfInterest, arm: &str| {
            assert_eq!(
                qoi.evaluate(mesh, &mat, &u),
                Err(expected.clone()),
                "{arm}: evaluate must NAME the collapsed element rather \
                 than average over it",
            );
            assert_eq!(
                qoi.dual_load(mesh, &mat, &u),
                Err(expected.clone()),
                "{arm}: dual_load must reject from its own direction too \
                 (C4), not hand the solver a g carrying non-finite entries",
            );
        };
        reject(
            &LocalNormalStressQoi {
                at,
                radius,
                normal: direction,
            },
            "LocalNormalStress (the witnessing arm)",
        );
        reject(
            &LocalDisplacementQoi {
                at,
                radius,
                direction,
            },
            "LocalDisplacement (the non-witnessing arm)",
        );
    }

    /// The containment fallback can never select a degenerate element — a
    /// RELEASE-only characterization.
    ///
    /// The per-element guard sits where both selection arms funnel into one
    /// `elements` vector, so it covers the fallback too. This test pins the
    /// SEPARATE reason the fallback was already safe, so that a later edit
    /// to either arm cannot widen the surface unnoticed.
    ///
    /// With a lone collapsed element and a radius too small for the
    /// centroid rule, `resolve` falls back to `locate_containing_element`.
    /// [`barycentric_p1`] carries only a `debug_assert!`, so in release its
    /// `det == 0` produces non-finite barycentric coordinates, every
    /// [`point_in_tet_p1`] comparison against a `NaN` is false, and no
    /// element is selected at all. MEASURED: `Err(PointOutsideBody)` from
    /// both functionals.
    ///
    /// The `cfg` is not a convenience. In DEBUG this fixture panics inside
    /// `interpolation.rs`'s guard before any `Result` exists, so no single
    /// assertion is true of both profiles — and release is the profile
    /// whose behaviour needs pinning anyway, being the one with no
    /// assertion to fall back on.
    #[cfg(not(debug_assertions))]
    #[test]
    fn the_containment_fallback_never_selects_a_degenerate_element() {
        let coords = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        let tets = vec![[0, 1, 2, 3]];
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = vec![0.01_f64; 12];
        // The element centroid is (0.5, 0.5, 0.0); this ball misses it by
        // far more than its radius, which is what forces the fallback arm.
        let at = [0.1, 0.1, 0.0];
        let radius = 0.05;
        let direction = [0.0, -1.0, 0.0];

        let expected = QoiError::PointOutsideBody { at };
        assert_eq!(
            LocalDisplacementQoi {
                at,
                radius,
                direction,
            }
            .evaluate(mesh, &mat, &u),
            Err(expected.clone()),
            "the fallback must decline to select the degenerate element, \
             leaving `at` outside every element of this mesh",
        );
        assert_eq!(
            LocalNormalStressQoi {
                at,
                radius,
                normal: direction,
            }
            .evaluate(mesh, &mat, &u),
            Err(expected),
            "and identically for the stress functional — the fallback is \
             in the shared `resolve`",
        );
    }

    /// A non-unit direction states a DIRECTION, not a scale.
    ///
    /// Nothing upstream of this module guarantees the `vec3` a designer
    /// wrote was normalized, and both functionals are pure projections onto
    /// it: an un-normalized direction would scale `J` AND `g` together, so
    /// the resulting error estimate would be wrong by that factor while
    /// still reporting as perfectly valid. `resolve` normalizes instead, so
    /// the premise is discharged rather than assumed.
    ///
    /// `[0, −2, 0]` normalizes to `[0, −1, 0]` EXACTLY in f64 — its norm is
    /// a power of two — which is what lets this be a BITWISE comparison
    /// rather than one carrying a tolerance that could mask a partial fix.
    #[test]
    fn a_non_unit_direction_resolves_to_the_same_functional_as_its_unit_form() {
        let at = FAN_CENTROIDS[0];
        let (j_unit, g_unit) = eval_and_dual_on_fan(
            &LocalDisplacementQoi {
                at,
                radius: 0.44,
                direction: [0.0, -1.0, 0.0],
            },
            "unit direction",
        );
        let (j_scaled, g_scaled) = eval_and_dual_on_fan(
            &LocalDisplacementQoi {
                at,
                radius: 0.44,
                direction: [0.0, -2.0, 0.0],
            },
            "the same direction, twice as long",
        );
        assert_eq!(
            j_scaled.to_bits(),
            j_unit.to_bits(),
            "J must be the ball mean along the direction, not one scaled by \
             ‖d‖; got {j_scaled} against {j_unit}",
        );
        assert_eq!(
            g_scaled, g_unit,
            "and the dual load must be scale-free for the same reason — a \
             scaled g solves to a scaled adjoint field",
        );

        // `n·σ·n` is QUADRATIC in ‖n‖, so an un-normalized normal would be
        // off by FOUR here rather than two.
        let u = unit_cube_nonuniform_u();
        let (s_unit, _) = eval_and_dual_on_cube(
            &LocalNormalStressQoi {
                at: [0.5, 0.5, 0.5],
                radius: 0.4,
                normal: [0.0, -1.0, 0.0],
            },
            &u,
            "unit normal",
        );
        let (s_scaled, _) = eval_and_dual_on_cube(
            &LocalNormalStressQoi {
                at: [0.5, 0.5, 0.5],
                radius: 0.4,
                normal: [0.0, -2.0, 0.0],
            },
            &u,
            "the same normal, twice as long",
        );
        assert_eq!(
            s_scaled.to_bits(),
            s_unit.to_bits(),
            "the normal-stress mean must not scale with ‖n‖² either; got \
             {s_scaled} against {s_unit}",
        );
    }

    // ── LocalNormalStressQoi (step-5) ──────────────────────────────────────

    /// The 8 corners of the unit cube, indexed so corner `i + 2j + 4k` sits
    /// at `(i, j, k)`.
    fn unit_cube_coords() -> Vec<[f64; 3]> {
        let mut c = Vec::with_capacity(8);
        for k in 0..2 {
            for j in 0..2 {
                for i in 0..2 {
                    c.push([i as f64, j as f64, k as f64]);
                }
            }
        }
        c
    }

    /// The unit cube Kuhn-split into 6 P1 tets, each of volume 1/6.
    ///
    /// Each tet is one monotone corner-0-to-corner-7 path, so tet `t` is the
    /// region where the coordinates are sorted in that path's order — tet 0
    /// is `1 ≥ x ≥ y ≥ z ≥ 0`, tet 1 is `1 ≥ x ≥ z ≥ y ≥ 0`, and so on.
    /// That makes "which element contains this point" answerable by
    /// inspection, which is what the fallback configuration below relies on.
    ///
    /// A box rather than the 2-tet fan because the normal-stress functional
    /// is a *gradient* quantity: it needs elements of several distinct
    /// orientations before a transposed or mis-strided extraction has
    /// anywhere to hide.
    fn unit_cube_kuhn_tets() -> Vec<[usize; 4]> {
        vec![
            [0, 1, 3, 7],
            [0, 1, 5, 7],
            [0, 2, 3, 7],
            [0, 2, 6, 7],
            [0, 4, 5, 7],
            [0, 4, 6, 7],
        ]
    }

    /// Centroid of each tet of [`unit_cube_kuhn_tets`], in element order.
    ///
    /// All six lie at the same distance `√0.125 ≈ 0.354` from the cube
    /// centre, by the symmetry of the Kuhn split.
    const CUBE_CENTROIDS: [[f64; 3]; 6] = [
        [0.75, 0.5, 0.25],
        [0.75, 0.25, 0.5],
        [0.5, 0.75, 0.25],
        [0.25, 0.75, 0.5],
        [0.5, 0.25, 0.75],
        [0.25, 0.5, 0.75],
    ];

    /// `u(x) = (a·x, 0, 0)` sampled at `coords`: the uniaxial-strain patch
    /// field whose closed-form stress `element_stress_p1` already pins.
    fn uniaxial_strain_u(coords: &[[f64; 3]], a: f64) -> Vec<f64> {
        let mut u = vec![0.0_f64; 3 * coords.len()];
        for (n, c) in coords.iter().enumerate() {
            u[3 * n] = a * c[0];
        }
        u
    }

    /// A deliberately NON-linear nodal field over the cube's 8 nodes.
    ///
    /// Non-linear matters: a linear field has a globally constant gradient,
    /// so every element sees the *same* σ, and an extraction that read the
    /// wrong element — or strided node/component the wrong way within an
    /// element — could still agree with `evaluate`. With this field each
    /// element's σ differs, so the contraction test has something to catch.
    fn unit_cube_nonuniform_u() -> Vec<f64> {
        vec![
            0.000, 0.000, 0.000, // node 0
            0.031, -0.012, 0.024, // node 1
            -0.018, 0.045, -0.007, // node 2
            0.052, 0.009, 0.038, // node 3
            0.014, -0.033, 0.021, // node 4
            -0.026, 0.017, 0.049, // node 5
            0.043, 0.028, -0.015, // node 6
            -0.009, 0.036, 0.011, // node 7
        ]
    }

    /// `(J(u_h), g)` for `qoi` on the Kuhn cube with the supplied field.
    fn eval_and_dual_on_cube(
        qoi: &dyn QuantityOfInterest,
        u: &[f64],
        case: &str,
    ) -> (f64, Vec<f64>) {
        let coords = unit_cube_coords();
        let tets = unit_cube_kuhn_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let j = qoi
            .evaluate(mesh, &mat, u)
            .unwrap_or_else(|e| panic!("{case}: evaluate must resolve, got {e}"));
        let g = qoi
            .dual_load(mesh, &mat, u)
            .unwrap_or_else(|e| panic!("{case}: dual_load must resolve, got {e}"));
        (j, g)
    }

    /// Three ball configurations selecting very different contributing sets
    /// on the Kuhn cube: `(at, radius, label)`.
    ///
    /// All six elements; exactly one element by the centroid rule; and one
    /// element via the step-4 fallback arm (`at` is strictly inside tet 0,
    /// since `1 > 0.8 > 0.5 > 0.2 > 0`, and the nearest centroid is ≈0.071
    /// away — well outside the 0.01 ball).
    const CUBE_BALLS: [([f64; 3], f64, &str); 3] = [
        ([0.5, 0.5, 0.5], 0.4, "all six elements"),
        ([0.75, 0.5, 0.25], 0.05, "tet0 alone, by centroid"),
        ([0.8, 0.5, 0.2], 0.01, "tet0 alone, via the fallback arm"),
    ];

    /// The Lamé constants of the shared test material, recomputed from `E`
    /// and `ν` exactly as `result.rs`'s and `constitutive.rs`'s patch tests
    /// do — independently of `d_matrix`, so this test can disagree with it.
    fn lame() -> (f64, f64) {
        let mat = dimensionless_steel_like();
        let nu = mat.poisson_ratio;
        let factor = mat.youngs_modulus / ((1.0 + nu) * (1.0 - 2.0 * nu));
        (factor * nu, factor * (1.0 - 2.0 * nu))
    }

    /// On a uniform-strain field, `LocalNormalStressQoi` recovers the Lamé
    /// diagonal exactly — whichever elements the ball selects.
    ///
    /// `u(x) = (a·x, 0, 0)` gives `σ = diag((λ+2μ)a, λa, λa)` in every
    /// element (`element_stress_p1`'s own patch test,
    /// `element_stress_p1_uniaxial_strain_patch_test_recovers_lame_diagonal`,
    /// pins that closed form and the Voigt layout behind it). A
    /// volume-weighted mean of a constant is that constant, so the ball mean
    /// must lift the closed form through unchanged — which is why the same
    /// three expected values hold across all three contributing sets, and
    /// why disagreement between the configurations would localise the bug to
    /// the weighting rather than to the stress kernel.
    ///
    /// `n·σ·n` for the three axis normals reads off the diagonal directly,
    /// so this also pins that `evaluate` contracts σ with `n ⊗ n` rather
    /// than, say, taking a trace or a von Mises norm.
    ///
    /// # TDD red→green
    ///
    /// **RED** (step-5): `LocalNormalStressQoi` does not exist, so this fails
    /// to COMPILE. **GREEN** (step-6).
    #[test]
    fn local_normal_stress_recovers_the_lame_diagonal_on_a_uniform_strain_field() {
        let a = 0.01_f64;
        let coords = unit_cube_coords();
        let u = uniaxial_strain_u(&coords, a);
        let (lambda, two_mu) = lame();

        for (at, radius, label) in CUBE_BALLS {
            for (normal, expected, which) in [
                ([1.0, 0.0, 0.0], (lambda + two_mu) * a, "σ_xx = (λ+2μ)a"),
                ([0.0, 1.0, 0.0], lambda * a, "σ_yy = λa"),
                ([0.0, 0.0, 1.0], lambda * a, "σ_zz = λa"),
            ] {
                let case = format!("{label}, {which}");
                let (j, _) = eval_and_dual_on_cube(
                    &LocalNormalStressQoi {
                        at,
                        radius,
                        normal,
                    },
                    &u,
                    &case,
                );
                assert_close(j, expected, 1e-12, CUBE_STRESS_SCALE, &case);
            }
        }
    }

    /// `J(v) = gᵀv` for the normal-stress functional too — the real check
    /// that the twelve-unit-displacement column extraction reproduces the
    /// same linear map `element_stress_p1` applies.
    ///
    /// The extraction recovers `g`'s element row by feeding
    /// `element_stress_p1` the twelve unit element displacements and
    /// contracting each image with `n ⊗ n`. That is exact rather than
    /// approximate only because the map really is linear with no affine
    /// offset. This test is what would catch a transposed or mis-strided
    /// node/component index in that probe loop: a wrong stride still
    /// produces a plausible non-zero `g`, but not one whose contraction with
    /// `u` reproduces `evaluate`.
    ///
    /// Run on the NON-linear field, where each element's σ differs — see
    /// [`unit_cube_nonuniform_u`].
    #[test]
    fn local_normal_stress_evaluate_equals_the_dual_load_contracted_with_the_field() {
        let u = unit_cube_nonuniform_u();
        let third = 1.0 / 3.0;
        let two_thirds = 2.0 / 3.0;

        for (at, radius, label) in CUBE_BALLS {
            for normal in [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [third, two_thirds, two_thirds],
                [-two_thirds, third, -two_thirds],
            ] {
                let case = format!("{label}, n = {normal:?}");
                let (j, g) = eval_and_dual_on_cube(
                    &LocalNormalStressQoi {
                        at,
                        radius,
                        normal,
                    },
                    &u,
                    &case,
                );
                let contracted: f64 = g.iter().zip(&u).map(|(gi, ui)| gi * ui).sum();
                assert_close(
                    contracted,
                    j,
                    1e-12,
                    CUBE_STRESS_SCALE,
                    &format!(
                        "{case}: J(u_h) = gᵀu_h must hold (evaluate is the \
                         expectation, gᵀu_h the measurement)"
                    ),
                );
            }
        }
    }

    /// `dual_load` does not depend on `u` — BITWISE.
    ///
    /// This is the property that lets a caller assemble `g` BEFORE the primal
    /// solve, which is what makes the `f = g` self-dual fixture (BT2)
    /// constructible at all: there is no chicken-and-egg between "solve for
    /// `u_h`" and "build the load". `evaluate`'s `u` argument exists so a
    /// linearized nonlinear functional fits the same seam later; for both
    /// functionals shipped today the dual load ignores it, and the assertion
    /// is bitwise rather than approximate so that a `g` computed by
    /// differencing around `u` could not slip through.
    #[test]
    fn local_normal_stress_dual_load_is_independent_of_the_displacement_field() {
        let coords = unit_cube_coords();
        let tets = unit_cube_kuhn_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let zero = vec![0.0_f64; 3 * coords.len()];
        let nonzero = unit_cube_nonuniform_u();

        for (at, radius, label) in CUBE_BALLS {
            let qoi = LocalNormalStressQoi {
                at,
                radius,
                normal: [0.0, 1.0, 0.0],
            };
            let g_zero = qoi.dual_load(mesh, &mat, &zero).expect("resolves on zero u");
            let g_nonzero = qoi
                .dual_load(mesh, &mat, &nonzero)
                .expect("resolves on a non-zero u");
            assert_eq!(g_zero.len(), g_nonzero.len(), "{label}: length differs");
            for (i, (a, b)) in g_zero.iter().zip(&g_nonzero).enumerate() {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "{label}: g[{i}] depends on u ({a} vs {b}); the dual load \
                     must be assemblable before the primal solve",
                );
            }
        }
    }

    /// C4's positive half for the stress functional, plus `kind()`.
    ///
    /// A silently zero `g` here would be just as fatal as for the
    /// displacement functional: `K z_h = 0` solves to the zero dual field and
    /// reports an error estimate of exactly zero — convergence — on every
    /// mesh.
    #[test]
    fn local_normal_stress_dual_load_is_never_zero_and_reports_its_own_kind() {
        let u = unit_cube_nonuniform_u();
        let third = 1.0 / 3.0;
        let two_thirds = 2.0 / 3.0;

        for (at, radius, label) in CUBE_BALLS {
            for normal in [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [third, two_thirds, two_thirds],
            ] {
                let case = format!("{label}, n = {normal:?}");
                let qoi = LocalNormalStressQoi {
                    at,
                    radius,
                    normal,
                };
                assert_eq!(
                    qoi.kind(),
                    QoiKind::LocalNormalStress,
                    "{case}: kind() is what tells the result layer which \
                     dimension evaluate() returned",
                );
                let (_, g) = eval_and_dual_on_cube(&qoi, &u, &case);
                assert!(
                    g.iter().any(|&x| x != 0.0),
                    "{case}: C4 — g is never zero for a non-zero normal",
                );
                assert!(
                    g.iter().all(|x| x.is_finite()),
                    "{case}: C4 — g never contains NaN or infinity",
                );
            }
        }
    }

    /// BT4 / C4 for `LocalNormalStressQoi`: the SAME typed-error arms as the
    /// displacement functional, from both `evaluate` and `dual_load`.
    ///
    /// Reusing `assert_both_directions_reject` verbatim is the point. Both
    /// functionals validate through one shared `resolve`, so the contract is
    /// enforced in exactly one place — and this test asserts that sameness
    /// rather than restating it: if a future functional grew its own
    /// validation path, only the shared helper's expectations would still
    /// hold here.
    ///
    /// A `normal` with no unit form — zero, or non-finite — maps to
    /// [`QoiError::NonNormalizableDirection`], the same variant a
    /// displacement direction produces: in both cases it is the vector the
    /// functional projects onto that has no direction to give.
    #[test]
    fn local_normal_stress_qoi_returns_typed_error_from_both_directions_for_every_unresolvable_input()
    {
        let good_at = [0.25, 0.25, 0.25];
        let good_normal = [1.0, 0.0, 0.0];

        for bad_radius in [0.0_f64, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_both_directions_reject(
                &LocalNormalStressQoi {
                    at: good_at,
                    radius: bad_radius,
                    normal: good_normal,
                },
                &format!("normal-stress radius = {bad_radius}"),
                |e| {
                    matches!(e, QoiError::NonPositiveRadius { radius }
                             if radius.to_bits() == bad_radius.to_bits())
                },
            );
        }

        for bad_normal in unusable_directions() {
            assert_both_directions_reject(
                &LocalNormalStressQoi {
                    at: good_at,
                    radius: 0.1,
                    normal: bad_normal,
                },
                &format!("normal = {bad_normal:?}"),
                |e| {
                    matches!(e, QoiError::NonNormalizableDirection { direction }
                             if direction_bits(direction) == direction_bits(&bad_normal))
                },
            );
        }

        let far_outside = [100.0, 100.0, 100.0];
        assert_both_directions_reject(
            &LocalNormalStressQoi {
                at: far_outside,
                radius: 0.1,
                normal: good_normal,
            },
            "normal-stress `at` far outside the body",
            |e| *e == QoiError::PointOutsideBody { at: far_outside },
        );
    }


    /// The three [`CUBE_BALLS`] configurations really do select three
    /// different contributing sets.
    ///
    /// Every other cube test loops over all three and expects the same
    /// answer from each. That is only evidence of anything if the three
    /// genuinely differ — on a uniform-strain field, in particular, σ is the
    /// same in every element, so those assertions would pass unchanged even
    /// if all three balls resolved to one identical set. This test is what
    /// makes "a disagreement BETWEEN configurations localises the bug to the
    /// weighting" a true statement about the suite rather than a hope.
    ///
    /// Membership is probed with `LocalDisplacementQoi`, not the stress
    /// functional: its `g_i = (V_K/V_E)·¼·d` is non-zero at every node of
    /// every contributing element for any non-zero `d`, so its support is
    /// exactly the union of those elements' nodes. The stress functional's
    /// support is a SUBSET — an individual probe's contraction can vanish
    /// for a particular normal and node — so it cannot pin membership. Both
    /// share one `resolve`, so what is measured here holds for both.
    #[test]
    fn the_three_cube_ball_configurations_select_three_different_contributing_sets() {
        let coords = unit_cube_coords();
        let tets = unit_cube_kuhn_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = unit_cube_nonuniform_u();

        // Expected support, and how many centroids the ball catches — the
        // latter is what separates "one element by the centroid rule" from
        // "one element via the fallback arm".
        let expected: [(Vec<usize>, usize); 3] = [
            ((0..8).collect(), 6),
            (vec![0, 1, 3, 7], 1),
            (vec![0, 1, 3, 7], 0),
        ];

        for ((at, radius, label), (support, n_centroids)) in CUBE_BALLS.iter().zip(expected) {
            let caught = CUBE_CENTROIDS
                .iter()
                .filter(|c| dist(*at, **c) <= *radius)
                .count();
            assert_eq!(
                caught, n_centroids,
                "{label}: expected the ball to catch {n_centroids} element \
                 centroid(s), it catches {caught}",
            );

            let g = LocalDisplacementQoi {
                at: *at,
                radius: *radius,
                direction: [0.0, -1.0, 0.0],
            }
            .dual_load(mesh, &mat, &u)
            .unwrap_or_else(|e| panic!("{label}: must resolve, got {e}"));
            assert_eq!(
                dual_load_support(&g),
                support,
                "{label}: contributing set differs from the one its label claims",
            );
        }
    }

}
