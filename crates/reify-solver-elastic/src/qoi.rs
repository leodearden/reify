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
