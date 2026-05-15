//! P2 — consumer-stub detector.
//!
//! Scans the added-lines portion of `git diff main..task-branch` (filtered to
//! `metadata.files`) for canonical stub markers and emits Medium-severity
//! Findings (Low when the task title contains "stub" or "placeholder").
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §5 P2.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity, TaskMetadata};

/// Returns `Some(label)` if the line matches any canonical stub-pattern family,
/// or `None` if the line is clean.
///
/// Six families (hand-rolled `&str` checks — `regex` is intentionally NOT a
/// dependency per design §12):
/// 1. TODO variants: `TODO(…pending)`, `TODO(post-\w+)`, `TODO(…later)`,
///    `TODO(task_\d+)` — substring scans on a lowercase copy.
/// 2. `unimplemented!(` — hard panic placeholder.
/// 3. `panic!(` + later `not yet` — explicit "not yet implemented" panic.
/// 4. `tracing::warn!(` + `reason="task_` + `_pending"` — structured warning.
/// 5. `Value::Undef` + comment substring `pending`, `stub`, or `placeholder`.
/// 6. Bare line-comments: `// stub`, `// placeholder`, `// fixme` (case-insensitive).
fn line_matches_stub(line: &str) -> Option<&'static str> {
    let lower = line.to_lowercase();

    // Family 1 — TODO variants.
    if lower.contains("todo(") {
        if lower.contains("pending") {
            return Some("TODO(pending)");
        }
        if lower.contains("post-") {
            return Some("TODO(post-\\w+)");
        }
        if lower.contains("later") {
            return Some("TODO(later)");
        }
        if lower.contains("task_") {
            // Confirm the numeric part exists: look for "task_" followed by at least one digit.
            if let Some(idx) = lower.find("task_") {
                let after = &lower[idx + 5..];
                if after.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    return Some("TODO(task_N)");
                }
            }
        }
    }

    // Family 2 — unimplemented!
    if lower.contains("unimplemented!(") {
        return Some("unimplemented!");
    }

    // Family 3 — panic!(... not yet ...)
    if lower.contains("panic!(") && lower.contains("not yet") {
        return Some("panic!(not yet)");
    }

    // Family 4 — tracing::warn! with task_*_pending reason field.
    if lower.contains("tracing::warn!(") && lower.contains("reason=\"task_") && lower.contains("_pending\"") {
        return Some("tracing::warn!(task_pending)");
    }

    // Family 5 — Value::Undef arm with pending/stub/placeholder in comment.
    if lower.contains("value::undef") {
        if lower.contains("pending") || lower.contains("stub") || lower.contains("placeholder") {
            return Some("Value::Undef(pending/stub/placeholder)");
        }
    }

    // Family 6 — bare line-comment markers.
    // Match `// stub`, `// placeholder`, `// fixme` anywhere in the line (case-insensitive).
    if lower.contains("// stub") || lower.contains("// placeholder") || lower.contains("// fixme") {
        if lower.contains("// stub") {
            return Some("// stub");
        }
        if lower.contains("// placeholder") {
            return Some("// placeholder");
        }
        return Some("// fixme");
    }

    None
}

/// Returns `true` when the path looks like a test file that should be
/// excluded from P2 scanning (false-positive guard per design §5 P2):
/// - paths containing `/tests/`      — Rust integration-test directories
/// - paths ending with `_test.rs`    — Go-style test files
/// - paths containing `__tests__/`   — JavaScript/TypeScript test directories
fn is_test_path(p: &str) -> bool {
    p.contains("/tests/") || p.ends_with("_test.rs") || p.contains("__tests__/")
}

/// Returns `true` when the task title itself signals that the task is
/// intentionally a stub or placeholder (case-insensitive substring match).
/// Used to downgrade finding severity from Medium to Low.
fn title_signals_stub(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("stub") || t.contains("placeholder")
}

// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    for meta in ctx.task_metadata.values() {
        // Optional single-task narrowing (mirrors p5_phantom_done::check_with_target).
        if let Some(target) = ctx.target_task_id.as_deref()
            && meta.task_id != target
        {
            continue;
        }

        let task_branch = format!("task/{}", meta.task_id);
        let severity = if title_signals_stub(&meta.title) {
            Severity::Low
        } else {
            Severity::Medium
        };

        for path in &meta.files {
            // Skip test-shaped paths to avoid false positives on intentional
            // stubs inside test helpers (design §5 P2 false-positive guards).
            if is_test_path(path) {
                continue;
            }
            let added = ctx.git.diff_added_lines("main", &task_branch, path);
            let mut matches: Vec<(usize, String, &'static str)> = Vec::new();
            for (line_no, content) in &added {
                if let Some(label) = line_matches_stub(content) {
                    matches.push((*line_no, content.clone(), label));
                }
            }
            if matches.is_empty() {
                continue;
            }
            let summary = {
                let count = matches.len();
                let details: Vec<String> = matches
                    .iter()
                    .map(|(ln, snippet, label)| {
                        let snip = if snippet.len() > 60 {
                            format!("{}…", &snippet[..60])
                        } else {
                            snippet.clone()
                        };
                        format!("line {} [{}]: {}", ln, label, snip.trim())
                    })
                    .collect();
                format!(
                    "{} stub marker(s) in added lines of {}: {}",
                    count,
                    path,
                    details.join("; ")
                )
            };
            findings.push(Finding {
                pattern: Pattern::P2ConsumerStub,
                severity,
                task_id: meta.task_id.clone(),
                summary,
                evidence: vec![EvidenceRef::File { path: path.clone() }],
            });
        }
    }

    findings
}
