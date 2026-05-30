//! Integration tests for the P1 producer-orphan detector.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §5 P1):
//!   `cargo test -p reify-audit p1::tests`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary. To make the substring `p1::tests`
//! resolve, the file body is wrapped in `mod p1 { mod tests { ... } }` so
//! each test's path becomes `p1::tests::<name>` — matching the p5.rs/p2.rs
//! convention.
//!
//! All tests use in-memory rusqlite + MockJCodemunchOps + MockGitOps so they
//! remain hermetic (P1 issues zero SQL; the in-memory connection satisfies
//! AuditContext.conn without requiring a schema).

mod p1 {

use reify_audit::{
    AuditContext, ChangedSymbol, DeadSymbol, DoneProvenance, EvidenceRef, Finding, JCodemunchOps,
    LayerViolation, MockGitOps, MockJCodemunchOps, Pattern, Severity, SymbolReference,
    TaskMetadata, UntestedSymbol, p1_producer_orphan,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// A fixed synthetic "now" (epoch-seconds) so grace-window boundaries are
/// deterministic. Tests derive `done_at` relative to this.
const NOW: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

/// Build a `done` producer task with a benign title (does NOT signal a
/// stub) and the given done-flip timestamp + originating PRD.
///
/// `done_provenance.commit` is derived from `task_id` as `"sha_{task_id}"`.
/// This lets each test compute the expected `(since_sha, until_sha)` range
/// as `(format!("{sha}^1"), sha)` without an out-of-band sha table.
/// `done_at` still governs grace-window math — the two fields are orthogonal.
fn done_meta(task_id: &str, done_at: i64, prd: Option<&str>) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        status: "done".to_string(),
        files: vec![],
        done_provenance: Some(DoneProvenance {
            kind: Some("merged".to_string()),
            commit: Some(format!("sha_{task_id}")),
            note: None,
        }),
        title: "Wire foo into bar".to_string(),
        prd: prd.map(|s| s.to_string()),
        consumer_ref: None,
        audit_foundation: None,
        done_at: Some(done_at),
    }
}

/// Derive the commit SHA for a task from its task_id (mirrors done_meta's
/// `done_provenance.commit` convention: `"sha_{task_id}"`).
fn task_sha(task_id: &str) -> String {
    format!("sha_{task_id}")
}

/// Build a `ChangedSymbol` with no suppression metadata (the orphan-candidate
/// default); individual tests flip `has_*` / `g_allow_marker` as needed.
fn changed_symbol(name: &str, file: &str) -> ChangedSymbol {
    ChangedSymbol {
        name: name.to_string(),
        file: file.to_string(),
        line: 42,
        has_allow_dead_code: false,
        has_cfg_test: false,
        g_allow_marker: None,
    }
}

mod tests {
    use super::*;

    /// Pin the public-API surface P1 adds by destructuring or exhaustively
    /// matching each new type. Adding a `Pattern` variant without an arm,
    /// renaming a `ChangedSymbol`/`SymbolReference` field, or changing the
    /// `AuditContext`/`TaskMetadata` shape will fail this test at compile
    /// time — exactly what downstream crates (T-4 CLI) need from a stable API.
    #[test]
    fn api_surface_pin() {
        // Pattern: spot-check that P1ProducerOrphan is accessible from this crate.
        // The exhaustive all-variants match lives in tests/p5.rs::api_surface_pin
        // (the canonical home) so future variant additions only require updating
        // one test file rather than two.
        let _: Pattern = Pattern::P1ProducerOrphan;

        // ChangedSymbol / SymbolReference: destructure every field by name.
        let ChangedSymbol {
            name: _,
            file: _,
            line: _,
            has_allow_dead_code: _,
            has_cfg_test: _,
            g_allow_marker: _,
        } = ChangedSymbol {
            name: "new_widget".to_string(),
            file: "crates/reify-x/src/widget.rs".to_string(),
            line: 42,
            has_allow_dead_code: false,
            has_cfg_test: false,
            g_allow_marker: None,
        };
        let SymbolReference { file: _, line: _ } = SymbolReference {
            file: "crates/reify-y/src/uses_widget.rs".to_string(),
            line: 7,
        };

        // AuditContext: populate the new `jcodemunch` + `now` fields.
        // TaskMetadata: populate the existing `title` + the 4 new P1 fields.
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "1".to_string(),
            TaskMetadata {
                task_id: "1".to_string(),
                status: "done".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Wire foo into bar".to_string(),
                prd: Some("docs/x.md".to_string()),
                consumer_ref: None,
                audit_foundation: None,
                done_at: Some(0),
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
            now: None,
            producer_branch: None,
        };

        // Default-empty mock → no changed symbols → no findings.
        let findings: Vec<Finding> = p1_producer_orphan::check(&ctx);
        assert!(
            findings.is_empty(),
            "default-empty mock must yield no findings; got {:?}",
            findings
        );

        // Severity remains reachable (pin alongside Pattern).
        for s in [Severity::Low, Severity::Medium, Severity::High] {
            match s {
                Severity::Low | Severity::Medium | Severity::High => {}
            }
        }
    }

    /// Required #1 — a `done` task introduced a public symbol with zero
    /// references, no pending consumer task, and a done-flip 15 days old
    /// (past the 14-day grace window) → exactly one Medium P1 finding
    /// citing the symbol's file via `EvidenceRef::File`.
    #[test]
    fn producer_orphan_flagged_medium_after_grace_window() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("7001");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![changed_symbol("new_widget", "crates/reify-x/src/widget.rs")],
        );
        jc.set_find_references("crates/reify-x/src/widget.rs", "new_widget", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7001".to_string(),
            done_meta("7001", done_at, Some("docs/x.md")),
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
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "15 days > 14-day grace → Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "7001");
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/widget.rs"
            )),
            "expected EvidenceRef::File for widget.rs; got {:?}",
            f.evidence
        );
    }

    /// Required #3 — a non-test workspace caller suppresses the finding,
    /// but a test-only reference does NOT (the filter excludes *test* refs,
    /// not all refs). One done task, two symbols: `new_widget` has a real
    /// caller (suppressed); `test_only_widget` is referenced only from a
    /// `*/tests/*` path (still flagged → exactly one surviving finding).
    #[test]
    fn non_test_caller_in_workspace_suppresses_finding() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("7003");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![
                changed_symbol("new_widget", "crates/reify-x/src/widget.rs"),
                changed_symbol("test_only_widget", "crates/reify-x/src/other.rs"),
            ],
        );
        // Real non-test caller → new_widget suppressed.
        jc.set_find_references(
            "crates/reify-x/src/widget.rs",
            "new_widget",
            vec![SymbolReference {
                file: "crates/reify-y/src/uses_widget.rs".to_string(),
                line: 7,
            }],
        );
        // Only a test-path caller → test_only_widget still flagged.
        jc.set_find_references(
            "crates/reify-x/src/other.rs",
            "test_only_widget",
            vec![SymbolReference {
                file: "crates/reify-y/tests/it.rs".to_string(),
                line: 3,
            }],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7003".to_string(),
            done_meta("7003", done_at, Some("docs/x.md")),
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
            "only test_only_widget should be flagged (new_widget has a \
             non-test caller); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan);
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/other.rs"
            )),
            "surviving finding must cite other.rs (test_only_widget); got {:?}",
            f.evidence
        );
        assert!(
            !f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/widget.rs"
            )),
            "new_widget (real caller) must not appear; got {:?}",
            f.evidence
        );
    }

    /// Required #2 — a producer-orphan whose done-flip is only 5 days old
    /// (inside the 14-day grace window) is flagged Low, not Medium, with a
    /// summary that mentions the grace window for human readers.
    #[test]
    fn low_severity_inside_grace_window() {
        let done_at = NOW - 5 * DAY;
        let sha = task_sha("7002");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![changed_symbol("fresh_widget", "crates/reify-x/src/fresh.rs")],
        );
        jc.set_find_references("crates/reify-x/src/fresh.rs", "fresh_widget", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7002".to_string(),
            done_meta("7002", done_at, Some("docs/x.md")),
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
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan);
        assert_eq!(
            f.severity,
            Severity::Low,
            "5 days < 14-day grace → Low; got {:?}",
            f.severity
        );
        assert!(
            f.summary.to_lowercase().contains("grace window"),
            "summary must mention the grace window for human readers; got {:?}",
            f.summary
        );
    }

    /// Required #4 — two independent suppression guards, each exercised in
    /// its own AuditContext within this test (both past grace, zero refs):
    ///   (a) `audit_foundation: Some(true)` on the producing task suppresses
    ///       the orphan entirely (foundation/scaffold task);
    ///   (b) a `pending` consumer task whose `consumer_ref` matches the
    ///       producer's `prd` suppresses it (a downstream consumer is queued).
    #[test]
    fn audit_foundation_metadata_suppresses_finding() {
        let done_at = NOW - 15 * DAY;

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        // Seed symbols for task 8001 (foundation guard fires first — commit-range
        // query would never be reached, but the fixture is keyed correctly).
        let sha_8001 = task_sha("8001");
        jc.set_changed_symbols(
            &format!("{sha_8001}^1"),
            &sha_8001,
            vec![changed_symbol(
                "scaffold_widget",
                "crates/reify-x/src/scaffold.rs",
            )],
        );
        jc.set_find_references("crates/reify-x/src/scaffold.rs", "scaffold_widget", vec![]);

        // (a) Foundation task → suppressed.
        let mut tm_foundation = HashMap::new();
        tm_foundation.insert(
            "8001".to_string(),
            TaskMetadata {
                audit_foundation: Some(true),
                ..done_meta("8001", done_at, Some("docs/x.md"))
            },
        );
        let ctx_a = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: tm_foundation,
            target_task_id: None,
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };
        let findings_a = p1_producer_orphan::check(&ctx_a);
        assert!(
            findings_a.is_empty(),
            "audit_foundation=true must suppress the orphan; got {:?}",
            findings_a
        );

        // (b) Pending consumer task referencing the producer's PRD → suppressed.
        let mut tm_consumer = HashMap::new();
        tm_consumer.insert(
            "8002".to_string(),
            done_meta("8002", done_at, Some("docs/x.md")),
        );
        tm_consumer.insert(
            "8003".to_string(),
            TaskMetadata {
                task_id: "8003".to_string(),
                status: "pending".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Consume the widget".to_string(),
                prd: None,
                consumer_ref: Some("docs/x.md".to_string()),
                audit_foundation: None,
                done_at: None,
            },
        );
        let ctx_b = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: tm_consumer,
            target_task_id: None,
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };
        let findings_b = p1_producer_orphan::check(&ctx_b);
        assert!(
            findings_b.is_empty(),
            "a pending consumer task referencing the producer PRD must \
             suppress the orphan; got {:?}",
            findings_b
        );
    }

    /// Required #5 — a non-blank `// G-allow:` marker on the symbol
    /// suppresses the finding; a blank marker (`Some("")`) does NOT,
    /// pinning the script's `//\s*G-allow:\s*(.+)` rule where `(.+)`
    /// requires non-empty content. One done task, two symbols → exactly
    /// one surviving finding (the blank-marker one).
    #[test]
    fn g_allow_marker_suppresses_finding() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("9001");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![
                ChangedSymbol {
                    g_allow_marker: Some(
                        "F-infra T-4 CLI consumer (crates/reify-audit-cli)".to_string(),
                    ),
                    ..changed_symbol("marked_widget", "crates/reify-x/src/marked.rs")
                },
                ChangedSymbol {
                    g_allow_marker: Some(String::new()),
                    ..changed_symbol("blank_marked_widget", "crates/reify-x/src/blank.rs")
                },
            ],
        );
        jc.set_find_references("crates/reify-x/src/marked.rs", "marked_widget", vec![]);
        jc.set_find_references("crates/reify-x/src/blank.rs", "blank_marked_widget", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "9001".to_string(),
            done_meta("9001", done_at, Some("docs/x.md")),
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
            "only the blank-marker symbol should flag (a non-blank marker \
             suppresses); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan);
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/blank.rs"
            )),
            "surviving finding must cite blank.rs; got {:?}",
            f.evidence
        );
        assert!(
            !f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/marked.rs"
            )),
            "marked_widget (non-blank G-allow) must not appear; got {:?}",
            f.evidence
        );
    }

    /// Coverage for the three per-symbol false-positive guards the required
    /// suite never exercises (the `changed_symbol` helper leaves them at
    /// their orphan-candidate defaults, so no other test flips them). One
    /// done task past grace, four symbols, zero workspace refs:
    ///   - `stdlib_widget` under `crates/reify-stdlib/` → scope-excluded;
    ///   - `dead_widget` with `has_allow_dead_code` → opt-out;
    ///   - `cfg_test_widget` with `has_cfg_test` → test-only;
    ///   - `live_widget` (no guard) → the only surviving finding.
    ///
    /// Inverting any guard boolean or changing the stdlib prefix makes the
    /// count != 1 and fails here — catching exactly the regressions that
    /// cause audit false-positive floods.
    #[test]
    fn per_symbol_guards_suppress_individually() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("7100");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![
                changed_symbol("stdlib_widget", "crates/reify-stdlib/src/prelude.rs"),
                ChangedSymbol {
                    has_allow_dead_code: true,
                    ..changed_symbol("dead_widget", "crates/reify-x/src/dead.rs")
                },
                ChangedSymbol {
                    has_cfg_test: true,
                    ..changed_symbol("cfg_test_widget", "crates/reify-x/src/cfgt.rs")
                },
                changed_symbol("live_widget", "crates/reify-x/src/live.rs"),
            ],
        );
        jc.set_find_references("crates/reify-stdlib/src/prelude.rs", "stdlib_widget", vec![]);
        jc.set_find_references("crates/reify-x/src/dead.rs", "dead_widget", vec![]);
        jc.set_find_references("crates/reify-x/src/cfgt.rs", "cfg_test_widget", vec![]);
        jc.set_find_references("crates/reify-x/src/live.rs", "live_widget", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7100".to_string(),
            done_meta("7100", done_at, Some("docs/x.md")),
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
            "only live_widget should survive the per-symbol guards; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan);
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/live.rs"
            )),
            "surviving finding must cite live.rs; got {:?}",
            f.evidence
        );
    }

    /// Pins the grace-window boundary to the design's strict ">14 days"
    /// wording (`f-infra-design.md` §5 P1 line 83). Three done tasks, one
    /// symbol each, zero refs, ages straddling the boundary:
    ///   - exactly `14 * DAY` old  → Low (the boundary is *inside* the window);
    ///   - `14 * DAY - 1` old      → Low;
    ///   - `14 * DAY + 1` old      → Medium.
    ///
    /// A `>=` regression (Medium at exactly 14 days) fails the first case.
    ///
    /// `done_at` (distinct per case) still governs the age/severity comparison;
    /// `done_provenance.commit` (also distinct per case — derived from `name`)
    /// governs the symbol-lookup range, pinning that the two fields are
    /// orthogonal.
    #[test]
    fn grace_window_boundary_is_strict() {
        const WINDOW: i64 = 14 * DAY;

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();

        let cases = [
            (WINDOW, "at_boundary", "crates/reify-x/src/at.rs", Severity::Low),
            (WINDOW - 1, "inside_window", "crates/reify-x/src/in.rs", Severity::Low),
            (WINDOW + 1, "past_window", "crates/reify-x/src/past.rs", Severity::Medium),
        ];

        for (age, name, file, expected) in cases {
            let done_at = NOW - age;
            let sha = task_sha(name);
            let mut jc = MockJCodemunchOps::new();
            jc.set_changed_symbols(&format!("{sha}^1"), &sha, vec![changed_symbol(name, file)]);
            jc.set_find_references(file, name, vec![]);

            let mut task_metadata = HashMap::new();
            task_metadata.insert(name.to_string(), done_meta(name, done_at, Some("docs/x.md")));

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
                "{}: expected exactly one finding; got {:?}",
                name,
                findings
            );
            assert_eq!(
                findings[0].severity, expected,
                "{}: age {}s vs window {}s → expected {:?}; got {:?}",
                name, age, WINDOW, expected, findings[0].severity
            );
        }
    }

    /// Step 3 (RED→GREEN via step 4) — when `target_task_id` is set, P1
    /// restricts scanning to that single task only (mirroring p2's guard).
    ///
    /// Two done orphan-candidate tasks (both 15 days past grace, zero refs),
    /// each introducing their own symbol. AuditContext built with
    /// `target_task_id: Some("task_A")`. Expected: exactly one finding, for
    /// task_A only; task_B's symbol is not reported.
    ///
    /// RED: current `check()` ignores target_task_id and scans both tasks,
    /// producing two findings.
    #[test]
    fn target_task_id_scopes_p1_to_one_task() {
        // Both tasks can share the same done_at — the mock now keys on
        // (since_sha, until_sha), so distinct task commits are sufficient.
        let done_at_a = NOW - 15 * DAY;
        let done_at_b = NOW - 15 * DAY;
        let sha_a = task_sha("task_A");
        let sha_b = task_sha("task_B");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        // task_A → widget_a (keyed by its commit range).
        jc.set_changed_symbols(
            &format!("{sha_a}^1"),
            &sha_a,
            vec![changed_symbol("widget_a", "crates/reify-x/src/a.rs")],
        );
        // task_B → widget_b (keyed by its commit range).
        jc.set_changed_symbols(
            &format!("{sha_b}^1"),
            &sha_b,
            vec![changed_symbol("widget_b", "crates/reify-x/src/b.rs")],
        );
        jc.set_find_references("crates/reify-x/src/a.rs", "widget_a", vec![]);
        jc.set_find_references("crates/reify-x/src/b.rs", "widget_b", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "task_A".to_string(),
            done_meta("task_A", done_at_a, Some("docs/a.md")),
        );
        task_metadata.insert(
            "task_B".to_string(),
            done_meta("task_B", done_at_b, Some("docs/b.md")),
        );

        // With target_task_id pointing at task_A, only task_A is scanned.
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: Some("task_A".to_string()),
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };

        let findings = p1_producer_orphan::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "with target_task_id=task_A, only task_A's orphan should fire; got {:?}",
            findings
        );
        assert_eq!(
            findings[0].task_id, "task_A",
            "finding must belong to task_A, not task_B; got {:?}",
            findings[0]
        );
    }

    /// Step 1 (RED→GREEN via step 2) — a consumer task with status=`review`
    /// whose `consumer_ref` matches the producer's `prd` must suppress the
    /// orphan finding, just like `pending`/`in-progress` consumers do.
    ///
    /// One done producer (15 days past grace, prd="docs/x.md", zero refs) +
    /// one `review`-status consumer task with `consumer_ref="docs/x.md"`.
    /// Expected: zero P1 findings.
    ///
    /// RED: current `has_pending_consumer` only matches `"pending"` |
    /// `"in-progress"`, so the `review` consumer is invisible and a finding fires.
    #[test]
    fn review_status_consumer_suppresses_finding() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("9100");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        // Consumer guard fires before the commit-range query — the fixture is
        // keyed correctly but would not be reached in practice.
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![changed_symbol("review_widget", "crates/reify-x/src/review.rs")],
        );
        jc.set_find_references("crates/reify-x/src/review.rs", "review_widget", vec![]);

        let mut task_metadata = HashMap::new();
        // The done producer.
        task_metadata.insert(
            "9100".to_string(),
            done_meta("9100", done_at, Some("docs/x.md")),
        );
        // The in-review consumer: status="review", consumer_ref matches producer's prd.
        task_metadata.insert(
            "9101".to_string(),
            TaskMetadata {
                task_id: "9101".to_string(),
                status: "review".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Consume the review widget".to_string(),
                prd: None,
                consumer_ref: Some("docs/x.md".to_string()),
                audit_foundation: None,
                done_at: None,
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
        assert!(
            findings.is_empty(),
            "a review-status consumer task referencing the producer PRD must \
             suppress the orphan; got {:?}",
            findings
        );
    }

    /// Step 7 (RED→GREEN via step 8) — `find_references` must disambiguate
    /// by declaration site (file + name), not just by bare name.
    ///
    /// One done task introduces two symbols both named "Builder" in different
    /// files: `crates/x/src/widget.rs` and `crates/y/src/widget.rs`. A single
    /// non-test caller is registered ONLY for the `(crates/y/..., "Builder")`
    /// pair. Expected: exactly one finding — for `crates/x/...` (no caller) —
    /// and the finding's evidence cites `crates/x/src/widget.rs`.
    ///
    /// RED: current bare-name keying in the mock (`HashMap<String, ...>`)
    /// means `set_find_references("Builder", refs)` covers BOTH symbols,
    /// suppressing both — the detector fires zero findings instead of one.
    #[test]
    fn find_references_disambiguates_by_declaration_site() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("5000");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![
                // Same name, different declaration files.
                changed_symbol("Builder", "crates/x/src/widget.rs"),
                changed_symbol("Builder", "crates/y/src/widget.rs"),
            ],
        );
        // Register a real non-test caller ONLY for crates/y/src/widget.rs's Builder.
        // crates/x/src/widget.rs's Builder has no callers — should be flagged.
        jc.set_find_references(
            "crates/y/src/widget.rs",
            "Builder",
            vec![SymbolReference {
                file: "crates/z/src/uses_builder.rs".to_string(),
                line: 5,
            }],
        );
        // crates/x/src/widget.rs's Builder: no callers registered → orphan.
        jc.set_find_references("crates/x/src/widget.rs", "Builder", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "5000".to_string(),
            done_meta("5000", done_at, Some("docs/x.md")),
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
            "only crates/x/src/widget.rs Builder should be flagged \
             (crates/y has a non-test caller); got {:?}",
            findings
        );
        assert!(
            findings[0].evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/x/src/widget.rs"
            )),
            "surviving finding must cite crates/x/src/widget.rs; got {:?}",
            findings[0].evidence
        );
    }

    /// Amendment (reviewer_comprehensive) — pins the subtle interaction
    /// between `target_task_id` scoping and `has_pending_consumer` suppression.
    ///
    /// The `target_task_id` guard skips non-target entries in the *producer*
    /// loop; `has_pending_consumer` independently scans the *full*
    /// `task_metadata` map. These are two different traversals: the first
    /// decides which producer tasks to audit; the second decides whether any
    /// consumer task (regardless of its own task_id) is already in-flight.
    ///
    /// A future refactor that moves the scoping filter earlier — e.g.
    /// pre-filtering `task_metadata` to `{ target }` before running check —
    /// would hide the consumer task from `has_pending_consumer` and silently
    /// break suppression. Neither `target_task_id_scopes_p1_to_one_task` (no
    /// consumer tasks) nor `review_status_consumer_suppresses_finding` (no
    /// target) exercises this cross-product; this test does.
    ///
    /// Setup: producer `"prod_1"` (done, 15 days past grace, prd="docs/p.md",
    /// zero refs) + `"review"`-status consumer `"cons_1"` (consumer_ref=
    /// "docs/p.md"). AuditContext has `target_task_id: Some("prod_1")`.
    /// Expected: zero findings — the scoped sweep still suppresses because
    /// `has_pending_consumer` scans the unfiltered metadata map.
    #[test]
    fn target_task_id_does_not_hide_consumer_from_suppression() {
        let done_at = NOW - 15 * DAY;
        let sha = task_sha("prod_1");

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        // Consumer guard fires before the commit-range query — fixture keyed
        // correctly (consumer suppresses before P1 would query jcodemunch).
        jc.set_changed_symbols(
            &format!("{sha}^1"),
            &sha,
            vec![changed_symbol("scoped_widget", "crates/reify-x/src/scoped.rs")],
        );
        jc.set_find_references("crates/reify-x/src/scoped.rs", "scoped_widget", vec![]);

        let mut task_metadata = HashMap::new();
        // The target producer — done, past grace, zero refs, orphan candidate.
        task_metadata.insert(
            "prod_1".to_string(),
            done_meta("prod_1", done_at, Some("docs/p.md")),
        );
        // A consumer whose task_id != target_task_id. The producer loop skips
        // it (it is not a candidate producer), but has_pending_consumer must
        // still discover it via ctx.task_metadata.values().
        task_metadata.insert(
            "cons_1".to_string(),
            TaskMetadata {
                task_id: "cons_1".to_string(),
                status: "review".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Consume the scoped widget".to_string(),
                prd: None,
                consumer_ref: Some("docs/p.md".to_string()),
                audit_foundation: None,
                done_at: None,
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: Some("prod_1".to_string()),
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };

        let findings = p1_producer_orphan::check(&ctx);
        assert!(
            findings.is_empty(),
            "scoped sweep (target_task_id=prod_1) must still see the out-of-scope \
             review consumer and suppress the orphan finding; got {:?}",
            findings
        );
    }

    /// Pins the new-detector API surface introduced by L-TRAIT: three new result
    /// structs (DeadSymbol / UntestedSymbol / LayerViolation) and three new
    /// Pattern variants (PDeadCode / PUntested / PLayerViolation). Renaming any
    /// struct field or Pattern variant will fail this test at compile time —
    /// exactly what downstream detector leaves (L-PDEAD / L-PUNTESTED / L-PLAYER)
    /// need from a stable API. No serde round-trip or docstring assertions —
    /// wire-shape decode is L-CLIENT's boundary test.
    #[test]
    fn new_detector_types_api_surface_pin() {
        let jc = MockJCodemunchOps::new();

        // DeadSymbol: destructure every field by name.
        let DeadSymbol {
            id: _,
            name: _,
            kind: _,
            file: _,
            line: _,
            confidence: _,
            signals: _,
        } = DeadSymbol {
            id: "sym_1".to_string(),
            name: "orphan_fn".to_string(),
            kind: "function".to_string(),
            file: "crates/x/src/lib.rs".to_string(),
            line: 10,
            confidence: 0.9,
            signals: vec!["no_callers".to_string()],
        };

        // UntestedSymbol: destructure every field by name.
        let UntestedSymbol {
            symbol_id: _,
            name: _,
            file: _,
            reached: _,
            confidence: _,
        } = UntestedSymbol {
            symbol_id: "sym_2".to_string(),
            name: "untested_fn".to_string(),
            file: "crates/y/src/lib.rs".to_string(),
            reached: false,
            confidence: 0.8,
        };

        // LayerViolation: destructure every field by name.
        let LayerViolation {
            from_file: _,
            to_file: _,
            rule: _,
        } = LayerViolation {
            from_file: "crates/gui/src/main.rs".to_string(),
            to_file: "crates/kernel/src/solver.rs".to_string(),
            rule: "gui-must-not-depend-on-kernel".to_string(),
        };

        // Pattern: reference each new variant.
        let _: Pattern = Pattern::PDeadCode;
        let _: Pattern = Pattern::PUntested;
        let _: Pattern = Pattern::PLayerViolation;

        // New mock methods: default-empty mock must return empty Vecs.
        let dead = jc.get_dead_code(0.5);
        assert!(
            dead.is_empty(),
            "default mock get_dead_code must return empty Vec; got {:?}",
            dead
        );

        let untested = jc.get_untested_symbols(0.5);
        assert!(
            untested.is_empty(),
            "default mock get_untested_symbols must return empty Vec; got {:?}",
            untested
        );

        let violations = jc.get_layer_violations();
        assert!(
            violations.is_empty(),
            "default mock get_layer_violations must return empty Vec; got {:?}",
            violations
        );
    }
}

} // mod p1
