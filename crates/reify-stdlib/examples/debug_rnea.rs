use reify_stdlib::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};
use reify_stdlib::dynamics::rnea::{default_gravity, inverse_dynamics_open_chain, RneaLink};

fn ry_quat(theta: f64) -> [f64; 4] {
    let (s, c) = (theta / 2.0).sin_cos();
    [c, 0.0, s, 0.0]
}

fn run_case(label: &str, q1: f64, q2: f64, qd1: f64, qd2: f64, qdd1: f64, qdd2: f64) {
    let link0 = RneaLink {
        parent: None,
        parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(ry_quat(q1), [0.0, 0.0, 0.0])),
        subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
        mass: 1.0, com: [0.5, 0.0, 0.0],
        inertia_about_com: [[0.0, 0.0, 0.0], [0.0, 1.0 / 12.0, 0.0], [0.0, 0.0, 0.0]],
        q_dot: vec![qd1], q_ddot: vec![qdd1],
    };
    let link1 = RneaLink {
        parent: Some(0),
        parent_to_child: SpatialTransform6::from_frame3(&Frame3::new(ry_quat(q2), [1.0, 0.0, 0.0])),
        subspace: vec![SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0, 0.0, 0.0])],
        mass: 1.0, com: [0.5, 0.0, 0.0],
        inertia_about_com: [[0.0, 0.0, 0.0], [0.0, 1.0 / 12.0, 0.0], [0.0, 0.0, 0.0]],
        q_dot: vec![qd2], q_ddot: vec![qdd2],
    };
    let tau = inverse_dynamics_open_chain(&[link0, link1], default_gravity());

    // Reference Lagrangian
    let g = 9.81;
    let c1 = q1.cos(); let c2 = q2.cos(); let s2 = q2.sin(); let c12 = (q1+q2).cos();
    let m11 = 5.0/3.0+c2; let m12 = 1.0/3.0+0.5*c2; let m22 = 1.0/3.0_f64;
    let h = 0.5*s2;
    let cc1 = -h*qd2*(2.0*qd1+qd2); let cc2 = h*qd1*qd1;
    let g1 = -g*(1.5*c1+0.5*c12); let g2 = -g*0.5*c12;
    let ref1 = m11*qdd1 + m12*qdd2 + cc1 + g1;
    let ref2 = m12*qdd1 + m22*qdd2 + cc2 + g2;

    println!("{}: RNEA=[{:.6},{:.6}] REF=[{:.6},{:.6}] Δ=[{:.3e},{:.3e}]",
             label, tau[0][0], tau[1][0], ref1, ref2,
             tau[0][0]-ref1, tau[1][0]-ref2);
}

fn main() {
    // Case 1: static gravity only
    run_case("grav_q1=0,q2=0", 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    run_case("grav_q1=0.3,q2=0.0", 0.3, 0.0, 0.0, 0.0, 0.0, 0.0);   // q2=0 should work
    run_case("grav_q1=0,q2=0.1", 0.0, 0.1, 0.0, 0.0, 0.0, 0.0);     // q1=0 only q2
    run_case("grav_q1=0.3,q2=0.1", 0.3, 0.1, 0.0, 0.0, 0.0, 0.0);   // both nonzero

    // Inertia and Coriolis
    run_case("inertia_q=0,qdd=[1,1]", 0.0, 0.0, 0.0, 0.0, 1.0, 1.0);
    run_case("inertia_q=[0.3,0.1],qdd=[1,1]", 0.3, 0.1, 0.0, 0.0, 1.0, 1.0);
    run_case("coriolis_q=0,qd=[1,-0.5]", 0.0, 0.0, 1.0, -0.5, 0.0, 0.0);
    run_case("coriolis_q=[0.3,0.1],qd=[1,-0.5]", 0.3, 0.1, 1.0, -0.5, 0.0, 0.0);

    // Full sample
    run_case("sample1", 0.1, 0.2, 1.0, -0.5, 0.5, 0.3);
}
