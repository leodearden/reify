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
//! content scan, per PRD §10). Walk the file lines; when a line contains
//! `diagnostics.push(Diagnostic::error(` and references `UnresolvedType`,
//! open a bounded backward window. If a subsequent line within the window
//! contains `dimensionless_scalar()` and does NOT carry a
//! `// ds-sentinel:allow` marker, flag it as an offender.
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
//! Add a `// ds-sentinel:allow <one-line rationale>` comment on the
//! `dimensionless_scalar()` line (or an adjacent line) to suppress a
//! legitimate KEEP site (e.g. the `functions.rs` arrow/function field
//! domain/codomain arms that are PRD §3 KEEP / esc-4646-3).
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

/// The forward-scan window: how many lines after an error-push line we look
/// for a `dimensionless_scalar()` call. A small window (e.g. 8 lines) is
/// enough to capture the common same-block pattern while avoiding false
/// positives from unrelated distant code.
const WINDOW: usize = 8;

/// Returns `true` when `line` is part of an `UnresolvedType` diagnostic push.
///
/// In production code the error push spans multiple lines:
/// ```rust
/// diagnostics.push(
///     Diagnostic::error(format!("..."))
///         .with_code(DiagnosticCode::UnresolvedType)  // ← trigger line
/// );
/// ```
/// We match any line that contains `DiagnosticCode::UnresolvedType` (the
/// `.with_code(...)` call that marks it as an error push). This handles both
/// the multi-line production form and any hypothetical single-line form.
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
/// error-push line referencing `UnresolvedType`, and carries no
/// `// ds-sentinel:allow` marker.
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
            // Check if we are within the forward window of a recent error push.
            if let Some(push_i) = last_error_push {
                let distance = i.saturating_sub(push_i);
                if distance > 0 && distance <= WINDOW {
                    // Check for allow marker on this line or either adjacent line.
                    let prev_allow = i > 0 && has_allow_marker(lines[i - 1]);
                    let next_allow = i + 1 < n && has_allow_marker(lines[i + 1]);
                    if !has_allow_marker(line) && !prev_allow && !next_allow {
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
        // Trigger line at line 1. WINDOW = 8. Place dimensionless at line 1 + WINDOW.
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
}
