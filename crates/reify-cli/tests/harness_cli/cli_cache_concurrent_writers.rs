//! Concurrent multi-process persistent-cache writers (task #2980, PRD case 3).
//!
//! PRD `docs/prds/v0_3/persistent-fea-cache.md` §"Concurrency" states the
//! contract this module pins: *"Duplicate work is acceptable… last writer wins…
//! Not worth a lock."* There is no file lock anywhere on the write path — safety
//! comes entirely from `write_entry` staging into a `.tmp.<random>` tempfile in
//! the destination shard dir and then `persist()`-ing it over the final name,
//! i.e. a POSIX `rename(2)`, which is atomic within a filesystem.
//!
//! ## Why the assertion is "one final file + ZERO surviving tempfiles"
//!
//! Task #2980's text says the race should leave "one tempfile". Taken literally
//! that assertion would be FALSE against a correct implementation: each writer
//! mints its OWN tempfile via
//! `tempfile::Builder::prefix(".tmp.").tempfile_in(shard_dir)`, so at the peak
//! of the race two `.tmp.*` files exist, and `persist()` consumes each one by
//! renaming it away. What the contract actually guarantees at rest is that the
//! shard ends up with exactly one published `.bin` (+ its `.meta`) and no debris
//! — which is what this module asserts.
//!
//! ## Grounding (measured, not assumed)
//!
//! A SINGLE `reify eval --cache-dir <tmp> tests/fixtures/fea_cantilever_deterministic.ri`
//! run at task #2980 implementation time exited 0 in ~1.5 s wall (debug profile,
//! cold cache) and left exactly:
//!
//! ```text
//! <root>/ae333bea8df962b09ffa4ca919456f1d/bc/bc8f4e786e0e07cc1b15ba72ab2f79b9.bin   165881 bytes
//! <root>/ae333bea8df962b09ffa4ca919456f1d/bc/bc8f4e786e0e07cc1b15ba72ab2f79b9.meta       1 byte
//! ```
//!
//! i.e. one `.bin`, one `.meta`, zero `.tmp.*`, under
//! `<root>/<ENGINE_VERSION_HASH>/<hash[0..2]>/<hash>.{bin,meta}`. The input hash
//! is 32 lowercase hex chars and the shard dir is its first two. The exact hash
//! values above are NOT asserted anywhere below — they move whenever the engine
//! version or the fixture changes — but the SHAPE is, and the ~1.5 s figure is
//! why racing two of these is affordable inside the merge gate.
//!
//! The fixture passes `ElasticOptions(deterministic: true)`, which is what makes
//! "both writers produce the same result" a contract rather than an accident of
//! scheduling; see the fixture's own header.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use reify_eval::persistent_cache::{
    CacheEntryHeader, ENGINE_VERSION_HASH, ElasticResult, read_entry,
};
use tempfile::tempdir;

/// Absolute path to the deterministic cantilever fixture both writers solve.
fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fea_cantilever_deterministic.ri")
}

/// A `reify eval --cache-dir <cache>` invocation, not yet spawned.
///
/// The `--cache-dir` FLAG is used rather than `REIFY_CACHE_DIR` because it has
/// the highest precedence (`crates/reify-cli/src/main.rs:2049`, applied at
/// :2115/:2133 after env and config resolution), so the test needs no env at
/// all to steer the binary. The three `env_remove`s below still matter: a stale
/// dev-shell `REIFY_CACHE_MAX_BYTES` can fail max-bytes resolution outright, and
/// leaving `REIFY_CACHE_DIR`/`XDG_CACHE_HOME` set would let a startup sweep
/// touch the developer's real cache.
fn eval_command(cache: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify"));
    cmd.arg("eval")
        .arg("--cache-dir")
        .arg(cache)
        .arg(fixture_path())
        .env_remove("REIFY_CACHE_DIR")
        .env_remove("REIFY_CACHE_MAX_BYTES")
        .env_remove("XDG_CACHE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Every `.bin` under `dir`, recursively, sorted for a stable comparison.
fn collect_bins(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_bins_into(dir, &mut out);
    out.sort();
    out
}

fn collect_bins_into(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_bins_into(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("bin") {
            out.push(path);
        }
    }
}

/// Count `.tmp.*` crashed-writer / in-flight leftovers anywhere under `dir`.
fn count_tmp_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            n += count_tmp_files(&path);
        } else if path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.starts_with(".tmp."))
        {
            n += 1;
        }
    }
    n
}

/// The 32-hex input hash of a cache entry, taken from its `.bin` file stem.
fn bin_input_hash(bin: &Path) -> String {
    bin.file_stem()
        .and_then(|s| s.to_str())
        .expect("a cache .bin path always has a UTF-8 stem")
        .to_string()
}

/// Case 3 — two concurrent `reify eval` processes racing the same cache key
/// leave exactly one entry and no orphan tempfiles.
///
/// This is a REAL multi-process race, not a simulation: both children are
/// `spawn()`ed back-to-back before either is waited on, so their solves and
/// their `write_entry` calls genuinely overlap. Whether they collide inside the
/// rename window on any given run is up to the scheduler — the contract asserted
/// here (one published entry, no debris, no corruption, value-identical to a
/// solitary solve) is what must hold either way.
#[test]
fn two_concurrent_reify_eval_processes_leave_one_entry_and_no_orphan_tempfiles() {
    let cache = tempdir().expect("cache tempdir must be creatable");

    // ── (b) Both children in flight before either is waited on ──────────────

    let first = eval_command(cache.path())
        .spawn()
        .expect("spawning the first reify eval must succeed");
    let second = eval_command(cache.path())
        .spawn()
        .expect("spawning the second reify eval must succeed");

    let first = first
        .wait_with_output()
        .expect("waiting on the first reify eval must succeed");
    let second = second
        .wait_with_output()
        .expect("waiting on the second reify eval must succeed");

    // ── (c) Both succeeded, and neither silently degraded its dispatch ──────
    //
    // The trampoline guard is the positive check borrowed from
    // `cli_build_fea.rs`: without a registered compute trampoline the engine
    // falls back to body-inlining, `max_von_mises` becomes Undef, and the run
    // still exits 0 — writing NO cache entry. Without this guard the whole test
    // could pass vacuously.

    for (label, out) in [("first", &first), ("second", &second)] {
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "the {label} concurrent writer must exit 0; stderr:\n{stderr}",
        );
        assert!(
            !stderr.contains("no registered compute trampoline"),
            "the {label} concurrent writer must dispatch solver::elastic_static rather \
             than falling back to body-inlining; stderr:\n{stderr}",
        );
    }

    // ── (d) THE CONTRACT: one published entry, no debris ────────────────────

    let version_dir = cache.path().join(ENGINE_VERSION_HASH);
    assert!(
        version_dir.is_dir(),
        "the engine-version subdir must exist after two successful writers",
    );

    let bins = collect_bins(&version_dir);
    assert_eq!(
        bins.len(),
        1,
        "last-writer-wins must leave exactly ONE published .bin, found: {bins:?}",
    );
    let bin = &bins[0];
    let meta = bin.with_extension("meta");
    assert!(
        meta.is_file(),
        "the surviving .bin must have its .meta sidecar beside it: {meta:?}",
    );
    assert_eq!(
        count_tmp_files(cache.path()),
        0,
        "no .tmp.* may survive anywhere under the cache root — each writer mints its \
         own tempfile and persist() consumes it via rename(2)",
    );

    // ── (e) The survivor is not a torn interleaving of the two writers ──────

    let hash = bin_input_hash(bin);
    let survivor: ElasticResult = read_entry(cache.path(), ENGINE_VERSION_HASH, &hash)
        .expect("read_entry must not error on the surviving entry")
        .expect("the surviving entry must decode");
    assert!(
        survivor.converged,
        "the surviving entry must be a converged solve, not a partial write",
    );

    let mut f = std::fs::File::open(bin).expect("the surviving .bin must open");
    let header = CacheEntryHeader::read_from(&mut f)
        .expect("the surviving entry's header must decode — a torn rename would not");
    header
        .verify_format_version()
        .expect("the surviving entry's format_version must be current");
    let mut expected_engine = [0u8; 32];
    expected_engine.copy_from_slice(ENGINE_VERSION_HASH.as_bytes());
    let mut expected_input = [0u8; 32];
    expected_input.copy_from_slice(hash.as_bytes());
    header
        .verify_field_echoes(&expected_engine, &expected_input)
        .expect("the surviving entry's engine and input echoes must match its own path");

    // ── (f) Whichever writer won produced the canonical result ──────────────
    //
    // A third, SOLITARY run into a fresh root gives an uncontended reference.
    // The fixture is `deterministic: true`, so the race's survivor must agree
    // with it — both on the cache KEY (same input hash → same file name) and on
    // the VALUE.

    let reference_cache = tempdir().expect("reference cache tempdir must be creatable");
    let reference = eval_command(reference_cache.path())
        .output()
        .expect("the solitary reference run must execute");
    let reference_stderr = String::from_utf8_lossy(&reference.stderr);
    assert!(
        reference.status.success(),
        "the solitary reference run must exit 0; stderr:\n{reference_stderr}",
    );

    let reference_bins = collect_bins(&reference_cache.path().join(ENGINE_VERSION_HASH));
    assert_eq!(
        reference_bins.len(),
        1,
        "the solitary reference run must leave exactly one .bin, found: {reference_bins:?}",
    );
    let reference_hash = bin_input_hash(&reference_bins[0]);
    assert_eq!(
        hash, reference_hash,
        "the raced entry and the solitary reference must share the SAME cache key — \
         a differing input hash would mean the race changed what was being cached",
    );

    let reference_result: ElasticResult =
        read_entry(reference_cache.path(), ENGINE_VERSION_HASH, &reference_hash)
            .expect("read_entry must not error on the reference entry")
            .expect("the reference entry must decode");

    // The house tolerance for FEA scalar agreement
    // (`persistent_cache_compute_round_trip.rs:188-196`).
    let rel_err = (survivor.max_von_mises - reference_result.max_von_mises).abs()
        / reference_result.max_von_mises.abs().max(f64::MIN_POSITIVE);
    assert!(
        rel_err < 1e-10,
        "the race survivor's max_von_mises ({}) must match the solitary reference ({}) \
         to rel_err < 1e-10, got {rel_err:e}",
        survivor.max_von_mises,
        reference_result.max_von_mises,
    );
}
