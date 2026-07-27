//! OCCT-gated integration tests for the mount→origin handshake seam —
//! geometric-joints δ (task 4398).
//!
//! ## What this tests
//!
//! The B5 signal: after the per-scope relate-solve (ζ, task 4386) yields a concrete
//! mount `Value::Frame` for a joint's mounting datums, `reify_stdlib::set_mount_origin`
//! lifts that Frame into a `Value::Transform` and inserts it under the `"origin"` key
//! of the joint `Value::Map` — the field KIN-OFFSET α (task 4331) threads through
//! `transform_at`'s `origin ∘ motion` pre-compose.
//!
//! ## Test organisation
//!
//! - **B5 (direct path, step-3)**: drives `collect_relate_scope` →
//!   `realize_operand_datums` → `solve_relate_scope` over the §1 bolt-plate example
//!   with a real OCCT kernel, obtains the bolt's solved `Value::Frame`, then manually
//!   constructs a revolute joint and applies `set_mount_origin` — asserting the
//!   resulting joint Map's `"origin"` is a `Value::Transform` whose translation equals
//!   the solved mount's translation.  The §1 geometry places the bolt at identity
//!   (trivially satisfied constraints), so the translation may be zero; the test's
//!   real value is verifying the full realize → solve → write chain is correct,
//!   independent of magnitude.  Guards with `OCCT_AVAILABLE`.
//!
//! - **B9 back-compat (step-5)**: added by step-5/6 — engine-build-path tests that
//!   assert joints NOT mounted by a relate scope carry NO `"origin"` key.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are
// by-design.
#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;

use reify_eval::relate_solve::{RelateScope, collect_relate_scope, realize_operand_datums, solve_relate_scope};
use reify_ir::{ExportFormat, Value};
use reify_test_support::compile_source_with_stdlib;

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Spawn an OCCT-backed `Engine`, mirroring the `relate_solve_e2e.rs` harness.
fn occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)))
}

/// Read the §1 bolt-plate example source (same file as `relate_solve_e2e.rs` uses),
/// so the two test files exercise identical geometry without source drift.
fn bolt_plate_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/geometric_relations/bolt_plate.ri"
    );
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read §1 example {path}: {e}"))
}

/// An identity seed Frame for the bolt's `at auto` unknown.  Realization is
/// pose-independent, so any seed yields identical local datums.
fn identity_bolt_seeds() -> HashMap<String, Value> {
    [("bolt".to_string(), Value::Frame {
        origin: Box::new(Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
        basis: Box::new(Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }),
    })]
    .into_iter()
    .collect()
}

/// Decompose a `Value::Transform` into `((w,x,y,z), [tx,ty,tz])`, panicking with
/// `label` on mismatch.
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
    let read_f64 = |v: &Value, l: &str| v.as_f64().unwrap_or_else(|| panic!("{l}: not numeric"));
    (
        (w, x, y, z),
        [
            read_f64(&comps[0], &format!("{label}.t[0]")),
            read_f64(&comps[1], &format!("{label}.t[1]")),
            read_f64(&comps[2], &format!("{label}.t[2]")),
        ],
    )
}

// ── B5: direct-path test (step-3) ────────────────────────────────────────────

/// B5 (step-3/6, OCCT-gated): the relate-solve → `set_mount_origin` chain writes
/// the solved `Value::Frame` into the joint Map's `"origin"` key as a
/// `Value::Transform` that matches the solved placement.
///
/// Drives the real per-scope relate-solve over the §1 bolt-plate scope, obtains
/// the bolt's solved `Value::Frame` from `RelateSolution.poses["bolt"]`, constructs
/// a bare (no-origin) revolute joint via `reify_stdlib::eval_builtin`, applies
/// `reify_stdlib::set_mount_origin`, and asserts:
///
/// - The result is a `Value::Map` (joint Map preserved).
/// - `"kind"`, `"axis"`, `"range"` keys survive the write.
/// - `"origin"` is a `Value::Transform` (not Undef, not absent).
/// - `origin.translation` matches the solved bolt Frame's own origin (the Frame
///   the relate-solve placed the bolt at IS the origin the joint Map carries).
/// - The DOF accounting is correct: 5 DOF spent, 1 residual spin.
///
/// NOTE: the §1 bolt-plate geometry has both the bolt shank axis and the plate hole
/// axis at the Z-axis through origin in their respective local frames.  The relate-
/// solve correctly places the bolt at identity (the constraints are trivially satisfied
/// there) — the bolt's translation may be zero.  The nonzero-pivot B5 variant is
/// tested kernel-free in the unit tests (`set_mount_origin_writes_frame_origin_into_joint_map`
/// in `crates/reify-stdlib/src/joints.rs`).  This integration test verifies the FULL
/// CHAIN (realize → solve → write) is correct, independent of the translation magnitude.
#[test]
fn relate_solve_chain_writes_solved_frame_origin_into_joint() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping relate_solve_chain_writes_solved_frame_origin_into_joint (B5): \
             OCCT not available"
        );
        return;
    }

    // 1. Compile the §1 bolt-plate, collect + realize + solve.
    let source = bolt_plate_source();
    let module = compile_source_with_stdlib(&source);

    let bp_template = module
        .templates
        .iter()
        .find(|t| t.name == "BoltPlate")
        .unwrap_or_else(|| panic!("BoltPlate template not found; module diagnostics: {:#?}", module.diagnostics));
    let scope: RelateScope = collect_relate_scope(bp_template);

    let mut engine = occt_engine();
    let realized = realize_operand_datums(&scope, &module, &mut engine, &identity_bolt_seeds());
    let solution = solve_relate_scope(&scope, &realized);

    // 2. The solve must have placed the bolt (no infeasibility diagnostics).
    assert!(
        solution.diagnostics.is_empty(),
        "relate-solve must be clean (no conflict/assertion diagnostics), got: {:#?}",
        solution.diagnostics
    );
    let bolt_frame = solution
        .poses
        .get("bolt")
        .unwrap_or_else(|| panic!("solve must produce a solved Frame for the bolt auto-sub"));
    assert!(
        matches!(bolt_frame, Value::Frame { .. }),
        "solved bolt pose must be Value::Frame, got {bolt_frame:?}"
    );

    // 3. Extract the solved Frame's origin components for later comparison.
    let frame_origin_comps = match bolt_frame {
        Value::Frame { origin, .. } => match origin.as_ref() {
            Value::Point(c) if c.len() == 3 => c.clone(),
            other => panic!("bolt Frame origin must be Point(3), got {other:?}"),
        },
        _ => unreachable!(),
    };
    let frame_tx = frame_origin_comps[0].as_f64().expect("frame origin[0] numeric");
    let frame_ty = frame_origin_comps[1].as_f64().expect("frame origin[1] numeric");
    let frame_tz = frame_origin_comps[2].as_f64().expect("frame origin[2] numeric");

    // 3a. Verify the DOF accounting (§1: 5 spent, 1 residual, 2 driving, 0 redundant).
    assert_eq!(solution.spent, 5, "§1 spends 5 DOF (concentric 4 + flush net 1)");
    assert_eq!(solution.free, 1, "§1 leaves 1 residual DOF (spin about shared axis)");
    assert_eq!(solution.driving, 2, "§1 has 2 driving relations");
    assert_eq!(solution.redundant, 0, "§1 has 0 redundant relations");

    // 4. Construct a bare revolute joint (no origin), apply set_mount_origin.
    // Axis: unit +Z vector (the standard revolute axis in the test fixtures).
    let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
    // Range: [0, π] angle range.
    let range = Value::Range {
        lower: Some(Box::new(Value::angle(0.0))),
        upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
        lower_inclusive: true,
        upper_inclusive: true,
    };
    let bare_joint = reify_stdlib::eval_builtin("revolute", &[axis, range]);
    assert!(
        matches!(&bare_joint, Value::Map(m) if !m.contains_key(&Value::String("origin".to_string()))),
        "bare 2-arg revolute must have no 'origin' key (precondition)"
    );

    let joint_with_origin = reify_stdlib::set_mount_origin(bare_joint, bolt_frame);

    // 5. Assert the result Map has the correct "origin" Transform.
    let map = match &joint_with_origin {
        Value::Map(m) => m,
        other => panic!("set_mount_origin must return a Map, got {other:?}"),
    };

    // Structural keys preserved.
    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("revolute".to_string())),
        "'kind' must be preserved after set_mount_origin"
    );
    assert!(map.contains_key(&Value::String("axis".to_string())), "'axis' must be preserved");
    assert!(map.contains_key(&Value::String("range".to_string())), "'range' must be preserved");

    // "origin" must now be present and be a Transform (B5 core: the write happened).
    let origin = map
        .get(&Value::String("origin".to_string()))
        .unwrap_or_else(|| panic!("set_mount_origin must insert 'origin' key for a Frame mount"));
    assert!(
        matches!(origin, Value::Transform { .. }),
        "'origin' must be Value::Transform, got {origin:?}"
    );

    // "origin.translation" must match the solved bolt Frame's origin exactly
    // (the full-chain correctness assertion: realize → solve → set_mount_origin → read back).
    let (_, [tx, ty, tz]) = decompose_transform(origin, "joint origin");
    assert!(
        (tx - frame_tx).abs() < 1e-9,
        "origin.tx must equal solved bolt Frame tx ({frame_tx}), got {tx}"
    );
    assert!(
        (ty - frame_ty).abs() < 1e-9,
        "origin.ty must equal solved bolt Frame ty ({frame_ty}), got {ty}"
    );
    assert!(
        (tz - frame_tz).abs() < 1e-9,
        "origin.tz must equal solved bolt Frame tz ({frame_tz}), got {tz}"
    );
}

// ── B9: back-compat / no spurious origin (step-5) ────────────────────────────

/// Build a module with a relate-placed scope AND an unrelated standalone joint
/// that the scope does NOT mount.  Asserts the unrelated joint Map carries NO
/// `"origin"` key — only joints actually mounted by a relate scope (via
/// `joint … with`, task #4399) receive an origin.
///
/// This verifies that the engine_build.rs wiring (step-4) does NOT write
/// origins to joints it has no named association with (the `mounted_joint_cell`
/// stub correctly returns `None` for every unrelated joint).
///
/// OCCT-gated: the module has a relate scope that requires the geometry kernel
/// for the relate-solve. When OCCT is unavailable the test skips cleanly.
#[test]
fn unrelated_joint_in_relate_scope_module_has_no_origin() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping unrelated_joint_in_relate_scope_module_has_no_origin (B9): \
             OCCT not available"
        );
        return;
    }

    // A BoltPlate-like module that also defines a standalone joint
    // (`let arm_joint = revolute(...)`) that is NOT connected to the relate scope.
    let source = r#"
structure Bolt {
    let shank = cylinder(3mm, 20mm)
    let shank_axis : Axis = shank.axis
    let seat = rectangle(12mm, 12mm)
    let seat_plane : Plane = seat.plane
}

structure Plate {
    let body = box(40mm, 40mm, 5mm)
    let hole = cylinder(3.2mm, 5mm)
    let hole_axis : Axis = hole.axis
    let top = rectangle(40mm, 40mm)
    let top_plane : Plane = top.plane
}

structure BoltPlateWithJoint {
    let arm_joint = revolute(vec3(0, 0, 1), 0rad .. 3.14rad)
    sub bolt : Bolt at auto
    sub plate : Plate
    relate {
        concentric(bolt.shank_axis, plate.hole_axis)
        flush(bolt.seat_plane, plate.top_plane)
    }
}
"#;

    use reify_core::ValueCellId;

    let compiled = compile_source_with_stdlib(source);
    assert!(
        compiled.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
        "source must compile without errors; got: {:#?}",
        compiled.diagnostics
    );

    let mut engine = occt_engine();
    let result = engine.build(&compiled, ExportFormat::Step);

    // The build must not error (relate-solve is expected to succeed).
    let build_errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "build must produce no Error diagnostics, got: {build_errors:#?}"
    );

    // Find the `arm_joint` value cell.
    let arm_joint_id = ValueCellId::new("BoltPlateWithJoint", "arm_joint");
    let arm_joint_val = result
        .values
        .get(&arm_joint_id)
        .unwrap_or_else(|| panic!("BoltPlateWithJoint.arm_joint not found in build result"));

    // It must be a Map (revolute produces a Value::Map).
    let map = match arm_joint_val {
        Value::Map(m) => m,
        other => panic!("arm_joint must be a Value::Map, got {other:?}"),
    };

    // B9: the unrelated joint must NOT have an "origin" key.
    assert!(
        !map.contains_key(&Value::String("origin".to_string())),
        "B9: unrelated joint Map must carry NO 'origin' key; \
         mounted_joint_cell should return None for a joint with no \
         `joint … with` association, got map keys: {:?}",
        map.keys().collect::<Vec<_>>()
    );

    // The standard joint fields must still be present (byte-identical structure).
    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("revolute".to_string())),
        "B9: joint 'kind' field must be preserved"
    );
    assert!(
        map.contains_key(&Value::String("axis".to_string())),
        "B9: joint 'axis' field must be preserved"
    );
    assert!(
        map.contains_key(&Value::String("range".to_string())),
        "B9: joint 'range' field must be preserved"
    );
}

/// A pure-mechanism module with no relate scope leaves joint Maps byte-identical
/// (no `"origin"` key added by the engine build pass).
///
/// Does not require OCCT: no relate scope → relate-solve is skipped → the
/// `mounted_joint_cell` path is never entered.
#[test]
fn pure_mechanism_module_joint_has_no_origin() {
    use reify_core::ValueCellId;
    use reify_test_support::make_simple_engine;

    let source = r#"
structure PureMechanism {
    let j = revolute(vec3(0, 0, 1), 0rad .. 3.14rad)
}
"#;

    let compiled = compile_source_with_stdlib(source);
    assert!(
        compiled.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
        "source must compile without errors; got: {:#?}",
        compiled.diagnostics
    );

    let mut engine = make_simple_engine();
    let result = engine.build(&compiled, ExportFormat::Step);

    let j_id = ValueCellId::new("PureMechanism", "j");
    let j_val = result
        .values
        .get(&j_id)
        .unwrap_or_else(|| panic!("PureMechanism.j not found in build result"));

    let map = match j_val {
        Value::Map(m) => m,
        other => panic!("PureMechanism.j must be a Value::Map, got {other:?}"),
    };

    // B9: no relate scope → no origin must be injected.
    assert!(
        !map.contains_key(&Value::String("origin".to_string())),
        "B9: pure-mechanism joint must have NO 'origin' key; \
         got map keys: {:?}",
        map.keys().collect::<Vec<_>>()
    );

    // Standard joint fields intact.
    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("revolute".to_string())),
        "B9: pure-mechanism joint 'kind' must be 'revolute'"
    );
}
