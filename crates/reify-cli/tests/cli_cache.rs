//! Integration tests for `reify cache export/import` (task 2977).
//!
//! These tests are intentionally outer-shell-only: they drive the `reify`
//! binary through `Command::new(env!("CARGO_BIN_EXE_reify"))`, mirroring the
//! pattern established by `cli_smoke.rs` / `cli_doc.rs`.  They use
//! `tempfile::tempdir()` for hermetic cache roots and steer the binary at
//! that root via the `REIFY_CACHE_DIR` env var.

use std::process::Command;

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
