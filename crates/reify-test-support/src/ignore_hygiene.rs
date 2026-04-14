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
pub fn walk_test_rs_files(_workspace_root: &Path) -> Vec<PathBuf> {
    unimplemented!()
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
}
