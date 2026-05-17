//! Integration smoke test for the F-infra slice 1 detector core.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §14 hand-off):
//!   `cargo test -p reify-audit --test audit_integration`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary (the binary's own filename is not
//! part of those paths). To make the substring `audit_integration::tests`
//! resolve, the file body is wrapped in
//! `mod audit_integration { mod tests { ... } }` so each test's path
//! becomes `audit_integration::tests::<name>` — matching the p5.rs/p1.rs/p2.rs
//! convention.
//!
//! ## Relationship to per-detector test files
//!
//! `tests/p5.rs`, `tests/p1.rs`, and `tests/p2.rs` are the authoritative
//! detectors for their respective detectors and already cover each scenario in
//! depth. The three single-detector tests here (`p5_phantom_done_task_3242_*`,
//! `p1_producer_orphan_c04_*`, `p2_consumer_stub_c39_*`) are minimal smoke
//! anchors that confirm this binary compiles and links against the public lib
//! surface correctly — they are intentionally thin.
//!
//! The genuinely unique contribution of this binary is the
//! `seven_prepd_legacy_tasks_produce_no_false_positives` test, which exercises
//! a single shared `AuditContext` against all three detectors simultaneously and
//! asserts no cross-detector false positives on the pre-`/prd` legacy task shape.
//! That cross-detector scenario cannot be expressed as a single test in any of
//! the per-detector files.
//!
//! All fixtures are in-memory; no git repo, no runs.db file required.

mod common;

mod audit_integration {

use crate::common::schema::*;
use crate::common::fixtures::legacy_meta;
use reify_audit::{
    AuditContext, ChangedSymbol, DoneProvenance, EvidenceRef, MockGitOps, MockJCodemunchOps,
    Pattern, Severity, TaskMetadata, p1_producer_orphan, p2_consumer_stub, p5_phantom_done,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// Fixed synthetic "now" (epoch-seconds) so grace-window boundaries are
/// deterministic across runs. Tests derive `done_at` relative to this.
/// Mirrors `const NOW` in p1.rs:29.
const NOW: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

mod tests {
    use super::*;

    /// False-positive cross-check — seven pre-`/prd` legacy tasks (sourced
    /// from the phase-3-files-synthesis.md cluster index) must produce zero
    /// findings across all three detectors in a single shared AuditContext.
    ///
    /// Each fixture defeats a distinct false-positive guard; the guard
    /// inventory is:
    ///
    ///  - task 215  → P5 guard A3: kind=merged + DB event + diff covers files
    ///                → `missing.is_empty()` → P5 returns `None`.
    ///  - task 250  → P5 guard A2: kind=found_on_main + `files=[]` → empty-files
    ///                early-return before any git-diff call.
    ///  - task 2347 → P1 guard B3: `audit_foundation=Some(true)` → P1
    ///                short-circuits before per-symbol iteration (symbol mock
    ///                seeded so the guard is demonstrably load-bearing).
    ///  - task 2358 → P1 guard B4: `prd` + paired pending consumer (task 2358_c,
    ///                `consumer_ref` matching 2358's `prd`) → `has_pending_consumer`
    ///                returns true → P1 short-circuits.
    ///  - task 2658 → P1 guard B5: changed-symbol file in `crates/reify-stdlib/`
    ///                → stdlib scope-exclude fires → P1 skips the symbol.
    ///  - task 2699 → P1 guard B7: `g_allow_marker=Some("non-blank")` →
    ///                `is_g_allow_suppressed` returns true → P1 skips the symbol.
    ///  - task 2954 → P2 guard C2: `files=["crates/x/tests/foo.rs"]` →
    ///                `is_test_path` fires → P2 skips the diff scan entirely.
    ///
    /// The DB is seeded so fixture 1 (task 215) can satisfy
    /// `has_task_completed_event`; the other six fixtures and the 8th paired
    /// consumer (2358_c) all early-return before any DB query — five on
    /// `done_provenance.as_ref()?` (guard A1), one on `meta.files.is_empty()`
    /// (guard A2, task 250), and 2358_c on `status != "done"`.
    ///
    /// Task IDs trace to the phase-3-files-synthesis.md cluster index:
    /// 215 (C-24), 250 (C-07), 2347 (C-07 doc), 2358 (C-36),
    /// 2658 (C-04 substitute — avoids reusing 2657 from the P2 positive
    /// fixture in step-3), 2699 (C-07 dispatch), 2954 (C-07 docs-only).
    #[test]
    fn seven_prepd_legacy_tasks_produce_no_false_positives() {
        let conn = seed_db();
        // Seed the DB so fixture 1 (task 215, P5 corroborated path) can pass
        // has_task_completed_event. Fixtures 2–7 and the paired consumer
        // task 2358_c all have done_provenance=None or non-"merged" kind,
        // so they early-return before any DB query.
        insert_task_completed_event(&conn, "215");

        // Fixture 1 (task 215, guard A3): primary diff covers the only claimed
        // file → missing.is_empty() → P5 returns None (no phantom-done finding).
        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "sha_215",
            vec!["crates/x/215_file.rs".to_string()],
        );

        // Fixtures 3–6 (tasks 2347/2358/2658/2699) are P1 producers. Each has
        // a distinct done_at to avoid MockJCodemunchOps key collisions on
        // (branch, since_epoch). All four timestamps are >14 days past NOW so
        // the absent guards would emit Medium findings if not suppressed —
        // making each fixture's guard exercise load-bearing.
        let mut jc = MockJCodemunchOps::new();

        // Fixture 3 (task 2347, guard B3 audit_foundation=Some(true)): symbol
        // is seeded so that without the audit_foundation guard P1 would reach
        // per-symbol iteration; with the guard it short-circuits first.
        jc.set_changed_symbols(
            "main",
            NOW - 20 * DAY,
            vec![ChangedSymbol {
                name: "would_be_orphan".to_string(),
                file: "crates/reify-x/src/foo.rs".to_string(),
                line: 10,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // Fixture 4 (task 2358, guard B4 pending-consumer-ref): symbol seeded
        // so P1 would reach per-symbol iteration without the consumer-ref guard.
        jc.set_changed_symbols(
            "main",
            NOW - 25 * DAY,
            vec![ChangedSymbol {
                name: "producer_fn".to_string(),
                file: "crates/reify-y/src/bar.rs".to_string(),
                line: 20,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // Fixture 5 (task 2658, guard B5 stdlib scope-exclude): file is in
        // crates/reify-stdlib/ → P1 skips the symbol before checking callers.
        jc.set_changed_symbols(
            "main",
            NOW - 30 * DAY,
            vec![ChangedSymbol {
                name: "stdlib_def".to_string(),
                file: "crates/reify-stdlib/src/foo.ri".to_string(),
                line: 5,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // Fixture 6 (task 2699, guard B7 G-allow marker): g_allow_marker is
        // non-blank → is_g_allow_suppressed returns true → P1 skips the symbol.
        jc.set_changed_symbols(
            "main",
            NOW - 35 * DAY,
            vec![ChangedSymbol {
                name: "g_allowed_fn".to_string(),
                file: "crates/reify-z/src/baz.rs".to_string(),
                line: 7,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: Some("// G-allow: consumed by upcoming PRD consumer".to_string()),
            }],
        );

        let mut task_metadata = HashMap::new();

        // Fixture 1 — task 215, guard A3 (P5 corroborated path):
        // kind=merged + task_completed event in DB + primary diff covers
        // the file → missing.is_empty() → P5 returns None.
        // done_at=None → P1 early-returns (guard B2).
        task_metadata.insert(
            "215".to_string(),
            TaskMetadata {
                task_id: "215".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/215_file.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("sha_215".to_string()),
                    note: None,
                }),
                done_at: None,
                ..legacy_meta("215")
            },
        );

        // Fixture 2 — task 250, guard A2 (P5 found_on_main + empty files):
        // kind=found_on_main skips the DB check; meta.files.is_empty() fires
        // → P5 returns None. done_at=None → P1 early-returns (guard B2).
        task_metadata.insert(
            "250".to_string(),
            TaskMetadata {
                task_id: "250".to_string(),
                status: "done".to_string(),
                files: vec![],
                done_provenance: Some(DoneProvenance {
                    kind: Some("found_on_main".to_string()),
                    commit: None,
                    note: None,
                }),
                done_at: None,
                ..legacy_meta("250")
            },
        );

        // Fixture 3 — task 2347, guard B3 (P1 audit_foundation=Some(true)):
        // P1 short-circuits before iterating changed_symbols, even though
        // set_changed_symbols was seeded for NOW-20*DAY (making the guard
        // exercise load-bearing — without it P1 would flag the symbol).
        // done_provenance=None (from legacy_meta) → P5 early-returns (guard A1).
        task_metadata.insert(
            "2347".to_string(),
            TaskMetadata {
                task_id: "2347".to_string(),
                audit_foundation: Some(true),
                done_at: Some(NOW - 20 * DAY),
                ..legacy_meta("2347")
            },
        );

        // Fixture 4 — task 2358, guard B4 (P1 pending-consumer-ref):
        // has_pending_consumer matches task 2358_c's consumer_ref against this
        // prd → P1 short-circuits before iterating changed_symbols.
        // done_provenance=None → P5 early-returns (guard A1).
        task_metadata.insert(
            "2358".to_string(),
            TaskMetadata {
                task_id: "2358".to_string(),
                prd: Some("docs/feature-x.md".to_string()),
                done_at: Some(NOW - 25 * DAY),
                ..legacy_meta("2358")
            },
        );
        // 8th paired pending-consumer task (not one of the seven asserted tasks):
        // status=pending → P5/P1 skip (status guards); files=[] → P2 sees no work.
        task_metadata.insert(
            "2358_c".to_string(),
            TaskMetadata {
                task_id: "2358_c".to_string(),
                status: "pending".to_string(),
                consumer_ref: Some("docs/feature-x.md".to_string()),
                ..legacy_meta("2358_c")
            },
        );

        // Fixture 5 — task 2658, guard B5 (P1 stdlib scope-exclude):
        // changed_symbols file starts with crates/reify-stdlib/ → P1 skips
        // the symbol without calling find_references.
        // done_provenance=None → P5 early-returns (guard A1).
        task_metadata.insert(
            "2658".to_string(),
            TaskMetadata {
                task_id: "2658".to_string(),
                done_at: Some(NOW - 30 * DAY),
                ..legacy_meta("2658")
            },
        );

        // Fixture 6 — task 2699, guard B7 (P1 G-allow marker):
        // changed_symbols entry has g_allow_marker=Some("non-blank") →
        // is_g_allow_suppressed returns true → P1 skips the symbol.
        // done_provenance=None → P5 early-returns (guard A1).
        task_metadata.insert(
            "2699".to_string(),
            TaskMetadata {
                task_id: "2699".to_string(),
                done_at: Some(NOW - 35 * DAY),
                ..legacy_meta("2699")
            },
        );

        // Fixture 7 — task 2954, guard C2 (P2 is_test_path):
        // files=[".../tests/foo.rs"] → P2 skips the path before calling
        // diff_added_lines → no stub-marker scan → no findings.
        // done_provenance=None → P5 early-returns (guard A1).
        // done_at=None → P1 early-returns (guard B2).
        task_metadata.insert(
            "2954".to_string(),
            TaskMetadata {
                task_id: "2954".to_string(),
                files: vec!["crates/x/tests/foo.rs".to_string()],
                ..legacy_meta("2954")
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };

        let p5_findings = p5_phantom_done::check(&ctx);
        assert!(
            p5_findings.is_empty(),
            "P5: expected no findings on seven legacy tasks; got {:?}",
            p5_findings
        );

        let p1_findings = p1_producer_orphan::check(&ctx);
        assert!(
            p1_findings.is_empty(),
            "P1: expected no findings on seven legacy tasks; got {:?}",
            p1_findings
        );

        let p2_findings = p2_consumer_stub::check(&ctx);
        assert!(
            p2_findings.is_empty(),
            "P2: expected no findings on seven legacy tasks; got {:?}",
            p2_findings
        );
    }

    /// Seeded incident #3 — P2 consumer-stub, synthetic cluster C-39 shape:
    /// Manifold `KernelAttributeHook::propagate_attributes` done task whose
    /// added diff introduces BOTH `tracing::warn!(reason="task_9_pending", …)`
    /// AND `unimplemented!()` on the same file. Both stub markers must appear
    /// in the single consolidated finding (per-file aggregation).
    #[test]
    fn p2_consumer_stub_c39_manifold_hook_shape_flags_both_markers() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        // P2 issues zero SQL; in-memory connection satisfies AuditContext.conn.
        let hook_path = "crates/reify-kernel-manifold/src/attr_hook.rs";

        let mut git = MockGitOps::new();
        // The task branch's diff of attr_hook.rs adds two stub lines that
        // mirror the actual C-39 cluster body (task_9_pending + unimplemented!).
        git.set_diff_added_lines(
            "main",
            "task/2657",
            hook_path,
            vec![
                (
                    15,
                    "        tracing::warn!(reason=\"task_9_pending\", \"manifold hook deferred\")"
                        .to_string(),
                ),
                (16, "        unimplemented!()".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "2657".to_string(),
            TaskMetadata {
                task_id: "2657".to_string(),
                status: "done".to_string(),
                files: vec![hook_path.to_string()],
                done_provenance: None,
                // Benign title: no "stub"/"placeholder" → Medium not Low.
                title: "Wire Manifold attribute hook".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new(); // P2 ignores jcodemunch
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

        let findings = p2_consumer_stub::check(&ctx);
        // P2 aggregates per (task, file): two stub lines on one file → one finding.
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one P2 finding (dual-marker on one file); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P2ConsumerStub, "wrong pattern: {:?}", f);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "benign title → Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "2657");
        // Evidence must reference the hook path.
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == hook_path
            )),
            "expected EvidenceRef::File for attr_hook.rs; got {:?}",
            f.evidence
        );
        // The §14 hand-off requirement: summary must contain BOTH canonical labels.
        assert!(
            f.summary.contains("tracing::warn!(task_pending)"),
            "summary must contain tracing::warn! label; got: {:?}",
            f.summary
        );
        assert!(
            f.summary.contains("unimplemented!"),
            "summary must contain unimplemented! label; got: {:?}",
            f.summary
        );
    }

    /// Seeded incident #2 — P1 producer-orphan, synthetic cluster C-04 shape:
    /// public symbol `resolve_unique_by_attribute` introduced by a done task
    /// 15 days ago (past the 14-day grace window); zero workspace callers;
    /// no audit_foundation or pending consumer task → exactly one Medium finding.
    #[test]
    fn p1_producer_orphan_c04_shape_flagged_medium_post_grace() {
        // 15 days past done-flip: strictly beyond the 14-day grace window.
        let done_at = NOW - 15 * DAY;

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        // P1 issues zero SQL; in-memory connection satisfies AuditContext.conn.
        let git = MockGitOps::new(); // P1 doesn't touch git

        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            "main",
            done_at,
            vec![ChangedSymbol {
                name: "resolve_unique_by_attribute".to_string(),
                file: "crates/reify-eval/src/selector_resolution.rs".to_string(),
                line: 42,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // Zero callers → true orphan.
        jc.set_find_references("crates/reify-eval/src/selector_resolution.rs", "resolve_unique_by_attribute", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7301".to_string(),
            TaskMetadata {
                task_id: "7301".to_string(),
                status: "done".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Wire persistent naming resolution".to_string(),
                prd: Some("docs/persistent-naming-v2.md".to_string()),
                consumer_ref: None,
                audit_foundation: None,
                done_at: Some(done_at),
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };

        let findings = p1_producer_orphan::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one P1 finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan, "wrong pattern: {:?}", f);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "15 days > 14-day grace → Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "7301");
        // Evidence must include EvidenceRef::File for the symbol's declaring file.
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-eval/src/selector_resolution.rs"
            )),
            "expected EvidenceRef::File for selector_resolution.rs; got {:?}",
            f.evidence
        );
        // Severity::Medium + Pattern::P1ProducerOrphan already prove the post-grace
        // branch executed; a summary-substring check would pin prose rather than
        // behaviour and is already covered by tests/p1.rs.
    }

    /// Seeded incident #1 — P5 phantom-done, May-09 task-3242 shape:
    /// kind=merged, claimed commit's diff does NOT cover `metadata.files`;
    /// `git log main --grep` returns empty (no sibling-FF rescue).
    ///
    /// Re-exercises the same incident shape as
    /// `p5::tests::task_3242_shape_returns_high_severity_phantom_done` (in
    /// `tests/p5.rs`) but via the integration-test binary's public-lib
    /// invocation path rather than the unit-test seam.
    #[test]
    fn p5_phantom_done_task_3242_shape_flagged_high() {
        let conn = seed_db();
        // task_completed event is present (the orchestrator wrote one) —
        // the phantom-done signal is in the *git* corroboration, not the DB.
        insert_task_completed_event(&conn, "3242");

        let mut git = MockGitOps::new();
        // No sibling-FF rescue: log_grep returns empty.
        git.set_log_grep("main", "3242", vec![]);
        // Claimed commit's diff covers only an unrelated path; the
        // `metadata.files` entry (pruning.rs) is absent → phantom-done.
        git.set_diff_changed_paths(
            "main",
            "7958491da22f",
            vec!["docs/unrelated.md".to_string()],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "3242".to_string(),
            TaskMetadata {
                task_id: "3242".to_string(),
                status: "done".to_string(),
                // The actual May-09 3242 incident path from the task write-up.
                files: vec!["crates/reify-shell-extract/src/pruning.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("7958491da22f".to_string()),
                    note: None,
                }),
                title: "Wire shell-extract pruning".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new(); // P5 ignores jcodemunch
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
        assert_eq!(f.pattern, Pattern::P5PhantomDone, "wrong pattern: {:?}", f);
        assert_eq!(f.severity, Severity::High, "expected High severity; got {:?}", f.severity);
        assert_eq!(f.task_id, "3242");

        // Evidence must include a MetadataFiles ref naming the missing path.
        let meta_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = meta_files_evidence.expect("MetadataFiles evidence must be present");
        assert!(
            entries.iter().any(|p| p == "crates/reify-shell-extract/src/pruning.rs"),
            "expected pruning.rs in MetadataFiles evidence; got {:?}",
            entries
        );
    }
}

} // mod audit_integration
