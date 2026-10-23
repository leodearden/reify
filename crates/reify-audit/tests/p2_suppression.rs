//! Integration tests for the stub matches the P2 consumer-stub detector does
//! NOT report because something else owns, escapes or has removed them.
//!
//! Wrapped in `mod p2_suppression { mod tests { ... } }` so each test's path is
//! `p2_suppression::tests::<name>`, matching tests/p2.rs. Every test drives the
//! public `p2_consumer_stub::check` through `MockGitOps`; P2 issues no SQL, so
//! an in-memory rusqlite connection satisfies `AuditContext.conn`.

mod p2_suppression {

    use reify_audit::{
        AuditContext, DoneProvenance, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps,
        TaskMetadata, p2_consumer_stub,
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

    fn owned_lines(lines: &[(usize, &str)]) -> Vec<(usize, String)> {
        lines.iter().map(|(n, l)| (*n, l.to_string())).collect()
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
        git.set_diff_added_lines("main", &format!("task/{task_id}"), path, owned_lines(lines));
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

            let findings = run_p2(
                &git,
                corpus(vec![meta("9601", "in_progress", &[detector, control])]),
            );

            assert_eq!(
                finding_paths(&findings),
                vec![control.to_string()],
                "got {findings:?}"
            );
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
                    (
                        1297,
                        "            unimplemented!() // ptodo:allow exhaustiveness/stub arm - not tracked debt",
                    ),
                    (1400, "            unimplemented!()"),
                ],
            );

            let findings = run_p2(&git, corpus(vec![meta("9602", "in_progress", &[path])]));

            assert_eq!(
                finding_paths(&findings),
                vec![path.to_string()],
                "got {findings:?}"
            );
            let summary = &findings[0].summary;
            assert!(
                summary.contains("line 1400"),
                "unescaped line must be reported: {summary}"
            );
            assert!(
                !summary.contains("line 1297"),
                "escaped line must not be reported: {summary}"
            );
        }

        /// Introducer 9610 (in_progress) plus cited tasks: 7001 pending,
        /// 7005 in-progress, 7002 done, 7003 cancelled. 7004 is deliberately absent.
        fn cite_corpus(introducer_files: &[&str]) -> HashMap<String, TaskMetadata> {
            corpus(vec![
                meta("9610", "in_progress", introducer_files),
                meta("7001", "pending", &[]),
                meta("7005", "in-progress", &[]),
                meta("7002", "done", &[]),
                meta("7003", "cancelled", &[]),
            ])
        }

        #[test]
        fn marker_citing_live_other_task_suppressed() {
            let control = "crates/x/control.rs";
            let cases = [
                ("crates/x/live.rs", "    // TODO(#7001): wire the consumer"),
                ("crates/x/live_inprogress.rs", "    // TODO(#7005): wire"),
                (
                    "crates/x/live_trailing.rs",
                    "    unimplemented!() // owned by #7001",
                ),
                ("crates/x/mixed.rs", "    // TODO(#7002, #7001): wire"),
                (control, "    // TODO(impl pending)"),
            ];
            let mut git = MockGitOps::new();
            for (path, line) in cases {
                add_on_branch(&mut git, "9610", path, &[(10, line)]);
            }
            let paths: Vec<&str> = cases.iter().map(|(p, _)| *p).collect();

            let findings = run_p2(&git, cite_corpus(&paths));

            assert_eq!(
                finding_paths(&findings),
                vec![control.to_string()],
                "got {findings:?}"
            );
        }

        #[test]
        fn marker_citing_terminal_absent_or_self_still_reported() {
            let cases = [
                ("crates/x/done.rs", "    // TODO(#7002): wire"),
                ("crates/x/cancelled.rs", "    // TODO(#7003): wire"),
                ("crates/x/absent.rs", "    // TODO(#7004): wire"),
                ("crates/x/self.rs", "    // TODO(#9610): wire"),
            ];
            let mut git = MockGitOps::new();
            for (path, line) in cases {
                add_on_branch(&mut git, "9610", path, &[(10, line)]);
            }
            let paths: Vec<&str> = cases.iter().map(|(p, _)| *p).collect();

            let findings = run_p2(&git, cite_corpus(&paths));

            let mut expected: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
            expected.sort();
            assert_eq!(finding_paths(&findings), expected, "got {findings:?}");
        }

        /// Route 1 reads a historical merge-commit diff: a marker main has since
        /// removed is gone, while one main still carries (re-indented and moved)
        /// is the same debt.
        #[test]
        fn merged_commit_marker_absent_from_main_dropped() {
            let kept = "crates/x/kept.rs";
            let deleted = "crates/x/deleted.rs";
            let mut git = MockGitOps::new();
            git.set_is_ancestor("m1", "main", true);
            git.set_diff_added_lines_in_commit(
                "m1",
                kept,
                owned_lines(&[
                    (10, "    // TODO(impl pending)"),
                    (20, "    unimplemented!()"),
                ]),
            );
            git.set_file_lines_on(
                "main",
                kept,
                owned_lines(&[(3, "fn f() {"), (44, "        unimplemented!()"), (45, "}")]),
            );
            // No file_lines_on for `deleted`: the file is gone from main.
            git.set_diff_added_lines_in_commit("m1", deleted, owned_lines(&[(5, "    // stub")]));
            let mut task = meta("9620", "done", &[kept, deleted]);
            task.done_at = Some(1000);
            task.done_provenance = Some(DoneProvenance {
                kind: Some("merged".to_string()),
                commit: Some("m1".to_string()),
                note: None,
            });

            let findings = run_p2(&git, corpus(vec![task]));

            assert_eq!(
                finding_paths(&findings),
                vec![kept.to_string()],
                "got {findings:?}"
            );
            let summary = &findings[0].summary;
            assert!(
                summary.contains("line 20"),
                "marker still on main must be reported: {summary}"
            );
            assert!(
                !summary.contains("line 10"),
                "marker gone from main must be dropped: {summary}"
            );
        }

        /// Route 3's added lines live on the task branch and are by definition not
        /// on main yet, so the presence check must not apply to them.
        #[test]
        fn live_branch_marker_not_yet_on_main_still_reported() {
            let path = "crates/x/wip.rs";
            let mut git = MockGitOps::new();
            add_on_branch(&mut git, "9621", path, &[(7, "    unimplemented!()")]);
            git.set_file_lines_on("main", path, owned_lines(&[(1, "fn g() {}")]));

            let findings = run_p2(&git, corpus(vec![meta("9621", "in_progress", &[path])]));

            assert_eq!(
                finding_paths(&findings),
                vec![path.to_string()],
                "got {findings:?}"
            );
        }
    }
}
