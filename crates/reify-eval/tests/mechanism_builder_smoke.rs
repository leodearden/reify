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

use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId, ValueMap};

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
