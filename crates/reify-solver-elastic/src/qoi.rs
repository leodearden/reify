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

use crate::constitutive::IsotropicElastic;
use crate::result::tet_volume_p1;

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
    /// The direction (or stress normal) has zero length, so `d · u` — and
    /// hence the dual load — would be identically zero.
    ZeroDirection,
    /// No element centroid lies within the ball, and `at` itself lies inside
    /// no element of this mesh.
    PointOutsideBody {
        /// The offending query point.
        at: [f64; 3],
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
            QoiError::ZeroDirection => write!(
                f,
                "quantity-of-interest direction has zero length; the \
                 functional and its dual load would both be identically zero",
            ),
            QoiError::PointOutsideBody { at } => write!(
                f,
                "quantity-of-interest point ({}, {}, {}) lies outside every \
                 element of this mesh",
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
/// Constructed only by [`resolve`], which guarantees the invariant every
/// consumer divides by: `elements` is non-empty and `total_volume` is
/// strictly positive.
struct ContributingSet {
    /// Contributing elements in ascending element index.
    elements: Vec<ContributingElement>,
    /// `V_E = Σ_K V_K` over `elements`.
    total_volume: f64,
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
///    [`QoiError::ZeroDirection`].
/// 3. `debug_assert!` that `direction` is unit length. Directions are
///    normalized by the extractor, so this is a *caller precondition*, not a
///    runtime branch. It must come AFTER check 2: a zero vector has to reach
///    the typed error rather than trip this assertion, or C4's
///    `direction = vec3(0,0,0)` case is unpassable in a debug build.
/// 4. `E = { K : ‖centroid(K) − at‖ ≤ radius }`, in ascending element index.
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
/// [`QoiError`] per the check order above. An empty `E` is
/// [`QoiError::PointOutsideBody`]; step-4 adds the `locate_element_p1`
/// fallback arm ahead of that.
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
        return Err(QoiError::ZeroDirection);
    }
    debug_assert!(
        (norm_sq.sqrt() - 1.0).abs() <= 1e-9,
        "quantity-of-interest direction must be unit length (the extractor \
         normalizes it); got ‖d‖ = {}",
        norm_sq.sqrt(),
    );

    let radius_sq = radius * radius;
    let mut elements = Vec::new();
    let mut total_volume = 0.0_f64;
    for (index, tet) in mesh.tets.iter().enumerate() {
        let nodes = tet_nodes(mesh, tet);
        let c = centroid(&nodes);
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
        return Err(QoiError::PointOutsideBody { at });
    }

    debug_assert!(
        total_volume > 0.0,
        "contributing set of {} element(s) has non-positive total volume {} \
         — the mesh has degenerate tets near {:?}",
        elements.len(),
        total_volume,
        at,
    );
    Ok(ContributingSet {
        elements,
        total_volume,
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

/// Arithmetic mean of a tet's four corners — its centroid, since the P1
/// barycentric coordinates there are `(¼, ¼, ¼, ¼)`.
#[inline]
fn centroid(nodes: &[[f64; 3]; 4]) -> [f64; 3] {
    let mut c = [0.0_f64; 3];
    for n in nodes {
        for k in 0..3 {
            c[k] += n[k];
        }
    }
    for cell in &mut c {
        *cell *= 0.25;
    }
    c
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
#[derive(Debug, Clone, PartialEq)]
pub struct LocalDisplacementQoi {
    /// Ball centre, in the same coordinates as `P1TetMeshRef::coords`.
    pub at: [f64; 3],
    /// Ball radius. Required, finite and strictly positive (C4).
    pub radius: f64,
    /// Unit direction the displacement is projected onto. Normalized by the
    /// caller; a zero vector is [`QoiError::ZeroDirection`].
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
                    q += self.direction[k] * u[3 * node + k];
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
                    g[3 * node + k] += w * self.direction[k];
                }
            }
        }
        Ok(g)
    }

    fn kind(&self) -> QoiKind {
        QoiKind::LocalDisplacement
    }
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
    /// (shares face {1,2,3} with tet0). Both tets have volume 1/6.
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
    /// * `direction = [0,0,0]` → [`QoiError::ZeroDirection`].
    /// * `at` outside every element, with a radius too small to catch any
    ///   centroid → [`QoiError::PointOutsideBody`].
    ///
    /// # The zero-direction case runs without `#[should_panic]` deliberately
    ///
    /// Directions are normalized by the extractor, and `resolve` asserts
    /// unit length as a caller precondition — but that assertion is a
    /// `debug_assert!`, and this test runs with `debug_assertions` on. A zero
    /// vector must therefore reach the typed error *before* the unit-length
    /// assertion fires. If the check order were reversed, this test would
    /// panic rather than fail an assertion, which is why the ordering inside
    /// `resolve` is fixed and documented rather than incidental.
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

        assert_both_directions_reject(
            &LocalDisplacementQoi {
                at: good_at,
                radius: 0.1,
                direction: [0.0, 0.0, 0.0],
            },
            "direction = [0,0,0]",
            |e| *e == QoiError::ZeroDirection,
        );

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
    /// below: adding a third variant makes this function fail to compile.
    /// `kind()` is what tells the result layer which `QoIEstimate` variant —
    /// hence which dimension — `evaluate` returned.
    #[test]
    fn qoi_kind_has_exactly_the_two_shipped_variants_and_local_displacement_reports_its_own() {
        fn label(k: QoiKind) -> &'static str {
            match k {
                QoiKind::LocalDisplacement => "LocalDisplacement",
                QoiKind::LocalNormalStress => "LocalNormalStress",
            }
        }

        assert_eq!(label(QoiKind::LocalDisplacement), "LocalDisplacement");
        assert_eq!(label(QoiKind::LocalNormalStress), "LocalNormalStress");

        let qoi = LocalDisplacementQoi {
            at: [0.25, 0.25, 0.25],
            radius: 0.1,
            direction: [0.0, -1.0, 0.0],
        };
        assert_eq!(qoi.kind(), QoiKind::LocalDisplacement);
    }
}
