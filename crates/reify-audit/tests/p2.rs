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

    /// Verify that a stub marker appearing in two tasks' diffs at the same location
    /// produces exactly ONE finding, attributed to the introducer (earliest done_at).
    ///
    /// Tasks 7001 (done_at=200) and 7002 (done_at=100) both surface the identical
    /// added line at widget.rs:42. The introducer is 7002 (smaller done_at).
    #[test]
    fn shared_file_markers_attributed_once_to_introducer() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let path = "crates/shared/widget.rs";
        let shared_line = (42usize, "    unimplemented!()".to_string());

        let mut git = MockGitOps::new();
        git.set_diff_added_lines("main", "task/7001", path, vec![shared_line.clone()]);
        git.set_diff_added_lines("main", "task/7002", path, vec![shared_line.clone()]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert("7001".to_string(), TaskMetadata {
            task_id: "7001".to_string(),
            status: "done".to_string(),
            files: vec![path.to_string()],
            done_provenance: None,
            title: "Wire foo into bar".to_string(),
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: Some(200), // later → NOT the introducer
        });
        task_metadata.insert("7002".to_string(), TaskMetadata {
            task_id: "7002".to_string(),
            status: "done".to_string(),
            files: vec![path.to_string()],
            done_provenance: None,
            title: "Wire foo into bar".to_string(),
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: Some(100), // earlier → the introducer
        });

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
        assert_eq!(findings.len(), 1,
            "expected exactly 1 finding after de-dup (not one per task); got {:?}", findings);
        let f = &findings[0];
        assert_eq!(f.task_id, "7002",
            "finding must be attributed to introducer task 7002 (done_at=100); got {:?}", f.task_id);
        assert_eq!(f.pattern, Pattern::P2ConsumerStub);
        assert_eq!(f.severity, Severity::Medium);
        assert!(f.evidence.iter().any(|e| match e {
            EvidenceRef::File { path: p } => p == path, _ => false,
        }), "finding must reference the shared path; got {:?}", f.evidence);
    }

    /// Verify that `unimplemented!()` lines inside an inline `#[cfg(test)]` module
    /// are suppressed while a genuine production stub BEFORE the gate still flags.
    ///
    /// Mirrors geometry.rs where CountingKernel/FailAfterKernel stubs at lines
    /// 3035/3049 live inside `#[cfg(test)] mod tests {` opened at line 2688.
    #[test]
    fn cfg_test_inline_module_markers_suppressed() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9204";
        let path = "crates/reify-ir/src/geometry.rs";

        let mut git = MockGitOps::new();
        git.set_diff_added_lines(
            "main",
            &format!("task/{}", task_id),
            path,
            vec![
                (10,   "    unimplemented!() // genuine production stub".to_string()),
                (2688, "#[cfg(test)]".to_string()),
                (2689, "mod tests {".to_string()),
                (3035, "        unimplemented!(\"CountingKernel only supports query\")".to_string()),
                (3049, "        unimplemented!(\"CountingKernel only supports query\")".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![path.to_string()]));

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
        assert_eq!(findings.len(), 1, "expected exactly 1 finding; got {:?}", findings);
        let summary = &findings[0].summary;
        assert!(summary.contains("line 10"), "production stub at line 10 must be in summary; got: {summary}");
        assert!(!summary.contains("line 3035"), "inline-test stub at 3035 must NOT be in summary; got: {summary}");
        assert!(!summary.contains("line 3049"), "inline-test stub at 3049 must NOT be in summary; got: {summary}");
    }

    /// Verify that the detector's own source file is excluded from P2 scanning.
    ///
    /// The live code `return Some("TODO(post-)")` at p2_consumer_stub.rs:41
    /// lowercases to contain `todo(post-)` and self-matches Family 1. An identical
    /// line in a control path must still flag — proving exclusion keys on path, not
    /// content.
    #[test]
    fn detector_own_source_excluded() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9203";
        let task_branch = format!("task/{}", task_id);
        let self_path = "crates/reify-audit/src/p2_consumer_stub.rs";
        let control_path = "crates/other/src/real.rs";
        let self_matching_line = r#"        if inner.contains("post-") { return Some("TODO(post-)"); }"#;

        let mut git = MockGitOps::new();
        git.set_diff_added_lines("main", &task_branch, self_path,
            vec![(41, self_matching_line.to_string())]);
        git.set_diff_added_lines("main", &task_branch, control_path,
            vec![(41, self_matching_line.to_string())]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![
            self_path.to_string(),
            control_path.to_string(),
        ]));

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
        assert_eq!(findings.len(), 1,
            "expected exactly 1 finding (only control path); got {:?}", findings);
        let f = &findings[0];
        assert!(f.evidence.iter().any(|e| match e {
            EvidenceRef::File { path } => path == control_path, _ => false,
        }), "finding must reference {control_path}; got {:?}", f.evidence);
        assert!(!f.evidence.iter().any(|e| match e {
            EvidenceRef::File { path } => path == self_path, _ => false,
        }), "detector's own source {self_path} must not appear in findings");
    }

    /// Verify that non-executable file extensions (.ri, .yaml, .md) are excluded
    /// from P2 scanning even when they carry stub-pattern text in their diffs.
    /// Only a `.rs` control path must produce a finding.
    #[test]
    fn non_code_extension_paths_excluded() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9202";
        let task_branch = format!("task/{}", task_id);

        let mut git = MockGitOps::new();
        git.set_diff_added_lines("main", &task_branch, "crates/reify-stdlib/solver_elastic.ri",
            vec![(3, "// placeholder".to_string())]);
        git.set_diff_added_lines("main", &task_branch, "review/briefing.yaml",
            vec![(15, "  status: TODO(task_42) pending wiring".to_string())]);
        git.set_diff_added_lines("main", &task_branch, "docs/notes.md",
            vec![(8, "prose mentioning unimplemented!() here".to_string())]);
        git.set_diff_added_lines("main", &task_branch, "crates/x/real.rs",
            vec![(1, "    unimplemented!()".to_string())]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![
            "crates/reify-stdlib/solver_elastic.ri".to_string(),
            "review/briefing.yaml".to_string(),
            "docs/notes.md".to_string(),
            "crates/x/real.rs".to_string(),
        ]));

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
        assert_eq!(findings.len(), 1,
            "expected exactly 1 finding (only crates/x/real.rs); got {:?}", findings);
        let f = &findings[0];
        assert!(f.evidence.iter().any(|e| match e {
            EvidenceRef::File { path } => path == "crates/x/real.rs", _ => false,
        }), "the single finding must reference crates/x/real.rs; got {:?}", f.evidence);
        for excluded in &["crates/reify-stdlib/solver_elastic.ri", "review/briefing.yaml", "docs/notes.md"] {
            assert!(!f.evidence.iter().any(|e| match e {
                EvidenceRef::File { path } => path == excluded, _ => false,
            }), "excluded path {excluded} must not appear in findings");
        }
    }

    /// Verify that bare-comment prose (where the stub word is a sentence subject,
    /// not a label) is NOT flagged, while label-form comments ARE.
    ///
    /// Mirrors expr.rs:545 `// Placeholder is a leaf — no children to traverse.`
    /// which was falsely flagged by the bare `contains("// placeholder")` check.
    /// Lines 546–548 are label-form markers that must still flag.
    #[test]
    fn bare_comment_prose_not_flagged_label_form_still_flagged() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9201";
        let path = "crates/reify-ir/src/expr.rs";

        let mut git = MockGitOps::new();
        git.set_diff_added_lines(
            "main",
            &format!("task/{}", task_id),
            path,
            vec![
                (545, "            // Placeholder is a leaf \u{2014} no children to traverse.".to_string()),
                (546, "    // placeholder: TBD".to_string()),
                (547, "    // stub".to_string()),
                (548, "    // fixme".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![path.to_string()]));

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
        assert_eq!(findings.len(), 1, "expected exactly 1 finding (bundled for expr.rs); got {:?}", findings);
        let summary = &findings[0].summary;
        assert!(summary.contains("line 546"), "label-form placeholder at 546 must be in summary; got: {summary}");
        assert!(summary.contains("line 547"), "label-form stub at 547 must be in summary; got: {summary}");
        assert!(summary.contains("line 548"), "label-form fixme at 548 must be in summary; got: {summary}");
        assert!(!summary.contains("line 545"), "prose placeholder at 545 must NOT be in summary; got: {summary}");
    }

    /// Verify that doc-comment lines (`///` and `//!`) are NOT flagged as stubs,
    /// even when they contain stub-pattern text (Family 1/2 matches).
    ///
    /// Mirrors geometry.rs:3024 where `/// … a stub unimplemented!()` was being
    /// falsely flagged. Line 102 is a genuine code-level `unimplemented!()`.
    #[test]
    fn doc_comment_prose_not_flagged() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9200";
        let path = "crates/reify-ir/src/geometry.rs";

        let mut git = MockGitOps::new();
        git.set_diff_added_lines(
            "main",
            &format!("task/{}", task_id),
            path,
            vec![
                (100, "    /// not-supported default or a stub `unimplemented!()` — so we can".to_string()),
                (101, "    //! module doc: TODO(impl pending) historically".to_string()),
                (102, "    unimplemented!()".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![path.to_string()]));

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
            "expected exactly 1 finding (only line 102 is a real stub); got {:?}",
            findings
        );
        let summary = &findings[0].summary;
        assert!(summary.contains("line 102"), "summary must reference line 102; got: {summary}");
        assert!(!summary.contains("line 100"), "doc-comment line 100 must NOT appear in summary; got: {summary}");
        assert!(!summary.contains("line 101"), "doc-comment line 101 must NOT appear in summary; got: {summary}");
    }

    /// Regression guard: `#[cfg(not(test))]` must NOT trigger the inline-test gate.
    ///
    /// `#[cfg(not(test))]` is a production-only guard — code following it compiles
    /// ONLY when not running under the test harness. It must NOT suppress subsequent
    /// stub markers; those are genuine production stubs that P2 should flag.
    ///
    /// Before the fix the gate predicate was `starts_with("#[cfg(") && contains("test")`,
    /// which blindly matched `not(test)` and permanently suppressed all lines after it.
    #[test]
    fn cfg_not_test_does_not_suppress_production_stubs() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9205";
        let path = "crates/x/prod_guarded.rs";

        let mut git = MockGitOps::new();
        git.set_diff_added_lines(
            "main",
            &format!("task/{}", task_id),
            path,
            vec![
                // production-only guard — must NOT suppress the stub that follows
                (10, "#[cfg(not(test))]".to_string()),
                (11, "    unimplemented!() // production stub after not(test)".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![path.to_string()]));

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
            findings.len(), 1,
            "#[cfg(not(test))] must NOT suppress the production stub at line 11; got {:?}",
            findings
        );
        assert!(
            findings[0].summary.contains("line 11"),
            "production stub at line 11 must appear in summary; got: {}",
            findings[0].summary
        );
    }

    /// Pins the deliberate exclusion of `.py` and other non-allowlist extensions.
    ///
    /// P2 scanning is restricted to `.rs`/`.ts`/`.tsx`/`.js`/`.jsx` by `is_code_ext`.
    /// This test documents that `.py` (e.g. vendored sandbox helpers) and `.sh`
    /// exclusion is intentional — updating the extension set requires updating this
    /// test alongside `is_code_ext`, making the decision discoverable.
    #[test]
    fn python_and_other_extensions_excluded() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9206";
        let task_branch = format!("task/{}", task_id);

        let mut git = MockGitOps::new();
        // .py: vendored sandbox helper with a stub-like raise — excluded
        git.set_diff_added_lines("main", &task_branch,
            "gui/src-tauri/sandbox/landlock.py",
            vec![(5, "    raise NotImplementedError('TODO(pending)')".to_string())]);
        // .sh: build script — excluded
        git.set_diff_added_lines("main", &task_branch,
            "scripts/build.sh",
            vec![(3, "    unimplemented!()".to_string())]);
        // .rs: production code — must be flagged (control)
        git.set_diff_added_lines("main", &task_branch,
            "crates/x/real.rs",
            vec![(1, "    unimplemented!()".to_string())]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(task_id.to_string(), benign_meta(task_id, vec![
            "gui/src-tauri/sandbox/landlock.py".to_string(),
            "scripts/build.sh".to_string(),
            "crates/x/real.rs".to_string(),
        ]));

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
            findings.len(), 1,
            "only the .rs path must produce a finding; .py/.sh must be excluded; got {:?}",
            findings
        );
        assert!(findings[0].evidence.iter().any(|e| match e {
            EvidenceRef::File { path } => path == "crates/x/real.rs", _ => false,
        }), "the single finding must reference the .rs path; got {:?}", findings[0].evidence);
        assert!(!findings[0].evidence.iter().any(|e| match e {
            EvidenceRef::File { path } => path.ends_with(".py"), _ => false,
        }), ".py path must not appear in findings");
        assert!(!findings[0].evidence.iter().any(|e| match e {
            EvidenceRef::File { path } => path.ends_with(".sh"), _ => false,
        }), ".sh path must not appear in findings");
    }

    /// Verify that `Severity::Low` is preserved through de-dup when the winning
    /// (introducer) task's title signals a stub.
    ///
    /// Task 8001 (done_at=50, title contains "stub") is the introducer and
    /// would individually yield Severity::Low.  Task 8002 (done_at=200, benign
    /// title) is the later task and would yield Severity::Medium.  After de-dup
    /// the single finding must carry Severity::Low, attributed to task 8001.
    /// This pins the Phase-4 severity lookup path through the winning task's
    /// `title_signals_stub` branch.
    #[test]
    fn low_severity_preserved_after_dedup() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let path = "crates/shared/stub_module.rs";
        let shared_line = (10usize, "    unimplemented!()".to_string());

        let mut git = MockGitOps::new();
        // Task 8001: earliest done_at → the introducer; stub-signaling title → Low.
        git.set_diff_added_lines("main", "task/8001", path, vec![shared_line.clone()]);
        // Task 8002: later done_at → NOT the introducer; benign title → would be Medium.
        git.set_diff_added_lines("main", "task/8002", path, vec![shared_line.clone()]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert("8001".to_string(), TaskMetadata {
            task_id: "8001".to_string(),
            status: "done".to_string(),
            files: vec![path.to_string()],
            done_provenance: None,
            title: "Add stub for new subsystem".to_string(), // signals "stub" → Low
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: Some(50), // earliest → the introducer
        });
        task_metadata.insert("8002".to_string(), TaskMetadata {
            task_id: "8002".to_string(),
            status: "done".to_string(),
            files: vec![path.to_string()],
            done_provenance: None,
            title: "Wire foo into bar".to_string(), // benign → would be Medium
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: Some(200), // later → NOT the introducer
        });

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
        assert_eq!(findings.len(), 1,
            "expected exactly 1 finding after de-dup; got {:?}", findings);
        let f = &findings[0];
        assert_eq!(f.task_id, "8001",
            "finding must be attributed to introducer 8001 (done_at=50); got: {:?}", f.task_id);
        assert_eq!(f.severity, Severity::Low,
            "introducer's stub-signaling title must yield Severity::Low after dedup; got: {:?}",
            f.severity);
        assert_eq!(f.pattern, Pattern::P2ConsumerStub);
        assert!(f.evidence.iter().any(|e| match e {
            EvidenceRef::File { path: p } => p == path, _ => false,
        }), "finding must reference the shared path; got {:?}", f.evidence);
    }

    /// RED step 3 / GREEN step 4: Reaped-branch recall via done_provenance.commit.
    ///
    /// A `done` task whose `task/N` branch has been reaped (deleted) by the
    /// orchestrator still has a surviving `done_provenance.commit` (the merge
    /// commit on main).  The mock deliberately does NOT register a
    /// `set_diff_added_lines("main", "task/4500", ...)` entry — exactly as
    /// `RealGitOps` would return empty for `git diff main..task/4500` on a reaped
    /// branch (`fatal: bad revision`).
    ///
    /// The task HAS:
    ///   - `done_provenance: Some(commit = "m1")`
    ///   - `is_ancestor("m1", "main") = true`
    ///   - `diff_added_lines_in_commit("m1", "crates/x/widget.rs") = [(12, "    unimplemented!()")]`
    ///
    /// Expected: exactly 1 P2 finding (sourced from the commit diff, not the
    /// empty branch diff).  Current behaviour (pre-step-4): 0 findings (reaped
    /// branch → empty diff → missed).
    #[test]
    fn reaped_branch_recall_via_provenance_commit() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "4500";
        let path = "crates/x/widget.rs";

        let mut git = MockGitOps::new();
        // Merge commit is reachable from main.
        git.set_is_ancestor("m1", "main", true);
        // The commit diff surfaces the stub line.
        git.set_diff_added_lines_in_commit(
            "m1",
            path,
            vec![(12, "    unimplemented!()".to_string())],
        );
        // Deliberately do NOT set_diff_added_lines("main", "task/4500", ...)
        // → simulates a reaped branch returning empty from RealGitOps.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            TaskMetadata {
                task_id: task_id.to_string(),
                status: "done".to_string(),
                files: vec![path.to_string()],
                done_provenance: Some(reify_audit::DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("m1".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: Some(1000),
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

        let findings = p2_consumer_stub::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "reaped-branch task with reachable provenance commit must produce 1 P2 finding; \
             got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P2ConsumerStub);
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.task_id, task_id);
        assert!(
            f.evidence.iter().any(|e| match e {
                EvidenceRef::File { path: p } => p == path,
                _ => false,
            }),
            "finding must reference {path}; got {:?}",
            f.evidence
        );
    }

    /// RED step 7 / GREEN step 8: Unreachable (recycled/gc'd) commit falls back
    /// to a full-file content scan via `file_lines_on("main", path)`.
    ///
    /// The task has `done_provenance.commit = "gone"` but
    /// `is_ancestor("gone", "main") = false` (SHA is gc'd / recycled).
    ///
    /// The mock registers `file_lines_on("main", path)` with a full-file line
    /// stream that includes a genuine production stub.  Neither
    /// `set_diff_added_lines_in_commit` nor `set_diff_added_lines` is set
    /// (both return empty by default), proving the finding originates from
    /// the content scan.
    ///
    /// Also asserts that `scan_file_added_lines`'s positional `#[cfg(test)]`
    /// gate still works on the full-file stream: a stub AFTER a `#[cfg(test)]`
    /// line is suppressed.
    ///
    /// Expected: exactly 1 P2 finding (from the pre-gate stub line); the
    /// post-gate stub is suppressed.  Current behaviour (pre-step-8): 0 findings.
    #[test]
    fn unreachable_commit_falls_back_to_content_scan() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "4600";
        let path = "crates/x/recycled.rs";

        let mut git = MockGitOps::new();
        // SHA is gc'd / recycled — NOT an ancestor of main.
        git.set_is_ancestor("gone", "main", false);
        // Full-file content on main: a genuine production stub before the cfg(test) gate.
        git.set_file_lines_on(
            "main",
            path,
            vec![
                (1, "fn f() {}".to_string()),
                (2, "    unimplemented!()".to_string()),
                (3, "#[cfg(test)]".to_string()),
                (4, "mod tests {".to_string()),
                (5, "    unimplemented!() // inside test module — should be suppressed".to_string()),
            ],
        );
        // Deliberately set neither diff_added_lines_in_commit NOR diff_added_lines.

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            TaskMetadata {
                task_id: task_id.to_string(),
                status: "done".to_string(),
                files: vec![path.to_string()],
                done_provenance: Some(reify_audit::DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("gone".to_string()),
                    note: None,
                }),
                title: "Wire foo into bar".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: Some(2000),
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

        let findings = p2_consumer_stub::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "unreachable-commit task with content-scan fallback must produce 1 P2 finding; \
             got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P2ConsumerStub);
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.task_id, task_id);
        assert!(
            f.evidence.iter().any(|e| match e {
                EvidenceRef::File { path: p } => p == path,
                _ => false,
            }),
            "finding must reference {path}; got {:?}",
            f.evidence
        );
        // The summary must reference line 2 (pre-gate stub) but NOT line 5 (post-gate).
        let summary = &f.summary;
        assert!(
            summary.contains("line 2"),
            "production stub at line 2 must appear in summary; got: {summary}",
        );
        assert!(
            !summary.contains("line 5"),
            "test-module stub at line 5 must be suppressed by cfg(test) gate; got: {summary}",
        );
    }

} // mod tests

} // mod p2
