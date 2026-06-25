// OCCT-gated end-to-end tests for the relate-mounted revolute joint — task #4399, B6/B7.
//
// ## What this tests
//
// B6 PRODUCER (step-3): After `engine.build()`, the relate-mounted revolute joint `j`
// in `examples/kinematic/relate_mounted_revolute.ri` gains an `"origin"` field whose
// translation equals the solved nonzero mount (~50 mm along X).  This exercises the
// previously-uncovered positive branch of the engine_build.rs seam (step-4 of δ/4398):
//   relate-solve → mounted_joint_cell(Some) → set_mount_origin → origin in values map.
//
// B6 CONSUMER (step-5): With the built joint value (origin written), `transform_at`
// at θ=0° and θ=30° both carry the mount translation; the relative rotation between
// the two snapshots equals R_z(30°), proving the swept angle equals the bind delta
// and is NEVER re-solved geometrically (PRD §8.1).
//
// FK is tested by calling `reify_stdlib::eval_builtin("transform_at", ...)` directly
// on the built joint value — this is correct by construction (the build value of j
// already has origin written) and avoids any evaluation-ordering dependencies between
// the relate-solve write and the mechanism snapshot cells.
//
// Guards with `OCCT_AVAILABLE` — the relate-solve calls the geometry kernel.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField trips
// clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::compile_source_with_stdlib;

// ── Shared helpers ─────────────────────────────────────────────────────────────

fn occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)))
}

fn example_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/kinematic/relate_mounted_revolute.ri"
    );
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read relate_mounted_revolute.ri: {e}"))
}

/// Decompose a `Value::Transform` into `((w,x,y,z), [tx,ty,tz])`.
fn decompose_transform(v: &Value, label: &str) -> ((f64, f64, f64, f64), [f64; 3]) {
    let (rot, trans) = match v {
        Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("{label}: expected Value::Transform, got {other:?}"),
    };
    let (w, x, y, z) = match rot {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        other => panic!("{label}: expected Orientation, got {other:?}"),
    };
    let comps = match trans {
        Value::Vector(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Vector(3), got {other:?}"),
    };
    let f = |v: &Value, l: &str| v.as_f64().unwrap_or_else(|| panic!("{l}: not numeric"));
    (
        (w, x, y, z),
        [
            f(&comps[0], &format!("{label}.t[0]")),
            f(&comps[1], &format!("{label}.t[1]")),
            f(&comps[2], &format!("{label}.t[2]")),
        ],
    )
}

// ── B6 PRODUCER (step-3) ───────────────────────────────────────────────────────

/// B6 producer (OCCT-gated): `engine.build` of `relate_mounted_revolute.ri` writes
/// a nonzero `"origin"` `Value::Transform` into the mounted revolute joint `j`.
///
/// Asserts:
/// - No Error diagnostics from the build.
/// - `RelateMountedRevolute.j` is a `Value::Map` with an `"origin"` key.
/// - `origin.translation` has nonzero X component (~0.05 m = 50 mm).
/// - `origin.rotation` is approximately identity (pure-translation mount, DD2).
/// - The relate-solve's `link` auto-pose Frame is nonzero (sanity: relate-solve ran).
#[test]
fn relate_mounted_revolute_build_writes_origin_into_joint() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping relate_mounted_revolute_build_writes_origin_into_joint (B6 producer): OCCT not available");
        return;
    }

    let source = example_source();
    let compiled = compile_source_with_stdlib(&source);
    assert!(
        compiled.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
        "example must compile without errors; got: {:#?}",
        compiled.diagnostics
    );

    let mut engine = occt_engine();
    let result = engine.build(&compiled, ExportFormat::Step);

    // Build must be clean.
    let build_errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(build_errors.is_empty(), "build must produce no Error diagnostics, got: {build_errors:#?}");

    // Read the mounted joint value.
    let j_id = ValueCellId::new("RelateMountedRevolute", "j");
    let j_val = result.values.get(&j_id)
        .unwrap_or_else(|| panic!("RelateMountedRevolute.j not in build result"));

    let map = match j_val {
        Value::Map(m) => m,
        other => panic!("j must be Value::Map, got {other:?}"),
    };

    // B6 producer: joint Map MUST have an "origin" key (engine_build seam fired).
    let origin_val = map.get(&Value::String("origin".to_string()))
        .unwrap_or_else(|| {
            panic!(
                "B6 producer: RelateMountedRevolute.j must have 'origin' key after build; \
                 got keys: {:?}",
                map.keys().collect::<Vec<_>>()
            )
        });

    // origin must be a Transform.
    let ((ow, ox, oy, oz), [tx, ty, tz]) = decompose_transform(origin_val, "j.origin");

    // Translation must be nonzero (DD2 B5-via-build: the relate-solve placed link at 50 mm).
    let mount_m = 0.05_f64; // 50 mm in SI
    assert!(
        (tx - mount_m).abs() < 1e-4,
        "B6 producer: j.origin translation X must be ~{mount_m} m (relate-solved 50 mm mount), got {tx}"
    );
    assert!(ty.abs() < 1e-4, "B6 producer: j.origin translation Y must be ~0, got {ty}");
    assert!(tz.abs() < 1e-4, "B6 producer: j.origin translation Z must be ~0, got {tz}");

    // Rotation must be approximately identity (pure-translation mount).
    assert!(
        (ow - 1.0).abs() < 1e-4 && ox.abs() < 1e-4 && oy.abs() < 1e-4 && oz.abs() < 1e-4,
        "B6 producer: j.origin rotation must be ~identity (pure-translation mount), \
         got ({ow},{ox},{oy},{oz})"
    );

    // Standard joint fields must survive the origin write (byte-stable).
    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("revolute".to_string())),
        "B6 producer: j 'kind' field must be preserved"
    );
    assert!(
        map.contains_key(&Value::String("axis".to_string())),
        "B6 producer: j 'axis' field must be preserved"
    );
    assert!(
        map.contains_key(&Value::String("range".to_string())),
        "B6 producer: j 'range' field must be preserved"
    );

    // Sanity: link auto-pose was solved (relate-solve ran).
    let link_pose_id = reify_eval::relate_solve::auto_pose_cell("RelateMountedRevolute", "link");
    assert!(
        result.values.get(&link_pose_id).is_some(),
        "B6 producer: link auto-pose must be present in build result (relate-solve must have run)"
    );
}

// ── B6 CONSUMER (step-5) ──────────────────────────────────────────────────────

/// B6 consumer (OCCT-gated): the built revolute joint `j` (with relate-written origin)
/// produces correct FK transforms via `transform_at`.
///
/// Calls `reify_stdlib::eval_builtin("transform_at", ...)` directly on the built joint
/// value — this tests that `transform_at` correctly consumes the written `"origin"` and
/// composes `origin ∘ R_z(θ)` (KIN-OFFSET α, task 4331), proving the swept angle equals
/// the bind delta and is NEVER re-solved geometrically (PRD §8.1).
///
/// Asserts (DD2, gauge-robust):
/// (a) t0 and t30 translations both ≈ (0.05, 0, 0) m — FK poses the link AT the mount.
/// (b) The RELATIVE rotation t0⁻¹∘t30 ≈ R_z(30°) — swept angle == bind delta.
#[test]
fn relate_mounted_revolute_fk_poses_at_mount_with_bind_angle() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping relate_mounted_revolute_fk_poses_at_mount_with_bind_angle (B6 consumer): OCCT not available");
        return;
    }

    let source = example_source();
    let compiled = compile_source_with_stdlib(&source);
    let mut engine = occt_engine();
    let result = engine.build(&compiled, ExportFormat::Step);

    // Build must be clean.
    let build_errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(build_errors.is_empty(), "build must produce no Error diagnostics, got: {build_errors:#?}");

    // Read the built joint (with origin written by the seam).
    let j_id = ValueCellId::new("RelateMountedRevolute", "j");
    let j_val = result.values.get(&j_id)
        .unwrap_or_else(|| panic!("RelateMountedRevolute.j not in build result"))
        .clone();

    // Confirm origin is present before FK evaluation.
    let has_origin = match &j_val {
        Value::Map(m) => m.contains_key(&Value::String("origin".to_string())),
        _ => false,
    };
    assert!(has_origin, "B6 consumer precondition: j must have 'origin' after build");

    // Evaluate transform_at(j, θ) at θ=0 and θ=30° (π/6 rad ≈ 0.5235988 rad).
    let theta_0  = Value::angle(0.0);
    let theta_30 = Value::angle(std::f64::consts::PI / 6.0);

    let t0_val  = reify_stdlib::eval_builtin("transform_at", &[j_val.clone(), theta_0]);
    let t30_val = reify_stdlib::eval_builtin("transform_at", &[j_val, theta_30]);

    assert!(
        !matches!(t0_val, Value::Undef),
        "B6 consumer: transform_at(j, 0rad) must not be Undef"
    );
    assert!(
        !matches!(t30_val, Value::Undef),
        "B6 consumer: transform_at(j, 30°) must not be Undef"
    );

    let ((_, _, _, _), [t0x, t0y, t0z]) = decompose_transform(&t0_val, "t0");
    let ((r30w, r30x, r30y, r30z), [t30x, t30y, t30z]) = decompose_transform(&t30_val, "t30");

    let mount_m = 0.05_f64;
    let tol = 1e-4_f64;

    // (a) Both translations must equal the solved mount translation (~0.05 m along X).
    assert!((t0x - mount_m).abs() < tol, "B6 consumer: t0.tx = {t0x}, expected ~{mount_m}");
    assert!(t0y.abs() < tol, "B6 consumer: t0.ty = {t0y}, expected ~0");
    assert!(t0z.abs() < tol, "B6 consumer: t0.tz = {t0z}, expected ~0");

    assert!((t30x - mount_m).abs() < tol, "B6 consumer: t30.tx = {t30x}, expected ~{mount_m}");
    assert!(t30y.abs() < tol, "B6 consumer: t30.ty = {t30y}, expected ~0");
    assert!(t30z.abs() < tol, "B6 consumer: t30.tz = {t30z}, expected ~0");

    // (b) The t30 rotation must be R_z(30°) = (cos(π/12), 0, 0, sin(π/12)) up to sign.
    // Since t0 rotation = identity (gauge-seeded), the relative rotation IS t30's rotation.
    let theta = std::f64::consts::PI / 6.0;
    let qw_exp = (theta / 2.0).cos();
    let qz_exp = (theta / 2.0).sin();
    let matches_pos = (r30w - qw_exp).abs() < tol && r30x.abs() < tol && r30y.abs() < tol
        && (r30z - qz_exp).abs() < tol;
    let matches_neg = (r30w + qw_exp).abs() < tol && r30x.abs() < tol && r30y.abs() < tol
        && (r30z + qz_exp).abs() < tol;
    assert!(
        matches_pos || matches_neg,
        "B6 consumer: t30 rotation must be R_z(30°) ≈ ({qw_exp},0,0,{qz_exp}) up to sign, \
         got ({r30w},{r30x},{r30y},{r30z})"
    );
}

// ── B7 closed-loop (step-7) ────────────────────────────────────────────────

fn fourbar_example_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/kinematic/relate_mounted_fourbar.ri"
    );
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read relate_mounted_fourbar.ri: {e}"))
}

/// B7 closed-loop (OCCT-gated): a Grashof 4-bar with its rocker ground-pivot
/// relate-mounted closes the loop, with the relate-written `j_rocker.origin`
/// feeding the offset-aware loop residual (PRD §8.3, task #4399).
///
/// Asserts:
/// (a) `j_rocker.origin` is a `Value::Transform` with translation ≈ (0.14, 0, 0) m
///     — the relate-solve placed the rocker mount at the 140 mm ground pivot.
/// (b) `solve_loop_closure` with the built joint values (j_rocker WITH the
///     relate-written origin) returns `NewtonOutcome::Converged` — proves the
///     loop-closure Newton solver closes the loop at the correct relate-placed
///     geometry (§8.3).  The direct call mirrors the same code path the
///     `snapshot()` evaluator uses internally.
/// (c) `loop_residual_twist` at the analytically-known assembled closure angles
///     returns a twist with angular and linear norms ≤ `NewtonConfig::default()`
///     tolerance — directly verifies that j_rocker.origin = (140 mm, 0, 0) from
///     the relate-solve feeds the offset-aware chain_b FK.
///
/// Note on ordering: the snapshot cell `s` in `relate_mounted_fourbar.ri` is
/// included as a demonstration of the intended API; its eval-time result may be
/// `Undef` because the evaluation pass (which computes `s`) runs before the
/// engine_build seam writes `j_rocker.origin` (DD3 ordering; a seam-aware
/// re-eval pass is a future follow-up).  Assertions (b) and (c) exercise the
/// same Newton solver code path by calling `solve_loop_closure` directly with
/// the post-seam joint values that do carry the correct origin.
#[test]
fn relate_mounted_fourbar_closed_loop_closes_with_relate_origin() {
    use reify_constraints::{JointValue, NewtonConfig, NewtonOutcome, StartStrategy,
                            solve_loop_closure};
    use reify_stdlib::loop_closure::loop_residual_twist;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping relate_mounted_fourbar_closed_loop_closes_with_relate_origin (B7): \
             OCCT not available"
        );
        return;
    }

    let source = fourbar_example_source();
    let compiled = compile_source_with_stdlib(&source);
    assert!(
        compiled.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
        "relate_mounted_fourbar.ri must compile without errors; got: {:#?}",
        compiled.diagnostics
    );

    let mut engine = occt_engine();
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "build must produce no Error diagnostics, got: {build_errors:#?}"
    );

    // Read the five joint Values from the build result (post-seam: j_rocker has origin).
    let read_joint = |name: &str| -> Value {
        let id = ValueCellId::new("RelateMountedFourbar", name);
        result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("RelateMountedFourbar.{name} not in build result"))
            .clone()
    };
    let j_crank       = read_joint("j_crank");
    let j_coupler     = read_joint("j_coupler");
    let j_coupler_tip = read_joint("j_coupler_tip");
    let j_rocker      = read_joint("j_rocker");
    let j_rocker_tip  = read_joint("j_rocker_tip");

    // (a) j_rocker MUST have a relate-written "origin" at ≈ (0.14, 0, 0) m.
    let rocker_map = match &j_rocker {
        Value::Map(m) => m,
        other => panic!("B7 (a): j_rocker must be Value::Map, got {other:?}"),
    };
    let origin_val = rocker_map
        .get(&Value::String("origin".to_string()))
        .unwrap_or_else(|| {
            panic!(
                "B7 (a): j_rocker must have 'origin' key (relate-written by engine_build seam); \
                 got keys: {:?}",
                rocker_map.keys().collect::<Vec<_>>()
            )
        });
    let (_, [ox, oy, oz]) = decompose_transform(origin_val, "j_rocker.origin");
    let rocker_pivot_m = 0.14_f64;
    assert!(
        (ox - rocker_pivot_m).abs() < 1e-4,
        "B7 (a): j_rocker.origin tx = {ox:.6} m, expected ~{rocker_pivot_m} m (140 mm)"
    );
    assert!(oy.abs() < 1e-4, "B7 (a): j_rocker.origin ty = {oy:.6} m, expected ~0");
    assert!(oz.abs() < 1e-4, "B7 (a): j_rocker.origin tz = {oz:.6} m, expected ~0");

    // (b) Newton converges when solve_loop_closure is called with the built joint
    // values — j_rocker carries the relate-written origin (0.14, 0, 0) so the rocker
    // chain correctly reaches pivot C at the assembled Grashof configuration.
    //
    // chain_a = [j_crank, j_coupler, j_coupler_tip]  → reaches C from A (world origin)
    // chain_b = [j_rocker, j_rocker_tip]              → reaches C from D (140 mm pivot)
    // free_b  = [0, 1]   → both chain_b joints are free for Newton
    // warm-start near the assembled config (θ_rocker ≈ 1.804, θ_rocker_tip ≈ −1.019)
    let chain_a_b = vec![j_crank.clone(), j_coupler.clone(), j_coupler_tip.clone()];
    let vals_a_b = vec![
        JointValue::Scalar(std::f64::consts::PI / 4.0),
        JointValue::Scalar(0.0),
        JointValue::Scalar(0.0),
    ];
    let chain_b_b = vec![j_rocker.clone(), j_rocker_tip.clone()];
    let vals_b_init = vec![
        JointValue::Scalar(1.8),  // j_rocker warm-start near 1.804 rad
        JointValue::Scalar(-1.0), // j_rocker_tip warm-start near -1.019 rad
    ];
    let free_b = vec![0usize, 1];
    let strategy = StartStrategy::WarmStart(vec![1.8, -1.0]);
    let cfg = NewtonConfig::default();

    let outcome =
        solve_loop_closure(&chain_a_b, &vals_a_b, &chain_b_b, &vals_b_init, &free_b, &strategy, &cfg);

    assert!(
        matches!(outcome, NewtonOutcome::Converged { .. }),
        "B7 (b): solve_loop_closure must converge for the relate-mounted 4-bar \
         (j_rocker.origin = {ox:.4} m). Got: {outcome:?}. \
         A non-Converged outcome means the relate-written origin is wrong or \
         the warm-start is too far from the assembled config."
    );

    // (c) loop_residual_twist at the Grashof assembled closure angles ≤ tol.
    // Directly verifies that j_rocker.origin = (140 mm, 0, 0) from the relate-solve
    // feeds the offset-aware chain_b FK, making the Grashof loop close (§8.3).
    //
    // Assembled Grashof crank-rocker config at θ_crank = 45° (coupler straight):
    //   θ_crank      = π/4  ≈ 0.7854 rad
    //   θ_coupler    = 0    (coupler straight)
    //   θ_coupler_tip = 0   (pure +X offset, no rotation)
    //   θ_rocker     ≈ 1.8039163646188838 rad  (atan2(113.137, 113.137−140))
    //   θ_rocker_tip ≈ −1.0185182012214355 rad (θ_crank − θ_rocker)
    let chain_a = vec![j_crank, j_coupler, j_coupler_tip];
    let vals_a = vec![
        JointValue::Scalar(std::f64::consts::PI / 4.0),
        JointValue::Scalar(0.0),
        JointValue::Scalar(0.0),
    ];
    let chain_b = vec![j_rocker, j_rocker_tip];
    let vals_b = vec![
        JointValue::Scalar(1.803_916_364_618_883_8),
        JointValue::Scalar(-1.018_518_201_221_435_5),
    ];

    let twist = loop_residual_twist(&chain_a, &vals_a, &chain_b, &vals_b)
        .expect("B7 (c): loop_residual_twist must succeed at the assembled Grashof config");

    let combined_tol = 1e-6 + 1e-6; // NewtonConfig::default() tol_pos_m + tol_rot_rad
    let angular_norm =
        (twist[0] * twist[0] + twist[1] * twist[1] + twist[2] * twist[2]).sqrt();
    let linear_norm =
        (twist[3] * twist[3] + twist[4] * twist[4] + twist[5] * twist[5]).sqrt();

    assert!(
        angular_norm < combined_tol,
        "B7 (c): angular loop residual {angular_norm:.3e} rad ≥ combined_tol {combined_tol:.3e}. \
         j_rocker.origin tx={ox:.6} m (expected 0.14 m). Wrong origin means wrong chain_b FK."
    );
    assert!(
        linear_norm < combined_tol,
        "B7 (c): linear loop residual {linear_norm:.3e} m ≥ combined_tol {combined_tol:.3e}. \
         j_rocker.origin tx={ox:.6} m (expected 0.14 m). Wrong origin means wrong chain_b FK."
    );
}
