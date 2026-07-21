//! End-to-end smoke test for the v0.1 mechanism builder stdlib (task 2528).
//!
//! Drives the new `mechanism()` / `body()` / `body_id_of()` / `world()`
//! builtins through the full `parse → compile_with_stdlib → eval` pipeline.
//! Mirrors the structure of `kinematic_stdlib_smoke.rs` (joint builtins) and
//! `kinematic_loop_closure_machinery.rs` (loop-closure machinery).
//!
//! See docs/prds/kinematic-constraints.md task 3 and
//! `docs/reify-stdlib-reference.md` §13.2.
//!
//! Locks in that the mechanism Map shape produced by the stdlib survives the
//! parse → compile → eval round-trip — no compile-time pruning that would
//! silently drop the call, no eval-pipeline glue that would mangle the Map
//! structure. The structured-error fields tested here are what a future
//! snapshot/eval-pipeline integration (deferred per design-decisions in the
//! task plan) will read to synthesise a `Diagnostic` with
//! `DiagnosticCode::KinematicClosedChain`.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

/// Source: a `Kinematic` structure that builds a 2-body open-chain
/// mechanism using the new `mechanism()` / `body()` / `body_id_of()`
/// stdlib builtins.
///
/// Bindings:
///   `j_a`   = `prismatic(vec3(1,0,0), 0mm..1000mm)`
///   `j_b`   = `revolute(vec3(0,0,1), 0rad..3.14rad)`
///   `m0`    = `mechanism()` — empty Mechanism Map
///   `m1`    = `body(m0, "solid_a", j_a)` — append body 0 at j_a (parent=world)
///   `m2`    = `body(m1, "solid_b", j_b, j_a)` — append body 1 at j_b (parent=j_a)
///   `id_a`  = `body_id_of(m2, "solid_a")` — must be Int(0)
const HAPPY_SOURCE: &str = r#"
structure def Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = revolute(vec3(0, 0, 1), 0rad .. 3.14rad)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_a)
    let m2  = body(m1, "solid_b", j_b, j_a)
    let id_a = body_id_of(m2, "solid_a")
}
"#;

/// Smoke test: parse, compile, eval the happy-path mechanism builder source
/// and assert the resulting Mechanism Map has the expected shape (no error
/// fields, two bodies, body_id_of returns Int(0) for the first solid).
#[test]
fn mechanism_builder_happy_path_e2e() {
    let compiled = parse_and_compile_with_stdlib(HAPPY_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;

    // m2 must be a Mechanism Map with no error fields and bodies length 2.
    let m2 = get_value(v, "m2");
    let map = match m2 {
        Value::Map(m) => m,
        other => panic!("m2 should be a Map, got {other:?}"),
    };

    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("mechanism".to_string())),
        "m2.kind should be 'mechanism'"
    );
    assert!(
        !map.contains_key(&Value::String("error".to_string())),
        "m2 should have no 'error' key (open chain), got error={:?}",
        map.get(&Value::String("error".to_string()))
    );

    let bodies = match map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        other => panic!("m2.bodies should be a List, got {other:?}"),
    };
    assert_eq!(bodies.len(), 2, "m2.bodies should have exactly 2 records");

    // id_a must be Int(0) — body_id_of looked up the first solid.
    let id_a = get_value(v, "id_a");
    assert_eq!(
        id_a,
        &Value::Int(0),
        "body_id_of(m2, \"solid_a\") should be Int(0)"
    );
}

/// Source: a `Kinematic` structure that constructs a closed chain via the
/// parent-conflict path. Joint `j_x` is recorded with parent `j_a` in the
/// first `body()` call and then re-recorded with a different parent `j_b`
/// in the second call — v0.2 behaviour: this is a valid closed chain;
/// the Mechanism Map carries a `loop_closures` entry instead of an error.
///
/// Bindings:
///   `j_a` = `prismatic(vec3(1,0,0), 0mm..1000mm)` — X-axis translation
///   `j_b` = `prismatic(vec3(0,1,0), 0mm..1000mm)` — Y-axis translation
///   `j_x` = `revolute(vec3(0,0,1), 0rad..3.14rad)` — Z-axis rotation
///   `m0`  = `mechanism()`
///   `m1`  = `body(m0, "solid_a", j_x, j_a)` — records j_x → j_a
///   `m2`  = `body(m1, "solid_b", j_x, j_b)` — closing edge: j_x already → j_a
const CLOSED_CHAIN_SOURCE: &str = r#"
structure def Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)
    let j_x = revolute(vec3(0, 0, 1), 0rad .. 3.14rad)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_x, j_a)
    let m2  = body(m1, "solid_b", j_x, j_b)
}
"#;

/// Closed-chain e2e (v0.2): parse, compile, eval a source that triggers the
/// parent-conflict closed-chain path and assert the Mechanism Map carries
/// a `loop_closures` entry instead of an `error` field.
///
/// Locks in that loop-closure recording survives the full parse → compile →
/// eval round-trip — no compile-time pruning, no eval-pipeline glue that
/// could mangle the `loop_closures` Map structure. Assertions mirror the
/// unit-test contract in `mechanism.rs::tests::parent_conflict_records_
/// loop_closure_constraint`.
///
/// No `Diagnostic` is expected on the eval pipeline — closed chains are now
/// valid v0.2 mechanisms. Eval errors are still asserted absent.
#[test]
fn mechanism_builder_closed_chain_records_loop_closure_e2e() {
    let compiled = parse_and_compile_with_stdlib(CLOSED_CHAIN_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics; got: {eval_errors:?}"
    );

    let v = &result.values;
    let m2 = get_value(v, "m2");
    let map = match m2 {
        Value::Map(m) => m,
        other => panic!("m2 should be a Map, got {other:?}"),
    };

    // v0.2: closed chains are valid — no error key.
    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("mechanism".to_string())),
        "m2.kind should be 'mechanism'"
    );
    assert!(
        !map.contains_key(&Value::String("error".to_string())),
        "m2 must NOT have an 'error' key in v0.2 (closed chains are valid); \
         got error={:?}",
        map.get(&Value::String("error".to_string()))
    );

    // Both bodies are present (closing body IS appended).
    let bodies = match map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        other => panic!("m2.bodies should be a List, got {other:?}"),
    };
    assert_eq!(bodies.len(), 2, "m2.bodies should have exactly 2 records");

    // loop_closures has exactly one entry.
    let loop_closures = match map.get(&Value::String("loop_closures".to_string())) {
        Some(Value::List(lc)) => lc,
        other => panic!("m2.loop_closures should be a List, got {other:?}"),
    };
    assert_eq!(
        loop_closures.len(),
        1,
        "m2.loop_closures should have exactly one entry"
    );

    let lc = match &loop_closures[0] {
        Value::Map(m) => m,
        other => panic!("loop_closures[0] should be a Map, got {other:?}"),
    };
    assert_eq!(
        lc.get(&Value::String("kind".to_string())),
        Some(&Value::String("loop_closure".to_string())),
        "loop_closure entry kind should be 'loop_closure'"
    );

    // Pin path_a = [world, j_a, j_x] and path_b = [world, j_b, j_x] using
    // the actual joint Values resolved from the eval result. Mirrors the
    // unit-test path-pinning idiom in mechanism.rs::tests.
    let j_a = get_value(v, "j_a");
    let j_b = get_value(v, "j_b");
    let j_x = get_value(v, "j_x");
    let world = Value::Map({
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("world".to_string()),
        );
        m
    });

    let path_a = match lc.get(&Value::String("path_a".to_string())) {
        Some(Value::List(p)) => p,
        other => panic!("loop_closure path_a should be a List, got {other:?}"),
    };
    let path_b = match lc.get(&Value::String("path_b".to_string())) {
        Some(Value::List(p)) => p,
        other => panic!("loop_closure path_b should be a List, got {other:?}"),
    };
    assert_eq!(
        path_a,
        &vec![world.clone(), j_a.clone(), j_x.clone()],
        "loop_closure path_a should be [world, j_a, j_x]"
    );
    assert_eq!(
        path_b,
        &vec![world, j_b.clone(), j_x.clone()],
        "loop_closure path_b should be [world, j_b, j_x]"
    );
}
