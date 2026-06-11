//! Integration tests for the PTODO structural-lane detector (`ptodo::check`).
//!
//! User-observable signal:
//!   `cargo test -p reify-audit ptodo::tests`
//!
//! These tests drive `check()` against a real on-disk working tree (a
//! `tempfile` tempdir as `project_root`) plus a `MockGitOps` whose
//! `set_ls_files` supplies the tracked-path enumeration. The structural lane
//! reads working-tree content via `std::fs::read_to_string(project_root.join(
//! path))`, so the files must exist on disk; only the file *list* is mocked.
//! In-memory rusqlite + MockJCodemunchOps satisfy `AuditContext` (the
//! structural lane issues no SQL and no jcodemunch queries).

mod ptodo {

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, Severity,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// Write `content` to relative `path` inside `root`, creating parent dirs.
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write file");
}

mod tests {
    use super::*;

    /// The structural lane emits exactly the three content-marker kinds
    /// (untracked / malformed-cite / phantom-tracking), one per offending
    /// swept file, and suppresses each of: an allowlisted-prefix path, an
    /// inline-escaped line, a non-swept extension, and a canonically-cited
    /// marker.
    #[test]
    fn check_emits_three_kinds_and_suppresses_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Three offending swept files → one finding each.
        write_file(root, "untracked.rs", "// TODO: wire this\n");
        write_file(root, "malformed.rs", "// TODO(task δ): migrate\n");
        write_file(root, "phantom.rs", "// tracked as a follow-up task\n");
        // Suppressed paths: allowlisted prefix, inline escape, non-swept ext,
        // and a canonically-cited marker (tracked → deferred to β).
        write_file(root, "crates/reify-audit/x.rs", "// TODO: allowlisted self\n");
        write_file(root, "escaped.rs", "// TODO: leave me  // ptodo:allow\n");
        write_file(root, "notes.md", "// TODO: in a non-swept doc\n");
        write_file(root, "cited.rs", "// TODO(#4553): cited\n");

        let all_paths: Vec<String> = [
            "untracked.rs",
            "malformed.rs",
            "phantom.rs",
            "crates/reify-audit/x.rs",
            "escaped.rs",
            "notes.md",
            "cited.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let mut git = MockGitOps::new();
        git.set_ls_files(all_paths);

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

        // Exactly three findings, all PTodo + Medium.
        assert_eq!(
            findings.len(),
            3,
            "expected exactly 3 PTODO findings; got {findings:?}"
        );
        for f in &findings {
            assert_eq!(f.pattern, Pattern::PTodo, "wrong pattern: {f:?}");
            assert_eq!(f.severity, Severity::Medium, "wrong severity: {f:?}");
        }

        // Locate the finding whose evidence references `path`.
        let finding_for = |path: &str| -> &Finding {
            findings
                .iter()
                .find(|f| {
                    f.evidence
                        .iter()
                        .any(|e| matches!(e, EvidenceRef::File { path: p } if p == path))
                })
                .unwrap_or_else(|| panic!("no finding referencing {path}; findings={findings:?}"))
        };

        // §8.3 kind carried as a stable summary prefix `"<kind>: …"`.
        assert!(
            finding_for("untracked.rs").summary.starts_with("untracked:"),
            "untracked.rs summary must start with the kind token: {:?}",
            finding_for("untracked.rs").summary
        );
        assert!(
            finding_for("malformed.rs")
                .summary
                .starts_with("malformed-cite:"),
            "malformed.rs summary must start with the kind token: {:?}",
            finding_for("malformed.rs").summary
        );
        assert!(
            finding_for("phantom.rs")
                .summary
                .starts_with("phantom-tracking:"),
            "phantom.rs summary must start with the kind token: {:?}",
            finding_for("phantom.rs").summary
        );

        // The four suppressed files must yield NO finding.
        for suppressed in [
            "crates/reify-audit/x.rs",
            "escaped.rs",
            "notes.md",
            "cited.rs",
        ] {
            let any = findings.iter().any(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: p } if p == suppressed))
            });
            assert!(
                !any,
                "{suppressed} must not produce a finding; findings={findings:?}"
            );
        }
    }
}

}
