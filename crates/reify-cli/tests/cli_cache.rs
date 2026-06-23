//! Integration tests for `reify cache export/import` (task 2977).
//!
//! These tests are intentionally outer-shell-only: they drive the `reify`
//! binary through `Command::new(env!("CARGO_BIN_EXE_reify"))`, mirroring the
//! pattern established by `cli_smoke.rs` / `cli_doc.rs`.  They use
//! `tempfile::tempdir()` for hermetic cache roots and steer the binary at
//! that root via the `REIFY_CACHE_DIR` env var.

use std::io::{Cursor, Read, Write};
use std::process::{Command, Stdio};

use reify_eval::persistent_cache::{
    CacheEntryHeader, ENGINE_VERSION_HASH, ENTRY_FORMAT_VERSION, ElasticResult, STALE_TEMPFILE_AGE,
    read_entry, shard_dir, write_entry,
};
use tempfile::tempdir;

/// Build a tetrahedral-only (no shell elements) `ElasticResult` fixture for
/// cache round-trip tests.
///
/// Tetrahedral-only is signalled by `shell_channels: None` (per-element
/// top/bottom shell stress + local frames are absent because the FEA mesh
/// has no shell elements).  The v2 encoder emits a single zero discriminator
/// byte after the existing slabs and the v2 reader decodes back to `None`,
/// so write-then-read round-trips by `PartialEq`.
fn make_elastic_result_fixture() -> ElasticResult {
    ElasticResult {
        displacement: vec![1.0, 2.0, 3.0],
        stress: vec![4.0, 5.0, 6.0],
        max_von_mises: 7.5,
        converged: true,
        iterations: 9,
        solve_time_ms: 42,
        shell_channels: None,
        // v3 fields (task #3428 step-4): minimal non-zero values so the
        // serialised header is well-formed and the round-trip verifies.
        grid_bounds_min: [0.0, 0.0, 0.0],
        grid_bounds_max: [1.0, 1.0, 1.0],
        grid_counts: [1, 1, 1],
        divergence: vec![0.1],
        gradient: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9],
        curl: vec![0.1, 0.2, 0.3],
    }
}

#[test]
fn help_text_mentions_cache_export_subcommand() {
    // `reify` with no args should mention `cache export` alongside the other
    // commands so operators can discover the subcommand from `--help`.
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify with no args should exit non-zero"
    );
    assert!(
        stderr.contains("cache export"),
        "help text should mention 'cache export' subcommand, got: {stderr}"
    );
}

#[test]
fn cache_with_no_subcommand_shows_usage() {
    // `reify cache` (no sub-subcommand) should exit non-zero and print the
    // cache-specific usage banner.
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify cache with no sub-subcommand should exit non-zero"
    );
    assert!(
        stderr.contains("Usage: reify cache"),
        "should show cache-specific usage message, got: {stderr}"
    );
}

#[test]
fn cache_unknown_subcommand_shows_usage() {
    // `reify cache foo` (unknown sub-subcommand) should be rejected with the
    // cache-specific usage banner.
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "foo"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify cache foo should exit non-zero"
    );
    assert!(
        stderr.contains("Usage: reify cache"),
        "should show cache-specific usage message, got: {stderr}"
    );
}

#[test]
fn cache_export_with_no_hash_shows_export_usage() {
    // `reify cache export` (no positional hash) should exit non-zero with the
    // export-specific usage banner.  Pinned cache dir keeps the test hermetic.
    let cache_dir = tempdir().expect("tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "export"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify cache export with no hash should exit non-zero"
    );
    assert!(
        stderr.contains("Usage: reify cache export <hash>"),
        "should show export-specific usage, got: {stderr}"
    );
}

#[test]
fn export_existing_entry_writes_tar_with_bin_and_meta_to_stdout() {
    // Seed a cache entry, run `reify cache export <hash>`, and verify the
    // captured stdout is a tar containing `<hash>.bin` and `<hash>.meta`.
    // The bin's leading 92 bytes must decode as a `CacheEntryHeader` whose
    // `engine_version_hash` matches the live `ENGINE_VERSION_HASH`.
    let cache_dir = tempdir().expect("tempdir");
    let input_hash = "a".repeat(32);
    let fixture = make_elastic_result_fixture();

    write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
        .expect("write_entry must seed the source cache");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "export", &input_hash])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache export should succeed; status={:?} stderr={stderr}",
        output.status
    );

    let mut archive = tar::Archive::new(Cursor::new(output.stdout));
    let mut entries_seen: Vec<(String, Vec<u8>)> = Vec::new();
    for entry_result in archive
        .entries()
        .expect("tar entries iterator must construct")
    {
        let mut entry = entry_result.expect("tar entry must decode");
        let path = entry
            .path()
            .expect("tar entry path must decode")
            .display()
            .to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry body");
        entries_seen.push((path, bytes));
    }

    let names: Vec<&str> = entries_seen.iter().map(|(p, _)| p.as_str()).collect();
    let expected_bin = format!("{input_hash}.bin");
    let expected_meta = format!("{input_hash}.meta");
    assert!(
        names.iter().any(|n| *n == expected_bin),
        "tar must contain {expected_bin}, got entries: {names:?}"
    );
    assert!(
        names.iter().any(|n| *n == expected_meta),
        "tar must contain {expected_meta}, got entries: {names:?}"
    );

    let bin_bytes = &entries_seen
        .iter()
        .find(|(p, _)| p == &expected_bin)
        .expect("bin entry found")
        .1;
    let header =
        CacheEntryHeader::read_from(&mut Cursor::new(bin_bytes)).expect("bin header must decode");
    assert_eq!(
        &header.engine_version_hash[..],
        ENGINE_VERSION_HASH.as_bytes(),
        "exported bin header must carry the live ENGINE_VERSION_HASH"
    );
}

#[test]
fn export_with_missing_entry_writes_error_and_exits_failure() {
    // `reify cache export <hash>` against a hash that doesn't exist in the
    // cache must print a `no such cache entry` error to stderr and exit
    // non-zero.
    let cache_dir = tempdir().expect("tempdir");
    let hash = "00112233445566778899aabbccddeeff";

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "export", hash])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify cache export with no such entry should exit non-zero"
    );
    assert!(
        stderr.contains("no such cache entry"),
        "stderr should mention 'no such cache entry', got: {stderr}"
    );
}

/// Walk `dir` recursively and return paths to any `.bin` or `.meta` files.
/// Used by import tests to verify the destination cache is (or isn't) populated.
fn collect_cache_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        let read = match std::fs::read_dir(&p) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("bin") | Some("meta")
            ) {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn import_malformed_tar_exits_failure_and_leaves_cache_empty() {
    // Pipe random non-tar bytes (sized large enough that the tar parser
    // actually tries to read the first header) and verify `reify cache
    // import` rejects them cleanly without writing to the cache.
    let cache_dir = tempdir().expect("tempdir");
    let garbage: Vec<u8> = std::iter::repeat_n(b'X', 4096).collect();

    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "import"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reify");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(&garbage).expect("write garbage to stdin");
    }
    let output = child.wait_with_output().expect("wait_with_output");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "import of garbage bytes should fail; stderr={stderr}"
    );
    assert!(
        stderr.contains("reify cache import:"),
        "stderr should mention 'reify cache import:', got: {stderr}"
    );
    // The stderr must surface tar-parser-shaped error verbiage so we know
    // the import body actually attempted to parse — not just that the stub
    // returned FAILURE.  Tar errors usually mention "archive" or "header"
    // or "block" or "tar"; we accept any of those.
    let stderr_lc = stderr.to_ascii_lowercase();
    assert!(
        stderr_lc.contains("tar")
            || stderr_lc.contains("archive")
            || stderr_lc.contains("header")
            || stderr_lc.contains("block")
            || stderr_lc.contains("checksum")
            || stderr_lc.contains("invalid")
            || stderr_lc.contains("magic"),
        "stderr should surface tar-parser error verbiage, got: {stderr}"
    );

    let cache_files = collect_cache_files(cache_dir.path());
    assert!(
        cache_files.is_empty(),
        "cache dir should remain empty after failed import, found: {cache_files:?}"
    );
}

#[test]
fn round_trip_export_import_preserves_elastic_result() {
    // Full pipeline: seed `src` cache, export to a tar byte buffer, import the
    // buffer into a fresh `dst` cache, then read the entry back and verify it
    // round-trips by `PartialEq` (covers all fields including
    // `shell_channels: None`).
    let src = tempdir().expect("src tempdir");
    let dst = tempdir().expect("dst tempdir");
    let input_hash = "b".repeat(32);
    let fixture = make_elastic_result_fixture();

    write_entry(src.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
        .expect("write_entry must seed source cache");

    // (1) Export from src.
    let export_output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "export", &input_hash])
        .env("REIFY_CACHE_DIR", src.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn reify cache export");
    assert!(
        export_output.status.success(),
        "export must succeed; stderr={}",
        String::from_utf8_lossy(&export_output.stderr)
    );
    let tar_bytes = export_output.stdout;
    assert!(!tar_bytes.is_empty(), "exported tar must be non-empty");

    // (2) Import into dst.
    let mut import_child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "import"])
        .env("REIFY_CACHE_DIR", dst.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reify cache import");
    {
        let stdin = import_child.stdin.as_mut().expect("import stdin");
        stdin
            .write_all(&tar_bytes)
            .expect("write tar to import stdin");
    }
    let import_output = import_child
        .wait_with_output()
        .expect("wait import process");
    assert!(
        import_output.status.success(),
        "import must succeed; stderr={}",
        String::from_utf8_lossy(&import_output.stderr)
    );

    // (3) Read the entry back from dst and verify equality.
    let round_tripped = read_entry::<ElasticResult>(dst.path(), ENGINE_VERSION_HASH, &input_hash)
        .expect("read_entry must not error");
    let round_tripped = round_tripped.expect("dst cache must contain the imported entry");
    assert_eq!(
        round_tripped, fixture,
        "round-tripped ElasticResult must equal the seeded fixture"
    );
}

#[test]
fn import_with_mismatched_engine_version_warns_and_skips() {
    // Hand-build a 1-entry tar whose `<hash>.bin` carries a well-formed
    // `CacheEntryHeader` but with `engine_version_hash` set to 32 ASCII zeros
    // (the synthesized-mismatch sentinel — see plan Design Decision: the live
    // ENGINE_VERSION_HASH is baked at build time so we can't perturb it at
    // test time).  Import should warn-and-skip (non-fatal exit SUCCESS), and
    // the destination cache must remain empty: neither the synthesized
    // engine-version directory nor the live one should be populated.
    let bogus_evh: [u8; 32] = *b"00000000000000000000000000000000";
    let input_hash_bytes: [u8; 32] = *b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let header = CacheEntryHeader {
        format_version: ENTRY_FORMAT_VERSION,
        engine_version_hash: bogus_evh,
        input_hash: input_hash_bytes,
        solve_time_ms: 0,
        byte_size: 0,
        written_at: -1,
    };
    let mut bin_body: Vec<u8> = Vec::new();
    header.write_to(&mut bin_body).expect("header must encode");
    // ~16 bytes of arbitrary trailing data — won't be decoded since the
    // engine-version check short-circuits before body decode.
    bin_body.extend_from_slice(&[0u8; 16]);

    let mut tar_bytes: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(bin_body.len() as u64);
        header.set_mode(0o644);
        header
            .set_path(format!(
                "{}.bin",
                std::str::from_utf8(&input_hash_bytes).expect("ascii input hash")
            ))
            .expect("set_path");
        header.set_cksum();
        builder
            .append(&header, bin_body.as_slice())
            .expect("append synthesized bin");
        builder.finish().expect("tar finish");
    }

    let cache_dir = tempdir().expect("tempdir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "import"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reify import");
    {
        let stdin = child.stdin.as_mut().expect("import stdin");
        stdin
            .write_all(&tar_bytes)
            .expect("write tar to import stdin");
    }
    let output = child.wait_with_output().expect("wait import");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Warn-and-skip is non-fatal: the run as a whole succeeds even though one
    // entry was rejected.
    assert!(
        output.status.success(),
        "import should exit SUCCESS on engine-version mismatch (warn-and-skip is non-fatal); \
         stderr={stderr}"
    );
    let stderr_lc = stderr.to_ascii_lowercase();
    assert!(
        stderr_lc.contains("engine-version"),
        "stderr should mention 'engine-version', got: {stderr}"
    );
    assert!(
        stderr_lc.contains("skip"),
        "stderr should mention 'skip', got: {stderr}"
    );

    // The destination cache must contain no `.bin` file under either the
    // synthesized engine-version directory or the live one.
    let cache_files = collect_cache_files(cache_dir.path());
    assert!(
        cache_files.is_empty(),
        "cache dir should remain empty after warn-and-skip, found: {cache_files:?}"
    );
}

#[test]
fn import_with_path_traversal_input_hash_warns_and_skips_no_filesystem_writes() {
    // Hand-build a tar with one entry whose tar-path stem is well-formed (32
    // 'a' hex digits — passes the existing single-component tar-slip check)
    // but whose internal `CacheEntryHeader::input_hash` carries a
    // path-traversal payload (`../pwn...`, 32 bytes total). Critically the
    // `engine_version_hash` field is forged to the LIVE ENGINE_VERSION_HASH
    // so the entry sails past the engine-version gate from step-14 and
    // reaches the placement code — only step-16's hex/echo validation can
    // stop the malicious write.
    //
    // Resolution of the traversal: `header.input_hash =
    // "../pwn0000000000000000000000000a"` makes `shard_dir = <cache>/<engine>/..`
    // (OS-resolves to `<cache>/`) and `entry_bin_path = <cache>/<engine>/../../
    // pwn...a.bin` (OS-resolves to `<outer>/pwn...a.bin`), so the vulnerable
    // path writes a file at `<outer>/pwn...a.bin` outside the cache root.
    let evh_bytes: [u8; 32] = ENGINE_VERSION_HASH
        .as_bytes()
        .try_into()
        .expect("ENGINE_VERSION_HASH is 32 ASCII bytes");
    let malicious_input_hash: [u8; 32] = *b"../pwn0000000000000000000000000a";
    let header = CacheEntryHeader {
        format_version: ENTRY_FORMAT_VERSION,
        engine_version_hash: evh_bytes,
        input_hash: malicious_input_hash,
        solve_time_ms: 0,
        byte_size: 0,
        written_at: -1,
    };
    let mut bin_body: Vec<u8> = Vec::new();
    header.write_to(&mut bin_body).expect("header must encode");
    // ~16 bytes of arbitrary trailing data — won't be decoded since the
    // echo/path validation short-circuits before body decode.
    bin_body.extend_from_slice(&[0u8; 16]);

    // Tar entry stem is a well-formed 32-hex string. The tar-path layer is
    // intentionally innocent — only the header echo carries the traversal.
    let tar_stem = "a".repeat(32);
    let mut tar_bytes: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut tar_header = tar::Header::new_gnu();
        tar_header.set_size(bin_body.len() as u64);
        tar_header.set_mode(0o644);
        tar_header
            .set_path(format!("{tar_stem}.bin"))
            .expect("set_path");
        tar_header.set_cksum();
        builder
            .append(&tar_header, bin_body.as_slice())
            .expect("append synthesized bin");
        builder.finish().expect("tar finish");
    }

    // NESTED-tempdir setup for hermetic outside-cache-root assertions: the
    // cache lives at `outer/cache/`, so if the malicious entry escapes the
    // cache root it lands in `outer/` where we can detect it (and tempdir
    // cleanup still reaps it).
    let outer = tempdir().expect("outer tempdir");
    let cache_dir = outer.path().join("cache");
    std::fs::create_dir(&cache_dir).expect("create cache subdir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "import"])
        .env("REIFY_CACHE_DIR", &cache_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reify import");
    {
        let stdin = child.stdin.as_mut().expect("import stdin");
        stdin
            .write_all(&tar_bytes)
            .expect("write tar to import stdin");
    }
    let output = child.wait_with_output().expect("wait import");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // (i) Warn-and-skip is non-fatal per PRD.
    assert!(
        output.status.success(),
        "import should exit SUCCESS on path-traversal warn-and-skip (non-fatal); \
         stderr={stderr}"
    );
    // (ii) stderr surfaces a 'skip' verb so operators see the rejection.
    let stderr_lc = stderr.to_ascii_lowercase();
    assert!(
        stderr_lc.contains("skip"),
        "stderr should mention 'skip', got: {stderr}"
    );
    // (iii) No `.bin`/`.meta` anywhere under the cache root.
    let cache_files = collect_cache_files(&cache_dir);
    assert!(
        cache_files.is_empty(),
        "cache dir should remain empty after warn-and-skip, found: {cache_files:?}"
    );
    // (iv) The malicious path-traversal target file must not exist anywhere
    // outside the cache root.
    let pwn_path = outer.path().join("pwn0000000000000000000000000a.bin");
    assert!(
        !pwn_path.exists(),
        "path-traversal target must not exist: {pwn_path:?}"
    );
    // (v) Defense-in-depth: `outer/` must contain exactly `cache/` — nothing
    // else leaked into the parent.
    let outer_entries: Vec<std::ffi::OsString> = std::fs::read_dir(outer.path())
        .expect("read_dir outer")
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        outer_entries.len(),
        1,
        "outer dir should contain only `cache/`, found: {outer_entries:?}"
    );
}

#[test]
fn cache_export_rejects_invalid_hash_without_panic() {
    // Regression: `cmd_cache_export` previously passed the user-supplied hash
    // straight to `shard_dir`, whose `&input_hash[..2]` slice panics in release
    // builds on short hashes (`""`, `"a"`) and on multibyte UTF-8 that straddles
    // byte boundary 2.  The fix gates on `is_32_lowercase_hex` and emits a
    // usage-style error.  Exercise the three classes of bad input here; each
    // must exit non-zero, surface the new error verbiage, and leave NO panic
    // trace on stderr.
    let cache_dir = tempdir().expect("tempdir");

    let bad_hashes: &[&str] = &[
        "",                                  // empty: would panic at &""[..2]
        "a",                                 // shorter than 2 bytes
        "a×",                                // multibyte char straddling byte 2
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",  // 32 chars but uppercase
        "ZZ",                                // non-hex 2-byte string
        "00112233445566778899aabbccddeeff0", // 33 chars (one over)
    ];

    for bad in bad_hashes {
        let output = Command::new(env!("CARGO_BIN_EXE_reify"))
            .args(["cache", "export", bad])
            .env("REIFY_CACHE_DIR", cache_dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("failed to execute reify binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "reify cache export {bad:?} should exit non-zero; stderr={stderr}"
        );
        let stderr_lc = stderr.to_ascii_lowercase();
        assert!(
            !stderr_lc.contains("panic")
                && !stderr_lc.contains("byte index")
                && !stderr_lc.contains("char boundary"),
            "stderr should not contain panic-shaped output for {bad:?}, got: {stderr}"
        );
        assert!(
            stderr.contains("hash must be 32 lowercase hex digits"),
            "stderr should surface hash-shape error for {bad:?}, got: {stderr}"
        );
    }
}

#[test]
fn import_with_traversal_shaped_tar_path_exits_failure_no_filesystem_writes() {
    // Hand-build a tar with one entry whose tar-path is a literal traversal
    // shape (`../foo.bin`).  This hits the existing tar-slip defense at
    // `entry_path.is_absolute() || entry_path.components().count() != 1`,
    // which produces FAILURE (not warn-and-skip).  Pinning this contract in
    // a test guards against a future refactor that would soften the
    // multi-component-path branch into a warn-and-skip without a deliberate
    // decision.
    let mut tar_bytes: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let body = vec![0u8; 16];
        let mut tar_header = tar::Header::new_gnu();
        tar_header.set_size(body.len() as u64);
        tar_header.set_mode(0o644);
        // GNU tar Header `set_path` rejects literal `..`-leading paths since
        // tar-0.4 (defense-in-depth at the construction layer), so we use
        // `set_path("foo.bin")` to construct a valid header and then patch
        // the path block in-place via `as_old_mut().name`.  The on-disk tar
        // body that the reify import sees will then carry `../pwn.bin`,
        // exercising the `components().count() != 1` branch when the reader
        // resolves the path.
        tar_header.set_path("foo.bin").expect("set_path");
        // Overwrite the path field with `../pwn.bin` directly (USTAR/GNU
        // both allow up to 100 ASCII bytes in the `name` block).  The
        // checksum must be recomputed AFTER the in-place edit.
        let traversal = b"../pwn.bin";
        let name_field = &mut tar_header.as_old_mut().name;
        name_field.fill(0);
        name_field[..traversal.len()].copy_from_slice(traversal);
        tar_header.set_cksum();
        builder
            .append(&tar_header, body.as_slice())
            .expect("append traversal-path entry");
        builder.finish().expect("tar finish");
    }

    let outer = tempdir().expect("outer tempdir");
    let cache_dir = outer.path().join("cache");
    std::fs::create_dir(&cache_dir).expect("create cache subdir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "import"])
        .env("REIFY_CACHE_DIR", &cache_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reify import");
    {
        let stdin = child.stdin.as_mut().expect("import stdin");
        stdin
            .write_all(&tar_bytes)
            .expect("write tar to import stdin");
    }
    let output = child.wait_with_output().expect("wait import");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // FAILURE per the existing tar-slip defense — distinct from the
    // warn-and-skip semantics for header echo / non-hex stem / engine-version
    // mismatch.  The contract distinction is intentional: a tar entry path
    // shaped like `../foo` could not have been produced by `cache export`,
    // so it's treated as malformed input rather than a recoverable per-entry
    // skip.
    assert!(
        !output.status.success(),
        "import of traversal-shaped tar path should exit FAILURE; stderr={stderr}"
    );
    let stderr_lc = stderr.to_ascii_lowercase();
    assert!(
        stderr_lc.contains("traversal") || stderr_lc.contains("rejecting"),
        "stderr should surface traversal-rejection verbiage, got: {stderr}"
    );
    let cache_files = collect_cache_files(&cache_dir);
    assert!(
        cache_files.is_empty(),
        "cache dir must remain empty after FAILURE, found: {cache_files:?}"
    );
    // Also: no write escaped into the outer dir.
    let outer_entries: Vec<std::ffi::OsString> = std::fs::read_dir(outer.path())
        .expect("read_dir outer")
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        outer_entries.len(),
        1,
        "outer dir should contain only `cache/`, found: {outer_entries:?}"
    );
}

#[test]
fn import_with_non_hex_stem_warns_and_skips_no_filesystem_writes() {
    // Hand-build a tar with one entry whose tar-path stem is not a 32-hex
    // string (`hello.bin`).  This hits the `is_32_lowercase_hex(&stem)`
    // warn-and-skip gate added to close the path-traversal hole, BEFORE any
    // body decode runs.  Exit must be SUCCESS (warn-and-skip is non-fatal),
    // stderr must mention 'skip', and no files may be created under the
    // cache root.
    let mut tar_bytes: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let body = vec![0u8; 16];
        let mut tar_header = tar::Header::new_gnu();
        tar_header.set_size(body.len() as u64);
        tar_header.set_mode(0o644);
        tar_header.set_path("hello.bin").expect("set_path");
        tar_header.set_cksum();
        builder
            .append(&tar_header, body.as_slice())
            .expect("append non-hex-stem entry");
        builder.finish().expect("tar finish");
    }

    let cache_dir = tempdir().expect("tempdir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "import"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reify import");
    {
        let stdin = child.stdin.as_mut().expect("import stdin");
        stdin
            .write_all(&tar_bytes)
            .expect("write tar to import stdin");
    }
    let output = child.wait_with_output().expect("wait import");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "import of non-hex stem should exit SUCCESS (warn-and-skip is non-fatal); \
         stderr={stderr}"
    );
    let stderr_lc = stderr.to_ascii_lowercase();
    assert!(
        stderr_lc.contains("skip"),
        "stderr should mention 'skip', got: {stderr}"
    );
    assert!(
        stderr_lc.contains("non-hex") || stderr_lc.contains("hello"),
        "stderr should surface the non-hex-stem-shape diagnostic, got: {stderr}"
    );
    let cache_files = collect_cache_files(cache_dir.path());
    assert!(
        cache_files.is_empty(),
        "cache dir should remain empty after warn-and-skip, found: {cache_files:?}"
    );
}

#[test]
fn cache_stats_on_empty_cache_succeeds_and_prints_entry_count_zero() {
    // `reify cache stats` against an empty cache root must succeed and surface
    // a labelled `Entry count: 0` line plus a `Total size: 0` line.  Pinning
    // the labels here keeps the schema discoverable for both human operators
    // and the later golden test (step-5).
    let cache_dir = tempdir().expect("tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "stats"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache stats on empty cache should succeed; status={:?} stderr={stderr}",
        output.status
    );
    assert!(
        stdout.contains("Entry count: 0"),
        "stdout should contain 'Entry count: 0', got: {stdout}"
    );
    assert!(
        stdout.contains("Total size: 0"),
        "stdout should contain 'Total size: 0', got: {stdout}"
    );
}

#[test]
fn cache_stats_reports_correct_entry_count_and_total_size_for_seeded_cache() {
    // Seed three entries via the persistent_cache::write_entry test helper
    // and assert that `reify cache stats` walks the cache root, counts the
    // .bin files, and reports a non-zero total-size.  Pinning the count
    // (3) keeps the assertion robust to byte-format changes in step-6's
    // golden — only the numeric prefix of the size line is validated here.
    let cache_dir = tempdir().expect("tempdir");
    let fixture = make_elastic_result_fixture();
    for c in ['a', 'b', 'c'] {
        let input_hash: String = std::iter::repeat_n(c, 32).collect();
        write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed the cache");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "stats"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache stats should succeed; status={:?} stderr={stderr}",
        output.status
    );
    assert!(
        stdout.contains("Entry count: 3"),
        "stdout should contain 'Entry count: 3', got: {stdout}"
    );
    // Total-size must be a non-zero numeric value.  Find the `Total size:`
    // line, take the next whitespace-delimited token, parse as u64, assert > 0.
    let size_line = stdout
        .lines()
        .find(|l| l.starts_with("Total size:"))
        .unwrap_or_else(|| panic!("stdout must contain a 'Total size:' line, got: {stdout}"));
    let size_token = size_line
        .split_whitespace()
        .nth(2)
        .unwrap_or_else(|| panic!("'Total size:' line must have a numeric value: {size_line}"));
    let size_n: u64 = size_token
        .parse()
        .unwrap_or_else(|_| panic!("'Total size:' value must parse as u64: {size_line}"));
    assert!(
        size_n > 0,
        "Total size value should be > 0 for a seeded cache, got: {size_line}"
    );
}

#[test]
fn cache_stats_output_schema_golden_with_top_n_and_hit_rate_caveat() {
    // Pin the full stats schema: 7 seeded entries with strictly increasing
    // payload sizes (so the .bin byte sizes differ and the top-N ordering is
    // deterministic) must produce:
    //   (a) `Cache directory: <tempdir path>`
    //   (b) `Entry count: 7`
    //   (c) a non-zero `Total size:` line
    //   (d) a `Top 5 largest entries:` section listing exactly 5 entries by
    //       32-hex input_hash, sorted descending by byte size
    //   (e) a hit-rate caveat sentence mentioning `hit rate` and
    //       `per-process` (or `current process`)
    let cache_dir = tempdir().expect("tempdir");

    // Seed 7 entries with strictly increasing displacement vec lengths so each
    // .bin is a different size on disk.  Use the index character as the hash
    // prefix so the hash strings are distinct AND the ordering is recoverable
    // from the test (largest = 'g'×32 = idx 6, smallest = 'a'×32 = idx 0).
    let chars = ['a', 'b', 'c', 'd', 'e', 'f', 'g'];
    for (i, ch) in chars.iter().enumerate() {
        let input_hash: String = std::iter::repeat_n(*ch, 32).collect();
        let displacement: Vec<f64> = (0..(i + 1) * 64).map(|n| n as f64).collect();
        let stress: Vec<f64> = (0..(i + 1) * 64).map(|n| (n as f64) * 1.5).collect();
        let n = displacement.len();
        let fixture = ElasticResult {
            displacement,
            stress,
            max_von_mises: 7.5,
            converged: true,
            iterations: 9,
            solve_time_ms: 42,
            shell_channels: None,
            // v3 fields (task #3428 step-4): grid proportional to entry size.
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [1.0, 1.0, 1.0],
            grid_counts: [n as u64, 1, 1],
            divergence: vec![0.0; n],
            gradient: vec![0.0; n * 9],
            curl: vec![0.0; n * 3],
        };
        write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed the cache");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "stats"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache stats should succeed; status={:?} stderr={stderr}",
        output.status
    );

    // (a) Cache-directory line carries the tempdir path.
    let cache_dir_str = cache_dir.path().display().to_string();
    assert!(
        stdout.contains(&format!("Cache directory: {cache_dir_str}")),
        "stdout should contain 'Cache directory: {cache_dir_str}', got: {stdout}"
    );

    // (b) Entry count is 7.
    assert!(
        stdout.contains("Entry count: 7"),
        "stdout should contain 'Entry count: 7', got: {stdout}"
    );

    // (c) Total size is non-zero.
    let size_line = stdout
        .lines()
        .find(|l| l.starts_with("Total size:"))
        .unwrap_or_else(|| panic!("stdout must contain a 'Total size:' line, got: {stdout}"));
    let size_n: u64 = size_line
        .split_whitespace()
        .nth(2)
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| panic!("'Total size:' line must have a u64 value: {size_line}"));
    assert!(
        size_n > 0,
        "Total size should be > 0 for a seeded cache, got: {size_line}"
    );

    // (d) Top-N section: header label + exactly 5 hash-prefixed lines, sorted
    // descending by byte size.  Find the header by its fixed label, then take
    // the next 5 non-blank lines and assert each contains a 32-hex input_hash
    // (one of the seeded chars).  Also assert the descending-by-size
    // invariant by taking the trailing numeric token (byte size) on each row.
    let top_header_idx = stdout
        .lines()
        .position(|l| l.starts_with("Top 5 largest entries"))
        .unwrap_or_else(|| {
            panic!("stdout must contain a 'Top 5 largest entries' section header, got: {stdout}")
        });
    let top_lines: Vec<&str> = stdout
        .lines()
        .skip(top_header_idx + 1)
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect();
    assert_eq!(
        top_lines.len(),
        5,
        "Top-5 section should list exactly 5 entries, got: {top_lines:?}"
    );
    // Each row must contain a 32-char repeating hash (we'll just check that
    // the 32-char repeating prefix substring of one of the seeded chars
    // appears in the row).
    let mut prev_size: Option<u64> = None;
    for row in &top_lines {
        let mut found_hash = None;
        for ch in chars.iter() {
            let hash: String = std::iter::repeat_n(*ch, 32).collect();
            if row.contains(&hash) {
                found_hash = Some(hash);
                break;
            }
        }
        assert!(
            found_hash.is_some(),
            "Top-N row must contain a seeded 32-hex hash, got: {row}"
        );
        // Pull the trailing whitespace-delimited numeric token as the byte
        // size and assert descending order.
        let size_token: u64 = row
            .split_whitespace()
            .filter_map(|t| t.parse::<u64>().ok())
            .next_back()
            .unwrap_or_else(|| panic!("Top-N row must contain a u64 byte size, got: {row}"));
        if let Some(prev) = prev_size {
            assert!(
                size_token <= prev,
                "Top-N rows must be sorted descending by size; \
                 prev={prev}, current={size_token}, row={row}"
            );
        }
        prev_size = Some(size_token);
    }

    // (e) Hit-rate caveat sentence.  Lower-case the haystack so we don't
    // pin the exact capitalization of the sentence.
    let stdout_lc = stdout.to_ascii_lowercase();
    assert!(
        stdout_lc.contains("hit rate"),
        "stdout should contain 'hit rate' caveat, got: {stdout}"
    );
    assert!(
        stdout_lc.contains("per-process") || stdout_lc.contains("current process"),
        "stdout should contain 'per-process' or 'current process' caveat, got: {stdout}"
    );
}

#[test]
fn cache_stats_aggregates_across_engine_versions() {
    // Reviewer-driven amendment (suggestion #6): the stats walk is documented
    // to aggregate across ALL engine-version subdirs (cross-version bloat is
    // visible to the operator), but every existing stats test seeds under the
    // LIVE `ENGINE_VERSION_HASH`, so a regression that silently restricted the
    // walk to the live subdir would pass the whole suite.  Seed under TWO
    // distinct synthesized 32-hex engine-version hashes (one matching the
    // live constant — so we never depend on hash drift — plus one definitely-
    // stale fake) and assert (a) `Entry count: 2` and (b) both input hashes
    // appear somewhere in the Top-N section.
    let cache_dir = tempdir().expect("tempdir");
    // The "stale" engine-version hash must be 32 lowercase hex and must NOT
    // equal the live ENGINE_VERSION_HASH constant — `"1".repeat(32)` is well-
    // defined and clearly synthetic.  If the live hash ever happens to be
    // "1"*32 the assertion below catches it before the test misleads.
    let stale_engine = "1".repeat(32);
    assert_ne!(
        stale_engine.as_str(),
        ENGINE_VERSION_HASH,
        "test invariant: synthesized stale engine-version hash must differ from live"
    );
    let live_input = "a".repeat(32);
    let stale_input = "b".repeat(32);
    let fixture = make_elastic_result_fixture();
    write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &live_input, &fixture)
        .expect("write_entry must seed under live engine version");
    write_entry(cache_dir.path(), &stale_engine, &stale_input, &fixture)
        .expect("write_entry must seed under synthesized stale engine version");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "stats"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify stats");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache stats should succeed; stderr={stderr}"
    );
    assert!(
        stdout.contains("Entry count: 2"),
        "stats must aggregate both engine-version subdirs (Entry count: 2), got: {stdout}"
    );
    // Locate the Top-N section and assert both input-hash stems appear in it.
    // Pinning hash presence (not row-by-row equality) keeps the assertion
    // robust to ordering changes — what we're proving is "stats sees BOTH
    // engine-version subdirs", not "stats sorts in any particular order".
    let top_header_idx = stdout
        .lines()
        .position(|l| l.starts_with("Top 5 largest entries"))
        .unwrap_or_else(|| {
            panic!("stdout must contain a 'Top 5 largest entries' section header, got: {stdout}")
        });
    let top_section: String = stdout
        .lines()
        .skip(top_header_idx + 1)
        .take(5)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        top_section.contains(&live_input),
        "Top-N section must contain live-engine input hash {live_input}, got: {top_section}"
    );
    assert!(
        top_section.contains(&stale_input),
        "Top-N section must contain stale-engine input hash {stale_input}, got: {top_section}"
    );
}

#[test]
fn cache_clear_without_yes_refuses_and_exits_failure_and_preserves_entries() {
    // `reify cache clear` (no `--yes`) must refuse the destructive op,
    // exit non-zero, mention `--yes` on stderr, and leave the seeded
    // entry untouched.
    let cache_dir = tempdir().expect("tempdir");
    let input_hash = "c".repeat(32);
    let fixture = make_elastic_result_fixture();
    write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
        .expect("write_entry must seed the cache");
    // Sanity-check the seed.
    let pre = collect_cache_files(cache_dir.path());
    assert!(
        pre.iter()
            .any(|p| p.extension().and_then(|e| e.to_str()) == Some("bin")),
        "test setup: cache must contain a .bin before clear; found: {pre:?}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "clear"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "reify cache clear without --yes should exit non-zero; stderr={stderr}"
    );
    assert!(
        stderr.contains("--yes"),
        "stderr should explicitly mention '--yes' for the destructive-op refusal, got: {stderr}"
    );

    let post: Vec<_> = collect_cache_files(cache_dir.path())
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
        .collect();
    assert_eq!(
        post.len(),
        1,
        "seeded .bin must remain after the refused clear, found: {post:?}"
    );
}

#[test]
fn cache_clear_yes_then_stats_round_trip_reports_empty() {
    // Canonical clear+stats round-trip from the task description: seed three
    // entries via write_entry, run `reify cache clear --yes`, then run
    // `reify cache stats` against the same cache root and assert the
    // filesystem is empty AND stats reports zero entries.
    let cache_dir = tempdir().expect("tempdir");
    let fixture = make_elastic_result_fixture();
    for c in ['a', 'b', 'c'] {
        let input_hash: String = std::iter::repeat_n(c, 32).collect();
        write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed the cache");
    }

    // (1) clear --yes
    let clear_output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "clear", "--yes"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify clear");
    assert!(
        clear_output.status.success(),
        "reify cache clear --yes should succeed; stderr={}",
        String::from_utf8_lossy(&clear_output.stderr)
    );

    // (2) stats reports 0
    let stats_output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "stats"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify stats");
    let stats_stdout = String::from_utf8_lossy(&stats_output.stdout);
    assert!(
        stats_output.status.success(),
        "reify cache stats post-clear should succeed; stderr={}",
        String::from_utf8_lossy(&stats_output.stderr)
    );
    assert!(
        stats_stdout.contains("Entry count: 0"),
        "post-clear stats should report 'Entry count: 0', got: {stats_stdout}"
    );
    assert!(
        stats_stdout.contains("Total size: 0"),
        "post-clear stats should report 'Total size: 0', got: {stats_stdout}"
    );

    // (3) Filesystem is empty.
    let post = collect_cache_files(cache_dir.path());
    assert!(
        post.is_empty(),
        "cache root should contain no .bin/.meta files after clear --yes; found: {post:?}"
    );
}

#[test]
fn cache_clear_with_engine_version_yes_clears_only_target_subdir_and_preserves_others() {
    // Seed entries under TWO synthesized engine-version subdirs by passing
    // distinct fake 32-hex strings to write_entry — bypassing the live
    // ENGINE_VERSION_HASH so the test isn't sensitive to build-time hash
    // drift.  Run `reify cache clear --yes --engine-version <hash_a>` and
    // assert (a) exit SUCCESS, (b) the hash_a subdir is gone, (c) the hash_b
    // subdir and its entries still exist.
    let cache_dir = tempdir().expect("tempdir");
    let hash_a = "1".repeat(32);
    let hash_b = "2".repeat(32);
    let input_hash_a = "a".repeat(32);
    let input_hash_b = "b".repeat(32);
    let fixture = make_elastic_result_fixture();
    write_entry(cache_dir.path(), &hash_a, &input_hash_a, &fixture)
        .expect("write_entry must seed engine-version A");
    write_entry(cache_dir.path(), &hash_b, &input_hash_b, &fixture)
        .expect("write_entry must seed engine-version B");

    let subdir_a = cache_dir.path().join(&hash_a);
    let subdir_b = cache_dir.path().join(&hash_b);
    assert!(
        subdir_a.is_dir(),
        "test setup: subdir_a must exist before clear: {subdir_a:?}"
    );
    assert!(
        subdir_b.is_dir(),
        "test setup: subdir_b must exist before clear: {subdir_b:?}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "clear", "--yes", "--engine-version", &hash_a])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify clear");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache clear --yes --engine-version <hash_a> should succeed; stderr={stderr}"
    );

    assert!(
        !subdir_a.exists(),
        "subdir_a must be removed after clear --engine-version: {subdir_a:?}"
    );
    assert!(
        subdir_b.is_dir(),
        "subdir_b must remain after clear --engine-version <hash_a>: {subdir_b:?}"
    );
    // The hash_b entry's .bin must still be on disk.
    let surviving: Vec<_> = collect_cache_files(cache_dir.path())
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
        .collect();
    assert_eq!(
        surviving.len(),
        1,
        "exactly one .bin (under engine_version B) must survive, found: {surviving:?}"
    );
}

#[test]
fn cache_clear_engine_version_nondir_target_is_idempotent_success_and_preserves_file() {
    // Regression guard for esc-2976-107: `reify cache clear --yes
    // --engine-version <hash>` against a *regular file* (not a directory) at
    // cache_root/<hash> must return SUCCESS, not FAILURE (ENOTDIR from
    // remove_dir_all).  This mirrors the bulk-clear branch's `if !path.is_dir()
    // { continue }` guard (cache.rs:348-350) which already silently skips stray
    // regular files.
    let cache_dir = tempdir().expect("tempdir");
    let hash = "0123456789abcdef0123456789abcdef".to_string();

    // Seed a regular file (not a directory) at the would-be engine-version path.
    let stray = cache_dir.path().join(&hash);
    std::fs::write(&stray, b"not a directory").expect("seed stray regular file");
    assert!(
        stray.is_file(),
        "test setup: stray must be a regular file before running the command: {stray:?}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "clear", "--yes", "--engine-version", &hash])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify cache clear");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache clear --yes --engine-version against a stray regular file \
         should succeed (idempotent no-op); stderr={stderr}"
    );

    // Defense-in-depth: the stray file must not have been deleted.
    assert!(
        stray.is_file(),
        "stray regular file must still exist after idempotent clear; stray={stray:?}"
    );
    assert_eq!(
        std::fs::read(&stray).expect("stray file must be readable"),
        b"not a directory",
        "stray file contents must be unchanged after idempotent clear"
    );
}

#[test]
fn cache_gc_under_cap_is_no_op_and_preserves_all_entries() {
    // With REIFY_CACHE_MAX_BYTES set well above the seeded total, `reify
    // cache gc` must (a) succeed, (b) report 0 evicted entries / 0 evicted
    // bytes, and (c) leave all seeded .bin files on disk.
    let cache_dir = tempdir().expect("tempdir");
    let fixture = make_elastic_result_fixture();
    for c in ['a', 'b', 'c'] {
        let input_hash: String = std::iter::repeat_n(c, 32).collect();
        write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed the cache");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "gc"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .env("REIFY_CACHE_MAX_BYTES", "10000000000") // 10 GB — well above the seeded footprint
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify gc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache gc under cap should succeed; stderr={stderr}"
    );
    assert!(
        stdout.contains("Evicted entries: 0"),
        "stdout should report 'Evicted entries: 0' under cap, got: {stdout}"
    );
    assert!(
        stdout.contains("Evicted bytes: 0"),
        "stdout should report 'Evicted bytes: 0' under cap, got: {stdout}"
    );

    let bins: Vec<_> = collect_cache_files(cache_dir.path())
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
        .collect();
    assert_eq!(
        bins.len(),
        3,
        "all three seeded .bins must remain after no-op gc, found: {bins:?}"
    );
}

#[test]
fn cache_gc_evicts_when_forced_over_cap() {
    // Seed several entries with non-trivial body sizes (large displacement
    // vectors) so the total .bin footprint exceeds a 1 KiB synthetic cap.
    // Run `reify cache gc` with REIFY_CACHE_MAX_BYTES=1024 and assert
    // (a) exit SUCCESS, (b) evicted_count > 0, (c) remaining_bytes <= 1024,
    // (d) collect_cache_files reports strictly fewer .bins than seeded.
    //
    // Critically: seed under the LIVE ENGINE_VERSION_HASH (not a synthesized
    // hash) because cmd_cache_gc hard-codes ENGINE_VERSION_HASH per the
    // design decision.
    let cache_dir = tempdir().expect("tempdir");
    let chars = ['a', 'b', 'c', 'd', 'e'];
    for ch in chars.iter() {
        let input_hash: String = std::iter::repeat_n(*ch, 32).collect();
        // 4096 doubles = 32 KiB uncompressed displacement; the compressed
        // .bin will still exceed 1 KiB on disk for each entry, ensuring the
        // total tops the synthetic cap by a wide margin.
        let n = 4096_usize;
        let displacement: Vec<f64> = (0..n).map(|n| (n as f64).sin()).collect();
        let stress: Vec<f64> = (0..n).map(|n| (n as f64).cos()).collect();
        let fixture = ElasticResult {
            displacement,
            stress,
            max_von_mises: 7.5,
            converged: true,
            iterations: 9,
            solve_time_ms: 42,
            shell_channels: None,
            // v3 fields (task #3428 step-4): minimal non-zero values.
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [1.0, 1.0, 1.0],
            grid_counts: [n as u64, 1, 1],
            divergence: vec![0.0; n],
            gradient: vec![0.0; n * 9],
            curl: vec![0.0; n * 3],
        };
        write_entry(cache_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed the cache");
    }

    let pre_count = collect_cache_files(cache_dir.path())
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
        .count();
    assert_eq!(
        pre_count,
        chars.len(),
        "test setup: all seeded .bins must be present pre-gc"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "gc"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .env("REIFY_CACHE_MAX_BYTES", "1024")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify gc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache gc with tiny cap should succeed; stderr={stderr}"
    );

    let evicted_n: u64 = parse_labelled_u64(&stdout, "Evicted entries:")
        .unwrap_or_else(|| panic!("stdout must report 'Evicted entries: <n>', got: {stdout}"));
    assert!(
        evicted_n > 0,
        "Evicted entries must be > 0 when over cap, got: {stdout}"
    );
    let remaining: u64 = parse_labelled_u64(&stdout, "Remaining bytes:")
        .unwrap_or_else(|| panic!("stdout must report 'Remaining bytes: <n>', got: {stdout}"));
    assert!(
        remaining <= 1024,
        "Remaining bytes must be <= 1024 cap, got: {remaining}"
    );

    let post_count = collect_cache_files(cache_dir.path())
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
        .count();
    assert!(
        post_count < pre_count,
        "post-gc .bin count ({post_count}) must be strictly less than pre-gc ({pre_count})"
    );
}

/// Find a `<label> <u64>` line in `stdout` and return the parsed integer.
///
/// Leading whitespace on the line is tolerated so a future cosmetic change
/// (e.g. indenting gc rows for visual grouping) doesn't silently break the
/// parser — the helper would otherwise return `None` and tests would panic
/// from `unwrap_or_else` with a misleading "label missing" message.
fn parse_labelled_u64(stdout: &str, label: &str) -> Option<u64> {
    for line in stdout.lines() {
        if let Some(rest) = line.trim_start().strip_prefix(label) {
            return rest.split_whitespace().next().and_then(|t| t.parse().ok());
        }
    }
    None
}

#[test]
fn cache_stats_honors_cache_dir_flag_overriding_env_var() {
    // Per the design decision, --cache-dir is parsed per-subcommand and is
    // placed AFTER the sub-subcommand name (e.g.
    // `reify cache stats --cache-dir /tmp/foo`).  Verify the flag overrides
    // REIFY_CACHE_DIR by seeding two entries in flag_dir and one in env_dir,
    // then running `reify cache stats --cache-dir <flag_dir>` with
    // REIFY_CACHE_DIR=<env_dir> in the child env: stats must report the
    // flag_dir's count (2), not the env_dir's (1).
    let flag_dir = tempdir().expect("flag tempdir");
    let env_dir = tempdir().expect("env tempdir");
    let fixture = make_elastic_result_fixture();
    for c in ['a', 'b'] {
        let input_hash: String = std::iter::repeat_n(c, 32).collect();
        write_entry(flag_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed flag_dir");
    }
    {
        let input_hash: String = "c".repeat(32);
        write_entry(env_dir.path(), ENGINE_VERSION_HASH, &input_hash, &fixture)
            .expect("write_entry must seed env_dir");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "cache",
            "stats",
            "--cache-dir",
            &flag_dir.path().display().to_string(),
        ])
        .env("REIFY_CACHE_DIR", env_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify stats");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify cache stats --cache-dir should succeed; stderr={stderr}"
    );
    assert!(
        stdout.contains("Entry count: 2"),
        "stats with --cache-dir <flag_dir> must report flag_dir's count (2), \
         not env_dir's (1); got: {stdout}"
    );
    assert!(
        !stdout.contains("Entry count: 1"),
        "stats must NOT report env_dir's count (1), got: {stdout}"
    );
}

#[test]
fn cache_export_with_extra_positional_shows_export_usage() {
    // `reify cache export aaa bbb` (extra positional past the hash) should be
    // rejected with the export-specific usage banner.
    let cache_dir = tempdir().expect("tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["cache", "export", "aaa", "bbb"])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify cache export with extra positional should exit non-zero"
    );
    assert!(
        stderr.contains("Usage: reify cache export <hash>"),
        "should show export-specific usage, got: {stderr}"
    );
}

#[test]
fn cli_check_sweeps_stale_persistent_cache_tempfile_at_startup() {
    // Seed a stale .tmp.* file at the correct cache layout location:
    //   <cache_root>/<ENGINE_VERSION_HASH>/<shard>/.tmp.stale_seed
    // Backdate its mtime past STALE_TEMPFILE_AGE, then run
    // `reify check bracket.ri` with REIFY_CACHE_DIR pointing at the tempdir.
    //
    // Asserts: (a) exit success, (b) stale file gone.
    //
    // Exercises the wiring added by task 3698: the CLI calls
    // cache::run_startup_sweep() before the command dispatcher so every
    // engine-using invocation inherits the cleanup for free.
    use std::fs::{self, File, OpenOptions};
    use std::io::Write as _;
    use std::time::{Duration, SystemTime};

    let cache_dir = tempdir().expect("tempdir");

    // 32-char hex hash; two-char prefix "aa" determines the shard subdir.
    let input_hash = "aa00000000000000000000000000dead";
    let shard = shard_dir(cache_dir.path(), ENGINE_VERSION_HASH, input_hash);
    fs::create_dir_all(&shard).expect("create shard dir");

    let stale_path = shard.join(".tmp.stale_seed");
    {
        let mut f = File::create(&stale_path).expect("create stale tempfile");
        f.write_all(b"stale content").expect("write stale content");
    }

    // Backdate mtime to > STALE_TEMPFILE_AGE (1 h) in the past; 2-min buffer
    // guards against races on slow CI machines.
    let stale_mtime = SystemTime::now() - (STALE_TEMPFILE_AGE + Duration::from_secs(120));
    let times = std::fs::FileTimes::new().set_modified(stale_mtime);
    {
        let file = OpenOptions::new()
            .write(true)
            .open(&stale_path)
            .expect("open stale file to backdate mtime");
        file.set_times(times).expect("backdate mtime");
    }

    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", fixture.to_str().unwrap()])
        .env("REIFY_CACHE_DIR", cache_dir.path())
        // Remove vars the resolver also consults so a stale dev-shell env
        // cannot trigger an InvalidMaxBytes error and skip the sweep.
        .env_remove("REIFY_CACHE_MAX_BYTES")
        .env_remove("XDG_CACHE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify check bracket.ri should exit 0; stderr={stderr}"
    );
    assert!(
        !stale_path.exists(),
        "stale .tmp.* file must be removed by startup sweep; path={stale_path:?}"
    );
}
