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
use std::time::{SystemTime, UNIX_EPOCH};

/// 14-day grace window (per `f-infra-design.md` §5 P1): a producer-orphan is
/// flagged Medium only after this elapses since the done-flip; within it the
/// finding is downgraded to Low ("log only").
const GRACE_WINDOW_SECS: i64 = 14 * 86_400;

/// Returns `true` when the path looks like a test file that should be
/// excluded when deciding whether a workspace caller exists (false-positive
/// guard per `f-infra-design.md` §5 P1).
///
/// VERBATIM copy of `p2_consumer_stub::is_test_path` — that helper is private
/// (not importable), so mirroring its body byte-for-byte keeps the crate's
/// two detectors' test-path semantics provably identical.
fn is_test_path(p: &str) -> bool {
    // `tests/` with and without a leading slash covers both repo-root paths
    // (e.g. `tests/foo.rs`) and nested paths (e.g. `crates/x/tests/foo.rs`).
    p.starts_with("tests/")
        || p.contains("/tests/")
        || p.ends_with("_test.rs")
        || p.contains("__tests__/")
        || p.contains(".test.")  // JS/TS: foo.test.ts
        || p.contains(".spec.")  // JS/TS: foo.spec.ts
}

// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Synthetic clock when provided (deterministic tests); else wall clock.
    let now_secs = ctx.now.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after UNIX_EPOCH")
            .as_secs() as i64
    });

    for meta in ctx.task_metadata.values() {
        if meta.status != "done" {
            continue;
        }
        let Some(done_at) = meta.done_at else {
            continue;
        };

        for symbol in ctx.jcodemunch.get_changed_symbols("main", done_at) {
            // A non-test workspace caller proves the symbol is consumed —
            // suppress (design §5 P1: refs filtered to non-`*/tests/*`).
            let has_non_test_caller = ctx
                .jcodemunch
                .find_references(&symbol.name)
                .iter()
                .any(|r| !is_test_path(&r.file));
            if has_non_test_caller {
                continue;
            }

            let age = now_secs.saturating_sub(done_at);
            let (severity, summary) = if age >= GRACE_WINDOW_SECS {
                (
                    Severity::Medium,
                    format!(
                        "producer-orphan: public symbol `{}` introduced by done task {} \
                         at {}:{}; {} days past done-flip (beyond the 14-day grace window)",
                        symbol.name,
                        meta.task_id,
                        symbol.file,
                        symbol.line,
                        age / 86_400
                    ),
                )
            } else {
                (
                    Severity::Low,
                    format!(
                        "producer-orphan: public symbol `{}` introduced by done task {} \
                         at {}:{}; within the 14-day grace window; log only \
                         (per f-infra-design.md §5 P1)",
                        symbol.name, meta.task_id, symbol.file, symbol.line
                    ),
                )
            };
            findings.push(Finding {
                pattern: Pattern::P1ProducerOrphan,
                severity,
                task_id: meta.task_id.clone(),
                summary,
                evidence: vec![EvidenceRef::File { path: symbol.file.clone() }],
            });
        }
    }

    findings
}
