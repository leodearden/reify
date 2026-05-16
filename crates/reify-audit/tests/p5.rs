//! Integration tests for the P5 phantom-done detector.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §10):
//!   `cargo test -p reify-audit p5::tests`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary (the binary's own filename is not
//! part of those paths). To make the substring `p5::tests` resolve, the
//! file body is wrapped in `mod p5 { mod tests { ... } }` so each test's
//! path becomes `p5::tests::<name>`.
//!
//! All tests use in-memory rusqlite + MockGitOps so they remain hermetic
//! (no real git repo, no real runs.db file).

mod p5 {

use reify_audit::{
    AuditContext, DoneProvenance, EvidenceRef, Finding, GitCommit, MockGitOps, MockJCodemunchOps,
    Pattern, Severity, TaskMetadata, p5_phantom_done,
};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::PathBuf;

/// Minimal schema — pin reflects only the columns the production
/// `has_task_completed_event` query reads (`events.task_id` and
/// `events.event_type`). P1/P2 detectors (landed via T-2/T-3) issue zero SQL,
/// so they add no columns here; future detectors that DO query the DB will add
/// the columns they need when those queries land.
const RUNS_DB_SCHEMA: &str = r#"
CREATE TABLE events (task_id TEXT, event_type TEXT);
"#;

fn seed_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(RUNS_DB_SCHEMA).expect("create schema");
    conn
}

fn insert_task_completed_event(conn: &Connection, task_id: &str) {
    conn.execute(
        "INSERT INTO events (task_id, event_type) VALUES (?, 'task_completed')",
        rusqlite::params![task_id],
    )
    .unwrap();
}

mod tests {
    use super::*;

    #[test]
    fn happy_path_returns_no_findings() {
        // Seed runs.db with a clean done task: a `task_completed` event exists
        // and metadata.files is corroborated by the claimed merge commit.
        let conn = seed_db();
        insert_task_completed_event(&conn, "1000");

        // MockGitOps reports the claimed commit's diff covers metadata.files
        // and nothing is gitignored.
        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "abc123",
            vec!["crates/x/foo.rs".to_string()],
        );
        git.set_log_grep("main", "1000", vec![]);
        // (gitignored: default false for any path not explicitly set)

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "1000".to_string(),
            TaskMetadata {
                task_id: "1000".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/foo.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("abc123".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert!(
            findings.is_empty(),
            "expected no findings on clean happy-path; got {:?}",
            findings
        );
    }

    /// Pin the public-API surface of every detector type by destructuring
    /// or exhaustively matching each variant. Renaming a field, adding a
    /// variant without an arm, or changing a tuple's arity will fail this
    /// test at compile time — which is exactly what we want from downstream
    /// crates (T-4 CLI, eventual D-1 hook) that depend on a stable shape.
    ///
    /// Unlike a `let _ = Pattern::Variant` sanity-touch, the exhaustive
    /// `match` here forces a test update on enum extensions and the struct
    /// destructure forces one on field additions/renames.
    #[test]
    fn api_surface_pin() {
        // Severity: every variant must be reachable.
        for s in [Severity::Low, Severity::Medium, Severity::High] {
            match s {
                Severity::Low | Severity::Medium | Severity::High => {}
            }
        }

        // Pattern: all FOUR variants; adding another must force this test to gain arms.
        for p in [
            Pattern::P5PhantomDone,
            Pattern::P2ConsumerStub,
            Pattern::P1ProducerOrphan,
            Pattern::MetadataFilesGitignored,
        ] {
            match p {
                Pattern::P5PhantomDone => {}
                Pattern::P2ConsumerStub => {}
                Pattern::P1ProducerOrphan => {}
                Pattern::MetadataFilesGitignored => {}
            }
        }

        // EvidenceRef: every variant exhaustively destructured.
        let refs = [
            EvidenceRef::File {
                path: "x".to_string(),
            },
            EvidenceRef::Commit {
                sha: "s".to_string(),
                subject: "t".to_string(),
            },
            EvidenceRef::MetadataFiles {
                entries: vec!["e".to_string()],
            },
            EvidenceRef::RunsDb {
                table: "events".to_string(),
                key: "k".to_string(),
            },
        ];
        for r in refs {
            match r {
                EvidenceRef::File { path: _ } => {}
                EvidenceRef::Commit { sha: _, subject: _ } => {}
                EvidenceRef::MetadataFiles { entries: _ } => {}
                EvidenceRef::RunsDb { table: _, key: _ } => {}
            }
        }

        // Finding: destructure every field by name.
        let Finding {
            pattern: _,
            severity: _,
            task_id: _,
            summary: _,
            evidence: _,
        } = Finding {
            pattern: Pattern::P5PhantomDone,
            severity: Severity::Low,
            task_id: "0".to_string(),
            summary: "s".to_string(),
            evidence: vec![],
        };

        // GitCommit: destructure every field by name.
        let GitCommit { sha: _, subject: _ } = GitCommit {
            sha: "s".to_string(),
            subject: "t".to_string(),
        };

        // TaskMetadata / DoneProvenance: destructure every field by name.
        let TaskMetadata {
            task_id: _,
            status: _,
            files: _,
            done_provenance: _,
            title: _,
            prd: _,
            consumer_ref: _,
            audit_foundation: _,
            done_at: _,
        } = TaskMetadata {
            task_id: "0".to_string(),
            status: "done".to_string(),
            files: vec![],
            done_provenance: Some(DoneProvenance {
                kind: None,
                commit: None,
                note: None,
            }),
            title: "Wire foo into bar".to_string(),
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: None,
        };
        let DoneProvenance {
            kind: _,
            commit: _,
            note: _,
        } = DoneProvenance {
            kind: None,
            commit: None,
            note: None,
        };
    }

    /// Models the May-09 task 3242 incident
    /// (`~/.claude/projects/-home-leo-src-reify/memory/project_task3242_unblock.md`):
    /// kind=merged, claimed commit not reachable from main, single-file
    /// `crates/reify-shell-extract/src/pruning.rs` edit. The corroborating
    /// diff does NOT cover that path → high-severity phantom-done.
    #[test]
    fn task_3242_shape_returns_high_severity_phantom_done() {
        let conn = seed_db();
        // Task completed event is present (the orchestrator did write one) —
        // the smoking gun is that the *git* corroboration fails, not the DB.
        insert_task_completed_event(&conn, "3242");

        let mut git = MockGitOps::new();
        // log_grep returns empty → no sibling-FF rescue.
        git.set_log_grep("main", "3242", vec![]);
        // diff of the claimed commit against main does NOT include
        // pruning.rs → metadata.files mismatch → phantom-done.
        git.set_diff_changed_paths(
            "main",
            "7958491da22f",
            // Only some unrelated path showed up in the claimed commit.
            vec!["docs/unrelated.md".to_string()],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "3242".to_string(),
            TaskMetadata {
                task_id: "3242".to_string(),
                status: "done".to_string(),
                files: vec!["crates/reify-shell-extract/src/pruning.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("7958491da22f".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one phantom-done finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.task_id, "3242");

        // Evidence must include a MetadataFiles ref naming the missing path.
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence.expect("MetadataFiles evidence present");
        assert!(
            entries
                .iter()
                .any(|p| p == "crates/reify-shell-extract/src/pruning.rs"),
            "expected pruning.rs in MetadataFiles evidence; got {:?}",
            entries
        );
    }

    /// Models the Cargo.lock-only false-positive
    /// (`~/.claude/projects/-home-leo-src-reify/memory/project_post_merge_equivalence_false_positive_cargo_lock.md`):
    /// every "real" metadata.files path is covered by the claimed commit's
    /// diff; only Cargo.lock fell out because main absorbed an unrelated
    /// dependency bump in the meantime. Should downgrade to Severity::Low
    /// rather than scream Severity::High.
    #[test]
    fn cargo_lock_only_divergence_downgrades_to_low() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "2000");

        let mut git = MockGitOps::new();
        // Claimed commit's diff covers ONLY foo.rs — Cargo.lock is missing.
        git.set_diff_changed_paths(
            "main",
            "def456",
            vec!["crates/reify-x/src/foo.rs".to_string()],
        );
        // Sibling-FF rescue is irrelevant for the Cargo.lock case.
        git.set_log_grep("main", "2000", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "2000".to_string(),
            TaskMetadata {
                task_id: "2000".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-x/src/foo.rs".to_string(),
                    "Cargo.lock".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("def456".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding (downgraded); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "Cargo.lock-only divergence must downgrade to Low; got {:?}",
            f.severity
        );
        assert!(
            f.summary.to_lowercase().contains("cargo.lock"),
            "summary should mention Cargo.lock; got {:?}",
            f.summary
        );
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence.expect("MetadataFiles evidence present");
        assert!(
            entries.iter().any(|p| p == "Cargo.lock"),
            "expected Cargo.lock in MetadataFiles evidence; got {:?}",
            entries
        );
    }

    /// Models the convergent-fast-forward false-positive
    /// (`~/.claude/projects/-home-leo-src-reify/memory/project_unblock_convergent_ff_worktree_reap.md`):
    /// the task's branch tip got reaped after a sibling FF; the claimed
    /// commit no longer reaches main, but `git log main --grep <task_id>`
    /// finds the sibling commit whose diff covers all metadata.files.
    /// Should downgrade to Severity::Low and cite the sibling SHA.
    #[test]
    fn convergent_fast_forward_downgrades_to_low() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "4000");

        let mut git = MockGitOps::new();
        // Primary corroboration FAILS: the claimed branch tip diffs empty
        // against main (worktree was reaped, branch tip no longer reachable).
        git.set_diff_changed_paths("main", "old_branch_tip", vec![]);
        // Sibling rescue: log_grep finds a parallel task's SHA whose diff
        // covers both metadata.files entries.
        let sibling = GitCommit {
            sha: "sib_sha_aaaa".to_string(),
            subject: "task 4000 — sibling fast-forward absorbed our work".to_string(),
        };
        git.set_log_grep("main", "4000", vec![sibling.clone()]);
        git.set_diff_changed_paths(
            "main",
            "sib_sha_aaaa",
            vec![
                "crates/x/foo.rs".to_string(),
                "crates/x/bar.rs".to_string(),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4000".to_string(),
            TaskMetadata {
                task_id: "4000".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/x/foo.rs".to_string(),
                    "crates/x/bar.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("found_on_main".to_string()),
                    commit: Some("old_branch_tip".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding (downgraded); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "convergent-FF should downgrade to Low; got {:?}",
            f.severity
        );
        let s = f.summary.to_lowercase();
        assert!(
            s.contains("convergent") || s.contains("sibling") || s.contains("fast-forward"),
            "summary should mention convergent / sibling / fast-forward; got {:?}",
            f.summary
        );
        // Evidence must cite the sibling commit SHA.
        let cited_sibling = f.evidence.iter().any(|e| match e {
            EvidenceRef::Commit { sha, .. } => sha == "sib_sha_aaaa",
            _ => false,
        });
        assert!(
            cited_sibling,
            "expected EvidenceRef::Commit citing sib_sha_aaaa; got {:?}",
            f.evidence
        );
    }

    /// Models the gitignored-metadata.files false-positive
    /// (`~/.claude/projects/-home-leo-src-reify/memory/project_steward_metadata_files_gitignore_falsepositive.md`):
    /// metadata.files contains a generated/gitignored path that "looks
    /// missing" because it's not committed. Even when corroboration is
    /// otherwise clean, flag a separate Severity::Medium finding so the
    /// user knows to strip the gitignored entry.
    #[test]
    fn gitignored_metadata_files_flagged() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "6000");

        let mut git = MockGitOps::new();
        // Corroboration is otherwise clean: diff covers BOTH files.
        git.set_diff_changed_paths(
            "main",
            "abc123",
            vec![
                "tree-sitter-reify/src/parser.c".to_string(),
                "real/file.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "6000", vec![]);
        // The generated parser.c is gitignored; real source isn't.
        git.set_is_gitignored("tree-sitter-reify/src/parser.c", true);
        git.set_is_gitignored("real/file.rs", false);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6000".to_string(),
            TaskMetadata {
                task_id: "6000".to_string(),
                status: "done".to_string(),
                files: vec![
                    "tree-sitter-reify/src/parser.c".to_string(),
                    "real/file.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("abc123".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one (gitignore) finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::MetadataFilesGitignored);
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.task_id, "6000");
        assert!(
            f.summary.to_lowercase().contains("gitignored"),
            "summary should mention gitignored entry; got {:?}",
            f.summary
        );
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence.expect("MetadataFiles evidence present");
        assert!(
            entries.iter().any(|p| p == "tree-sitter-reify/src/parser.c"),
            "expected parser.c in MetadataFiles evidence; got {:?}",
            entries
        );
        assert!(
            !entries.iter().any(|p| p == "real/file.rs"),
            "real/file.rs is NOT gitignored — should not appear; got {:?}",
            entries
        );
    }

    /// Seeds one `task_completed` row and asserts the production query (sourced
    /// from `p5_phantom_done::PRODUCTION_QUERY`) prepares and matches the row
    /// against the seeded `RUNS_DB_SCHEMA`. Fails if the schema or the query
    /// string drift apart (column rename, type swap, query rewrite). Column
    /// additions do not fail this test (they'd be caught by future detectors'
    /// own tests when they add the columns they need).
    #[test]
    fn runs_db_schema_pin() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "test-task");
        let mut stmt = conn
            .prepare(p5_phantom_done::PRODUCTION_QUERY)
            .expect("production query must prepare against seeded schema");
        let found: Option<i64> = stmt
            .query_row(["test-task"], |r| r.get(0))
            .optional()
            .expect("query_row");
        assert_eq!(found, Some(1));
    }

    /// Coverage gap pin — verifies the Err arm of `has_task_completed_event`
    /// survives future refactors. No production-code change required; this
    /// branch was added in amend e5e8932cb6 but never had a direct test.
    ///
    /// Trigger: open a bare `Connection::open_in_memory()` WITHOUT seeding the
    /// schema. `stmt.prepare("SELECT 1 FROM events ...")` returns
    /// `Err(SqliteFailure(... "no such table: events" ...))`, which exercises
    /// the Err arm → Medium "runs.db unreadable" finding with EvidenceRef::RunsDb.
    #[test]
    fn runs_db_unreadable_emits_medium_breadcrumb() {
        // Bare connection — no schema seeded, so `events` table does not exist.
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");

        let git = MockGitOps::new();
        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "8000".to_string(),
            TaskMetadata {
                task_id: "8000".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/foo.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("abc123".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding (runs.db unreadable); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "unreadable runs.db must emit Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "8000");
        assert!(
            f.summary.to_lowercase().contains("runs.db unreadable"),
            "summary should mention 'runs.db unreadable'; got {:?}",
            f.summary
        );
        // Evidence must be RunsDb citing the events table and task_id.
        let runsdb_ev = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::RunsDb { table, key } => Some((table, key)),
            _ => None,
        });
        let (table, key) = runsdb_ev.expect("EvidenceRef::RunsDb must be present");
        assert_eq!(table, "events");
        assert!(
            key.contains("8000"),
            "RunsDb key should contain task_id '8000'; got {:?}",
            key
        );
    }

    /// Degenerate Cargo.lock-only case: when `metadata.files` contains ONLY
    /// `Cargo.lock` (no other entries to corroborate), the `is_cargo_lock_only`
    /// guard's precondition is violated — there are no "other entries" that
    /// were covered by the diff to validate the low-noise claim. Should NOT
    /// downgrade to Low; instead fall through to sibling-FF rescue (empty) and
    /// emit High. See S1.5 in the carry-forward plan.
    #[test]
    fn cargo_lock_only_with_single_metadata_file_does_not_downgrade() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "7000");

        let mut git = MockGitOps::new();
        // Empty primary diff: claimed commit covers nothing, so Cargo.lock is
        // in `missing`. No sibling-FF rescue either.
        git.set_diff_changed_paths("main", "degenerate_sha", vec![]);
        git.set_log_grep("main", "7000", vec![]);
        // (gitignored: default false for any path not explicitly set)

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7000".to_string(),
            TaskMetadata {
                task_id: "7000".to_string(),
                status: "done".to_string(),
                // Only one file: Cargo.lock. No corroborating entries exist.
                files: vec!["Cargo.lock".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("degenerate_sha".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(
            f.severity,
            Severity::High,
            "degenerate single-file Cargo.lock must NOT downgrade to Low; got {:?}",
            f.severity
        );
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert!(
            f.summary.to_lowercase().contains("mismatch")
                || f.summary.to_lowercase().contains("not reachable"),
            "summary should mention mismatch or not reachable; got {:?}",
            f.summary
        );
    }

    /// `check_pre_done` is the entry point for the eventual D-1 dark-factory
    /// hook (see `f-infra-design.md` §11). It must scope to a single task_id
    /// even when other phantom-done tasks coexist in `task_metadata`.
    #[test]
    fn check_pre_done_filters_to_single_task() {
        let conn = seed_db();
        // Phantom-done task 5000 (task-3242 shape).
        insert_task_completed_event(&conn, "5000");
        // Clean done task 5001 (happy-path shape).
        insert_task_completed_event(&conn, "5001");

        let mut git = MockGitOps::new();
        // Task 5000: claimed commit's diff doesn't cover the metadata file.
        git.set_diff_changed_paths(
            "main",
            "phantom_sha",
            vec!["docs/unrelated.md".to_string()],
        );
        git.set_log_grep("main", "5000", vec![]);
        // Task 5001: claimed commit's diff covers the metadata file cleanly.
        git.set_diff_changed_paths(
            "main",
            "clean_sha",
            vec!["crates/x/clean.rs".to_string()],
        );
        git.set_log_grep("main", "5001", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "5000".to_string(),
            TaskMetadata {
                task_id: "5000".to_string(),
                status: "done".to_string(),
                files: vec!["crates/reify-shell-extract/src/pruning.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("phantom_sha".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );
        task_metadata.insert(
            "5001".to_string(),
            TaskMetadata {
                task_id: "5001".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/clean.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("clean_sha".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        // Sanity: full check returns the single phantom finding.
        let all = p5_phantom_done::check(&ctx);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].task_id, "5000");

        // Scoped to "5000" → returns the phantom finding.
        let phantom_only = p5_phantom_done::check_pre_done(&ctx, "5000");
        assert_eq!(
            phantom_only.len(),
            1,
            "check_pre_done(5000) should return only 5000's finding; got {:?}",
            phantom_only
        );
        assert_eq!(phantom_only[0].task_id, "5000");
        assert_eq!(phantom_only[0].severity, Severity::High);

        // Scoped to "5001" (clean) → returns empty.
        let clean_only = p5_phantom_done::check_pre_done(&ctx, "5001");
        assert!(
            clean_only.is_empty(),
            "check_pre_done(5001) should be empty; got {:?}",
            clean_only
        );
    }
}

} // mod p5
