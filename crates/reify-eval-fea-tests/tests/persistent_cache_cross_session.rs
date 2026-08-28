//! Cross-session persistent-FEA-cache integration tests (task #2980).
//!
//! PRD: `docs/prds/v0_3/persistent-fea-cache.md`. This binary is the home for
//! the PRD cases whose subject is the composed public surface — `Engine` plus
//! `reify_eval::persistent_cache`'s free functions plus
//! `reify_eval::sweep_persistent_cache_at_startup` — rather than any one unit.
//!
//! ## Observable signal
//!
//! The two existing round-trips (`reify-eval`'s
//! `persistent_cache_compute_round_trip.rs` and this crate's
//! `buckling_persistent_cache_round_trip.rs`) already establish that a fresh
//! engine on a warm cache dir HITS, and that one headline scalar survives.
//! They stop there. The layer above is what this file pins:
//!
//! * **case 1** — a HIT leaves the on-disk entry byte-for-byte untouched, and
//!   the reconstructed `Value` is bit-identical across the whole `displacement`
//!   and `stress` slabs, not merely to a tolerance on one scalar.
//!
//! ## Dispatch-count probe
//!
//! "The trampoline was not invoked" is read off the engine counters, which move
//! only for persistable targets (`solver::elastic_static` here):
//! `persistent_hit_count() == 1` means the lookup-before-invoke path returned
//! the cached result; `persistent_miss_count() == 0` means nothing fell through
//! to a solve.
//!
//! ## Structure
//!
//! One ephemeral `TempDir` per test, shared across the two engines that stand
//! in for two sessions. Engine A cold-solves and writes; Engine B is a brand-new
//! `Engine` on the same root, which is what makes it a *cross-session* rather
//! than an in-process-cache observation.
//!
//! ## Helpers are file-local by design
//!
//! `reify-eval-fea-tests` has no harness: it is 30 standalone `tests/*.rs`
//! binaries whose helpers are duplicated per file deliberately. The helpers
//! below follow that convention and are not an oversight.
//!
//! ## Cost
//!
//! One cantilever solve per session, ~0.7 s each in debug. Deliberately
//! **not** `debug_assertions`-gated, matching the ungated sibling
//! `persistent_cache_compute_round_trip.rs` (the buckling round-trip is gated
//! only because a buckling solve is ~1000 s in debug).

use reify_core::Severity;
use reify_eval::persistent_cache::{ENGINE_VERSION_HASH, ElasticResult, read_entry};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Cantilever smoke source (compile-time include for binary/source sync).
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
}

/// Recursively check whether a `.bin` file exists anywhere under `dir`.
///
/// Mirrors the same-named helper in `buckling_persistent_cache_round_trip.rs`
/// (this crate duplicates helpers per standalone binary by design).
fn has_bin_file(dir: &std::path::Path) -> bool {
    !collect_bin_files(dir).is_empty()
}

/// Collect every `.bin` path under `dir`, recursively.
fn collect_bin_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(collect_bin_files(&p));
        } else if p.extension().is_some_and(|x| x == "bin") {
            out.push(p);
        }
    }
    out
}

/// Locate THE single `.bin` under `dir`, panicking if there is not exactly one.
///
/// The exactly-one check is load-bearing: a second entry would mean the eval
/// dispatched a compute target this test did not account for, which would make
/// "snapshot the entry" ambiguous rather than merely noisy.
fn find_single_bin(dir: &std::path::Path) -> std::path::PathBuf {
    let mut found = collect_bin_files(dir);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one .bin under {}, found {}: {:?}",
        dir.display(),
        found.len(),
        found,
    );
    found.pop().expect("length was just asserted to be 1")
}

/// Find the `ElasticResult` `StructureInstance` in an engine's snapshot.
fn find_elastic_result(engine: &reify_eval::Engine) -> Value {
    let state = engine
        .eval_state()
        .expect("Engine must have eval_state after eval");
    state
        .snapshot
        .values
        .values()
        .find(|(v, _)| matches!(v, Value::StructureInstance(d) if d.type_name == "ElasticResult"))
        .map(|(v, _)| v.clone())
        .expect("An ElasticResult StructureInstance must exist in the snapshot")
}

/// Extract the raw bit patterns of a sampled field's flat data slab.
///
/// `ElasticResult.displacement` / `.stress` are
/// `Value::Field { source: Sampled, lambda: Arc<Value::SampledField(sf)> }`,
/// and `sf.data` is the row-major slab in SI units. Comparing `to_bits()`
/// rather than `==` is what makes the assertion an EXACTNESS claim: it
/// distinguishes `+0.0` from `-0.0` and treats same-bit NaNs as equal, so a
/// reconstruction that is merely numerically close cannot pass.
fn slab_bits(result: &Value, field: &str) -> Vec<u64> {
    let Value::StructureInstance(data) = result else {
        panic!("result must be a StructureInstance, got: {result:?}");
    };
    match data.fields.get(field) {
        Some(Value::Field { lambda, .. }) => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.iter().map(|x| x.to_bits()).collect(),
            other => panic!("`{field}` must carry a SampledField lambda, got: {other:?}"),
        },
        other => panic!("`{field}` must be a Value::Field, got: {other:?}"),
    }
}

/// Extract `max_von_mises` as a raw bit pattern.
fn max_von_mises_bits(result: &Value) -> u64 {
    let Value::StructureInstance(data) = result else {
        panic!("result must be a StructureInstance, got: {result:?}");
    };
    match data.fields.get("max_von_mises") {
        Some(Value::Scalar { si_value, .. }) => si_value.to_bits(),
        other => panic!("max_von_mises must be a Scalar, got: {other:?}"),
    }
}

/// Case 1 — a cross-session HIT returns the identical entry and never rewrites it.
///
/// The sibling round-trips (`persistent_cache_compute_round_trip.rs`,
/// `buckling_persistent_cache_round_trip.rs`) already assert that a warm engine
/// hits and that ONE headline scalar survives the round trip. This test asserts
/// the strictly stronger properties that neither of them checks:
///
/// * the on-disk `.bin` is byte-for-byte unchanged by session 2 (a hit must not
///   rewrite — `persistent_write` runs only on the trampoline's `Completed` arm,
///   which a hit skips entirely);
/// * `read_entry` decodes to a `PartialEq`-equal `ElasticResult` from both
///   sessions; and
/// * EVERY `f64` of the `displacement` and `stress` slabs — not just
///   `max_von_mises` — is bit-identical between the cold and warm `Value`s.
#[test]
fn cross_session_hit_returns_byte_identical_entry_and_does_not_rewrite_it() {
    let tmp = tempfile::TempDir::new().expect("tmp dir creation must succeed");
    let source = cantilever_source();

    // ── Session 1 (Engine A): cold solve, writes the entry ──────────────────

    let mut engine_a = make_simple_engine();
    engine_a.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_a);

    let compiled_a = parse_and_compile_with_stdlib(source);
    let result_a = engine_a.eval(&compiled_a);

    let errors_a: Vec<_> = result_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_a.is_empty(),
        "session 1 eval must succeed with no Error diagnostics, got: {:?}",
        errors_a,
    );
    assert_eq!(
        engine_a.persistent_hit_count(),
        0,
        "session 1 is a cold solve — persistent_hit_count must be 0",
    );
    assert!(
        has_bin_file(tmp.path()),
        "a .bin must exist under the cache dir after the session-1 cold solve",
    );

    // Snapshot the WHOLE entry, plus the sidecar's mtime, before session 2.
    let bin_path = find_single_bin(tmp.path());
    let input_hash = bin_path
        .file_stem()
        .expect("`.bin` path must have a stem")
        .to_str()
        .expect("cache filenames are ASCII hex")
        .to_string();
    let bytes_before = std::fs::read(&bin_path).expect("the session-1 .bin must be readable");
    let meta_path = bin_path.with_extension("meta");
    assert!(
        meta_path.is_file(),
        "the .meta sidecar must exist beside the .bin after a cold solve",
    );

    let value_a = find_elastic_result(&engine_a);
    let decoded_a: ElasticResult = read_entry(tmp.path(), ENGINE_VERSION_HASH, &input_hash)
        .expect("read_entry must not error on a freshly written entry")
        .expect("the session-1 entry must decode to Some");

    // ── Session 2 (Engine B): warm lookup, must NOT solve and must NOT write ─

    let mut engine_b = make_simple_engine();
    engine_b.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_b);

    let compiled_b = parse_and_compile_with_stdlib(source);
    let result_b = engine_b.eval(&compiled_b);

    let errors_b: Vec<_> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_b.is_empty(),
        "session 2 eval must succeed with no Error diagnostics, got: {:?}",
        errors_b,
    );
    assert_eq!(
        engine_b.persistent_hit_count(),
        1,
        "session 2 must get exactly 1 persistent hit (the cross-session lookup)",
    );
    assert_eq!(
        engine_b.persistent_miss_count(),
        0,
        "session 2 must have 0 persistent misses (no fall-through to the trampoline)",
    );

    // (1) THE NO-REWRITE ASSERTION. A hit skips the trampoline's Completed arm,
    //     so nothing may touch the `.bin`. Byte identity here is available only
    //     because the file is not rewritten at all: the header's `written_at`
    //     field is wall-clock, so any genuine rewrite would perturb these bytes
    //     even for an identical result.
    let bytes_after = std::fs::read(&bin_path).expect("the .bin must still be readable");
    assert_eq!(
        bytes_before.len(),
        bytes_after.len(),
        "session 2 changed the .bin length ({} -> {}); a cache HIT must never \
         rewrite the entry",
        bytes_before.len(),
        bytes_after.len(),
    );
    assert!(
        bytes_before == bytes_after,
        "session 2 rewrote the .bin (first differing offset: {:?}); a cache HIT \
         skips the trampoline's Completed arm, so persistent_write must not run",
        bytes_before
            .iter()
            .zip(bytes_after.iter())
            .position(|(x, y)| x != y),
    );

    // (2) Both sessions' entries decode to PartialEq-equal ElasticResults.
    let decoded_b: ElasticResult = read_entry(tmp.path(), ENGINE_VERSION_HASH, &input_hash)
        .expect("read_entry must not error after the warm session")
        .expect("the entry must still decode to Some after the warm session");
    assert!(
        decoded_a == decoded_b,
        "the decoded ElasticResult must be unchanged across the cross-session hit",
    );

    // (3) EVERY f64 of the displacement and stress slabs is bit-identical
    //     between the cold Value and the warm one — not merely the headline
    //     scalar the sibling round-trips check.
    let value_b = find_elastic_result(&engine_b);
    for field in ["displacement", "stress"] {
        let bits_a = slab_bits(&value_a, field);
        let bits_b = slab_bits(&value_b, field);
        assert!(
            !bits_a.is_empty(),
            "the `{field}` slab must be non-empty, or this assertion is vacuous",
        );
        assert_eq!(
            bits_a.len(),
            bits_b.len(),
            "the `{field}` slab changed length across the cross-session hit \
             ({} -> {})",
            bits_a.len(),
            bits_b.len(),
        );
        let first_diff = bits_a.iter().zip(bits_b.iter()).position(|(x, y)| x != y);
        assert!(
            first_diff.is_none(),
            "the `{field}` slab is not bit-identical across the cross-session hit \
             (first differing index: {first_diff:?}); value_from_elastic_result must \
             reconstruct every sample exactly, not just to a tolerance",
        );
    }

    // (4) max_von_mises bit-exactly, completing the "whole value, not one scalar"
    //     claim.
    assert_eq!(
        max_von_mises_bits(&value_a),
        max_von_mises_bits(&value_b),
        "max_von_mises must be bit-identical across the cross-session hit",
    );
}
