use std::path::{Path, PathBuf};

/// Scan `source` for `#[ignore = "..."]` reason strings that contain a stale
/// transient-plan-doc pointer (e.g. a `plan step-N` breadcrumb). Returns one
/// human-readable violation string per offender. Empty Vec means clean.
///
/// The marker and needle are assembled at runtime so this source file does not
/// contain the literal substrings and does not self-trigger when scanned.
pub fn find_stale_plan_pointers_in_source(source: &str) -> Vec<String> {
    // Assembled at runtime — do not inline these as literals.
    let marker = ["#[ignore", " = \""].concat();
    let needle = ["plan", " step-"].concat();

    let mut violations = Vec::new();
    let mut remaining = source;
    let mut byte_offset: usize = 0;

    while let Some(rel_pos) = remaining.find(marker.as_str()) {
        let abs_pos = byte_offset + rel_pos;
        let line_num = source[..abs_pos].bytes().filter(|&b| b == b'\n').count() + 1;

        let after_marker = &remaining[rel_pos + marker.len()..];
        if let Some(end) = after_marker.find('"') {
            let reason = &after_marker[..end];
            if reason.contains(needle.as_str()) {
                let preview: String = reason.chars().take(80).collect();
                violations.push(format!("line {line_num}: {preview:?}"));
            }
        }

        byte_offset += rel_pos + 1;
        remaining = &remaining[rel_pos + 1..];
    }

    violations
}

/// Recursively walk `workspace_root` collecting every `.rs` file whose path
/// contains a directory component named `tests`. Skips `target`, `.git`,
/// `.worktrees`, and any directory whose name starts with `.`.
///
/// Uses `std::fs::read_dir` with an explicit stack (no recursion, no external
/// `walkdir` dep) — matching the existing convention in `reify-kernel-occt/build.rs`.
///
/// # Panics
///
/// Does not panic — I/O errors on individual directories are silently skipped.
pub fn walk_test_rs_files(workspace_root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![workspace_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                // Skip build artifacts, git internals, worktrees, and all dot-dirs.
                if name == "target" || name.starts_with('.') {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                // Only include files that have "tests" as a directory component
                // in the path relative to workspace_root.
                if has_tests_component(&path, workspace_root) {
                    result.push(path);
                }
            }
        }
    }

    result
}

/// Returns true when `path` (relative to `workspace_root`) contains at least
/// one directory component whose name is exactly `"tests"`.
fn has_tests_component(path: &Path, workspace_root: &Path) -> bool {
    let rel = match path.strip_prefix(workspace_root) {
        Ok(r) => r,
        Err(_) => return false,
    };
    // Iterate directory components only (skip the final filename component).
    rel.parent()
        .unwrap_or(Path::new(""))
        .components()
        .any(|c| matches!(c, std::path::Component::Normal(name) if name == "tests"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_stale_plan_pointers_in_source ────────────────────────────────────

    /// (a) Empty source → empty Vec.
    #[test]
    fn fspp_empty_source_is_clean() {
        assert!(find_stale_plan_pointers_in_source("").is_empty());
    }

    /// (b) Source with one stale pointer → exactly one violation containing
    /// a preview of the offending reason.
    /// Marker and needle assembled at runtime so this file does not contain
    /// the literal substrings and does not self-trigger the workspace guard.
    #[test]
    fn fspp_one_stale_pointer_returns_one_violation_with_preview() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let source = format!("{marker}{needle}3 reference\"]");
        let violations = find_stale_plan_pointers_in_source(&source);
        assert_eq!(
            violations.len(),
            1,
            "expected exactly one violation, got: {violations:?}"
        );
        let expected_fragment = format!("{needle}3 reference");
        assert!(
            violations[0].contains(&expected_fragment),
            "violation {:?} should contain the reason preview {:?}",
            violations[0],
            expected_fragment
        );
    }

    /// (c) Two #[ignore] attrs, only one with a stale pointer → one violation.
    #[test]
    fn fspp_two_ignores_only_one_stale_returns_one_violation() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let source = format!(
            "{marker}{needle}7 reference\"]\n{marker}known bug: valid reason\"]"
        );
        let violations = find_stale_plan_pointers_in_source(&source);
        assert_eq!(
            violations.len(),
            1,
            "expected one violation, got: {violations:?}"
        );
    }

    /// (d) `#[ignore = "known bug: ..."]` with no stale pointer → clean.
    #[test]
    fn fspp_known_bug_reason_is_clean() {
        let marker = ["#[ignore", " = \""].concat();
        let source = format!("{marker}known bug: dimension returns wrong type\"]");
        assert!(
            find_stale_plan_pointers_in_source(&source).is_empty(),
            "expected no violations for a known-bug reason"
        );
    }

    /// (e) `#[ignore]` with no reason string → clean.
    /// Assembled at runtime so the literal marker is not present in this file.
    #[test]
    fn fspp_bare_ignore_without_reason_is_clean() {
        // Builds "#[ignore]" without putting the full marker literal in this file.
        let bare = ["#[ignore", "]"].concat();
        let source = format!("{bare}\nfn some_test() {{}}");
        assert!(
            find_stale_plan_pointers_in_source(&source).is_empty(),
            "expected no violations for bare #[ignore] with no reason string"
        );
    }

    // ── walk_test_rs_files ────────────────────────────────────────────────────

    /// Build a synthetic workspace tree using tempfile::tempdir() and verify that
    /// walk_test_rs_files returns exactly the files whose path contains a `tests`
    /// directory component, skipping `target`, dot-dirs, and `src/` files.
    #[test]
    fn walk_includes_tests_dir_files_and_excludes_others() {
        use tempfile::TempDir;

        let tmp: TempDir = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();

        // Files that SHOULD be returned (path has "tests" component)
        let included = [
            "crates/foo/tests/bar.rs",
            "crates/foo/src/tests/inner.rs",
            "tree-sitter-reify/tests/build.rs",
        ];
        // Files that should NOT be returned
        let excluded = [
            "crates/foo/src/lib.rs",       // no "tests" component
            "target/tests/skipme.rs",       // "target" dir excluded
            ".git/tests/skip.rs",           // dot-dir excluded
        ];

        for rel in included.iter().chain(excluded.iter()) {
            let full = root.join(rel);
            std::fs::create_dir_all(full.parent().unwrap())
                .expect("create parent dirs");
            std::fs::write(&full, b"// synthetic\n").expect("write file");
        }

        let found = walk_test_rs_files(root);

        // Convert to relative paths for easy comparison
        let found_rel: std::collections::HashSet<String> = found
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/")
            })
            .collect();

        for rel in &included {
            assert!(
                found_rel.contains(*rel),
                "expected {rel:?} to be returned by walk_test_rs_files, got: {found_rel:?}"
            );
        }
        for rel in &excluded {
            assert!(
                !found_rel.contains(*rel),
                "expected {rel:?} to be excluded by walk_test_rs_files, got: {found_rel:?}"
            );
        }
    }
}
