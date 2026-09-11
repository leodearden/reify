//! Synthetic jcodemunch index fixtures for the §4.3 freshness-gate tests.
//!
//! The §4.3 gate is a pure function of (index dir, repo id, live HEAD), so the
//! degenerate corpus shapes it must refuse can be *manufactured* on disk
//! rather than provoked out of a real jcodemunch serve. That is what lets the
//! boundary scenarios be hermetic, gate-resident cargo tests instead of
//! `#[ignore]`-gated live ones — a PASS-shaped skip proves nothing, which is
//! precisely the vacuity the freshness gate exists to eliminate.
//!
//! The DB writer itself lives in the library behind the `test-support`
//! feature (`jcodemunch_index::write_index_db`) and is re-exported below: it
//! encodes a captured upstream jcodemunch schema, so exactly one copy must
//! exist or a schema change would have to be mirrored into two suites. What
//! this module owns is the *independent* half — the on-disk filename the
//! operator would predict, and a real one-commit git repo to compare heads
//! against.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Where jcodemunch stores the index for `repo_id`: the id with `/` flattened
/// to `-`, plus `.db`.
///
/// Deliberately re-derived here rather than calling the library's
/// `jcodemunch_index::index_db_path`. The tests assert the binary probes the
/// path the *operator* would predict, so computing the expected path with the
/// same function under test would make a wrong-but-self-consistent derivation
/// pass — the test must hold an independent opinion about the filename.
pub fn index_db_path(index_dir: &Path, repo_id: &str) -> PathBuf {
    index_dir.join(format!("{}.db", repo_id.replace('/', "-")))
}

/// Write a synthetic index DB for `repo_id` under `index_dir`.
///
/// Re-exported from the library's `test-support` writer rather than
/// re-implemented here. The doc on `index_db_path` above explains why THAT is
/// deliberately duplicated — the tests must hold an independent opinion about
/// the filename the operator would predict — but the argument does not extend
/// to the DB *writer*: it is not a function under test, it encodes a captured
/// upstream jcodemunch schema, and two copies would mean a schema change had
/// to be mirrored twice or the unit and integration suites would silently be
/// testing different corpora.
// `allow(unused_imports)` for the same reason this module carries
// `allow(dead_code)`: `tests/common/` is compiled into EVERY integration test
// binary, and only `cli.rs` uses the §4.3 fixtures.
#[allow(unused_imports)]
pub use reify_audit::jcodemunch_index::write_index_db;

/// Build a real single-commit git repo at `dir` and return its HEAD sha.
///
/// A real repo (not a stub) is the point: the gate reads `live_head` by
/// shelling out to `git rev-parse HEAD`, so the comparison it performs is only
/// meaningful against a genuine commit. Identity and gpgsign are pinned the
/// way `tests/real_git_ops.rs` pins them, so the helper works on a host with
/// no git identity configured.
pub fn init_git_repo_with_one_commit(dir: &Path) -> String {
    let run = |args: &[&str]| {
        let status = crate::common::git_env::git_cmd(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
        assert!(status.success(), "git {args:?} exited {:?}", status.code());
    };
    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("seed.rs"), "pub fn seed() {}\n").expect("write seed file");
    run(&["add", "."]);
    run(&["commit", "-m", "seed"]);

    let out = crate::common::git_env::git_cmd(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse HEAD");
    assert!(out.status.success(), "git rev-parse HEAD failed");
    let head = String::from_utf8(out.stdout).expect("utf8 sha").trim().to_string();
    assert_eq!(head.len(), 40, "expected a full 40-char sha, got {head:?}");
    head
}

/// Derive the repo id the binary will compute for `project_root`, independently
/// of the implementation under test.
///
/// Reproduces jcodemunch's `_local_repo_name`
/// (`local/<basename>-<sha1(abs_path)[..8]>`) via the library's `sha1_hex`.
/// The SHA-1 itself is pinned to NIST vectors and two measured ground truths in
/// the library's own unit tests, so reusing it here borrows a *verified*
/// primitive while keeping this helper's opinion about the id's SHAPE
/// independent.
pub fn expected_repo_id(project_root: &Path) -> String {
    let abs = std::fs::canonicalize(project_root).expect("canonicalize project root");
    let basename = abs
        .file_name()
        .expect("project root has a final component")
        .to_string_lossy()
        .into_owned();
    let digest = reify_audit::jcodemunch_index::sha1_hex(abs.to_string_lossy().as_bytes());
    format!("local/{}-{}", basename, &digest[..8])
}
