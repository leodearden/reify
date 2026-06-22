//! Cross-restart persistent-cache e2e integration test (task #3428 step-9).
//!
//! ## Observable signal (PRD §8-ι / §7.2)
//!
//! An FEA `.ri` file evaluates; engine exits; a fresh engine restarts with
//! the same cache dir; the first eval of the same file HITS the persistent
//! on-disk cache with NO trampoline call and `result.max_von_mises` matches.
//!
//! ## Dispatch-count probe
//!
//! "Trampoline not invoked" is confirmed via the engine counters:
//! - `persistent_hit_count() == 1` → the lookup-before-invoke path fired
//!   and returned the cached result without calling the trampoline.
//! - `persistent_miss_count() == 0` → no lookup miss occurred (which would
//!   have fallen through to the trampoline).
//!
//! Together these two counters constitute the dispatch-count probe: a miss
//! (trampoline call) would flip `persistent_miss_count` to 1 and leave
//! `persistent_hit_count` at 0.
//!
//! ## Structure
//!
//! 1. `tmp` — one ephemeral `TempDir` shared across both engines.
//! 2. **Engine A** — cold solve: `set_persistent_cache_dir(Some(tmp))`, eval
//!    the cantilever fixture, assert `max_von_mises` is sane and a `.bin` now
//!    exists under `tmp`.
//! 3. **Engine B** — warm lookup: brand-new engine, same `tmp`, eval the same
//!    source, assert `persistent_hit_count() == 1` (no solve invoked).
//!
//! ## RED / GREEN
//!
//! The test is written in the step-9 commit. It passes GREEN immediately
//! because all of steps 2/4/6/8 are already implemented. The test pins the
//! full assembled pipeline end-to-end so future regressions are caught.

use reify_core::Severity;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Cantilever smoke source (compile-time include for binary/source sync).
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
}

/// Extract `max_von_mises` scalar from an `ElasticResult`
/// `Value::StructureInstance`.
fn extract_max_von_mises(result: &Value) -> f64 {
    match result {
        Value::StructureInstance(data) => {
            match data.fields.get(&"max_von_mises".to_string()) {
                Some(Value::Scalar { si_value, .. }) => *si_value,
                other => panic!("max_von_mises must be Scalar, got: {:?}", other),
            }
        }
        other => panic!("result must be StructureInstance, got: {:?}", other),
    }
}

/// Find the `ElasticResult` `StructureInstance` value in an engine's snapshot.
fn find_elastic_result(engine: &reify_eval::Engine) -> Value {
    let state = engine
        .eval_state()
        .expect("Engine must have eval_state after eval");
    state
        .snapshot
        .values
        .values()
        .find(|(v, _)| {
            matches!(v, Value::StructureInstance(d) if d.type_name == "ElasticResult")
        })
        .map(|(v, _)| v.clone())
        .expect("An ElasticResult StructureInstance must exist in the snapshot")
}

/// Cross-restart persistent-cache round-trip (the headline observable).
///
/// Engine A evaluates the cantilever fixture cold (writing to cache).
/// Engine B evaluates the same fixture with the same cache dir, hitting the
/// persistent entry — `persistent_hit_count() == 1` and
/// `persistent_miss_count() == 0` prove the solver trampoline was not invoked.
#[test]
fn persistent_cache_cross_restart_round_trip() {
    let tmp = tempfile::TempDir::new().expect("tmp dir creation must succeed");
    let source = cantilever_source();

    // ── Engine A: cold solve ────────────────────────────────────────────────

    let mut engine_a = make_simple_engine();
    engine_a.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_a);

    let compiled_a = parse_and_compile_with_stdlib(source);
    let result_a = engine_a.eval(&compiled_a);

    // (A1) No error diagnostics.
    let errors_a: Vec<_> = result_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_a.is_empty(),
        "Engine A eval must succeed with no Error diagnostics, got: {:?}",
        errors_a
    );

    // (A2) Extract max_von_mises from Engine A result.
    let result_val_a = find_elastic_result(&engine_a);
    let max_vm_a = extract_max_von_mises(&result_val_a);
    assert!(
        max_vm_a.is_finite() && max_vm_a > 0.0,
        "Engine A max_von_mises must be finite and > 0, got: {}",
        max_vm_a
    );

    // (A3) Engine A must not have had any persistent HIT (cold path, first run).
    assert_eq!(
        engine_a.persistent_hit_count(),
        0,
        "Engine A is a cold solve — persistent_hit_count must be 0",
    );

    // (A4) A .bin must now exist somewhere under the 2-level-sharded cache dir.
    fn has_bin_file(dir: &std::path::Path) -> bool {
        std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .any(|e| {
                let p = e.path();
                if p.is_dir() {
                    has_bin_file(&p)
                } else {
                    p.extension().is_some_and(|x| x == "bin")
                }
            })
    }
    assert!(
        has_bin_file(tmp.path()),
        "A .bin file must exist under the cache dir after Engine A cold solve"
    );

    // ── Engine B: warm lookup ───────────────────────────────────────────────

    let mut engine_b = make_simple_engine();
    engine_b.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_b);

    let compiled_b = parse_and_compile_with_stdlib(source);
    let result_b = engine_b.eval(&compiled_b);

    // (B1) No error diagnostics.
    let errors_b: Vec<_> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_b.is_empty(),
        "Engine B eval must succeed with no Error diagnostics, got: {:?}",
        errors_b
    );

    // (B2) Persistent HIT count must be exactly 1.
    //      This is the primary RED signal until step-8 wires the lookup.
    assert_eq!(
        engine_b.persistent_hit_count(),
        1,
        "Engine B must get exactly 1 persistent cache hit (the cross-restart lookup); \
         step-8 wires the lookup-before-invoke path in run_compute_dispatch",
    );

    // (B3) No lookup MISS occurred — this confirms the trampoline was not invoked
    //      (a miss would flip persistent_miss_count to 1 and call the trampoline).
    assert_eq!(
        engine_b.persistent_miss_count(),
        0,
        "Engine B must have 0 persistent miss (no fall-through to trampoline); \
         persistent_miss_count > 0 would mean the cache was not consulted or the \
         key did not match",
    );

    // (B4) result.max_von_mises must match Engine A's — value_from_elastic_result
    //      must reconstruct faithfully from the on-disk entry.
    let result_val_b = find_elastic_result(&engine_b);
    let max_vm_b = extract_max_von_mises(&result_val_b);
    let rel_err = (max_vm_b - max_vm_a).abs() / max_vm_a.abs().max(f64::EPSILON);
    assert!(
        rel_err < 1e-10,
        "Engine B max_von_mises {:.6e} must match Engine A {:.6e} (rel_err={:.2e}); \
         value_from_elastic_result must faithfully reconstruct the result",
        max_vm_b,
        max_vm_a,
        rel_err,
    );
}
