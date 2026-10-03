//! Behaviour tests for PPRDSTATUS, driven only through the public
//! `reify_audit::pprdstatus::check` entry point.
//!
//! Each case builds a tempdir project root holding the PRD text, a
//! `MockGitOps` tracked-file list, and a `task_metadata` corpus whose leaves
//! carry `prd`. Lane assertions filter on their summary prefix, so one lane's
//! findings cannot perturb the other's counts.
//!
//! Fixture provenance: `fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md`
//! and `.post-edd9703fae.md` are verbatim excerpts of
//! `docs/prds/kernel-seam-contracts.md` around its SHIPPED re-stamp, commit
//! `edd9703fae`. The pre file is the `edd9703fae^` header (lines 1-5) plus the
//! two §9 body lines that cite #4876. The post file is the re-stamped header
//! (lines 1-11) plus the AS-AUTHORED #4876 line the frozen body still carries.

mod common;

use common::fixtures::legacy_meta;
use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, Severity,
    TaskMetadata,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use tempfile::TempDir;

const PRE_FIX: &str = include_str!("fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md");
const POST_FIX: &str = include_str!("fixtures/pprdstatus/kernel-seam-contracts.post-edd9703fae.md");

const KERNEL_SEAM_CONTRACTS: &str = "docs/prds/kernel-seam-contracts.md";
const STALE_STATUS_HEADER: &str = "stale-status-header:";

/// The kernel-seam-contracts decomposition: α #5102 … ξ #5116, plus the
/// adopted #4876.
fn kernel_seam_leaf_ids() -> Vec<String> {
    (5102..=5116)
        .chain([4876])
        .map(|id| id.to_string())
        .collect()
}

fn leaf(id: &str, status: &str, prd: &str) -> TaskMetadata {
    TaskMetadata {
        status: status.to_string(),
        prd: Some(prd.to_string()),
        ..legacy_meta(id)
    }
}

/// A hermetic project root: files on disk, the tracked subset, and the task
/// corpus handed to the detector.
struct Project {
    root: TempDir,
    tracked: Vec<String>,
    tasks: HashMap<String, TaskMetadata>,
}

impl Project {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("tempdir"),
            tracked: Vec::new(),
            tasks: HashMap::new(),
        }
    }

    /// Write `content` at `path` and list it as tracked.
    fn tracked_file(mut self, path: &str, content: &str) -> Self {
        write_file(self.root.path(), path, content);
        self.tracked.push(path.to_string());
        self
    }

    /// Write `content` at `path` WITHOUT listing it as tracked.
    fn untracked_file(self, path: &str, content: &str) -> Self {
        write_file(self.root.path(), path, content);
        self
    }

    fn task(mut self, meta: TaskMetadata) -> Self {
        self.tasks.insert(meta.task_id.clone(), meta);
        self
    }

    fn leaves(self, prd: &str, ids: &[String], status: &str) -> Self {
        ids.iter()
            .fold(self, |project, id| project.task(leaf(id, status, prd)))
    }

    fn check(&self) -> Vec<Finding> {
        let mut git = MockGitOps::new();
        git.set_ls_files(self.tracked.clone());
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let jcodemunch = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: self.root.path().to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jcodemunch,
            task_metadata: self.tasks.clone(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };
        reify_audit::pprdstatus::check(&ctx)
    }
}

fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

fn with_prefix(findings: Vec<Finding>, prefix: &str) -> Vec<Finding> {
    findings
        .into_iter()
        .filter(|finding| finding.summary.starts_with(prefix))
        .collect()
}

fn stale_headers(project: &Project) -> Vec<Finding> {
    with_prefix(project.check(), STALE_STATUS_HEADER)
}

fn ids(range: std::ops::RangeInclusive<u32>) -> Vec<String> {
    range.map(|id| id.to_string()).collect()
}

const LIVE_HEADER: &str = "# A live PRD\n\nStatus: active — decomposed and queued.\n";

// ─────────────────────────────────────────────────────────────────────────────
// Lane 1: stale-status-header
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn all_leaves_done_under_a_live_header_is_one_stale_header_finding() {
    let project = Project::new()
        .tracked_file(KERNEL_SEAM_CONTRACTS, PRE_FIX)
        .leaves(KERNEL_SEAM_CONTRACTS, &kernel_seam_leaf_ids(), "done");

    let findings = stale_headers(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    let finding = &findings[0];
    assert_eq!(finding.pattern, Pattern::PPrdStatus);
    assert_eq!(finding.severity, Severity::High);
    assert_eq!(finding.task_id, KERNEL_SEAM_CONTRACTS);
    for needle in ["'contract'", "stamp SHIPPED", "#4876", "#5116"] {
        assert!(
            finding.summary.contains(needle),
            "{needle} missing: {}",
            finding.summary
        );
    }
    assert!(
        finding.evidence.contains(&EvidenceRef::File {
            path: KERNEL_SEAM_CONTRACTS.to_string()
        }),
        "{:?}",
        finding.evidence
    );
}

#[test]
fn terminal_header_over_the_same_leaves_is_silent() {
    let project = Project::new()
        .tracked_file(KERNEL_SEAM_CONTRACTS, POST_FIX)
        .leaves(KERNEL_SEAM_CONTRACTS, &kernel_seam_leaf_ids(), "done");

    assert_eq!(stale_headers(&project), Vec::<Finding>::new());
}

#[test]
fn one_live_leaf_keeps_the_prd_live() {
    let project = Project::new()
        .tracked_file(KERNEL_SEAM_CONTRACTS, PRE_FIX)
        .leaves(KERNEL_SEAM_CONTRACTS, &kernel_seam_leaf_ids(), "done")
        .task(leaf("5110", "pending", KERNEL_SEAM_CONTRACTS));

    assert_eq!(stale_headers(&project), Vec::<Finding>::new());
}

#[test]
fn all_leaves_cancelled_recommends_withdrawn() {
    let prd = "docs/prds/abandoned.md";
    let project =
        Project::new()
            .tracked_file(prd, LIVE_HEADER)
            .leaves(prd, &ids(6001..=6003), "cancelled");

    let findings = stale_headers(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    let summary = &findings[0].summary;
    assert!(summary.contains("stamp WITHDRAWN"), "{summary}");
    assert!(!summary.contains("SHIPPED"), "{summary}");
}

#[test]
fn done_and_cancelled_leaves_recommend_shipped() {
    let prd = "docs/prds/mixed.md";
    let project = Project::new()
        .tracked_file(prd, LIVE_HEADER)
        .leaves(prd, &ids(6001..=6002), "done")
        .leaves(prd, &ids(6003..=6004), "cancelled");

    let findings = stale_headers(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert!(
        findings[0].summary.contains("stamp SHIPPED"),
        "{}",
        findings[0].summary
    );
}

#[test]
fn absent_status_label_is_reported_as_absent() {
    let prd = "docs/prds/unlabelled.md";
    let project = Project::new()
        .tracked_file(prd, "# An unlabelled PRD\n\nNo header here.\n")
        .leaves(prd, &ids(6001..=6002), "done");

    let findings = stale_headers(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert!(
        findings[0].summary.contains("is absent"),
        "{}",
        findings[0].summary
    );
}

#[test]
fn out_of_scope_paths_are_silent() {
    let untracked = "docs/prds/untracked.md";
    let note = "docs/notes/x.md";
    let manifest = "docs/prds/x.capability-manifest.md";
    let project = Project::new()
        .untracked_file(untracked, LIVE_HEADER)
        .tracked_file(note, LIVE_HEADER)
        .tracked_file(manifest, LIVE_HEADER)
        .leaves(untracked, &ids(6001..=6002), "done")
        .leaves(note, &ids(6003..=6004), "done")
        .leaves(manifest, &ids(6005..=6006), "done");

    assert_eq!(stale_headers(&project), Vec::<Finding>::new());
}

#[test]
fn prd_paths_are_normalised_before_grouping() {
    let prd = "docs/prds/x.md";
    let project = Project::new()
        .tracked_file(prd, LIVE_HEADER)
        .task(leaf("6001", "done", "./docs/prds/x.md"))
        .task(leaf("6002", "done", " docs/prds/x.md "))
        .task(leaf("6003", "done", prd))
        .task(TaskMetadata {
            status: "pending".to_string(),
            ..legacy_meta("6004")
        });

    let findings = stale_headers(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].task_id, prd);
    for id in ["#6001", "#6002", "#6003"] {
        assert!(
            findings[0].summary.contains(id),
            "{id}: {}",
            findings[0].summary
        );
    }
    assert!(
        !findings[0].summary.contains("#6004"),
        "{}",
        findings[0].summary
    );

    let with_live_dotted_leaf = project.task(leaf("6005", "pending", "./docs/prds/x.md"));
    assert_eq!(stale_headers(&with_live_dotted_leaf), Vec::<Finding>::new());
}

#[test]
fn empty_task_corpus_yields_no_findings() {
    let project = Project::new().tracked_file(KERNEL_SEAM_CONTRACTS, PRE_FIX);

    assert_eq!(project.check(), Vec::<Finding>::new());
}

/// The overlay's case pair: a Title-Case terminal token is terminal, and a
/// terminal word that is not the FIRST token is not.
#[test]
fn only_the_first_status_token_decides_terminality() {
    let auto_type_param = "docs/prds/auto-type-param-resolution.md";
    let kinematic = "docs/prds/kinematic-constraints.md";
    let project = Project::new()
        .tracked_file(
            auto_type_param,
            "# PRD: `auto` Type-Parameter Resolution (`Bearing<auto: Seal>`)\n\n\
             Status: Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md \
             (v0.3 completion contract).\n",
        )
        .tracked_file(
            kinematic,
            "# Kinematic Constraints — Forward, Open-Chain, Library-Level\n\n\
             ## §0 — Superseded\n\n\
             Status: deferred — superseded by `docs/prds/v0_3/kinematic-constraints-completion.md`\n",
        )
        .leaves(auto_type_param, &ids(6001..=6002), "done")
        .leaves(kinematic, &ids(6003..=6004), "done");

    let findings = stale_headers(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].task_id, kinematic);
    assert!(
        findings[0].summary.contains("'deferred'"),
        "{}",
        findings[0].summary
    );
}

#[test]
fn findings_are_sorted_by_prd_path() {
    let project = Project::new()
        .tracked_file("docs/prds/b.md", LIVE_HEADER)
        .tracked_file("docs/prds/a.md", LIVE_HEADER)
        .leaves("docs/prds/b.md", &ids(6001..=6002), "done")
        .leaves("docs/prds/a.md", &ids(6003..=6004), "done");

    let paths: Vec<String> = stale_headers(&project)
        .into_iter()
        .map(|finding| finding.task_id)
        .collect();

    assert_eq!(paths, ["docs/prds/a.md", "docs/prds/b.md"]);
}

/// The six already-terminal PRDs under the case-insensitive first-token rule:
/// the overlay's five ALL-CAPS stamps plus the Title-Case
/// `auto-type-param-resolution.md`. Header lines condensed from each.
#[test]
fn already_terminal_header_shapes_are_silent() {
    let terminal_headers = [
        (
            "docs/prds/v0_6/data-carrying-enums.md",
            "**Status:** **SHIPPED (v0.6)** — all decomposition leaves **landed**.",
        ),
        (
            "docs/prds/v0_6/generic-data-carrying-enums.md",
            "**Status:** **SHIPPED (v0.6)** — all decomposition leaves **landed**.",
        ),
        (
            "docs/prds/v0_6/result-and-fallback.md",
            "**Status:** **SHIPPED (v0.6) — both layers.** Layer A and Layer B landed.",
        ),
        (
            KERNEL_SEAM_CONTRACTS,
            "**Status: SHIPPED.** All 16 decomposition leaves have landed.",
        ),
        (
            "docs/prds/v0_6/process-dfm-geometry-metrology.md",
            "**Status:** SUPERSEDED 2026-06-08 · split into two PRDs after a feasibility sweep",
        ),
        (
            "docs/prds/auto-type-param-resolution.md",
            "Status: Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md",
        ),
    ];
    let project = terminal_headers.iter().enumerate().fold(
        Project::new(),
        |project, (index, (path, status_line))| {
            let first = 6001 + 10 * index as u32;
            project
                .tracked_file(path, &format!("# A terminal PRD\n\n{status_line}\n"))
                .leaves(path, &ids(first..=first + 1), "done")
        },
    );

    assert_eq!(stale_headers(&project), Vec::<Finding>::new());
}

// ─────────────────────────────────────────────────────────────────────────────
// Lane 2: cite-status-contradiction
// ─────────────────────────────────────────────────────────────────────────────

const CITE_STATUS_CONTRADICTION: &str = "cite-status-contradiction:";

fn cite_contradictions(project: &Project) -> Vec<Finding> {
    with_prefix(project.check(), CITE_STATUS_CONTRADICTION)
}

/// A task that belongs to no PRD, so lane 1 never groups it.
fn task(id: &str, status: &str) -> TaskMetadata {
    TaskMetadata {
        status: status.to_string(),
        ..legacy_meta(id)
    }
}

/// [`LIVE_HEADER`], a blank line, then `body` starting at line 5.
fn live_prd(body: &[&str]) -> String {
    format!("{LIVE_HEADER}\n{}\n", body.join("\n"))
}

/// The `(path, cited id)` of each finding, in emission order. The cited id is
/// the `#NNNN` right after the summary's `line N: `.
fn cited(findings: &[Finding]) -> Vec<(String, String)> {
    findings
        .iter()
        .map(|finding| {
            let after_line = finding
                .summary
                .split_once(": #")
                .map(|(_, rest)| rest)
                .expect("summary carries ': #<id>'");
            let id: String = after_line
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            (finding.task_id.clone(), id)
        })
        .collect()
}

#[test]
fn contradicted_status_parenthetical_is_one_finding() {
    let prd = "docs/prds/live.md";
    let project = Project::new()
        .tracked_file(
            prd,
            &live_prd(&["Adopt existing task **#4876** (`deferred`, high) — do not duplicate."]),
        )
        .task(task("4876", "done"));

    let findings = cite_contradictions(&project);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    let finding = &findings[0];
    assert_eq!(finding.pattern, Pattern::PPrdStatus);
    assert_eq!(finding.severity, Severity::High);
    assert_eq!(finding.task_id, prd);
    for needle in ["line 5", "#4876", "'deferred'", "task is done"] {
        assert!(
            finding.summary.contains(needle),
            "{needle} missing: {}",
            finding.summary
        );
    }
    assert_eq!(
        finding.evidence,
        vec![EvidenceRef::File {
            path: prd.to_string()
        }]
    );
}

/// The calibration pair: the pre-fix doc is stale on both lanes, and the
/// re-stamped doc is silent although its frozen body keeps the same #4876
/// line, because a terminal header marks the body as a record.
#[test]
fn calibration_pair_pre_fix_fires_both_lanes_and_post_fix_is_silent() {
    let pre = Project::new()
        .tracked_file(KERNEL_SEAM_CONTRACTS, PRE_FIX)
        .leaves(KERNEL_SEAM_CONTRACTS, &kernel_seam_leaf_ids(), "done");

    let findings = pre.check();

    assert_eq!(findings.len(), 2, "{findings:#?}");
    let contradictions = with_prefix(findings.clone(), CITE_STATUS_CONTRADICTION);
    assert_eq!(
        cited(&contradictions),
        [(KERNEL_SEAM_CONTRACTS.to_string(), "4876".to_string())]
    );
    assert_eq!(with_prefix(findings, STALE_STATUS_HEADER).len(), 1);

    let post = Project::new()
        .tracked_file(KERNEL_SEAM_CONTRACTS, POST_FIX)
        .leaves(KERNEL_SEAM_CONTRACTS, &kernel_seam_leaf_ids(), "done");

    assert_eq!(post.check(), Vec::<Finding>::new());
}

#[test]
fn parenthetical_without_a_status_word_is_silent() {
    let project = Project::new()
        .tracked_file(
            "docs/prds/live.md",
            &live_prd(&["**#4876 (preflight) — DONE.** Rust-side watertightness preflight"]),
        )
        .task(task("4876", "cancelled"));

    assert_eq!(cite_contradictions(&project), Vec::<Finding>::new());
}

#[test]
fn dated_parentheticals_are_silent_even_when_contradicted() {
    let project = Project::new()
        .tracked_file(
            "docs/prds/live.md",
            &live_prd(&[
                "Leaf #5830 (in-progress 2026-08-07) owns the guard.",
                "Leaf #6759 (in-progress at freeze time, claimed by a lane) owns the port.",
                "Leaf #1234 (pending as of the decompose) owns the docs.",
            ]),
        )
        .task(task("5830", "done"))
        .task(task("6759", "done"))
        .task(task("1234", "done"));

    assert_eq!(cite_contradictions(&project), Vec::<Finding>::new());
}

/// Statuses are compared by class {live, done, cancelled}: live statuses churn
/// on a timescale no doc tracks, while a terminal mismatch never heals.
#[test]
fn status_classes_decide_contradiction() {
    let prd = "docs/prds/live.md";
    let project = Project::new()
        .tracked_file(
            prd,
            &live_prd(&[
                "- #5101 (pending)",
                "- #5102 (deferred)",
                "- #5103 (done)",
                "- #5104 (cancelled)",
                "- #5105 (pending-high)",
                "- #5106 (`in-progress`, claimed — lane 7)",
                "- #5107 (blocked, LIVE)",
                "- #5108 (done)",
                "- #5109 (canceled)",
                "- #5110 (review)",
                "- #5111 (review)",
            ]),
        )
        .task(task("5101", "pending"))
        .task(task("5102", "pending"))
        .task(task("5103", "cancelled"))
        .task(task("5104", "done"))
        .task(task("5105", "done"))
        .task(task("5106", "done"))
        .task(task("5107", "done"))
        .task(task("5108", "pending"))
        .task(task("5109", "cancelled"))
        .task(task("5110", "done"))
        .task(task("5111", "review"));

    let fired: Vec<String> = cited(&cite_contradictions(&project))
        .into_iter()
        .map(|(_, id)| id)
        .collect();

    assert_eq!(
        fired,
        ["5103", "5104", "5105", "5106", "5107", "5108", "5110"]
    );
}

/// The status word need not close the parenthetical. On the live corpus
/// (2026-10-03) these three shapes were 4 of the 17 true lane-2 positives,
/// and no hit read a word like `Done when …` or `review-gated` as a status,
/// so requiring `)`, `,` or a backtick after the word would only lose them.
#[test]
fn status_word_followed_by_prose_still_asserts_the_status() {
    let prd = "docs/prds/live.md";
    let project = Project::new()
        .tracked_file(
            prd,
            &live_prd(&[
                "two-FixedSupport pin-collapse #6663 (in-progress; inherited, not owned);",
                "**Deps:** #6759 (in-progress standard leaf — real edge kept).",
                "**Supersedes:** task **#3114** (deferred — \"Tighten structural_physical.ri\").",
            ]),
        )
        .task(task("6663", "done"))
        .task(task("6759", "done"))
        .task(task("3114", "cancelled"));

    let fired: Vec<String> = cited(&cite_contradictions(&project))
        .into_iter()
        .map(|(_, id)| id)
        .collect();

    assert_eq!(fired, ["6663", "6759", "3114"]);
}

#[test]
fn unknown_ids_and_prd_relative_indices_are_silent() {
    let project = Project::new()
        .tracked_file(
            "docs/prds/live.md",
            &live_prd(&[
                "Leaf #5999 (pending) was never filed.",
                "See task #5 (done) in the plan table.",
                "This upholds invariant #7 (pending).",
            ]),
        )
        .task(task("5", "pending"))
        .task(task("7", "done"));

    assert_eq!(cite_contradictions(&project), Vec::<Finding>::new());
}

#[test]
fn capability_manifests_are_silent() {
    let project = Project::new()
        .tracked_file(
            "docs/prds/x.capability-manifest.md",
            &live_prd(&["Adopt existing task **#4876** (`deferred`, high) — do not duplicate."]),
        )
        .task(task("4876", "done"));

    assert_eq!(cite_contradictions(&project), Vec::<Finding>::new());
}

#[test]
fn only_the_cite_adjacent_to_the_parenthetical_is_read() {
    let project = Project::new()
        .tracked_file(
            "docs/prds/live.md",
            &live_prd(&["Chain #5825→#5844 (pending)."]),
        )
        .task(task("5825", "done"))
        .task(task("5844", "done"));

    let fired: Vec<String> = cited(&cite_contradictions(&project))
        .into_iter()
        .map(|(_, id)| id)
        .collect();

    assert_eq!(fired, ["5844"]);
}

#[test]
fn findings_are_per_cite_deduplicated_and_sorted_by_path_line_and_id() {
    let project = Project::new()
        .tracked_file("docs/prds/b.md", &live_prd(&["Leaf #5304 (pending)."]))
        .tracked_file(
            "docs/prds/a.md",
            &live_prd(&[
                "Leaves #5303 (pending) and #5301 (in-progress), and again #5303 (pending).",
                "Leaf #5302 (pending).",
            ]),
        )
        .task(task("5301", "done"))
        .task(task("5302", "done"))
        .task(task("5303", "done"))
        .task(task("5304", "done"));

    let findings = cite_contradictions(&project);

    assert_eq!(
        cited(&findings),
        [
            ("docs/prds/a.md".to_string(), "5301".to_string()),
            ("docs/prds/a.md".to_string(), "5303".to_string()),
            ("docs/prds/a.md".to_string(), "5302".to_string()),
            ("docs/prds/b.md".to_string(), "5304".to_string()),
        ]
    );
    assert!(
        findings[0].summary.contains("line 5"),
        "{}",
        findings[0].summary
    );
    assert!(
        findings[2].summary.contains("line 6"),
        "{}",
        findings[2].summary
    );
}

#[test]
fn non_prd_tracked_files_are_silent() {
    let project = Project::new()
        .tracked_file(
            "docs/notes/x.md",
            &live_prd(&["Adopt existing task **#4876** (`deferred`, high) — do not duplicate."]),
        )
        .task(task("4876", "done"));

    assert_eq!(cite_contradictions(&project), Vec::<Finding>::new());
}
