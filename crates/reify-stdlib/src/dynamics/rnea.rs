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

#[cfg(test)]
mod tests {
    use super::{default_gravity, inverse_dynamics_open_chain, RneaLink};
    use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

    // ── single-pendulum static gravity-torque ─────────────────────────────────
    //
    // A 1 kg point mass hanging at L = 100 mm along the link's −z axis when
    // θ = 0 (so com = [0, 0, −0.1] in the body frame).  The joint is revolute
    // about +y; at θ = +30° = π/6 (pivot at origin, link frame rotated by θ
    // about y), the mass swings toward −x.
    //
    // Expected actuator torque holding the pendulum static:
    //     τ = m · g · L · sin(30°) = 1 · 9.81 · 0.1 · 0.5 = 0.4905 N·m
    //
    // With q_dot = q_ddot = 0 the velocity-product (Coriolis/centrifugal) terms
    // vanish, so only the gravity/inertia/transmission path is exercised.
    #[test]
    fn single_pendulum_static_gravity_torque() {
        // Rotation: +30° about the +y axis.
        // Unit quaternion for angle θ about axis (0,1,0):
        //   w = cos(θ/2), x = 0, y = sin(θ/2), z = 0
        let theta = std::f64::consts::PI / 6.0; // 30°
        let (half_sin, half_cos) = ((theta / 2.0).sin(), (theta / 2.0).cos());
        let q = [half_cos, 0.0, half_sin, 0.0]; // (w, x, y, z)

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
