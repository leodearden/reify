//! P-DEAD — dead-code detector.
//!
//! For every public symbol reported by
//! [`JCodemunchOps::get_dead_code`] (equivalent to
//! `mcp__jcodemunch__get_dead_code_v2(min_confidence)`), emits an advisory
//! [`Finding`] with [`Severity::Low`] (never promoted).
//!
//! Reference: `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3/§4-d.
//!
//! ## Honest-bound / Low-only discipline
//!
//! Findings are pinned to `Severity::Low` and no follow-up tasks are
//! auto-filed. `get_dead_code_v2` is a repo-wide multi-signal heuristic;
//! jcodemunch's Rust dead-code accuracy is unproven at this stage.
//! Promotion to Medium / auto-file is explicitly deferred (PRD §5) until a
//! live-corpus false-positive review.
//!
//! ## Scope excludes
//!
//! Mirrors the P1 producer-orphan excludes "where sensible":
//! - `crates/reify-stdlib/` — stdlib `.ri` defs are always "orphan" until consumed.
//! - `crate::is_test_path` — test files produce noise unrelated to production dead-code.
//!
//! `#[cfg(test)]`-in-src symbols are NOT path-excludable; the live
//! `get_dead_code_v2` server already defaults `include_tests=False`, and PDEAD
//! is advisory/Low — over-engineering an attribute-aware exclude is unwarranted.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

const DEFAULT_MIN_CONFIDENCE: f64 = 0.5;

pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    for sym in ctx.jcodemunch.get_dead_code(DEFAULT_MIN_CONFIDENCE) {
        // Mirror P1's stdlib/test scope-excludes (see p1_producer_orphan.rs:136-138).
        if sym.file.starts_with("crates/reify-stdlib/") {
            continue;
        }
        if crate::is_test_path(&sym.file) {
            continue;
        }

        let signals_part = if sym.signals.is_empty() {
            String::new()
        } else {
            format!("; signals: {}", sym.signals.join(", "))
        };
        let summary = format!(
            "{kind} `{name}` in {file}:{line} — confidence {confidence:.2}{signals_part}",
            kind = sym.kind,
            name = sym.name,
            file = sym.file,
            line = sym.line,
            confidence = sym.confidence,
            signals_part = signals_part,
        );

        findings.push(Finding {
            pattern: Pattern::PDeadCode,
            severity: Severity::Low,
            // Empty task_id is intentional: PDEAD is repo-wide, not task-scoped.
            // Downstream consumers must tolerate empty task_id for repo-wide detectors.
            task_id: String::new(),
            summary,
            evidence: vec![EvidenceRef::File { path: sym.file.clone() }],
        });
    }

    findings
}
