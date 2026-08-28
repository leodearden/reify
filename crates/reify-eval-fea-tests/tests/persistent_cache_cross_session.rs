//! Cross-session persistent-FEA-cache integration tests (task #2980).
//!
//! Step-1 RED: the test body below references file-local helpers
//! (`find_single_bin`, `find_elastic_result`, `slab_bits`, `has_bin_file`)
//! that step-2 adds. Until then this binary does not compile — that failure
//! IS the RED signal.

use reify_core::Severity;
use reify_eval::persistent_cache::{ENGINE_VERSION_HASH, ElasticResult, read_entry};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Cantilever smoke source (compile-time include for binary/source sync).
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
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
        let first_diff = bits_a
            .iter()
            .zip(bits_b.iter())
            .position(|(x, y)| x != y);
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
