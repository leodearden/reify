//! Repo-wide G-allow advisory lane — `ptodo::check()` integration tests.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test g_allow_repo_wide_advisory`
//!
//! Two tests:
//! - **Test A** (hermetic, always runs): drives `ptodo::check()` end-to-end
//!   over three fixture files in a tempdir.  Asserts:
//!   - exactly 2 `g-allow-orphaned` findings (for the un-annotated and γ-style
//!     cites, both with a seeded `done` task), both `Severity::Medium`
//!     (advisory/exit-neutral);
//!   - ZERO `g-allow-orphaned` for the DONE-annotated cite (broadened grammar
//!     exemption); and
//!   - each orphaned finding names the relevant task id.
//! - **Test B** (live anti-drift, `#[ignore]`): runs `ptodo::check()` over the
//!   real repo against the real `tasks.db`; graceful-skip when git or the DB is
//!   absent.  Asserts every `g-allow-orphaned` finding is `Severity::Medium`
//!   (the advisory lane is NEVER a hard failure in the repo-wide sweep).

mod common;

use common::schema::seed_tasks_db_at;
use reify_audit::{AuditContext, MockGitOps, MockJCodemunchOps, Severity};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// Write `content` to relative `path` inside `root`, creating parent dirs.
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// Test A: hermetic end-to-end check() with a seeded tasks.db.
///
/// Three `.rs` fixtures are written to a tempdir (NOT under crates/reify-audit/,
/// so not allowlisted):
///
/// - `tots_done.rs`   — `// G-allow: helper, task #3870 (κ — TOTS SQP, DONE)`
///                      DONE token in following paren → exempt by broadened rule (a)
/// - `unannotated.rs` — `// G-allow: task #1234 const-slice registry; consumer same-file`
///                      no exemption annotation → owner cite → g-allow-orphaned
/// - `gamma.rs`       — `// G-allow: task #5678 (γ) fn-pointer blind spot`
///                      γ paren has no done/cancelled token → owner cite → g-allow-orphaned
///
/// All three tasks are seeded as `done` in the DB, so both un-exempt owners
/// trigger `g-allow-orphaned`.  The advisory lane surfaces them at
/// `Severity::Medium` (exit-neutral — never a hard failure in the repo-wide sweep).
///
/// RED until step-4 wires the advisory lane into check().
#[test]
fn g_allow_repo_wide_advisory_hermetic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Fixture 1: DONE-annotated cite — exempt by broadened grammar
    write_file(
        root,
        "tots_done.rs",
        "// G-allow: helper, task #3870 (\u{03ba} \u{2014} TOTS SQP, DONE)\n",
    );
    // Fixture 2: plain owner cite — no exemption annotation
    write_file(
        root,
        "unannotated.rs",
        "// G-allow: task #1234 const-slice registry; consumer same-file\n",
    );
    // Fixture 3: γ-style following paren with no done/cancelled token
    write_file(
        root,
        "gamma.rs",
        "// G-allow: task #5678 (\u{03b3}) fn-pointer blind spot\n",
    );

    // Seed the DB at the default resolved path (no env override needed).
    seed_tasks_db_at(
        &root.join(".taskmaster/tasks/tasks.db"),
        &[
            ("master", 3870, "done"),
            ("master", 1234, "done"),
            ("master", 5678, "done"),
        ],
    );

    let mut git = MockGitOps::new();
    git.set_ls_files(vec![
        "tots_done.rs".to_string(),
        "unannotated.rs".to_string(),
        "gamma.rs".to_string(),
    ]);

    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: root.to_path_buf(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let findings = reify_audit::ptodo::check(&ctx);

    // Collect g-allow-orphaned findings only.
    let orphaned: Vec<_> = findings
        .iter()
        .filter(|f| f.summary.starts_with("g-allow-orphaned:"))
        .collect();

    // Exactly 2 orphaned findings (unannotated.rs #1234 and gamma.rs #5678).
    assert_eq!(
        orphaned.len(),
        2,
        "expected exactly 2 g-allow-orphaned findings; got {findings:?}"
    );

    // Both must be Severity::Medium — the advisory lane is exit-neutral.
    for f in &orphaned {
        assert_eq!(
            f.severity,
            Severity::Medium,
            "advisory g-allow-orphaned must be Severity::Medium; got {f:?}"
        );
    }

    // tots_done.rs #3870 is exempt — zero findings for it.
    let tots_orphaned = orphaned
        .iter()
        .any(|f| f.summary.contains("#3870"));
    assert!(
        !tots_orphaned,
        "tots_done.rs #3870 is DONE-annotated and must not appear in orphaned; \
         findings={findings:?}"
    );

    // Each orphaned finding names its id.
    let has_1234 = orphaned.iter().any(|f| f.summary.contains("#1234"));
    let has_5678 = orphaned.iter().any(|f| f.summary.contains("#5678"));
    assert!(has_1234, "expected g-allow-orphaned for #1234; findings={findings:?}");
    assert!(has_5678, "expected g-allow-orphaned for #5678; findings={findings:?}");
}

/// Test B: live anti-drift guard.
///
/// Runs `ptodo::check()` over the real repo against the real `tasks.db`
/// (via `tasks_db_path()`).  Graceful-skip when git or the DB is absent
/// (mirrors the PTODO §6.7 degradation contract).
///
/// The only invariant asserted is that every `g-allow-orphaned` finding is
/// `Severity::Medium` — the advisory lane must NEVER hard-fail the repo-wide
/// sweep.  The count is printed but NOT asserted (avoids a time-bomb as
/// CU1-CU8 cleanups land).
#[ignore = "on-demand live anti-drift guard; run via --ignored. Graceful-skip \
    when git or tasks.db is absent."]
#[test]
fn g_allow_repo_wide_advisory_live() {
    use reify_audit::{AuditContext, MockJCodemunchOps, RealGitOps};
    use rusqlite::Connection;
    use std::collections::HashMap;

    // Resolve workspace root from CARGO_MANIFEST_DIR (crates/reify-audit).
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let ws_root = Path::new(manifest_dir)
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .to_path_buf();

    // Graceful-skip if git is not available.
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("g_allow_repo_wide_advisory_live: skipping — git not available");
        return;
    }

    // Graceful-skip if this does not look like a real repo.
    if !ws_root.join(".git").exists() && !ws_root.join(".git").is_file() {
        eprintln!("g_allow_repo_wide_advisory_live: skipping — not a git repo");
        return;
    }

    // Graceful-skip if tasks.db is absent (task worktrees don't have one).
    let db_path = reify_audit::ptodo::tasks_db_path(&ws_root);
    if !db_path.exists() {
        eprintln!(
            "g_allow_repo_wide_advisory_live: skipping — tasks.db not found at {db_path:?}"
        );
        return;
    }

    let git = RealGitOps::new(ws_root.clone());
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: ws_root.clone(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let findings = reify_audit::ptodo::check(&ctx);

    let orphaned: Vec<_> = findings
        .iter()
        .filter(|f| f.summary.starts_with("g-allow-orphaned:"))
        .collect();

    // Print count for visibility but do NOT assert a fixed N.
    eprintln!(
        "g_allow_repo_wide_advisory_live: {} g-allow-orphaned finding(s) in live repo",
        orphaned.len()
    );

    // The invariant: advisory lane findings must NEVER be Severity::High.
    for f in &orphaned {
        assert_eq!(
            f.severity,
            Severity::Medium,
            "repo-wide g-allow-orphaned must be Severity::Medium (advisory/exit-neutral); \
             got {f:?}"
        );
    }
}
