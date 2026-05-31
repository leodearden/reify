//! P-UNTESTED — static test-reachability detector.
//!
//! For every symbol reported by
//! [`JCodemunchOps::get_untested_symbols`] (equivalent to
//! `mcp__jcodemunch__get_untested_symbols(min_confidence)`) that is not
//! reached by any test path, emits an advisory [`Finding`] with
//! [`Severity::Low`] (never promoted).
//!
//! Reference: `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3/§4-b/§4-d/§8.
//!
//! ## Honest-bound / Low-only discipline
//!
//! Findings are pinned to `Severity::Low` and no follow-up tasks are
//! auto-filed. Reify has many legitimately-untested symbols (high noise ratio);
//! promotion to Medium / auto-file is deferred until a live-corpus
//! false-positive review (PRD §8 routing table; §4-b).
//!
//! This detector reports STATIC test-reachability (jcodemunch static analysis),
//! NOT runtime coverage. The distinction is surfaced in every finding summary
//! so downstream consumers are not misled (PRD §4-d).
//!
//! ## Scope excludes
//!
//! Mirrors the P1 producer-orphan excludes and the PDEAD sibling:
//! - `crates/reify-stdlib/` — stdlib `.ri` defs are always "untested" until consumed.
//! - `crate::is_test_path` — a symbol declared in a test file being "untested"
//!   is pure noise unrelated to production coverage.
//!
//! ## Reached-symbol suppression
//!
//! jcodemunch returns symbols filtered only by `min_confidence`. A symbol with
//! `reached: true` is genuinely reached by some test path and must not be flagged
//! (it is not untested). The detector drops these before emitting findings so
//! only `reached: false` symbols surface.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

const DEFAULT_MIN_CONFIDENCE: f64 = 0.5;

pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    for sym in ctx.jcodemunch.get_untested_symbols(DEFAULT_MIN_CONFIDENCE) {
        // A reached symbol is not untested — suppress it.
        if sym.reached {
            continue;
        }
        // Mirror P1/PDEAD stdlib/test scope-excludes.
        if sym.file.starts_with("crates/reify-stdlib/") {
            continue;
        }
        if crate::is_test_path(&sym.file) {
            continue;
        }

        let summary = format!(
            "`{}` in {} is not reached by any test — confidence {:.2} \
            (static test-reachability, not runtime coverage; advisory)",
            sym.name, sym.file, sym.confidence,
        );

        findings.push(Finding {
            pattern: Pattern::PUntested,
            severity: Severity::Low,
            // Empty task_id is intentional: PUNTESTED is repo-wide, not task-scoped.
            task_id: String::new(),
            summary,
            evidence: vec![EvidenceRef::File { path: sym.file.clone() }],
        });
    }

    findings
}
