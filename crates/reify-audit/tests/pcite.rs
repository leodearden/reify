//! Integration tests for the PCITE capability-manifest cite detector
//! (`pcite::check`).
//!
//! - **Hermetic fixture trees** — a tempdir as `project_root` and a
//!   `MockGitOps::set_ls_files` naming what is tracked; `check()` reads real
//!   content from disk. Every verdict is pinned here, never against the real
//!   tree.
//! - **CLI** — the real binary over a staged fixture repo, proving the lane is
//!   reachable by `--pattern PCITE` and exit-neutral (report-only).
//! - **Real corpus** — a floor guard on the cite grammar over the tracked
//!   manifests, and a well-formedness smoke over `check()`; neither pins the
//!   residual count, which is the lane's legitimate report.

use reify_audit::pcite::{MANIFEST_ROOT, MANIFEST_SUFFIX, cited_symbols};
use reify_audit::{
    AuditContext, EvidenceRef, Finding, GitOps, MockGitOps, MockJCodemunchOps, Pattern, RealGitOps,
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

    fn check(&self, root: &Path) -> Vec<Finding> {
        reify_audit::pcite::check(&AuditContext {
            project_root: root.to_path_buf(),
            conn: &self.conn,
            git: &self.git,
            jcodemunch: &self.jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        })
    }
}

/// Write `files` under a fresh tempdir, track `tracked` of them, run `check()`.
fn check_fixture(files: &[(&str, &str)], tracked: &[&str]) -> Vec<Finding> {
    let dir = tempfile::tempdir().expect("tempdir");
    for (path, content) in files {
        write_file(dir.path(), path, content);
    }
    Harness::new(tracked).check(dir.path())
}

const MANIFEST: &str = "docs/prds/x.capability-manifest.md";
const OTHER_MANIFEST: &str = "docs/prds/v0_6/y.capability-manifest.md";
const SOURCE: &str = "crates/a/src/lib.rs";
const GHOST_CITE: &str = "| wired | grep: `ghost_symbol` at `crates/a/src/lib.rs:3` | PASS |\n";

fn manifest(rows: &str) -> String {
    format!("# Capability manifest\n\n{rows}")
}

fn category(f: &Finding) -> &str {
    f.summary.split_once(':').map(|(c, _)| c).unwrap_or("")
}

fn name(f: &Finding) -> &str {
    let rest = f.summary.split_once(": ").map(|(_, r)| r).unwrap_or("");
    rest.split_whitespace().next().unwrap_or(rest)
}

fn names_in(findings: &[Finding], wanted: &str) -> Vec<String> {
    findings
        .iter()
        .filter(|f| category(f) == wanted)
        .map(|f| name(f).to_string())
        .collect()
}

#[test]
fn a_cite_no_tracked_source_contains_is_one_medium_fabricated_cite() {
    let findings = check_fixture(
        &[
            (MANIFEST, &manifest(GHOST_CITE)),
            (SOURCE, "pub fn other() {}\n"),
        ],
        &[MANIFEST, SOURCE],
    );
    assert_eq!(findings.len(), 1, "exactly one finding; got {findings:#?}");
    let f = &findings[0];
    assert_eq!(f.pattern, Pattern::PManifestCite);
    assert_eq!(f.severity, Severity::Medium, "PCITE is report-only");
    assert!(
        f.summary.starts_with("fabricated-cite: ghost_symbol — "),
        "summary carries the category prefix and the cited name: {}",
        f.summary
    );
    assert!(
        f.summary.contains(&format!("{MANIFEST}:3")),
        "summary cites the manifest line: {}",
        f.summary
    );
    assert_eq!(
        f.evidence,
        vec![EvidenceRef::File {
            path: MANIFEST.to_string()
        }]
    );
    assert_eq!(f.task_id, MANIFEST);
}

#[test]
fn a_tracked_source_containing_the_symbol_vouches_for_it() {
    let findings = check_fixture(
        &[
            (MANIFEST, &manifest(GHOST_CITE)),
            (SOURCE, "pub fn ghost_symbol() {}\n"),
        ],
        &[MANIFEST, SOURCE],
    );
    assert!(findings.is_empty(), "the cite resolves; got {findings:#?}");
}

#[test]
fn markdown_and_docs_never_vouch() {
    let findings = check_fixture(
        &[
            (MANIFEST, &manifest(GHOST_CITE)),
            (SOURCE, "pub fn other() {}\n"),
            ("docs/notes/n.md", "`ghost_symbol` is described here\n"),
            ("docs/notes/snippet.rs", "fn ghost_symbol() {}\n"),
            ("README.md", "ghost_symbol\n"),
            ("crates/a/DESIGN.md", "ghost_symbol\n"),
        ],
        &[
            MANIFEST,
            SOURCE,
            "docs/notes/n.md",
            "docs/notes/snippet.rs",
            "README.md",
            "crates/a/DESIGN.md",
        ],
    );
    assert_eq!(
        names_in(&findings, "fabricated-cite"),
        vec!["ghost_symbol"],
        "prose restating a cite is not evidence for it; got {findings:#?}"
    );
}

#[test]
fn only_tracked_manifests_are_scanned_and_only_tracked_sources_vouch() {
    let untracked_source = check_fixture(
        &[
            (MANIFEST, &manifest(GHOST_CITE)),
            (SOURCE, "pub fn ghost_symbol() {}\n"),
        ],
        &[MANIFEST],
    );
    assert_eq!(
        names_in(&untracked_source, "fabricated-cite"),
        vec!["ghost_symbol"],
        "an untracked source does not vouch"
    );

    let untracked_manifest = check_fixture(
        &[
            (MANIFEST, &manifest(GHOST_CITE)),
            (SOURCE, "pub fn other() {}\n"),
        ],
        &[SOURCE],
    );
    assert!(
        untracked_manifest.is_empty(),
        "an untracked manifest is not scanned; got {untracked_manifest:#?}"
    );

    let not_manifests = check_fixture(
        &[
            ("docs/prds/x.md", &manifest(GHOST_CITE)),
            ("docs/notes/z.capability-manifest.md", &manifest(GHOST_CITE)),
            (SOURCE, "pub fn other() {}\n"),
        ],
        &[
            "docs/prds/x.md",
            "docs/notes/z.capability-manifest.md",
            SOURCE,
        ],
    );
    assert!(
        not_manifests.is_empty(),
        "only docs/prds/**/*.capability-manifest.md is the corpus; got {not_manifests:#?}"
    );
}

#[test]
fn one_finding_per_manifest_and_name_at_its_first_line() {
    let rows = format!("{GHOST_CITE}| other | PASS |\n{GHOST_CITE}");
    let findings = check_fixture(
        &[
            (MANIFEST, &manifest(&rows)),
            (OTHER_MANIFEST, &manifest(GHOST_CITE)),
            (SOURCE, "pub fn other() {}\n"),
        ],
        &[MANIFEST, OTHER_MANIFEST, SOURCE],
    );
    assert_eq!(findings.len(), 2, "one per manifest; got {findings:#?}");
    let in_manifest: Vec<&Finding> = findings.iter().filter(|f| f.task_id == MANIFEST).collect();
    assert_eq!(in_manifest.len(), 1, "two cites, one manifest, one finding");
    assert!(
        in_manifest[0].summary.contains(&format!("{MANIFEST}:3"))
            && !in_manifest[0].summary.contains(&format!("{MANIFEST}:5")),
        "the finding cites the FIRST occurrence: {}",
        in_manifest[0].summary
    );
    assert!(findings.iter().any(|f| f.task_id == OTHER_MANIFEST));
}

#[test]
fn a_reasoned_allow_marker_suppresses_and_a_reasonless_one_is_itself_reported() {
    let reasoned = check_fixture(
        &[
            (
                MANIFEST,
                &manifest(
                    "| wired | grep: `ghost_symbol` | PASS | <!-- pcite:allow — dark-factory symbol -->\n",
                ),
            ),
            (SOURCE, "pub fn other() {}\n"),
        ],
        &[MANIFEST, SOURCE],
    );
    assert!(
        reasoned.is_empty(),
        "a reasoned marker exempts its line; got {reasoned:#?}"
    );

    let reasonless = check_fixture(
        &[
            (
                MANIFEST,
                &manifest("| wired | grep: `ghost_symbol` | PASS | <!-- pcite:allow -->\n"),
            ),
            (SOURCE, "pub fn other() {}\n"),
        ],
        &[MANIFEST, SOURCE],
    );
    assert_eq!(
        names_in(&reasonless, "allow-missing-reason"),
        vec![format!("{MANIFEST}:3")],
        "a reasonless marker is one finding keyed by its line; got {reasonless:#?}"
    );
    assert_eq!(
        names_in(&reasonless, "fabricated-cite"),
        vec!["ghost_symbol"],
        "a reasonless marker confers no exemption"
    );
    assert!(
        reasonless
            .iter()
            .all(|f| f.severity == Severity::Medium && f.pattern == Pattern::PManifestCite),
        "every PCITE finding is Medium: {reasonless:#?}"
    );
}

#[test]
fn findings_are_sorted_by_category_name_path_and_deterministic() {
    let rows = "| a | grep: `zeta_ghost` | PASS |\n\
                | b | grep: `alpha_ghost` | PASS | <!-- pcite:allow -->\n";
    let files = [
        (MANIFEST, manifest(rows)),
        (OTHER_MANIFEST, manifest(rows)),
        (SOURCE, "pub fn other() {}\n".to_string()),
    ];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
    let tracked = [MANIFEST, OTHER_MANIFEST, SOURCE];

    let first = check_fixture(&files, &tracked);
    let second = check_fixture(&files, &tracked);
    assert_eq!(first, second, "two runs over one tree must agree");

    let keys: Vec<(String, String, String)> = first
        .iter()
        .map(|f| {
            (
                category(f).to_string(),
                name(f).to_string(),
                f.task_id.clone(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "findings are ordered by (category, name, path)"
    );
    assert_eq!(
        keys.len(),
        6,
        "two manifests x (two fabricated cites + one reasonless marker); got {keys:#?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI — reachable by `--pattern PCITE`, and exit-neutral
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
fn pattern_pcite_reports_a_fabricated_cite_and_exits_zero() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("pcite: skipping the CLI test — git not available");
        return;
    }
    let repo = tempfile::tempdir().expect("repo tempdir");
    let aux = tempfile::tempdir().expect("aux tempdir");
    git_in(repo.path(), &["init", "-q"]);
    write_file(repo.path(), MANIFEST, &manifest(GHOST_CITE));
    write_file(repo.path(), SOURCE, "pub fn other() {}\n");
    git_in(repo.path(), &["add", "-A"]);

    let tasks_file = aux.path().join("tasks.json");
    std::fs::write(&tasks_file, "[]").expect("write tasks.json");
    let runs_db = aux.path().join("runs.db");
    Connection::open(&runs_db)
        .expect("open runs.db")
        .execute_batch("CREATE TABLE events (task_id TEXT, event_type TEXT);")
        .expect("create events table");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify-audit"));
    cmd.args(["--pattern", "PCITE", "--no-jcodemunch", "--project-root"])
        .arg(repo.path())
        .arg("--tasks-file")
        .arg(&tasks_file)
        .arg("--runs-db")
        .arg(&runs_db);
    reify_audit::git_env::sanitize(&mut cmd);
    let out = cmd.output().expect("invoke reify-audit --pattern PCITE");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(0),
        "PCITE is Medium-only, so it never moves the exit code; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("\"PManifestCite\""),
        "the dispatch arm must route to the PCITE lane; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("fabricated-cite: ghost_symbol"),
        "the seeded fabrication must be reported; stderr:\n{stderr}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Real corpus
// ─────────────────────────────────────────────────────────────────────────────

fn real_manifests(root: &Path, tracked: &[String]) -> Vec<(String, String)> {
    tracked
        .iter()
        .filter(|p| p.starts_with(MANIFEST_ROOT) && p.ends_with(MANIFEST_SUFFIX))
        .filter_map(|p| {
            std::fs::read_to_string(root.join(p))
                .ok()
                .map(|c| (p.clone(), c))
        })
        .collect()
}

/// The cite grammar is silent on failure: a grammar that extracts nothing
/// makes the lane report clean. So the real corpus must keep yielding cites
/// above a floor (measured 620 segments in 60 manifests at 16de4dd187 —
/// floors, never counts), and a known cite must still be extracted.
#[test]
fn real_manifest_cite_floor_guard() {
    let root = repo_root();
    let tracked = RealGitOps::new(root.clone()).ls_files();
    assert!(
        !tracked.is_empty(),
        "ls_files() is empty — not a git work-tree; fail rather than pass vacuously"
    );
    let manifests = real_manifests(&root, &tracked);

    let mut segments = 0;
    let mut citing = 0;
    for (_, content) in &manifests {
        let cited: usize = content.lines().map(|l| cited_symbols(l).len()).sum();
        segments += cited;
        citing += usize::from(cited > 0);
    }
    assert!(
        segments >= 200 && citing >= 20,
        "cite grammar yielded {segments} segments across {citing} manifests \
         (of {} tracked) — below the 200 / 20 floor; fix the grammar, do not \
         lower the floor",
        manifests.len()
    );

    let anchor = "docs/prds/struct-ctor-field-type-conformance.capability-manifest.md";
    let (_, content) = manifests
        .iter()
        .find(|(p, _)| p == anchor)
        .unwrap_or_else(|| panic!("{anchor} must be a tracked manifest"));
    assert!(
        content
            .lines()
            .any(|l| cited_symbols(l).contains(&"check_expr_struct_ctor_args")),
        "the extraction anchor `check_expr_struct_ctor_args` must be cited in {anchor}"
    );
}

/// `check()` over the real tree: every finding is well-formed and the output
/// is deterministic. No count is pinned.
#[test]
fn real_repo_pcite_smoke() {
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
    let first = reify_audit::pcite::check(&ctx);
    let second = reify_audit::pcite::check(&ctx);
    assert_eq!(first, second, "check() must be deterministic");

    let known: BTreeSet<&str> = ["fabricated-cite", "allow-missing-reason"].into();
    for f in &first {
        assert_eq!(f.pattern, Pattern::PManifestCite, "{f:#?}");
        assert_eq!(f.severity, Severity::Medium, "{f:#?}");
        assert!(known.contains(category(f)), "unknown category: {f:#?}");
        match f.evidence.as_slice() {
            [EvidenceRef::File { path }] => assert!(
                path.starts_with(MANIFEST_ROOT) && path.ends_with(MANIFEST_SUFFIX),
                "evidence must be a manifest: {f:#?}"
            ),
            other => panic!("exactly one File evidence expected, got {other:?}"),
        }
    }
}
