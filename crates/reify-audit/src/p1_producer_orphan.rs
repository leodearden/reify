//! P1 — producer-orphan detector.
//!
//! For every public symbol a `done` task introduced (via
//! [`JCodemunchOps::get_changed_symbols`] keyed on the task's done-flip
//! timestamp), flags a Finding when the symbol has no non-test caller in the
//! workspace and no pending/in-progress consumer task — Medium past the
//! 14-day grace window, Low within it.
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §5 P1.
//!
//! Guards (each suppresses the finding; firing order built up across
//! steps 6/8/10/12): `audit_foundation` → stdlib-scope → symbol attributes
//! (`#[allow(dead_code)]`/`#[cfg(test)]`) → `// G-allow:` marker →
//! pending-consumer task → non-test workspace caller → grace window.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    for meta in ctx.task_metadata.values() {
        if meta.status != "done" {
            continue;
        }
        let Some(done_at) = meta.done_at else {
            continue;
        };

        for symbol in ctx.jcodemunch.get_changed_symbols("main", done_at) {
            let summary = format!(
                "producer-orphan: public symbol `{}` introduced by done task {} at {}:{}",
                symbol.name, meta.task_id, symbol.file, symbol.line
            );
            findings.push(Finding {
                pattern: Pattern::P1ProducerOrphan,
                severity: Severity::Medium,
                task_id: meta.task_id.clone(),
                summary,
                evidence: vec![EvidenceRef::File { path: symbol.file.clone() }],
            });
        }
    }

    findings
}
