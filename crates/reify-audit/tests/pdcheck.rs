//! Integration tests for the PDCHECK `delivered_checks` dead-path lane.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test pdcheck`
//!
//! The lane is task-state-shaped, so these drive
//! `pdcheck::resolve_delivered_check_paths` against an in-memory `tasks` table
//! (raw JSON TEXT metadata, exactly as the live `.taskmaster/tasks/tasks.db`
//! stores it) plus a `MockGitOps` supplying the tracked-file set and the
//! commit/rename history. No real git repo and no real DB — the same hermetic
//! shape as `tests/ptodo.rs`'s `mod inverse`.

mod common;

mod pdcheck {

use common::schema::{insert_task, insert_task_with_metadata, seed_tasks_db};
use reify_audit::{
    AuditContext, EvidenceRef, Finding, GitCommit, GitOps, MockGitOps, MockJCodemunchOps, Pattern,
    Severity,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common;

/// The live #5778 shape: `crates/reify-eval/src/arg_acceptance.rs` was
/// relocated to `crates/reify-ir/...` by #5791, leaving two `expect: present`
/// rows permanently unsatisfiable.
const DEAD: &str = "crates/reify-eval/src/arg_acceptance.rs";
const LIVE: &str = "crates/reify-ir/src/arg_acceptance.rs";

fn tracked(paths: &[&str]) -> HashSet<String> {
    paths.iter().map(|s| s.to_string()).collect()
}

fn mock_commit(sha: &str, subject: &str) -> GitCommit {
    GitCommit { sha: sha.to_string(), subject: subject.to_string() }
}

/// A `delivered_checks` metadata blob carrying one grep row.
fn one_grep_row(name: &str, expect: &str, paths: &[&str]) -> String {
    let paths: Vec<String> = paths.iter().map(|p| format!("{p:?}")).collect();
    format!(
        r#"{{"delivered_checks":[{{"name":"{name}","kind":"grep","expect":"{expect}","pattern":"pub fn angle_spec","paths":[{}]}}]}}"#,
        paths.join(",")
    )
}

fn resolve(
    conn: &rusqlite::Connection,
    git: &dyn GitOps,
    tracked: &HashSet<String>,
) -> Vec<Finding> {
    resolve_for(conn, git, tracked, None)
}

/// The narrowed form: `target_task_id` is what `--task <id>` supplies.
fn resolve_for(
    conn: &rusqlite::Connection,
    git: &dyn GitOps,
    tracked: &HashSet<String>,
    target_task_id: Option<&str>,
) -> Vec<Finding> {
    reify_audit::pdcheck::resolve_delivered_check_paths(conn, git, tracked, target_task_id)
        .expect("resolve_delivered_check_paths")
}

fn check_names(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| {
            f.evidence
                .iter()
                .find_map(|e| match e {
                    EvidenceRef::DeliveredCheck { check_name, .. } => Some(check_name.clone()),
                    _ => None,
                })
                .expect("every finding carries a DeliveredCheck evidence ref")
        })
        .collect()
}

/// A `GitOps` that counts `last_commit_for_path` calls and delegates
/// everything else to an inner [`MockGitOps`].
///
/// Exists solely for the memoization scenario: `MockGitOps` answers from a
/// `HashMap` and so cannot distinguish one call from two. Delegation is
/// spelled out rather than stubbed with `unimplemented!()`, which would be an
/// untracked placeholder under this repo's PTODO grammar.
struct CountingGitOps {
    inner: MockGitOps,
    last_commit_calls: AtomicUsize,
}

impl CountingGitOps {
    fn new(inner: MockGitOps) -> Self {
        Self { inner, last_commit_calls: AtomicUsize::new(0) }
    }

    fn last_commit_calls(&self) -> usize {
        self.last_commit_calls.load(Ordering::SeqCst)
    }
}

impl GitOps for CountingGitOps {
    fn last_commit_for_path(&self, path: &str) -> Option<GitCommit> {
        self.last_commit_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.last_commit_for_path(path)
    }

    fn try_log_grep(&self, branch: &str, pattern: &str) -> Result<Vec<GitCommit>, String> {
        self.inner.try_log_grep(branch, pattern)
    }
    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String> {
        self.inner.diff_changed_paths(from, to)
    }
    fn changed_paths_in_commit(&self, commit: &str) -> Vec<String> {
        self.inner.changed_paths_in_commit(commit)
    }
    fn try_is_gitignored(&self, path: &str) -> Result<bool, String> {
        self.inner.try_is_gitignored(path)
    }
    fn try_path_tracked_on(&self, branch: &str, path: &str) -> Result<bool, String> {
        self.inner.try_path_tracked_on(branch, path)
    }
    fn diff_added_lines(&self, from: &str, to: &str, path: &str) -> Vec<(usize, String)> {
        self.inner.diff_added_lines(from, to, path)
    }
    fn diff_added_lines_in_commit(&self, commit: &str, path: &str) -> Vec<(usize, String)> {
        self.inner.diff_added_lines_in_commit(commit, path)
    }
    fn file_lines_on(&self, reference: &str, path: &str) -> Vec<(usize, String)> {
        self.inner.file_lines_on(reference, path)
    }
    fn is_ancestor(&self, commit: &str, branch: &str) -> bool {
        self.inner.is_ancestor(commit, branch)
    }
    fn ls_files(&self) -> Vec<String> {
        self.inner.ls_files()
    }
    fn rename_target_for_path(&self, path: &str, sha: &str) -> Option<String> {
        self.inner.rename_target_for_path(path, sha)
    }
}

// -------------------------------------------------------------------------
// Scenario 1 — terminal tasks are skipped
// -------------------------------------------------------------------------

/// Only a NON-terminal task can still block a dependent by landing, so a
/// `done` or `cancelled` task's stale row is inert history, not a defect.
#[test]
fn terminal_tasks_are_skipped() {
    let conn = seed_tasks_db();
    for (id, status) in [(10, "done"), (11, "cancelled")] {
        insert_task_with_metadata(
            &conn,
            "master",
            id,
            status,
            &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
        );
    }

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));
    assert!(findings.is_empty(), "terminal tasks must yield nothing; got {findings:?}");
}

// -------------------------------------------------------------------------
// Scenario 2 — the live #5778 shape, with a repointable rename target
// -------------------------------------------------------------------------

#[test]
fn unsatisfiable_row_with_rename_target_emits_high_finding() {
    let conn = seed_tasks_db();
    insert_task_with_metadata(
        &conn,
        "master",
        5778,
        "deferred",
        &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
    );

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance to reify-ir"));
    git.set_rename_target_for_path(DEAD, "abc123", LIVE);

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));

    assert_eq!(findings.len(), 1, "expected exactly one finding; got {findings:?}");
    let f = &findings[0];
    assert_eq!(f.pattern, Pattern::PDeliveredCheckPath, "pattern: {f:?}");
    assert_eq!(
        f.severity,
        Severity::High,
        "an unsatisfiable expect=present row blocks every dependent forever: {f:?}"
    );
    assert_eq!(f.task_id, "5778", "task_id: {f:?}");
    assert!(
        f.summary.starts_with("delivered-check-unsatisfiable-path:"),
        "summary must lead with the kind prefix: {}",
        f.summary
    );
    assert!(f.summary.contains("5778"), "summary must name the task: {}", f.summary);
    assert!(
        f.summary.contains("angle-spec-absent-today"),
        "summary must name the check row: {}",
        f.summary
    );
    assert!(f.summary.contains(DEAD), "summary must name the dead path: {}", f.summary);

    assert!(
        f.evidence.iter().any(|e| matches!(
            e,
            EvidenceRef::DeliveredCheck { check_name, paths }
                if check_name == "angle-spec-absent-today" && paths == &vec![DEAD.to_string()]
        )),
        "evidence must carry the row's name and paths: {:?}",
        f.evidence
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == LIVE)),
        "evidence must carry the rename target as the repair hint: {:?}",
        f.evidence
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::Commit { sha, .. } if sha == "abc123")),
        "evidence must carry the last-touching commit: {:?}",
        f.evidence
    );
}

// -------------------------------------------------------------------------
// Scenario 3 — deleted, not renamed: same kind, no repair hint
// -------------------------------------------------------------------------

/// The kind does NOT split on rename-vs-delete: that axis changes only the
/// repair hint, which is evidence. What is wrong is identical either way.
#[test]
fn deleted_path_yields_the_same_kind_without_a_rename_hint() {
    let conn = seed_tasks_db();
    insert_task_with_metadata(
        &conn,
        "master",
        5778,
        "pending",
        &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
    );

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("dead99", "drop arg_acceptance"));
    // No rename target set → a genuine delete.

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));

    assert_eq!(findings.len(), 1, "expected exactly one finding; got {findings:?}");
    let f = &findings[0];
    assert_eq!(f.pattern, Pattern::PDeliveredCheckPath);
    assert_eq!(f.severity, Severity::High);
    assert!(
        f.summary.starts_with("delivered-check-unsatisfiable-path:"),
        "kind must not split on rename-vs-delete: {}",
        f.summary
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::Commit { sha, .. } if sha == "dead99")),
        "evidence must carry the commit: {:?}",
        f.evidence
    );
    assert!(
        !f.evidence.iter().any(|e| matches!(e, EvidenceRef::File { .. })),
        "a deleted path has no rename target to advertise: {:?}",
        f.evidence
    );
}

// -------------------------------------------------------------------------
// Scenario 4 — a rename target that is itself absent is never advertised
// -------------------------------------------------------------------------

/// Mirrors `resolve_inverse`'s `.filter(path_present_in_tracked)`: never point
/// the reader at a path they cannot open.
#[test]
fn rename_target_absent_from_tracked_is_not_advertised() {
    let conn = seed_tasks_db();
    insert_task_with_metadata(
        &conn,
        "master",
        5778,
        "pending",
        &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
    );

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate then delete"));
    git.set_rename_target_for_path(DEAD, "abc123", "crates/reify-ir/src/gone_again.rs");

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));

    assert_eq!(findings.len(), 1, "expected exactly one finding; got {findings:?}");
    let f = &findings[0];
    assert!(
        !f.evidence.iter().any(|e| matches!(e, EvidenceRef::File { .. })),
        "a rename target absent from the tracked set must not be advertised: {:?}",
        f.evidence
    );
}

// -------------------------------------------------------------------------
// Scenario 5 — never-existed paths pass
// -------------------------------------------------------------------------

/// A delivered_check may legitimately name a file the task is about to
/// CREATE. That is the healthy post-state shape, and it is the majority
/// shape — flagging it would make the lane useless.
#[test]
fn never_existed_path_yields_no_finding() {
    let conn = seed_tasks_db();
    insert_task_with_metadata(
        &conn,
        "master",
        5778,
        "pending",
        &one_grep_row("angle-spec-lands-here", "present", &["crates/reify-ir/src/to_be_born.rs"]),
    );

    // No git history for the path at all.
    let git = MockGitOps::new();

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));
    assert!(
        findings.is_empty(),
        "a path with no git history is presumed to-be-created; got {findings:?}"
    );
}

// -------------------------------------------------------------------------
// Scenario 6 — the vacuous half
// -------------------------------------------------------------------------

/// The invisible defect: rc=1 on a dead pathspec reads as PASSED under
/// `expect: absent`, so the check succeeds while asserting nothing.
#[test]
fn vacuous_absent_row_emits_medium_finding() {
    let conn = seed_tasks_db();
    insert_task_with_metadata(
        &conn,
        "master",
        5790,
        "pending",
        &one_grep_row("bare-angle-resolver-retired", "absent", &[DEAD]),
    );

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));

    assert_eq!(findings.len(), 1, "expected exactly one finding; got {findings:?}");
    let f = &findings[0];
    assert_eq!(f.pattern, Pattern::PDeliveredCheckPath);
    assert_eq!(
        f.severity,
        Severity::Medium,
        "a vacuous check is a silent hole, not an outage: {f:?}"
    );
    assert!(
        f.summary.starts_with("delivered-check-vacuous-absent-path:"),
        "summary must lead with the vacuous kind prefix: {}",
        f.summary
    );
    assert!(
        f.summary.contains("bare-angle-resolver-retired"),
        "summary must name the check row: {}",
        f.summary
    );
}

// -------------------------------------------------------------------------
// Scenario 7 — determinism
// -------------------------------------------------------------------------

/// Sorted by (task_id as INTEGER, check name). The integer parse is what keeps
/// #100 after #20 rather than lexicographically before it.
#[test]
fn findings_are_sorted_by_numeric_task_id_then_check_name() {
    let conn = seed_tasks_db();
    let two_rows = |a: &str, b: &str| {
        format!(
            r#"{{"delivered_checks":[
                {{"name":"{a}","kind":"grep","expect":"present","pattern":"x","paths":["{DEAD}"]}},
                {{"name":"{b}","kind":"grep","expect":"present","pattern":"x","paths":["{DEAD}"]}}
            ]}}"#
        )
    };
    // Inserted out of order, and each task's rows are out of order too.
    insert_task_with_metadata(&conn, "master", 100, "pending", &two_rows("z-row", "a-row"));
    insert_task_with_metadata(&conn, "master", 20, "pending", &two_rows("z-row", "a-row"));

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));

    let first = resolve(&conn, &git, &tracked(&[LIVE]));
    let ids: Vec<&str> = first.iter().map(|f| f.task_id.as_str()).collect();
    assert_eq!(ids, ["20", "20", "100", "100"], "task ids must sort numerically: {ids:?}");
    assert_eq!(
        check_names(&first),
        ["a-row", "z-row", "a-row", "z-row"],
        "rows must sort by check name within a task"
    );

    let second = resolve(&conn, &git, &tracked(&[LIVE]));
    assert_eq!(first, second, "the order must be stable across runs");
}

// -------------------------------------------------------------------------
// Single-task narrowing — what `--task <id>` means for a task-state lane
// -------------------------------------------------------------------------

/// `--task <id>` spot-checks ONE task. This lane iterates the tasks table, so
/// unlike the purely structural detectors it has a task to narrow to, and the
/// likeliest caller is someone unblocking a single stuck dependent.
#[test]
fn a_target_task_id_narrows_the_sweep_to_that_task() {
    let conn = seed_tasks_db();
    let row = |name: &str| one_grep_row(name, "present", &[DEAD]);
    insert_task_with_metadata(&conn, "master", 5778, "deferred", &row("angle-spec-absent-today"));
    insert_task_with_metadata(&conn, "master", 5743, "pending", &row("length-hint-absent-today"));

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));
    let tracked = tracked(&[LIVE]);

    assert_eq!(
        resolve(&conn, &git, &tracked).len(),
        2,
        "the unnarrowed sweep sees both seeded tasks"
    );

    let narrowed = resolve_for(&conn, &git, &tracked, Some("5778"));
    let ids: Vec<&str> = narrowed.iter().map(|f| f.task_id.as_str()).collect();
    assert_eq!(ids, ["5778"], "only the targeted task may be reported: {ids:?}");
}

/// The narrowing is exact, not a prefix or substring match: targeting a task
/// with findings of its own must not surface a neighbour's.
#[test]
fn a_target_task_id_matching_no_task_yields_nothing() {
    let conn = seed_tasks_db();
    insert_task_with_metadata(
        &conn,
        "master",
        5778,
        "deferred",
        &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
    );

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));

    assert!(
        resolve_for(&conn, &git, &tracked(&[LIVE]), Some("9999")).is_empty(),
        "a target id no task carries must report nothing"
    );
    assert!(
        resolve_for(&conn, &git, &tracked(&[LIVE]), Some("577")).is_empty(),
        "the id comparison is exact, not a prefix match"
    );
}

// -------------------------------------------------------------------------
// Scenario 8 — robustness against every malformed metadata shape
// -------------------------------------------------------------------------

#[test]
fn malformed_metadata_yields_no_findings_and_no_panic() {
    let conn = seed_tasks_db();
    // metadata NULL.
    insert_task(&conn, "master", 1, "pending");
    let malformed = [
        "not json at all",
        r#"{"files":["crates/x.rs"]}"#,
        r#"{"delivered_checks":"nope"}"#,
        r#"{"delivered_checks":{}}"#,
        r#"{"delivered_checks":null}"#,
        r#"{"delivered_checks":["nope",7,null,[]]}"#,
        "",
    ];
    for (offset, metadata) in malformed.iter().enumerate() {
        insert_task_with_metadata(&conn, "master", 2 + offset as i64, "pending", metadata);
    }

    let mut git = MockGitOps::new();
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));
    assert!(findings.is_empty(), "malformed metadata must be inert; got {findings:?}");
}

// -------------------------------------------------------------------------
// Scenario 9 — the git seam is memoized per run
// -------------------------------------------------------------------------

/// `resolve_inverse` caches for a MEASURED reason (2 renamed paths cited by 6
/// tasks), so assert the cache rather than trusting it: two tasks citing the
/// same dead path must cost ONE `last_commit_for_path` spawn, not two.
#[test]
fn the_git_seam_is_called_once_per_distinct_path() {
    let conn = seed_tasks_db();
    for id in [5778, 5779] {
        insert_task_with_metadata(
            &conn,
            "master",
            id,
            "pending",
            &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
        );
    }

    let mut inner = MockGitOps::new();
    inner.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));
    let git = CountingGitOps::new(inner);

    let findings = resolve(&conn, &git, &tracked(&[LIVE]));

    assert_eq!(findings.len(), 2, "both tasks must be flagged; got {findings:?}");
    assert_eq!(
        git.last_commit_calls(),
        1,
        "two tasks citing one dead path must cost one git spawn, not two"
    );
}

// -------------------------------------------------------------------------
// Scenario 10 — fail-soft degradation at the check() entry point
// -------------------------------------------------------------------------

/// With no `.taskmaster/tasks/tasks.db` under `project_root`, `check` must
/// return zero findings and not panic. The exit class is untouched by an
/// absent optional substrate; 125 is reserved for genuine arg/IO misconfig.
///
/// The accompanying stderr breadcrumb is a single `eprintln!` at the one
/// degradation site, and like PTODO's §6.7 breadcrumb it is not captured
/// in-process — this test pins the behavioural half only.
#[test]
fn check_degrades_to_zero_findings_when_tasks_db_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut git = MockGitOps::new();
    git.set_ls_files(vec![LIVE.to_string()]);
    // Git knows the dead path's history, so nothing but the absent DB can
    // explain an empty result.
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));

    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: dir.path().to_path_buf(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let findings = reify_audit::pdcheck::check(&ctx);
    assert!(
        findings.is_empty(),
        "an absent task DB must degrade fail-soft, not fabricate findings; got {findings:?}"
    );
}

/// The complement of the test above: with the DB present at the DEFAULT path
/// the lane runs, so an empty result there is a fact rather than a silent
/// degradation.
#[test]
fn check_resolves_the_lane_against_the_default_db_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::schema::seed_tasks_db_at_with_metadata(
        &dir.path().join(".taskmaster/tasks/tasks.db"),
        &[(
            "master",
            5778,
            "deferred",
            &one_grep_row("angle-spec-absent-today", "present", &[DEAD]),
        )],
    );

    let mut git = MockGitOps::new();
    git.set_ls_files(vec![LIVE.to_string()]);
    git.set_last_commit_for_path(DEAD, mock_commit("abc123", "relocate arg_acceptance"));
    git.set_rename_target_for_path(DEAD, "abc123", LIVE);

    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: dir.path().to_path_buf(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let findings = reify_audit::pdcheck::check(&ctx);
    assert_eq!(findings.len(), 1, "expected the seeded row to be flagged; got {findings:?}");
    assert_eq!(findings[0].severity, Severity::High);
    assert!(findings[0].summary.starts_with("delivered-check-unsatisfiable-path:"));
}

}
