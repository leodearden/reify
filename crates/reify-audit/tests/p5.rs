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

mod common;

mod p5 {

use crate::common::schema::{seed_db, insert_event, insert_task_completed_event};
use reify_audit::{
    AuditContext, DoneProvenance, EvidenceRef, Finding, GitCommit, MockGitOps, MockJCodemunchOps,
    Pattern, Severity, TaskMetadata, p5_phantom_done,
};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::PathBuf;

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
            producer_branch: None,
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

        // Pattern: every variant; adding another must force this test to gain arms.
        for p in [
            Pattern::P5PhantomDone,
            Pattern::P2ConsumerStub,
            Pattern::P1ProducerOrphan,
            Pattern::P5MetadataFilesGitignored,
            Pattern::PDeadCode,
            Pattern::PUntested,
            Pattern::PLayerViolation,
            Pattern::P5TestsAssertEmpty,
            Pattern::P5LivePathStranded,
            Pattern::PTodo,
            Pattern::PDsSentinel,
            Pattern::PDocCover,
        ] {
            match p {
                Pattern::P5PhantomDone => {}
                Pattern::P2ConsumerStub => {}
                Pattern::P1ProducerOrphan => {}
                Pattern::P5MetadataFilesGitignored => {}
                Pattern::PDeadCode => {}
                Pattern::PUntested => {}
                Pattern::PLayerViolation => {}
                Pattern::P5TestsAssertEmpty => {}
                Pattern::P5LivePathStranded => {}
                Pattern::PTodo => {}
                Pattern::PDsSentinel => {}
                Pattern::PDocCover => {}
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
            producer_branch: None,
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
            producer_branch: None,
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
            producer_branch: None,
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one (gitignore) finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5MetadataFilesGitignored);
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

    /// Pins both clauses of the production query (sourced from
    /// `p5_phantom_done::PRODUCTION_QUERY`) against the seeded `RUNS_DB_SCHEMA`.
    ///
    /// Three assertions together enforce the full contract:
    /// 1. **Positive case** — `('test-task', 'task_completed')` row → `Some(1)`.
    ///    Confirms the query prepares and executes against the seeded schema.
    /// 2. **Negative-case 1** — querying for `'missing-task'` → `None`.
    ///    Pins `task_id = ?`: no row exists with that task_id, so the bind
    ///    parameter must be enforced.
    /// 3. **Negative-case 2** — querying for `'other-task'` (which has a
    ///    `'task_started'` row) → `None`.  Pins `AND event_type = 'task_completed'`:
    ///    if that clause is dropped, the query matches the `task_started` row and
    ///    returns `Some(1)`, failing this assertion.
    ///
    /// Fails if the schema or the query string drift apart (column rename, type
    /// swap, query rewrite). Column additions do not fail this test (they'd be
    /// caught by future detectors' own tests when they add the columns they need).
    #[test]
    fn runs_db_schema_pin() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "test-task");
        // Also seed a row with a different task_id AND a different event_type
        // ('task_started'). This is the control row for the event_type-filter
        // assertion below.
        insert_event(&conn, "other-task", "task_started");

        let mut stmt = conn
            .prepare(p5_phantom_done::PRODUCTION_QUERY)
            .expect("production query must prepare against seeded schema");
        let found: Option<i64> = stmt
            .query_row(["test-task"], |r| r.get(0))
            .optional()
            .expect("query_row");
        assert_eq!(found, Some(1));

        // Negative-case 1 — task_id bind parameter:
        // 'missing-task' has no row at all, so the query returns None regardless
        // of event_type.  Pins `task_id = ?`.
        let not_found: Option<i64> = stmt
            .query_row(["missing-task"], |r| r.get(0))
            .optional()
            .expect("query_row negative case");
        assert_eq!(not_found, None);

        // Negative-case 2 — event_type filter:
        // 'other-task' has a row, but its event_type is 'task_started', not
        // 'task_completed'.  The query must return None, confirming
        // `AND event_type = 'task_completed'` is enforced.  If that clause is
        // dropped from PRODUCTION_QUERY, the unfiltered query matches the
        // 'task_started' row and returns Some(1), failing this assertion.
        let other_event_type: Option<i64> = stmt
            .query_row(["other-task"], |r| r.get(0))
            .optional()
            .expect("query_row event_type filter case");
        assert_eq!(other_event_type, None);
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
            producer_branch: None,
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
            producer_branch: None,
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
            producer_branch: None,
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

    /// Pins the structural-equivalence contract that `check_pre_done(ctx, id)`
    /// and `check(ctx)` with `target_task_id = Some(id)` must produce identical
    /// findings for the same single-task scenario — the property the `check_task`
    /// helper extraction enforces by construction. Without this pin, a future
    /// per-task pass added to only one of the two call sites could silently drift.
    ///
    /// Task 5001 is deliberately set up as a *second* phantom-done case so that
    /// the `target_task_id` filter is genuinely load-bearing: without the filter,
    /// `scoped_findings` would contain both 5000 and 5001. The assertion that
    /// `scoped_findings.len() == 1` therefore validates both equivalence *and*
    /// that the target filter actually excludes 5001.
    ///
    /// This test passes today (before/after the refactor) because both call sites
    /// already implement equivalent logic; it future-proofs against drift if one
    /// site grows a new pass that the other forgets. It also fills a real coverage
    /// gap: no existing test exercises `target_task_id: Some(...)`.
    #[test]
    fn check_pre_done_equivalent_to_scoped_check() {
        let conn = seed_db();
        // Phantom-done task 5000 — same shape as check_pre_done_filters_to_single_task.
        insert_task_completed_event(&conn, "5000");
        // Phantom-done task 5001 — deliberately a second phantom so the target filter
        // is load-bearing (without it scoped_findings would contain both 5000 and 5001).
        insert_task_completed_event(&conn, "5001");

        let mut git = MockGitOps::new();
        // Task 5000: claimed commit's diff doesn't cover the metadata file.
        git.set_diff_changed_paths(
            "main",
            "phantom_sha",
            vec!["docs/unrelated.md".to_string()],
        );
        git.set_log_grep("main", "5000", vec![]);
        // Task 5001: second phantom-done case — diff does NOT cover its metadata
        // file. This makes the target_task_id filter load-bearing: without it,
        // scoped_findings would contain both 5000 and 5001.
        git.set_diff_changed_paths(
            "main",
            "clean_sha",
            vec!["docs/unrelated2.md".to_string()],
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

        // ctx_a: no target filter — passed to check_pre_done with explicit task_id.
        let ctx_a = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: task_metadata.clone(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        // ctx_b: scoped to task 5000 — passed to check (the periodic-sweep entry point).
        let ctx_b = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: Some("5000".to_string()),
            window: None,
            now: None,
            producer_branch: None,
        };

        let pre_done_findings = p5_phantom_done::check_pre_done(&ctx_a, "5000");
        let scoped_findings = p5_phantom_done::check(&ctx_b);

        // Both paths must agree on finding count for task 5000.
        assert_eq!(
            pre_done_findings.len(),
            scoped_findings.len(),
            "check_pre_done and scoped check must return the same number of findings; \
             pre_done={:?}, scoped={:?}",
            pre_done_findings,
            scoped_findings
        );
        assert_eq!(
            pre_done_findings.len(),
            1,
            "expected exactly one phantom-done finding; got {:?}",
            pre_done_findings
        );

        // Full Finding equality — `Finding` derives `PartialEq`, so all fields
        // (task_id, severity, pattern, summary, evidence) are covered. Any
        // future field added to `Finding` is automatically included in this pin;
        // field-by-field comparisons would silently miss newly added fields.
        assert_eq!(pre_done_findings, scoped_findings);
    }

    /// A `TaskMetadata` in the exact state the D-1 pre-done hook observes:
    /// pre-transition `status`, and `done_provenance: None` because the
    /// interceptor has not written it yet.
    fn pre_done_meta(task_id: &str, status: &str, files: &[&str]) -> TaskMetadata {
        TaskMetadata {
            task_id: task_id.to_string(),
            status: status.to_string(),
            files: files.iter().map(|s| s.to_string()).collect(),
            done_provenance: None,
            title: format!("Task {}", task_id),
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: None,
        }
    }

    /// A gitignored `metadata.files` entry must never refuse a done-flip.
    ///
    /// A gitignored path can never resolve on main *by construction*, so
    /// letting it block a fail-closed state transition is a guaranteed false
    /// positive (memory:
    /// `project_steward_metadata_files_gitignore_falsepositive.md`). The
    /// gitignored aspect already has its own dedicated breadcrumb — the
    /// `P5MetadataFilesGitignored` Medium from `check_gitignored` — which is
    /// the correct channel for it.
    ///
    /// Note the deliberate divergence from the SWEEP, which does NOT exclude
    /// gitignored entries from `genuinely_absent` (see the comment above the
    /// deliverable-presence rescue in `p5_phantom_done.rs`). The gate diverges
    /// on purpose: refusing a state transition is a far costlier error than a
    /// Low sweep finding.
    #[test]
    fn pre_done_gate_does_not_refuse_on_gitignored_entry() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        git.set_path_tracked_on("main", "crates/reify-audit/src/lib.rs", true);
        git.set_is_gitignored("target/debug/generated.rs", true);
        // `path_tracked_on` for the gitignored path stays at its `false`
        // default — a build artefact is never tracked on main.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345G".to_string(),
            pre_done_meta(
                "6345G",
                "in-progress",
                &["crates/reify-audit/src/lib.rs", "target/debug/generated.rs"],
            ),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345G");

        let highs: Vec<&Finding> =
            findings.iter().filter(|f| f.severity == Severity::High).collect();
        assert!(
            highs.is_empty(),
            "a gitignored metadata.files entry must not refuse the done-flip; got {:?}",
            highs
        );
        assert_eq!(
            findings.len(),
            1,
            "expected exactly the gitignored breadcrumb; got {:?}",
            findings
        );
        assert_eq!(findings[0].pattern, Pattern::P5MetadataFilesGitignored);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    /// The ALL-gitignored declared set must reach `check_pre_done_landing`'s
    /// `declared.is_empty()` early return, not the `absent.is_empty()` one.
    ///
    /// `cli-invocation.md` promises the gate "never refuses" on "a list
    /// consisting solely of gitignored entries", and the code implements that
    /// by `retain`-ing the gitignored subset away FIRST and returning on the
    /// resulting empty set. The sibling test
    /// `pre_done_gate_does_not_refuse_on_gitignored_entry` mixes one gitignored
    /// entry with one tracked-on-main entry, so it exits through the LATER
    /// `absent.is_empty()` leg and leaves the promised branch unexecuted — a
    /// refactor that moved the gitignored filter after the tracked-on-main
    /// filter would keep it green while refusing every all-gitignored flip.
    ///
    /// Here the single declared entry is gitignored AND not tracked on main
    /// (the `path_tracked_on` default), so only the first return can produce
    /// the empty result.
    #[test]
    fn pre_done_gate_does_not_refuse_when_every_entry_is_gitignored() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        git.set_is_gitignored("target/debug/generated.rs", true);
        // `path_tracked_on` stays false and `is_ancestor("main", "main")` stays
        // false: if control ever reached the refusal road, the MAIN_BASE probe
        // would fire and this test would see an advisory Low — so a green run
        // proves the gitignored filter ran FIRST.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345GALL".to_string(),
            pre_done_meta("6345GALL", "in-progress", &["target/debug/generated.rs"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345GALL");

        assert!(
            findings.iter().all(|f| f.pattern != Pattern::P5PhantomDone),
            "a declared set consisting solely of gitignored entries must never \
             produce a pre-done refusal at ANY severity; got {:?}",
            findings
        );
        assert_eq!(
            findings.len(),
            1,
            "expected exactly the gitignored breadcrumb; got {:?}",
            findings
        );
        assert_eq!(findings[0].pattern, Pattern::P5MetadataFilesGitignored);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    /// Defect 2, on the SWEEP path: a task whose claimed commit has LANDED
    /// must corroborate via that commit's own delta, not via `main..<commit>`.
    ///
    /// `main..<commit>` is a two-point TREE diff. Once `<commit>` is an
    /// ancestor of main the two trees agree on exactly the paths the commit
    /// introduced, so the task's own files are EXCLUDED by construction and
    /// what comes back is the reverse-delta of whatever landed afterwards.
    /// MEASURED on the live repo: for merge `bc8f74a4d4`, `main..M` returned
    /// 6 paths and all six of that task's own files were absent from the set.
    ///
    /// The fixture reproduces exactly that shape: the claimed commit IS an
    /// ancestor, the degenerate reverse-delta names only an unrelated later
    /// merge's file, and there is no sibling rescue available — so the primary
    /// leg alone must decide, and it must decide correctly.
    #[test]
    fn landed_ancestor_commit_corroborates_via_its_own_delta() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "6345A");

        let mut git = MockGitOps::new();
        // The claimed commit is on main.
        git.set_is_ancestor("mergesha", "main", true);
        // The degenerate reverse-delta: an unrelated later merge's file, and
        // NOT the task's own.
        git.set_diff_changed_paths(
            "main",
            "mergesha",
            vec!["crates/reify-other/src/unrelated.rs".to_string()],
        );
        // The true delta.
        git.set_changed_paths_in_commit(
            "mergesha",
            vec!["crates/reify-x/src/landed.rs".to_string()],
        );
        // No sibling rescue — the primary leg alone must decide.
        git.set_log_grep("main", "6345A", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345A".to_string(),
            TaskMetadata {
                task_id: "6345A".to_string(),
                status: "done".to_string(),
                files: vec!["crates/reify-x/src/landed.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("mergesha".to_string()),
                    note: None,
                }),
                title: "A task whose merge commit has landed".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert!(
            findings.is_empty(),
            "a task whose landed merge commit's own delta covers metadata.files must \
             produce no finding — reading the degenerate main..<commit> instead is the \
             false-High this fix removes; got {:?}",
            findings
        );
    }

    /// A `log_grep` rescue must only fire on a commit that references the task
    /// id as a WHOLE NUMBER.
    ///
    /// `git log --grep=<id>` is a bare substring match, so `--grep=593` matches
    /// "Merge task/5937 into main". The pure-reachability fallback fires
    /// whenever `siblings` is non-empty, so a short or colliding id silently
    /// downgrades a genuine phantom-done to Low — a FALSE NEGATIVE. (The code
    /// comments recorded this as an accepted task-4464 bias toward fewer
    /// false-Highs; this task is explicitly the false-negative direction, so
    /// the bias is now inverted.)
    ///
    /// The positive control in the same test proves the filter NARROWS the
    /// match rather than disabling the rescue.
    #[test]
    fn log_grep_rescue_requires_task_id_digit_boundary() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "593");
        insert_task_completed_event(&conn, "5937");

        let mut git = MockGitOps::new();

        // --- the collision: `--grep=593` also matches 5937 and 5930 ---
        git.set_log_grep(
            "main",
            "593",
            vec![
                GitCommit {
                    sha: "c7f659fd6a".to_string(),
                    subject: "Merge task/5937 into main".to_string(),
                },
                GitCommit {
                    sha: "aa11bb22".to_string(),
                    subject: "Merge task/5930 into main".to_string(),
                },
            ],
        );
        // Neither colliding commit covers task 593's deliverable.
        git.set_diff_changed_paths(
            "main",
            "c7f659fd6a",
            vec!["crates/reify-other/src/a.rs".to_string()],
        );
        git.set_diff_changed_paths(
            "main",
            "aa11bb22",
            vec!["crates/reify-other/src/b.rs".to_string()],
        );
        // The claimed commit covers nothing.
        git.set_diff_changed_paths("main", "staleref", vec![]);

        // --- the positive control: task 5937's OWN landing commit ---
        git.set_log_grep(
            "main",
            "5937",
            vec![GitCommit {
                sha: "c7f659fd6a".to_string(),
                subject: "Merge task/5937 into main".to_string(),
            }],
        );
        git.set_diff_changed_paths("main", "staleref5937", vec![]);

        // `path_tracked_on` and `is_ancestor` stay at their `false` defaults for
        // every path/commit — no other rescue is available to either task.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "593".to_string(),
            TaskMetadata {
                task_id: "593".to_string(),
                status: "done".to_string(),
                files: vec!["crates/reify-x/src/never_landed.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("staleref".to_string()),
                    note: None,
                }),
                title: "A genuine phantom-done with a short id".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );
        task_metadata.insert(
            "5937".to_string(),
            TaskMetadata {
                task_id: "5937".to_string(),
                status: "done".to_string(),
                files: vec!["crates/reify-y/src/landed_via_sibling.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("staleref5937".to_string()),
                    note: None,
                }),
                title: "The task the colliding subject actually names".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);

        let for_593: Vec<&Finding> = findings.iter().filter(|f| f.task_id == "593").collect();
        assert_eq!(
            for_593.len(),
            1,
            "expected exactly one finding for task 593; got {:?}",
            for_593
        );
        assert_eq!(for_593[0].pattern, Pattern::P5PhantomDone);
        assert_eq!(
            for_593[0].severity,
            Severity::High,
            "\"Merge task/5937 into main\" must NOT rescue task 593 — a substring \
             collision is not landing evidence; got {:?}",
            for_593[0]
        );

        let for_5937: Vec<&Finding> = findings.iter().filter(|f| f.task_id == "5937").collect();
        assert_eq!(
            for_5937.len(),
            1,
            "expected exactly one finding for task 5937; got {:?}",
            for_5937
        );
        assert_eq!(
            for_5937[0].severity,
            Severity::Low,
            "the filter must NARROW the rescue, not disable it: the same subject \
             \"Merge task/5937 into main\" must still rescue task 5937; got {:?}",
            for_5937[0]
        );
    }

    /// The pre-done gate must accept a task whose deliverable was DELETED (or
    /// renamed away) by its landing commit.
    ///
    /// This is why defects 1 and 2 had to land together. A removed file has
    /// `path_tracked_on(main, p) == false`, so the tracked-on-main leg alone
    /// would refuse every removal/refactor task. Only the landing commit's own
    /// delta (`<merge>^1..<merge>`) shows a deletion — `main..<merge>` can
    /// never show it, because a path absent from both trees is not in a
    /// two-point diff at all.
    ///
    /// Case (b) pins that the rescue NARROWS the gate rather than defeating
    /// it: a landing commit that touched something else entirely is not
    /// evidence that this task's deliverable landed.
    #[test]
    fn pre_done_gate_accepts_deletion_task_via_landing_commit() {
        let conn = seed_db();

        let mut git = MockGitOps::new();

        // The repo-health probe: `is_ancestor(main, main)` is exit-0 iff `main`
        // resolves. A pre-done refusal is only allowed to fire on a healthy
        // repo — without this the gate reports an advisory Low ("git degraded")
        // instead, which is exactly the fail-safe that keeps an infra hiccup
        // from blocking a legitimate done-flip.
        git.set_is_ancestor("main", "main", true);

        // (a) the deletion task — its landing commit removed the declared file.
        git.set_log_grep(
            "main",
            "6345D",
            vec![GitCommit {
                sha: "mergesha".to_string(),
                subject: "Merge task/6345D into main".to_string(),
            }],
        );
        git.set_is_ancestor("mergesha", "main", true);
        git.set_changed_paths_in_commit(
            "mergesha",
            vec!["crates/reify-x/src/removed.rs".to_string()],
        );

        // (b) same shape, but the landing commit's delta covers an unrelated
        // path only — no evidence this task's deliverable landed.
        git.set_log_grep(
            "main",
            "6345E",
            vec![GitCommit {
                sha: "mergeshaE".to_string(),
                subject: "Merge task/6345E into main".to_string(),
            }],
        );
        git.set_is_ancestor("mergeshaE", "main", true);
        git.set_changed_paths_in_commit(
            "mergeshaE",
            vec!["crates/reify-other/src/something_else.rs".to_string()],
        );

        // `path_tracked_on` stays at its `false` default for both declared
        // files — that is the whole point: the file is gone from main.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345D".to_string(),
            pre_done_meta("6345D", "review", &["crates/reify-x/src/removed.rs"]),
        );
        task_metadata.insert(
            "6345E".to_string(),
            pre_done_meta("6345E", "review", &["crates/reify-x/src/never_landed.rs"]),
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
            producer_branch: None,
        };

        // (a) must PASS the gate.
        let findings = p5_phantom_done::check_pre_done(&ctx, "6345D");
        assert!(
            findings.is_empty(),
            "a task whose landing commit DELETED its declared file must pass the \
             pre-done gate — path_tracked_on is false by construction for a removal; \
             got {:?}",
            findings
        );

        // (b) must still be REFUSED.
        let findings = p5_phantom_done::check_pre_done(&ctx, "6345E");
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one refusal for 6345E; got {:?}",
            findings
        );
        assert_eq!(findings[0].pattern, Pattern::P5PhantomDone);
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "a landing commit that touched something else entirely must not rescue \
             an absent deliverable; got {:?}",
            findings[0]
        );
        assert!(
            findings[0].evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::MetadataFiles { entries }
                    if entries == &vec!["crates/reify-x/src/never_landed.rs".to_string()]
            )),
            "the refusal must cite the still-absent entry; got {:?}",
            findings[0].evidence
        );
    }

    /// The deletion rescue must be DIRECTORY-aware, mirroring `path_tracked_on`.
    ///
    /// `git ls-tree main -- <dir>` resolves a directory entry, so a
    /// `metadata.files` entry may legitimately name a directory. A
    /// `--name-only` delta, by contrast, lists the individual files beneath it
    /// and never the directory itself — so matching the delta by exact string
    /// equality refuses a task whose declared DIRECTORY was removed or renamed
    /// away by its own landing commit, even though the removal is right there
    /// in the delta.
    ///
    /// Case (b) pins that the prefix match is anchored: a sibling directory
    /// with a shared textual prefix (`crates/reify-x/src/gone_too/`) must not
    /// satisfy a declared `crates/reify-x/src/gone` entry.
    ///
    /// Case (c) is the same directory written WITH a trailing slash —
    /// `metadata.files` is hand-authored and nothing normalises it. Without
    /// trimming, the anchor check indexes the byte after the slash and can
    /// never see a `/`, so the entry stays absent and the flip is refused for
    /// work that did land. The healthy `path_tracked_on` leg cannot mask it:
    /// `git ls-tree main -- <dir>/` is equally empty once the directory is
    /// gone from main.
    #[test]
    fn pre_done_gate_accepts_deleted_directory_entry_via_landing_commit() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        git.set_is_ancestor("main", "main", true);

        // (a) the declared entry is a DIRECTORY the landing commit removed;
        // the delta lists the files that were under it.
        git.set_log_grep(
            "main",
            "6345DIR",
            vec![GitCommit {
                sha: "dirmerge".to_string(),
                subject: "Merge task/6345DIR into main".to_string(),
            }],
        );
        git.set_is_ancestor("dirmerge", "main", true);
        git.set_changed_paths_in_commit(
            "dirmerge",
            vec![
                "crates/reify-x/src/gone/mod.rs".to_string(),
                "crates/reify-x/src/gone/inner/impl.rs".to_string(),
            ],
        );

        // (b) the delta only touches a DIFFERENT directory that happens to
        // share a textual prefix with the declared one.
        git.set_log_grep(
            "main",
            "6345PFX",
            vec![GitCommit {
                sha: "pfxmerge".to_string(),
                subject: "Merge task/6345PFX into main".to_string(),
            }],
        );
        git.set_is_ancestor("pfxmerge", "main", true);
        git.set_changed_paths_in_commit(
            "pfxmerge",
            vec!["crates/reify-x/src/gone_too/mod.rs".to_string()],
        );

        // (c) the SAME removed directory as (a), but declared with a trailing
        // slash — the form `metadata.files` may legitimately carry.
        git.set_log_grep(
            "main",
            "6345SLASH",
            vec![GitCommit {
                sha: "slashmerge".to_string(),
                subject: "Merge task/6345SLASH into main".to_string(),
            }],
        );
        git.set_is_ancestor("slashmerge", "main", true);
        git.set_changed_paths_in_commit(
            "slashmerge",
            vec!["crates/reify-x/src/gone/mod.rs".to_string()],
        );

        // `path_tracked_on` stays false for both declared entries — the
        // directory is gone from main, which is the whole premise.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345DIR".to_string(),
            pre_done_meta("6345DIR", "review", &["crates/reify-x/src/gone"]),
        );
        task_metadata.insert(
            "6345PFX".to_string(),
            pre_done_meta("6345PFX", "review", &["crates/reify-x/src/gone"]),
        );
        task_metadata.insert(
            "6345SLASH".to_string(),
            pre_done_meta("6345SLASH", "review", &["crates/reify-x/src/gone/"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345DIR");
        assert!(
            findings.is_empty(),
            "a declared DIRECTORY removed by the task's own landing commit must pass \
             the pre-done gate — the delta lists the files beneath it, never the \
             directory itself; got {:?}",
            findings
        );

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345PFX");
        assert_eq!(
            findings.len(),
            1,
            "a sibling directory sharing a textual prefix must not satisfy the \
             declared entry — the prefix match must be anchored at a `/`; got {:?}",
            findings
        );
        assert_eq!(findings[0].severity, Severity::High);

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345SLASH");
        assert!(
            findings.is_empty(),
            "a declared directory written WITH a trailing slash must behave exactly \
             like the un-slashed form — the trailing separator is not evidence that \
             the work failed to land; got {:?}",
            findings
        );
    }

    /// A pre-done refusal must rest on evidence actually gathered.
    ///
    /// Every git leg in this crate fail-safes to `false`/empty on error. In the
    /// SWEEP that converges on "no finding"; on the pre-done path it converges
    /// on a High that BLOCKS a state transition, so an infra hiccup would be
    /// indistinguishable from a genuine phantom-done. The `MAIN_BASE` probe
    /// inverts that: with git degraded the finding is still emitted (visible)
    /// but as an advisory Low, which cannot block the flip.
    #[test]
    fn pre_done_gate_degraded_git_downgrades_to_advisory_low() {
        let conn = seed_db();

        // Nothing is set on the mock at all: every git call fail-safes exactly
        // as `RealGitOps` does when the repo/ref/binary is unavailable, and
        // `is_ancestor("main", "main")` is false — the probe's failure signal.
        let git = MockGitOps::new();

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345INFRA".to_string(),
            pre_done_meta("6345INFRA", "review", &["crates/reify-x/src/thing.rs"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345INFRA");
        assert_eq!(
            findings.len(),
            1,
            "the degraded-git case must still be VISIBLE, not silent; got {:?}",
            findings
        );
        assert_eq!(findings[0].pattern, Pattern::P5PhantomDone);
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "a refusal built on a git failure must not block a done-flip; got {:?}",
            findings[0]
        );
        assert!(
            findings[0].summary.contains("[advisory"),
            "the downgrade reason must be legible in the summary; got {:?}",
            findings[0].summary
        );
    }

    /// A PER-CALL `git ls-tree` failure must not manufacture a refusal.
    ///
    /// The whole-repo `MAIN_BASE` probe is not sufficient on its own: it
    /// cannot distinguish a per-call git failure from a genuine observation.
    /// `path_tracked_on` fail-safes to `false` on ANY `ls-tree` error
    /// (unreadable pack, fd exhaustion under orchestrator load, index
    /// contention), so with `main` still resolving, one transient failure plus
    /// an empty `log_grep` yielded a BLOCKING High against a legitimate
    /// done-flip — the exact fail-safe inversion the guards exist to close.
    ///
    /// The control task in the same fixture is byte-identical except that git
    /// ANSWERS "not tracked" instead of failing, and must still be refused at
    /// High — so this test cannot pass by simply muting the gate.
    #[test]
    fn pre_done_gate_ls_tree_failure_downgrades_to_advisory_low() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        // The repo is HEALTHY: `main` resolves, so the MAIN_BASE probe passes
        // and cannot be what downgrades the finding.
        git.set_is_ancestor("main", "main", true);

        // (a) git FAILED for the declared entry — the question is unanswered.
        git.set_path_tracked_on_error(
            "main",
            "crates/reify-x/src/flaky.rs",
            "git exited Some(128): fatal: unable to read tree",
        );
        git.set_log_grep("main", "6345LSERR", vec![]);

        // (b) control: git ANSWERED "not tracked" for an identical-shaped task.
        git.set_path_tracked_on("main", "crates/reify-x/src/absent.rs", false);
        git.set_log_grep("main", "6345LSOK", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345LSERR".to_string(),
            pre_done_meta("6345LSERR", "review", &["crates/reify-x/src/flaky.rs"]),
        );
        task_metadata.insert(
            "6345LSOK".to_string(),
            pre_done_meta("6345LSOK", "review", &["crates/reify-x/src/absent.rs"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345LSERR");
        assert_eq!(
            findings.len(),
            1,
            "a degraded ls-tree must stay VISIBLE, not silent; got {:?}",
            findings
        );
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "a refusal resting on an ls-tree that FAILED must not block a done-flip \
             — `false` from a failed `git ls-tree` is not evidence of absence; \
             got {:?}",
            findings[0]
        );
        assert!(
            findings[0].summary.contains("[advisory")
                && findings[0].summary.contains("ls-tree"),
            "the failing seam must be named in the summary so an operator can \
             re-check it by hand; got {:?}",
            findings[0].summary
        );

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345LSOK");
        assert_eq!(findings.len(), 1, "expected one refusal; got {:?}", findings);
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "git ANSWERING \"not tracked\" is evidence and must still refuse — \
             otherwise the fix above has merely muted the gate; got {:?}",
            findings[0]
        );
    }

    /// A `git log --grep` failure must not manufacture a refusal either.
    ///
    /// `log_grep` fail-safes to an empty vec, which empties the rescue
    /// candidate list — so a genuinely-absent declared entry that a landing
    /// commit WOULD have accounted for is refused because the search never
    /// ran. The control task differs only in that git answers "no matching
    /// commits" rather than failing, and must still be refused at High.
    #[test]
    fn pre_done_gate_log_grep_failure_downgrades_to_advisory_low() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        git.set_is_ancestor("main", "main", true);

        // Both tasks: git ANSWERS "not tracked on main" — genuine absence.
        git.set_path_tracked_on("main", "crates/reify-x/src/gone.rs", false);

        // (a) the rescue search itself failed.
        git.set_log_grep_error(
            "main",
            "6345LGERR",
            "git exited Some(128): fatal: bad revision 'main'",
        );
        // (b) control: the rescue search ran and matched nothing.
        git.set_log_grep("main", "6345LGOK", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345LGERR".to_string(),
            pre_done_meta("6345LGERR", "review", &["crates/reify-x/src/gone.rs"]),
        );
        task_metadata.insert(
            "6345LGOK".to_string(),
            pre_done_meta("6345LGOK", "review", &["crates/reify-x/src/gone.rs"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345LGERR");
        assert_eq!(findings.len(), 1, "expected one finding; got {:?}", findings);
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "an empty rescue candidate list produced by a FAILED `git log --grep` \
             is not evidence that no commit references the task; got {:?}",
            findings[0]
        );
        assert!(
            findings[0].summary.contains("[advisory")
                && findings[0].summary.contains("log --grep"),
            "the failing seam must be named in the summary; got {:?}",
            findings[0].summary
        );

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345LGOK");
        assert_eq!(findings.len(), 1, "expected one refusal; got {:?}", findings);
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "a rescue search that RAN and matched nothing is evidence and must \
             still refuse; got {:?}",
            findings[0]
        );
    }

    /// A `log_grep`-derived sibling is an ancestor of `MAIN_BASE` by
    /// construction, so the pre-done scan must not fork to ask.
    ///
    /// `git log <MAIN_BASE> --grep=…` lists only commits REACHABLE FROM
    /// `MAIN_BASE` — the same relation `git merge-base --is-ancestor` tests,
    /// against the same ref in the same repo in the same process. Asking
    /// anyway doubled the per-sibling fork count on the one path that runs
    /// inside fused-memory's per-project write lock under a 30 s hard timeout:
    /// 2 × `PRE_DONE_SIBLING_SCAN_CAP` ≈ 5.7 s at the measured ~57 ms/fork.
    ///
    /// The fixture pins the priming directly rather than counting forks:
    /// `is_ancestor` for the sibling is LEFT at MockGitOps' `false` default,
    /// so only a primed cache can send `changed_paths_for_claim` down the
    /// `<commit>^1..<commit>` arm where the rescue evidence lives. Unprimed,
    /// it takes the degenerate `main..<sha>` arm, finds nothing, and refuses.
    ///
    /// The invariant is provable at THIS call site only, which is why the
    /// sweep's rescue leg still asks git.
    #[test]
    fn pre_done_gate_primes_ancestry_for_log_grep_siblings() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        // The repo-health probe keys on ("main", "main") — a different entry
        // from the sibling's, which stays deliberately unset.
        git.set_is_ancestor("main", "main", true);

        git.set_log_grep(
            "main",
            "6345PRIME",
            vec![GitCommit {
                sha: "primemerge".to_string(),
                subject: "Merge task/6345PRIME into main".to_string(),
            }],
        );
        // NOT set: `git.set_is_ancestor("primemerge", "main", true)`.
        git.set_changed_paths_in_commit(
            "primemerge",
            vec!["crates/reify-x/src/landed.rs".to_string()],
        );
        // The degenerate arm carries nothing, exactly as post-merge reality.
        git.set_diff_changed_paths("main", "primemerge", vec![]);

        // The declared file was removed by its own landing commit, so
        // `path_tracked_on` stays false and the rescue leg must decide.
        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345PRIME".to_string(),
            pre_done_meta("6345PRIME", "review", &["crates/reify-x/src/landed.rs"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345PRIME");
        assert!(
            findings.is_empty(),
            "a `log_grep`-derived sibling must be treated as an ancestor without \
             a `merge-base` fork — the reachability invariant makes the answer \
             `true` by construction; got {:?}",
            findings
        );
    }

    /// A truncated sibling scan must not refuse: the corroborating commit may
    /// simply have been the one past the cap.
    ///
    /// `PRE_DONE_SIBLING_SCAN_CAP` bounds the scan so the hook cannot
    /// head-of-line block fused-memory's per-project write lock. Emitting a
    /// blocking High off a knowingly incomplete search inverts the crate's
    /// fail-safe direction, so truncation downgrades to an advisory Low.
    #[test]
    fn pre_done_gate_truncated_sibling_scan_downgrades_to_advisory_low() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        git.set_is_ancestor("main", "main", true);

        // More candidates than the cap, none of which cover the declared file.
        let siblings: Vec<GitCommit> = (0..80)
            .map(|i| GitCommit {
                sha: format!("sib{i}"),
                subject: format!("Merge task/6345CAP into main ({i})"),
            })
            .collect();
        for c in &siblings {
            git.set_is_ancestor(&c.sha, "main", true);
            git.set_changed_paths_in_commit(
                &c.sha,
                vec!["crates/reify-other/src/unrelated.rs".to_string()],
            );
        }
        git.set_log_grep("main", "6345CAP", siblings);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6345CAP".to_string(),
            pre_done_meta("6345CAP", "review", &["crates/reify-x/src/thing.rs"]),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6345CAP");
        assert_eq!(findings.len(), 1, "expected one finding; got {:?}", findings);
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "a refusal built on a scan truncated at the cap must be advisory, not \
             blocking — the corroborating commit may be the one never inspected; \
             got {:?}",
            findings[0]
        );
        assert!(
            findings[0].summary.contains("[advisory"),
            "the truncation must be legible in the summary; got {:?}",
            findings[0].summary
        );
    }

    /// The digit-boundary filter must drop only DEMONSTRATED collisions, never
    /// a commit that references the task somewhere `log_grep` cannot show us.
    ///
    /// `git log --grep=<id>` matches the WHOLE commit message, but
    /// `LOG_GREP_FORMAT` (`%H%x09%s`) carries only the subject. So a commit
    /// citing the task only in its body arrives with a subject that does not
    /// mention the id at all — MEASURED on the live repo: `git log main
    /// --grep=6200` returns `1881ede9ac docs(6211): …` and `09de21ab8e Merge
    /// main into task/6211`. Dropping those would silently widen the collision
    /// filter into "reject every body-only reference", producing a false High
    /// in the sweep and a wrongful refusal at the gate.
    #[test]
    fn log_grep_rescue_keeps_body_only_reference() {
        let conn = seed_db();

        let mut git = MockGitOps::new();
        git.set_is_ancestor("main", "main", true);

        // The subject does not contain "6200" anywhere: git matched the body.
        git.set_log_grep(
            "main",
            "6200",
            vec![GitCommit {
                sha: "bodyonly".to_string(),
                subject: "docs(6211): record the pre-done hook contract".to_string(),
            }],
        );
        git.set_is_ancestor("bodyonly", "main", true);
        git.set_changed_paths_in_commit(
            "bodyonly",
            vec!["crates/reify-x/src/landed_then_removed.rs".to_string()],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "6200".to_string(),
            pre_done_meta(
                "6200",
                "review",
                &["crates/reify-x/src/landed_then_removed.rs"],
            ),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check_pre_done(&ctx, "6200");
        assert!(
            findings.is_empty(),
            "a log_grep hit whose subject does not mention the id at all matched on \
             the BODY, which LOG_GREP_FORMAT never returns — it must still count as \
             task-referencing; got {:?}",
            findings
        );
    }

    /// Fix 1 downgrade (RED — S1): when the claimed provenance commit is
    /// unreachable AND no sibling-FF covers the missing set, but every
    /// metadata.files entry resolves to a tracked path on main (dir-aware,
    /// via path_tracked_on), the deliverable landed and only the provenance
    /// pointer is stale — downgrade High → Low.
    ///
    /// This FAILS on current code, which returns Severity::High
    /// "metadata.files mismatch / commit not reachable from main".
    #[test]
    fn deliverable_present_on_main_downgrades_to_low() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "4100");

        let mut git = MockGitOps::new();
        // Claimed commit covers nothing → everything in metadata.files is "missing".
        git.set_diff_changed_paths("main", "stale_sha", vec![]);
        // No sibling-FF rescue.
        git.set_log_grep("main", "4100", vec![]);
        // Both metadata.files entries are present on main: one is a regular
        // file, the other exercises the dir-aware case (a directory that
        // contains tracked files → git ls-tree returns non-empty).
        git.set_path_tracked_on("main", "crates/reify-x/src/foo.rs", true);
        git.set_path_tracked_on("main", "gui/src-tauri/src/tests", true);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4100".to_string(),
            TaskMetadata {
                task_id: "4100".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-x/src/foo.rs".to_string(),
                    "gui/src-tauri/src/tests".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("stale_sha".to_string()),
                    note: None,
                }),
                title: "Fix 1 downgrade test task".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "all entries present on main → must downgrade to Low; got {:?}",
            f.severity
        );
        let s = f.summary.to_lowercase();
        assert!(
            s.contains("deliverable") && s.contains("present"),
            "summary should mention 'deliverable' and 'present'; got {:?}",
            f.summary
        );
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence
            .expect("MetadataFiles evidence must be present");
        assert!(
            !entries.is_empty(),
            "MetadataFiles evidence must be non-empty (the missing set); got {:?}",
            entries
        );
    }

    /// Fix 1 true-positive guard (RED — S3): when one metadata.files entry IS
    /// present on main but another is genuinely absent, the finding must stay
    /// High and the MetadataFiles evidence must cite ONLY the absent entry,
    /// not the present one.
    ///
    /// This FAILS after S2, whose surviving High finding still cites the full
    /// `missing` set (both present.rs and absent.rs), so the "must not contain
    /// present.rs" assertion fails.
    #[test]
    fn partially_absent_deliverable_stays_high_and_cites_only_absent() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "4101");

        let mut git = MockGitOps::new();
        // Empty primary diff → everything in metadata.files is in `missing`.
        git.set_diff_changed_paths("main", "stale_sha_2", vec![]);
        // No sibling-FF rescue.
        git.set_log_grep("main", "4101", vec![]);
        // present.rs is on main; absent.rs is NOT (defaults to false).
        git.set_path_tracked_on("main", "crates/reify-x/src/present.rs", true);
        // crates/reify-x/src/absent.rs → not set → defaults false (genuinely absent).

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4101".to_string(),
            TaskMetadata {
                task_id: "4101".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-x/src/present.rs".to_string(),
                    "crates/reify-x/src/absent.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("stale_sha_2".to_string()),
                    note: None,
                }),
                title: "Partially-absent deliverable test task".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::High,
            "absent entry remains → must stay High; got {:?}",
            f.severity
        );
        let s = f.summary.to_lowercase();
        assert!(
            s.contains("mismatch") || s.contains("not reachable"),
            "summary should mention 'mismatch' or 'not reachable'; got {:?}",
            f.summary
        );
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence.expect("MetadataFiles evidence must be present");
        assert!(
            entries.iter().any(|p| p == "crates/reify-x/src/absent.rs"),
            "absent.rs must appear in evidence; got {:?}",
            entries
        );
        assert!(
            !entries.iter().any(|p| p == "crates/reify-x/src/present.rs"),
            "present.rs must NOT appear in evidence (it is on main); got {:?}",
            entries
        );
    }

    /// Fix 2 downgrade + true-positive preservation (RED — S5): a merged task
    /// with NO task_completed event in runs.db but whose claimed commit IS an
    /// ancestor of main must downgrade to Low (rebuild coverage gap, not a
    /// defect). A sibling task with a non-ancestor commit must stay High.
    ///
    /// On current code BOTH tasks are High, so the 4200 "must be Low" assertion
    /// FAILS (red-first); the 4201 High assertion locks the true-positive after
    /// the fix.
    #[test]
    fn merged_no_event_but_commit_ancestor_downgrades_to_low() {
        let conn = seed_db();
        // Deliberately DO NOT insert task_completed events for 4200 or 4201
        // so the runs.db Ok(false) arm fires for both.

        let mut git = MockGitOps::new();
        // Task 4200: ancestor_sha is an ancestor of main → eligible for Low.
        git.set_is_ancestor("ancestor_sha", "main", true);
        // Task 4201: orphan_sha is NOT an ancestor of main → must stay High.
        // (not set → defaults false)

        // Provide empty diffs/log-greps so other legs don't rescue these tasks.
        git.set_diff_changed_paths("main", "ancestor_sha", vec![]);
        git.set_diff_changed_paths("main", "orphan_sha", vec![]);
        git.set_log_grep("main", "4200", vec![]);
        git.set_log_grep("main", "4201", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4200".to_string(),
            TaskMetadata {
                task_id: "4200".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/foo.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("ancestor_sha".to_string()),
                    note: None,
                }),
                title: "Ancestor-corroborated task".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );
        task_metadata.insert(
            "4201".to_string(),
            TaskMetadata {
                task_id: "4201".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/bar.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("orphan_sha".to_string()),
                    note: None,
                }),
                title: "Orphan-commit task".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        // Find findings for each task.
        let f4200 = findings.iter().find(|f| f.task_id == "4200")
            .expect("finding for task 4200 must exist");
        let f4201 = findings.iter().find(|f| f.task_id == "4201")
            .expect("finding for task 4201 must exist");

        assert_eq!(
            f4200.severity,
            Severity::Low,
            "ancestor commit → must downgrade to Low; got {:?}",
            f4200.severity
        );
        let s4200 = f4200.summary.to_lowercase();
        assert!(
            s4200.contains("deliverable") && s4200.contains("ancestor"),
            "summary should mention 'deliverable' and 'ancestor'; got {:?}",
            f4200.summary
        );

        assert_eq!(
            f4201.severity,
            Severity::High,
            "non-ancestor commit → must stay High; got {:?}",
            f4201.severity
        );
        let s4201 = f4201.summary.to_lowercase();
        assert!(
            s4201.contains("no task_completed event") || s4201.contains("task_completed"),
            "summary should mention 'no task_completed event'; got {:?}",
            f4201.summary
        );
    }

    /// Fix 2 Low finding must cite the corroborating ancestor commit (RED — S7):
    /// the evidence must include an EvidenceRef::Commit whose sha equals the
    /// claimed commit.
    ///
    /// This FAILS after S6, whose Low finding carries only EvidenceRef::RunsDb
    /// evidence and no Commit ref.
    #[test]
    fn ancestor_corroboration_cites_commit_evidence() {
        let conn = seed_db();
        // No task_completed event → Ok(false) arm fires.

        let mut git = MockGitOps::new();
        git.set_is_ancestor("ancestor_sha2", "main", true);
        // Provide empty diffs/log-greps so other legs don't rescue.
        git.set_diff_changed_paths("main", "ancestor_sha2", vec![]);
        git.set_log_grep("main", "4202", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4202".to_string(),
            TaskMetadata {
                task_id: "4202".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/foo.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("ancestor_sha2".to_string()),
                    note: None,
                }),
                title: "Ancestor commit evidence test task".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "ancestor commit → must be Low; got {:?}",
            f.severity
        );
        // The evidence must include an EvidenceRef::Commit citing the ancestor sha.
        let cited_commit = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::Commit { sha, .. } => Some(sha.as_str()),
            _ => None,
        });
        assert_eq!(
            cited_commit,
            Some("ancestor_sha2"),
            "Low finding must include EvidenceRef::Commit {{ sha: 'ancestor_sha2' }}; got {:?}",
            f.evidence
        );
    }

    // ── H1 test helper ───────────────────────────────────────────────────────
    //
    // Build the full single-file AuditContext fixture, run `p5_phantom_done::check`,
    // and return only the Pattern::P5TestsAssertEmpty findings.
    //
    // Use this for H1 tests that involve exactly one metadata.files path; the
    // commit sha is derived deterministically from task_id (lowercase + "_commit")
    // so callers don't have to manage separate identifiers.
    //
    // Multi-file or check_pre_done tests should build their own context because
    // they need richer fixture control (multiple paths, H2 JCodemunch stubs, …).
    fn run_h1_single_file(
        task_id: &str,
        path: &str,
        added_lines: Vec<(usize, String)>,
    ) -> Vec<Finding> {
        let conn = seed_db();
        insert_task_completed_event(&conn, task_id);

        let commit = format!("{}_commit", task_id.to_lowercase());
        let mut git = MockGitOps::new();
        git.set_diff_changed_paths("main", &commit, vec![path.to_string()]);
        git.set_log_grep("main", task_id, vec![]);
        git.set_diff_added_lines_in_commit(&commit, path, added_lines);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            TaskMetadata {
                task_id: task_id.to_string(),
                status: "done".to_string(),
                files: vec![path.to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some(commit),
                    note: None,
                }),
                title: format!("{} single-file test task", task_id),
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
            producer_branch: None,
        };

        p5_phantom_done::check(&ctx)
            .into_iter()
            .filter(|f| f.pattern == Pattern::P5TestsAssertEmpty)
            .collect()
    }
    // ── end H1 test helper ────────────────────────────────────────────────────

    /// H1 integration test: a done task whose file-level corroboration is clean
    /// (diff covers metadata.files, task_completed event present) but whose added
    /// test lines contain a placeholder-named fn with a vacuous empty assertion.
    ///
    /// Replicates the task 1638/1904/2199 phantom-done pattern:
    /// the test fn `activate_expands_geometric_params_placeholder_to_empty_list`
    /// asserts `is_empty()` — both the placeholder marker in the fn name AND the
    /// empty-assertion signal are present → one P5TestsAssertEmpty Medium finding.
    ///
    /// RED until check_tests_assert_empty is implemented (step-4).
    #[test]
    fn h1_placeholder_empty_assertion_test_flags_phantom_done() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H1T1");

        let mut git = MockGitOps::new();
        // File-level corroboration is clean: diff covers both files.
        git.set_diff_changed_paths(
            "main",
            "h1_commit",
            vec![
                "crates/reify-eval/src/expander.rs".to_string(),
                "crates/reify-eval/tests/expander_tests.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H1T1", vec![]);

        // Seed the added lines in the test file: a placeholder fn with is_empty().
        // The fn name contains "placeholder" (marker) and "empty_list" (vacuous hint);
        // the body asserts is_empty() — both gates triggered.
        git.set_diff_added_lines_in_commit(
            "h1_commit",
            "crates/reify-eval/tests/expander_tests.rs",
            vec![
                (10, "    #[test]".to_string()),
                (11, "    fn activate_expands_geometric_params_placeholder_to_empty_list() {".to_string()),
                (12, "        let result = activate_expands_geometric_params();".to_string()),
                (13, "        assert!(result.is_empty());".to_string()),
                (14, "    }".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H1T1".to_string(),
            TaskMetadata {
                task_id: "H1T1".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-eval/src/expander.rs".to_string(),
                    "crates/reify-eval/tests/expander_tests.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h1_commit".to_string()),
                    note: None,
                }),
                title: "Activate geometric param expansion".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h1_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5TestsAssertEmpty)
            .collect();
        assert_eq!(
            h1_findings.len(),
            1,
            "expected exactly one P5TestsAssertEmpty finding; got {:?}",
            findings
        );
        let f = h1_findings[0];
        assert_eq!(f.severity, Severity::Medium, "H1 must fire at Medium; got {:?}", f.severity);
        assert_eq!(f.task_id, "H1T1");
        // Evidence must cite the test file.
        let file_ev = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::File { path } => Some(path.as_str()),
            _ => None,
        });
        assert!(
            file_ev.is_some_and(|p| p.contains("expander_tests.rs")),
            "evidence must cite the test file; got {:?}",
            f.evidence
        );

        // check_pre_done must also return the H1 finding (D-1 parity).
        let pre_done = p5_phantom_done::check_pre_done(&ctx, "H1T1");
        let h1_pre: Vec<_> = pre_done
            .iter()
            .filter(|f| f.pattern == Pattern::P5TestsAssertEmpty)
            .collect();
        assert_eq!(
            h1_pre.len(),
            1,
            "check_pre_done must also find P5TestsAssertEmpty; got {:?}",
            pre_done
        );
    }

    /// H1 FP guard (a): a done task whose added test fn has NO placeholder/empty/
    /// not_yet/stub marker in its name, but DOES assert an empty result.
    ///
    /// A test like `fn returns_no_warnings_for_valid_input()` that asserts
    /// `is_empty()` is a LEGITIMATE empty-result test — not a placeholder.
    /// The double-gate must suppress it: zero P5TestsAssertEmpty findings.
    ///
    /// RED against the naive step-4 impl (which fires on any empty assertion).
    #[test]
    fn h1_legit_empty_assertion_without_marker_not_flagged() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H1FP1");

        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "h1fp1_commit",
            vec![
                "crates/reify-lint/src/checker.rs".to_string(),
                "crates/reify-lint/tests/checker_tests.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H1FP1", vec![]);

        // Test fn name has NO placeholder/stub/empty/not_yet marker.
        // It legitimately tests that a valid input produces no warnings.
        git.set_diff_added_lines_in_commit(
            "h1fp1_commit",
            "crates/reify-lint/tests/checker_tests.rs",
            vec![
                (5, "    #[test]".to_string()),
                (6, "    fn returns_no_warnings_for_valid_input() {".to_string()),
                (7, "        let result = check_valid_input();".to_string()),
                (8, "        assert!(result.is_empty(), \"valid input should produce no warnings\");".to_string()),
                (9, "    }".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H1FP1".to_string(),
            TaskMetadata {
                task_id: "H1FP1".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-lint/src/checker.rs".to_string(),
                    "crates/reify-lint/tests/checker_tests.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h1fp1_commit".to_string()),
                    note: None,
                }),
                title: "Add lint checker for valid input".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h1_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5TestsAssertEmpty)
            .collect();
        assert!(
            h1_findings.is_empty(),
            "legit empty-assertion test without placeholder marker must NOT be flagged; \
             got {:?}",
            h1_findings
        );
    }

    /// H2 integration test: a done task whose file-level corroboration is clean
    /// and whose metadata.files span >=2 distinct crates/<name>/ roots. A
    /// changed capability symbol has no non-test workspace caller (stranded by
    /// a live-path relocation). Expects exactly one P5LivePathStranded Medium finding.
    ///
    /// Replicates the cross-crate relocation pattern from task 1638/1904/2199:
    /// compile_purpose moved from reify-compiler to reify-eval, leaving the old
    /// symbol unreachable on the live path.
    ///
    /// RED until check_live_path_stranded is implemented (step-8).
    #[test]
    fn h2_cross_crate_stranded_symbol_flags_phantom_done() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H2T1");

        let mut git = MockGitOps::new();
        // File-level corroboration is clean: diff covers all metadata.files.
        git.set_diff_changed_paths(
            "main",
            "h2_commit",
            vec![
                "crates/reify-compiler/src/compile.rs".to_string(),
                "crates/reify-eval/src/lib.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H2T1", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H2T1".to_string(),
            TaskMetadata {
                task_id: "H2T1".to_string(),
                status: "done".to_string(),
                // Two distinct crates/<name>/ roots: reify-compiler + reify-eval.
                files: vec![
                    "crates/reify-compiler/src/compile.rs".to_string(),
                    "crates/reify-eval/src/lib.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h2_commit".to_string()),
                    note: None,
                }),
                title: "Relocate compile_purpose to reify-eval".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        // The changed symbol is in crates/reify-eval/src/lib.rs with no callers.
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            "h2_commit^1",
            "h2_commit",
            vec![reify_audit::ChangedSymbol {
                name: "compile_purpose".to_string(),
                file: "crates/reify-eval/src/lib.rs".to_string(),
                line: 42,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // No callers returned → stranded.
        // (set_find_references not called → defaults to empty vec)

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h2_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5LivePathStranded)
            .collect();
        assert_eq!(
            h2_findings.len(),
            1,
            "expected exactly one P5LivePathStranded finding; got {:?}",
            findings
        );
        let f = h2_findings[0];
        assert_eq!(f.severity, Severity::Medium, "H2 must fire at Medium; got {:?}", f.severity);
        assert_eq!(f.task_id, "H2T1");
        // Evidence must cite the symbol's file.
        let file_ev = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::File { path } => Some(path.as_str()),
            _ => None,
        });
        assert!(
            file_ev.is_some_and(|p| p.contains("reify-eval")),
            "evidence must cite the symbol's file in reify-eval; got {:?}",
            f.evidence
        );

        // check_pre_done must also return the H2 finding (D-1 parity).
        let pre_done = p5_phantom_done::check_pre_done(&ctx, "H2T1");
        let h2_pre: Vec<_> = pre_done
            .iter()
            .filter(|f| f.pattern == Pattern::P5LivePathStranded)
            .collect();
        assert_eq!(
            h2_pre.len(),
            1,
            "check_pre_done must also find P5LivePathStranded; got {:?}",
            pre_done
        );
    }

    /// H1 FP guard (b): a done task whose added test fn name DOES carry a
    /// placeholder marker (e.g. "stub") but whose added lines assert a
    /// NON-empty result.
    ///
    /// A test named `fn stub_returns_items()` that asserts `len() == 3` is not
    /// vacuous — the name has a marker but the assertion is real.
    /// The double-gate must suppress it: zero P5TestsAssertEmpty findings.
    ///
    /// RED against the naive step-4 impl (which doesn't check the fn name at all).
    /// This test is GREEN even on the naive impl because the naive impl
    /// checks for empty assertion patterns, and `len() == 3` doesn't match.
    /// Step-5 is designed to produce RED only for case (a) above.
    /// This test is included for completeness to lock in the double-gate contract.
    #[test]
    fn h1_marker_without_empty_assertion_not_flagged() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H1FP2");

        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "h1fp2_commit",
            vec![
                "crates/reify-eval/src/evaluator.rs".to_string(),
                "crates/reify-eval/tests/eval_tests.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H1FP2", vec![]);

        // Test fn name has "stub" marker but the assertion is non-empty (len == 3).
        git.set_diff_added_lines_in_commit(
            "h1fp2_commit",
            "crates/reify-eval/tests/eval_tests.rs",
            vec![
                (20, "    #[test]".to_string()),
                (21, "    fn stub_returns_items() {".to_string()),
                (22, "        let result = evaluate_stub();".to_string()),
                (23, "        assert_eq!(result.len(), 3, \"stub should return 3 items\");".to_string()),
                (24, "    }".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H1FP2".to_string(),
            TaskMetadata {
                task_id: "H1FP2".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-eval/src/evaluator.rs".to_string(),
                    "crates/reify-eval/tests/eval_tests.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h1fp2_commit".to_string()),
                    note: None,
                }),
                title: "Evaluate stub items".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h1_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5TestsAssertEmpty)
            .collect();
        assert!(
            h1_findings.is_empty(),
            "placeholder-named fn with non-empty assertion must NOT be flagged; \
             got {:?}",
            h1_findings
        );
    }

    /// H2 FP guard (a): a cross-crate done task with a changed symbol that HAS
    /// a non-test workspace caller → not stranded → zero P5LivePathStranded findings.
    ///
    /// RED against naive step-8 impl if it doesn't check for non-test callers
    /// (but step-8 does check, so this is GREEN already — included to lock the contract).
    #[test]
    fn h2_live_non_test_caller_not_flagged() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H2FP1");

        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "h2fp1_commit",
            vec![
                "crates/reify-compiler/src/compile.rs".to_string(),
                "crates/reify-eval/src/lib.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H2FP1", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H2FP1".to_string(),
            TaskMetadata {
                task_id: "H2FP1".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-compiler/src/compile.rs".to_string(),
                    "crates/reify-eval/src/lib.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h2fp1_commit".to_string()),
                    note: None,
                }),
                title: "Cross-crate with live caller".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            "h2fp1_commit^1",
            "h2fp1_commit",
            vec![reify_audit::ChangedSymbol {
                name: "compile_purpose".to_string(),
                file: "crates/reify-eval/src/lib.rs".to_string(),
                line: 42,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // There IS a non-test caller → symbol is not stranded.
        jc.set_find_references(
            "crates/reify-eval/src/lib.rs",
            "compile_purpose",
            vec![reify_audit::SymbolReference {
                file: "crates/reify-gui/src/main.rs".to_string(),
                line: 77,
            }],
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h2_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5LivePathStranded)
            .collect();
        assert!(
            h2_findings.is_empty(),
            "symbol with a live non-test caller must NOT be flagged; got {:?}",
            h2_findings
        );
    }

    /// H2 FP guard (b): a done task whose changed symbol has no caller but whose
    /// metadata.files stay within ONE crate root → not cross-crate → zero
    /// P5LivePathStranded findings. Single-crate orphans are P1's domain.
    ///
    /// RED against naive step-8 impl (which doesn't check the cross-crate gate).
    #[test]
    fn h2_single_crate_not_flagged() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H2FP2");

        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "h2fp2_commit",
            vec![
                "crates/reify-eval/src/lib.rs".to_string(),
                "crates/reify-eval/src/expander.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H2FP2", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H2FP2".to_string(),
            TaskMetadata {
                task_id: "H2FP2".to_string(),
                status: "done".to_string(),
                // Both files are under crates/reify-eval/ → single crate root.
                files: vec![
                    "crates/reify-eval/src/lib.rs".to_string(),
                    "crates/reify-eval/src/expander.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h2fp2_commit".to_string()),
                    note: None,
                }),
                title: "Single-crate refactor".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let mut jc = MockJCodemunchOps::new();
        // Symbol in the single crate, no callers → would be flagged by naive impl.
        jc.set_changed_symbols(
            "h2fp2_commit^1",
            "h2fp2_commit",
            vec![reify_audit::ChangedSymbol {
                name: "expand_purpose".to_string(),
                file: "crates/reify-eval/src/expander.rs".to_string(),
                line: 15,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // No callers set → empty.

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h2_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5LivePathStranded)
            .collect();
        assert!(
            h2_findings.is_empty(),
            "single-crate task must NOT be flagged by H2; got {:?}",
            h2_findings
        );
    }

    /// H2 FP guard (c): a cross-crate done task with a changed symbol that carries
    /// a suppression marker (has_allow_dead_code) → zero P5LivePathStranded findings.
    ///
    /// RED against naive step-8 impl (which doesn't apply suppression guards).
    #[test]
    fn h2_suppressed_symbol_not_flagged() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "H2FP3");

        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "h2fp3_commit",
            vec![
                "crates/reify-compiler/src/compile.rs".to_string(),
                "crates/reify-eval/src/lib.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "H2FP3", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "H2FP3".to_string(),
            TaskMetadata {
                task_id: "H2FP3".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-compiler/src/compile.rs".to_string(),
                    "crates/reify-eval/src/lib.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("h2fp3_commit".to_string()),
                    note: None,
                }),
                title: "Cross-crate with suppressed symbol".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let mut jc = MockJCodemunchOps::new();
        // Symbol carries #[allow(dead_code)] → intentional orphan, must be suppressed.
        jc.set_changed_symbols(
            "h2fp3_commit^1",
            "h2fp3_commit",
            vec![reify_audit::ChangedSymbol {
                name: "internal_helper".to_string(),
                file: "crates/reify-eval/src/lib.rs".to_string(),
                line: 100,
                has_allow_dead_code: true,  // opt-out
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // No callers.

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        let h2_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.pattern == Pattern::P5LivePathStranded)
            .collect();
        assert!(
            h2_findings.is_empty(),
            "#[allow(dead_code)] symbol must NOT be flagged by H2; got {:?}",
            h2_findings
        );
    }

    /// H1 FP guard (negation): a placeholder-named fn that asserts
    /// `assert!(!result.is_empty())` (negated — asserting NON-empty) must NOT
    /// be flagged.
    ///
    /// The `.is_empty()` pattern in gate (b) fires only when NOT immediately
    /// preceded by `!`. A test like `fn placeholder_checks_non_empty_result()`
    /// that asserts `!result.is_empty()` is a real correctness check, not a
    /// vacuous placeholder — the double-gate must suppress it.
    ///
    /// Locks the negation-guard added in the amendment pass (task 4140 review
    /// suggestion 1).
    #[test]
    fn h1_negated_is_empty_not_flagged() {
        // fn name has "placeholder" marker BUT asserts !is_empty() (non-empty).
        let findings = run_h1_single_file(
            "H1NEG1",
            "crates/reify-eval/tests/neg_tests.rs",
            vec![
                (1, "    #[test]".to_string()),
                (2, "    fn placeholder_checks_non_empty_result() {".to_string()),
                (3, "        let result = compute();".to_string()),
                (4, "        assert!(!result.is_empty(), \"result must not be empty\");".to_string()),
                (5, "    }".to_string()),
            ],
        );
        assert!(
            findings.is_empty(),
            "placeholder-named fn asserting !is_empty() (non-empty) must NOT be flagged; \
             got {:?}",
            findings
        );
    }

    /// H1 FP guard (empty-as-domain-noun): a test fn whose name contains 'empty'
    /// as a descriptive noun (e.g. `handles_empty_input_returns_no_warnings`) but
    /// that legitimately asserts `is_empty()` must NOT be flagged.
    ///
    /// 'empty' was formerly in PLACEHOLDER_MARKERS, which caused false positives
    /// for names like `handles_empty_input`, `returns_error_on_empty_list`, and
    /// `empty_collection_is_valid`. This test locks the non-flagging after
    /// dropping 'empty' from the marker set (task 4140 review suggestion 2).
    #[test]
    fn h1_empty_in_name_legitimate_assertion_not_flagged() {
        // fn name contains 'empty' as a domain noun — no stronger placeholder marker.
        // The fn legitimately asserts is_empty() for a valid empty-input case.
        let findings = run_h1_single_file(
            "H1EMPTY",
            "crates/reify-lint/tests/empty_tests.rs",
            vec![
                (1, "    #[test]".to_string()),
                (2, "    fn handles_empty_input_returns_no_warnings() {".to_string()),
                (3, "        let result = lint_empty_input();".to_string()),
                (4, "        assert!(result.is_empty(), \"empty input should produce no warnings\");".to_string()),
                (5, "    }".to_string()),
            ],
        );
        assert!(
            findings.is_empty(),
            "fn with 'empty' as a domain noun (not a placeholder marker) must NOT be flagged; \
             got {:?}",
            findings
        );
    }

    /// End-to-end regression pin for the task 1638/1904/2199 phantom-done class.
    ///
    /// Combines all three incident traits in one fixture:
    /// - Clean file-level corroboration (check_one returns None — the incident
    ///   slipped through WITH clean corroboration).
    /// - H1: a placeholder-named test fn asserting is_empty().
    /// - H2: a cross-crate changed capability symbol with no non-test caller.
    ///
    /// Asserts:
    /// - `check(&ctx)` returns >=1 finding with Pattern::P5TestsAssertEmpty or
    ///   Pattern::P5LivePathStranded (the two new heuristics must fire).
    /// - `check_pre_done(&ctx, id)` returns the same findings (D-1 hook parity).
    ///
    /// Also includes a fully-clean cross-crate control task (non-empty test +
    /// live non-test caller) and asserts it yields zero new-pattern findings
    /// (the "no new false-positives" criterion).
    #[test]
    fn regression_1638_1904_2199_phantom_done_flagged() {
        let conn = seed_db();
        // Both tasks have clean runs.db events.
        insert_task_completed_event(&conn, "REGR1");
        insert_task_completed_event(&conn, "REGR2");

        let mut git = MockGitOps::new();
        // REGR1 (incident shape): clean file-level corroboration.
        git.set_diff_changed_paths(
            "main",
            "regr1_commit",
            vec![
                "crates/reify-compiler/src/compile.rs".to_string(),
                "crates/reify-eval/tests/expand_tests.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "REGR1", vec![]);
        // H1: placeholder fn + empty assertion in the test file.
        git.set_diff_added_lines_in_commit(
            "regr1_commit",
            "crates/reify-eval/tests/expand_tests.rs",
            vec![
                (5,  "    #[test]".to_string()),
                (6,  "    fn activate_expands_geometric_params_placeholder_to_empty_list() {".to_string()),
                (7,  "        let result = activate_geometric_params();".to_string()),
                (8,  "        assert!(result.is_empty());".to_string()),
                (9,  "    }".to_string()),
            ],
        );

        // REGR2 (clean control): same cross-crate shape but everything is legit.
        git.set_diff_changed_paths(
            "main",
            "regr2_commit",
            vec![
                "crates/reify-compiler/src/compile.rs".to_string(),
                "crates/reify-eval/tests/clean_tests.rs".to_string(),
            ],
        );
        git.set_log_grep("main", "REGR2", vec![]);
        // Non-placeholder fn name, non-empty assertion.
        git.set_diff_added_lines_in_commit(
            "regr2_commit",
            "crates/reify-eval/tests/clean_tests.rs",
            vec![
                (1, "    #[test]".to_string()),
                (2, "    fn compiles_geometric_params_correctly() {".to_string()),
                (3, "        let result = compile_geometric_params();".to_string()),
                (4, "        assert_eq!(result.len(), 3);".to_string()),
                (5, "    }".to_string()),
            ],
        );

        let mut task_metadata = std::collections::HashMap::new();
        task_metadata.insert(
            "REGR1".to_string(),
            TaskMetadata {
                task_id: "REGR1".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-compiler/src/compile.rs".to_string(),
                    "crates/reify-eval/tests/expand_tests.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("regr1_commit".to_string()),
                    note: None,
                }),
                title: "Activate geometric param expansion".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );
        task_metadata.insert(
            "REGR2".to_string(),
            TaskMetadata {
                task_id: "REGR2".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-compiler/src/compile.rs".to_string(),
                    "crates/reify-eval/tests/clean_tests.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("regr2_commit".to_string()),
                    note: None,
                }),
                title: "Compile geometric params".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let mut jc = MockJCodemunchOps::new();
        // H2 for REGR1: stranded symbol with no callers.
        jc.set_changed_symbols(
            "regr1_commit^1",
            "regr1_commit",
            vec![reify_audit::ChangedSymbol {
                name: "expand_purpose_reflective_placeholders".to_string(),
                file: "crates/reify-compiler/src/compile.rs".to_string(),
                line: 58,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // No callers → stranded.

        // H2 for REGR2: live non-test caller → not stranded (clean control).
        jc.set_changed_symbols(
            "regr2_commit^1",
            "regr2_commit",
            vec![reify_audit::ChangedSymbol {
                name: "compile_purpose".to_string(),
                file: "crates/reify-compiler/src/compile.rs".to_string(),
                line: 20,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        jc.set_find_references(
            "crates/reify-compiler/src/compile.rs",
            "compile_purpose",
            vec![reify_audit::SymbolReference {
                file: "crates/reify-eval/src/lib.rs".to_string(),
                line: 33,
            }],
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        // REGR1: must produce at least one new-pattern finding.
        let all_findings = p5_phantom_done::check(&ctx);
        let regr1_new: Vec<_> = all_findings
            .iter()
            .filter(|f| f.task_id == "REGR1")
            .filter(|f| {
                f.pattern == Pattern::P5TestsAssertEmpty
                    || f.pattern == Pattern::P5LivePathStranded
            })
            .collect();
        assert!(
            !regr1_new.is_empty(),
            "incident shape (REGR1) must trigger at least one new-pattern finding; \
             got {:?}",
            all_findings
        );

        // check_pre_done must agree with check for REGR1.
        let pre_done_regr1 = p5_phantom_done::check_pre_done(&ctx, "REGR1");
        let pre_done_new: Vec<_> = pre_done_regr1
            .iter()
            .filter(|f| {
                f.pattern == Pattern::P5TestsAssertEmpty
                    || f.pattern == Pattern::P5LivePathStranded
            })
            .collect();
        assert_eq!(
            regr1_new.len(),
            pre_done_new.len(),
            "check and check_pre_done must agree on new-pattern finding count for REGR1; \
             check={:?}, pre_done={:?}",
            regr1_new,
            pre_done_new
        );

        // REGR2 (clean control): must produce ZERO new-pattern findings.
        let regr2_new: Vec<_> = all_findings
            .iter()
            .filter(|f| f.task_id == "REGR2")
            .filter(|f| {
                f.pattern == Pattern::P5TestsAssertEmpty
                    || f.pattern == Pattern::P5LivePathStranded
            })
            .collect();
        assert!(
            regr2_new.is_empty(),
            "clean control task (REGR2) must yield zero new-pattern findings; \
             got {:?}",
            regr2_new
        );
    }

    /// H1 FP guard — domain-noun "placeholder" in a test fn name that is a
    /// LEGITIMATE product-module sentinel test, NOT a placeholder test.
    ///
    /// Real corpus name: `tessellate_sentinel_placeholder_continues_independent_ops`
    /// (crates/reify-eval/tests/harness_geometry/geometry_error_handling.rs). The word
    /// "placeholder" here describes a sentinel value / code-path placeholder in
    /// the *geometry kernel*, not a not-yet-implemented test. The fn body
    /// legitimately asserts `is_empty()` because the sentinel path produces an
    /// empty geometry list.
    ///
    /// RED against the current two-gate (name-marker AND body-empty-assertion):
    /// fn_name.contains("placeholder") is true, and the body has is_empty() —
    /// so the two-gate fires, producing a false positive. After the three-signal
    /// gate (name-marker AND name-empty-intent AND body-empty-assertion) the
    /// name lacks any empty-intent token ("empty", "none", "nil", "zero",
    /// "vacuous", "nothing", "no_") and is correctly suppressed.
    #[test]
    fn h1_domain_noun_placeholder_in_sentinel_not_flagged() {
        // Real corpus fn name: contains "placeholder" as a domain noun (sentinel
        // geometry value), NOT as a marker for "not yet implemented". The body
        // legitimately asserts that the sentinel path produces an empty geometry list.
        let findings = run_h1_single_file(
            "H1DN1",
            "crates/reify-eval/tests/harness_geometry/geometry_error_handling.rs",
            vec![
                (1, "    #[test]".to_string()),
                (2, "    fn tessellate_sentinel_placeholder_continues_independent_ops() {".to_string()),
                (3, "        let result = tessellate_with_sentinel_placeholder();".to_string()),
                (4, "        assert!(result.is_empty(), \"sentinel placeholder path yields no geometry\");".to_string()),
                (5, "    }".to_string()),
            ],
        );
        assert!(
            findings.is_empty(),
            "domain-noun 'placeholder' (sentinel geometry fn) must NOT be flagged as \
             phantom-done; got {:?}",
            findings
        );
    }

    /// Step-5 RED — P5 git-diff-leg: task-id commit reachable on main (siblings
    /// non-empty) but coverage check fails and deliverable-presence also fails.
    /// Pure-reachability fallback in the siblings block should fire.
    ///
    /// Fixture (models task 4242 in the git-diff leg):
    /// - task_completed event IS present for "4242" (bypasses merged-arm entirely)
    /// - kind="found_on_main"; commit="stale_sha_4242"
    /// - diff_changed_paths("main","stale_sha_4242") → [] (all files missing)
    /// - log_grep("main","4242") → [sibling_sha] (siblings non-empty → block entered)
    /// - diff_changed_paths("main","sibling_sha") → [] (coverage FAILS — none of
    ///   the missing files covered by sibling)
    /// - path_tracked_on("main","crates/x/deliver.rs") defaults false → (b) doesn't rescue
    ///
    /// Expected: exactly one P5PhantomDone at Severity::Low citing the sibling commit.
    ///
    /// Fails on current code (coverage fails + files absent → High).
    #[test]
    fn git_diff_leg_task_id_commit_reachable_downgrades_to_low() {
        let conn = seed_db();
        insert_task_completed_event(&conn, "4242");

        let mut git = MockGitOps::new();
        // Primary diff: claimed commit covers nothing → all files missing.
        git.set_diff_changed_paths("main", "stale_sha_4242", vec![]);
        // Sibling rescue: siblings non-empty but their diff covers NONE of the
        // missing files → coverage check fails (still_missing non-empty).
        let sibling = GitCommit {
            sha: "sibling_sha_4242".to_string(),
            subject: "feat(task-4242) step-2 GREEN".to_string(),
        };
        git.set_log_grep("main", "4242", vec![sibling]);
        // Sibling diff covers an unrelated file — NOT crates/x/deliver.rs.
        git.set_diff_changed_paths("main", "sibling_sha_4242", vec!["docs/unrelated.md".to_string()]);
        // path_tracked_on defaults false → deliverable-presence does NOT rescue.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4242".to_string(),
            TaskMetadata {
                task_id: "4242".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/deliver.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("found_on_main".to_string()),
                    commit: Some("stale_sha_4242".to_string()),
                    note: None,
                }),
                title: "Git-diff-leg pure-reachability test task".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "task-id commit reachable (siblings non-empty) → must downgrade to Low; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "4242");
        // Evidence must cite the sibling commit.
        let cited_sibling = f.evidence.iter().any(|e| match e {
            EvidenceRef::Commit { sha, .. } => sha == "sibling_sha_4242",
            _ => false,
        });
        assert!(
            cited_sibling,
            "expected EvidenceRef::Commit citing sibling_sha_4242; got {:?}",
            f.evidence
        );
    }

    /// Step-3 RED — P5 merged-arm: all deliverable files present on main despite
    /// no task_completed event and no reachable task-id commit. (b) rescue.
    ///
    /// Fixture:
    /// - NO task_completed event for "4285"
    /// - kind="merged"; commit="reaped_sha_4285" (not an ancestor)
    /// - log_grep("main","4285") → [] so (a) rescue does NOT fire
    /// - Every metadata.files entry set_path_tracked_on true → (b) fires
    ///
    /// Expected: exactly one P5PhantomDone at Severity::Low whose summary
    /// mentions "deliverable" and "present".
    ///
    /// Fails after step-2 impl (still High because (a) is empty and merged
    /// arm has no (b) rescue yet).
    #[test]
    fn merged_no_event_deliverable_present_downgrades_to_low() {
        let conn = seed_db();
        // No task_completed event.

        let mut git = MockGitOps::new();
        // Claimed commit not an ancestor.
        git.set_is_ancestor("reaped_sha_4285", "main", false);
        // log_grep returns empty → (a) rescue does NOT fire.
        git.set_log_grep("main", "4285", vec![]);
        // Empty diff for the claimed commit.
        git.set_diff_changed_paths("main", "reaped_sha_4285", vec![]);
        // ALL metadata.files entries are tracked on main → (b) should fire.
        git.set_path_tracked_on("main", "crates/x/foo.rs", true);
        git.set_path_tracked_on("main", "crates/x/bar.rs", true);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4285".to_string(),
            TaskMetadata {
                task_id: "4285".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/x/foo.rs".to_string(),
                    "crates/x/bar.rs".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("reaped_sha_4285".to_string()),
                    note: None,
                }),
                title: "Wire foo and bar".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "all deliverables present on main → must downgrade to Low; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "4285");
        let s = f.summary.to_lowercase();
        assert!(
            s.contains("deliverable") && s.contains("present"),
            "summary should mention 'deliverable' and 'present'; got {:?}",
            f.summary
        );
    }

    /// Step-1 RED — P5 merged-arm: task-id-referencing commit reachable from
    /// main despite no task_completed event in runs.db. Models the task-4284
    /// false-positive: the merge commit `4f7b633f` is reachable on main but
    /// the branch ref was reaped, so the claimed commit SHA is unresolvable
    /// and is_ancestor returns false.
    ///
    /// Fixture:
    /// - NO task_completed event in runs.db for "4284"
    /// - kind="merged"; commit="reaped_sha_4284" (not an ancestor of main)
    /// - log_grep("main","4284") → [GitCommit{sha:"4f7b633f", ...}]  ← (a) fires
    /// - metadata.files non-empty; path_tracked_on defaults false         ← (b) does not fire
    ///
    /// Expected: exactly one P5PhantomDone at Severity::Low whose summary
    /// mentions the work landed/is reachable on main and whose evidence
    /// includes an EvidenceRef::Commit citing "4f7b633f".
    ///
    /// Fails on current code (merged Ok(false) + non-ancestor → High
    /// "no task_completed event").
    #[test]
    fn merged_no_event_task_id_commit_reachable_downgrades_to_low() {
        let conn = seed_db();
        // Deliberately do NOT insert a task_completed event for "4284".

        let mut git = MockGitOps::new();
        // The claimed commit is NOT an ancestor of main (branch was reaped).
        git.set_is_ancestor("reaped_sha_4284", "main", false);
        // log_grep finds the real merge commit on main — (a) rescue should fire.
        let merge_commit = GitCommit {
            sha: "4f7b633f".to_string(),
            subject: "Merge task/4284 into main".to_string(),
        };
        git.set_log_grep("main", "4284", vec![merge_commit]);
        // Provide empty diff so the git-diff leg produces nothing useful.
        git.set_diff_changed_paths("main", "reaped_sha_4284", vec![]);
        // metadata.files has a real entry but path_tracked_on defaults false,
        // so (b) does NOT rescue.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "4284".to_string(),
            TaskMetadata {
                task_id: "4284".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/deliver.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("reaped_sha_4284".to_string()),
                    note: None,
                }),
                title: "Wire deliver into bar".to_string(),
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
            producer_branch: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "task-id commit reachable on main → must downgrade to Low; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "4284");
        // Summary must mention reachability / landed / not phantom-done.
        let s = f.summary.to_lowercase();
        assert!(
            s.contains("reachable") || s.contains("landed") || s.contains("not phantom"),
            "summary should mention reachable/landed/not phantom; got {:?}",
            f.summary
        );
        // Evidence must include EvidenceRef::Commit citing the merge commit sha.
        let cited_commit = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::Commit { sha, .. } => Some(sha.as_str()),
            _ => None,
        });
        assert_eq!(
            cited_commit,
            Some("4f7b633f"),
            "Low finding must include EvidenceRef::Commit {{ sha: '4f7b633f' }}; got {:?}",
            f.evidence
        );
    }

    /// H1 FP guard — domain-noun "stub" in a test fn name that tests a
    /// LEGITIMATE stub-kernel module, NOT a placeholder test.
    ///
    /// Real corpus pattern: `stub_kernel_export_returns_error`
    /// (crates/reify-kernel-occt/src/stubs.rs). The word "stub" names the
    /// product module (a kernel stub / shim layer), not a not-yet-implemented
    /// test body. The fn body asserts an error code via assert_eq!(result, 0)
    /// (a zero error code meaning success — legitimate empty/zero assertion).
    ///
    /// RED against the current two-gate: fn_name.contains("stub") is true, and
    /// "assert_eq!(result, 0)" is in EMPTY_ASSERTION_PATTERNS — so the two-gate
    /// fires, a false positive. After the three-signal gate the name lacks any
    /// empty-intent token and is correctly suppressed.
    #[test]
    fn h1_domain_noun_stub_kernel_not_flagged() {
        // Real corpus pattern: "stub" is the product module name (kernel stub/shim),
        // not a marker for an unimplemented test. assert_eq!(result, 0) is a
        // legitimate zero-error-code assertion. Use the bare form (no message arg)
        // so the EMPTY_ASSERTION_PATTERNS substring "assert_eq!(result, 0)" matches.
        let findings = run_h1_single_file(
            "H1DN2",
            "crates/reify-kernel-occt/tests/stub_tests.rs",
            vec![
                (1, "    #[test]".to_string()),
                (2, "    fn stub_kernel_export_returns_error() {".to_string()),
                (3, "        let result = stub_kernel_export();".to_string()),
                (4, "        assert_eq!(result, 0);".to_string()),
                (5, "    }".to_string()),
            ],
        );
        assert!(
            findings.is_empty(),
            "domain-noun 'stub' (stub-kernel module fn) must NOT be flagged as \
             phantom-done; got {:?}",
            findings
        );
    }
}


} // mod p5
