//! Integration tests for the PLINECITE hard line-cite detector
//! (`plinecite::check`).
//!
//! - **Hermetic fixture trees** — a tempdir as `project_root` and a
//!   `MockGitOps::set_ls_files` naming what is tracked; `check()` reads real
//!   content from disk. Every verdict is pinned here, never against the real
//!   tree.
//! - **CLI** — the real binary over a staged fixture repo, proving the lane is
//!   reachable by `--pattern PLINECITE` and exit-neutral (report-only).
//! - **Real tree** — a well-formedness smoke over `check()`. No count is
//!   pinned: the residual is debt meant to shrink, so a count is a time bomb
//!   on every crate's comments.

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, RealGitOps,
    Severity,
};
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

struct Harness {
    conn: Connection,
    git: MockGitOps,
    jc: MockJCodemunchOps,
}

impl Harness {
    fn new(tracked: &[&str]) -> Self {
        let mut git = MockGitOps::new();
        git.set_ls_files(tracked.iter().map(|p| p.to_string()).collect());
        Self {
            conn: Connection::open_in_memory().expect("in-memory sqlite"),
            git,
            jc: MockJCodemunchOps::new(),
        }
    }

    fn ctx(&self, root: &Path) -> AuditContext<'_> {
        AuditContext {
            project_root: root.to_path_buf(),
            conn: &self.conn,
            git: &self.git,
            jcodemunch: &self.jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        }
    }
}

/// Write `files` under a fresh tempdir, track `tracked` of them, run `check()`.
fn check_fixture(files: &[(&str, &str)], tracked: &[&str]) -> Vec<Finding> {
    let dir = tempfile::tempdir().expect("tempdir");
    for (path, content) in files {
        write_file(dir.path(), path, content);
    }
    reify_audit::plinecite::check(&Harness::new(tracked).ctx(dir.path()))
}

/// The committed ledger's path: the handle of every finding about the tree
/// as a whole rather than about one citing line.
const LEDGER: &str = "crates/reify-audit/plinecite-baseline.txt";

const STDLIB: &str = "crates/reify-compiler/stdlib/a.ri";
const OTHER_STDLIB: &str = "crates/reify-compiler/stdlib/b.ri";
const NESTED_STDLIB: &str = "crates/reify-compiler/stdlib/sub/x.ri";
/// A tracked in-scope file with no cite, so a fixture asserting "nothing
/// reported" is not instead reporting an empty scope.
const CLEAN_STDLIB: &str = "crates/reify-compiler/stdlib/clean.ri";
const CLEAN: &str = "// no line cite here\nstructure Clean {}\n";

const CITING: &str = "structure S {}\n// header\n// see cache.rs:617-621 for the rule\n";

fn category(f: &Finding) -> &str {
    f.summary.split_once(':').map(|(c, _)| c).unwrap_or("")
}

/// The handle between `<category>: ` and the ` — ` that opens the detail. An
/// esc-anchored cite holds a space, so the first whitespace token is not it.
fn name(f: &Finding) -> &str {
    let rest = f.summary.split_once(": ").map(|(_, r)| r).unwrap_or("");
    rest.split_once(" — ").map(|(n, _)| n).unwrap_or(rest)
}

fn names_in(findings: &[Finding], wanted: &str) -> Vec<String> {
    findings
        .iter()
        .filter(|f| category(f) == wanted)
        .map(|f| name(f).to_string())
        .collect()
}

fn file_line(path: &str, line: usize, symbol: Option<&str>) -> EvidenceRef {
    EvidenceRef::FileLine {
        path: path.to_string(),
        line,
        symbol: symbol.map(str::to_string),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v1 — the stdlib pilot
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_comment_cite_in_a_tracked_stdlib_file_is_one_medium_line_cite() {
    let findings = check_fixture(&[(STDLIB, CITING)], &[STDLIB]);
    assert_eq!(findings.len(), 1, "exactly one finding; got {findings:#?}");
    let f = &findings[0];
    assert_eq!(f.pattern, Pattern::PLineCite);
    assert_eq!(f.severity, Severity::Medium, "PLINECITE is report-only");
    assert_eq!(f.task_id, STDLIB);
    assert!(
        f.summary.starts_with("line-cite: cache.rs:617 — "),
        "summary carries the category prefix and the cite: {}",
        f.summary
    );
    assert!(
        f.summary.contains(&format!("{STDLIB}:3")),
        "summary names the citing line: {}",
        f.summary
    );
    assert_eq!(
        f.evidence,
        vec![file_line(STDLIB, 3, Some("cache.rs:617"))],
        "the cite is the structured handle, not summary prose"
    );
}

#[test]
fn a_nested_stdlib_file_is_in_scope() {
    let findings = check_fixture(&[(NESTED_STDLIB, CITING)], &[NESTED_STDLIB]);
    assert_eq!(names_in(&findings, "line-cite"), vec!["cache.rs:617"]);
    assert_eq!(findings[0].task_id, NESTED_STDLIB);
}

#[test]
fn cites_on_code_lines_and_in_string_literals_are_not_reported() {
    let code = "param p = 1 // cache.rs:12\nassert(ok, \"see cache.rs:13\")\n";
    let findings = check_fixture(&[(STDLIB, code)], &[STDLIB]);
    assert!(
        findings.is_empty(),
        "only comment-leading lines are in grammar; got {findings:#?}"
    );
}

#[test]
fn untracked_and_out_of_scope_files_are_not_scanned() {
    let out_of_scope = [
        "docs/notes/n.md",
        "examples/a.ri",
        "tests/prd-gate/fixtures/f.ri",
        "crates/reify-eval/tests/t.rs",
    ];
    let mut files: Vec<(&str, &str)> = out_of_scope.iter().map(|p| (*p, CITING)).collect();
    files.push((STDLIB, CITING));
    files.push((CLEAN_STDLIB, CLEAN));
    let mut tracked = out_of_scope.to_vec();
    tracked.push(CLEAN_STDLIB);

    let findings = check_fixture(&files, &tracked);
    assert!(
        findings.is_empty(),
        "an untracked stdlib file and tracked out-of-scope files are never \
         scanned; got {findings:#?}"
    );
}

#[test]
fn a_repeated_cite_is_one_finding_at_its_first_line() {
    let content = "// see a.rs:1\n// again a.rs:1\n// and b.rs:2\n";
    let findings = check_fixture(
        &[(STDLIB, content), (OTHER_STDLIB, "// see a.rs:1\n")],
        &[STDLIB, OTHER_STDLIB],
    );
    let evidence: Vec<&EvidenceRef> = findings.iter().flat_map(|f| &f.evidence).collect();
    assert_eq!(
        evidence,
        vec![
            &file_line(STDLIB, 1, Some("a.rs:1")),
            &file_line(OTHER_STDLIB, 1, Some("a.rs:1")),
            &file_line(STDLIB, 3, Some("b.rs:2")),
        ],
        "one finding per (file, cite), at its first line; got {findings:#?}"
    );
}

#[test]
fn a_reasoned_allow_marker_exempts_its_line() {
    let content = "// see a.rs:1 plinecite:allow — dated evidence\n";
    let findings = check_fixture(&[(STDLIB, content)], &[STDLIB]);
    assert!(findings.is_empty(), "got {findings:#?}");
}

#[test]
fn a_reasonless_allow_marker_is_reported_and_exempts_nothing() {
    let content = "// first line\n// see a.rs:2 plinecite:allow\n";
    let findings = check_fixture(&[(STDLIB, content)], &[STDLIB]);
    let missing: Vec<&Finding> = findings
        .iter()
        .filter(|f| category(f) == "allow-missing-reason")
        .collect();
    assert_eq!(missing.len(), 1, "got {findings:#?}");
    assert_eq!(name(missing[0]), format!("{STDLIB}:2"));
    assert_eq!(missing[0].evidence, vec![file_line(STDLIB, 2, None)]);
    assert_eq!(
        names_in(&findings, "line-cite"),
        vec!["a.rs:2"],
        "a reasonless marker does not exempt the cite on its line"
    );
}

#[test]
fn an_empty_scope_is_one_scope_empty_finding_never_a_clean_run() {
    for tracked in [vec!["docs/notes/n.md"], vec![]] {
        let findings = check_fixture(&[("docs/notes/n.md", CITING)], &tracked);
        assert_eq!(findings.len(), 1, "tracked {tracked:?}; got {findings:#?}");
        let f = &findings[0];
        assert_eq!(category(f), "scope-empty", "{f:#?}");
        assert_eq!(f.pattern, Pattern::PLineCite);
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.task_id, LEDGER);
        assert_eq!(
            f.evidence,
            vec![EvidenceRef::File {
                path: LEDGER.to_string()
            }]
        );
    }
}

#[test]
fn findings_are_sorted_by_category_name_path_and_deterministic() {
    let content = "// see z.rs:9\n// see a.rs:1 plinecite:allow\n// see m.ri:5\n";
    let files = [(STDLIB, content), (OTHER_STDLIB, content)];
    let tracked = [STDLIB, OTHER_STDLIB];
    let first = check_fixture(&files, &tracked);
    let second = check_fixture(&files, &tracked);
    assert_eq!(first, second, "two runs over one tree agree");

    let keys: Vec<(&str, &str, &str)> = first
        .iter()
        .map(|f| (category(f), name(f), f.task_id.as_str()))
        .collect();
    assert!(
        keys.windows(2).all(|pair| pair[0] < pair[1]),
        "findings are strictly ordered by (category, name, path): {keys:#?}"
    );
    assert_eq!(
        keys.len(),
        8,
        "two files x (three cites + one reasonless marker); got {keys:#?}"
    );
    assert!(
        first.iter().all(|f| f.severity == Severity::Medium),
        "every PLINECITE finding is Medium"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI — reachable by `--pattern PLINECITE`, and exit-neutral
// ─────────────────────────────────────────────────────────────────────────────

fn git_in(root: &Path, args: &[&str]) {
    let out = reify_audit::git_env::command(root)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn pattern_plinecite_reports_a_line_cite_and_exits_zero() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("plinecite: skipping the CLI test — git not available");
        return;
    }
    let repo = tempfile::tempdir().expect("repo tempdir");
    let aux = tempfile::tempdir().expect("aux tempdir");
    git_in(repo.path(), &["init", "-q"]);
    write_file(repo.path(), STDLIB, CITING);
    git_in(repo.path(), &["add", "-A"]);

    let tasks_file = aux.path().join("tasks.json");
    std::fs::write(&tasks_file, "[]").expect("write tasks.json");
    let runs_db = aux.path().join("runs.db");
    Connection::open(&runs_db)
        .expect("open runs.db")
        .execute_batch("CREATE TABLE events (task_id TEXT, event_type TEXT);")
        .expect("create events table");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify-audit"));
    cmd.args(["--pattern", "PLINECITE", "--no-jcodemunch", "--project-root"])
        .arg(repo.path())
        .arg("--tasks-file")
        .arg(&tasks_file)
        .arg("--runs-db")
        .arg(&runs_db);
    reify_audit::git_env::sanitize(&mut cmd);
    let out = cmd.output().expect("invoke reify-audit --pattern PLINECITE");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(0),
        "PLINECITE is Medium-only, so it never moves the exit code; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("\"PLineCite\""),
        "the dispatch arm must route to the PLINECITE lane; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("line-cite: cache.rs:617"),
        "the seeded cite must be reported; stderr:\n{stderr}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Real tree
// ─────────────────────────────────────────────────────────────────────────────

/// `check()` over the real tree: every finding is well-formed and the output
/// is deterministic. No count is pinned.
///
/// Determinism is proved from ONE run: strictly increasing
/// `(category, name, path)` keys make the order total, so it cannot depend on
/// hash iteration. Two-run equality is pinned hermetically.
#[test]
fn real_repo_plinecite_smoke() {
    let root = repo_root();
    let git = RealGitOps::new(root.clone());
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: root,
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };
    let findings = reify_audit::plinecite::check(&ctx);
    let keys: Vec<(&str, &str, &str)> = findings
        .iter()
        .map(|f| (category(f), name(f), f.task_id.as_str()))
        .collect();
    assert!(
        keys.windows(2).all(|pair| pair[0] < pair[1]),
        "findings must be strictly ordered by (category, name, path): {keys:#?}"
    );

    let known: BTreeSet<&str> = ["line-cite", "allow-missing-reason"].into();
    for f in &findings {
        assert_eq!(f.pattern, Pattern::PLineCite, "{f:#?}");
        assert_eq!(f.severity, Severity::Medium, "{f:#?}");
        assert!(
            known.contains(category(f)),
            "unknown category (a scope-empty here means the real tree's \
             in-scope corpus was not read): {f:#?}"
        );
        let [EvidenceRef::FileLine { path, line, symbol }] = f.evidence.as_slice() else {
            panic!("exactly one FileLine evidence expected: {f:#?}");
        };
        assert!(
            reify_audit::plinecite::in_scope(path) && *line >= 1,
            "evidence must be an in-scope line: {f:#?}"
        );
        assert_eq!(f.task_id, *path, "{f:#?}");
        let handle = symbol.clone().unwrap_or_else(|| format!("{path}:{line}"));
        assert_eq!(
            name(f),
            handle,
            "the summary names the handle the evidence carries: {f:#?}"
        );
        assert_eq!(
            symbol.is_some(),
            category(f) == "line-cite",
            "only a line cite names a cite: {f:#?}"
        );
    }
}
