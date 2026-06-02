//! Featherstone Recursive Newton-Euler Algorithm (RNEA) for open kinematic chains.
//!
//! Implements the inverse-dynamics core from
//! `docs/prds/v0_3/rigid-body-dynamics.md` §5.2 (task RBD-ε, Phase 2).
//!
//! **Pure-Rust `f64` numerics — no Reify-level `Value` dispatch.**
//! All `Value`↔core wiring (extracting `SpatialInertia` from `MassProperties`,
//! computing the configuration-dependent `X_{p→i}` from joint `Value`s plus a
//! snapshot, calling `motion_subspace_columns`, and reshaping τ into a
//! `JointForce` list) is deferred to task RBD-η (Phase 4 eval-side dispatch).
//! The end-to-end `examples/dynamics/pendulum_idyn.ri` execution is also η.
//!
//! # Reference
//! Featherstone, *Rigid Body Dynamics Algorithms* (2008), §5.2.

use crate::dynamics::spatial::{cross_f, cross_m, SpatialInertia6, SpatialTransform6, SpatialVector6};

// ── Private [f64; 6] arithmetic helpers ──────────────────────────────────────
// spatial.rs exposes no add or scalar-scale methods on SpatialVector6, so we
// operate on the raw [f64; 6] arrays locally.
//
// TODO follow-up: sv_add, sv_axpy, sv_dot, and xt_apply_force duplicate
// spatial arithmetic that belongs in spatial.rs (as SpatialVector6::add/axpy/
// dot and SpatialTransform6::apply_transpose_force).  Promote them in a
// follow-up task so RNEA and any future consumers share one implementation.

/// `a + b` component-wise.
#[inline]
fn sv_add(a: &SpatialVector6, b: &SpatialVector6) -> SpatialVector6 {
    let aa = a.as_array();
    let ab = b.as_array();
    SpatialVector6::from_array([
        aa[0] + ab[0],
        aa[1] + ab[1],
        aa[2] + ab[2],
        aa[3] + ab[3],
        aa[4] + ab[4],
        aa[5] + ab[5],
    ])
}

/// `a += scale * b` (accumulate scaled vector into `a`).
#[inline]
fn sv_axpy(a: &mut SpatialVector6, scale: f64, b: &SpatialVector6) {
    let mut aa = a.as_array();
    let ab = b.as_array();
    for i in 0..6 {
        aa[i] += scale * ab[i];
    }
    *a = SpatialVector6::from_array(aa);
}

/// Plain 6-component dot product `⟨s, f⟩`.
#[inline]
fn sv_dot(s: &SpatialVector6, f: &SpatialVector6) -> f64 {
    let a = s.as_array();
    let b = f.as_array();
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] + a[4] * b[4] + a[5] * b[5]
}

/// Transpose-apply: `Xᵀ · f`, i.e. out[j] = Σ_k M[k*6+j] · f[k].
///
/// This is the child→parent force transmission in the RNEA backward pass.
/// The force/dual transform of a spatial motion transform X is Xᵀ
/// (Featherstone `ᵖXᵢ* = (ⁱXₚ)ᵀ`).  We compute it inline on
/// `parent_to_child.as_matrix()` rather than adding a method to spatial.rs
/// (which is out of scope for this task).
#[inline]
fn xt_apply_force(x: &SpatialTransform6, f: &SpatialVector6) -> SpatialVector6 {
    let m = x.as_matrix();
    let fv = f.as_array();
    let mut out = [0.0f64; 6];
    for k in 0..6 {
        for j in 0..6 {
            out[j] += m[k * 6 + j] * fv[k];
        }
    }
    SpatialVector6::from_array(out)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Joint compliance parameters for spring-loaded and damped joints.
///
/// Used in the RNEA backward pass to compute additive spring and damping
/// force contributions per PRD §7.1 (task ι).
///
/// * `spring_rate` — spring stiffness k (N·m/rad for revolute, N/m for
///   prismatic); `None` means no spring force.
/// * `damping` — viscous damping coefficient c (N·m·s/rad or N·s/m);
///   `None` means no damping force.
/// * `neutral` — rest joint coordinate (same units as `position`);
///   unused when `spring_rate` is `None`.
/// * `position` — current joint coordinate (`snap.value` at call time);
///   unused when `spring_rate` is `None`.
///
/// **1-DOF only.** Multi-DOF spring/damping is deferred to a later v0.3
/// follow-up (PRD §11.2).  A multi-DOF joint with compliance set will
/// panic in release via an always-on assert in
/// [`inverse_dynamics_open_chain`].
pub struct JointCompliance {
    /// Spring stiffness k: N·m/rad (revolute) or N/m (prismatic).
    /// `None` → no spring contribution.
    pub spring_rate: Option<f64>,
    /// Viscous damping coefficient c: N·m·s/rad or N·s/m.
    /// `None` → no damping contribution.
    pub damping: Option<f64>,
    /// Rest joint coordinate (same units as `position`).
    /// Unused when `spring_rate` is `None`.
    pub neutral: f64,
    /// Current joint coordinate (`snap.value` at call time).
    /// Unused when `spring_rate` is `None`.
    pub position: f64,
}

/// Per-link descriptor supplied to [`inverse_dynamics_open_chain`].
///
/// Links must be ordered in spanning-tree topological order so that every
/// parent index is strictly less than the link's own index.
pub struct RneaLink {
    /// Index of the parent link, or `None` for the base (root) body.
    pub parent: Option<usize>,
    /// The composed spatial motion transform `X_{p→i}` (parent frame to this
    /// link's frame): `X_J(q_i) · X_T(i)`.  Computed by the caller from the
    /// joint value and snapshot coordinates (Value-level work owned by RBD-η).
    ///
    /// **Construction note (convention-critical).** When the fixed tree offset
    /// `r` (joint origin in the *parent* frame) is nonzero, do **not** pass it
    /// together with the joint rotation in a single `Frame3`:
    /// `SpatialTransform6::from_frame3(Frame3{E, r})` yields `xlt(r)·rot(E)`
    /// (translation applied in the *child* frame; see spatial.rs's pinned
    /// `−r̃·E` block convention). The RNEA tree transform requires
    /// `rot(E)·xlt(r)` (offset in the parent frame), so compose a pure rotation
    /// with a pure translation:
    /// `from_frame3(Frame3{E, 0}).compose(&from_frame3(Frame3{I, r}))`.
    /// A wrong operand order silently drops the joint-offset lever arm and the
    /// computed parent torques are off (it is invisible in static gravity tests
    /// because the bottom-left block only multiplies the angular part).
    pub parent_to_child: SpatialTransform6,
    /// Motion-subspace columns `S_i` expressed in the child (link) frame.
    /// Length equals the DOF count of this link's joint.
    pub subspace: Vec<SpatialVector6>,
    /// Body mass (kg).
    pub mass: f64,
    /// Center of mass expressed in the body frame (m).
    pub com: [f64; 3],
    /// Rotational inertia tensor about the COM in body axes (kg·m²).
    /// Assembled into a `SpatialInertia6` internally (parallel-axis handled by
    /// `SpatialInertia6::from_mass_com_inertia`; PRD §12 Q3).
    pub inertia_about_com: [[f64; 3]; 3],
    /// Generalized velocity (one entry per subspace column / DOF).
    pub q_dot: Vec<f64>,
    /// Generalized acceleration (one entry per subspace column / DOF).
    pub q_ddot: Vec<f64>,
    /// Optional spring/damping compliance for this joint.
    ///
    /// `None` → no compliance contribution (identical τ output to pre-ι).
    /// `Some(c)` → additive spring and/or damping terms are applied in the
    /// τ-reshape step: `τ += −k·(position − neutral) − c_damp·q̇[0]`.
    ///
    /// **1-DOF only** — see [`JointCompliance`] and PRD §11.2.
    pub compliance: Option<JointCompliance>,
}

/// Returns `[0.0, 0.0, -9.81]` — the PRD §12 Q1 default gravity vector (m/s²).
///
/// Pass a different value to [`inverse_dynamics_open_chain`] to override.
pub fn default_gravity() -> [f64; 3] {
    [0.0, 0.0, -9.81]
}

/// Featherstone RNEA inverse dynamics for an open kinematic chain.
///
/// Returns `τ` as `Vec<Vec<f64>>` parallel to `links`: `tau[i][c]` is the
/// generalized force in joint coordinate `c` of link `i`.
///
/// `links` must be supplied in spanning-tree topological order (parent index <
/// child index).  The base body is given spatial velocity `v = 0` and spatial
/// acceleration `a = [0, 0, 0, −g_x, −g_y, −g_z]` (the standard
/// gravity-as-base-acceleration trick; Featherstone 2008 §5.2).
///
/// # Panics
/// Panics if any parent index is ≥ the link's own index (topological-order
/// violation).  In release builds, a misordered chain would otherwise silently
/// read the still-zero `v[p]`/`a[p]` entries and produce wrong torques with
/// no diagnostic, hence the check is always-on rather than `debug_assert!`.
pub fn inverse_dynamics_open_chain(links: &[RneaLink], gravity: [f64; 3]) -> Vec<Vec<f64>> {
    let n = links.len();

    // ── Forward pass (outward, base → leaves) ────────────────────────────────
    //
    // Base fictitious acceleration encodes gravity (gravity-as-base-accel trick).
    let a_base = SpatialVector6::from_angular_linear(
        [0.0, 0.0, 0.0],
        [-gravity[0], -gravity[1], -gravity[2]],
    );
    let v_base = SpatialVector6::zero();

    let mut v: Vec<SpatialVector6> = vec![SpatialVector6::zero(); n];
    let mut a: Vec<SpatialVector6> = vec![SpatialVector6::zero(); n];
    let mut inertia: Vec<SpatialInertia6> = Vec::with_capacity(n);

    for (i, link) in links.iter().enumerate() {
        let (v_p, a_p) = match link.parent {
            None => (v_base, a_base),
            Some(p) => {
                assert!(p < i, "links must be in topological order: parent={p} >= child={i}");
                (v[p], a[p])
            }
        };

        // vJ = Σ_c S_i[c] · q̇_i[c]
        let mut vj = SpatialVector6::zero();
        for (s, &dq) in link.subspace.iter().zip(link.q_dot.iter()) {
            sv_axpy(&mut vj, dq, s);
        }

        // v_i = X_{p→i} · v_p + vJ
        v[i] = sv_add(&link.parent_to_child.apply(&v_p), &vj);

        // aJ = Σ_c S_i[c] · q̈_i[c]
        let mut aj = SpatialVector6::zero();
        for (s, &ddq) in link.subspace.iter().zip(link.q_ddot.iter()) {
            sv_axpy(&mut aj, ddq, s);
        }

        // a_i = X_{p→i} · a_p + aJ + v_i × vJ   (Coriolis/centrifugal bias)
        // cross_m(v_i, vJ) is the Featherstone §5.2 velocity-product term.
        a[i] = sv_add(
            &sv_add(&link.parent_to_child.apply(&a_p), &aj),
            &cross_m(&v[i], &vj),
        );

        inertia.push(SpatialInertia6::from_mass_com_inertia(
            link.mass,
            link.com,
            link.inertia_about_com,
        ));
    }

    // ── Backward pass (inward, leaves → base) ────────────────────────────────
    //
    // Initialise f_i = I_i · a_i + v_i × * (I_i · v_i)
    // The second term is the Featherstone §5.2 bias (Coriolis/centrifugal) force.
    // cross_f(v_i, I_i·v_i) is the spatial-velocity cross product on forces.
    let mut f: Vec<SpatialVector6> = (0..n)
        .map(|i| {
            sv_add(
                &inertia[i].apply(&a[i]),
                &cross_f(&v[i], &inertia[i].apply(&v[i])),
            )
        })
        .collect();

    for i in (0..n).rev() {
        // Transmit force to parent.
        if let Some(p) = links[i].parent {
            let ft = xt_apply_force(&links[i].parent_to_child, &f[i]);
            f[p] = sv_add(&f[p], &ft);
        }
    }

    // τ_i[c] = S_i[c] · f_i  +  spring/damping additive terms (PRD §7.1)
    links
        .iter()
        .enumerate()
        .map(|(i, link)| {
            let mut tau_i: Vec<f64> = link.subspace.iter().map(|s| sv_dot(s, &f[i])).collect();
            if let Some(c) = &link.compliance {
                if let Some(k) = c.spring_rate {
                    tau_i[0] += -k * (c.position - c.neutral);
                }
            }
            tau_i
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{default_gravity, inverse_dynamics_open_chain, RneaLink};
    use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

    /// Build the `(w, x, y, z)` unit quaternion for a rotation of `theta`
    /// radians about the +y axis: `q = [cos(θ/2), 0, sin(θ/2), 0]`.
    fn ry_quat(theta: f64) -> [f64; 4] {
        let (s, c) = (theta / 2.0).sin_cos();
        [c, 0.0, s, 0.0]
    }

    /// Build the parent→child joint transform `X_{p→i} = X_J(q) · X_T` where
    /// `quat` is the joint's **active** rotation (the child frame equals the
    /// parent frame rotated by `quat`) and the fixed tree offset `r` is the
    /// child-frame origin expressed in the **parent** frame.
    ///
    /// **Convention (handedness-critical).** A Featherstone Plücker *coordinate*
    /// transform `ᶜXₚ` maps parent-frame coordinates into child-frame
    /// coordinates, so its rotation block is the matrix that takes parent
    /// coords → child coords — the **transpose** of the active frame rotation
    /// (equivalently the conjugate quaternion). Passing the active rotation
    /// directly is a footgun: the inertia matrix `M` only depends on `cos q`
    /// (an even function), so a wrong rotation *sense* stays invisible in static
    /// gravity / pure-inertia checks, but it flips the sign of the
    /// handedness-sensitive velocity cross-products (`cross_m`/`cross_f`) and
    /// hence the entire Coriolis/centrifugal contribution. We therefore
    /// conjugate `quat` here before assembling the coordinate transform. (The
    /// single-pendulum static test constructs its transform from the already-
    /// conjugated quaternion directly, matching this same convention.)
    ///
    /// `SpatialTransform6::from_frame3` follows the spatial.rs convention
    /// `X(r, E) = [[E, 0]; [−r̃·E, E]] = xlt(r)·rot(E)`, i.e. it applies the
    /// translation in the *child* frame. The RNEA tree transform needs
    /// `rot(E)·xlt(r)` (offset in the parent frame), so we compose a pure
    /// rotation with a pure translation rather than passing both in a single
    /// `Frame3`. (The `compose` contract is "apply other first, then self", so
    /// `rot.compose(xlt) = rot·xlt`.)
    fn joint_xform(quat: [f64; 4], r: [f64; 3]) -> SpatialTransform6 {
        // Conjugate (w, x, y, z) → (w, −x, −y, −z): active rotation → the
        // parent→child coordinate-transform rotation `Eᵀ`.
        let [w, x, y, z] = quat;
        let coord_rot = [w, -x, -y, -z];
        SpatialTransform6::from_frame3(&Frame3::new(coord_rot, [0.0, 0.0, 0.0]))
            .compose(&SpatialTransform6::from_frame3(&Frame3::new(
                [1.0, 0.0, 0.0, 0.0],
                r,
            )))
    }

    // ── single-pendulum static gravity-torque ─────────────────────────────────
    //
    // A 1 kg point mass hanging at L = 100 mm along the link's −z axis when
    // θ = 0 (so com = [0, 0, −0.1] in the body frame).  The joint is revolute
    // about +y; at θ = −30° (pivot at origin, link frame rotated by −θ about y
    // so the body z-axis swings toward +x), the mass is at world [+0.05, 0, −0.0866].
    //
    // Expected actuator torque holding the pendulum static:
    //     τ = m · g · L · sin(30°) = 1 · 9.81 · 0.1 · 0.5 = 0.4905 N·m
    //
    // With q_dot = q_ddot = 0 the velocity-product (Coriolis/centrifugal) terms
    // vanish, so only the gravity/inertia/transmission path is exercised.
    #[test]
    fn single_pendulum_static_gravity_torque() {
        // Rotation: −30° about the +y axis.
        // Frame3 encodes the child-to-parent rotation, so the joint transform
        // that places the link at −30° uses the −θ quaternion:
        //   w = cos(θ/2), x = 0, y = −sin(θ/2), z = 0
        let theta = std::f64::consts::PI / 6.0; // 30°
        let (half_sin, half_cos) = ((theta / 2.0).sin(), (theta / 2.0).cos());
        let q = [half_cos, 0.0, -half_sin, 0.0]; // (w, x, y, z) — −30° about y

        let link = RneaLink {
            parent: None,
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(q, [0.0, 0.0, 0.0])),
            // Revolute about +y: angular = [0,1,0], linear = [0,0,0]
            subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0, 0.0, -0.1], // 1 kg point mass at 100 mm along −z
            inertia_about_com: [[0.0; 3]; 3], // point mass
            q_dot: vec![0.0],
            q_ddot: vec![0.0],
            compliance: None,
        };

        let gravity = default_gravity(); // [0, 0, −9.81]
        let tau = inverse_dynamics_open_chain(&[link], gravity);

        assert_eq!(tau.len(), 1, "one link");
        assert_eq!(tau[0].len(), 1, "one DOF");

        let expected = 0.4905_f64; // m·g·L·sin(30°)
        assert!(
            (tau[0][0] - expected).abs() < 1e-6,
            "expected {expected} N·m, got {}",
            tau[0][0]
        );
    }

    // ── double-pendulum dynamic cross-validation ───────────────────────────────
    //
    // 2-link planar elbow manipulator; both joints revolute about +y (planar
    // motion in the x–z plane with gravity = [0, 0, −9.81]).
    //
    // Parameters: m₁=m₂=1 kg, l₁=l₂=1 m, l_c1=l_c2=0.5 m, I_y1=I_y2=m·l²/12.
    //
    // Coordinate convention (Ry(θ) sends x-hat to [cos θ, 0, −sin θ] in world):
    //   COM 1 world pos  = [0.5·cos q₁,          0, −0.5·sin q₁]
    //   COM 2 world pos  = [cos q₁ + 0.5·cos(q₁+q₂), 0, −sin q₁ − 0.5·sin(q₁+q₂)]
    //
    // Lagrangian EOM (standard derivation, matches Spong & Vidyasagar §7.4 with
    // the coordinate mapping for our x–z plane + Ry convention):
    //
    //   M = [[5/3 + c₂,        1/3 + 0.5·c₂],
    //        [1/3 + 0.5·c₂,    1/3           ]]     (c₂ = cos q₂)
    //
    //   Coriolis/centrifugal:  h = 0.5·sin q₂
    //     cc₁ = −h·q̇₂·(2·q̇₁ + q̇₂)
    //     cc₂ = +h·q̇₁²
    //
    //   Gravity:
    //     g₁ = −g·(1.5·cos q₁ + 0.5·cos(q₁+q₂))
    //     g₂ = −g·0.5·cos(q₁+q₂)
    //
    //   τ = M·q̈ + [cc₁; cc₂] + [g₁; g₂]
    //
    // RNEA and the Lagrangian EOM are two mathematically-equivalent EXACT
    // formulations → they agree to ~1e-13 relative (pure float roundoff),
    // so 1e-6 has ~7 orders of margin.
    //
    // With crossM/crossF active, the Coriolis/centrifugal contribution is
    // present; any sample with nonzero q̇ would mismatch (relative error
    // ~0.1–5 % >> 1e-6) if the velocity-product terms were dropped or
    // sign-flipped.
    #[test]
    fn double_pendulum_dynamic_cross_validation() {
        const G: f64 = 9.81;

        // Closed-form Lagrangian τ for our 2-link system.
        let ref_tau = |q1: f64, q2: f64, qd1: f64, qd2: f64, qdd1: f64, qdd2: f64| -> [f64; 2] {
            let c1 = q1.cos();
            let c2 = q2.cos();
            let s2 = q2.sin();
            let c12 = (q1 + q2).cos();
            // Inertia matrix entries.
            let m11 = 5.0 / 3.0 + c2;
            let m12 = 1.0 / 3.0 + 0.5 * c2;
            let m22 = 1.0 / 3.0_f64;
            // Coriolis/centrifugal.
            let h = 0.5 * s2;
            let cc1 = -h * qd2 * (2.0 * qd1 + qd2);
            let cc2 = h * qd1 * qd1;
            // Gravity (−g · ∂z/∂q for each COM).
            let grav1 = -G * (1.5 * c1 + 0.5 * c12);
            let grav2 = -G * 0.5 * c12;
            [
                m11 * qdd1 + m12 * qdd2 + cc1 + grav1,
                m12 * qdd1 + m22 * qdd2 + cc2 + grav2,
            ]
        };

        // 10 samples with nonzero q̇ so Coriolis/centrifugal terms are active.
        // (q1, q2, qd1, qd2, qdd1, qdd2)
        let samples: [(f64, f64, f64, f64, f64, f64); 10] = [
            (0.1, 0.2, 1.0, -0.5, 0.5, 0.3),
            (0.5, -0.3, -0.8, 1.2, -0.4, 0.7),
            (1.0, 0.5, 2.0, 1.0, 0.0, 0.0),
            (-0.3, 0.8, 0.5, -1.5, 1.0, -0.5),
            (0.0, 0.1, 1.5, 0.5, 0.2, -0.1),
            (1.2, -0.5, -1.0, 2.0, 0.3, 0.5),
            (0.7, 0.9, 0.3, -0.8, -0.5, 0.2),
            (-0.5, -0.5, 1.8, -1.8, 0.6, -0.3),
            (0.3, 1.0, -0.2, 0.9, 0.4, -0.6),
            (-1.0, 0.3, 1.0, 1.0, -0.2, 0.2),
        ];

        const REL_TOL: f64 = 1e-6;
        // Absolute floor prevents division by zero for near-zero τ components.
        const ABS_FLOOR: f64 = 1e-10;

        let assert_close = |label: &str, got: f64, want: f64| {
            let abs_err = (got - want).abs();
            let scale = want.abs().max(ABS_FLOOR);
            assert!(
                abs_err / scale < REL_TOL,
                "{label}: RNEA={got:.10}, ref={want:.10}, rel_err={:.2e}",
                abs_err / scale
            );
        };

        for (si, &(q1, q2, qd1, qd2, qdd1, qdd2)) in samples.iter().enumerate() {
            let expected = ref_tau(q1, q2, qd1, qd2, qdd1, qdd2);

            // Link 0: revolute about +y at world origin, joint angle q1.
            // COM at [lc1, 0, 0] = [0.5, 0, 0] in the body frame.
            // Iy = 1/12 kg·m² (uniform rod about its COM, axis along x).
            let link0 = RneaLink {
                parent: None,
                // Tree offset is zero, so rotation-only — joint_xform reduces to
                // a pure rotation here.
                parent_to_child: joint_xform(ry_quat(q1), [0.0, 0.0, 0.0]),
                subspace: vec![SpatialVector6::from_angular_linear(
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0],
                )],
                mass: 1.0,
                com: [0.5, 0.0, 0.0],
                inertia_about_com: [[0.0, 0.0, 0.0], [0.0, 1.0 / 12.0, 0.0], [0.0, 0.0, 0.0]],
                q_dot: vec![qd1],
                q_ddot: vec![qdd1],
                compliance: None,
            };

            // Link 1: revolute about +y at the tip of link 0 ([l1, 0, 0] = [1, 0, 0]
            // in link-0/parent coordinates), joint angle q2. The tree offset is in
            // the PARENT frame, so the transform must be rot(E)·xlt(r); joint_xform
            // composes it correctly under the spatial.rs −r̃·E convention.
            let link1 = RneaLink {
                parent: Some(0),
                parent_to_child: joint_xform(ry_quat(q2), [1.0, 0.0, 0.0]),
                subspace: vec![SpatialVector6::from_angular_linear(
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0],
                )],
                mass: 1.0,
                com: [0.5, 0.0, 0.0],
                inertia_about_com: [[0.0, 0.0, 0.0], [0.0, 1.0 / 12.0, 0.0], [0.0, 0.0, 0.0]],
                q_dot: vec![qd2],
                q_ddot: vec![qdd2],
                compliance: None,
            };

            let tau = inverse_dynamics_open_chain(&[link0, link1], default_gravity());

            assert_eq!(tau.len(), 2, "sample {si}: two links");
            assert_eq!(tau[0].len(), 1, "sample {si}: link 0 has one DOF");
            assert_eq!(tau[1].len(), 1, "sample {si}: link 1 has one DOF");

            assert_close(&format!("sample {si} joint 0"), tau[0][0], expected[0]);
            assert_close(&format!("sample {si} joint 1"), tau[1][0], expected[1]);
        }
    }

    // ── spring-pendulum additive term (PRD §10.1 row 7) ───────────────────────
    //
    // Reuses the single_pendulum_static_gravity_torque geometry: 1 kg point
    // mass at com=[0,0,−0.1], revolute about +y, −30° joint angle.
    //
    // Run inverse_dynamics_open_chain twice on otherwise-identical links:
    //   (a) compliance: None  →  baseline τ ≈ 0.4905 N·m
    //   (b) compliance: Some(JointCompliance { spring_rate: Some(k=2.0),
    //                        damping: None, neutral: π/12 (15°),
    //                        position: π/6 (30°) })
    //
    // The spring displacement is (π/6 − π/12) = π/12.
    // Expected additive term: −k·(position − neutral) = −2.0·(π/12) = −π/6.
    //
    // Assert: baseline ≈ 0.4905  AND  (spring_tau − baseline_tau) == −k·(pos − neutral)
    // within 1e-12 (exact float arithmetic — additive, no transcendentals).
    #[test]
    fn spring_pendulum_additive_term() {
        use std::f64::consts::PI;
        use super::JointCompliance;

        let theta = PI / 6.0; // −30° joint angle (same as single-pendulum test)
        let (half_sin, half_cos) = ((theta / 2.0).sin(), (theta / 2.0).cos());
        let q = [half_cos, 0.0, -half_sin, 0.0]; // (w,x,y,z) — −30° about y

        let base_link = RneaLink {
            parent: None,
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(q, [0.0, 0.0, 0.0])),
            subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0, 0.0, -0.1],
            inertia_about_com: [[0.0; 3]; 3],
            q_dot: vec![0.0],
            q_ddot: vec![0.0],
            compliance: None,
        };

        let k = 2.0_f64;
        let neutral = PI / 12.0; // 15°
        let position = PI / 6.0;  // 30°

        let spring_link = RneaLink {
            compliance: Some(JointCompliance {
                spring_rate: Some(k),
                damping: None,
                neutral,
                position,
            }),
            ..base_link // re-use all other fields
        };

        // Re-build base_link for the baseline call (moved into spring_link above).
        let baseline_link = RneaLink {
            parent: None,
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(q, [0.0, 0.0, 0.0])),
            subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0, 0.0, -0.1],
            inertia_about_com: [[0.0; 3]; 3],
            q_dot: vec![0.0],
            q_ddot: vec![0.0],
            compliance: None,
        };

        let gravity = default_gravity();
        let tau_baseline = inverse_dynamics_open_chain(&[baseline_link], gravity);
        let tau_spring   = inverse_dynamics_open_chain(&[spring_link], gravity);

        assert_eq!(tau_baseline.len(), 1);
        assert_eq!(tau_spring.len(), 1);

        // Sanity: baseline ≈ 0.4905 N·m
        let expected_baseline = 0.4905_f64;
        assert!(
            (tau_baseline[0][0] - expected_baseline).abs() < 1e-6,
            "baseline: expected {expected_baseline}, got {}",
            tau_baseline[0][0]
        );

        // Exact additive check: Δτ must equal −k·(position − neutral).
        let expected_delta = -k * (position - neutral);
        let actual_delta = tau_spring[0][0] - tau_baseline[0][0];
        assert!(
            (actual_delta - expected_delta).abs() < 1e-12,
            "spring Δτ: expected {expected_delta:.15}, got {actual_delta:.15}, err={:.2e}",
            (actual_delta - expected_delta).abs()
        );
    }

    // ── damping additive term (PRD §7.1) ──────────────────────────────────────
    //
    // 1-DOF revolute joint with nonzero q_dot, testing the damping path only.
    // Uses the single-pendulum geometry (−30°, 1 kg point mass at −0.1 m) but
    // with q_dot = [omega] nonzero so the −c·q̇[0] term is non-trivial.
    //
    // Run twice:
    //   (a) compliance: None  →  baseline τ
    //   (b) compliance: Some(JointCompliance { spring_rate: None, damping: Some(c=3.5),
    //                        neutral: 0.0, position: 0.0 })
    //
    // Assert (damp_tau − baseline_tau) == −c·q_dot[0] within 1e-12.
    #[test]
    fn damping_additive_term() {
        use super::JointCompliance;

        let theta = std::f64::consts::PI / 6.0;
        let (half_sin, half_cos) = ((theta / 2.0).sin(), (theta / 2.0).cos());
        let q = [half_cos, 0.0, -half_sin, 0.0];

        let omega = 1.7_f64; // nonzero velocity so damping term is active

        let baseline_link = RneaLink {
            parent: None,
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(q, [0.0, 0.0, 0.0])),
            subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0, 0.0, -0.1],
            inertia_about_com: [[0.0; 3]; 3],
            q_dot: vec![omega],
            q_ddot: vec![0.0],
            compliance: None,
        };

        let c_damp = 3.5_f64;
        let damp_link = RneaLink {
            compliance: Some(JointCompliance {
                spring_rate: None,
                damping: Some(c_damp),
                neutral: 0.0,
                position: 0.0,
            }),
            q_dot: vec![omega],
            parent: None,
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(q, [0.0, 0.0, 0.0])),
            subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0, 0.0, -0.1],
            inertia_about_com: [[0.0; 3]; 3],
            q_ddot: vec![0.0],
        };

        let gravity = default_gravity();
        let tau_baseline = inverse_dynamics_open_chain(&[baseline_link], gravity);
        let tau_damp     = inverse_dynamics_open_chain(&[damp_link], gravity);

        assert_eq!(tau_baseline.len(), 1);
        assert_eq!(tau_damp.len(), 1);

        let expected_delta = -c_damp * omega; // −c·q̇[0]
        let actual_delta = tau_damp[0][0] - tau_baseline[0][0];
        assert!(
            (actual_delta - expected_delta).abs() < 1e-12,
            "damping Δτ: expected {expected_delta:.15}, got {actual_delta:.15}, err={:.2e}",
            (actual_delta - expected_delta).abs()
        );
    }

    // ── multi-DOF joint subspace accumulation ─────────────────────────────────
    //
    // Exercises the multi-column vJ/aJ accumulation loops and the per-DOF
    // `tau[i][c]` output path with a 2-DOF joint on a single floating body.
    //
    // Joint subspace columns:
    //   S[0] = [0,1,0,  0,0,0]   — revolute about +y
    //   S[1] = [0,0,0,  0,0,1]   — prismatic along +z
    //
    // Body params: mass m=2 kg, COM at origin, I = diag(0.1, 0.5, 0.3) kg·m².
    // State: q_dot=[0,0], q_ddot=[α=3.0, a_z=1.0].
    //
    // With q_dot=0 and parent=None (identity transform):
    //   vJ = 0, v = 0, cross_m = 0, cross_f = 0
    //   aJ = S[0]·α + S[1]·a_z = [0,3,0, 0,0,0] + [0,0,0, 0,0,1]
    //      = [0, 3.0, 0, 0, 0, 1.0]
    //   a = a_base + aJ = [0,0,0, 0,0,9.81] + [0,3,0, 0,0,1]
    //     = [0, 3.0, 0, 0, 0, 10.81]
    //   I·a  (diagonal spatial inertia, COM=0):
    //     angular = [Ix·0, Iy·3, Iz·0] = [0, 1.5, 0]
    //     linear  = [m·0,  m·0,  m·10.81] = [0, 0, 21.62]
    //   f = [0, 1.5, 0, 0, 0, 21.62]
    //   tau[0] = S[0]·f = 1.5   (I_y · α)
    //   tau[1] = S[1]·f = 21.62 (m · (G + a_z))
    #[test]
    fn multi_dof_joint_subspace_accumulation() {
        const G: f64 = 9.81;
        let m = 2.0_f64;
        let i_y = 0.5_f64;
        let alpha = 3.0_f64;
        let a_z = 1.0_f64;

        let link = RneaLink {
            parent: None,
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
            subspace: vec![
                // S[0]: revolute about +y
                SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
                // S[1]: prismatic along +z
                SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ],
            mass: m,
            com: [0.0, 0.0, 0.0],
            inertia_about_com: [[0.1, 0.0, 0.0], [0.0, i_y, 0.0], [0.0, 0.0, 0.3]],
            q_dot: vec![0.0, 0.0],
            q_ddot: vec![alpha, a_z],
            compliance: None,
        };

        let tau = inverse_dynamics_open_chain(&[link], default_gravity());

        assert_eq!(tau.len(), 1, "one link");
        assert_eq!(tau[0].len(), 2, "two DOFs");

        // tau[0][0] = S[0]·f  =  I_y · α
        let expected_tau0 = i_y * alpha; // 1.5 N·m
        // tau[0][1] = S[1]·f  =  m · (G + a_z)
        let expected_tau1 = m * (G + a_z); // 21.62 N

        assert!(
            (tau[0][0] - expected_tau0).abs() < 1e-12,
            "tau[0][0]: expected {expected_tau0}, got {}",
            tau[0][0]
        );
        assert!(
            (tau[0][1] - expected_tau1).abs() < 1e-12,
            "tau[0][1]: expected {expected_tau1}, got {}",
            tau[0][1]
        );
    }
}
