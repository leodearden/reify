//! Integration tests for the stub matches the P2 consumer-stub detector does
//! NOT report because something else owns, escapes or has removed them.
//!
//! Wrapped in `mod p2_suppression { mod tests { ... } }` so each test's path is
//! `p2_suppression::tests::<name>`, matching tests/p2.rs. Every test drives the
//! public `p2_consumer_stub::check` through `MockGitOps`; P2 issues no SQL, so
//! an in-memory rusqlite connection satisfies `AuditContext.conn`.

mod p2_suppression {

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, TaskMetadata,
    p2_consumer_stub,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// A task with a benign title (no "stub"/"placeholder", so P2 emits Medium).
fn meta(task_id: &str, status: &str, files: &[&str]) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        status: status.to_string(),
        files: files.iter().map(|f| f.to_string()).collect(),
        done_provenance: None,
        title: "Wire foo into bar".to_string(),
        prd: None,
        consumer_ref: None,
        audit_foundation: None,
        done_at: None,
    }
}

fn corpus(tasks: Vec<TaskMetadata>) -> HashMap<String, TaskMetadata> {
    tasks.into_iter().map(|t| (t.task_id.clone(), t)).collect()
}

fn run_p2(git: &MockGitOps, task_metadata: HashMap<String, TaskMetadata>) -> Vec<Finding> {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: PathBuf::from("/tmp/fake-project"),
        conn: &conn,
        git,
        jcodemunch: &jc,
        task_metadata,
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };
    p2_consumer_stub::check(&ctx)
}

/// Sorted `EvidenceRef::File` paths across `findings`.
fn finding_paths(findings: &[Finding]) -> Vec<String> {
    let mut paths: Vec<String> = findings
        .iter()
        .flat_map(|f| &f.evidence)
        .filter_map(|e| match e {
            EvidenceRef::File { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    paths.sort();
    paths
}

/// Registers `lines` as what task `task_id`'s live branch adds to `path`.
fn add_on_branch(git: &mut MockGitOps, task_id: &str, path: &str, lines: &[(usize, &str)]) {
    git.set_diff_added_lines(
        "main",
        &format!("task/{task_id}"),
        path,
        lines.iter().map(|(n, l)| (*n, l.to_string())).collect(),
    );
}

mod tests {
    use super::*;

    #[test]
    fn detector_crate_source_excluded() {
        let detector = "crates/reify-audit/src/ptodo.rs";
        let control = "crates/other/src/real.rs";
        let fixture_line = "            \"// TODO(impl pending)\",";
        let mut git = MockGitOps::new();
        add_on_branch(&mut git, "9601", detector, &[(120, fixture_line)]);
        add_on_branch(&mut git, "9601", control, &[(120, fixture_line)]);

        let findings = run_p2(&git, corpus(vec![meta("9601", "in_progress", &[detector, control])]));

        assert_eq!(finding_paths(&findings), vec![control.to_string()], "got {findings:?}");
    }

    #[test]
    fn ptodo_allow_escaped_line_not_flagged() {
        let path = "crates/reify-eval/src/realization_content.rs";
        let mut git = MockGitOps::new();
        add_on_branch(
            &mut git,
            "9602",
            path,
            &[
                (1297, "            unimplemented!() // ptodo:allow exhaustiveness/stub arm - not tracked debt"),
                (1400, "            unimplemented!()"),
            ],
        );

        let findings = run_p2(&git, corpus(vec![meta("9602", "in_progress", &[path])]));

        assert_eq!(finding_paths(&findings), vec![path.to_string()], "got {findings:?}");
        let summary = &findings[0].summary;
        assert!(summary.contains("line 1400"), "unescaped line must be reported: {summary}");
        assert!(!summary.contains("line 1297"), "escaped line must not be reported: {summary}");
    }
}

}
