//! The audit data model: what a detector is handed ([`AuditContext`], [`TaskMetadata`]) and what it emits ([`Finding`] with its [`Severity`], [`Pattern`] and [`EvidenceRef`]s).

#[cfg(test)]
mod tests {
    /// Every arm of the tracked-file read that PTODO, PDSSENTINEL and
    /// PDOCCOVER each used to inline separately and none of them tested
    /// directly: the path resolves against `project_root` rather than the
    /// process cwd, and anything unreadable — absent, a directory, or not
    /// valid UTF-8 — yields `None` rather than a finding or a panic.
    ///
    /// The encoding arm is the one a real corpus reaches: `read_to_string`
    /// fails on invalid UTF-8, so any tracked file with a swept extension and
    /// binary-ish content exercises it, and it is what the detectors'
    /// `files_scanned` accounting means when it excludes paths whose read
    /// yielded `None`.
    #[test]
    fn read_relative_resolves_against_project_root_and_skips_unreadable() {
        use super::AuditContext;
        use crate::git_ops::MockGitOps;
        use crate::jcodemunch_ops::MockJCodemunchOps;
        use rusqlite::Connection;
        use std::collections::HashMap;

        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("crates/x/src")).expect("mkdir src");
        std::fs::write(root.path().join("crates/x/src/a.rs"), "fn a() {}\n").expect("write");
        std::fs::create_dir_all(root.path().join("crates/x/tests")).expect("mkdir tests");

        let conn = Connection::open_in_memory().expect("in-memory db");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: root.path().to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        assert_eq!(
            ctx.read_relative("crates/x/src/a.rs").as_deref(),
            Some("fn a() {}\n"),
            "the path is resolved against project_root, not the process cwd",
        );
        assert_eq!(
            ctx.read_relative("crates/x/src/absent.rs"),
            None,
            "an absent file is skipped fail-safe",
        );
        assert_eq!(
            ctx.read_relative("crates/x/tests"),
            None,
            "a directory is unreadable — `None`, never a panic",
        );

        std::fs::write(root.path().join("crates/x/src/bad.rs"), [0xFF, 0xFE, 0x00])
            .expect("write non-UTF-8");
        assert_eq!(
            ctx.read_relative("crates/x/src/bad.rs"),
            None,
            "a non-UTF-8 file is skipped fail-safe, not a panic",
        );
    }
}
