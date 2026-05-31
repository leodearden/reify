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

use crate::dynamics::spatial::{SpatialInertia6, SpatialTransform6, SpatialVector6};

// ── Private [f64; 6] arithmetic helpers ──────────────────────────────────────
// spatial.rs exposes no add or scalar-scale methods on SpatialVector6, so we
// operate on the raw [f64; 6] arrays locally.

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
/// Panics in debug builds if any parent index is ≥ the link's own index
/// (would violate topological ordering).
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
                debug_assert!(p < i, "links must be in topological order");
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

        // a_i = X_{p→i} · a_p + aJ   (velocity-product term added in step-4)
        a[i] = sv_add(&link.parent_to_child.apply(&a_p), &aj);

        inertia.push(SpatialInertia6::from_mass_com_inertia(
            link.mass,
            link.com,
            link.inertia_about_com,
        ));
    }

    // ── Backward pass (inward, leaves → base) ────────────────────────────────
    //
    // Initialise f_i = I_i · a_i   (bias-force term added in step-4).
    let mut f: Vec<SpatialVector6> = (0..n).map(|i| inertia[i].apply(&a[i])).collect();

    for i in (0..n).rev() {
        // Transmit force to parent.
        if let Some(p) = links[i].parent {
            let ft = xt_apply_force(&links[i].parent_to_child, &f[i]);
            f[p] = sv_add(&f[p], &ft);
        }
    }

    // τ_i[c] = S_i[c] · f_i
    links
        .iter()
        .enumerate()
        .map(|(i, link)| link.subspace.iter().map(|s| sv_dot(s, &f[i])).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{default_gravity, inverse_dynamics_open_chain, RneaLink};
    use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

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
}
