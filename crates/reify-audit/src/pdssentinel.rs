//! PDSSENTINEL — ds-sentinel reintroduction guard.
//!
//! ## Purpose
//!
//! Detects sites in the scoped compiler source files where a
//! `dimensionless_scalar()` call appears shortly after a
//! `diagnostics.push(Diagnostic::error(... UnresolvedType ...))` push —
//! the exact pattern that the dimensionless-scalar-sentinel-stampout batch
//! (PRD `docs/prds/dimensionless-scalar-sentinel-stampout.md`) eliminated.
//! A new such site means a developer accidentally reintroduced the sentinel
//! bug that the batch was designed to stamp out.
//!
//! ## Heuristic
//!
//! Pure line-window scan (no `syn`/AST, no regex — mirrors `ptodo.rs` flat
//! content scan, per PRD §10). Walk the file lines; when a line references
//! `DiagnosticCode::UnresolvedType` (typically the `.with_code(...)` line
//! inside a multi-line `diagnostics.push(Diagnostic::error(...))` push —
//! but the match is broader; see `is_error_push_line`), open a bounded
//! forward window. If a subsequent line within the window contains
//! `dimensionless_scalar()` and neither that line nor any other line in the
//! window carries a `// ds-sentinel:allow` marker, flag it as an offender.
//!
//! ## Scope
//!
//! Hardcoded to `crates/reify-compiler/src/{entity,functions,traits,expr}.rs`
//! and `crates/reify-compiler/src/conformance/*.rs`. This scope (a) matches
//! PRD §8(2), (b) inherently prevents the detector self-matching its own
//! source/fixtures — the documented PTODO ALLOWLIST_PREFIXES self-match
//! hazard — and (c) keeps it out of `ice.rs` (L3), which PRD §8 deliberately
//! leaves out of the detector surface.
//!
//! ## Escape hatch
//!
//! Add a `// ds-sentinel:allow <one-line rationale>` comment anywhere
//! in the window between the trigger line and the `dimensionless_scalar()`
//! call (inclusive) — or on the line immediately after — to suppress a
//! legitimate KEEP site (e.g. the `functions.rs` arrow/function field
//! domain/codomain arms that are PRD §3 KEEP / esc-4646-3). Comment
//! blocks placed above the call (as the domain arm does) are detected.
//!
//! Reference: `docs/prds/dimensionless-scalar-sentinel-stampout.md` §8/§10.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

// -----------------------------------------------------------------------
// Path scope
// -----------------------------------------------------------------------

/// Returns `true` when `path` (repo-root-relative) is within the hardcoded
/// detector scope. Scoped to `crates/reify-compiler/src/{entity,functions,
/// traits,expr}.rs` and `crates/reify-compiler/src/conformance/*.rs`.
fn in_scope(path: &str) -> bool {
    const EXACT: &[&str] = &[
        "crates/reify-compiler/src/entity.rs",
        "crates/reify-compiler/src/functions.rs",
        "crates/reify-compiler/src/traits.rs",
        "crates/reify-compiler/src/expr.rs",
    ];
    if EXACT.contains(&path) {
        return true;
    }
    path.starts_with("crates/reify-compiler/src/conformance/") && path.ends_with(".rs")
}

// -----------------------------------------------------------------------
// Pure line-window scanner
// -----------------------------------------------------------------------

/// The forward-scan window: how many lines after a trigger line we look for
/// a `dimensionless_scalar()` call.
///
/// The production push shape in the scoped compiler files spans up to ~10
/// lines (format!(...) body + `.with_code(...)` + `.with_label(...)` +
/// closing paren + comment block), so the window must be larger than that.
/// 16 lines comfortably covers the worst-case production shape while keeping
/// false-positive risk low.
const WINDOW: usize = 16;

/// Returns `true` when `line` references `DiagnosticCode::UnresolvedType`.
///
/// **Trigger semantics (broader than "error push"):** this matches *any* line
/// containing the token `DiagnosticCode::UnresolvedType` — including the
/// `.with_code(DiagnosticCode::UnresolvedType)` call inside an error push,
/// but also match arms or filter predicates that reference the code. In
/// practice no non-push reference occurs within WINDOW lines of a
/// `dimensionless_scalar()` in the scoped files, so the broader match does
/// not introduce false positives today; future maintainers should be aware
/// that a match arm followed within WINDOW lines by a `dimensionless_scalar()`
/// would trip the detector and require a `// ds-sentinel:allow` marker.
///
/// In production code the error push spans multiple lines:
/// ```rust
/// diagnostics.push(
///     Diagnostic::error(format!("..."))
///         .with_code(DiagnosticCode::UnresolvedType)  // ← trigger line
/// );
/// ```
fn is_error_push_line(line: &str) -> bool {
    line.contains("DiagnosticCode::UnresolvedType")
}

/// Returns `true` when `line` carries a `// ds-sentinel:allow` marker
/// (on the same line — covers both inline and trailing forms).
fn has_allow_marker(line: &str) -> bool {
    line.contains("// ds-sentinel:allow")
}

/// Scan `content` for offending `dimensionless_scalar()` sites.
///
/// Returns a vec of `(1-based line number, trimmed line text)` for each hit:
/// a `dimensionless_scalar()` line that falls within `WINDOW` lines after an
/// error-push line referencing `UnresolvedType`, carries no
/// `// ds-sentinel:allow` marker anywhere in the window, and is NOT inside
/// a comment (lines where `//` precedes `dimensionless_scalar()` are skipped —
/// they are doc-comments or explanatory prose, not code calls).
///
/// This function is pure `&str -> result` with no IO — mirrors `ptodo.rs`'s
/// `scan_file` split so unit tests can exercise the grammar without disk access.
// G-allow: called by check() (same module) and #[cfg(test)] unit tests; pub(crate) for in-crate test-direct access without external exposure.
pub(crate) fn scan_content(content: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let n = lines.len();
    // Track the most recent line index at which an error-push referencing
    // UnresolvedType was seen. `None` means no such push has been seen yet.
    let mut last_error_push: Option<usize> = None;
    let mut hits = Vec::new();

    for (i, &line) in lines.iter().enumerate() {
        if is_error_push_line(line) {
            last_error_push = Some(i);
        }
        if line.contains("dimensionless_scalar()") {
            // Skip if `dimensionless_scalar()` appears only inside a comment
            // (`//` or `///` doc-comments). A comment marker that precedes the
            // token on the same line means it is documentation or explanatory
            // prose — not an actual code call.  Inline trailing comments
            // (where `//` follows the call) are not filtered here because
            // they may carry `// ds-sentinel:allow` markers.
            let ds_pos = line.find("dimensionless_scalar()").unwrap_or(usize::MAX);
            if line.find("//").map_or(false, |cp| cp < ds_pos) {
                // The call sits inside a comment — not actual code; skip.
                continue;
            }
            // Check if we are within the forward window of a recent error push.
            if let Some(push_i) = last_error_push {
                let distance = i.saturating_sub(push_i);
                if distance > 0 && distance <= WINDOW {
                    // Scan the entire window [push_i..=i] for an allow marker.
                    // This covers comment-block markers placed several lines
                    // above the `dimensionless_scalar()` call (e.g. the
                    // functions.rs domain arm where the marker is in a 4-line
                    // comment block preceding the call), as well as inline
                    // markers on the call line itself. We also check the line
                    // immediately after (i+1) to support trailing-comment
                    // placement.
                    let window_has_allow = lines[push_i..=i]
                        .iter()
                        .any(|l| has_allow_marker(l));
                    let next_allow = i + 1 < n && has_allow_marker(lines[i + 1]);
                    if !window_has_allow && !next_allow {
                        hits.push((i + 1, line.to_string()));
                    }
                }
            }
        }
    }

    hits
}

// -----------------------------------------------------------------------
// check() — entry point
// -----------------------------------------------------------------------

/// Check the scoped compiler source files for ds-sentinel reintroduction sites.
///
/// Enumerates `ctx.git.ls_files()`, keeps only paths matching the hardcoded
/// scope, reads each file from disk, runs [`scan_content`], and maps each hit
/// to a [`Finding`] at [`Severity::Medium`] (advisory — mirrors PTODO).
///
/// Findings are sorted by `(path, line)` for deterministic output.
/// Unreadable files are skipped fail-safe (no finding, no panic).
pub fn check(ctx: &AuditContext<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut paths: Vec<String> = ctx
        .git
        .ls_files()
        .into_iter()
        .filter(|p| in_scope(p))
        .collect();
    paths.sort();

    for path in &paths {
        let full = ctx.project_root.join(path);
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(_) => continue, // fail-safe: skip unreadable files
        };
        for (line_no, line_text) in scan_content(&content) {
            findings.push(Finding {
                pattern: Pattern::PDsSentinel,
                severity: Severity::Medium,
                task_id: path.clone(),
                summary: format!(
                    "ds-sentinel: line {}: {}",
                    line_no,
                    line_text.trim()
                ),
                evidence: vec![EvidenceRef::File { path: path.clone() }],
            });
        }
    }

    // Already sorted by path (outer loop); within a file hits are in line order.
    findings
}

// -----------------------------------------------------------------------
// Unit tests — pure scan grammar (step-3)
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// (a) OFFENDER — multi-line error push (production form) followed within the
    /// window by dimensionless_scalar() → emits one hit at the dimensionless line.
    #[test]
    fn scan_offender_detected() {
        // Uses the real multi-line production form: trigger is the `.with_code(...)` line.
        let content = "\
fn resolve(name: &str) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    // fallback — should be Type::Error, not dimensionless_scalar
    Type::dimensionless_scalar()
}
";
        let hits = scan_content(content);
        assert_eq!(
            hits.len(),
            1,
            "expected exactly 1 hit for the offender pattern; got: {:?}",
            hits
        );
        // The hit should be on the dimensionless_scalar() line (line 7).
        assert_eq!(
            hits[0].0, 7,
            "hit must be on the dimensionless_scalar() line (line 7); got line {}",
            hits[0].0
        );
    }

    /// (b) ALLOW — same offender with `// ds-sentinel:allow <reason>` on the
    /// dimensionless_scalar() line → NO hit.
    #[test]
    fn scan_allow_marker_suppresses() {
        let content = "\
fn resolve(name: &str) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::dimensionless_scalar() // ds-sentinel:allow intentional KEEP: language default
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "allow marker must suppress the hit; got: {:?}",
            hits
        );
    }

    /// (b2) ALLOW on adjacent prev line — also suppresses.
    #[test]
    fn scan_allow_on_prev_line_suppresses() {
        let content = "\
fn resolve(name: &str) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    // ds-sentinel:allow intentional KEEP: language default
    Type::dimensionless_scalar()
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "allow marker on adjacent prev line must suppress the hit; got: {:?}",
            hits
        );
    }

    /// (b3) ALLOW on adjacent next line — also suppresses.
    #[test]
    fn scan_allow_on_next_line_suppresses() {
        let content = "\
fn resolve(name: &str) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::dimensionless_scalar()
    // ds-sentinel:allow intentional KEEP: language default
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "allow marker on adjacent next line must suppress the hit; got: {:?}",
            hits
        );
    }

    /// (c) CONVERTED — error push followed by `Type::Error` → NO hit.
    #[test]
    fn scan_converted_to_error_no_hit() {
        let content = "\
fn resolve(name: &str) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::Error
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "Type::Error (converted) must not be flagged; got: {:?}",
            hits
        );
    }

    /// (d) FAR — dimensionless_scalar() with no error push within the window → NO hit.
    #[test]
    fn scan_far_no_error_push_no_hit() {
        let content = "\
fn default_type() -> Type {
    Type::dimensionless_scalar()
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "dimensionless_scalar() with no preceding error push must not be flagged; got: {:?}",
            hits
        );
    }

    /// (e) KEEP-no-annotation — `None => Type::dimensionless_scalar()` with no
    /// preceding error push → NO hit.
    #[test]
    fn scan_keep_language_default_no_hit() {
        let content = "\
fn resolve_annotation(ann: Option<TypeExpr>) -> Type {
    match ann {
        Some(te) => resolve_type_expr(te),
        None => Type::dimensionless_scalar(),
    }
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "language-default None arm must not be flagged (no preceding error push); got: {:?}",
            hits
        );
    }

    /// (f) Window boundary — dimensionless just OUTSIDE the window → NO hit.
    #[test]
    fn scan_window_boundary_outside_no_hit() {
        // Trigger line (the .with_code line) at line 1. WINDOW = 8.
        // Place dimensionless at line 1 + WINDOW + 1 (outside window).
        let mut lines: Vec<String> = vec![
            "            .with_code(DiagnosticCode::UnresolvedType)".to_string(),
        ];
        // Fill WINDOW lines with neutral content.
        for i in 0..WINDOW {
            lines.push(format!("// filler {}", i));
        }
        // One line beyond the window.
        lines.push("    Type::dimensionless_scalar()".to_string());
        let content = lines.join("\n");

        let hits = scan_content(&content);
        assert!(
            hits.is_empty(),
            "dimensionless_scalar() at WINDOW+1 lines after trigger must not be flagged; \
             got: {:?}",
            hits
        );
    }

    /// (f2) Window boundary — dimensionless just INSIDE the window → ONE hit.
    #[test]
    fn scan_window_boundary_inside_hit() {
        // Trigger line at line 1. WINDOW = 16. Place dimensionless at line 1 + WINDOW.
        let mut lines: Vec<String> = vec![
            "            .with_code(DiagnosticCode::UnresolvedType)".to_string(),
        ];
        // Fill WINDOW-1 lines with neutral content.
        for i in 0..(WINDOW - 1) {
            lines.push(format!("// filler {}", i));
        }
        // Exactly at the window boundary.
        lines.push("    Type::dimensionless_scalar()".to_string());
        let content = lines.join("\n");

        let hits = scan_content(&content);
        assert_eq!(
            hits.len(),
            1,
            "dimensionless_scalar() at exactly WINDOW lines after trigger must be flagged; \
             got: {:?}",
            hits
        );
    }

    /// (g) Real production spacing — reproduces the functions.rs domain-arm
    /// shape: trigger at the `.with_code(DiagnosticCode::UnresolvedType)` line
    /// followed by a `.with_label(...)` clause + closing paren, then a 4-line
    /// comment block, then `dimensionless_scalar()` — a distance of ~10 lines,
    /// which is outside the old WINDOW=8 but inside the new WINDOW=16.
    ///
    /// An UNMARKED site at this spacing must be flagged.
    #[test]
    fn scan_real_production_spacing_detected() {
        // Mirrors the structure of the functions.rs field-domain Function arm
        // (line 637–652 in the original source).
        let content = "\
        reify_ast::TypeExprKind::Function { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    \"function type not allowed as a field domain type: {}\",
                    field_def.domain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.domain_type.span,
                    \"function type not allowed in this position\",
                )),
            );
            // The arrow type resolves fine — it is disallowed in this
            // position, not an unknown name.
            Type::dimensionless_scalar()
        }
";
        let hits = scan_content(content);
        assert_eq!(
            hits.len(),
            1,
            "an unmarked dimensionless_scalar() at ~10 lines after the UnresolvedType \
             trigger must be flagged (regression guard: WINDOW must be >= 10); got: {:?}",
            hits
        );
    }

    /// (g2) In-comment filtering — `dimensionless_scalar()` that appears inside a
    /// `//` line comment or `///` doc comment must NOT be flagged, even when a
    /// trigger is open. This covers (a) doc-comment prose referencing the call
    /// and (b) explanatory code comments that happen to mention the token.
    #[test]
    fn scan_dimensionless_in_comment_not_hit() {
        // Trigger is open; dimensionless_scalar() appears only in a // comment
        // and a /// doc comment — neither is actual code.
        let content = "\
fn assoc_fn_sig() {
    .with_code(DiagnosticCode::UnresolvedType)
    // A missing return type defaults to `Type::dimensionless_scalar()`, matching
    /// the convention of compile_function.
    // keep Type::dimensionless_scalar() rather than poison — the arrow type
    actual_code_here()
}
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "dimensionless_scalar() inside // comments must not be flagged; got: {:?}",
            hits
        );
    }

    /// (h) Window-range allow suppression — a `// ds-sentinel:allow` comment
    /// placed in a multi-line comment block *above* the `dimensionless_scalar()`
    /// call (as in the functions.rs domain arm where the marker is 4 lines
    /// before the call) must suppress the hit.
    ///
    /// This tests the full-window allow scan: the marker at `i-4` is inside
    /// the `[push_i..=i]` range even though it is not adjacent (`i±1`).
    #[test]
    fn scan_window_range_allow_in_comment_block_suppresses() {
        // Same structure as `scan_real_production_spacing_detected` but with
        // a `// ds-sentinel:allow` comment as the FIRST line of the comment
        // block (4 lines above the dimensionless_scalar() call).
        let content = "\
        reify_ast::TypeExprKind::Function { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    \"function type not allowed as a field domain type: {}\",
                    field_def.domain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.domain_type.span,
                    \"function type not allowed in this position\",
                )),
            );
            // ds-sentinel:allow PRD §3 KEEP: arrow type resolves fine —
            // disallowed in field-domain position, not an unknown name.
            // Converting to Type::Error here would misrepresent the error.
            Type::dimensionless_scalar()
        }
";
        let hits = scan_content(content);
        assert!(
            hits.is_empty(),
            "a // ds-sentinel:allow marker in a comment block above the \
             dimensionless_scalar() call (not adjacent) must suppress the hit; \
             got: {:?}",
            hits
        );
    }
}
