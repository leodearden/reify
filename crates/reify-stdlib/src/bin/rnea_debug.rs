use reify_stdlib::dynamics::rnea::{default_gravity, inverse_dynamics_open_chain, RneaLink};
use reify_stdlib::dynamics::spatial::{cross_f, cross_m, Frame3, SpatialInertia6, SpatialTransform6, SpatialVector6};

fn ry_quat(theta: f64) -> [f64; 4] {
    let (s, c) = (theta / 2.0).sin_cos();
    [c, 0.0, s, 0.0]
}

fn joint_xform(quat: [f64; 4], r: [f64; 3]) -> SpatialTransform6 {
    SpatialTransform6::from_frame3(&Frame3::new(quat, [0.0, 0.0, 0.0]))
        .compose(&SpatialTransform6::from_frame3(&Frame3::new([1.0, 0.0, 0.0, 0.0], r)))
}

fn sv_add(a: &SpatialVector6, b: &SpatialVector6) -> SpatialVector6 {
    let aa = a.as_array();
    let ab = b.as_array();
    SpatialVector6::from_array([
        aa[0]+ab[0], aa[1]+ab[1], aa[2]+ab[2],
        aa[3]+ab[3], aa[4]+ab[4], aa[5]+ab[5],
    ])
}

fn sv_axpy(a: &mut SpatialVector6, scale: f64, b: &SpatialVector6) {
    let mut aa = a.as_array();
    let ab = b.as_array();
    for i in 0..6 { aa[i] += scale * ab[i]; }
    *a = SpatialVector6::from_array(aa);
}

fn main() {
    // qd=(1,0) case: debug the Coriolis contribution for link 1 (joint 2)
    let q1 = 0.1_f64;
    let q2 = 0.2_f64;
    let qd1 = 1.0_f64;
    let qd2 = 0.0_f64;
    
    // Build the transforms
    let x0 = joint_xform(ry_quat(q1), [0.0, 0.0, 0.0]);
    let x1 = joint_xform(ry_quat(q2), [1.0, 0.0, 0.0]);
    
    // Forward pass
    let a_base = SpatialVector6::from_angular_linear([0.0,0.0,0.0], [0.0,0.0,9.81]);
    let v_base = SpatialVector6::zero();
    
    // Link 0
    let s0 = SpatialVector6::from_angular_linear([0.0,1.0,0.0],[0.0,0.0,0.0]);
    let mut vj0 = SpatialVector6::zero();
    sv_axpy(&mut vj0, qd1, &s0);
    let v0 = sv_add(&x0.apply(&v_base), &vj0);
    let mut aj0 = SpatialVector6::zero(); // qdd1=0
    let crossm0 = cross_m(&v0, &vj0);
    let a0 = sv_add(&sv_add(&x0.apply(&a_base), &aj0), &crossm0);
    
    println!("v0 = {:?}", v0.as_array());
    println!("vJ0 = {:?}", vj0.as_array());
    println!("crossM(v0,vJ0) = {:?}", crossm0.as_array());
    println!("a0 = {:?}", a0.as_array());
    
    // Link 1
    let s1 = SpatialVector6::from_angular_linear([0.0,1.0,0.0],[0.0,0.0,0.0]);
    let mut vj1 = SpatialVector6::zero();
    sv_axpy(&mut vj1, qd2, &s1);  // qd2=0, so vj1=0
    let v1 = sv_add(&x1.apply(&v0), &vj1);
    let mut aj1 = SpatialVector6::zero(); // qdd2=0
    let crossm1 = cross_m(&v1, &vj1);
    let a1 = sv_add(&sv_add(&x1.apply(&a0), &aj1), &crossm1);
    
    println!("\nv1 = {:?}", v1.as_array());
    println!("vJ1 = {:?}", vj1.as_array());
    println!("crossM(v1,vJ1) = {:?}", crossm1.as_array());
    println!("a1 = {:?}", a1.as_array());
    
    // Inertia for link 1
    let i1 = SpatialInertia6::from_mass_com_inertia(1.0, [0.5,0.0,0.0],
        [[0.0,0.0,0.0],[0.0,1.0/12.0,0.0],[0.0,0.0,0.0]]);
    
    let i1a1 = i1.apply(&a1);
    let i1v1 = i1.apply(&v1);
    let bias_force = cross_f(&v1, &i1v1);
    
    println!("\nI1*a1 = {:?}", i1a1.as_array());
    println!("I1*v1 = {:?}", i1v1.as_array());
    println!("crossF(v1, I1*v1) = {:?}", bias_force.as_array());
    
    let f1 = sv_add(&i1a1, &bias_force);
    println!("f1 = {:?}", f1.as_array());
    
    // tau_1 = S1 . f1 (joint 1 torque)
    let sv_dot = |a: &SpatialVector6, b: &SpatialVector6| -> f64 {
        let aa = a.as_array(); let bb = b.as_array();
        aa.iter().zip(bb.iter()).map(|(x,y)| x*y).sum()
    };
    let tau1 = sv_dot(&s1, &f1);
    println!("\ntau_joint1 = {:.6}", tau1);
    
    // Now compare with gravity baseline (qd=0)
    let vj0_g = SpatialVector6::zero();
    let v0_g = sv_add(&x0.apply(&v_base), &vj0_g);
    let crossm0_g = cross_m(&v0_g, &vj0_g);
    let a0_g = sv_add(&sv_add(&x0.apply(&a_base), &SpatialVector6::zero()), &crossm0_g);
    
    let vj1_g = SpatialVector6::zero();
    let v1_g = sv_add(&x1.apply(&v0_g), &vj1_g);
    let crossm1_g = cross_m(&v1_g, &vj1_g);
    let a1_g = sv_add(&sv_add(&x1.apply(&a0_g), &SpatialVector6::zero()), &crossm1_g);
    
    let f1_g = sv_add(&i1.apply(&a1_g), &cross_f(&v1_g, &i1.apply(&v1_g)));
    let tau1_g = sv_dot(&s1, &f1_g);
    
    println!("tau_joint1_gravity = {:.6}", tau1_g);
    println!("tau_joint1 - tau_gravity = {:.6}", tau1 - tau1_g);
    println!("Expected Coriolis: +{:.6} (from h*qd1^2 = 0.5*sin(0.2)*1)", 0.5*0.2_f64.sin());
    
    // Also look at the sign of the cross_f angular y component
    println!("\ncrossF angular_y component = {:.6}", bias_force.as_array()[1]);
    println!("Expected: +{:.6}", 0.5*0.2_f64.sin());
}
