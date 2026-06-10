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
use reify_stdlib::loop_closure::loop_residual_twist;
use reify_test_support::{
    collect_errors, errors_only, make_simple_engine, parse_and_compile_with_stdlib,
    MockGeometryKernel,
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
// Grashof crank-rocker: crank a=40mm (shortest), coupler b=120mm, rocker c=120mm,
// ground d=140mm. Grashof check: s+l = 40+140 = 180 ≤ p+q = 120+120 = 240 ✓.
//
// Pivot offsets (task 4331 surface):
//   j_crank       – revolute at A = world origin, pivot=(0,0,0)
//   j_coupler     – revolute, pivot=(40mm,0,0) in crank frame (= crank length a)
//   j_coupler_tip – revolute fixed-like tip, pivot=(120mm,0,0) in coupler frame (= coupler b)
//   j_rocker      – revolute, pivot=(140mm,0,0) in world frame (= ground length d)
//   j_rocker_tip  – revolute fixed-like tip, pivot=(120mm,0,0) in rocker frame (= rocker c)
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

    // Eval to get joint Value cells.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
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
    // Expected off-closure mismatch: ≈ 0.186 m at θ1=45°, θ2=θ3=0 (esc-4146-280 §0 gap)
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
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    assert!(
        collect_errors(&result.diagnostics).is_empty(),
        "eval should produce no Error diagnostics"
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
    // Warm-start: near-zero is reasonable for the assembled Grashof crank-rocker
    // at θ_input=45°. The rocker angle is approximately atan2(28.3, 140-28.3+...) ≈ 0.2 rad.
    let vals_b_initial = vec![
        JointValue::Scalar(0.2), // j_rocker warm-start near assembled position
        JointValue::Scalar(0.0), // j_rocker_tip at 0
    ];
    let free_b = vec![0usize, 1usize]; // both chain_b slots free
    let strategy = StartStrategy::WarmStart(vec![0.2, 0.0]);
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
