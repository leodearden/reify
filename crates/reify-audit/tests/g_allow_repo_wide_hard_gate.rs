//! Repo-wide G-allow hard-gate lane — `ptodo::check()` integration tests.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test g_allow_repo_wide_hard_gate`
//!
//! Two tests:
//! - **Test A** (hermetic, always runs): drives `ptodo::check()` end-to-end
//!   over five fixture files in a tempdir.  Asserts:
//!   - exactly 2 `g-allow-orphaned` findings (for the un-annotated and γ-style
//!     cites, both with a seeded `done` task), BOTH `Severity::High`
//!     (hard gate — the flip from the former advisory `Severity::Medium`);
//!   - ZERO `g-allow-orphaned` for the DONE-annotated cite (broadened grammar
//!     exemption, rule (a) `(…DONE…)` form);
//!   - ZERO findings for the pending cite (non-terminal acceptance); and
//!   - exactly 1 `g-allow-unknown-id` finding at `Severity::Medium` (DB-absent
//!     race is fail-soft; the remap removal must not promote unknown-id to High).
//! - **Test B** (live anti-drift, `#[ignore]`): runs `ptodo::check()` over the
//!   real repo against the real `tasks.db`; graceful-skip when git or the DB is
//!   absent.  Asserts ZERO `g-allow-orphaned` — the "PASSES on the now-clean
//!   tree" guard that is the precondition for the hard gate landing safely.
//!
//! RED until `check()` removes the `f.severity = Severity::Medium` remap (step-3).

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
/// Five `.rs` fixtures are written to a tempdir (NOT under crates/reify-audit/,
/// so not allowlisted):
///
/// - `tots_done.rs`   — `// G-allow: helper, task #3870 (κ — TOTS SQP, DONE)`
///   DONE token in following paren → exempt by broadened rule (a)
///   → PROVENANCE EXEMPTION: ZERO findings for #3870
///
/// - `unannotated.rs` — `// G-allow: task #1234 const-slice registry; consumer same-file`
///   no exemption annotation, task seeded done → TERMINAL-CITE REJECTION
///   → one g-allow-orphaned (Severity::High)
///
/// - `gamma.rs`       — `// G-allow: task #5678 (γ) fn-pointer blind spot`
///   γ paren has no done/cancelled token, task seeded done → TERMINAL-CITE REJECTION
///   → one g-allow-orphaned (Severity::High)
///
/// - `live.rs`        — `// G-allow: task #4321 live owner`
///   task seeded pending → NON-TERMINAL ACCEPTANCE: ZERO findings for #4321
///
/// - `unknown.rs`     — `// G-allow: task #9999 unknown owner`
///   #9999 NOT in the seeded DB → DB-sync race path
///   → one g-allow-unknown-id (Severity::Medium — fail-soft, never bumps High)
///   Verifies the remap removal did NOT accidentally promote unknown-id to High.
///
/// Assertions (HARD-GATE semantics — flipped from former advisory Medium):
///   - exactly 2 g-allow-orphaned findings
///   - BOTH Severity::High
///   - none name #3870 or #4321
///   - the two name #1234 and #5678
///   - exactly 1 g-allow-unknown-id finding, Severity::Medium
#[test]
fn g_allow_repo_wide_hard_gate_hermetic() {
    // Guard: if REIFY_PTODO_TASKS_DB is set to a non-empty path, check() will
    // read that DB instead of the one we seed at root/.taskmaster/tasks/tasks.db,
    // which would cause the liveness assertions to fail spuriously.
    // The env var is documented as subprocess-only (see ptodo.rs tasks_db_path).
    if std::env::var_os("REIFY_PTODO_TASKS_DB")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        eprintln!(
            "g_allow_repo_wide_hard_gate_hermetic: skipping — \
             REIFY_PTODO_TASKS_DB is set; test requires default DB resolution"
        );
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Fixture 1: DONE-annotated cite — exempt by broadened grammar rule (a).
    write_file(
        root,
        "tots_done.rs",
        "// G-allow: helper, task #3870 (\u{03ba} \u{2014} TOTS SQP, DONE)\n",
    );
    // Fixture 2: plain owner cite, done task — no exemption annotation.
    write_file(
        root,
        "unannotated.rs",
        "// G-allow: task #1234 const-slice registry; consumer same-file\n",
    );
    // Fixture 3: γ-style following paren with no done/cancelled token, done task.
    write_file(
        root,
        "gamma.rs",
        "// G-allow: task #5678 (\u{03b3}) fn-pointer blind spot\n",
    );
    // Fixture 4: plain owner cite, PENDING task — non-terminal acceptance.
    write_file(
        root,
        "live.rs",
        "// G-allow: task #4321 live owner\n",
    );
    // Fixture 5: plain owner cite, task ABSENT from DB — DB-sync race (unknown-id).
    // Verifies the remap removal did NOT promote g-allow-unknown-id to High;
    // it must remain Severity::Medium (fail-soft).
    write_file(
        root,
        "unknown.rs",
        "// G-allow: task #9999 unknown owner\n",
    );

    // Seed the DB at the default resolved path (no env override needed).
    // #9999 is intentionally NOT seeded — triggers the unknown-id path.
    seed_tasks_db_at(
        &root.join(".taskmaster/tasks/tasks.db"),
        &[
            ("master", 3870, "done"),
            ("master", 1234, "done"),
            ("master", 5678, "done"),
            ("master", 4321, "pending"),
        ],
    );

    let mut git = MockGitOps::new();
    git.set_ls_files(vec![
        "tots_done.rs".to_string(),
        "unannotated.rs".to_string(),
        "gamma.rs".to_string(),
        "live.rs".to_string(),
        "unknown.rs".to_string(),
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

    // Both must be Severity::High — the hard gate is never exit-neutral.
    // RED until step-3 removes `f.severity = Severity::Medium` from check().
    for f in &orphaned {
        assert_eq!(
            f.severity,
            Severity::High,
            "hard-gate g-allow-orphaned must be Severity::High; got {f:?}"
        );
    }

    // tots_done.rs #3870 is DONE-annotated — zero findings for it (provenance exemption).
    let tots_orphaned = orphaned.iter().any(|f| f.summary.contains("#3870"));
    assert!(
        !tots_orphaned,
        "tots_done.rs #3870 is DONE-annotated and must not appear in orphaned; \
         findings={findings:?}"
    );

    // live.rs #4321 is a pending task — zero findings for it (non-terminal acceptance).
    let live_orphaned = orphaned.iter().any(|f| f.summary.contains("#4321"));
    assert!(
        !live_orphaned,
        "live.rs #4321 is a pending owner and must not appear in orphaned; \
         findings={findings:?}"
    );

    // Each orphaned finding names its id.
    let has_1234 = orphaned.iter().any(|f| f.summary.contains("#1234"));
    let has_5678 = orphaned.iter().any(|f| f.summary.contains("#5678"));
    assert!(has_1234, "expected g-allow-orphaned for #1234; findings={findings:?}");
    assert!(has_5678, "expected g-allow-orphaned for #5678; findings={findings:?}");

    // Fixture 5 verification: unknown-id stays Severity::Medium (fail-soft).
    // The remap removal must NOT accidentally promote g-allow-unknown-id to High.
    let unknown_id_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.summary.starts_with("g-allow-unknown-id:"))
        .collect();
    assert_eq!(
        unknown_id_findings.len(),
        1,
        "expected exactly 1 g-allow-unknown-id finding for #9999; got {findings:?}"
    );
    assert_eq!(
        unknown_id_findings[0].severity,
        Severity::Medium,
        "g-allow-unknown-id must remain Severity::Medium (DB-sync race, fail-soft); \
         remap removal must not promote it; got {findings:?}"
    );
    // Confirm the unknown-id finding names #9999.
    assert!(
        unknown_id_findings[0].summary.contains("#9999"),
        "g-allow-unknown-id finding must name #9999; got {:?}",
        unknown_id_findings[0]
    );
}

/// Test B: live anti-drift guard.
///
/// Runs `ptodo::check()` over the real repo against the real `tasks.db`
/// (via `tasks_db_path()`).  Graceful-skip when git or the DB is absent
/// (mirrors the PTODO §6.7 degradation contract; worktree tasks don't carry a
/// local tasks.db — the live guard fires in the `/audit` sweep where the DB is
/// present in the main checkout).
///
/// The invariant: ZERO `g-allow-orphaned` findings in the live repo. This is the
/// "PASSES on the now-clean tree" precondition that makes the hard gate safe to
/// land.  (The cleanup tasks #4776–#4783 re-homed/exempted the ~50 orphaned
/// markers before this task landed.)  The count is printed for visibility.
#[ignore = "on-demand live anti-drift guard; run via --ignored or /audit sweep. \
    Graceful-skip when git or tasks.db is absent."]
#[test]
fn g_allow_repo_wide_hard_gate_live() {
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
        eprintln!("g_allow_repo_wide_hard_gate_live: skipping — git not available");
        return;
    }

    // Graceful-skip if this does not look like a real repo.
    if !ws_root.join(".git").exists() && !ws_root.join(".git").is_file() {
        eprintln!("g_allow_repo_wide_hard_gate_live: skipping — not a git repo");
        return;
    }

    // Graceful-skip if tasks.db is absent (task worktrees don't have one).
    let db_path = reify_audit::ptodo::tasks_db_path(&ws_root);
    if !db_path.exists() {
        eprintln!(
            "g_allow_repo_wide_hard_gate_live: skipping — tasks.db not found at {db_path:?}"
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

    // Print count for visibility.
    eprintln!(
        "g_allow_repo_wide_hard_gate_live: {} g-allow-orphaned finding(s) in live repo",
        orphaned.len()
    );

    // The hard-gate invariant: ZERO orphaned cites in the live tree.
    // If this fires, a `// G-allow:` owner cite in a swept .rs file (outside
    // crates/reify-audit/) points to a done/cancelled task. Either re-point the
    // marker to a live owner task, or annotate with `(…DONE…)` / `(…CANCELLED…)`
    // to exempt it as provenance. See: PTODO §6.7, task η #4559.
    assert!(
        orphaned.is_empty(),
        "ZERO g-allow-orphaned expected on the clean tree (hard gate). \
         {} orphaned finding(s) found:\n{}",
        orphaned.len(),
        orphaned
            .iter()
            .map(|f| format!("  {}: {}", f.task_id, f.summary))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
