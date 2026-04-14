/// Workspace-wide regression guard: no `#[ignore]` reason string in any test
/// file under the workspace should contain a stale transient-plan-doc pointer
/// (e.g. `plan step-N` breadcrumbs).
///
/// The marker and needle are assembled at runtime so this integration test file
/// does not itself contain the literal substrings it guards against.
///
/// See also: `crates/reify-expr/tests/field_calculus_tests.rs` —
/// `ignore_reason_strings_have_no_stale_plan_pointers`, which performs
/// file-local checks (including the positive `"known bug:"` prefix invariant)
/// for that specific file. This test adds the negative guard workspace-wide.
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

    let test_files = reify_test_support::ignore_hygiene::walk_test_rs_files(workspace_root);

    // Assemble marker and needle at runtime so this file does not contain
    // the literal substrings and does not self-trigger when scanned.
    let mut all_violations: Vec<String> = Vec::new();
    for path in &test_files {
        // Skip unreadable files (e.g. deleted mid-walk during concurrent cargo
        // runs) rather than failing the regression test with a false positive.
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let violations =
            reify_test_support::ignore_hygiene::find_stale_plan_pointers_in_source(&source);
        for violation in violations {
            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or(path)
                .display()
                .to_string();
            all_violations.push(format!("{rel}: {violation}"));
        }
    }

    assert!(
        all_violations.is_empty(),
        "Found stale plan-step pointer(s) in #[ignore] reason strings.\n\
         Replace each pointer with a self-contained inline summary \
         (e.g. \"known bug: describe the actual failure mode here\").\n\
         Offenders:\n  {}",
        all_violations.join("\n  ")
    );
}
