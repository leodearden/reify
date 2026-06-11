//! End-to-end fixture test for the CLOSED-chain inverse-dynamics bridge
//! (task 4146; descoped from `docs/prds/v0_3/rigid-body-dynamics.md` §5.3 /
//! RBD-η task 3836).
//!
//! Drives `examples/dynamics/closed_2prismatic_idyn.ri` through the full
//! parse → `parse_and_compile_with_stdlib` → `Engine::build` pipeline and
//! asserts the virtual-work POWER identity for a vertical 2-prismatic CLOSED
//! loop under gravity:
//!
//!     Σ τ_i·q̇_i  =  (m_a + m_b)·v·(a + 9.81)
//!                =  (2 + 3)·0.7·(1.5 + 9.81)  =  39.585 W
//!
//! ── What this validates (transparent scope) ───────────────────────────────────
//! End-to-end Value-level marshalling of a CLOSED mechanism (non-empty
//! `loop_closures`) through `inverse_dynamics(m, traj)`: multi-body RNEA, M
//! assembly, loop detection + chain extraction + constraint-Jacobian assembly +
//! rank reduction, the KKT solve, and a physically-correct gravity-loaded energy
//! rate. Strictly stronger than the pure-Rust step-7 finiteness smoke test
//! (`reify-stdlib/.../dynamics/eval.rs::closed_chain_inverse_dynamics_routing_finite_on_prismatic_loop`,
//! which has q̇=q̈=0 and no gravity work).
//!
//! ── What this does NOT validate (and why) ─────────────────────────────────────
//! For a prismatic-closing loop whose closing joint shares the residual axis,
//! `reduce_constraint_rank` projects out the entire residual row → `m_eff = 0`:
//! there is no LIVE constraint, so the closed path reduces to per-DOF open-chain
//! RNEA (τ = τ_open). The power identity Σ τ_i·q̇_i = τ_open·q̇ = dE/dt is exact
//! by the work-energy theorem and holds for `m_eff = 0` too, but this fixture
//! therefore does NOT exercise the nonzero-constraint machinery (λ, `m_eff ≥ 1`,
//! incidence map, rank reduction to a non-empty A). That machinery is covered by
//! the existing array-level unit tests (steps 3–6 — incl. a synthetic revolute
//! rank-reduction case — in `closed_chain.rs` / `loop_closure.rs` / `rnea.rs`).
//! A live-constraint (`m_eff ≥ 1`) *e2e* requires the deferred kinematic
//! inter-joint-offset feature (docs/prds/v0_6/kinematic-inter-joint-offsets.md),
//! which the current kinematic layer cannot express (esc-4146-280).
//!
//! Kernel-INDEPENDENT: `inverse_dynamics` derives mass from each body's
//! `MassProperties` solid and needs no `GeometryKernel`, so a
//! `MockGeometryKernel` suffices (mirrors `rigid_body_dynamics_e2e.rs`).

use reify_constraints::{JointValue, NewtonConfig, NewtonOutcome, StartStrategy, solve_loop_closure};
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_stdlib::loop_closure::{loop_residual_jacobian_by_joint, loop_residual_twist};
use reify_test_support::{
    collect_errors, errors_only, parse_and_compile_with_stdlib, MockGeometryKernel,
};

/// Absolute path to the closed 2-prismatic inverse-dynamics example fixture.
/// Mirrors the CARGO_MANIFEST_DIR pattern from `rigid_body_dynamics_e2e.rs`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dynamics/closed_2prismatic_idyn.ri"
);

/// Absolute path to the closed 4-bar inverse-dynamics example fixture (β1).
const FOUR_BAR_EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dynamics/closed_4bar_idyn.ri"
);

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`). Panics on a non-numeric cell so a shape regression fails loudly.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// Pull a named field out of a `StructureInstance`, asserting its `type_name`.
fn field<'a>(v: &'a Value, type_name: &str, member: &str) -> &'a Value {
    match v {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, type_name,
                "expected a {type_name} instance, got type_name {}",
                data.type_name
            );
            data.fields
                .get(member)
                .unwrap_or_else(|| panic!("{type_name} missing field `{member}`"))
        }
        other => panic!("expected a {type_name} StructureInstance, got {other:?}"),
    }
}

/// `inverse_dynamics(m, traj)` on the vertical 2-prismatic CLOSED loop yields a
/// finite `List<List<JointForce>>` of shape 1×2 whose two prismatic
/// `ScalarForce` magnitudes (τ_a, τ_b) satisfy the virtual-work power identity
/// `Σ τ_i·q̇_i = (m_a+m_b)·v·(a+9.81) = 39.585 W` within 1 µW.
#[test]
fn closed_2prismatic_virtual_work_identity() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/dynamics/closed_2prismatic_idyn.ri should exist (task 4146 fixture)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "closed_2prismatic_idyn.ri should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Kernel-independent: inverse_dynamics reads mass from each body's
    // MassProperties solid, so a plain mock kernel is enough.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Locate the top-level `forces` cell. structure_def = `Closed2PrismaticIdyn`;
    // the closed-chain inverse-dynamics result binds to `forces`.
    let cell = reify_core::ValueCellId::new("Closed2PrismaticIdyn", "forces");
    let per_sample = match result.values.get(&cell) {
        Some(Value::List(s)) => s,
        other => panic!(
            "Closed2PrismaticIdyn.forces must be a List<List<JointForce>>, got {other:?}\n\
             (NOT Undef ⇒ closed routing wired; all diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(
        per_sample.len(),
        1,
        "one trajectory sample ⇒ one inner force list"
    );

    // Inner List<JointForce>: length = tree-joint count = 2 (j_a, j_b).
    // n_tree = bodies − loop_closures = 3 − 1 = 2 (closing body m_c excluded).
    let forces = match &per_sample[0] {
        Value::List(f) => f,
        other => panic!("sample 0: expected a List<JointForce>, got {other:?}"),
    };
    assert_eq!(
        forces.len(),
        2,
        "two spanning-tree joints (j_a, j_b) ⇒ two JointForce entries"
    );

    // The fixture's per-tree-DOF rates (BODIES order: q̇_a, q̇_b). `forces[i]`
    // is returned in the same bodies order, so forces[i] pairs with q_dot[i].
    let q_dot = [0.7_f64, 0.7_f64];

    // Σ τ_i·q̇_i over the returned (signed) prismatic generalized forces.
    let mut power = 0.0_f64;
    for (i, jf) in forces.iter().enumerate() {
        let value = field(jf, "JointForce", "value");
        // Both joints are prismatic ⇒ ScalarForce { magnitude } (signed f64).
        let mag = num(field(value, "ScalarForce", "magnitude"));
        assert!(
            mag.is_finite(),
            "force[{i}].ScalarForce.magnitude must be finite (⇒ KKT nonsingular), got {mag}"
        );
        power += mag * q_dot[i];
    }

    // Virtual-work power identity: dE/dt = (m_a+m_b)·v·(a+9.81)
    //                                    = 5.0·0.7·11.31 = 39.585 W.
    // Exact to numerical roundoff by the work-energy theorem (b ≡ 0, constraint
    // forces do no work on the supplied velocities) ⇒ 1 µW has orders of margin.
    let expected = 39.585_f64;
    assert!(
        (power - expected).abs() < 1e-6,
        "virtual-work power identity Σ τ_i·q̇_i: expected {expected} W, got {power} W \
         (Δ = {} W). A mismatch indicates a real bridge bug — diagnose, do not retune.",
        power - expected
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// KIN-OFFSET β1: closed 4-bar loop-residual tests (B4 signal, task 4428)
// ──────────────────────────────────────────────────────────────────────────────
//
// Grashof crank-rocker: crank a=40mm (shortest), coupler b=120mm,
// rocker c=116.282484506mm (the EXACT length closing the loop at θ_input=45°
// with the coupler straight — see the .ri header), ground d=140mm.
// Grashof check: s+l = 40+140 = 180 ≤ p+q = 116.2825+120 = 236.2825 ✓.
//
// Pivot offsets (task 4331 surface):
//   j_crank       – revolute at A = world origin, pivot=(0,0,0)
//   j_coupler     – revolute, pivot=(40mm,0,0) in crank frame (= crank length a)
//   j_coupler_tip – revolute fixed-like tip, pivot=(120mm,0,0) in coupler frame (= coupler b)
//   j_rocker      – revolute, pivot=(140mm,0,0) in world frame (= ground length d)
//   j_rocker_tip  – revolute fixed-like tip, pivot=(116.282484506mm,0,0) in rocker frame (= rocker c)
//
// chain_a = [j_crank, j_coupler, j_coupler_tip] → FK reaches pivot C via crank+coupler
// chain_b = [j_rocker, j_rocker_tip]            → FK reaches pivot C via rocker
//
// §0 gap: with all-coincident-origin revolutes (no pivot offsets), the translational
// loop residual is identically zero. Non-coincident pivots make the residual nonzero
// and config-dependent (esc-4146-280, PRD §0).

/// Helper: get a joint Value cell from the four-bar example's eval result.
fn get_four_bar_joint<'a>(
    values: &'a reify_ir::ValueMap,
    name: &str,
) -> &'a Value {
    let id = ValueCellId::new("ClosedFourBarIdyn", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("ClosedFourBarIdyn.{name} not found in eval result"))
}

/// B4 — §8 live loop-residual signal:
///
/// (a) `closed_4bar_idyn.ri` compiles with no error-severity diagnostics.
/// (b) The loop residual translational norm at θ_crank=45° (off-closure) is
///     ≫ kernel roundoff (> 1 mm ≫ 1 µm solver tolerance) — the §0 gap broken.
/// (c) The residual translation vector DIFFERS between two distinct input angles —
///     the residual is genuinely config-dependent.
///
/// Both (b) and (c) depend only on task 4331 (pivot-offset FK) + the loop-closure
/// machinery; they do NOT assert any inverse_dynamics `forces` value (that is β2).
#[test]
fn closed_4bar_live_loop_residual() {
    let source = std::fs::read_to_string(FOUR_BAR_EXAMPLE_PATH)
        .expect("examples/dynamics/closed_4bar_idyn.ri should exist (task 4428 β1 fixture)");

    // (a) compile-clean gate
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "closed_4bar_idyn.ri should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Eval to get joint Value cells. Mirror the production engine setup
    // (reify-cli main.rs / gui engine.rs): checker + mock kernel, PLUS
    // register_compute_fns so the `@optimized("dynamics::inverse_dynamics")`
    // target resolves to its trampoline instead of emitting an
    // unregistered-target Error diagnostic.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.build(&compiled, ExportFormat::Step);
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;
    let j_crank = get_four_bar_joint(v, "j_crank").clone();
    let j_coupler = get_four_bar_joint(v, "j_coupler").clone();
    let j_coupler_tip = get_four_bar_joint(v, "j_coupler_tip").clone();
    let j_rocker = get_four_bar_joint(v, "j_rocker").clone();
    let j_rocker_tip = get_four_bar_joint(v, "j_rocker_tip").clone();

    // Build the two open chains:
    //   chain_a = [j_crank, j_coupler, j_coupler_tip]  → reaches C from A via crank+coupler
    //   chain_b = [j_rocker, j_rocker_tip]              → reaches C from D via rocker
    let chain_a = vec![j_crank.clone(), j_coupler.clone(), j_coupler_tip.clone()];
    let chain_b = vec![j_rocker.clone(), j_rocker_tip.clone()];

    // Off-closure joint values at θ_crank = 45° (π/4 rad), θ_coupler = 0, θ_rocker = 0.
    // These are NOT the assembled-closure angles → residual is nonzero.
    let theta_45 = std::f64::consts::PI / 4.0;
    let vals_a_45 = vec![
        JointValue::Scalar(theta_45), // j_crank at 45°
        JointValue::Scalar(0.0),      // j_coupler at 0 (off-closure)
        JointValue::Scalar(0.0),      // j_coupler_tip at 0 (pure translation of b)
    ];
    let vals_b_45 = vec![
        JointValue::Scalar(0.0), // j_rocker at 0 (off-closure)
        JointValue::Scalar(0.0), // j_rocker_tip at 0 (pure translation of c)
    ];

    // (b) Residual translational norm ≫ 1 µm at off-closure config.
    let twist_45 = loop_residual_twist(&chain_a, &vals_a_45, &chain_b, &vals_b_45)
        .expect("loop_residual_twist must succeed at off-closure config (θ_crank=45°)");
    // twist = [ω_x, ω_y, ω_z, v_x, v_y, v_z]
    let linear_norm_45 = (twist_45[3] * twist_45[3]
        + twist_45[4] * twist_45[4]
        + twist_45[5] * twist_45[5])
    .sqrt();
    // Expected off-closure mismatch: ≈ 0.182 m at θ1=45°, θ2=θ3=0 (esc-4146-280 §0 gap)
    assert!(
        linear_norm_45 > 1e-3,
        "loop residual at θ_crank=45° (off-closure) must be ≫ solver tolerance (1 µm), \
         got linear_norm = {linear_norm_45:.6e} m (expected > 1e-3 m). \
         Non-coincident pivot offsets are the load-bearing requirement — check that \
         closed_4bar_idyn.ri uses non-zero pivot point3 args (esc-4146-280 §0 gap)."
    );

    // (c) Residual at a second input angle DIFFERS — config-dependent.
    let theta_90 = std::f64::consts::PI / 2.0;
    let vals_a_90 = vec![
        JointValue::Scalar(theta_90), // j_crank at 90°
        JointValue::Scalar(0.0),
        JointValue::Scalar(0.0),
    ];
    let vals_b_90 = vec![
        JointValue::Scalar(0.0),
        JointValue::Scalar(0.0),
    ];

    let twist_90 = loop_residual_twist(&chain_a, &vals_a_90, &chain_b, &vals_b_90)
        .expect("loop_residual_twist must succeed at off-closure config (θ_crank=90°)");
    let linear_norm_90 = (twist_90[3] * twist_90[3]
        + twist_90[4] * twist_90[4]
        + twist_90[5] * twist_90[5])
    .sqrt();

    // The translation vectors at 45° and 90° must differ by ≫ roundoff.
    let diff_x = twist_45[3] - twist_90[3];
    let diff_y = twist_45[4] - twist_90[4];
    let diff_z = twist_45[5] - twist_90[5];
    let diff_norm = (diff_x * diff_x + diff_y * diff_y + diff_z * diff_z).sqrt();
    assert!(
        diff_norm > 1e-6,
        "loop residual translation must change between θ_crank=45° and θ_crank=90° \
         (config-dependent, §0 gap). \
         |v@45°| = {linear_norm_45:.6e} m, |v@90°| = {linear_norm_90:.6e} m, \
         |v@45°−v@90°| = {diff_norm:.6e} m (expected > 1e-6 m)."
    );
}

/// B4 — solve_loop_closure convergence: a consistent closure exists for the
/// Grashof 4-bar at θ_crank=45°.
///
/// Holds chain_a (crank) at θ_input=45°; frees the coupler and rocker DOFs on
/// chain_b; warm-starts near zero. Asserts `NewtonOutcome::Converged` with a
/// residual below the combined position + rotation tolerance, and recomputes
/// `loop_residual_twist` at the converged config to confirm both linear and
/// angular norms are below the same tolerance (the planar-in-loop machinery
/// test's converged-config recheck pattern, kinematic_loop_closure_machinery.rs).
#[test]
fn closed_4bar_loop_closes_consistently() {
    let source = std::fs::read_to_string(FOUR_BAR_EXAMPLE_PATH)
        .expect("examples/dynamics/closed_4bar_idyn.ri should exist (task 4428 β1 fixture)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "closed_4bar_idyn.ri should compile clean"
    );
    // Same engine setup as the sibling tests (see closed_4bar_live_loop_residual).
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.build(&compiled, ExportFormat::Step);
    assert!(
        collect_errors(&result.diagnostics).is_empty(),
        "eval should produce no Error diagnostics, got: {:#?}",
        collect_errors(&result.diagnostics)
    );

    let v = &result.values;
    let j_crank = get_four_bar_joint(v, "j_crank").clone();
    let j_coupler = get_four_bar_joint(v, "j_coupler").clone();
    let j_coupler_tip = get_four_bar_joint(v, "j_coupler_tip").clone();
    let j_rocker = get_four_bar_joint(v, "j_rocker").clone();
    let j_rocker_tip = get_four_bar_joint(v, "j_rocker_tip").clone();

    // chain_a: hold crank at θ_input = 45°; coupler_tip at 0 (pure translation)
    let chain_a = vec![j_crank, j_coupler.clone(), j_coupler_tip.clone()];
    let theta_input = std::f64::consts::PI / 4.0;
    let vals_a = vec![
        JointValue::Scalar(theta_input),
        JointValue::Scalar(0.0), // coupler angle — NOT free, held at 0 for chain_a definition
        JointValue::Scalar(0.0), // tip at 0
    ];

    // chain_b: [j_rocker, j_rocker_tip] — free DOFs are [j_rocker angle, j_rocker_tip angle]
    // Physically only j_rocker is free (j_rocker_tip should stay 0); expose both as free
    // so the solver has enough DOFs to close the loop. Use index 0 (j_rocker) as the
    // primary free DOF and index 1 (j_rocker_tip) as secondary.
    let chain_b = vec![j_rocker, j_rocker_tip];
    // Warm-start near the ASSEMBLED configuration (an "approximate assembled
    // guess"). With the coupler straight at θ_input=45°, pivot C sits at
    // (113.137, 113.137) mm, so the assembled rocker angle is
    // atan2(113.137, 113.137−140) ≈ 1.804 rad, and orientation closure puts the
    // rocker tip at θ_crank − θ_rocker ≈ −1.019 rad. Seed near (not at) the
    // solution so Newton does real work.
    let vals_b_initial = vec![
        JointValue::Scalar(1.8),  // j_rocker warm-start near assembled position
        JointValue::Scalar(-1.0), // j_rocker_tip near assembled tip angle
    ];
    let free_b = vec![0usize, 1usize]; // both chain_b slots free
    let strategy = StartStrategy::WarmStart(vec![1.8, -1.0]);
    let cfg = NewtonConfig::default();

    let outcome = solve_loop_closure(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    match outcome {
        NewtonOutcome::Converged {
            x,
            iters,
            residual_norm,
        } => {
            let combined_tol = cfg.tol_pos_m + cfg.tol_rot_rad;
            assert!(
                residual_norm < combined_tol,
                "solve_loop_closure residual_norm {residual_norm:.3e} must be below \
                 combined_tol {combined_tol:.3e} (tol_pos_m + tol_rot_rad). \
                 A mismatch indicates the Grashof 4-bar's link lengths or warm-start \
                 don't satisfy the closure condition — refine closed_4bar_idyn.ri."
            );
            assert!(
                iters < 50,
                "expected convergence in <50 iters, got {iters} (residual={residual_norm:.3e})"
            );
            assert_eq!(x.len(), 2, "two free variables (j_rocker, j_rocker_tip)");

            // Recompute loop_residual_twist at converged config, mirroring the
            // planar-in-loop pattern in kinematic_loop_closure_machinery.rs.
            let vals_b_final = vec![
                JointValue::Scalar(x[0]),
                JointValue::Scalar(x[1]),
            ];
            let twist =
                loop_residual_twist(&chain_a, &vals_a, &chain_b, &vals_b_final)
                    .expect("loop_residual_twist must succeed at converged config");
            let angular_norm = (twist[0] * twist[0]
                + twist[1] * twist[1]
                + twist[2] * twist[2])
            .sqrt();
            let linear_norm = (twist[3] * twist[3]
                + twist[4] * twist[4]
                + twist[5] * twist[5])
            .sqrt();
            assert!(
                angular_norm < combined_tol,
                "recomposed angular residual {angular_norm:.3e} must be below \
                 combined_tol {combined_tol:.3e}"
            );
            assert!(
                linear_norm < combined_tol,
                "recomposed linear residual {linear_norm:.3e} must be below \
                 combined_tol {combined_tol:.3e}"
            );
        }
        other => panic!(
            "expected NewtonOutcome::Converged for the Grashof 4-bar at θ_input=45°, \
             got {other:?}. Check closed_4bar_idyn.ri link lengths satisfy the Grashof \
             condition (s+l ≤ p+q) and the warm-start is near the assembled config."
        ),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// KIN-OFFSET β2: B6 live constraint rank + B7 virtual-work identity (task 4429)
// ──────────────────────────────────────────────────────────────────────────────
//
// Energy ledger derivation (prereq-1; see plan.json design_decisions):
//
//   The linkage is planar in XY; default_gravity=[0,0,−9.81] ⊥ plane ⇒ dPE/dt=0.
//   dE/dt = dKE/dt = q̇ᵀMq̈ + ½q̇ᵀṀq̇.
//
//   At the assembled config (θ_crank=45°, coupler straight) with ω=2π rad/s:
//     • Pivot C (coupler-tip / rocker-tip) is at instantaneous standstill (v_C=0).
//       Proof: v_B = ω·a (crank tip), ω_cp_abs = −ω/3, r_{B→C} = b/√2·(1,1,0).
//       v_C_x = −ω·a/√2 + (ω/3)·b/√2 = ω/√2·(b/3−a) = 0 (since a=b/3=0.04 m). ✓
//     • Therefore ½q̇ᵀṀq̇ = 0 (J′_eff(45°)=0: effective inertia is stationary).
//     • dE/dt = q̇ᵀMq̈ exactly at this config.
//
//   Loading accels = vels×(α/ω) gives q̇ᵀMq̈ = (α/ω)·q̇ᵀMq̇ = (α/ω)·2·KE = J_eff·ω·α.
//
//   J_eff(45°) = I_cr + m_cp·a² + (I_cp + I_ct + I_rt)/9
//             = 0.10 + 3·0.04² + (0.20+0.02+0.01)/9
//             = 0.10 + 0.0048 + 0.23/9
//             ≈ 0.130356 kg·m²
//
//   Absolute angular velocities:
//     ω_cr=ω=2π, ω_cp=ω_ct=ω_rt=−ω/3=−2π/3, ω_rk=0.
//   Body COMs: crank/rocker at pivots A/D (v=0), coupler at crank-tip B
//   (|v_B|=ω·a), coupler_tip/rocker_tip at standstill pivot C (v=0).
//
//   At α=π rad/s²: dE/dt = J_eff·2π·π ≈ 0.130356·2π² ≈ 2.573 W.
//
//   Two-sample trajectory:
//     sample_0: accels=0           → power_0 ≈ 0 (measures Newton-residual floor)
//     sample_1: accels=vels×(π/2π) → power_α ≈ dE_dt_analytic
//   Increment power_α−power_0 isolates J_eff·ω·α and cancels the floor.
//
//   Tolerance discipline (G6 / esc-3821-44 / esc-3453):
//     Floor from KKT: Σ τ·q̇ = τ_open·q̇ + λᵀ(Aq̇); λᵀ(Aq̇) is the floor.
//     q̇ consistent at the exact config; A is the FD Jacobian at the Newton-converged
//     config (ε_N ≤ tol_pos_m=1e-6 m), so ‖A·q̇‖~O(ε_N·ω).
//     A-priori: |λ|~O(m·α·r)~O(1 kg·π·0.12 m)~0.38 Nm; floor~2.4 µW.
//     Empirical floor measured in B7 via power_0 (step-4 finalizes tol to ≥10×).
//     NOT the 2-prismatic 1 µW literal (different mechanism, different λ magnitude).

/// B6 — §8 live constraint rank (KIN-OFFSET β2, task 4429):
///
/// At the ASSEMBLED config of the Grashof 4-bar, the loop-constraint Jacobian
/// (6×5, one column per spanning-tree joint) has effective rank ≥ 1 after
/// projecting out the closing +Z-revolute's absorbed ω_z row via
/// `reduce_constraint_rank`.
///
/// Expected m_eff = 2: in-plane (v_x, v_y) rows are active; ω_z is absorbed
/// by the closing revolute; ω_x, ω_y, v_z are structurally zero for the
/// planar-XY linkage.  This exercises the live-constraint branch of the
/// closed-chain bridge — unreachable by the 2-prismatic fixture (m_eff=0 there
/// because the closing prismatic absorbs the entire residual row).
///
/// Characterization/contract assertion: β1's non-coincident pivot offsets make
/// the FD Jacobian non-degenerate at the Grashof θ=45° config, so the test
/// is expected to pass on arrival (no .ri change required).
#[test]
fn closed_4bar_live_constraint_rank() {
    let source = std::fs::read_to_string(FOUR_BAR_EXAMPLE_PATH)
        .expect("examples/dynamics/closed_4bar_idyn.ri should exist (task 4429 β2 fixture)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "closed_4bar_idyn.ri should compile with no error-severity diagnostics: {:#?}",
        errors_only(&compiled)
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.build(&compiled, ExportFormat::Step);
    assert!(
        collect_errors(&result.diagnostics).is_empty(),
        "eval should produce no Error diagnostics, got: {:#?}",
        collect_errors(&result.diagnostics)
    );

    let v = &result.values;
    let j_crank       = get_four_bar_joint(v, "j_crank").clone();
    let j_coupler     = get_four_bar_joint(v, "j_coupler").clone();
    let j_coupler_tip = get_four_bar_joint(v, "j_coupler_tip").clone();
    let j_rocker      = get_four_bar_joint(v, "j_rocker").clone();
    let j_rocker_tip  = get_four_bar_joint(v, "j_rocker_tip").clone();

    // Open chains at the ASSEMBLED closure config (analytically derived; see .ri header).
    let chain_a = vec![j_crank.clone(), j_coupler.clone(), j_coupler_tip.clone()];
    let chain_b = vec![j_rocker.clone(), j_rocker_tip.clone()];

    // Assembled joint values:
    //   θ_crank=π/4, θ_coupler=0, θ_coupler_tip=0    (chain_a)
    //   θ_rocker=atan2(113.14,113.14−140)≈1.8039 rad  (chain_b)
    //   θ_rocker_tip=θ_crank−θ_rocker≈−1.0185 rad    (chain_b)
    let vals_a = vec![
        JointValue::Scalar(std::f64::consts::PI / 4.0), // θ_crank = 45°
        JointValue::Scalar(0.0),                         // θ_coupler = 0 (coupler straight)
        JointValue::Scalar(0.0),                         // θ_coupler_tip = 0
    ];
    let vals_b = vec![
        JointValue::Scalar(1.8039163646188838),  // θ_rocker (exact closure angle)
        JointValue::Scalar(-1.0185182012214355), // θ_rocker_tip (orientation closure)
    ];

    // The 5 spanning-tree joints in bodies order:
    //   crank, coupler, coupler_tip, rocker, rocker_tip.
    // This is the same ordering the bridge uses for its ordered_joints (eval.rs:1341).
    let target_joints = vec![
        j_crank,
        j_coupler,
        j_coupler_tip.clone(), // also the closing joint
        j_rocker,
        j_rocker_tip,
    ];

    // Raw 6×5 loop-constraint Jacobian via central FD (eps=1e-7, same as bridge).
    // Returns one [f64;6] twist column per spanning-tree joint.
    let raw_cols = loop_residual_jacobian_by_joint(
        &chain_a, &vals_a, &chain_b, &vals_b, &target_joints, 1e-7,
    )
    .expect("loop_residual_jacobian_by_joint must succeed at assembled config");

    assert_eq!(raw_cols.len(), 5, "5 spanning-tree joints → 5 Jacobian columns");

    // Transpose to 6×5 row-major (a_raw[row*5+col] = col[row]).
    // Mirrors eval.rs:1361: for (col_idx, col) in raw_cols.iter().enumerate() { ... }
    let n = 5usize;
    let mut a_raw = vec![0.0f64; 6 * n];
    for (col_idx, col) in raw_cols.iter().enumerate() {
        for row_idx in 0..6 {
            a_raw[row_idx * n + col_idx] = col[row_idx];
        }
    }

    // Closing joint's motion subspace: j_coupler_tip is a +Z revolute.
    // Spatial twist convention: [ω_x, ω_y, ω_z, v_x, v_y, v_z].
    // A +Z-revolute has ω=(0,0,1), v=(0,0,0) → subspace column [0,0,1,0,0,0].
    // (motion_subspace_columns is pub(crate) in reify-stdlib, so we build the
    // literal directly — all 5 joints are +Z revolutes per closed_4bar_idyn.ri.)
    let s_close = [[0.0f64, 0.0, 1.0, 0.0, 0.0, 0.0]];

    // Project out the closing joint's absorbed ω_z direction and row-reduce.
    // Exact call the bridge makes at eval.rs:1377.
    let (_a_red, m_eff) = reify_stdlib::dynamics::closed_chain::reduce_constraint_rank(
        &a_raw, 6, n, &s_close, 1e-10,
    );

    // Expected m_eff = 2 (row-by-row reasoning):
    //   row 0 (ω_x): identically zero for planar-XY linkage → reduced to near-zero → dropped
    //   row 1 (ω_y): identically zero for planar-XY          → dropped
    //   row 2 (ω_z): absorbed by the closing +Z revolute's subspace → subtracted → zero
    //   row 3 (v_x): ACTIVE in-plane translational constraint → survives
    //   row 4 (v_y): ACTIVE in-plane translational constraint → survives
    //   row 5 (v_z): identically zero for planar-XY          → dropped
    //
    // The 2-prismatic e2e gets m_eff=0 because its closing prismatic absorbs the
    // sole non-zero residual row entirely.  This 4-bar's +Z revolute projects out
    // only ω_z, leaving the translational (v_x, v_y) rows as live constraints.
    assert!(
        m_eff >= 1,
        "reduce_constraint_rank must return m_eff ≥ 1 at the assembled 4-bar config \
         (expected m_eff=2: in-plane v_x,v_y active; ω_z absorbed by closing +Z revolute; \
         ω_x,ω_y,v_z structurally zero for planar-XY). Got m_eff={m_eff}. \
         Indicates a degenerate FD Jacobian at the assembled config — check \
         closed_4bar_idyn.ri link lengths and pivot offsets (β1 §0 gap requirement)."
    );
}

/// B7 — virtual-work power identity for the closed 4-bar (headline, KIN-OFFSET β2, task 4429):
///
/// The closed-chain inverse dynamics bridge returns KKT-corrected torques τ_i
/// satisfying the virtual-work theorem:
///   Σ τ_i · q̇_i  =  dE/dt  =  J_eff(45°) · ω · α  ≈  2.573 W
///
/// Two-sample trajectory (see the energy ledger derivation in the β2 header above):
///   sample_0: accels=0          → power_0 ≈ 0 (residual floor; validates ½q̇ᵀṀq̇=0)
///   sample_1: accels=vels×(α/ω) → power_α  ≈ dE_dt_analytic
///   increment power_α−power_0  isolates J_eff·ω·α, cancels the floor.
///
/// J_eff(45°) = I_cr + m_cp·a² + (I_cp+I_ct+I_rt)/9 ≈ 0.130356 kg·m² (prereq-1).
/// α = π rad/s²; ω = 2π rad/s; dE_dt_analytic ≈ 2.573 W.
///
/// Tolerance: step-4 finalizes tol to ≥10× the MEASURED power_0 floor (NOT the
/// 2-prismatic 1 µW literal — capability-manifest β G6 DERIVE-AND-BIND mandate).
///
/// RED until step-4: β1's single-sample trajectory ⇒ forces shape 1×5, not 2×5.
#[test]
fn closed_4bar_virtual_work_identity() {
    let source = std::fs::read_to_string(FOUR_BAR_EXAMPLE_PATH)
        .expect("examples/dynamics/closed_4bar_idyn.ri should exist (task 4429 β2 fixture)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "closed_4bar_idyn.ri should compile with no error-severity diagnostics: {:#?}",
        errors_only(&compiled)
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.build(&compiled, ExportFormat::Step);
    assert!(
        collect_errors(&result.diagnostics).is_empty(),
        "eval should produce no Error diagnostics, got: {:#?}",
        collect_errors(&result.diagnostics)
    );

    // forces: List<List<JointForce>> with shape 2×5 (two trajectory samples × 5 tree joints).
    // RED until step-4 updates closed_4bar_idyn.ri to two-sample trajectory.
    let cell = ValueCellId::new("ClosedFourBarIdyn", "forces");
    let per_sample = match result.values.get(&cell) {
        Some(Value::List(s)) => s,
        other => panic!(
            "ClosedFourBarIdyn.forces must be a List<List<JointForce>>, got {other:?}\n\
             (diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(
        per_sample.len(),
        2,
        "two trajectory samples (α=0 and α=π/ω·vels) ⇒ forces shape 2×5; \
         got {}. RED if closed_4bar_idyn.ri still has the β1 single-sample trajectory \
         (step-4 replaces it with two samples).",
        per_sample.len()
    );

    // Spanning-tree joint velocities (bodies order): q̇ = [ω, −(a+b)/b·ω, 0, 0, −ω/3]
    // where a=0.04 m, b=0.12 m, ω=2π rad/s, (a+b)/b=4/3 → q̇_1=−8π/3.
    // forces[i] is returned in the same bodies order, so forces[i] pairs with q̇[i].
    let q_dot = [
        6.283185307179586_f64,   // j_crank:       ω = 2π rad/s
        -8.377580409572781_f64,  // j_coupler:     −(a+b)/b·ω = −8π/3 rad/s (relative)
        0.0_f64,                  // j_coupler_tip: 0
        0.0_f64,                  // j_rocker:      0
        -2.0943951023931953_f64,  // j_rocker_tip:  −ω/3 = −2π/3 rad/s (relative)
    ];

    // ── Sample 0 (accels=0): measure the Newton-residual power floor ────────────
    let forces_0 = match &per_sample[0] {
        Value::List(f) => f,
        other => panic!("sample 0: expected List<JointForce>, got {other:?}"),
    };
    assert_eq!(forces_0.len(), 5, "5 spanning-tree joints ⇒ 5 JointForce entries (sample 0)");

    let mut power_0 = 0.0_f64;
    for (i, jf) in forces_0.iter().enumerate() {
        let value = field(jf, "JointForce", "value");
        // All five joints are +Z revolutes ⇒ ScalarTorque { magnitude } (NOT ScalarForce).
        let mag = num(field(value, "ScalarTorque", "magnitude"));
        assert!(
            mag.is_finite(),
            "forces[0][{i}].ScalarTorque.magnitude must be finite (KKT nonsingular), got {mag}"
        );
        power_0 += mag * q_dot[i];
    }

    // ── Sample 1 (accels=vels×α/ω): the loaded inertial sample ────────────────
    let forces_1 = match &per_sample[1] {
        Value::List(f) => f,
        other => panic!("sample 1: expected List<JointForce>, got {other:?}"),
    };
    assert_eq!(forces_1.len(), 5, "5 spanning-tree joints ⇒ 5 JointForce entries (sample 1)");

    let mut power_alpha = 0.0_f64;
    for (i, jf) in forces_1.iter().enumerate() {
        let value = field(jf, "JointForce", "value");
        let mag = num(field(value, "ScalarTorque", "magnitude"));
        assert!(
            mag.is_finite(),
            "forces[1][{i}].ScalarTorque.magnitude must be finite (KKT nonsingular), got {mag}"
        );
        power_alpha += mag * q_dot[i];
    }

    // ── Virtual-work power identity (B7 headline) ──────────────────────────────
    //
    // J_eff(45°) derivation (prereq-1; independent Lagrangian energy method):
    //   J_eff = I_cr + m_cp·a² + (I_cp + I_ct + I_rt) / 9
    //         = 0.10 + 3·0.04² + (0.20 + 0.02 + 0.01)/9
    //         = 0.10 + 0.0048 + 0.23/9   ← (I_cp+I_ct+I_rt)/9: each term (ω_abs/ω)²=1/9
    //         ≈ 0.130356 kg·m²
    //   (Coupler COM at crank-tip B: |v_B|=ω·a; crank/rocker COMs at pivots: v=0;
    //    coupler_tip/rocker_tip COMs at standstill pivot C: v=0; see header.)
    //
    // At α=π rad/s² (chosen ≠ω for accels≠vels distinctness on sample_1):
    //   dE_dt_analytic = J_eff · ω · α = 0.130356 · 2π · π ≈ 2.573 W.
    //
    // Tolerance (G6 DERIVE-AND-BIND; NOT the 2-prismatic 1 µW literal):
    //   The power floor ~|λ|·O(ε_N·ω) where ε_N≤tol_pos_m=1e-6 m, ω=2π.
    //   A-priori: |λ|~O(1 kg·π·0.12 m)~0.38 Nm → floor~2.4 µW.
    //   tol is set to 10× the MEASURED floor (power_0.abs()), with the
    //   provenance and measured value recorded in the comment below.
    //   Step-4 replaces this placeholder with the measured binding.
    let omega = 2.0 * std::f64::consts::PI; // crank rate (rad/s)
    let alpha = std::f64::consts::PI;        // chosen crank angular accel (rad/s²), ≠ω
    let j_eff = 0.130356_f64;               // kg·m² (see provenance above)
    let de_dt_analytic = j_eff * omega * alpha; // ≈ 2.573 W

    // PLACEHOLDER tolerance — finalized in step-4 to 10× MEASURED power_0 floor.
    // The a-priori estimate (~24 µW) confirms the placeholder (1e-2 W) is safe.
    let tol = 1e-2_f64;

    // Margin guard: dE/dt ≫ tol (test is non-vacuous; dE/dt≈2.573 W ≫ 0.01 W).
    assert!(
        de_dt_analytic > tol * 10.0,
        "analytic dE/dt ({de_dt_analytic:.6} W) must be ≫ tol ({tol:.6e} W) \
         for a non-vacuous test (margin guard)"
    );

    // (a) α=0 sample: |power_0| < tol validates ½q̇ᵀṀq̇=0 at standstill AND
    //     measures the Newton-residual floor for this four-bar.
    assert!(
        power_0.abs() < tol,
        "power at accels=0 (power_0={power_0:.6e} W) must be < tol={tol:.6e} W \
         (validates ½q̇ᵀṀq̇=0 at standstill pivot-C AND measures residual floor). \
         A mismatch indicates a real bridge bug — diagnose, do not retune. \
         Step-4 finalizes tol to 10× this measured floor."
    );

    // (b) Power increment isolates J_eff·ω·α and cancels the floor.
    let delta = (power_alpha - power_0) - de_dt_analytic;
    assert!(
        delta.abs() < tol,
        "virtual-work identity: (power_α − power_0) = {:.6} W, analytic dE/dt = {:.6} W, \
         Δ = {:.6e} W (tol = {tol:.6e} W). \
         J_eff = {j_eff} kg·m², ω = {omega:.6} rad/s, α = {alpha:.6} rad/s². \
         A mismatch indicates a real bridge bug — diagnose, do not retune.",
        power_alpha - power_0,
        de_dt_analytic,
        delta
    );
}
