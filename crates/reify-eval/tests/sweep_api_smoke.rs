//! End-to-end smoke test for the v0.1 batch-sweep stdlib (task 2529).
//!
//! Drives the new `dim()` / `sweep()` / `sweep_grid()` builtins through the
//! full `parse → compile_with_stdlib → eval` pipeline.  Mirrors the structure
//! of `forward_kinematics_e2e.rs` (snapshot pipeline) and
//! `mechanism_builder_smoke.rs` (mechanism builder).
//!
//! See docs/prds/kinematic-constraints.md task 5 and
//! `docs/reify-stdlib-reference.md` §13.4.
//!
//! Locks in that the SweepDim / List<Snapshot> shapes produced by the stdlib
//! survive the parse → compile → eval round-trip — no compile-time pruning
//! that would silently drop the call, no eval-pipeline glue that would mangle
//! the cross-product iteration.

use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId, ValueMap};

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

/// Source: 1-body prismatic +X mechanism (range 0..1m).  Drives one 1-D
/// sweep of 11 evenly-spaced snapshots and one 2-step `sweep_grid`.
///
/// Expected:
/// - `snaps`: List of 11 Snapshot Maps.  Body 0 world translation is
///   `(i / 10, 0, 0)` metres for i ∈ 0..=10.
/// - `grid`:  List of 2 Snapshot Maps (cross-product of a single 2-step
///   dim).  Body 0 world translation is `(0, 0, 0)` then `(1, 0, 0)`.
const HAPPY_SOURCE: &str = r#"
structure def Kinematic {
    let j = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let m = body(mechanism(), "a", j)

    let snaps = sweep(m, j, 0mm .. 1000mm, 11)
    let grid  = sweep_grid(m, [dim(j, 0mm .. 1000mm, 2)])
}
"#;

/// Read a numeric component (Real or Scalar) as f64 SI value.
fn read_f64(v: &Value, label: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        Value::Int(i) => *i as f64,
        other => panic!("{label}: expected numeric component, got {other:?}"),
    }
}

/// Extract a snapshot's body 0 world translation.
fn body_0_translation(snapshot: &Value, label: &str) -> [f64; 3] {
    let smap = match snapshot {
        Value::Map(m) => m,
        other => panic!("{label}: expected Snapshot Map, got {other:?}"),
    };
    let bodies = match smap.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        other => panic!("{label}: expected snapshot bodies List, got {other:?}"),
    };
    let body = match &bodies[0] {
        Value::Map(b) => b,
        other => panic!("{label}: expected snapshot body record Map, got {other:?}"),
    };
    let wt = body
        .get(&Value::String("world_transform".to_string()))
        .unwrap_or_else(|| panic!("{label}: body record must carry world_transform"));
    let trans = match wt {
        Value::Transform { translation, .. } => translation.as_ref(),
        other => panic!("{label}: expected Value::Transform, got {other:?}"),
    };
    let comps = match trans {
        Value::Vector(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Vector len=3, got {other:?}"),
    };
    [
        read_f64(&comps[0], &format!("{label}.t[0]")),
        read_f64(&comps[1], &format!("{label}.t[1]")),
        read_f64(&comps[2], &format!("{label}.t[2]")),
    ]
}

/// Smoke test: parse, compile, eval the sweep API source and assert the
/// pipeline produces the expected per-snapshot world translations and
/// list lengths.
#[test]
fn sweep_api_round_trip_e2e() {
    let compiled = parse_and_compile_with_stdlib(HAPPY_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;

    // ── 1-D sweep: 11 evenly-spaced snapshots ─────────────────────────────
    let snaps = get_value(v, "snaps");
    let snaps_list = match snaps {
        Value::List(l) => l,
        other => panic!("snaps should be a List, got {other:?}"),
    };
    assert_eq!(
        snaps_list.len(),
        11,
        "snaps should be a List of 11 snapshots"
    );

    // Each entry is a Snapshot Map with body 0 at (i/10, 0, 0).
    for (i, snap) in snaps_list.iter().enumerate() {
        let smap = match snap {
            Value::Map(m) => m,
            other => panic!("snaps[{i}] should be a Map, got {other:?}"),
        };
        assert_eq!(
            smap.get(&Value::String("kind".to_string())),
            Some(&Value::String("snapshot".to_string())),
            "snaps[{i}].kind should be 'snapshot'"
        );
    }

    let [tx0, ty0, tz0] = body_0_translation(&snaps_list[0], "snaps[0]");
    assert!(
        tx0.abs() < 1e-9,
        "snaps[0] body 0 tx should be 0, got {tx0}"
    );
    assert!(ty0.abs() < 1e-9, "snaps[0] body 0 ty should be 0, got {ty0}");
    assert!(tz0.abs() < 1e-9, "snaps[0] body 0 tz should be 0, got {tz0}");

    let [tx10, ty10, tz10] = body_0_translation(&snaps_list[10], "snaps[10]");
    assert!(
        (tx10 - 1.0).abs() < 1e-9,
        "snaps[10] body 0 tx should be 1.0 (range upper), got {tx10}"
    );
    assert!(
        ty10.abs() < 1e-9,
        "snaps[10] body 0 ty should be 0, got {ty10}"
    );
    assert!(
        tz10.abs() < 1e-9,
        "snaps[10] body 0 tz should be 0, got {tz10}"
    );

    // ── 2-step sweep_grid: 2 Snapshot Maps over a single 2-step dim ──────
    let grid = get_value(v, "grid");
    let grid_list = match grid {
        Value::List(l) => l,
        other => panic!("grid should be a List, got {other:?}"),
    };
    assert_eq!(
        grid_list.len(),
        2,
        "sweep_grid with [dim(j, 0..1m, 2)] should produce 2 snapshots"
    );
    for (i, snap) in grid_list.iter().enumerate() {
        match snap {
            Value::Map(m) => assert_eq!(
                m.get(&Value::String("kind".to_string())),
                Some(&Value::String("snapshot".to_string())),
                "grid[{i}].kind should be 'snapshot'"
            ),
            other => panic!("grid[{i}] should be a Map, got {other:?}"),
        }
    }
    let [gx0, _, _] = body_0_translation(&grid_list[0], "grid[0]");
    let [gx1, _, _] = body_0_translation(&grid_list[1], "grid[1]");
    assert!(gx0.abs() < 1e-9, "grid[0] body 0 tx should be 0, got {gx0}");
    assert!(
        (gx1 - 1.0).abs() < 1e-9,
        "grid[1] body 0 tx should be 1.0, got {gx1}"
    );
}
