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

/// Extract the 32-hex input hash from a cache entry's `.bin` path.
fn bin_input_hash(bin: &std::path::Path) -> String {
    bin.file_stem()
        .expect("a `.bin` path must have a stem")
        .to_str()
        .expect("cache filenames are ASCII hex")
        .to_string()
}

/// Backdate the mtime of `path` to `age_secs` seconds in the past.
///
/// A file-local clone of `reify_eval::persistent_cache::backdate_mtime`
/// (persistent_cache.rs:1737), which is `#[cfg(test)] pub(crate)` and so is
/// invisible from an integration test in another crate — `cfg(test)` applies
/// only when reify-eval is itself the crate under test, not when it is a
/// dev-dependency. This is a deliberate mirror of that helper, not an
/// accidental duplicate; keep the two bodies in step if either changes.
///
/// Directories are opened read-only and regular files write-only: on Linux
/// `futimens` requires only ownership of the inode, not write access on the
/// file descriptor, which is what makes the directory case work at all (a
/// directory cannot be opened `O_WRONLY`).
fn backdate_mtime(path: &std::path::Path, age_secs: u64) {
    use std::fs::FileTimes;
    use std::time::{Duration, SystemTime};
    let t = SystemTime::now()
        .checked_sub(Duration::from_secs(age_secs))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let times = FileTimes::new().set_modified(t);
    let f = if path.is_dir() {
        std::fs::File::open(path).expect("opening a directory read-only must succeed")
    } else {
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("opening a file write-only must succeed")
    };
    f.set_times(times).expect("set_times must succeed");
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

/// A synthetic 32-ASCII-char stand-in for a *different* build's engine-version
/// hash.
///
/// `cache_key_to_ascii_32` validates LENGTH only (hex-ness is unchecked), which
/// is what makes a fabricated version usable as a directory name. The same
/// idiom is used at `crates/reify-cli/tests/harness_cli/cli_cache.rs:1065` and
/// inside `persistent_cache.rs`'s own `mod tests`.
const FAKE_EVH: &str = "beef0000000000000000000000000eef";

/// Case 2 — an engine-version bump misses and cold-solves, and the superseded
/// generation stays on disk until the startup sweep's grace period expires.
///
/// `ENGINE_VERSION_HASH` is `env!("REIFY_ENGINE_VERSION_HASH")`, a compile-time
/// const emitted unconditionally by `crates/reify-eval/build.rs` — there is no
/// runtime, env or builder override to point the engine at a different version.
/// So the bump is simulated from the other side: session 1 writes under the live
/// version, the directory is then RENAMED to `FAKE_EVH`, and session 2's live
/// lookup finds nothing where it looks. That reproduces the post-bump on-disk
/// state exactly — a stale generation parked beside a freshly-minted live one —
/// with no override hook needed.
#[test]
fn engine_version_bump_misses_cold_solves_and_leaves_old_subdir_until_sweep_prunes_it() {
    let tmp = tempfile::TempDir::new().expect("tmp dir creation must succeed");
    let source = cantilever_source();

    assert_eq!(
        FAKE_EVH.len(),
        32,
        "test invariant: a fabricated engine version must be 32 ASCII chars, or \
         cache_key_to_ascii_32 rejects it before any filesystem work",
    );
    assert_ne!(
        FAKE_EVH, ENGINE_VERSION_HASH,
        "test invariant: the fabricated stale version must differ from the live one",
    );

    // ── Session 1: cold solve under the LIVE engine version ─────────────────

    let mut engine_a = make_simple_engine();
    engine_a.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_a);
    let compiled_a = parse_and_compile_with_stdlib(source);
    let diags_a = engine_a.eval(&compiled_a);
    assert!(
        !diags_a
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "session 1 eval must succeed with no Error diagnostics",
    );

    let live_dir = tmp.path().join(ENGINE_VERSION_HASH);
    let old_bin = find_single_bin(&live_dir);
    let old_hash = bin_input_hash(&old_bin);
    let old_bytes = std::fs::read(&old_bin).expect("the session-1 .bin must be readable");

    // ── Simulate the engine-version bump by renaming the generation aside ───

    let orphan_dir = tmp.path().join(FAKE_EVH);
    std::fs::rename(&live_dir, &orphan_dir).expect("renaming the generation aside must succeed");
    assert!(
        !live_dir.exists(),
        "after the rename the live engine-version subdir must be absent, so \
         session 2 genuinely starts cold",
    );

    // ── Session 2: the live lookup must MISS and cold-solve ─────────────────

    let mut engine_b = make_simple_engine();
    engine_b.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    reify_eval::compute_targets::register_compute_fns(&mut engine_b);
    let compiled_b = parse_and_compile_with_stdlib(source);
    let diags_b = engine_b.eval(&compiled_b);
    assert!(
        !diags_b
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "session 2 eval must succeed with no Error diagnostics",
    );
    assert_eq!(
        engine_b.persistent_hit_count(),
        0,
        "a version bump must invalidate every entry — session 2 must get 0 hits",
    );
    assert!(
        engine_b.persistent_miss_count() >= 1,
        "session 2 must register at least one MISS (the lookup that fell through \
         to a cold solve), got {}",
        engine_b.persistent_miss_count(),
    );

    // A fresh live-version subdir was recreated with its own entry.
    assert!(
        live_dir.is_dir(),
        "session 2 must recreate the live engine-version subdir",
    );
    let new_bin = find_single_bin(&live_dir);
    let new_hash = bin_input_hash(&new_bin);

    // The INPUT hash is content-derived and does not move with the engine
    // version: the same input keys to the same filename under a new version
    // dir. This is the PRD's "bumps invalidate cleanly via miss with no
    // migration code" made concrete — only the directory generation changed.
    assert_eq!(
        new_hash, old_hash,
        "the input hash is derived from the input, not the engine version, so a \
         version bump must re-key the SAME filename under a new generation dir",
    );

    // ── The superseded generation is intact, not corrupted ──────────────────

    assert!(
        orphan_dir.is_dir(),
        "the superseded generation must remain on disk after the bump — it is \
         pruned on an age schedule, never eagerly",
    );
    let orphan_bin = find_single_bin(&orphan_dir);
    assert!(
        std::fs::read(&orphan_bin).expect("the orphan .bin must be readable") == old_bytes,
        "the superseded generation's bytes must be untouched by session 2",
    );

    // "Intact" means the entry is still self-consistent: its header still echoes
    // the version it was WRITTEN under (the live one), not the directory it now
    // happens to sit in.
    //
    // NOTE: this is deliberately NOT `read_entry(tmp, FAKE_EVH, &old_hash) ==
    // Ok(Some(_))`. `read_entry` verifies the header echoes against the key
    // components taken from the PATH, and the echo is baked into the file at
    // write time — so an entry relocated under a different version dir is
    // reported as a miss by design. The assertion below reads the header
    // directly and checks it against the version the entry actually belongs to,
    // which is what "intact" really means here.
    let mut f = std::io::BufReader::new(
        std::fs::File::open(&orphan_bin).expect("the orphan .bin must open"),
    );
    let header =
        reify_eval::persistent_cache::CacheEntryHeader::read_from(&mut f).expect("header decodes");
    header
        .verify_format_version()
        .expect("the superseded entry's format_version must still be current");
    let mut expected_engine = [0u8; 32];
    expected_engine.copy_from_slice(ENGINE_VERSION_HASH.as_bytes());
    let mut expected_input = [0u8; 32];
    expected_input.copy_from_slice(old_hash.as_bytes());
    header
        .verify_field_echoes(&expected_engine, &expected_input)
        .expect(
            "the superseded entry must still echo the engine version it was written \
             under — the rename moved the directory, not the file's contents",
        );

    // And the safety property that falls out of the same mechanism: the
    // relocated entry is NOT served under the version it does not belong to.
    // A stale generation can never be mistaken for a live one.
    assert!(
        read_entry::<ElasticResult>(tmp.path(), FAKE_EVH, &old_hash)
            .expect("a mismatched echo is a miss, never an Err")
            .is_none(),
        "an entry sitting under a version dir it was not written for must read as \
         a MISS (echo mismatch), never as a stale value",
    );

    // ── Startup sweep: a 30-day grace, not an eager delete ──────────────────

    let report_fresh = reify_eval::sweep_persistent_cache_at_startup(tmp.path());
    assert_eq!(
        report_fresh.orphan_dirs_removed, 0,
        "a freshly-mtimed orphan generation must SURVIVE the sweep — ORPHAN_DIR_AGE \
         is a 30-day grace, not an immediate deletion",
    );
    assert!(
        orphan_dir.is_dir(),
        "the orphan generation must still exist after the grace-period sweep",
    );

    // Age the orphan past the grace period and sweep again.
    backdate_mtime(
        &orphan_dir,
        reify_eval::persistent_cache::ORPHAN_DIR_AGE.as_secs() + 24 * 3600,
    );
    let report_aged = reify_eval::sweep_persistent_cache_at_startup(tmp.path());
    assert_eq!(
        report_aged.orphan_dirs_removed, 1,
        "an orphan generation older than ORPHAN_DIR_AGE must be pruned",
    );
    assert!(
        !orphan_dir.exists(),
        "the aged orphan generation must be gone after the sweep",
    );

    // The LIVE generation is untouched by the prune — the whole point of the
    // exact-name check that runs before any age test.
    assert!(
        live_dir.is_dir(),
        "the live engine-version subdir must survive the orphan prune",
    );
    assert!(
        read_entry::<ElasticResult>(tmp.path(), ENGINE_VERSION_HASH, &new_hash)
            .expect("read_entry must not error on the live entry")
            .is_some(),
        "the live generation's entry must still be readable after the orphan prune",
    );
}
