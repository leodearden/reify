//! Behaviour tests for POLDCORE, driven only through the public
//! `reify_audit::poldcore::check` entry point.
//!
//! Each case builds a tempdir project root holding a hold list at the
//! detector's path, and a task corpus of `..legacy_meta(id)` records. Kind
//! assertions filter on the summary prefix, so one kind's findings cannot
//! perturb another's counts.

mod common;

use common::fixtures::legacy_meta;
use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, Severity,
    TaskMetadata,
};
use rusqlite::Connection;
use std::collections::HashMap;
use tempfile::TempDir;

const HOLD_LIST: &str = "docs/notes/old-evaluation-core-hold.toml";
const HOLDER: &str = "10510";

const HOLD_MISSED: &str = "hold-missed:";
const HOLD_MISSED_REGISTRATION_ONLY: &str = "hold-missed-registration-only:";
const HOLDER_STATE: &str = "holder-state:";
const HOLD_LIST_INVALID: &str = "hold-list-invalid:";

const ENGINE_EVAL: &str = "crates/reify-eval/src/engine_eval.rs";
const ENGINE_BUILD_STAGE: &str = "crates/reify-eval/src/engine_build/stage.rs";
const ENGINE_ADMIN: &str = "crates/reify-eval/src/engine_admin.rs";
const LIB_RS: &str = "crates/reify-eval/src/lib.rs";
const GUI_FILE: &str = "crates/reify-gui/src/x.rs";

const FIXTURE_LIST: &str = r#"holder = 10510
files = [
    "crates/reify-eval/src/engine_eval.rs",
    "crates/reify-eval/src/engine_edit.rs",
    "crates/reify-eval/src/engine_edit/",
    "crates/reify-eval/src/engine_build/",
    "crates/reify-eval/src/engine_admin.rs",
    "crates/reify-eval/src/lib.rs",
]
registration_only = ["crates/reify-eval/src/lib.rs"]
"#;

/// A hermetic project root: the hold list on disk, and the task corpus
/// handed to the detector.
struct Project {
    root: TempDir,
    tasks: HashMap<String, TaskMetadata>,
}

impl Project {
    /// A root holding the fixture list.
    fn new() -> Self {
        Self::bare().hold_list(FIXTURE_LIST)
    }

    /// A root with no hold list at all.
    fn bare() -> Self {
        Self {
            root: tempfile::tempdir().expect("tempdir"),
            tasks: HashMap::new(),
        }
    }

    fn hold_list(self, text: &str) -> Self {
        let path = self.root.path().join(HOLD_LIST);
        std::fs::create_dir_all(path.parent().expect("list has a parent")).expect("mkdir");
        std::fs::write(&path, text).expect("write hold list");
        self
    }

    fn task(mut self, meta: TaskMetadata) -> Self {
        self.tasks.insert(meta.task_id.clone(), meta);
        self
    }

    fn tasks(self, metas: impl IntoIterator<Item = TaskMetadata>) -> Self {
        metas.into_iter().fold(self, Project::task)
    }

    fn check(&self) -> Vec<Finding> {
        let git = MockGitOps::new();
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let jcodemunch = MockJCodemunchOps::new();
        let ctx = AuditContext::new(self.root.path().to_path_buf(), &conn, &git, &jcodemunch)
            .with_tasks(self.tasks.clone());
        reify_audit::poldcore::check(&ctx)
    }
}

/// The holder as the store keeps it: pending, kept from dispatching by a
/// deferred watch.
fn healthy_holder() -> TaskMetadata {
    TaskMetadata {
        status: "pending".to_string(),
        title: "[HOLDER] Old evaluation core".to_string(),
        has_deferred_watch: true,
        ..legacy_meta(HOLDER)
    }
}

/// A pending task listing `files`, with no edges and no release.
fn open_task(id: &str, files: &[&str]) -> TaskMetadata {
    TaskMetadata {
        status: "pending".to_string(),
        title: format!("Task {id} reshapes the engine"),
        files: files.iter().map(|f| f.to_string()).collect(),
        ..legacy_meta(id)
    }
}

fn with_prefix(findings: &[Finding], prefix: &str) -> Vec<Finding> {
    findings
        .iter()
        .filter(|finding| finding.summary.starts_with(prefix))
        .cloned()
        .collect()
}

fn task_ids(findings: &[Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.task_id.as_str()).collect()
}

/// The findings for a corpus of the healthy holder plus `task`.
fn check_one(task: TaskMetadata) -> Vec<Finding> {
    Project::new().task(healthy_holder()).task(task).check()
}

// ─────────────────────────────────────────────────────────────────────────────
// hold-missed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn only_the_task_with_no_edge_and_no_release_is_reported() {
    let files = [ENGINE_EVAL, ENGINE_BUILD_STAGE, GUI_FILE];
    let released = TaskMetadata {
        has_old_core_release: true,
        ..open_task("901", &files)
    };
    let held = TaskMetadata {
        dependencies: vec![HOLDER.to_string()],
        ..open_task("902", &files)
    };
    let findings = Project::new()
        .tasks([healthy_holder(), open_task("900", &files), released, held])
        .check();

    assert_eq!(findings.len(), 1, "{findings:#?}");
    let finding = &findings[0];
    assert_eq!(finding.pattern, Pattern::POldCoreHold);
    assert_eq!(finding.severity, Severity::High);
    assert_eq!(finding.task_id, "900");
    assert!(
        finding.summary.starts_with(HOLD_MISSED),
        "{}",
        finding.summary
    );
    for named in [
        "#900",
        "Task 900 reshapes the engine",
        ENGINE_EVAL,
        ENGINE_BUILD_STAGE,
    ] {
        assert!(
            finding.summary.contains(named),
            "summary names {named}: {}",
            finding.summary
        );
    }
    assert_eq!(
        finding.evidence,
        vec![
            EvidenceRef::MetadataFiles {
                entries: vec![ENGINE_EVAL.to_string(), ENGINE_BUILD_STAGE.to_string()],
            },
            EvidenceRef::File {
                path: HOLD_LIST.to_string(),
            },
        ]
    );
}

#[test]
fn a_task_whose_only_held_file_is_registration_only_is_low() {
    let findings = check_one(open_task(
        "900",
        &["crates/reify-eval/src/new_mod.rs", LIB_RS],
    ));

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Low);
    assert_eq!(findings[0].pattern, Pattern::POldCoreHold);
    assert!(
        findings[0]
            .summary
            .starts_with(HOLD_MISSED_REGISTRATION_ONLY),
        "{}",
        findings[0].summary
    );
}

#[test]
fn registration_only_beside_another_held_file_is_high() {
    let findings = check_one(open_task("900", &[LIB_RS, ENGINE_EVAL]));

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::High);
    assert!(
        findings[0].summary.starts_with(HOLD_MISSED),
        "{}",
        findings[0].summary
    );
}

#[test]
fn the_rule_is_half_or_more_of_the_non_test_files() {
    let reported = |files: &[&str]| !check_one(open_task("900", files)).is_empty();

    assert!(
        !reported(&[ENGINE_EVAL, GUI_FILE, "crates/reify-gui/src/y.rs"]),
        "1 held of 3 is under half"
    );
    assert!(reported(&[ENGINE_EVAL, GUI_FILE]), "1 held of 2 is half");
    assert!(
        reported(&[
            ENGINE_EVAL,
            "crates/reify-eval/tests/a.rs",
            "crates/reify-cli/tests/b.rs",
        ]),
        "test paths leave the denominator"
    );
    assert!(
        !reported(&[
            "crates/reify-eval/tests/a.rs",
            "crates/reify-eval/src/engine_build/tests/stage.rs",
        ]),
        "a test path is never held"
    );
    assert!(
        !reported(&["crates/reify-eval/src/engine_build/tests/"]),
        "a test directory under a held directory is a test path, never held"
    );
    assert!(!reported(&[]), "a task with no files is not reported");
}

#[test]
fn a_name_prefix_of_a_held_entry_is_not_held() {
    let findings = check_one(open_task(
        "900",
        &[
            "crates/reify-evalcore/src/engine_eval.rs",
            "crates/reify-eval/src/engine_editor.rs",
        ],
    ));

    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn only_an_open_task_is_reported() {
    let with_status = |id: &str, status: &str| TaskMetadata {
        status: status.to_string(),
        ..open_task(id, &[ENGINE_EVAL])
    };
    let findings = Project::new()
        .tasks([
            healthy_holder(),
            with_status("1", "done"),
            with_status("2", "cancelled"),
            with_status("3", "pending"),
            with_status("4", "in-progress"),
            with_status("5", "deferred"),
            with_status("6", "blocked"),
        ])
        .check();

    assert_eq!(task_ids(&findings), ["3", "4", "5", "6"], "{findings:#?}");
}

#[test]
fn the_holder_is_never_reported_as_missed() {
    let holder = TaskMetadata {
        files: vec![ENGINE_EVAL.to_string()],
        ..healthy_holder()
    };

    let findings = Project::new().task(holder).check();

    assert!(findings.is_empty(), "{findings:#?}");
}

/// Task #10532 before it was held: two of its three files are held, and its
/// one edge is on a task other than the holder.
#[test]
fn the_10532_replay_is_one_high_hold_missed() {
    let task = TaskMetadata {
        dependencies: vec!["10376".to_string()],
        ..open_task(
            "10532",
            &["crates/reify-eval/src/def_env.rs", ENGINE_ADMIN, LIB_RS],
        )
    };

    let findings = check_one(task);

    assert_eq!(findings.len(), 1, "{findings:#?}");
    let finding = &findings[0];
    assert_eq!(finding.severity, Severity::High);
    assert_eq!(finding.task_id, "10532");
    assert!(
        finding.summary.starts_with(HOLD_MISSED),
        "{}",
        finding.summary
    );
    for named in [ENGINE_ADMIN, LIB_RS, "2 of 3"] {
        assert!(
            finding.summary.contains(named),
            "summary names {named}: {}",
            finding.summary
        );
    }
    assert!(
        !finding.summary.contains("def_env.rs"),
        "def_env.rs is not held: {}",
        finding.summary
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// holder-state
// ─────────────────────────────────────────────────────────────────────────────

/// The holder-state findings for a corpus of `holder` plus one open task.
fn holder_state(holder: TaskMetadata) -> Vec<Finding> {
    let findings = Project::new()
        .task(holder)
        .task(open_task("900", &[GUI_FILE]))
        .check();
    with_prefix(&findings, HOLDER_STATE)
}

/// Asserts `findings` is one High holder-state finding naming each of `defects`.
fn assert_one_holder_state(findings: &[Finding], defects: &[&str]) {
    assert_eq!(findings.len(), 1, "{findings:#?}");
    let finding = &findings[0];
    assert_eq!(finding.pattern, Pattern::POldCoreHold);
    assert_eq!(finding.severity, Severity::High);
    assert_eq!(finding.task_id, HOLDER);
    assert_eq!(
        finding.evidence,
        vec![EvidenceRef::File {
            path: HOLD_LIST.to_string(),
        }]
    );
    for defect in defects {
        assert!(
            finding.summary.contains(defect),
            "summary names {defect}: {}",
            finding.summary
        );
    }
}

#[test]
fn a_healthy_holder_yields_no_holder_state() {
    assert!(holder_state(healthy_holder()).is_empty());
}

#[test]
fn a_closed_holder_is_one_holder_state_finding() {
    for status in ["done", "cancelled"] {
        let holder = TaskMetadata {
            status: status.to_string(),
            ..healthy_holder()
        };
        assert_one_holder_state(&holder_state(holder), &[status]);
    }
}

#[test]
fn a_holder_without_its_deferred_watch_is_one_holder_state_finding() {
    let holder = TaskMetadata {
        has_deferred_watch: false,
        ..healthy_holder()
    };
    assert_one_holder_state(&holder_state(holder), &["deferred_watch"]);
}

#[test]
fn a_holder_with_trigger_met_is_one_holder_state_finding() {
    let holder = TaskMetadata {
        trigger_met: true,
        ..healthy_holder()
    };
    assert_one_holder_state(&holder_state(holder), &["trigger_met"]);
}

#[test]
fn every_holder_defect_is_named_in_one_finding() {
    let holder = TaskMetadata {
        status: "done".to_string(),
        has_deferred_watch: false,
        trigger_met: true,
        ..healthy_holder()
    };
    assert_one_holder_state(
        &holder_state(holder),
        &["done", "deferred_watch", "trigger_met"],
    );
}

#[test]
fn a_holder_absent_from_the_corpus_is_one_holder_state_finding() {
    let findings = Project::new().task(open_task("900", &[GUI_FILE])).check();

    assert_one_holder_state(&with_prefix(&findings, HOLDER_STATE), &["absent"]);
}

// ─────────────────────────────────────────────────────────────────────────────
// hold-list-invalid
// ─────────────────────────────────────────────────────────────────────────────

/// Asserts `findings` is exactly one High hold-list-invalid finding whose
/// summary names each of `named`.
fn assert_only_list_fault(findings: &[Finding], named: &[&str]) {
    assert_eq!(findings.len(), 1, "{findings:#?}");
    let finding = &findings[0];
    assert_eq!(finding.pattern, Pattern::POldCoreHold);
    assert_eq!(finding.severity, Severity::High);
    assert_eq!(finding.task_id, HOLD_LIST);
    assert!(
        finding.summary.starts_with(HOLD_LIST_INVALID),
        "{}",
        finding.summary
    );
    for name in named {
        assert!(
            finding.summary.contains(name),
            "summary names {name}: {}",
            finding.summary
        );
    }
}

fn corpus_with_a_missed_task(project: Project) -> Vec<Finding> {
    project
        .task(healthy_holder())
        .task(open_task("900", &[ENGINE_EVAL]))
        .check()
}

#[test]
fn an_absent_hold_list_is_the_only_finding() {
    let findings = corpus_with_a_missed_task(Project::bare());

    assert_only_list_fault(&findings, &[HOLD_LIST]);
}

#[test]
fn a_malformed_hold_list_is_the_only_finding() {
    let findings = corpus_with_a_missed_task(Project::bare().hold_list("holder = \n"));

    assert_only_list_fault(&findings, &["TOML"]);
}

#[test]
fn a_hold_list_breaking_an_invariant_is_the_only_finding() {
    let list = "holder = 10510\n\
                files = [\"crates/reify-eval/src/engine_eval.rs\"]\n\
                registration_only = [\"crates/reify-eval/src/lib.rs\"]\n";

    let findings = corpus_with_a_missed_task(Project::bare().hold_list(list));

    assert_only_list_fault(&findings, &["not in files", LIB_RS]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn holder_state_comes_first_then_missed_tasks_by_numeric_id() {
    let broken_holder = TaskMetadata {
        status: "done".to_string(),
        ..healthy_holder()
    };
    let findings = Project::new()
        .tasks([
            open_task("10000", &[ENGINE_EVAL]),
            broken_holder,
            open_task("900", &[ENGINE_EVAL]),
        ])
        .check();

    assert_eq!(
        task_ids(&findings),
        [HOLDER, "900", "10000"],
        "{findings:#?}"
    );
    assert!(findings[0].summary.starts_with(HOLDER_STATE));
}

// ─────────────────────────────────────────────────────────────────────────────
// The tracked list
// ─────────────────────────────────────────────────────────────────────────────

/// Floor guard on the tracked hold list: it parses and meets every invariant,
/// its holder is 10510, the anchor `engine_eval.rs` is held, and the sibling
/// crate `reify-evalcore` is not. Anchors only, never counts, so editing the
/// list does not red it.
#[test]
fn the_tracked_hold_list_is_usable_and_holds_its_anchor() {
    let root = reify_test_support::workspace_root::workspace_root();
    let git = MockGitOps::new();
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jcodemunch = MockJCodemunchOps::new();
    let corpus = [
        healthy_holder(),
        open_task("1", &[ENGINE_EVAL]),
        open_task("2", &["crates/reify-evalcore/src/engine_eval.rs"]),
    ]
    .into_iter()
    .map(|task| (task.task_id.clone(), task))
    .collect();
    let ctx = AuditContext::new(root.to_path_buf(), &conn, &git, &jcodemunch).with_tasks(corpus);

    let findings = reify_audit::poldcore::check(&ctx);

    assert!(
        with_prefix(&findings, HOLD_LIST_INVALID).is_empty(),
        "the tracked list must parse and meet every invariant: {findings:#?}"
    );
    assert!(
        with_prefix(&findings, HOLDER_STATE).is_empty(),
        "the tracked list's holder must be {HOLDER}: {findings:#?}"
    );
    let missed = with_prefix(&findings, HOLD_MISSED);
    assert_eq!(
        task_ids(&missed),
        ["1"],
        "engine_eval.rs is held and reify-evalcore is not: {findings:#?}"
    );
    assert_eq!(missed[0].severity, Severity::High);
}
