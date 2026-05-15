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
    CacheEntryHeader, ENGINE_VERSION_HASH, ElasticResult, write_entry,
};
use tempfile::tempdir;

/// Build a tet-only `ElasticResult` fixture for cache round-trip tests.
///
/// Tet-only is signalled by `shell_channels: None`; the v2 encoder emits a
/// single zero discriminator byte after the existing slabs and the v2 reader
/// decodes back to `None`, so write-then-read round-trips by `PartialEq`.
fn make_elastic_result_fixture() -> ElasticResult {
    ElasticResult {
        displacement: vec![1.0, 2.0, 3.0],
        stress: vec![4.0, 5.0, 6.0],
        max_von_mises: 7.5,
        converged: true,
        iterations: 9,
        solve_time_ms: 42,
        shell_channels: None,
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

    write_entry(
        cache_dir.path(),
        ENGINE_VERSION_HASH,
        &input_hash,
        &fixture,
    )
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
    let header = CacheEntryHeader::read_from(&mut Cursor::new(bin_bytes))
        .expect("bin header must decode");
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
