/// Workspace-wide regression guard: no `#[ignore]` reason string in any test
/// file under the workspace should contain a stale transient-plan-doc pointer
/// (e.g. `plan step-N` breadcrumbs).
///
/// Delegates to `reify_test_support::ignore_hygiene::collect_workspace_stale_pointers`,
/// which combines `walk_test_rs_files` and `find_stale_plan_pointers_in_source`
/// into a single reusable helper.  The inlined walk-read-detect loop that
/// previously lived here has been extracted into that helper so this test, the
/// unit-level pin in `ignore_hygiene.rs`, and any future callers all share one
/// source of truth.
///
/// Doc-comment lines (`///`, `//!`) are skipped silently by
/// `find_stale_plan_pointers_in_source` via the shared `is_doc_comment_line`
/// predicate in `reify_test_support::ignore_hygiene`.  This is the same
/// predicate used by `check_ignore_reasons` (the file-local strict scanner),
/// so the two scanners cannot drift in how they classify doc-comment lines
/// (lock-step guarantee).
///
/// See also: `crates/reify-expr/tests/field_calculus_tests.rs` —
/// `ignore_reason_strings_have_no_stale_plan_pointers`, which performs
/// file-local strict checks (including the positive `"known bug:"` prefix
/// invariant) by calling `reify_test_support::ignore_hygiene::check_ignore_reasons`.
/// This workspace test uses the more permissive `find_stale_plan_pointers_in_source`
/// because nine test files in `reify-eval` use `#[ignore]` reasons that don't
/// yet follow the `"known bug:"` convention.
use std::path::Path;

#[test]
fn no_stale_plan_pointers_in_workspace_ignore_reasons() {
    // Resolve workspace root: CARGO_MANIFEST_DIR is crates/reify-test-support,
    // so two .parent() calls reach the workspace root.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .parent()
        .expect("crates/ parent of CARGO_MANIFEST_DIR")
        .parent()
        .expect("workspace root parent of crates/");

    let all_violations =
        reify_test_support::ignore_hygiene::collect_workspace_stale_pointers(workspace_root);

    assert!(
        all_violations.is_empty(),
        "Found stale plan-step pointer(s) in #[ignore] reason strings.\n\
         Replace each pointer with a self-contained inline summary \
         (e.g. \"known bug: describe the actual failure mode here\").\n\
         Offenders:\n  {}",
        all_violations.join("\n  ")
    );
}
