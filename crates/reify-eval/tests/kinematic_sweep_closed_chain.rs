//! End-to-end test for v0.2 closed-chain sweep API integration (task 2678).
//!
//! Drives the sweep API over a closed-chain mechanism through the full
//! `parse → compile_with_stdlib → eval` pipeline, exercising the warm-start
//! threading wired across `build_snapshot_list` (step-10) and the closed-
//! chain loop-closure solver invocation in `snapshot()` (step-4).
//!
//! Mirrors `sweep_api_smoke.rs` (e2e sweep template) and
//! `mechanism_builder_smoke.rs` / `kinematic_loop_closure_machinery.rs`
//! (closed-chain mechanism source pattern).
//!
//! Closure analysis for the 2-prismatic-X fixture:
//!   path_a = [world, jB]                   — spanning-tree (jB → world)
//!   path_b = [world, jA, jB]               — closing edge (jB → jA)
//!   chain_a translation = chain_b translation
//!   midpoint(jB) = jA_driver + jB_free_in_chain_b
//!   1.0          = driver + solved_jB        (jB range [0, 2]m → midpoint 1.0)
//!   ⇒ solved_jB = 1.0 − driver
//!
//! For driver ∈ [0, 1]m over 11 evenly-spaced steps, solved_jB ∈ [1.0, 0.0]m
//! — strictly monotonic decreasing, pinning warm-start continuity (a cold-
//! solver path would risk hitting a secondary minimum on later steps).
//!
//! Also verifies the open-chain regression in `sweep_api_smoke.rs`: an
//! open-chain mechanism still produces N snapshots, each with empty
//! `free_values`, so the warm-start arg threading is a no-op there.
//!
//! See docs/prds/v0_2/kinematic-constraints.md task 10.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId, ValueMap};

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

/// Source: 2-prismatic-X closed-chain mechanism driven by sweep over
/// `j_a` (the spanning-tree-side driver).  The closing edge re-records
/// `j_b`'s parent from world (m2) to `j_a` (m3), producing exactly one
/// `loop_closures` entry the snapshot evaluator solves per step.
///
/// `j_b`'s range is intentionally wider (0..2m) than `j_a`'s (0..1m) so
/// the loop-closure solution `solved_jB = 1.0 − driver` lies inside
/// `j_b`'s range for every driver value the sweep produces.
const CLOSED_CHAIN_SOURCE: &str = r#"
structure def Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(1, 0, 0), 0mm .. 2000mm)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_a)
    let m2  = body(m1, "solid_b", j_b)
    let m3  = body(m2, "solid_c", j_b, j_a)

    let snaps = sweep(m3, j_a, 0mm .. 1000mm, 11)
}
"#;

/// Source: 1-body open-chain mechanism — `sweep_api_smoke.rs`'s
/// HAPPY_SOURCE pattern, repeated here so the open-chain regression
/// lives alongside the closed-chain assertions.  Empty `free_values`
/// per step is the contract that lets `build_snapshot_list` thread an
/// empty outer-List warm-start arg into a `loop_closures`-empty
/// snapshot as a no-op fast path.
const OPEN_CHAIN_SOURCE: &str = r#"
structure def Kinematic {
    let j = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let m = body(mechanism(), "a", j)

    let snaps = sweep(m, j, 0mm .. 1000mm, 11)
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

/// Extract a snapshot's body-N world translation as `[x, y, z]` in SI
/// units.  Mirrors `body_0_translation` in `sweep_api_smoke.rs`,
/// generalised to an arbitrary body index.
fn body_n_translation(snapshot: &Value, n: usize, label: &str) -> [f64; 3] {
    let smap = match snapshot {
        Value::Map(m) => m,
        other => panic!("{label}: expected Snapshot Map, got {other:?}"),
    };
    let bodies = match smap.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        other => panic!("{label}: expected snapshot bodies List, got {other:?}"),
    };
    let body = match &bodies[n] {
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

/// Closed-chain e2e: parse → compile → eval the closed-chain sweep
/// source, then assert per-snapshot solver convergence, body-transform
/// consistency, `free_values` carrier shape, and warm-start continuity
/// (monotonic free-var trajectory across the 11 steps).
#[test]
fn sweep_closed_chain_warm_start_e2e() {
    let compiled = parse_and_compile_with_stdlib(CLOSED_CHAIN_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;
    let snaps = get_value(v, "snaps");
    let snaps_list = match snaps {
        Value::List(l) => l,
        other => panic!("snaps should be a List, got {other:?}"),
    };
    assert_eq!(
        snaps_list.len(),
        11,
        "closed-chain sweep should produce exactly 11 snapshots"
    );

    // Per-snapshot assertions:
    //   (a) Snapshot kind sanity ("snapshot").
    //   (b) `free_values` carrier shape: outer-length 1 (one loop_closures
    //       record), inner-length 1 (one free var jB), leaf is Value::Real.
    //   (c) Closure residual: solved jB matches `1.0 − driver` within 1e-6m
    //       (this is the loop-closure path-equality assertion — chain_a
    //       translation 1.0m must equal chain_b translation `driver+x`).
    //   (d) Body world_transforms reflect the loop-closure-solved bindings:
    //       body 0 at j_a → driver, body 1 at j_b → solved, body 2 (closing
    //       edge, walked via joint_parents which keeps jB → world) → solved.
    //   (e) Monotonic-decreasing trajectory of the free var across steps,
    //       captured as `solved[i] ≤ solved[i-1]`.  Without warm-start
    //       continuity a cold solve at step k could converge to an
    //       alternate root and break the monotonic invariant.
    let mut prev_solved: Option<f64> = None;
    for (i, snap) in snaps_list.iter().enumerate() {
        let smap = match snap {
            Value::Map(m) => m,
            other => panic!("snaps[{i}] should be a Map, got {other:?}"),
        };

        // (a) Snapshot kind.
        assert_eq!(
            smap.get(&Value::String("kind".to_string())),
            Some(&Value::String("snapshot".to_string())),
            "snaps[{i}].kind should be 'snapshot'"
        );

        // (b) free_values carrier shape.
        let fv = smap
            .get(&Value::String("free_values".to_string()))
            .unwrap_or_else(|| panic!("snaps[{i}] must carry a free_values key"));
        let outer = match fv {
            Value::List(l) => l,
            other => panic!("snaps[{i}] free_values must be a List, got {other:?}"),
        };
        assert_eq!(
            outer.len(),
            1,
            "snaps[{i}] outer free_values length must equal loop_closures.len() == 1"
        );
        let inner = match &outer[0] {
            Value::List(l) => l,
            other => panic!("snaps[{i}] free_values[0] must be a List, got {other:?}"),
        };
        assert_eq!(
            inner.len(),
            1,
            "snaps[{i}] inner free_values length must equal free-var count == 1"
        );
        let solved = match &inner[0] {
            Value::Real(r) => *r,
            other => panic!("snaps[{i}] free_values leaf must be Real, got {other:?}"),
        };

        // (c) Closure residual: chain_a == chain_b within 1e-6 m.  The
        // 11 driver values are evenly spaced over [0, 1]m:
        //   driver = i / 10  for i ∈ 0..=10
        // and the closure prediction is solved_jB = 1.0 − driver.
        let driver = (i as f64) / 10.0;
        let expected = 1.0 - driver;
        assert!(
            (solved - expected).abs() < 1e-6,
            "snaps[{i}] closure residual: solved jB = {solved} must match prediction {expected} \
             (driver = {driver}); residual = {}",
            (solved - expected).abs()
        );

        // (d) Body world_transforms.  Body 0 (at j_a, parent world) carries
        // the swept driver value; body 1 (at j_b, parent world) carries the
        // solver-driven solved_jB; body 2 (closing edge, recorded with
        // parent j_a but walked via joint_parents which kept j_b → world
        // from m2's earlier registration) also carries solved_jB —
        // confirming the FK re-walk consumed the synthesized binding for
        // the free joint.
        let [tx0, ty0, tz0] = body_n_translation(snap, 0, &format!("snaps[{i}].body[0]"));
        let [tx1, ty1, tz1] = body_n_translation(snap, 1, &format!("snaps[{i}].body[1]"));
        let [tx2, ty2, tz2] = body_n_translation(snap, 2, &format!("snaps[{i}].body[2]"));
        assert!(
            (tx0 - driver).abs() < 1e-6,
            "snaps[{i}] body 0 (at j_a) tx must be driver = {driver}, got {tx0}"
        );
        assert!(ty0.abs() < 1e-6, "snaps[{i}] body 0 ty must be 0, got {ty0}");
        assert!(tz0.abs() < 1e-6, "snaps[{i}] body 0 tz must be 0, got {tz0}");
        assert!(
            (tx1 - solved).abs() < 1e-6,
            "snaps[{i}] body 1 (at j_b) tx must be solved jB = {solved}, got {tx1}"
        );
        assert!(ty1.abs() < 1e-6, "snaps[{i}] body 1 ty must be 0, got {ty1}");
        assert!(tz1.abs() < 1e-6, "snaps[{i}] body 1 tz must be 0, got {tz1}");
        assert!(
            (tx2 - solved).abs() < 1e-6,
            "snaps[{i}] body 2 (closing edge) tx must be solved jB = {solved}, got {tx2}"
        );
        assert!(ty2.abs() < 1e-6, "snaps[{i}] body 2 ty must be 0, got {ty2}");
        assert!(tz2.abs() < 1e-6, "snaps[{i}] body 2 tz must be 0, got {tz2}");

        // (e) Monotonic-decreasing trajectory across steps.  Strict-
        // decreasing with a 1µm slack absorbs solver wobble; warm-start
        // is what keeps consecutive steps on the same continuous branch.
        if let Some(p) = prev_solved {
            assert!(
                solved < p + 1e-6,
                "snaps[{i}] monotonicity: solved jB = {solved} must be ≤ previous {p} \
                 (warm-start continuity)"
            );
        }
        prev_solved = Some(solved);
    }
}

/// Open-chain regression: confirms `sweep_api_smoke.rs`'s open-chain
/// pattern still works with the warm-start threading in place.  Each
/// snapshot must carry `free_values == Value::List(vec![])` because the
/// mechanism has no `loop_closures` records — threading an empty outer-
/// List into a closed-chain-less snapshot is a no-op fast path.
#[test]
fn sweep_open_chain_emits_empty_free_values_e2e() {
    let compiled = parse_and_compile_with_stdlib(OPEN_CHAIN_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;
    let snaps = get_value(v, "snaps");
    let snaps_list = match snaps {
        Value::List(l) => l,
        other => panic!("snaps should be a List, got {other:?}"),
    };
    assert_eq!(
        snaps_list.len(),
        11,
        "open-chain sweep should still produce 11 snapshots"
    );

    for (i, snap) in snaps_list.iter().enumerate() {
        let smap = match snap {
            Value::Map(m) => m,
            other => panic!("snaps[{i}] should be a Map, got {other:?}"),
        };
        let fv = smap
            .get(&Value::String("free_values".to_string()))
            .unwrap_or_else(|| panic!("snaps[{i}] must carry a free_values key"));
        assert_eq!(
            fv,
            &Value::List(vec![]),
            "snaps[{i}] open-chain free_values must be empty, got {fv:?}"
        );
    }
}
