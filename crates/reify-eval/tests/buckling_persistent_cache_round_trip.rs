//! Cross-restart persistent-cache e2e integration test for buckling
//! (task #3459 step-7).
//!
//! ## Observable signal (PRD §13-κ / §7.2)
//!
//! A buckling `.ri` file evaluates; engine exits; a fresh engine restarts with
//! the same cache dir; the first eval of the same file HITS the persistent
//! on-disk cache with NO trampoline call and `modes[0].eigenvalue` matches the
//! cold-solve value bit-for-bit.
//!
//! ## Dispatch-count probe
//!
//! "Trampoline not invoked" is confirmed via the engine counters:
//! - `persistent_hit_count() == 1` → the lookup-before-invoke path fired
//!   and returned the cached result without calling the trampoline.
//! - `persistent_miss_count() == 0` → no lookup miss occurred.
//!
//! ## Structure
//!
//! 1. `tmp` — one ephemeral `TempDir` shared across both engines.
//! 2. **Engine A** — cold solve: `set_persistent_cache_dir(Some(tmp))`, eval
//!    the buckling column fixture, assert no Error diagnostics, a `BucklingResult`
//!    exists, `persistent_hit_count() == 0`, and a `.bin` now exists under `tmp`.
//! 3. **Engine B** — warm lookup: brand-new engine, same `tmp`, eval the same
//!    source, assert `persistent_hit_count() == 1` and
//!    `persistent_miss_count() == 0` (trampoline NOT invoked), and
//!    `modes[0].eigenvalue` bit-matches Engine A.
//!
//! ## Release-only gate
//!
//! A full buckling solve takes ~25 s release / ~1000 s debug (nx=ny=8, nz=160;
//! ~13k grid points).  The test is skipped in debug builds via
//! `#[cfg_attr(debug_assertions, ignore = "...")]`, matching the gate on all
//! other buckling e2e tests in this crate.
//!
//! ## GREEN status
//!
//! Written as the step-7 commit; GREEN immediately because steps 2/4/6 are
//! already implemented.  Pins the full assembled pipeline end-to-end.

use reify_core::Severity;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Buckling column smoke fixture (compile-time include for binary/source sync).
fn buckling_source() -> &'static str {
    include_str!("../../../examples/buckling_column_smoke.ri")
}

/// Extract `modes[0].eigenvalue` from a `BucklingResult`-shaped
/// `Value::StructureInstance`.  Panics if the shape does not match.
fn extract_first_eigenvalue(result: &Value) -> f64 {
    match result {
        Value::StructureInstance(data) if data.type_name == "BucklingResult" => {
            match data.fields.get("modes") {
                Some(Value::List(modes)) => match modes.first() {
                    Some(Value::StructureInstance(mode_data)) => {
                        match mode_data.fields.get("eigenvalue") {
                            Some(Value::Real(r)) => *r,
                            other => panic!(
                                "modes[0].eigenvalue must be Real, got: {:?}",
                                other
                            ),
                        }
                    }
                    other => panic!("modes[0] must be StructureInstance, got: {:?}", other),
                },
                other => panic!("modes must be List, got: {:?}", other),
            }
        }
        other => panic!(
            "expected BucklingResult StructureInstance, got: {:?}",
            other
        ),
    }
}

/// Find the `BucklingResult` `StructureInstance` in an engine's snapshot.
fn find_buckling_result(engine: &reify_eval::Engine) -> Value {
    let state = engine
        .eval_state()
        .expect("Engine must have eval_state after eval");
    state
        .snapshot
        .values
        .values()
        .find(|(v, _)| {
            matches!(v, Value::StructureInstance(d) if d.type_name == "BucklingResult")
        })
        .map(|(v, _)| v.clone())
        .expect("A BucklingResult StructureInstance must exist in the snapshot")
}

/// Recursively check whether a `.bin` file exists anywhere under `dir`.
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

/// Cross-restart buckling persistent-cache round-trip (the headline observable).
///
/// Engine A evaluates the buckling column fixture cold (writing to cache).
/// Engine B evaluates the same fixture with the same cache dir, hitting the
/// persistent entry — `persistent_hit_count() == 1` and
/// `persistent_miss_count() == 0` prove the solver trampoline was not invoked.
#[cfg_attr(debug_assertions, ignore = "heavy buckling solve; release-only")]
#[test]
fn buckling_persistent_cache_cross_restart_round_trip() {
    let tmp = tempfile::TempDir::new().expect("tmp dir creation must succeed");
    let source = buckling_source();

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
        errors_a,
    );

    // (A2) Extract modes[0].eigenvalue from Engine A result.
    let result_val_a = find_buckling_result(&engine_a);
    let eigenvalue_a = extract_first_eigenvalue(&result_val_a);
    assert!(
        eigenvalue_a.is_finite() && eigenvalue_a > 0.0,
        "Engine A modes[0].eigenvalue must be finite and > 0, got: {}",
        eigenvalue_a,
    );

    // (A3) Engine A must not have had any persistent HIT (cold path, first run).
    assert_eq!(
        engine_a.persistent_hit_count(),
        0,
        "Engine A is a cold solve — persistent_hit_count must be 0",
    );

    // (A4) A .bin must now exist somewhere under the 2-level-sharded cache dir.
    assert!(
        has_bin_file(tmp.path()),
        "A .bin file must exist under the cache dir after Engine A cold buckling solve",
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
        errors_b,
    );

    // (B2) Persistent HIT count must be exactly 1.
    assert_eq!(
        engine_b.persistent_hit_count(),
        1,
        "Engine B must get exactly 1 persistent cache hit (the cross-restart lookup); \
         a hit_count of 0 means the persistent cache was not consulted or the key \
         did not match",
    );

    // (B3) No lookup MISS occurred — confirms the trampoline was not invoked.
    assert_eq!(
        engine_b.persistent_miss_count(),
        0,
        "Engine B must have 0 persistent misses (no fall-through to trampoline); \
         persistent_miss_count > 0 means the cache was not consulted or the key \
         did not match",
    );

    // (B4) modes[0].eigenvalue must bit-match Engine A.
    //      value_from_buckling_result must reconstruct faithfully from the on-disk entry.
    let result_val_b = find_buckling_result(&engine_b);
    let eigenvalue_b = extract_first_eigenvalue(&result_val_b);
    assert_eq!(
        eigenvalue_a.to_bits(),
        eigenvalue_b.to_bits(),
        "Engine B modes[0].eigenvalue {:.10e} must bit-match Engine A {:.10e}; \
         value_from_buckling_result must faithfully reconstruct the cached result",
        eigenvalue_b,
        eigenvalue_a,
    );
}
