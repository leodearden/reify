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

mod common;

mod ptodo {

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern,
    Severity,
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

        // Exactly three findings, all PTodo.
        assert_eq!(
            findings.len(),
            3,
            "expected exactly 3 PTODO findings; got {findings:?}"
        );
        for f in &findings {
            assert_eq!(f.pattern, Pattern::PTodo, "wrong pattern: {f:?}");
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

        // Per-kind severity: untracked→High; malformed-cite→Medium; phantom-tracking→Medium.
        assert_eq!(
            finding_for("untracked.rs").severity,
            Severity::High,
            "untracked must be High: {:?}",
            finding_for("untracked.rs")
        );
        assert_eq!(
            finding_for("malformed.rs").severity,
            Severity::Medium,
            "malformed-cite must stay Medium: {:?}",
            finding_for("malformed.rs")
        );
        assert_eq!(
            finding_for("phantom.rs").severity,
            Severity::Medium,
            "phantom-tracking must stay Medium: {:?}",
            finding_for("phantom.rs")
        );

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

    /// End-to-end `check` with the task DB PRESENT at the default path: a
    /// canonically-cited file (`#4444`, seeded `done`) and an untracked file
    /// coexist — the liveness lane emits an `orphaned` finding for the cited
    /// file (carrying `#4444` + `done`) while the structural lane emits the
    /// `untracked` finding for the other. (RED until step-10 wires the lane.)
    #[test]
    fn check_runs_liveness_lane_alongside_structural() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "cited.rs", "// TODO(#4444): x\n");
        write_file(root, "untracked.rs", "// TODO: wire\n");

        // Seed the DB at the DEFAULT resolved path (no env override needed).
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 4444, "done")],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["cited.rs".to_string(), "untracked.rs".to_string()]);

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

        let find_for = |path: &str| -> Option<&Finding> {
            findings.iter().find(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: p } if p == path))
            })
        };

        // The structural lane still flags the untracked marker.
        let untracked = find_for("untracked.rs")
            .unwrap_or_else(|| panic!("expected untracked finding; findings={findings:?}"));
        assert!(
            untracked.summary.starts_with("untracked:"),
            "summary: {}",
            untracked.summary
        );

        // The liveness lane flags the orphaned cite (terminal `done` status).
        let orphaned = find_for("cited.rs")
            .unwrap_or_else(|| panic!("expected orphaned finding; findings={findings:?}"));
        assert_eq!(orphaned.pattern, Pattern::PTodo);
        assert_eq!(orphaned.severity, Severity::High); // task η: orphaned → High
        assert!(
            orphaned.summary.starts_with("orphaned:"),
            "summary: {}",
            orphaned.summary
        );
        assert!(orphaned.summary.contains("#4444"), "summary: {}", orphaned.summary);
        assert!(orphaned.summary.contains("done"), "summary: {}", orphaned.summary);
    }

    /// End-to-end `check()` with a parked anchor in the DB: a file citing #42
    /// (deferred + do_not_complete:true) must yield a `parked-on-anchor` Medium
    /// finding; an untracked file must still yield the structural `untracked` High
    /// finding.
    #[test]
    fn check_parked_on_anchor_emits_finding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "parked.rs", "// TODO(#42): perf, see anchor\n");
        write_file(root, "untracked.rs", "// TODO: wire\n");

        // Seed the DB with the do_not_complete anchor at the default path.
        crate::common::schema::seed_tasks_db_at_with_metadata(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 42, "deferred", r#"{"do_not_complete":true}"#)],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["parked.rs".to_string(), "untracked.rs".to_string()]);

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

        let find_for = |path: &str| -> Option<&Finding> {
            findings.iter().find(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: p } if p == path))
            })
        };

        // The liveness lane emits a parked-on-anchor finding for the anchor cite.
        let parked = find_for("parked.rs")
            .unwrap_or_else(|| panic!("expected parked-on-anchor finding; findings={findings:?}"));
        assert_eq!(parked.pattern, Pattern::PTodo);
        assert_eq!(parked.severity, Severity::Medium, "parked-on-anchor must be Medium: {:?}", parked);
        assert!(
            parked.summary.starts_with("parked-on-anchor:"),
            "summary: {}",
            parked.summary
        );
        assert!(parked.summary.contains("#42"), "summary must carry id: {}", parked.summary);

        // The structural lane still flags the untracked marker.
        let untracked = find_for("untracked.rs")
            .unwrap_or_else(|| panic!("expected untracked finding; findings={findings:?}"));
        assert!(
            untracked.summary.starts_with("untracked:"),
            "summary: {}",
            untracked.summary
        );
        assert_eq!(untracked.severity, Severity::High);
    }

    /// §8.3 γ structural lane: a `#[ignore = "pending X"]` (blocker-prose, no
    /// cite) → `untracked:` PTodo/Medium finding; `#[ignore = "requires OCCT"]`
    /// (operational) → no finding.
    #[test]
    fn check_ignore_blocker_prose_emits_untracked_operational_passes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "ignore_blocker.rs", "#[ignore = \"pending X\"]\nfn f() {}\n");
        write_file(root, "ignore_op.rs", "#[ignore = \"requires OCCT\"]\nfn g() {}\n");

        let mut git = MockGitOps::new();
        git.set_ls_files(vec![
            "ignore_blocker.rs".to_string(),
            "ignore_op.rs".to_string(),
        ]);

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

        // Blocker-prose → exactly one untracked finding at ignore_blocker.rs.
        let blocker_finding = findings.iter().find(|f| {
            f.evidence.iter().any(|e| matches!(e, EvidenceRef::File { path: p } if p == "ignore_blocker.rs"))
        });
        let bf = blocker_finding.unwrap_or_else(|| {
            panic!("expected untracked finding for ignore_blocker.rs; findings={findings:?}")
        });
        assert_eq!(bf.pattern, Pattern::PTodo, "pattern: {bf:?}");
        assert_eq!(bf.severity, Severity::High, "severity: {bf:?}");
        assert!(
            bf.summary.starts_with("untracked:"),
            "summary must start with 'untracked:': {}",
            bf.summary
        );

        // Operational → no finding for ignore_op.rs.
        let op_finding = findings.iter().any(|f| {
            f.evidence.iter().any(|e| matches!(e, EvidenceRef::File { path: p } if p == "ignore_op.rs"))
        });
        assert!(
            !op_finding,
            "operational reason must yield no finding; findings={findings:?}"
        );
    }

    /// §8.3 γ cite-first: `#[ignore = "blocked on #4444"]` routes through β
    /// (cite wins over blocker-prose). With #4444 seeded `done` → `orphaned:`
    /// finding. With #4444 seeded `pending` → no finding (one live cite tracks).
    #[test]
    fn check_ignore_reason_with_cite_runs_liveness_lane() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "ignore_cite.rs", "#[ignore = \"blocked on #4444\"]\nfn f() {}\n");

        // Seed #4444 as 'done' (terminal) at the default path.
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 4444, "done")],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["ignore_cite.rs".to_string()]);

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

        // Terminal cite → orphaned finding.
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one orphaned finding; got {findings:?}"
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo);
        assert_eq!(f.severity, Severity::High); // task η: orphaned → High
        assert!(f.summary.starts_with("orphaned:"), "summary: {}", f.summary);
        assert!(f.summary.contains("#4444"), "summary must carry id: {}", f.summary);
        assert!(f.summary.contains("done"), "summary must carry status: {}", f.summary);

        // Now seed #4444 as 'pending' (live) and re-run → no finding.
        let dir2 = tempfile::tempdir().expect("tempdir");
        let root2 = dir2.path();
        write_file(root2, "ignore_cite.rs", "#[ignore = \"blocked on #4444\"]\nfn f() {}\n");
        crate::common::schema::seed_tasks_db_at(
            &root2.join(".taskmaster/tasks/tasks.db"),
            &[("master", 4444, "pending")],
        );
        let mut git2 = MockGitOps::new();
        git2.set_ls_files(vec!["ignore_cite.rs".to_string()]);
        let ctx2 = AuditContext {
            project_root: root2.to_path_buf(),
            conn: &conn,
            git: &git2,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };
        let findings2 = reify_audit::ptodo::check(&ctx2);
        assert!(
            findings2.is_empty(),
            "live cite (#4444 pending) must yield no finding; got {findings2:?}"
        );
    }

    /// §8.1 lane δ-A end-to-end through `check()` — the user-observable signal
    /// this task exists to deliver, shaped after
    /// `crates/reify-eval/src/engine_build.rs:12891`.
    ///
    /// A cited deferral rationale on an `#[allow(dead_code)]` attribute routes
    /// through the UNCHANGED β liveness lane: with the cite seeded `done` it is
    /// a High `orphaned:` finding. An uncited deferral rationale is High
    /// `untracked:` (SCOPE-1). A benign rationale — the dominant class among
    /// the 68 measured candidates — yields nothing.
    #[test]
    fn check_allow_dead_code_deferral_lane() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(
            root,
            "cited.rs",
            "#[allow(dead_code)] // production wiring pending task #4744 (volume-mesh)\nfn f() {}\n",
        );
        write_file(
            root,
            "uncited.rs",
            "#[allow(dead_code)] // wiring pending the morph rewrite\nfn g() {}\n",
        );
        write_file(
            root,
            "benign.rs",
            "#[allow(dead_code)] // used by some, but not all, test binaries\nfn h() {}\n",
        );

        // #4744 is `done` (terminal) → the cited line resolves to `orphaned`.
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 4744, "done")],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(vec![
            "cited.rs".to_string(),
            "uncited.rs".to_string(),
            "benign.rs".to_string(),
        ]);

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

        let for_path = |p: &str| -> Option<&Finding> {
            findings.iter().find(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: q } if q == p))
            })
        };

        let cited = for_path("cited.rs")
            .unwrap_or_else(|| panic!("expected orphaned finding for cited.rs; got {findings:?}"));
        assert_eq!(cited.pattern, Pattern::PTodo);
        assert_eq!(cited.severity, Severity::High);
        assert!(
            cited.summary.starts_with("orphaned:"),
            "summary must start with 'orphaned:': {}",
            cited.summary
        );
        assert!(
            cited.summary.contains("#4744"),
            "summary must carry the cite id: {}",
            cited.summary
        );

        let uncited = for_path("uncited.rs")
            .unwrap_or_else(|| panic!("expected untracked finding for uncited.rs; got {findings:?}"));
        assert_eq!(uncited.pattern, Pattern::PTodo);
        assert_eq!(uncited.severity, Severity::High);
        assert!(
            uncited.summary.starts_with("untracked:"),
            "summary must start with 'untracked:': {}",
            uncited.summary
        );

        assert!(
            for_path("benign.rs").is_none(),
            "a non-deferral allow-rationale must yield NO finding; got {findings:?}"
        );
        assert_eq!(
            findings.len(),
            2,
            "exactly two δ-A findings expected (cited + uncited); got {findings:?}"
        );
    }

    /// FP guard at the fixture level, per the esc-6087-1 ruling: the committed
    /// `scenario14_allow_dead_code_deferral.rs` fixture is entirely NEGATIVE —
    /// every line is an `#[allow(dead_code)]` whose rationale must NOT classify.
    ///
    /// The real class-(a) identifier sites (`mark_pending_with_cause`) are not
    /// δ-A candidates in-tree — they carry no allow-attribute — so guard 3
    /// cannot be pinned against live code at this level. This SYNTHETIC fixture
    /// supplies that pin. It lives under the already-allowlisted
    /// `crates/reify-audit/` prefix, so it is invisible to the live sweep and
    /// cannot pollute the §6.6 baseline; copied here into a temp root, its path
    /// is NOT allowlisted, so it is genuinely swept.
    ///
    /// `tests/cli.rs`'s whole-fixture-tree sweep gives this a second,
    /// independent enforcement point: because the fixture contributes zero
    /// findings, that test's exact `4 findings / exit 2` assertions stay true
    /// and go red if δ-A ever over-fires.
    #[test]
    fn committed_allow_dead_code_fixture_contributes_zero_findings() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ptodo/scenario14_allow_dead_code_deferral.rs");
        let content = std::fs::read_to_string(&fixture)
            .unwrap_or_else(|e| panic!("read committed fixture {}: {e}", fixture.display()));

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_file(root, "scenario14_allow_dead_code_deferral.rs", &content);

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["scenario14_allow_dead_code_deferral.rs".to_string()]);

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
        assert!(
            findings.is_empty(),
            "the δ-A negative fixture must contribute ZERO findings; got {findings:?}"
        );
    }

    /// §8.1 lane δ-B end-to-end through `check()` — the user-observable signal
    /// this task exists to deliver, shaped after
    /// `crates/reify-core/src/diagnostics.rs`'s `HexWedgeMeshOutcome` rustdoc
    /// ("blocked on VolumeMesh realization (task #2947)", cite `cancelled`).
    ///
    /// An ordinary comment that both defers work and names the task routes
    /// through the UNCHANGED β liveness lane: seeded terminal, it is a High
    /// `orphaned:` finding. Seeded non-terminal, it is silent. A benign
    /// explanatory comment — the dominant class in any real corpus — is silent
    /// regardless. δ-B emits no structural kind, so unlike δ-A there is no
    /// `untracked:` case to assert: an uncited deferral comment simply is not a
    /// candidate.
    #[test]
    fn check_delta_b_cited_deferral_lane() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(
            root,
            "cited.rs",
            "/// — that wiring is blocked on VolumeMesh realization (task #2947), mirroring\nfn f() {}\n",
        );
        write_file(
            root,
            "live.rs",
            "/// hydration is deferred to task #5235 landing first\nfn g() {}\n",
        );
        write_file(
            root,
            "benign.rs",
            "/// Maps each wedge outcome onto its diagnostic code (task #2947).\nfn h() {}\n",
        );

        // #2947 terminal (cancelled) → orphaned; #5235 non-terminal → silent.
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 2947, "cancelled"), ("master", 5235, "pending")],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(vec![
            "cited.rs".to_string(),
            "live.rs".to_string(),
            "benign.rs".to_string(),
        ]);

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

        let for_path = |p: &str| -> Option<&Finding> {
            findings.iter().find(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: q } if q == p))
            })
        };

        let cited = for_path("cited.rs")
            .unwrap_or_else(|| panic!("expected orphaned finding for cited.rs; got {findings:?}"));
        assert_eq!(cited.pattern, Pattern::PTodo);
        assert_eq!(cited.severity, Severity::High); // task η: orphaned → High
        assert!(
            cited.summary.starts_with("orphaned:"),
            "summary must start with 'orphaned:': {}",
            cited.summary
        );
        assert!(
            cited.summary.contains("#2947"),
            "summary must carry the cite id: {}",
            cited.summary
        );
        assert!(
            cited.summary.contains("cancelled"),
            "summary must carry the terminal status: {}",
            cited.summary
        );

        assert!(
            for_path("live.rs").is_none(),
            "a deferral citing a NON-terminal task must yield NO finding; got {findings:?}"
        );
        assert!(
            for_path("benign.rs").is_none(),
            "an explanatory comment with a cite but no deferral prose must yield NO finding; got {findings:?}"
        );
        assert_eq!(
            findings.len(),
            1,
            "exactly one δ-B finding expected (cited.rs only); got {findings:?}"
        );
    }

    /// FP guard at the fixture level for δ-B, mirroring
    /// `committed_allow_dead_code_fixture_contributes_zero_findings`: the
    /// committed `scenario20_delta_b_cited_deferral.rs` fixture is entirely
    /// NEGATIVE — every line must fail at least one of the lane's guards.
    ///
    /// This is the enforcement point that matters most for δ-B, because the
    /// lane was rejected once already (task #6087) at a 48% false-positive
    /// rate. The real class-(a) identifier sites and class-(b) PRD-relative
    /// sites ARE live in-tree, and are pinned verbatim at unit level in
    /// src/ptodo.rs; this fixture pins the same shapes through the whole
    /// `check()` pipeline, where an over-fire would actually reach the merge
    /// gate.
    ///
    /// It lives under the already-allowlisted `crates/reify-audit/` prefix, so
    /// the live sweep never sees it and it cannot pollute the §6.6 baseline;
    /// copied here into a temp root, its path is NOT allowlisted, so it IS
    /// genuinely swept.
    ///
    /// `tests/cli.rs`'s whole-fixture-tree sweep gives this a second,
    /// independent enforcement point: because the fixture contributes zero
    /// findings, that test's exact finding-count and exit-code assertions stay
    /// true and go red if δ-B ever over-fires.
    #[test]
    fn committed_delta_b_fixture_contributes_zero_findings() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ptodo/scenario20_delta_b_cited_deferral.rs");
        let content = std::fs::read_to_string(&fixture)
            .unwrap_or_else(|e| panic!("read committed fixture {}: {e}", fixture.display()));

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_file(root, "scenario20_delta_b_cited_deferral.rs", &content);

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["scenario20_delta_b_cited_deferral.rs".to_string()]);

        // Seed every id the fixture cites TERMINAL wherever a terminal status is
        // safe. `done`/`cancelled` is the worst case for this lane — it is what
        // turns a `Cited` entry into a High `orphaned` finding — so the
        // fixture's silence is proof the GUARDS held, not an artefact of a
        // missing or still-live task row.
        //
        // #7777 is the discriminating class-(d) cite: the G-allow lane exempts
        // it under its own rule (c) (`PRD ` immediately left), so seeding it
        // terminal isolates δ-B's `g_allow_marker_body` guard as the ONLY thing
        // preventing a finding on that line.
        //
        // #4092 / #3429 are the OWNER-LESS class-(d) seam: the G-allow lane
        // exempts both as provenance (rule (a) "(done)", rule (b) "re-homed
        // from cancelled"), so it has no owner to resolve; seeding them
        // terminal isolates δ-B's guard as the only thing keeping that line
        // silent.
        //
        // #5235 is the exception, and deliberately non-terminal. It is the
        // owner cite of the VERBATIM live G-allow shape, which rule (c) does NOT
        // exempt — so a terminal #5235 would make the independent G-allow lane
        // fire `g-allow-orphaned` on that line, breaking this fixture's
        // zero-findings contract for a reason that has nothing to do with δ-B.
        // Live today it is `pending`, and the fixture mirrors that.
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[
                ("master", 2326, "done"),
                ("master", 2330, "done"),
                ("master", 2335, "done"),
                ("master", 4739, "done"),
                ("master", 7777, "done"),
                ("master", 4092, "done"),
                ("master", 3429, "cancelled"),
                ("master", 5235, "pending"),
            ],
        );

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
        assert!(
            findings.is_empty(),
            "the δ-B negative fixture must contribute ZERO findings; got {findings:?}"
        );
    }

    /// ζ inverse lane: end-to-end `check()` integration. Seeds an on-disk DB
    /// with a non-terminal (pending) task whose metadata.files lists:
    ///   - a DELETED path (mock: absent from tracked, git has history)
    ///   - an EXISTING directory (mock: dir-prefix of a tracked file → FP guard)
    ///   - a TO-BE-CREATED path (mock: absent from tracked, git has no history)
    ///
    /// Asserts exactly one `task-cites-deleted-path` finding for the deleted
    /// path (carrying the task id, path, and commit sha), and NONE for the
    /// directory or to-be-created path. Also asserts that structural findings
    /// from a co-seeded untracked.rs still coexist.
    ///
    /// This is the RED counterpart: `check()` does NOT yet call `resolve_inverse`,
    /// so zero inverse findings are returned even though the DB is populated.
    /// The RED assertion (finding count == 1 for the deleted path) will FAIL.
    #[test]
    fn check_runs_inverse_lane_for_deleted_metadata_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Structural offender that must coexist with the inverse finding.
        write_file(root, "untracked.rs", "// TODO: wire this\n");

        // Seed the on-disk tasks DB with a pending task whose metadata.files
        // names a deleted path, an existing directory, and a new (never) path.
        crate::common::schema::seed_tasks_db_at_with_metadata(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 99, "pending", r#"{"files":["crates/deleted/mod.rs","crates/existing","crates/new_module.rs"]}"#)],
        );

        // tracked set: contains a file under "crates/existing" (dir prefix FP
        // guard), but NOT "crates/deleted/mod.rs" (deleted) or "crates/new_module.rs".
        let mut git = MockGitOps::new();
        git.set_ls_files(vec![
            "untracked.rs".to_string(),
            "crates/existing/src/lib.rs".to_string(),
        ]);

        // Git history: deleted path has a commit; new path has none (never existed).
        git.set_last_commit_for_path(
            "crates/deleted/mod.rs",
            reify_audit::GitCommit {
                sha: "deadbeef".to_string(),
                subject: "delete crates/deleted/mod.rs".to_string(),
            },
        );
        // "crates/new_module.rs" is NOT set in the mock → returns None → no finding.

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

        // Structural finding must still be present.
        assert!(
            findings.iter().any(|f| {
                f.evidence.iter().any(|e| matches!(e, EvidenceRef::File { path } if path == "untracked.rs"))
                    && f.summary.starts_with("untracked:")
            }),
            "structural untracked finding must be present; findings={findings:?}"
        );

        // Inverse: exactly one task-cites-deleted-path finding for the deleted path.
        let inverse_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.summary.starts_with("task-cites-deleted-path:"))
            .collect();
        assert_eq!(
            inverse_findings.len(),
            1,
            "expected exactly one task-cites-deleted-path finding; got {findings:?}"
        );
        let inv = inverse_findings[0];
        assert_eq!(inv.pattern, Pattern::PTodo, "pattern: {inv:?}");
        assert_eq!(inv.severity, Severity::Medium, "severity: {inv:?}");
        assert_eq!(inv.task_id, "99", "task_id must be the task id: {inv:?}");
        assert!(
            inv.summary.contains("crates/deleted/mod.rs"),
            "summary must contain the path: {}",
            inv.summary
        );
        assert!(
            inv.summary.contains("deadbeef"),
            "summary must contain the commit sha: {}",
            inv.summary
        );

        // FP guard: no finding for the directory "crates/existing".
        assert!(
            !findings.iter().any(|f| f.summary.contains("crates/existing")),
            "no finding must reference the existing directory; findings={findings:?}"
        );

        // To-be-created: no finding for "crates/new_module.rs".
        assert!(
            !findings.iter().any(|f| f.summary.contains("crates/new_module.rs")),
            "no finding for a never-existed path; findings={findings:?}"
        );
    }

    /// ζ degrade-together: with NO tasks.db at the default path, the inverse
    /// lane (like the liveness lane) must produce NO `task-cites-deleted-path`
    /// finding — both lanes degrade under the single existing DB-absent breadcrumb.
    #[test]
    fn check_inverse_degrades_when_tasks_db_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "untracked.rs", "// TODO: wire this\n");
        // NB: no tasks.db seeded → DB absent → both liveness + inverse degrade.

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["untracked.rs".to_string()]);
        // Even if git knows about a deleted path, the inverse lane must not fire
        // when the DB is absent (the task row can't be read, so no finding).
        git.set_last_commit_for_path(
            "crates/deleted/mod.rs",
            reify_audit::GitCommit {
                sha: "deadbeef".to_string(),
                subject: "delete crates/deleted/mod.rs".to_string(),
            },
        );

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

        assert!(
            !findings.iter().any(|f| f.summary.starts_with("task-cites-deleted-path:")),
            "no inverse finding may be emitted when tasks.db is absent; findings={findings:?}"
        );
    }

    /// §6.7 degradation (in-process): the SAME tree as the liveness test but
    /// with NO task DB at the default path. `check` must skip the liveness lane
    /// silently — only the structural `untracked` finding is returned, with no
    /// `orphaned`/`unknown-id` finding for the cited file, and no panic.
    #[test]
    fn check_degrades_when_tasks_db_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "cited.rs", "// TODO(#4444): x\n");
        write_file(root, "untracked.rs", "// TODO: wire\n");
        // NB: no `.taskmaster/tasks/tasks.db` is seeded → the default path is
        // absent, so the read-only open fails and the liveness lane degrades.

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["cited.rs".to_string(), "untracked.rs".to_string()]);

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

        // The structural lane still flags the untracked marker.
        assert!(
            findings.iter().any(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path } if path == "untracked.rs"))
                    && f.summary.starts_with("untracked:")
            }),
            "untracked structural finding must survive DB-absent degradation; findings={findings:?}"
        );

        // The liveness lane is skipped entirely: no orphaned/unknown-id/parked-on-anchor
        // finding, and in particular none referencing the cited file.
        for f in &findings {
            assert!(
                !f.summary.starts_with("orphaned:")
                    && !f.summary.starts_with("unknown-id:")
                    && !f.summary.starts_with("parked-on-anchor:"),
                "no liveness finding may be emitted when the DB is absent; got {:?}",
                f.summary
            );
        }
        assert!(
            !findings.iter().any(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path } if path == "cited.rs"))
            }),
            "cited file must yield no finding when the DB is absent; findings={findings:?}"
        );
    }

    /// Liveness-lane integration: the 6 SWEPT perf-marker files (real content
    /// copied into a tempdir) must produce ZERO orphaned #4590 findings after the
    /// retarget to the live anchor #4592. A synthetic `// TODO(#4590)` positive
    /// control asserts the fixture DB + orphaned classification are live
    /// (prevents a vacuously-green result if seeding fails or markers graduate).
    ///
    /// RED (step-1): real files still cite the terminal task #4590 → orphaned
    ///   findings fire → assertion (a) fails.
    /// GREEN (step-2): cites retargeted to in-progress #4592 → no orphaned
    ///   finding for the real files; control is still orphaned.
    #[test]
    fn perf_anchor_v3_perf_cites_resolve_live() {
        // Workspace root: CARGO_MANIFEST_DIR = <ws>/crates/reify-audit
        let ws_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap() // <ws>/crates
            .parent()
            .unwrap(); // <ws>

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // The 6 SWEPT perf-marker files (workspace-relative paths).
        // NOTE: two additional files were also retargeted #4590→#4592 as correctness
        // hygiene (crates/reify-audit/src/p1_producer_orphan.rs:53 and
        // crates/reify-audit/src/p2_consumer_stub.rs:408) but are intentionally
        // excluded from this liveness guard.  The PTODO detector allowlists the
        // entire crates/reify-audit/ subtree (§6.8), so it never emits findings for
        // those files regardless of which task id they cite.  Including them in
        // swept_paths would cause the "no orphaned" assertion to pass trivially
        // (detector skipped them, not that the cite is correct), providing false
        // confidence.  Regressions in the two audit-crate markers are caught instead
        // by a live /audit run.
        let swept_paths = [
            "crates/reify-eval/src/engine_purposes.rs",
            "crates/reify-eval/src/dispatcher.rs",
            "crates/reify-eval/src/engine_eval.rs",
            "crates/reify-kernel-fidget/src/kernel.rs",
            "crates/reify-expr/src/analysis.rs",
            "crates/reify-expr/src/calculus.rs",
        ];

        // Copy real file content into tempdir (skip gracefully if gone).
        let mut tracked_paths: Vec<String> = Vec::new();
        for rel_path in &swept_paths {
            let src = ws_root.join(rel_path);
            if src.exists() {
                let content = std::fs::read_to_string(&src)
                    .unwrap_or_else(|e| panic!("read {}: {}", src.display(), e));
                write_file(root, rel_path, &content);
                tracked_paths.push(rel_path.to_string());
            }
        }

        // Synthetic positive-control: always orphaned (4590 stays done).
        write_file(root, "zzz_ptodo_control.rs", "// TODO(#4590): orphan control\n");
        tracked_paths.push("zzz_ptodo_control.rs".to_string());

        // Guard: assert all 6 real swept files are present + the 1 control file.
        // Renames/moves cause src.exists() to fail → fewer real files → caught here.
        // Graduations (file deleted when improvement lands) require updating this count
        // and swept_paths to reflect the remaining markers.
        assert!(
            tracked_paths.len() >= 7,
            "expected 6 real swept files + 1 control in tracked_paths, got {}; \
             if a swept file was renamed/moved update swept_paths; \
             if all 6 have graduated, delete this test",
            tracked_paths.len()
        );

        // Seed fixture DB: 4590=done (terminal), 4592=in-progress (live).
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 4590, "done"), ("master", 4592, "in-progress")],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(tracked_paths);

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

        // (a) No real swept perf file may produce an orphaned #4590 finding.
        //     RED: real files cite #4590 (done) → orphaned fires → assertion fails.
        //     GREEN: after retarget to #4592 (in-progress) → no orphaned finding.
        //     Match "#4590 " (with trailing space) not bare "#4590" to avoid false
        //     positives from longer ids such as #45900 (summary format: "… #NNN status=…").
        for path in &swept_paths {
            let has_orphaned_4590 = findings.iter().any(|f| {
                f.summary.starts_with("orphaned:")
                    && f.summary.contains("#4590 ")
                    && f.evidence
                        .iter()
                        .any(|e| matches!(e, EvidenceRef::File { path: p } if p == *path))
            });
            assert!(
                !has_orphaned_4590,
                "real perf file {path} must NOT produce an orphaned #4590 finding \
                 after retarget to #4592; findings={findings:?}"
            );
        }

        // (b) The synthetic control file MUST produce an orphaned #4590 finding
        //     (proves fixture DB is live and orphaned classification fires).
        //     Match "#4590 " (trailing space) to pin the exact id token, not a prefix.
        let control_orphaned = findings.iter().any(|f| {
            f.summary.starts_with("orphaned:")
                && f.summary.contains("#4590 ")
                && f.summary.contains("done")
                && f.evidence.iter().any(|e| {
                    matches!(e, EvidenceRef::File { path: p } if p == "zzz_ptodo_control.rs")
                })
        });
        assert!(
            control_orphaned,
            "zzz_ptodo_control.rs must produce an orphaned #4590 finding (test has teeth); \
             findings={findings:?}"
        );
    }

    /// A bare `#[ignore]` (no reason string) emits a `bare-ignore:` finding
    /// with Severity::High (task η, #4559).
    #[test]
    fn bare_ignore_is_high() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "bare.rs", "#[ignore]\nfn t() {}\n");

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["bare.rs".to_string()]);

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

        assert_eq!(
            findings.len(),
            1,
            "expected exactly one bare-ignore finding; got {findings:?}"
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo, "pattern: {f:?}");
        assert_eq!(f.severity, Severity::High, "bare-ignore must be High: {f:?}");
        assert!(
            f.summary.starts_with("bare-ignore:"),
            "summary must start with bare-ignore: {}",
            f.summary
        );
    }

    // ---------------------------------------------------------------
    // §6.6 scan evidence — `check_with_stats` / `ScanStats` (task #6241)
    //
    // These pin the RUN evidence the §6.6 vacuity floor keys on. The
    // rationale lives in docs/prds/reify-audit-ptodo-detector.md §6.6;
    // it is deliberately not restated here.
    // ---------------------------------------------------------------

    /// Build the standard tempdir-backed `AuditContext` fixture pieces used by
    /// the scan-stats tests. Returns owned `(git, conn, jc)`; the caller
    /// assembles the `AuditContext` so the borrows stay local to the test.
    fn mock_ops(paths: &[&str]) -> (MockGitOps, Connection, MockJCodemunchOps) {
        let mut git = MockGitOps::new();
        git.set_ls_files(paths.iter().map(|s| s.to_string()).collect());
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        (git, conn, MockJCodemunchOps::new())
    }

    /// Stage the mixed fixture shared by the counting-contract and delegation
    /// tests: two marker-bearing swept files, one allowlisted-prefix path, one
    /// non-swept path, one marker-free swept file, and one tracked path that is
    /// deliberately NOT written to disk (unreadable → skipped fail-safe).
    ///
    /// Ground truth for the assertions below:
    ///   swept & non-allowlisted & readable = { alpha.rs, beta.rs, clean.rs } → 3
    ///   scan_file-classified marker lines   = 2 (alpha) + 1 (beta) + 0 (clean) → 3
    fn write_mixed_fixture(root: &Path) -> Vec<&'static str> {
        // alpha.rs: two classified lines — one Structural(Untracked), one Cited.
        write_file(root, "alpha.rs", "// TODO: wire this up\n// TODO(#4553): already cited\n");
        // beta.rs: one classified line, plus a plain line that classifies to nothing.
        write_file(root, "beta.rs", "fn beta() {}\n// FIXME: broken here\n");
        // Allowlisted prefix (§6.8) → never read, never counted.
        write_file(root, "crates/reify-audit/allowlisted.rs", "// TODO: allowlisted self\n");
        // Non-swept extension → never read, never counted.
        write_file(root, "notes.md", "// TODO: in a non-swept doc\n");
        // Swept and read, but carries zero markers → counted as scanned, 0 markers.
        write_file(root, "clean.rs", "fn clean() -> u32 { 7 }\n");
        // NOTE: "missing.rs" is intentionally absent from disk.
        vec![
            "alpha.rs",
            "beta.rs",
            "crates/reify-audit/allowlisted.rs",
            "notes.md",
            "clean.rs",
            "missing.rs",
        ]
    }

    /// (i) COUNTING CONTRACT — `files_scanned` counts exactly the tracked paths
    /// that survived `is_swept_ext && !is_allowlisted` AND were read
    /// successfully (so it excludes the allowlisted path, the non-swept path
    /// and the missing one, and INCLUDES the marker-free file);
    /// `markers_examined` counts every `scan_file`-classified line (Structural
    /// and Cited alike) across exactly those files.
    #[test]
    fn check_with_stats_counts_scanned_files_and_examined_markers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let paths = write_mixed_fixture(root);

        let (git, conn, jc) = mock_ops(&paths);
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

        let (_findings, stats) = reify_audit::ptodo::check_with_stats(&ctx);

        assert_eq!(
            stats.files_scanned, 3,
            "files_scanned must count exactly the swept, non-allowlisted, readable \
             paths (alpha.rs, beta.rs, clean.rs) — excluding the allowlisted prefix, \
             the non-swept .md and the tracked-but-missing path; got {stats:?}"
        );
        assert_eq!(
            stats.markers_examined, 3,
            "markers_examined must count every scan_file-classified line \
             (2 in alpha.rs + 1 in beta.rs + 0 in clean.rs); got {stats:?}"
        );
    }

    /// (ii) DECOUPLING PROPERTY — the load-bearing one for task #6241: a tree
    /// of swept files carrying NO markers yields zero findings yet still proves
    /// the detector RAN (`files_scanned >= 1`, `markers_examined == 0`). This is
    /// the "detector ran, tree is clean" partition that a floor on the live
    /// finding count cannot distinguish from "detector did not run".
    #[test]
    fn check_with_stats_proves_the_run_on_a_clean_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "clean_a.rs", "fn a() -> u32 { 1 }\n");
        write_file(root, "clean_b.rs", "pub fn b() {}\n");

        let (git, conn, jc) = mock_ops(&["clean_a.rs", "clean_b.rs"]);
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

        let (findings, stats) = reify_audit::ptodo::check_with_stats(&ctx);

        assert!(
            findings.is_empty(),
            "a marker-free tree must yield zero findings; got {findings:?}"
        );
        assert_eq!(
            stats.files_scanned, 2,
            "both marker-free swept files must still count as scanned: {stats:?}"
        );
        assert!(
            stats.files_scanned >= 1,
            "scan evidence must be non-vacuous on a clean tree: {stats:?}"
        );
        assert_eq!(
            stats.markers_examined, 0,
            "a marker-free tree examines no markers: {stats:?}"
        );
    }

    /// (iii) DELEGATION — `check` stays a pure projection of `check_with_stats`,
    /// so the four existing call sites see byte-identical behaviour.
    ///
    /// Today `check` is literally `check_with_stats(ctx).0`, so this can only
    /// go RED if someone reimplements it as a SEPARATE sweep — which is exactly
    /// the drift it guards, and the only shape that could diverge silently.  It
    /// therefore compares whole `Finding`s (`Finding: PartialEq`), not just
    /// their summaries: a second sweep could agree on every summary while
    /// diverging on `severity`, `task_id`, `pattern` or `evidence`, and a
    /// summaries-only assert would wave that through.
    #[test]
    fn check_is_the_findings_projection_of_check_with_stats() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let paths = write_mixed_fixture(root);

        let (git, conn, jc) = mock_ops(&paths);
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

        let (with_stats, _stats) = reify_audit::ptodo::check_with_stats(&ctx);
        let plain = reify_audit::ptodo::check(&ctx);

        // Whole-value equality: same order, same count, and every field of
        // every Finding — not just the summaries.
        assert_eq!(
            with_stats, plain,
            "check() must be the exact findings projection of check_with_stats()"
        );
        // Guard against a vacuous comparison: the fixture does produce findings.
        assert!(
            !plain.is_empty(),
            "fixture must yield at least one finding or this assertion is vacuous"
        );
    }
}

}

// -----------------------------------------------------------------------
// §8.2/§8.3 liveness lane — resolve_liveness against a seeded tasks table
// -----------------------------------------------------------------------
//
// These tests drive `ptodo::resolve_liveness` directly against an in-memory
// `tasks` table (common::schema::seed_tasks_db), with NO filesystem and NO
// real task DB. They pin the §8.2 multi-cite rule and the §8.3 orphaned /
// unknown-id taxonomy (both Medium).

// -----------------------------------------------------------------------
// ζ inverse lane — resolve_inverse against a seeded tasks table + MockGitOps
// -----------------------------------------------------------------------
//
// These tests drive `ptodo::resolve_inverse` directly against an in-memory
// `tasks` table (common::schema::seed_tasks_db + insert_task_with_metadata),
// plus a `MockGitOps` with `set_last_commit_for_path` canned answers. No
// filesystem, no real task DB, no on-disk repo. Pins PRD §9 scenarios 11/12
// and the directory-prefix FP guard.

mod inverse {
    use crate::common::schema::{insert_task, insert_task_with_metadata, seed_tasks_db};
    use reify_audit::{EvidenceRef, GitCommit, MockGitOps, Pattern, Severity};
    use std::collections::HashSet;

    fn tracked(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    fn mock_commit(sha: &str, subject: &str) -> GitCommit {
        GitCommit { sha: sha.to_string(), subject: subject.to_string() }
    }

    // Scenario 11 (PRD §9): non-terminal task whose metadata.files names a
    // deleted path → exactly one task-cites-deleted-path finding.
    #[test]
    fn deleted_path_emits_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            42,
            "pending",
            r#"{"files":["crates/deleted.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        git.set_last_commit_for_path(
            "crates/deleted.rs",
            mock_commit("abc123", "delete crates/deleted.rs"),
        );

        let tracked = tracked(&["crates/alive.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert_eq!(
            findings.len(),
            1,
            "expected exactly one task-cites-deleted-path finding; got {findings:?}"
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo, "pattern: {f:?}");
        assert_eq!(f.severity, Severity::Medium, "severity: {f:?}");
        assert_eq!(f.task_id, "42", "task_id must equal the task id: {f:?}");
        assert!(
            f.summary.starts_with("task-cites-deleted-path:"),
            "summary must start with kind prefix: {}",
            f.summary
        );
        assert!(
            f.summary.contains("crates/deleted.rs"),
            "summary must contain the path: {}",
            f.summary
        );
        assert!(
            f.summary.contains("abc123"),
            "summary must contain the commit sha: {}",
            f.summary
        );
        // Evidence: MetadataFiles + Commit
        let has_metadata_files = f.evidence.iter().any(|e| {
            matches!(e, EvidenceRef::MetadataFiles { entries } if entries.contains(&"crates/deleted.rs".to_string()))
        });
        assert!(has_metadata_files, "evidence must contain MetadataFiles ref: {:?}", f.evidence);
        let has_commit = f.evidence.iter().any(|e| {
            matches!(e, EvidenceRef::Commit { sha, .. } if sha == "abc123")
        });
        assert!(has_commit, "evidence must contain Commit ref with sha: {:?}", f.evidence);
    }

    // Scenario 20 (PRD §9): non-terminal task whose metadata.files names a path
    // that was RENAMED (not deleted), to a target still tracked at HEAD → one
    // task-cites-renamed-path finding naming BOTH paths.
    //
    // The summary assertions are the point of the whole lane refinement: a
    // consumer must be able to read the finding and repoint metadata.files
    // without re-running any git archaeology.
    #[test]
    fn renamed_path_emits_renamed_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            50,
            "pending",
            r#"{"files":["crates/old.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        git.set_last_commit_for_path(
            "crates/old.rs",
            mock_commit("abc123", "absorb old.rs into new/"),
        );
        git.set_rename_target_for_path("crates/old.rs", "abc123", "crates/new/old.rs");

        // The NEW path is tracked at HEAD; the cited (old) one is not.
        let tracked = tracked(&["crates/new/old.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert_eq!(
            findings.len(),
            1,
            "expected exactly one task-cites-renamed-path finding; got {findings:?}"
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo, "pattern: {f:?}");
        // Medium keeps the inverse lane exit-neutral (PRD §8.4).
        assert_eq!(f.severity, Severity::Medium, "severity: {f:?}");
        assert_eq!(f.task_id, "50", "task_id must equal the task id: {f:?}");
        assert!(
            f.summary.starts_with("task-cites-renamed-path:"),
            "summary must start with the renamed kind prefix: {}",
            f.summary
        );
        assert!(
            f.summary.contains("crates/old.rs"),
            "summary must name the cited (old) path: {}",
            f.summary
        );
        assert!(
            f.summary.contains("crates/new/old.rs"),
            "summary must name the rename target: {}",
            f.summary
        );
        assert!(
            f.summary.contains("abc123"),
            "summary must carry the commit sha: {}",
            f.summary
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.summary.starts_with("task-cites-deleted-path:")),
            "a renamed path must not also be reported as deleted; got {findings:?}"
        );

        // Evidence: MetadataFiles carries ONLY the cited path (it is the sort
        // key, and the rename target is by definition not a metadata.files
        // entry); the target rides in a File ref; the commit in a Commit ref.
        let metadata_entries = f
            .evidence
            .iter()
            .find_map(|e| match e {
                EvidenceRef::MetadataFiles { entries } => Some(entries.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("evidence must contain MetadataFiles ref: {:?}", f.evidence));
        assert_eq!(
            metadata_entries,
            vec!["crates/old.rs".to_string()],
            "MetadataFiles must carry exactly the CITED path: {:?}",
            f.evidence
        );
        let has_file = f.evidence.iter().any(|e| {
            matches!(e, EvidenceRef::File { path } if path == "crates/new/old.rs")
        });
        assert!(
            has_file,
            "evidence must contain a File ref at the rename target: {:?}",
            f.evidence
        );
        let has_commit = f.evidence.iter().any(|e| {
            matches!(e, EvidenceRef::Commit { sha, .. } if sha == "abc123")
        });
        assert!(
            has_commit,
            "evidence must contain a Commit ref with the rename sha: {:?}",
            f.evidence
        );
    }

    // Scenario 18 (PRD §9): the rename target is itself absent from the tracked
    // set (renamed again, or subsequently deleted) → stays task-cites-deleted-path.
    //
    // The renamed kind's whole promise is "the summary names a path you can go
    // look at". Advertising a target that is itself gone would trade one
    // misleading finding for another, so the reclassification is gated on the
    // target being present at HEAD.
    #[test]
    fn renamed_target_absent_from_tracked_stays_deleted() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            51,
            "pending",
            r#"{"files":["crates/old.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        git.set_last_commit_for_path(
            "crates/old.rs",
            mock_commit("def456", "move old.rs again"),
        );
        git.set_rename_target_for_path("crates/old.rs", "def456", "crates/moved_again.rs");

        // NEITHER path is tracked at HEAD.
        let tracked = tracked(&["crates/unrelated.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {findings:?}"
        );
        assert!(
            findings[0].summary.starts_with("task-cites-deleted-path:"),
            "a rename target that is itself untracked must stay the deleted kind: {}",
            findings[0].summary
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.summary.starts_with("task-cites-renamed-path:")),
            "must not advertise a rename target that is itself gone; got {findings:?}"
        );
    }

    // Scenario 19 (PRD §9): regression pin beside scenario 11 — a genuine
    // deletion (no rename target resolvable) is NOT silently reclassified by
    // the class fix.
    #[test]
    fn genuine_delete_still_emits_deleted_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            52,
            "pending",
            r#"{"files":["crates/gone.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        git.set_last_commit_for_path("crates/gone.rs", mock_commit("beef01", "rm crates/gone.rs"));
        // No rename target canned → rename_target_for_path returns None, which
        // is also what the real seam returns for a delete commit, a merge
        // commit, and any git error.

        let tracked = tracked(&["crates/alive.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding; got {findings:?}"
        );
        let f = &findings[0];
        assert!(
            f.summary.starts_with("task-cites-deleted-path:"),
            "a genuine delete must stay the deleted kind: {}",
            f.summary
        );
        assert_eq!(f.severity, Severity::Medium, "severity: {f:?}");
        assert_eq!(f.task_id, "52", "task_id must equal the task id: {f:?}");
        let has_metadata_files = f.evidence.iter().any(|e| {
            matches!(e, EvidenceRef::MetadataFiles { entries } if entries == &vec!["crates/gone.rs".to_string()])
        });
        assert!(has_metadata_files, "evidence must contain MetadataFiles ref: {:?}", f.evidence);
        let has_commit = f
            .evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::Commit { sha, .. } if sha == "beef01"));
        assert!(has_commit, "evidence must contain Commit ref: {:?}", f.evidence);
        assert!(
            !f.evidence
                .iter()
                .any(|e| matches!(e, EvidenceRef::File { .. })),
            "a deleted-path finding must carry NO File ref (there is no target): {:?}",
            f.evidence
        );
    }

    // Several tasks citing the SAME renamed path all report — the measured
    // real-world shape (one renamed harness file cited by 4 tasks). Pins that
    // the per-run rename memo neither drops the second finding nor perturbs the
    // deterministic (task_id, path) order.
    #[test]
    fn two_tasks_citing_same_renamed_path_both_report() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            53,
            "pending",
            r#"{"files":["crates/old.rs"]}"#,
        );
        insert_task_with_metadata(
            &conn,
            "master",
            54,
            "in-progress",
            r#"{"files":["crates/old.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        git.set_last_commit_for_path("crates/old.rs", mock_commit("abc123", "absorb old.rs"));
        git.set_rename_target_for_path("crates/old.rs", "abc123", "crates/new/old.rs");

        let tracked = tracked(&["crates/new/old.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert_eq!(
            findings.len(),
            2,
            "both citing tasks must report; got {findings:?}"
        );
        assert_eq!(
            findings.iter().map(|f| f.task_id.as_str()).collect::<Vec<_>>(),
            vec!["53", "54"],
            "findings must stay ordered by task id; got {findings:?}"
        );
        for f in &findings {
            assert!(
                f.summary.starts_with("task-cites-renamed-path:"),
                "both findings must be the renamed kind: {}",
                f.summary
            );
            assert!(
                f.summary.contains("crates/new/old.rs"),
                "both findings must name the rename target: {}",
                f.summary
            );
        }
    }

    // Scenario 12 (PRD §9): path absent from tracked set but never committed
    // (mock returns None) → no finding (presumed to-be-created).
    #[test]
    fn never_existed_path_no_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            43,
            "in-progress",
            r#"{"files":["crates/new.rs"]}"#,
        );

        // MockGitOps with no entry for "crates/new.rs" → returns None
        let git = MockGitOps::new();
        let tracked = tracked(&["crates/existing.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert!(
            findings.is_empty(),
            "never-existed path must not produce a finding; got {findings:?}"
        );
    }

    // Path present as an exact match in the tracked set → no finding.
    #[test]
    fn exact_tracked_file_no_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            44,
            "deferred",
            r#"{"files":["crates/alive.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        // Even if git has history, the path is present → should not flag.
        git.set_last_commit_for_path("crates/alive.rs", mock_commit("dead", "rm"));

        let tracked = tracked(&["crates/alive.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert!(
            findings.is_empty(),
            "a path present in tracked set must not produce a finding; got {findings:?}"
        );
    }

    // FP guard: path present as a DIRECTORY PREFIX of a tracked file → no finding.
    // Mirrors the live case where task cites "crates/reify-audit/tests" (a dir)
    // while tracked contains "crates/reify-audit/tests/foo.rs".
    #[test]
    fn dir_prefix_of_tracked_file_no_finding() {
        let conn = seed_tasks_db();
        // Without trailing slash
        insert_task_with_metadata(
            &conn,
            "master",
            45,
            "pending",
            r#"{"files":["crates/x/tests"]}"#,
        );
        // Also insert a row citing a trailing-slash form
        insert_task_with_metadata(
            &conn,
            "master",
            46,
            "pending",
            r#"{"files":["crates/x/tests/"]}"#,
        );

        let mut git = MockGitOps::new();
        // Even with history mocked, dir prefixes must not flag.
        git.set_last_commit_for_path("crates/x/tests", mock_commit("sha1", "rm dir"));
        git.set_last_commit_for_path("crates/x/tests/", mock_commit("sha2", "rm dir slash"));

        // tracked contains a file UNDER the cited directory.
        let tracked = tracked(&["crates/x/tests/a.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert!(
            findings.is_empty(),
            "dir-prefix of tracked file must not produce a finding; got {findings:?}"
        );
    }

    // Terminal task (done or cancelled) citing a deleted path → no finding.
    #[test]
    fn terminal_task_no_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            47,
            "done",
            r#"{"files":["crates/old.rs"]}"#,
        );
        insert_task_with_metadata(
            &conn,
            "master",
            48,
            "cancelled",
            r#"{"files":["crates/old2.rs"]}"#,
        );

        let mut git = MockGitOps::new();
        git.set_last_commit_for_path("crates/old.rs", mock_commit("sha3", "rm old"));
        git.set_last_commit_for_path("crates/old2.rs", mock_commit("sha4", "rm old2"));

        let tracked = tracked(&["crates/something_else.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert!(
            findings.is_empty(),
            "terminal tasks (done/cancelled) must not produce inverse findings; got {findings:?}"
        );
    }

    // NULL metadata (no metadata column set) → gracefully produces no findings.
    #[test]
    fn null_metadata_no_finding() {
        let conn = seed_tasks_db();
        // insert_task inserts (tag, id, status) with metadata defaulting to NULL
        insert_task(&conn, "master", 49, "pending");

        let git = MockGitOps::new();
        let tracked = tracked(&["crates/anything.rs"]);

        let findings =
            reify_audit::ptodo::resolve_inverse(&conn, &git, &tracked).expect("resolve_inverse");

        assert!(
            findings.is_empty(),
            "NULL metadata must produce no findings; got {findings:?}"
        );
    }
}

mod liveness {
    use crate::common::schema::{insert_task, insert_task_with_metadata, seed_tasks_db};
    use reify_audit::{EvidenceRef, Pattern, Severity};

    /// Build a single cited-marker tuple `(path, line, ids, text)` — the shape
    /// `check` collects from `scan_file`'s `Cited` entries.
    fn marker(path: &str, line: usize, ids: &[u32], text: &str) -> (String, usize, Vec<u32>, String) {
        (path.to_string(), line, ids.to_vec(), text.to_string())
    }

    #[test]
    fn orphaned_done_carries_id_and_status() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4444, "done");

        let cited = vec![marker("a.rs", 2, &[4444], "// TODO(#4444): x")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "one orphaned finding; got {findings:?}");
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo);
        assert_eq!(f.severity, Severity::High); // task η: orphaned → High
        assert!(f.summary.starts_with("orphaned:"), "summary: {}", f.summary);
        assert!(f.summary.contains("#4444"), "summary must carry id: {}", f.summary);
        assert!(f.summary.contains("done"), "summary must carry status: {}", f.summary);
        assert_eq!(f.task_id, "a.rs");
        assert!(
            matches!(&f.evidence[..], [EvidenceRef::File { path }] if path == "a.rs"),
            "evidence must be a single File ref at the path: {:?}",
            f.evidence
        );
    }

    #[test]
    fn orphaned_cancelled_is_terminal() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 10, "cancelled");

        let cited = vec![marker("b.rs", 1, &[10], "// TODO(#10): y")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert_eq!(findings[0].severity, Severity::High); // task η: orphaned → High
        assert!(findings[0].summary.starts_with("orphaned:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("cancelled"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#10"), "{}", findings[0].summary);
    }

    #[test]
    fn live_statuses_emit_no_finding() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 1, "pending");
        insert_task(&conn, "master", 2, "in-progress");

        let cited = vec![
            marker("p.rs", 1, &[1], "// TODO(#1): a"),
            marker("p.rs", 2, &[2], "// TODO(#2): b"),
        ];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(findings.is_empty(), "live cites must not orphan; got {findings:?}");
    }

    #[test]
    fn absent_id_is_unknown_id() {
        let conn = seed_tasks_db();
        // id 999 is never seeded.
        let cited = vec![marker("c.rs", 5, &[999], "// TODO(#999): z")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert_eq!(findings[0].severity, Severity::Medium); // task η: unknown-id stays Medium (DB-sync race must not hard-fail)
        assert!(findings[0].summary.starts_with("unknown-id:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#999"), "{}", findings[0].summary);
    }

    #[test]
    fn one_live_cite_suffices_for_multi_cite_marker() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4444, "done");
        insert_task(&conn, "master", 5555, "pending");

        // A single marker citing one terminal (done) AND one live (pending) id
        // → tracked → NO finding (§8.2 "one live cite suffices").
        let cited = vec![marker("m.rs", 3, &[4444, 5555], "// TODO(#4444, #5555): x")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(findings.is_empty(), "one live cite suffices; got {findings:?}");
    }

    /// §6.7 master-only resolution (normative — PRD line 181 "rows filtered to
    /// `tag='master'`"): a cite whose id exists ONLY under a non-master tag is
    /// invisible to the liveness query and classifies as `unknown-id`, exactly as
    /// a wholly-absent id would — NOT tracked, NOT orphaned. Pins the documented
    /// master-only assumption so introducing a multi-tag task DB is a conscious,
    /// test-breaking decision rather than a silent classification change.
    #[test]
    fn non_master_tag_resolves_as_unknown_id() {
        let conn = seed_tasks_db();
        // id 7 exists and is live, but only under a non-master tag → the
        // `tag='master'` query never sees it.
        insert_task(&conn, "feature-x", 7, "pending");

        let cited = vec![marker("t.rs", 1, &[7], "// TODO(#7): x")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert!(
            findings[0].summary.starts_with("unknown-id:"),
            "non-master-tag id must classify as unknown-id: {}",
            findings[0].summary
        );
        assert!(findings[0].summary.contains("#7"), "{}", findings[0].summary);
    }

    // -------------------------------------------------------------------
    // Scenario 14: parked-on-anchor positive case
    // -------------------------------------------------------------------

    /// A cite resolving to a non-terminal task with `do_not_complete:true`
    /// must emit exactly one Medium `parked-on-anchor` finding.
    #[test]
    fn parked_on_anchor_cite_emits_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(&conn, "master", 42, "deferred", r#"{"do_not_complete":true}"#);

        let cited = vec![marker("perf.rs", 5, &[42], "// TODO(#42): perf, see anchor")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "expected one parked-on-anchor finding; got {findings:?}");
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo);
        assert_eq!(f.severity, Severity::Medium, "parked-on-anchor must be Medium: {:?}", f);
        assert!(
            f.summary.starts_with("parked-on-anchor:"),
            "summary must start with 'parked-on-anchor:': {}",
            f.summary
        );
        assert!(f.summary.contains("#42"), "summary must carry id: {}", f.summary);
        assert!(f.summary.contains("deferred"), "summary must carry status: {}", f.summary);
        assert!(f.summary.contains("do_not_complete"), "summary must cite the flag: {}", f.summary);
        assert_eq!(f.task_id, "perf.rs");
        assert!(
            matches!(&f.evidence[..], [EvidenceRef::File { path }] if path == "perf.rs"),
            "evidence must be a single File ref: {:?}",
            f.evidence
        );
    }

    // -------------------------------------------------------------------
    // Scenario 15a: deferred without do_not_complete — no finding (FP guard)
    // -------------------------------------------------------------------

    /// A deferred task with NULL metadata must NOT produce a parked-on-anchor
    /// finding — genuine paused/human-owned deferred tasks must stay live.
    #[test]
    fn deferred_without_do_not_complete_no_finding() {
        let conn = seed_tasks_db();
        // insert_task inserts with NULL metadata — deferred without the flag.
        insert_task(&conn, "master", 42, "deferred");

        let cited = vec![marker("p.rs", 1, &[42], "// TODO(#42): something")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(
            findings.is_empty(),
            "deferred task without do_not_complete must not produce a finding; got {findings:?}"
        );
    }

    // -------------------------------------------------------------------
    // Scenario 15b: do_not_dispatch-only — no finding (FP guard)
    // -------------------------------------------------------------------

    /// A task with `do_not_dispatch:true` but WITHOUT `do_not_complete` must
    /// NOT produce a parked-on-anchor finding.
    #[test]
    fn deferred_do_not_dispatch_only_no_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(
            &conn,
            "master",
            42,
            "deferred",
            r#"{"do_not_dispatch":true}"#,
        );

        let cited = vec![marker("q.rs", 2, &[42], "// TODO(#42): human-owned")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(
            findings.is_empty(),
            "do_not_dispatch-only task must not produce a parked-on-anchor finding; got {findings:?}"
        );
    }

    // -------------------------------------------------------------------
    // Scenario 16: §8.2 preservation — one genuinely-live cite suppresses finding
    // -------------------------------------------------------------------

    /// A marker citing both a parked anchor (#42, do_not_complete) AND a
    /// genuinely-live task (#43, pending) must emit NO finding — §8.2 "one live
    /// cite suffices".
    #[test]
    fn parked_anchor_with_one_live_cite_no_finding() {
        let conn = seed_tasks_db();
        insert_task_with_metadata(&conn, "master", 42, "deferred", r#"{"do_not_complete":true}"#);
        insert_task(&conn, "master", 43, "pending");

        let cited = vec![marker("m.rs", 3, &[42, 43], "// TODO(#42, #43): perf anchor + live")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(
            findings.is_empty(),
            "one genuinely-live cite must suppress the parked-on-anchor finding (§8.2); got {findings:?}"
        );
    }
}

// -----------------------------------------------------------------------
// G-allow owner-cite liveness resolver — resolve_g_allow_owner_liveness
// -----------------------------------------------------------------------
//
// These tests drive `ptodo::resolve_g_allow_owner_liveness` directly against
// an in-memory seeded `tasks` table. They pin:
// - g-allow-orphaned (High) for terminal owner cites
// - no finding for genuinely-live owner cites
// - g-allow-unknown-id (Medium) for absent ids
// - EVERY terminal owner flagged (no one-live-suffices semantics)

mod g_allow_liveness {
    use crate::common::schema::{insert_task, seed_tasks_db};
    use reify_audit::{Pattern, Severity};

    fn marker(path: &str, line: usize, ids: &[u32], text: &str) -> (String, usize, Vec<u32>, String) {
        (path.to_string(), line, ids.to_vec(), text.to_string())
    }

    /// A terminal (done) owner cite → one g-allow-orphaned High finding.
    #[test]
    fn terminal_done_owner_emits_g_allow_orphaned_high() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 3429, "cancelled");

        let cited = vec![marker("src/x.rs", 10, &[3429], "// G-allow: marker text")];
        let findings = reify_audit::ptodo::resolve_g_allow_owner_liveness(&conn, &cited)
            .expect("resolve");

        assert_eq!(findings.len(), 1, "one g-allow-orphaned; got {findings:?}");
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo);
        assert_eq!(f.severity, Severity::High);
        assert!(f.summary.starts_with("g-allow-orphaned:"), "summary: {}", f.summary);
        assert!(f.summary.contains("#3429"), "must carry id: {}", f.summary);
        assert!(f.summary.contains("cancelled"), "must carry status: {}", f.summary);
        assert_eq!(f.task_id, "src/x.rs");
    }

    /// A second terminal variant (done status) → High g-allow-orphaned.
    #[test]
    fn terminal_done_owner_also_emits_g_allow_orphaned() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4444, "done");

        let cited = vec![marker("src/y.rs", 5, &[4444], "// G-allow: done task cite")];
        let findings = reify_audit::ptodo::resolve_g_allow_owner_liveness(&conn, &cited)
            .expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert_eq!(findings[0].severity, Severity::High);
        assert!(findings[0].summary.starts_with("g-allow-orphaned:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#4444"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("done"), "{}", findings[0].summary);
    }

    /// A non-terminal (pending) owner cite → zero findings.
    #[test]
    fn live_owner_emits_no_finding() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4743, "pending");

        let cited = vec![marker("src/p.rs", 1, &[4743], "// G-allow: live owner")];
        let findings = reify_audit::ptodo::resolve_g_allow_owner_liveness(&conn, &cited)
            .expect("resolve");

        assert!(findings.is_empty(), "live owner must yield no finding; got {findings:?}");
    }

    /// An absent owner id → g-allow-unknown-id Medium (fail-soft, DB-sync race).
    #[test]
    fn absent_id_emits_g_allow_unknown_id_medium() {
        let conn = seed_tasks_db();
        // 99999 is never seeded

        let cited = vec![marker("src/c.rs", 5, &[99999], "// G-allow: absent id")];
        let findings = reify_audit::ptodo::resolve_g_allow_owner_liveness(&conn, &cited)
            .expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert_eq!(findings[0].severity, Severity::Medium);
        assert!(findings[0].summary.starts_with("g-allow-unknown-id:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#99999"), "{}", findings[0].summary);
    }

    /// Two-owner marker [pending, done] → exactly one g-allow-orphaned for the
    /// done owner; no finding for the pending one.  Proves owner semantics flag
    /// EVERY terminal cite — unlike resolve_liveness "one-live-suffices".
    #[test]
    fn every_terminal_owner_flagged_not_one_live_suffices() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4743, "pending");
        insert_task(&conn, "master", 4444, "done");

        let cited = vec![marker("src/m.rs", 3, &[4743, 4444], "// G-allow: mixed owner marker")];
        let findings = reify_audit::ptodo::resolve_g_allow_owner_liveness(&conn, &cited)
            .expect("resolve");

        // Exactly one finding: for the done owner #4444. The pending #4743 is live.
        assert_eq!(findings.len(), 1, "expected exactly one g-allow-orphaned for #4444; got {findings:?}");
        assert_eq!(findings[0].severity, Severity::High);
        assert!(findings[0].summary.starts_with("g-allow-orphaned:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#4444"), "{}", findings[0].summary);
    }
}
