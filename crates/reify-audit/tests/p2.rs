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
    TimeWindow, p2_consumer_stub,
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
    /// Each finding's summary is structurally verified to contain
    /// `[<non-empty label>]:` (confirming the rendering contract), and two
    /// representative family labels are pinned byte-for-byte (Family 1 and
    /// Family 6) to anchor label routing without hardcoding all eleven strings
    /// (which would force two-place edits on any future label rename).
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
            producer_branch: None,
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

        // Structural pin: every path must produce a finding whose summary uses
        // the "line N [<non-empty label>]: snippet" format.  This guards the
        // rendering contract without hardcoding all eleven label strings.
        for path in &paths {
            let has_structural_label = findings.iter().any(|f| {
                let refs_path = f.evidence.iter().any(|e| match e {
                    EvidenceRef::File { path: p } => p == path,
                    _ => false,
                });
                if !refs_path {
                    return false;
                }
                // Verify "[label]:" appears with a non-empty label between the brackets.
                f.summary.find('[').and_then(|open| {
                    f.summary[open + 1..].find("]:").map(|close| close > 0)
                }).unwrap_or(false)
            });
            assert!(
                has_structural_label,
                "no finding for {path} with '[<label>]:' rendering in summary; findings: {:?}",
                findings
            );
        }

        // Exact label pin for two representative families anchors the label-routing
        // contract.  A cross-family swap (e.g. Family-6 `// fixme` arm returning
        // `"// stub"`) is caught here without requiring all eleven strings to be
        // duplicated verbatim (reducing two-place-edit burden on future renames).
        let family1_bracketed = "[TODO(pending)]";
        let family1_ok = findings.iter().any(|f| {
            f.evidence.iter().any(|e| match e {
                EvidenceRef::File { path: p } => p == "crates/x/a.rs",
                _ => false,
            }) && f.summary.contains(family1_bracketed)
        });
        assert!(
            family1_ok,
            "Family-1 label 'TODO(pending)' missing from a.rs finding; findings: {:?}",
            findings
        );

        let family6_bracketed = "[// fixme]";
        let family6_ok = findings.iter().any(|f| {
            f.evidence.iter().any(|e| match e {
                EvidenceRef::File { path: p } => p == "crates/x/i.rs",
                _ => false,
            }) && f.summary.contains(family6_bracketed)
        });
        assert!(
            family6_ok,
            "Family-6 label '// fixme' missing from i.rs finding; findings: {:?}",
            findings
        );
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
            producer_branch: None,
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
            producer_branch: None,
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
                producer_branch: None,
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
            producer_branch: None,
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
    /// Seven near-miss added lines, each probing the discrimination boundary of one
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
    ///   (g) `let result = unimplemented_macro();` — "unimplemented" as a
    ///       substring but WITHOUT the trailing `!(` must NOT match Family 2
    ///       (`unimplemented!(`-exact check).
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
            "crates/x/near_g.rs",
        ];
        let near_miss_lines: Vec<&str> = vec![
            "    // TODO(refactor) // see task_123",
            "    panic!(\"bad input\")",
            "    // TODO(refactor)",
            "    Value::Undef => { /* unhandled */ }",
            "    tracing::warn!(reason=\"some_other_reason\", \"x\")",
            "    // TODO_LIST.md note about followup",
            "    let result = unimplemented_macro();",
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
            producer_branch: None,
        };

        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "near-miss lines must NOT produce findings; got {:?}",
            findings
        );
    }

    /// Pins the sweep-mode contract: `check()` must panic (via `debug_assert!`) when
    /// called with an unbounded backlog — `target_task_id=None`, `window=None`, and
    /// `task_metadata.len() > SWEEP_BACKLOG_WARN_THRESHOLD` (50).
    ///
    /// Without pre-narrowing, every in-progress task carrying a legitimate WIP
    /// `TODO(... pending)` would silently surface as a Medium-severity finding,
    /// because P2 has no internal status filter to suppress legitimate WIP markers.
    /// The contract enforces that sweep-mode callers MUST narrow `ctx.task_metadata`
    /// to closing-window tasks before calling `check()`.
    ///
    /// The 51-entry backlog (one above threshold) uses `files: vec![]` so no
    /// `diff_added_lines` calls fire — the guard panics before per-task iteration.
    ///
    /// Reference: esc-3752-365 (reviewer-accepted sweep-mode contract suggestion).
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "unbounded backlog")]
    fn sweep_mode_unbounded_backlog_panics_in_debug() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        // Construct 51 entries — one more than SWEEP_BACKLOG_WARN_THRESHOLD (50).
        // files: vec![] keeps the test hermetic — no diff calls fire because the
        // guard panics before per-task iteration begins.
        let mut task_metadata = HashMap::new();
        for i in 0..51usize {
            let id = format!("3000{i:02}");
            task_metadata.insert(id.clone(), benign_meta(&id, vec![]));
        }

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None, // added by task 3691 — required field
        };

        // Before the impl guard lands: check() returns Vec::new() without panicking,
        // causing #[should_panic] to fail ("test did not panic as expected").
        let _ = p2_consumer_stub::check(&ctx);
    }

    /// Pins the strict-`>` boundary of `SWEEP_BACKLOG_WARN_THRESHOLD` (50).
    ///
    /// A backlog of EXACTLY 50 tasks with `target_task_id=None` and `window=None`
    /// must NOT trigger the guard — the threshold constant's doc says "strict `>` so
    /// a backlog of exactly 50 does not warn".
    ///
    /// An off-by-one regression to `>=` would silently pass
    /// `sweep_mode_unbounded_backlog_panics_in_debug` (which uses 51 entries) yet
    /// would panic here, making this the sole regression-catcher for the
    /// inclusive/exclusive boundary.
    ///
    /// Reference: esc-3752-365 review suggestion 2 (boundary test).
    #[test]
    fn sweep_mode_at_threshold_does_not_panic() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        // Exactly 50 entries — equal to SWEEP_BACKLOG_WARN_THRESHOLD (not one above).
        // files: vec![] keeps the test hermetic (no diff calls fire).
        let mut task_metadata = HashMap::new();
        for i in 0..50usize {
            let id = format!("4000{i:02}");
            task_metadata.insert(id.clone(), benign_meta(&id, vec![]));
        }

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

        // Must return without panicking; 50 entries does NOT exceed the strict->50
        // threshold. Any panic here indicates an off-by-one regression (>= vs >).
        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "50 benign tasks with files=vec![] should produce no findings; got {:?}",
            findings
        );
    }

    /// Pins the allow-path of the sweep-mode guard: a backlog of 51 tasks (one above
    /// threshold) with `window=Some(...)` must NOT trigger the guard, because the
    /// conjunction includes `ctx.window.is_none()`.
    ///
    /// A regression that drops the `window.is_none()` conjunct (reducing the predicate
    /// to just `task_metadata.len() > 50`) would panic here and break every legitimate
    /// large `--since` sweep.
    ///
    /// NOTE: As documented by the KNOWN LIMITATION comment in check(), `ctx.window` is
    /// NOT actually consumed by P2, so window=Some does not guarantee that
    /// task_metadata was narrowed. This test pins the CURRENT guard behavior — the
    /// window=Some allow-path — not an ideal narrowing contract. See esc-3752-365
    /// review suggestion 1 for the full analysis.
    ///
    /// Reference: esc-3752-365 review suggestion 3 (allow-path test, window variant).
    #[test]
    fn sweep_mode_with_window_scoping_does_not_panic() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        // 51 entries — one above SWEEP_BACKLOG_WARN_THRESHOLD.
        let mut task_metadata = HashMap::new();
        for i in 0..51usize {
            let id = format!("5000{i:02}");
            task_metadata.insert(id.clone(), benign_meta(&id, vec![]));
        }

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            // window=Some suppresses the guard (ctx.window.is_none() == false).
            window: Some(TimeWindow {
                since: Some("2026-05-01T00:00:00Z".to_string()),
                until: None,
            }),
            now: None,
            producer_branch: None,
        };

        // Must return without panicking. A regression dropping `window.is_none()`
        // from the guard predicate would cause this to panic.
        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "51 benign tasks with files=vec![] and window=Some should produce no findings; got {:?}",
            findings
        );
    }

    /// Pins the allow-path of the sweep-mode guard: a backlog of 51 tasks (one above
    /// threshold) with `target_task_id=Some(...)` must NOT trigger the guard, because
    /// the conjunction includes `ctx.target_task_id.is_none()`.
    ///
    /// A regression dropping the `target_task_id.is_none()` conjunct would panic here
    /// and break every pre-done hook invocation where ctx.task_metadata happens to
    /// hold more than 50 entries.
    ///
    /// Reference: esc-3752-365 review suggestion 3 (allow-path test, target_task_id variant).
    #[test]
    fn sweep_mode_with_target_task_id_does_not_panic() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        // 51 entries — one above SWEEP_BACKLOG_WARN_THRESHOLD.
        let mut task_metadata = HashMap::new();
        for i in 0..51usize {
            let id = format!("6000{i:02}");
            task_metadata.insert(id.clone(), benign_meta(&id, vec![]));
        }

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            // target_task_id=Some suppresses the guard (ctx.target_task_id.is_none() == false).
            target_task_id: Some("600000".to_string()),
            window: None,
            now: None,
            producer_branch: None,
        };

        // Must return without panicking. A regression dropping
        // `target_task_id.is_none()` from the guard predicate would panic here.
        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "51 benign tasks with files=vec![] and target_task_id=Some should produce no findings; got {:?}",
            findings
        );
    }

} // mod tests

} // mod p2
