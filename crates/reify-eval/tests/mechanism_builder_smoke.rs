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

/// Source: a `Kinematic` structure that constructs a closed chain via the
/// parent-conflict path. Joint `j_x` is recorded with parent `j_a` in the
/// first `body()` call and then re-recorded with a different parent `j_b`
/// in the second call — the build-time DAG validation must surface this
/// as `error="closed_chain"` with both ancestor paths captured on the
/// resulting Mechanism Map.
///
/// Bindings:
///   `j_a` = `prismatic(vec3(1,0,0), 0mm..1000mm)` — X-axis translation
///   `j_b` = `prismatic(vec3(0,1,0), 0mm..1000mm)` — Y-axis translation
///   `j_x` = `revolute(vec3(0,0,1), 0rad..3.14rad)` — Z-axis rotation
///   `m0`  = `mechanism()`
///   `m1`  = `body(m0, "solid_a", j_x, j_a)` — records j_x → j_a
///   `m2`  = `body(m1, "solid_b", j_x, j_b)` — conflicts: j_x already → j_a
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

/// Closed-chain e2e: parse, compile, eval a source that triggers the
/// parent-conflict closed-chain path and assert the Mechanism Map carries
/// `error="closed_chain"` with non-empty `error_path1` and `error_path2`.
/// Locks in that closed-chain detection survives the full parse → compile →
/// eval round-trip — no compile-time pruning, no eval-pipeline glue that
/// could mangle the structured-error fields.
///
/// No `Diagnostic` is expected on the eval pipeline yet — that emission
/// step is deferred to a future snapshot/eval-pipeline integration (see
/// the design-decision note in the task plan and the
/// `DiagnosticCode::KinematicClosedChain` reservation in
/// `reify-types/src/diagnostics.rs`). Eval errors are still asserted
/// absent so a regression that began emitting an unrelated Error
/// diagnostic would surface here.
#[test]
fn mechanism_builder_closed_chain_e2e() {
    let compiled = parse_and_compile_with_stdlib(CLOSED_CHAIN_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics yet \
         (Diagnostic emission for closed_chain is deferred); got: {eval_errors:?}"
    );

    let v = &result.values;
    let m2 = get_value(v, "m2");
    let map = match m2 {
        Value::Map(m) => m,
        other => panic!("m2 should be a Map, got {other:?}"),
    };

    assert_eq!(
        map.get(&Value::String("kind".to_string())),
        Some(&Value::String("mechanism".to_string())),
        "m2.kind should still be 'mechanism' on errored Mechanism"
    );
    assert_eq!(
        map.get(&Value::String("error".to_string())),
        Some(&Value::String("closed_chain".to_string())),
        "m2.error should be 'closed_chain' for the parent-conflict scenario"
    );

    let path1 = match map.get(&Value::String("error_path1".to_string())) {
        Some(Value::List(p)) => p,
        other => panic!("m2.error_path1 should be a List, got {other:?}"),
    };
    let path2 = match map.get(&Value::String("error_path2".to_string())) {
        Some(Value::List(p)) => p,
        other => panic!("m2.error_path2 should be a List, got {other:?}"),
    };
    assert!(
        !path1.is_empty(),
        "m2.error_path1 should be non-empty (walks world → ... → j_a → j_x)"
    );
    assert!(
        !path2.is_empty(),
        "m2.error_path2 should be non-empty (walks world → ... → j_b → j_x)"
    );

    // Pin path1.last() / path2.last() to the actual j_x Value produced
    // by the eval pipeline. Mirrors the unit-test assertion in
    // `mechanism.rs::tests::closed_chain_via_parent_conflict_emits_
    // error_with_both_paths`, which pins the full path content. At
    // the e2e boundary we additionally guard the Value-equality
    // round-trip so a future eval-pipeline glue change that wrapped
    // path elements differently (e.g. boxing the joint Map, attaching
    // a span, swapping `kind` strings) would surface here.
    let j_x = get_value(v, "j_x");
    assert_eq!(
        path1.last(),
        Some(j_x),
        "m2.error_path1 should terminate at j_x (the conflicting joint)"
    );
    assert_eq!(
        path2.last(),
        Some(j_x),
        "m2.error_path2 should terminate at j_x (the conflicting joint)"
    );

    // The non-terminal entries of each path should be the parent
    // joints in canonical top-down order: path1 = [world, j_a, j_x],
    // path2 = [world, j_b, j_x]. Pin the parent joint in each path so
    // a regression that swapped j_a/j_b or dropped the world-prepend
    // would surface here as well.
    let j_a = get_value(v, "j_a");
    let j_b = get_value(v, "j_b");
    let world = Value::Map({
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("world".to_string()),
        );
        m
    });
    assert_eq!(
        path1,
        &vec![world.clone(), j_a.clone(), j_x.clone()],
        "m2.error_path1 should be [world, j_a, j_x]"
    );
    assert_eq!(
        path2,
        &vec![world, j_b.clone(), j_x.clone()],
        "m2.error_path2 should be [world, j_b, j_x]"
    );
}
