//! End-to-end smoke test for the v0.1 forward-kinematics stdlib (task 2535).
//!
//! Drives the new `bind()` / `snapshot()` / `bodies()` / `transform_of()` /
//! `bounding_box()` / `center_of_mass()` builtins through the full
//! `parse → compile_with_stdlib → eval` pipeline.  Mirrors the structure of
//! `mechanism_builder_smoke.rs` (mechanism builder) and
//! `kinematic_stdlib_smoke.rs` (joint builtins).
//!
//! See docs/prds/kinematic-constraints.md tasks 4 and 6 and
//! `docs/reify-stdlib-reference.md` §13.3.
//!
//! Locks in that the Snapshot Map shape produced by the stdlib survives the
//! parse → compile → eval round-trip — no compile-time pruning that would
//! silently drop the call, no eval-pipeline glue that would mangle the FK
//! transform / accessor outputs.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_test_support::{
    collect_errors, decompose_point3, make_simple_engine, parse_and_compile_with_stdlib, read_f64,
};
use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

/// Source: the analytic two-link chain from the PRD task 4 acceptance test.
///
/// Body B's expected world translation:
///   `T_b_world = R_z(π/4) ∘ T_x(2m)`
///   translation = `R_z(π/4) ⋅ (2, 0, 0) = (2·cos(π/4), 2·sin(π/4), 0) = (√2, √2, 0)`
///
/// `bbox` is over body world-frame origins (point-mass approximation per
/// the v0.1 spec): body A at (0, 0, 0), body B at (√2, √2, 0).
///   `bbox.min = (0, 0, 0)`
///   `bbox.max = (√2, √2, 0)`
///
/// `com` is the uniform-density mean of the two body origins:
///   `com = ((0 + √2)/2, (0 + √2)/2, 0) = (√2/2, √2/2, 0)`
const HAPPY_SOURCE: &str = r#"
structure def Kinematic {
    let j_rev  = revolute(vec3(0, 0, 1), 0rad .. 3.141592653589793rad)
    let j_pris = prismatic(vec3(1, 0, 0), 0mm .. 2000mm)

    let m0 = mechanism()
    let m1 = body(m0, "a", j_rev)
    let m2 = body(m1, "b", j_pris, j_rev)

    let bind_rev  = bind(j_rev, 0.7853981633974483rad)
    let bind_pris = bind(j_pris, 2000mm)
    let s = snapshot(m2, [bind_rev, bind_pris])

    let id_b = body_id_of(m2, "b")
    let t_b  = transform_of(s, id_b)
    let bbox = bounding_box(s)
    let com  = center_of_mass(s)
}
"#;

/// Decompose a `Value::Transform` into (rotation_quaternion, translation_si).
fn decompose_transform(v: &Value, label: &str) -> ((f64, f64, f64, f64), [f64; 3]) {
    let (rotation, translation) = match v {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("{label}: expected Value::Transform, got {other:?}"),
    };
    let (rw, rx, ry, rz) = match rotation {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        other => panic!("{label}: expected Value::Orientation, got {other:?}"),
    };
    let comps = match translation {
        Value::Vector(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Vector len=3, got {other:?}"),
    };
    (
        (rw, rx, ry, rz),
        [
            read_f64(&comps[0], &format!("{label}.t[0]")),
            read_f64(&comps[1], &format!("{label}.t[1]")),
            read_f64(&comps[2], &format!("{label}.t[2]")),
        ],
    )
}

/// Smoke test: parse, compile, eval the analytic two-link chain source and
/// assert the FK pipeline produces the expected world transform, bounding
/// box, and center of mass.
#[test]
fn forward_kinematics_two_link_chain_e2e() {
    let compiled = parse_and_compile_with_stdlib(HAPPY_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;

    // s must be a Snapshot Map with kind="snapshot" and 2 bodies.
    let s = get_value(v, "s");
    let smap = match s {
        Value::Map(m) => m,
        other => panic!("s should be a Map, got {other:?}"),
    };
    assert_eq!(
        smap.get(&Value::String("kind".to_string())),
        Some(&Value::String("snapshot".to_string())),
        "s.kind should be 'snapshot'"
    );
    let bodies = match smap.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        other => panic!("s.bodies should be a List, got {other:?}"),
    };
    assert_eq!(bodies.len(), 2, "s.bodies should have exactly 2 records");

    // id_b must be Int(1) — body B is the second body.
    let id_b = get_value(v, "id_b");
    assert_eq!(
        id_b,
        &Value::Int(1),
        "body_id_of(m2, \"b\") should be Int(1)"
    );

    // t_b: world transform of body B.  Translation = (√2, √2, 0).
    let t_b = get_value(v, "t_b");
    let ((rw, rx, ry, rz), [tx, ty, tz]) = decompose_transform(t_b, "t_b");
    let sqrt2 = std::f64::consts::SQRT_2;
    assert!(
        (tx - sqrt2).abs() < 1e-6,
        "t_b.tx should be √2 ≈ {sqrt2}, got {tx}"
    );
    assert!(
        (ty - sqrt2).abs() < 1e-6,
        "t_b.ty should be √2 ≈ {sqrt2}, got {ty}"
    );
    assert!(tz.abs() < 1e-6, "t_b.tz should be 0, got {tz}");

    // Rotation: quaternion(R_z(π/4)) = (cos(π/8), 0, 0, sin(π/8)) up to sign.
    let half = std::f64::consts::FRAC_PI_8;
    let qw = half.cos();
    let qz = half.sin();
    let matches_pos =
        (rw - qw).abs() < 1e-6 && rx.abs() < 1e-6 && ry.abs() < 1e-6 && (rz - qz).abs() < 1e-6;
    let matches_neg =
        (rw + qw).abs() < 1e-6 && rx.abs() < 1e-6 && ry.abs() < 1e-6 && (rz + qz).abs() < 1e-6;
    assert!(
        matches_pos || matches_neg,
        "t_b rotation should be quaternion(R_z(π/4)) ≈ ({qw}, 0, 0, {qz}) up to sign, \
         got ({rw}, {rx}, {ry}, {rz})"
    );

    // bbox: Map { min, max } of body world-frame origins.
    // Body A at world origin (0, 0, 0) (j_rev's frame is rotation-only).
    // Body B at (√2, √2, 0).
    let bbox = get_value(v, "bbox");
    let bbox_map = match bbox {
        Value::Map(m) => m,
        other => panic!("bbox should be a Map, got {other:?}"),
    };
    let min_v = bbox_map
        .get(&Value::String("min".to_string()))
        .expect("bbox should have a `min` field");
    let max_v = bbox_map
        .get(&Value::String("max".to_string()))
        .expect("bbox should have a `max` field");
    let [minx, miny, minz] = decompose_point3(min_v, "bbox.min");
    let [maxx, maxy, maxz] = decompose_point3(max_v, "bbox.max");
    assert!(minx.abs() < 1e-6, "bbox.min.x should be 0, got {minx}");
    assert!(miny.abs() < 1e-6, "bbox.min.y should be 0, got {miny}");
    assert!(minz.abs() < 1e-6, "bbox.min.z should be 0, got {minz}");
    assert!(
        (maxx - sqrt2).abs() < 1e-6,
        "bbox.max.x should be √2, got {maxx}"
    );
    assert!(
        (maxy - sqrt2).abs() < 1e-6,
        "bbox.max.y should be √2, got {maxy}"
    );
    assert!(maxz.abs() < 1e-6, "bbox.max.z should be 0, got {maxz}");

    // com: uniform-density mean of body world-frame origins.
    // = ((0 + √2)/2, (0 + √2)/2, 0) = (√2/2, √2/2, 0).
    let com = get_value(v, "com");
    let [cx, cy, cz] = decompose_point3(com, "com");
    let half_sqrt2 = sqrt2 / 2.0;
    assert!(
        (cx - half_sqrt2).abs() < 1e-6,
        "com.x should be √2/2 ≈ {half_sqrt2}, got {cx}"
    );
    assert!(
        (cy - half_sqrt2).abs() < 1e-6,
        "com.y should be √2/2 ≈ {half_sqrt2}, got {cy}"
    );
    assert!(cz.abs() < 1e-6, "com.z should be 0, got {cz}");
}

/// Source: an errored mechanism produced by duplicate solid — the same solid
/// string `"a"` is attached twice with different joints, which makes
/// `mechanism::body()` short-circuit the second-body call to a Map carrying
/// `error="duplicate_solid"`.  Any downstream `snapshot()` /
/// `transform_of(...)` must surface that as Undef.
///
/// Migration note: this fixture previously triggered `error="closed_chain"`
/// via a parent-conflict pattern (j_x reused as `at` with two distinct
/// parents), but under v0.2 closed kinematic chains are recorded as
/// loop-closure constraints rather than errored, so the parent-conflict
/// trigger no longer surfaces an `error` key.  The duplicate-solid trigger
/// (canonical recipe per crates/reify-stdlib/src/snapshot.rs:1217-1238)
/// preserves the test contract: an `error` key on m2 forces snapshot() to
/// short-circuit to Undef through the full parse → compile → eval pipeline.
const ERRORED_SOURCE: &str = r#"
structure def KinematicErrored {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)

    let m0 = mechanism()
    let m1 = body(m0, "a", j_a)
    // Re-attaching solid "a" with a different joint (j_b) is a duplicate
    // solid → m2 carries `error="duplicate_solid"`.
    let m2 = body(m1, "a", j_b)

    let s = snapshot(m2, [])
}
"#;

/// Errored-mechanism propagation: when source code constructs a closed-chain
/// mechanism, `snapshot(m, ...)` must yield Undef through the full eval
/// pipeline.  Mirrors `snapshot_on_errored_mechanism_returns_undef` (unit
/// test) but asserts the same invariant survives parse → compile → eval.
#[test]
fn forward_kinematics_errored_mechanism_propagates_undef_e2e() {
    let compiled = parse_and_compile_with_stdlib(ERRORED_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Eval should not raise an Error-severity diagnostic just because a
    // builtin returned Undef — Undef is a first-class value, not an error.
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    // s must be Undef — the errored mechanism short-circuits `snapshot()`.
    let s_id = ValueCellId::new("KinematicErrored", "s");
    let s = result
        .values
        .get(&s_id)
        .unwrap_or_else(|| panic!("KinematicErrored.s not found in eval result"));
    assert!(
        matches!(s, Value::Undef),
        "snapshot() on an errored mechanism must propagate Undef through \
         the eval pipeline, got {s:?}"
    );
}
