//! Stale `--test <stem>` doc-invocation guard.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test stale_test_target_doc_invocations`
//!
//! Scans tracked `crates/**/*.rs` source for hand-typed `cargo test --test
//! <stem>` / `--test <stem>` doc invocations and asserts each `<stem>`
//! resolves to an existing top-level `crates/<crate>/tests/<stem>.rs`. Where
//! the referenced test is `#[ignore]`d, the doc invocation is its entire
//! operational access route — a stale stem silently retires the guard it
//! documents (recurred in tasks 5282, 5649, 5687).
//!
//! Two tests:
//! - **Test A** (hermetic, always runs): drives `scan()` over synthetic
//!   fixture files in a tempdir — no git repo required.
//! - **Test B** (live anti-drift, no `#[ignore]`): runs `scan()` over the
//!   real repo's tracked `crates/**/*.rs`; graceful-skip when git is absent.
//!
//! Escape hatch: a line carrying the literal `stale-test-target:allow` opts
//! out of the sweep for that line (mirrors ptodo.rs's `ptodo:allow`, §6.8).
//! Same-line only; trailing `— reason` prose expected.

use std::path::Path;

/// Write `content` to relative `path` inside `root`, creating parent dirs.
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// Test A: hermetic extraction + resolution over two synthetic sources.
///
/// `crates/crate-a/src/live.rs` names a stem with a matching
/// `crates/crate-a/tests/live_stem.rs` — zero findings.
/// `crates/crate-a/src/stale.rs` names a stem with no matching test file —
/// exactly one finding.
#[test]
fn extracts_and_resolves_basic_stem() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        "crates/crate-a/src/live.rs",
        "//! Run with `cargo test --test live_stem`\n", // stale-test-target:allow — synthetic fixture
    );
    write_file(root, "crates/crate-a/tests/live_stem.rs", "// empty\n");

    write_file(
        root,
        "crates/crate-a/src/stale.rs",
        "//! Run with `cargo test --test stale_stem`\n", // stale-test-target:allow — synthetic fixture
    );

    let files = vec![
        "crates/crate-a/src/live.rs".to_string(),
        "crates/crate-a/tests/live_stem.rs".to_string(),
        "crates/crate-a/src/stale.rs".to_string(),
    ];

    let findings = scan(root, &files);

    assert_eq!(
        findings.len(),
        1,
        "expected exactly one finding; got {findings:?}"
    );
    assert_eq!(findings[0].crate_name, "crate-a");
    assert_eq!(findings[0].stem, "stale_stem");
    assert_eq!(findings[0].file, "crates/crate-a/src/stale.rs");
    assert_eq!(findings[0].line, 1);
}
