//! Synthetic jcodemunch index fixtures for the §4.3 freshness-gate tests.
//!
//! The §4.3 gate is a pure function of (index dir, repo id, live HEAD), so the
//! degenerate corpus shapes it must refuse can be *manufactured* on disk
//! rather than provoked out of a real jcodemunch serve. That is what lets the
//! boundary scenarios be hermetic, gate-resident cargo tests instead of
//! `#[ignore]`-gated live ones — a PASS-shaped skip proves nothing, which is
//! precisely the vacuity the freshness gate exists to eliminate.
//!
//! Schema is verbatim from a live jcodemunch index probed while writing this:
//! `CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT)` carrying a
//! `git_head` row, alongside a `symbols` table. Shape follows
//! `write_empty_runs_db` in `tests/cli.rs` (`Connection::open` +
//! `execute_batch` of a verbatim schema in a tempdir).

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
/// - `git_head`: `Some(sha)` writes the `meta.git_head` row; `None` omits it.
/// - `symbol_rows`: how many `symbols` rows to seed. Zero produces the
///   empty-husk shape a `delete-index` leaves behind — an index that exists
///   and knows its commit but answers every query with nothing.
pub fn write_index_db(
    index_dir: &Path,
    repo_id: &str,
    git_head: Option<&str>,
    symbol_rows: usize,
) -> PathBuf {
    std::fs::create_dir_all(index_dir).expect("create index dir");
    let path = index_db_path(index_dir, repo_id);
    let conn = rusqlite::Connection::open(&path).expect("open synthetic index db");
    conn.execute_batch(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);\
         CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT, path TEXT);",
    )
    .expect("create index schema");
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('index_version', '16')",
        [],
    )
    .expect("insert index_version");
    if let Some(sha) = git_head {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('git_head', ?)",
            rusqlite::params![sha],
        )
        .expect("insert git_head");
    }
    for i in 0..symbol_rows {
        conn.execute(
            "INSERT INTO symbols (name, path) VALUES (?, 'src/lib.rs')",
            rusqlite::params![format!("sym_{i}")],
        )
        .expect("insert symbol row");
    }
    path
}

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
