use std::path::{Path, PathBuf};

/// Returns `true` when `line` is an outer (`///`) or inner (`//!`) doc-comment
/// line, after stripping leading whitespace.  Regular `//` line comments and
/// `/* ... */` block comments return `false` — only `///` and `//!` are skipped
/// by both scanners.  Note that `////` (four or more slashes) also returns
/// `true` due to `starts_with("///")` semantics, preserving the existing
/// behavior from the original field-local scanner.
fn is_doc_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("///") || trimmed.starts_with("//!")
}

/// Extract the reason string from a single `#[ignore = "..."]` attribute line.
///
/// Returns `Some(reason)` for the canonical rustfmt form
/// `#[ignore = "reason"]` (space-equals-space-quote), where `reason` is the
/// slice between the opening `"` and the next raw `"` byte. Returns `None`
/// for:
/// - bare `#[ignore]` attributes (no reason string)
/// - non-`#[ignore]` lines
/// - `///` and `//!` doc-comment lines (prose mentions of the attribute)
/// - non-canonical forms (e.g. no spaces around `=`)
///
/// **Note:** escaped quotes (`\"`) inside the reason are not handled — the
/// reason is truncated at the first raw `"`. No current source file uses
/// escaped quotes inside `#[ignore]` reasons.
///
/// This is the line-level reason extractor shared with the PTODO detector
/// (§8.3 γ lane). It formalises the format-vs-liveness split: `reify-
/// test-support` owns extraction + format checks (`check_ignore_reasons`);
/// PTODO owns citation-liveness (`has_canonical_cite`/`resolve_liveness`).
pub fn extract_ignore_reason(line: &str) -> Option<&str> {
    // Skip doc-comment lines (`///`, `//!`) — prose mentions of the attribute
    // inside doc comments must not fire, mirroring `check_ignore_reasons`.
    if is_doc_comment_line(line) {
        return None;
    }
    // Recognise only the canonical rustfmt form: trim leading whitespace, then
    // match the `#[ignore = "` prefix exactly (space-equals-space-quote).
    // Non-canonical forms (no spaces around `=`) are silently ignored.
    let rest = line.trim_start().strip_prefix("#[ignore = \"")?;
    // The reason is everything up to the first raw `"` (escaped-quote
    // limitation documented in the function doc comment above).
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Scan `source` for `#[ignore = "..."]` reason strings that contain a stale
/// transient-plan-doc pointer (e.g. a `plan step-N` breadcrumb). Returns one
/// human-readable violation string per offender. Empty Vec means clean.
///
/// The marker and needle are assembled at runtime so this source file does not
/// contain the literal substrings and does not self-trigger when scanned.
///
/// Note: reason strings containing escaped quotes (`\"`) are not handled —
/// the captured reason is truncated at the first raw `"` byte. No current test
/// file uses escaped quotes inside `#[ignore]` reasons.
pub fn find_stale_plan_pointers_in_source(source: &str) -> Vec<String> {
    // Assembled at runtime — do not inline these as literals.
    let marker = ["#[ignore", " = \""].concat();
    let needle = ["plan", " step-"].concat();

    let mut violations = Vec::new();
    let mut remaining = source;
    let mut byte_offset: usize = 0;

    while let Some(rel_pos) = remaining.find(marker.as_str()) {
        let abs_pos = byte_offset + rel_pos;

        // Locate the bounds of the line that contains the marker, then check
        // whether it is a doc-comment line.  The check is on the MARKER's line,
        // not on any `\`-continuation lines, so multi-line reason support is
        // preserved (test g).
        //
        // line_start is computed first via rfind (one O(abs_pos) scan);
        // line_num is then derived from source[..line_start] (a strictly
        // smaller scan, O(line_start) ≤ O(abs_pos)) to avoid a redundant
        // full-range scan that would make the inner loop O(M·N) for M markers.
        let line_start = source[..abs_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_num = source[..line_start].bytes().filter(|&b| b == b'\n').count() + 1;
        let line_end = source[abs_pos..]
            .find('\n')
            .map(|p| abs_pos + p)
            .unwrap_or(source.len());
        let containing_line = &source[line_start..line_end];

        // Advance past the marker unconditionally (whether we record a
        // violation or not) so the byte-offset bookkeeping stays correct.
        byte_offset += rel_pos + marker.len();
        remaining = &remaining[rel_pos + marker.len()..];

        if is_doc_comment_line(containing_line) {
            // Marker lives inside a doc-comment line — skip silently.
            continue;
        }

        let after_marker = &source[abs_pos + marker.len()..];
        if let Some(end) = after_marker.find('"') {
            let reason = &after_marker[..end];
            if reason.contains(needle.as_str()) {
                let preview: String = reason.chars().take(80).collect();
                violations.push(format!("line {line_num}: {preview:?}"));
            }
        }
    }

    violations
}

/// Scans `source` (a Rust source file as a string) and verifies every
/// `#[ignore = "..."]` attribute complies with the Task 1622 convention.
///
/// Three guards are applied in order:
///
/// 1. **Bare-ignore rejection** — a `#[ignore]` attribute without a reason
///    string is rejected outright.
/// 2. **Positive invariant** — every reason string must begin with
///    `"known bug:"`.  This rejects wholly-replaced prefixes but does NOT
///    catch stale wordings *appended inside* an otherwise-compliant prefix
///    (e.g. `"known bug: see plan.md step-3"` would pass guard 2 and would
///    only trip guard 3 if it happened to contain the specific sentinel).
/// 3. **Negative sentinel** — guard 3 delegates to
///    `find_stale_plan_pointers_in_source` so stale-needle detection —
///    including multi-line reason support and `///`/`//!` doc-comment
///    skipping — has a single source of truth across both public scanners.
///    Any future scanner improvement (e.g. skipping `/* */` block comments)
///    benefits both call sites automatically.
///
///    Note: guard 3 only catches stale needles that appear *inside* an
///    `#[ignore = "..."]` reason string.  Occurrences in bare prose outside
///    any attribute are no longer caught here; guards 1+2 cover every
///    real-world case because the needle's space-dash boundary cannot appear
///    in a Rust identifier (underscores are the convention), and grep of the
///    current workspace confirms zero bare-prose occurrences.
///
/// Lines where `is_doc_comment_line` returns true (`///` or `//!`, after
/// `trim_start`) are skipped by guards 1+2 directly, and by guard 3
/// transitively through `find_stale_plan_pointers_in_source` — prose
/// mentions of `#[ignore]` in doc comments do not generate false positives.
///
/// All scanner constants are assembled at runtime via `.concat()` so the
/// source file does not contain the guarded sequences as adjacent characters.
///
/// **Rustfmt assumption:** the scanner recognises only the canonical
/// `#[ignore = "..."]` form (space-equals-space-quote).  Non-canonical
/// forms such as `#[ignore="..."]` are silently ignored; rustfmt
/// enforces the canonical form in practice.
pub fn check_ignore_reasons(source: &str) -> Result<(), String> {
    // DO NOT inline — split at boundary ["#[", "ignore"] keeps the guarded
    // 8-char sequence from appearing adjacent in this source file; inlining
    // would cause the scanner to self-trigger on this very line.
    let ignore_prefix = ["#[", "ignore"].concat();

    for (line_idx, line) in source.lines().enumerate() {
        // Skip doc-comment lines (`///`, `//!`) — they may mention the
        // bare-ignore form in prose without it being an actual attribute.
        // NOTE: regular `//` line comments and `/* */` block comments are NOT
        // skipped.  The file currently has no bare-ignore forms in regular
        // comments; if a future bare-ignore example inside a `//` comment
        // causes a spurious meta-test failure, rewrite it as a `///` doc
        // comment instead (doc comments ARE skipped).
        if is_doc_comment_line(line) {
            continue;
        }

        let mut rest = line;
        while let Some(pos) = rest.find(ignore_prefix.as_str()) {
            let after = &rest[pos + ignore_prefix.len()..];
            if after.starts_with(']') {
                return Err(format!(
                    "bare {} attribute found at line {} \
                     — reason string required (Task 1622/1641 convention): {line:?}",
                    ["#[", "ignore]"].concat(),
                    line_idx + 1,
                ));
            } else if let Some(reason_start) = after.strip_prefix(" = \"") {
                let preview: String = reason_start.chars().take(80).collect();
                if !reason_start.starts_with("known bug:") {
                    return Err(format!(
                        "An {} reason string at line {} does not begin with \
                         \"known bug:\":\n  {preview:?}\nReason strings must be \
                         self-contained inline summaries (Task 1622 convention).",
                        ["#[", "ignore]"].concat(),
                        line_idx + 1,
                    ));
                }
            }
            // Neither `]` nor ` = "` — advance past this match and continue
            // (e.g. the guarded sequence inside a string literal in source code).
            rest = &rest[pos + 1..];
        }
    }

    // Guard 3: delegate stale-pointer detection to find_stale_plan_pointers_in_source.
    // This gives guard 3 the same byte-level marker walk, multi-line-reason support,
    // and `///`/`//!` doc-comment skipping as the standalone scanner — a single source
    // of truth for stale-needle detection.  Note: guard 3 now only catches stale
    // needles that appear *inside* an `#[ignore = "..."]` reason string; bare-prose
    // occurrences outside any attribute are no longer caught here (guards 1+2 handle
    // every real-world case).
    let stale_violations = find_stale_plan_pointers_in_source(source);
    if !stale_violations.is_empty() {
        let violation_list = stale_violations.join("\n  ");
        return Err(format!(
            "Found stale-pointer substring in {} reason string(s). \
             Update affected reason strings to self-contained inline summaries \
             (Task 1622 convention):\n  {violation_list}",
            ["#[", "ignore]"].concat(),
        ));
    }

    Ok(())
}

/// Recursively walk `workspace_root` collecting every `.rs` file whose path
/// contains a directory component named `tests`. Skips the following directories:
///
/// - `target` — Cargo build artifacts
/// - Dot-dirs (any name starting with `.`) — VCS internals (`.git`), worktrees
///   (`.worktrees`), and similar tooling directories
/// - `node_modules` — JS/Node tooling output; `tree-sitter-reify/` can acquire
///   this directory from JS build steps, and `.rs` files inside it would produce
///   false matches
/// - `vendor` — Cargo vendored dependency trees; vendored crate source is not
///   project-owned test code and scanning it wastes time
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
            // Use entry.file_type() (does not follow symlinks) to avoid
            // infinite loops on symlink cycles in the workspace.
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Skip build artifacts, VCS/worktree noise, JS tooling, and vendored deps.
                if name == "target"
                    || name.starts_with('.')
                    || name == "node_modules"
                    || name == "vendor"
                {
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

/// Walk `workspace_root` for test `.rs` files and collect every stale
/// transient-plan-doc pointer found in `#[ignore]` reason strings.
///
/// Combines `walk_test_rs_files` + `find_stale_plan_pointers_in_source` into a
/// single reusable helper so callers (integration tests, CI scripts) don't need
/// to inline the walk-read-detect loop.
///
/// Each returned `String` has the form `"<relative/path>: <violation>"`, where
/// the relative path is stripped of `workspace_root` as a prefix and the
/// violation is produced by `find_stale_plan_pointers_in_source` (e.g.
/// `"line 12: \"known bug: see plan step-3\""`).
///
/// I/O errors on individual files are silently skipped — the same policy as
/// the integration test — so a file deleted mid-walk during a concurrent build
/// does not produce a spurious violation.
pub fn collect_workspace_stale_pointers(workspace_root: &Path) -> Vec<String> {
    walk_test_rs_files(workspace_root)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok().map(|s| (path, s)))
        .flat_map(|(path, source)| {
            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or(path)
                .display()
                .to_string();
            find_stale_plan_pointers_in_source(&source)
                .into_iter()
                .map(move |v| format!("{rel}: {v}"))
                .collect::<Vec<_>>()
        })
        .collect()
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
        let source = format!("{marker}{needle}7 reference\"]\n{marker}known bug: valid reason\"]");
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

    /// (f) Multi-line `#[ignore]` reason (raw `\` + newline continuation as it
    /// appears literally in source files) with no stale pointer → clean.
    /// Locks in the assumption that the function operates on raw bytes and
    /// correctly finds the closing `"` past the continuation character.
    #[test]
    fn fspp_multiline_reason_without_stale_pointer_is_clean() {
        let marker = ["#[ignore", " = \""].concat();
        // Raw source contains literal `\` + newline, as written in .rs files.
        let source = format!("{marker}known bug: first line, \\\nsecond line\"]");
        assert!(
            find_stale_plan_pointers_in_source(&source).is_empty(),
            "expected no violations for a multi-line known-bug reason"
        );
    }

    /// (g) Multi-line `#[ignore]` reason where the stale needle appears on the
    /// second line (after the `\` + newline continuation) → one violation.
    /// Verifies that the function scans raw bytes across the line break.
    #[test]
    fn fspp_multiline_reason_with_stale_pointer_on_second_line_returns_violation() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        // Raw source: `#[ignore = "known bug: see \<newline>plan step-3\"]`
        let source = format!("{marker}known bug: see \\\n{needle}3 reference\"]");
        let violations = find_stale_plan_pointers_in_source(&source);
        assert_eq!(
            violations.len(),
            1,
            "expected exactly one violation for stale pointer on second line, got: {violations:?}"
        );
        let expected_fragment = format!("{needle}3 reference");
        assert!(
            violations[0].contains(&expected_fragment),
            "violation {:?} should contain the needle fragment {:?}",
            violations[0],
            expected_fragment
        );
        assert!(
            violations[0].starts_with("line 1:"),
            "expected violation to report the attribute line (line 1), not the needle line (line 2): {:?}",
            violations[0]
        );
    }

    // ── find_stale_plan_pointers_in_source — doc-comment skipping ────────────

    /// (h) A `///` outer-doc-comment line containing the marker AND the stale
    /// needle must produce zero violations — doc-comment lines are skipped.
    #[test]
    fn fspp_skips_outer_doc_comment_marker_with_stale_needle() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let src = format!("/// {marker}{needle}3 reference\"]\n");
        let violations = find_stale_plan_pointers_in_source(&src);
        assert!(
            violations.is_empty(),
            "expected marker on a /// line to be skipped, got: {violations:?}"
        );
    }

    /// (i) A `//!` inner-doc-comment line containing the marker AND the stale
    /// needle must produce zero violations — pins the `//!` arm separately.
    #[test]
    fn fspp_skips_inner_doc_comment_marker_with_stale_needle() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let src = format!("//! {marker}{needle}3 reference\"]\n");
        let violations = find_stale_plan_pointers_in_source(&src);
        assert!(
            violations.is_empty(),
            "expected marker on a //! line to be skipped, got: {violations:?}"
        );
    }

    /// (j) A `///` line with leading whitespace before the `///` must still be
    /// detected as a doc-comment line (verifies `trim_start()` semantics).
    #[test]
    fn fspp_skips_indented_doc_comment_marker_with_stale_needle() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let src = format!("    /// {marker}{needle}3 reference\"]\n");
        let violations = find_stale_plan_pointers_in_source(&src);
        assert!(
            violations.is_empty(),
            "expected marker on an indented /// line to be skipped, got: {violations:?}"
        );
    }

    /// (k) A regular `//` line (NOT `///` or `//!`) containing the marker AND
    /// the stale needle must STILL produce a violation.  Locks the documented
    /// limitation that only `///` and `//!` are skipped, not plain `//`.
    #[test]
    fn fspp_does_not_skip_regular_double_slash_comment_with_marker_and_needle() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let src = format!("// {marker}{needle}3 reference\"]\n");
        let violations = find_stale_plan_pointers_in_source(&src);
        assert_eq!(
            violations.len(),
            1,
            "expected marker on a // (non-doc) comment line to NOT be skipped, got: {violations:?}"
        );
    }

    // ── lock-step agreement between both scanners ─────────────────────────────

    /// Lock-step test: both `find_stale_plan_pointers_in_source` and
    /// `check_ignore_reasons` must agree that a `///` doc-comment line
    /// containing the marker + stale needle produces zero violations.
    /// This test fails to compile until `check_ignore_reasons` is promoted
    /// to this module — that compile failure is the expected RED state.
    #[test]
    fn lock_step_doc_comment_skipping_in_both_scanners() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let src = format!("/// {marker}{needle}3 reference\"]\n");
        let fspp_result = find_stale_plan_pointers_in_source(&src);
        let cir_result = check_ignore_reasons(&src);
        assert!(
            fspp_result.is_empty() && cir_result.is_ok(),
            "both scanners must agree on doc-comment skipping: \
             fspp={fspp_result:?}, cir={cir_result:?}"
        );
    }

    // ── check_ignore_reasons unit tests ──────────────────────────────────────
    // These tests exercise the check_ignore_reasons helper promoted from
    // field_calculus_tests.rs (Task 1659).  All synthetic source strings use
    // runtime concatenation so this file does not contain the literal adjacent
    // sequences that the meta-test guards against.

    #[test]
    fn check_ignore_reasons_rejects_bare_ignore_in_code() {
        // A bare-ignore attribute (no reason string) in non-comment code must be
        // rejected outright.  Source assembled at runtime so this file does not
        // contain the guarded adjacent sequences.  The error must mention "bare"
        // so we can tell the bare-ignore guard fired (not the positive-invariant
        // guard or the stale-pointer sentinel).
        let src = ["#[", "ignore]\n#[test]\nfn foo() {}"].concat();
        let err = check_ignore_reasons(&src)
            .expect_err("expected bare-ignore attribute (no reason string) to be rejected");
        assert!(
            err.contains("bare"),
            "expected error message to identify the bare-ignore guard: {err:?}",
        );
    }

    #[test]
    fn check_ignore_reasons_allows_bare_ignore_mentioned_in_doc_comments() {
        // A bare-ignore form that appears only inside a `///` doc-comment line
        // must NOT trigger a rejection — doc-comment lines are skipped.
        // Source assembled at runtime so this file does not contain the guarded
        // adjacent sequences.
        let src = ["/// example: ", "#[", "ignore] is load-bearing\n"].concat();
        assert!(
            check_ignore_reasons(&src).is_ok(),
            "expected bare-ignore inside a doc comment to be allowed",
        );
    }

    #[test]
    fn check_ignore_reasons_accepts_compliant_source() {
        // A reason string beginning with "known bug:" complies with the Task 1622
        // convention and must be accepted.  Source assembled at runtime.
        let src = ["#[", "ignore = \"known bug: placeholder summary\"]"].concat();
        assert!(
            check_ignore_reasons(&src).is_ok(),
            "expected a compliant \"known bug:\" reason string to be accepted",
        );
    }

    #[test]
    fn check_ignore_reasons_rejects_non_known_bug_reason() {
        // A reason string that does NOT begin with "known bug:" must be rejected
        // by the positive-invariant guard (guard 2).  The reason deliberately does
        // not contain the stale-pointer needle so only the positive guard fires.
        // The error must mention "does not begin with" — a substring unique to
        // guard 2's error format — to confirm the positive-invariant guard fired
        // (not the bare-ignore guard or the stale-pointer sentinel).
        // Source assembled at runtime.
        let src = ["#[", "ignore = \"some other prefix here\"]"].concat();
        let err = check_ignore_reasons(&src)
            .expect_err("expected a non-\"known bug:\" reason string to be rejected");
        assert!(
            err.contains("does not begin with"),
            "expected error to identify the positive-invariant guard via 'does not begin with': {err:?}",
        );
    }

    #[test]
    fn check_ignore_reasons_rejects_stale_plan_step_needle() {
        // An `#[ignore]` reason string containing the stale-pointer needle
        // (assembled at runtime) must be rejected by the negative sentinel.
        // The input uses a proper `#[ignore = "known bug: ..."]` attribute so
        // find_stale_plan_pointers_in_source (now powering guard 3) detects it.
        // The error must mention "stale-pointer" so we can tell guard 3 fired
        // (not the bare-ignore guard or the positive-invariant guard).
        // Marker, needle, and source assembled at runtime so this file does not
        // contain the literal adjacent sequences.
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let src = format!("{marker}known bug: see {needle}5 reference\"]");
        let err = check_ignore_reasons(&src)
            .expect_err("expected source containing the stale-pointer needle to be rejected");
        assert!(
            err.contains("stale-pointer"),
            "expected error message to identify the negative-sentinel guard: {err:?}",
        );
    }

    #[test]
    fn check_ignore_reasons_allows_bare_ignore_in_inner_doc_comment() {
        // A bare-ignore form that appears only inside a `//!` inner-doc-comment
        // line must NOT trigger a rejection — `//!` lines are skipped alongside
        // `///` lines.  This complements the outer-doc-comment test above and
        // pins the `//!` branch of the skip logic separately so neither branch
        // can be removed silently.  Source assembled at runtime.
        let src = ["//! example: ", "#[", "ignore] is load-bearing\n"].concat();
        assert!(
            check_ignore_reasons(&src).is_ok(),
            "expected bare-ignore inside an inner doc comment (//!) to be allowed",
        );
    }

    #[test]
    fn check_ignore_reasons_regular_comment_is_not_skipped() {
        // Regular `//` line comments are NOT skipped — only `///` and `//!` are.
        // This pins the documented limitation so a future refactor that
        // accidentally starts skipping `//` is caught immediately.
        // The error must mention "bare" to confirm guard 1 (bare-ignore rejection)
        // fired — not a different guard — pinning which guard rejects the `//` line.
        // Source assembled at runtime to avoid self-triggering.
        let src = ["// regular comment: ", "#[", "ignore]\n"].concat();
        let err = check_ignore_reasons(&src).expect_err(
            "expected bare-ignore inside a regular // comment to be rejected \
                         (// comments are not skipped; only /// and //! are)",
        );
        assert!(
            err.contains("bare"),
            "expected the bare-ignore guard (guard 1) to fire, not a different guard: {err:?}",
        );
    }

    #[test]
    fn check_ignore_reasons_accepts_source_with_no_ignore_attributes() {
        // A source string containing no ignore attributes at all must be accepted,
        // pinning the empty-input / no-match contract.  If the loop logic ever
        // regressed (e.g. find always returning Some), this test would catch it.
        let src = "fn main() {}\nstruct Foo;\nfn bar() -> i32 { 42 }\n";
        assert!(
            check_ignore_reasons(src).is_ok(),
            "expected source with no ignore attributes to be accepted",
        );
    }

    #[test]
    fn check_ignore_reasons_non_canonical_form_is_silently_ignored() {
        // The scanner recognises only the canonical rustfmt form (space-equals-space-quote:
        // ` = "`).  Non-canonical attribute forms — no spaces around `=`, space only
        // before `=`, or space only after `=` — do not match any guard branch and are
        // silently passed over.  This test pins all three whitespace variants so any
        // future tightening (e.g. stripping whitespace before matching) must first
        // update this test.
        //
        // The reason string deliberately does NOT start with `known bug:`.  If a future
        // refactor starts matching non-canonical forms, guard 2 would reject this reason
        // and the test would fail loudly, providing a clear regression signal.
        //
        // Sources assembled at runtime so this file does not contain the guarded adjacent
        // sequences and does not self-trigger the meta-test scanner.
        let non_canonical_forms = [
            ["#[", "ignore=\"not a known bug: test\"]"].concat(), // no spaces around =
            ["#[", "ignore =\"not a known bug: test\"]"].concat(), // space before = only
            ["#[", "ignore= \"not a known bug: test\"]"].concat(), // space after = only
        ];
        for src in &non_canonical_forms {
            assert!(
                check_ignore_reasons(src).is_ok(),
                "expected non-canonical #[ignore...] form to be silently ignored: {src:?}",
            );
        }
    }

    /// Guard 3 "no-marker" path: a `///` doc-comment line that contains the
    /// stale needle but no `#[ignore]` marker must be accepted.  After the
    /// delegation refactor, guard 3 calls `find_stale_plan_pointers_in_source`,
    /// which only walks `#[ignore = "..."]` reason strings — so a bare doc-comment
    /// line without a marker is not flagged because there is simply no marker to
    /// scan, not because of guard-3-specific doc-comment filtering.
    ///
    /// Doc-comment skipping with a marker present is covered by the lock-step
    /// test `lock_step_doc_comment_skipping_in_both_scanners` (see above).
    #[test]
    fn check_ignore_reasons_guard3_skips_doc_comment_lines() {
        let needle = ["plan", " step-"].concat();
        // Source has no #[ignore] marker — only a doc-comment line with the needle.
        // find_stale_plan_pointers_in_source finds no markers, so guard 3 passes.
        let src = format!("/// contains needle: {needle}3\n");
        assert!(
            check_ignore_reasons(&src).is_ok(),
            "guard 3 should accept a source with no #[ignore] markers (needle only in a doc-comment)",
        );
    }

    // ── check_ignore_reasons guard-3 delegation pinning tests ────────────────

    /// Pins that guard 3 cites the line number of the offending `#[ignore]`
    /// attribute in its error message.  Written RED before the delegation to
    /// `find_stale_plan_pointers_in_source` (which carries line-number
    /// information), now green and kept as a regression pin.
    ///
    /// Source: a compliant `known bug:` prefix that also contains the stale
    /// needle, placed on line 3 (after two blank lines).
    #[test]
    fn check_ignore_reasons_guard3_error_cites_line_number() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        // Two blank lines place the attribute on line 3.
        let src = format!("\n\n{marker}known bug: see {needle}3 reference\"]");
        let err = check_ignore_reasons(&src)
            .expect_err("expected stale-pointer in #[ignore] reason to be rejected");
        assert!(
            err.contains("line 3"),
            "expected error to cite the offending line number (\"line 3\"): {err:?}"
        );
    }

    /// Pins that guard 3 error messages include the same violation preview
    /// produced by `find_stale_plan_pointers_in_source`, confirming the two
    /// are sourced from the same scanner.  Written RED when guard 3 used a
    /// line-level scan whose static error contained no preview; now green and
    /// kept as a regression pin against future divergence.
    ///
    /// Uses the test-(g) fixture: `\` + newline continuation with the stale
    /// needle on the second line — a cross-line case the old scan missed.
    #[test]
    fn check_ignore_reasons_delegates_guard3_to_find_stale_plan_pointers() {
        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        // `\` + newline continuation; stale needle appears on the second line.
        let src = format!("{marker}known bug: see \\\n{needle}3 reference\"]");

        // (a) find_stale_plan_pointers_in_source must detect exactly one violation.
        let violations = find_stale_plan_pointers_in_source(&src);
        assert_eq!(
            violations.len(),
            1,
            "find_stale_plan_pointers_in_source should detect one violation: {violations:?}"
        );

        // (b) check_ignore_reasons must reject the source AND its error message
        // must contain the violation preview produced by
        // find_stale_plan_pointers_in_source.  This pins that guard 3 is
        // sourced from the same scanner.
        let err = check_ignore_reasons(&src)
            .expect_err("expected stale-pointer in #[ignore] reason to be rejected");
        assert!(
            err.contains(&violations[0]),
            "expected check_ignore_reasons error to contain the violation preview {:?}: {err:?}",
            violations[0]
        );
    }

    // ── collect_workspace_stale_pointers ─────────────────────────────────────

    /// (a) Synthetic workspace with stale plan pointers in two different crate
    /// test dirs → collect_workspace_stale_pointers returns violations that cite
    /// both files.  Marker and needle assembled at runtime so this source file
    /// does not contain the literal guarded sequences.
    #[test]
    fn cwsp_synthetic_workspace_with_two_stale_files_returns_both_violations() {
        use tempfile::TempDir;

        let tmp: TempDir = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();

        let marker = ["#[ignore", " = \""].concat();
        let needle = ["plan", " step-"].concat();
        let stale_source = format!("{marker}known bug: see {needle}3 reference\"]");
        let good_source = format!("{marker}known bug: a clean summary\"]");

        // Two test files with stale pointers in different crates
        let stale_a = root.join("crates/alpha/tests/foo.rs");
        let stale_b = root.join("crates/beta/tests/bar.rs");
        // A clean file that should produce zero violations
        let clean = root.join("crates/gamma/tests/baz.rs");

        for (path, content) in [
            (&stale_a, stale_source.as_str()),
            (&stale_b, stale_source.as_str()),
            (&clean, good_source.as_str()),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dirs");
            std::fs::write(path, content.as_bytes()).expect("write file");
        }

        let violations = collect_workspace_stale_pointers(root);

        assert_eq!(
            violations.len(),
            2,
            "expected exactly 2 violations (one per stale file), got: {violations:?}"
        );
        // Each violation must include a relative path fragment for its file.
        let combined = violations.join("\n");
        assert!(
            combined.contains("crates/alpha/tests/foo.rs")
                || combined.contains("alpha/tests/foo.rs"),
            "expected violation to mention the alpha test file: {violations:?}"
        );
        assert!(
            combined.contains("crates/beta/tests/bar.rs") || combined.contains("beta/tests/bar.rs"),
            "expected violation to mention the beta test file: {violations:?}"
        );
    }

    /// (b) Synthetic workspace with only compliant `#[ignore]` reasons →
    /// collect_workspace_stale_pointers returns an empty Vec.  Marker assembled
    /// at runtime so this source file does not self-trigger.
    #[test]
    fn cwsp_clean_synthetic_workspace_returns_empty_vec() {
        use tempfile::TempDir;

        let tmp: TempDir = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();

        let marker = ["#[ignore", " = \""].concat();
        let clean_source = format!("{marker}known bug: a clean self-contained summary\"]");

        let test_file = root.join("crates/foo/tests/clean.rs");
        std::fs::create_dir_all(test_file.parent().unwrap()).expect("create parent dirs");
        std::fs::write(&test_file, clean_source.as_bytes()).expect("write file");

        let violations = collect_workspace_stale_pointers(root);

        assert!(
            violations.is_empty(),
            "expected no violations for a clean workspace, got: {violations:?}"
        );
    }

    // ── extract_ignore_reason ─────────────────────────────────────────────────

    // Tests assembled using runtime concat so this source file does not contain
    // the literal `#[ignore` substring and does not self-trigger the workspace
    // guard or check_ignore_reasons.

    /// (a) Canonical `#[ignore = "reason text"]` → Some("reason text").
    #[test]
    fn eir_canonical_form_returns_reason() {
        // Assembled at runtime so this file does not self-trigger.
        let line = ["#[ignore", " = \"reason text\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("reason text"));
    }

    /// (b) Leading-whitespace/indented form → Some(...).
    #[test]
    fn eir_indented_form_returns_reason() {
        let line = ["    #[ignore", " = \"indented reason\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("indented reason"));
    }

    /// (c) Reason carrying a cite → Some("see #42").
    #[test]
    fn eir_reason_with_cite_returns_reason() {
        let line = ["#[ignore", " = \"see #42\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("see #42"));
    }

    /// (d) Bare `#[ignore]` (no reason string) → None.
    #[test]
    fn eir_bare_ignore_returns_none() {
        let line = ["#[ignore", "]"].concat();
        assert_eq!(extract_ignore_reason(&line), None);
    }

    /// (e) A non-ignore line → None.
    #[test]
    fn eir_non_ignore_line_returns_none() {
        assert_eq!(extract_ignore_reason("fn some_test() {}"), None);
    }

    /// (f) A `///` outer doc-comment line mentioning the attribute in prose → None.
    #[test]
    fn eir_outer_doc_comment_returns_none() {
        let line = ["/// example: #[ignore", " = \"r\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), None);
    }

    /// (g) A `//!` inner doc-comment line mentioning the attribute in prose → None.
    #[test]
    fn eir_inner_doc_comment_returns_none() {
        let line = ["//! example: #[ignore", " = \"r\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), None);
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
            "crates/foo/src/lib.rs",                  // no "tests" component
            "target/tests/skipme.rs",                 // root-level "target" dir excluded
            "crates/foo/target/tests/skip_nested.rs", // nested "target" dir also excluded
            ".git/tests/skip.rs",                     // dot-dir excluded
            "node_modules/tests/skip_nm.rs",          // node_modules dir excluded
            "vendor/tests/skip_vendor.rs",            // vendor dir excluded
        ];

        for rel in included.iter().chain(excluded.iter()) {
            let full = root.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).expect("create parent dirs");
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
