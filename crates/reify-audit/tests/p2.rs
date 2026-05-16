//! Integration tests for the P2 consumer-stub detector.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §5 P2):
//!   `cargo test -p reify-audit p2::tests`
//!
//! The body is wrapped in `mod p2 { mod tests { ... } }` so each test's
//! path becomes `p2::tests::<name>` — matching the p5.rs convention.
//!
//! All tests use in-memory rusqlite + MockGitOps so they remain hermetic
//! (P2 issues zero SQL; the in-memory connection satisfies AuditContext.conn
//! without requiring a schema).

mod p2 {

use reify_audit::{
    AuditContext, EvidenceRef, MockGitOps, MockJCodemunchOps, Pattern, Severity, TaskMetadata,
    p2_consumer_stub,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// Build a minimal TaskMetadata with a benign title that does NOT
/// contain "stub" or "placeholder" (so P2 emits Medium, not Low).
fn benign_meta(task_id: &str, files: Vec<String>) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        status: "in_progress".to_string(),
        files,
        done_provenance: None,
        title: "Wire foo into bar".to_string(),
        prd: None,
        consumer_ref: None,
        audit_foundation: None,
        done_at: None,
    }
}

mod tests {
    use super::*;

    /// Verify that all eleven canonical stub-pattern families are detected when
    /// they appear on the added-lines side of a diff (i.e. as `+` lines).
    ///
    /// Families covered (one synthetic path per family):
    ///   a) `crates/x/a.rs` — `// TODO(impl pending)`        (TODO.*pending)
    ///   b) `crates/x/b.rs` — `// TODO(post-merge)`          (TODO post-\w+)
    ///   c) `crates/x/c.rs` — `// TODO(wire later)`          (TODO.*later)
    ///   d) `crates/x/d.rs` — `// TODO(task_9999)`           (TODO task_\d+)
    ///   e) `crates/x/e.rs` — `unimplemented!()`
    ///   f) `crates/x/f.rs` — `panic!("not yet wired")`
    ///   g) `crates/x/g.rs` — `tracing::warn!(reason="task_foo_pending", "x")`
    ///   h) `crates/x/h.rs` — `Value::Undef => { /* pending */ }`  (Undef+pending)
    ///   i) `crates/x/i.rs` — `// fixme`
    ///   j) `crates/x/j.rs` — `// stub: wire later`          (Family 6 // stub)
    ///   k) `crates/x/k.rs` — `// placeholder: TBD`          (Family 6 // placeholder)
    ///
    /// Each path has exactly one stub line → eleven findings expected.
    /// A per-family label assertion loop (see below) now pins each family's
    /// routed `&'static str` label so that any cross-family swap (e.g. a
    /// `// fixme` arm returning `"// stub"`) or mislabelling surfaces
    /// immediately — not just the presence-of-finding invariant.
    #[test]
    fn detects_canonical_stub_patterns_on_added_lines() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9001";
        let paths: Vec<&str> = vec![
            "crates/x/a.rs",
            "crates/x/b.rs",
            "crates/x/c.rs",
            "crates/x/d.rs",
            "crates/x/e.rs",
            "crates/x/f.rs",
            "crates/x/g.rs",
            "crates/x/h.rs",
            "crates/x/i.rs",
            "crates/x/j.rs",
            "crates/x/k.rs",
        ];
        let stub_lines: Vec<&str> = vec![
            "    // TODO(impl pending)",
            "    // TODO(post-merge)",
            "    // TODO(wire later)",
            "    // TODO(task_9999)",
            "    unimplemented!()",
            "    panic!(\"not yet wired\")",
            "    tracing::warn!(reason=\"task_foo_pending\", \"x\")",
            "    Value::Undef => { /* pending */ }",
            "    // fixme",
            "    // stub: wire later",
            "    // placeholder: TBD",
        ];

        let mut git = MockGitOps::new();
        let task_branch = format!("task/{}", task_id);
        for (path, stub_line) in paths.iter().zip(stub_lines.iter()) {
            git.set_diff_added_lines(
                "main",
                &task_branch,
                path,
                vec![(10, stub_line.to_string())],
            );
        }

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            benign_meta(task_id, paths.iter().map(|p| p.to_string()).collect()),
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

        let findings = p2_consumer_stub::check(&ctx);
        assert_eq!(
            findings.len(),
            11,
            "expected 11 findings (one per stub-pattern path); got {:?}",
            findings
        );
        for f in &findings {
            assert_eq!(f.pattern, Pattern::P2ConsumerStub, "wrong pattern: {:?}", f);
            assert_eq!(f.severity, Severity::Medium, "benign title should → Medium: {:?}", f);
        }
        // Each finding's evidence must reference the correct file path.
        for path in &paths {
            let found = findings.iter().any(|f| {
                f.evidence.iter().any(|e| match e {
                    EvidenceRef::File { path: p } => p == path,
                    _ => false,
                })
            });
            assert!(found, "no finding with EvidenceRef::File for {path}");
        }

        // Per-family label assertions: each finding's summary must contain
        // "[<label>]" matching the `&'static str` returned by `line_matches_stub`
        // for that family (rendered as `"line N [label]: snippet"` at
        // p2_consumer_stub.rs:151). Using the bracketed form pins the routed
        // position and guards against accidental fragment matches inside snippets.
        let expected_labels: Vec<&str> = vec![
            "TODO(pending)",                           // [0] a.rs — TODO(impl pending)
            "TODO(post-\\w+)",                         // [1] b.rs — TODO(post-merge)
            "TODO(later)",                             // [2] c.rs — TODO(wire later)
            "TODO(task_N)",                            // [3] d.rs — TODO(task_9999)
            "unimplemented!",                          // [4] e.rs — unimplemented!()
            "panic!(not yet)",                         // [5] f.rs — panic!("not yet wired")
            "tracing::warn!(task_pending)",            // [6] g.rs — tracing::warn! family
            "Value::Undef(pending/stub/placeholder)",  // [7] h.rs — Value::Undef family
            "// fixme",                                // [8] i.rs — // fixme
            "// stub",                                 // [9] j.rs — // stub: wire later
            "// placeholder",                          // [10] k.rs — // placeholder: TBD
        ];
        for (path, label) in paths.iter().zip(expected_labels.iter()) {
            let bracketed = format!("[{}]", label);
            let found = findings.iter().any(|f| {
                f.evidence.iter().any(|e| match e {
                    EvidenceRef::File { path: p } => p == path,
                    _ => false,
                }) && f.summary.contains(&bracketed)
            });
            assert!(
                found,
                "no finding for {path} with summary containing {bracketed}; findings: {:?}",
                findings
            );
        }
    }

    /// Verify that stub patterns that were already present on `main` (i.e. NOT
    /// added by this branch) are NOT flagged.  This pins the "added-lines seam
    /// only" invariant: the detector must consult `diff_added_lines`, never the
    /// full file contents.
    ///
    /// The mock returns a *non-empty* vec of added lines — all genuine
    /// implementation lines with no stub markers — to confirm the detector
    /// correctly classifies clean added content as non-findings, not just that
    /// an empty seam produces no output.
    #[test]
    fn moved_code_preexisting_not_flagged() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9002";
        let path = "crates/x/moved.rs";

        let mut git = MockGitOps::new();
        // Non-empty added-lines containing clean implementation code (no stubs).
        // The pre-existing `// TODO(pending)` in the file was NOT touched by this
        // branch, so it does not appear as a `+` line here.
        git.set_diff_added_lines(
            "main",
            &format!("task/{}", task_id),
            path,
            vec![
                (5, "    let x = compute_value();".to_string()),
                (6, "    Ok(())".to_string()),
                (7, "    // Proper implementation comment".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            benign_meta(task_id, vec![path.to_string()]),
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

        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "pre-existing stubs on main must NOT be flagged; got {:?}",
            findings
        );
    }

    /// Verify that paths whose shape indicates a test file are excluded from
    /// P2 scanning, even when they carry real stub markers on the added-lines
    /// side.  Six test-shaped paths and one non-test path:
    ///   - `crates/foo/tests/integration_bar.rs`  — contains `/tests/`
    ///   - `src/lexer_test.rs`                    — ends with `_test.rs`
    ///   - `frontend/__tests__/foo.ts`             — contains `__tests__/`
    ///   - `tests/root_foo.rs`                    — starts with `tests/` (no leading slash)
    ///   - `frontend/foo.test.ts`                  — contains `.test.` (JS/TS)
    ///   - `frontend/bar.spec.ts`                  — contains `.spec.` (JS/TS)
    ///   - `src/foo.rs`                            — production file → flagged
    ///
    /// All seven carry `// TODO(impl pending)` as an added line.  Exactly one
    /// finding must emerge and it must reference `src/foo.rs`.
    #[test]
    fn test_file_paths_excluded() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9003";
        let task_branch = format!("task/{}", task_id);

        let test_paths = vec![
            "crates/foo/tests/integration_bar.rs",
            "src/lexer_test.rs",
            "frontend/__tests__/foo.ts",
            "tests/root_foo.rs",
            "frontend/foo.test.ts",
            "frontend/bar.spec.ts",
        ];
        let prod_path = "src/foo.rs";
        let stub_line = (1usize, "    // TODO(impl pending)".to_string());

        let mut git = MockGitOps::new();
        for p in &test_paths {
            git.set_diff_added_lines("main", &task_branch, p, vec![stub_line.clone()]);
        }
        git.set_diff_added_lines("main", &task_branch, prod_path, vec![stub_line.clone()]);

        let all_files: Vec<String> = test_paths
            .iter()
            .map(|p| p.to_string())
            .chain(std::iter::once(prod_path.to_string()))
            .collect();

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, all_files));

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

        let findings = p2_consumer_stub::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding (only src/foo.rs); got {:?}",
            findings
        );
        let f = &findings[0];
        assert!(
            f.evidence.iter().any(|e| match e {
                EvidenceRef::File { path } => path == prod_path,
                _ => false,
            }),
            "the single finding must reference src/foo.rs; got {:?}",
            f.evidence
        );
        for tp in &test_paths {
            assert!(
                !f.evidence.iter().any(|e| match e {
                    EvidenceRef::File { path } => path == tp,
                    _ => false,
                }),
                "test-shaped path {tp} must not appear in findings"
            );
        }
    }

    /// Verify that when the task title contains "stub" or "placeholder"
    /// (case-insensitive), the P2 finding is downgraded to Severity::Low.
    ///
    /// Two sub-cases:
    ///   (a) title = "Add stub for foo subsystem"  — contains "stub"
    ///   (b) title = "Wire placeholder for bar"    — contains "placeholder"
    #[test]
    fn stub_in_title_downgrades_to_low() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let path = "src/foo.rs";
        let stub_line = vec![(5usize, "    unimplemented!()".to_string())];

        let check_downgrade = |title: &str, task_id: &str| {
            let mut git = MockGitOps::new();
            git.set_diff_added_lines(
                "main",
                &format!("task/{}", task_id),
                path,
                stub_line.clone(),
            );
            let mut task_metadata = HashMap::new();
            task_metadata.insert(
                task_id.to_string(),
                TaskMetadata {
                    task_id: task_id.to_string(),
                    status: "in_progress".to_string(),
                    files: vec![path.to_string()],
                    done_provenance: None,
                    title: title.to_string(),
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
            p2_consumer_stub::check(&ctx)
        };

        // (a) Title contains "stub"
        let findings_a = check_downgrade("Add stub for foo subsystem", "9010");
        assert_eq!(findings_a.len(), 1, "expected 1 finding for stub title; got {:?}", findings_a);
        assert_eq!(
            findings_a[0].severity,
            Severity::Low,
            "title with 'stub' must downgrade to Low; got {:?}",
            findings_a[0].severity
        );
        assert_eq!(findings_a[0].pattern, Pattern::P2ConsumerStub);

        // (b) Title contains "placeholder"
        let findings_b = check_downgrade("Wire placeholder for bar", "9011");
        assert_eq!(findings_b.len(), 1, "expected 1 finding for placeholder title; got {:?}", findings_b);
        assert_eq!(
            findings_b[0].severity,
            Severity::Low,
            "title with 'placeholder' must downgrade to Low; got {:?}",
            findings_b[0].severity
        );
        assert_eq!(findings_b[0].pattern, Pattern::P2ConsumerStub);
    }

    /// Pins the UTF-8-safety invariant for snippet truncation.
    ///
    /// A `+` line that straddles byte 60 with a multi-byte UTF-8 character
    /// (`…` = U+2026 = `e2 80 a6`, 3 bytes) must NOT cause a panic inside
    /// the truncation branch.  Before the fix, `&snippet[..60]` panics with
    /// "byte index 60 is not a char boundary".
    ///
    /// The prefix is 22 bytes, padding is 36 bytes → cumulative 58 bytes;
    /// the `…` glyph occupies bytes 58, 59, 60 so byte 60 is mid-char.
    #[test]
    fn truncates_long_snippet_on_char_boundary_without_panicking() {
        let prefix = "// TODO(impl pending) "; // 22 bytes; matches TODO(pending) family
        let padding = "x".repeat(36); // 36 ASCII bytes → cumulative 58 bytes
        let stub_line = format!(
            "{prefix}{padding}\u{2026}and trailing content to exceed sixty bytes total here."
        );
        // Self-documenting precondition asserts so the construction is
        // robust to future edits.
        assert_eq!(
            prefix.len() + padding.len(),
            58,
            "precondition: boundary starts at byte 58"
        );
        assert!(
            stub_line.len() > 60,
            "precondition: stub_line must exceed 60 bytes"
        );
        assert!(
            !stub_line.is_char_boundary(60),
            "precondition: byte 60 must be mid-char (inside the U+2026 glyph)"
        );

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let mut git = MockGitOps::new();
        git.set_diff_added_lines(
            "main",
            "task/9020",
            "src/foo.rs",
            vec![(7, stub_line.clone())],
        );

        let mut task_metadata = HashMap::new();
        task_metadata
            .insert("9020".to_string(), benign_meta("9020", vec!["src/foo.rs".to_string()]));

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

        // Before the fix this panics with "byte index 60 is not a char boundary".
        let findings = p2_consumer_stub::check(&ctx);

        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding; got {:?}",
            findings
        );
        assert_eq!(findings[0].pattern, Pattern::P2ConsumerStub);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert!(
            findings[0].summary.contains("src/foo.rs"),
            "summary must reference the path; got: {}",
            findings[0].summary
        );
        assert!(
            findings[0].summary.contains("TODO(pending)"),
            "summary must include the pattern label; got: {}",
            findings[0].summary
        );
    }


    /// Pins the discrimination half of `docs/architecture-audit/f-infra-design.md`
    /// §10 acceptance-criterion: "seven canonical stub patterns detected, seven
    /// non-stub patterns not" — this test covers the "not" side (regression-guard).
    ///
    /// Six near-miss added lines, each probing the discrimination boundary of one
    /// family:
    ///   (a) `// TODO(refactor) // see task_123` — `task_123` outside `TODO(...)`
    ///       parens must NOT match `TODO(task_N)` (Family 1 paren-scoping).
    ///   (b) `panic!("bad input")` — bare panic without `not yet` must NOT match
    ///       Family 3.
    ///   (c) `// TODO(refactor)` — TODO with no Family-1 sub-pattern must NOT match.
    ///   (d) `Value::Undef => { /* unhandled */ }` — Undef arm without
    ///       pending/stub/placeholder must NOT match Family 5.
    ///   (e) `tracing::warn!(reason="some_other_reason", "x")` — warn! without
    ///       `task_*_pending` reason must NOT match Family 4.
    ///   (f) `// TODO_LIST.md note about followup` — "todo" not followed by `(`
    ///       must NOT match any family (lexical `todo(` guard in Family 1).
    ///
    /// Expected outcome: `findings.is_empty()`. If this test fails, a real
    /// regression in `line_matches_stub`'s discrimination logic has been introduced.
    #[test]
    fn near_miss_lines_not_flagged() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9100";
        let near_miss_paths: Vec<&str> = vec![
            "crates/x/near_a.rs",
            "crates/x/near_b.rs",
            "crates/x/near_c.rs",
            "crates/x/near_d.rs",
            "crates/x/near_e.rs",
            "crates/x/near_f.rs",
        ];
        let near_miss_lines: Vec<&str> = vec![
            "    // TODO(refactor) // see task_123",
            "    panic!(\"bad input\")",
            "    // TODO(refactor)",
            "    Value::Undef => { /* unhandled */ }",
            "    tracing::warn!(reason=\"some_other_reason\", \"x\")",
            "    // TODO_LIST.md note about followup",
        ];

        let mut git = MockGitOps::new();
        let task_branch = format!("task/{}", task_id);
        for (path, line) in near_miss_paths.iter().zip(near_miss_lines.iter()) {
            git.set_diff_added_lines(
                "main",
                &task_branch,
                path,
                vec![(1, line.to_string())],
            );
        }

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            benign_meta(task_id, near_miss_paths.iter().map(|p| p.to_string()).collect()),
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

        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "near-miss lines must NOT produce findings; got {:?}",
            findings
        );
    }

} // mod tests

} // mod p2
