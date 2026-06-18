//! P1 — producer-orphan detector.
//!
//! For every public symbol a `done` task introduced (via
//! [`JCodemunchOps::get_changed_symbols`] keyed on the task's merged commit
//! range `{commit}^1..{commit}`), flags a Finding when the symbol has no
//! non-test caller in the workspace and no pending/in-progress/review consumer
//! task — Medium past the 14-day grace window, Low within it.
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §5 P1.
//!
//! The `done_provenance.commit` field (set by reify-orchestrator's resolution
//! path) is used to form `since_sha = "{commit}^1"` and `until_sha = commit`,
//! following the same `^1..commit` convention as
//! `RealGitOps::diff_added_lines_in_commit` (established by task 4074 for P2).
//! The `done_at` timestamp (epoch-seconds) still drives the 14-day grace-window
//! age calc — the two fields are orthogonal. Tasks without a resolvable commit
//! are skipped (jcodemunch has nothing to diff).
//!
//! False-positive guards, in firing order (each short-circuits the finding):
//!
//! - Per task: not `done`; no `done_at`; `audit_foundation`
//!   (foundation/scaffold task); a pending/in-progress/review consumer task
//!   whose `consumer_ref` matches this producer's `prd`; no `done_provenance.commit`.
//! - Per symbol: `#[allow(dead_code)]` / `#[cfg(test)]` attribute opt-out;
//!   a non-blank `// G-allow:` marker; a non-test workspace caller.
//! - Surviving symbols: severity is Medium only once *strictly more than*
//!   14 days have elapsed since the done-flip (design §5 P1, line 83:
//!   ">14 days"); at exactly the boundary and anywhere inside the window it
//!   is Low ("log only").

use crate::{AuditContext, ChangedSymbol, EvidenceRef, Finding, Pattern, Severity};
use std::time::{SystemTime, UNIX_EPOCH};

/// 14-day grace window. `f-infra-design.md` §5 P1 line 83 specifies a
/// producer-orphan is "flagged only if **>14 days** have passed since
/// done-flip", so the comparison is *strict*: the finding is Medium only
/// once the elapsed time exceeds this many seconds. At exactly the boundary
/// (`age == GRACE_WINDOW_SECS`) and anywhere inside the window the finding
/// is downgraded to Low ("log only").
const GRACE_WINDOW_SECS: i64 = 14 * 86_400;

/// Returns `true` when some active-consumer task's `consumer_ref` points at
/// `producer_prd` — i.e. a downstream consumer is already in flight, so the
/// producer's symbols are not truly orphaned (design §5 P1 false-positive
/// guard). The canonical Taskmaster pending-consumer statuses are:
///   `"pending"`, `"in-progress"`, `"review"`.
/// Using an explicit allow-list (rather than inverting against `{done,
/// cancelled, deferred, blocked}`) keeps semantic intent visible and
/// fails-safe: a future Taskmaster status won't silently suppress findings.
/// Status strings follow Taskmaster's canonical form (`in-progress`,
/// hyphenated); the T-4 CLI normalizes at the boundary. Per
/// `f-infra-design.md` §5 P1.
// Perf note: this rescans every task for each done producer that has a
//   `prd`, so the producer↔consumer correlation is O(tasks²). Harmless at
//   solo-OSS task volumes (the audit window is ~14 days of done-flips), but
//   if `task_metadata` ever grows, precompute a `HashSet<&str>` of the
//   `consumer_ref`s of pending/in-progress/review tasks once before the
//   producer loop and do an O(1) membership check here. Not required at
//   current scale. Reference: docs/architecture-audit/f-infra-design.md §5 P1.
fn has_pending_consumer(ctx: &AuditContext, producer_prd: &str) -> bool {
    ctx.task_metadata.values().any(|t| {
        matches!(t.status.as_str(), "pending" | "in-progress" | "review")
            && t.consumer_ref.as_deref() == Some(producer_prd)
    })
}

/// Returns `true` when the symbol carries a non-blank `// G-allow:` marker.
///
/// Mirrors `scripts/audit-orphan-producers.sh:150`
/// `G_ALLOW_RE = //\s*G-allow:\s*(.+)`: the `(.+)` requires at least one
/// non-whitespace character, so a blank/whitespace-only marker does NOT
/// suppress — keeping this detector and the orphan script in lockstep.
fn is_g_allow_suppressed(symbol: &ChangedSymbol) -> bool {
    symbol
        .g_allow_marker
        .as_deref()
        .is_some_and(|r| !r.trim().is_empty())
}

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
        // Optional single-task narrowing: mirrors p2_consumer_stub::check at
        // lines 132-136 and p5_phantom_done::check_with_target — keeps all
        // three detectors' scoping behaviour in lockstep.
        if let Some(target) = ctx.target_task_id.as_deref()
            && meta.task_id != target
        {
            continue;
        }
        if meta.status != "done" {
            continue;
        }
        let Some(done_at) = meta.done_at else {
            continue;
        };

        // Per-task guard: foundation/scaffold task — its symbols are
        // intentionally not yet consumed (design §5 P1 false-positive guard).
        if meta.audit_foundation == Some(true) {
            continue;
        }
        // Per-task guard: a pending/in-progress/review consumer task already
        // references this producer's PRD (design §5 P1 false-positive guard).
        if let Some(prd) = meta.prd.as_deref()
            && has_pending_consumer(ctx, prd)
        {
            continue;
        }

        // Commit-range resolution: derive (since_sha, until_sha) from
        // done_provenance.commit using the `{commit}^1..{commit}` convention
        // (same as RealGitOps::diff_added_lines_in_commit — established by
        // task 4074 for P2). A task with no resolvable commit is skipped:
        // jcodemunch has no range to diff, so no symbols can be reported.
        let Some(commit) = meta.done_provenance.as_ref().and_then(|p| p.commit.as_deref()) else {
            continue;
        };
        let since_sha = format!("{commit}^1");
        let until_sha = commit;

        for symbol in ctx.jcodemunch.get_changed_symbols(&since_sha, until_sha) {
            // Per-symbol guard: intentional-orphan opt-outs —
            // `#[allow(dead_code)]` / `#[cfg(test)]` (design §5 P1).
            if symbol.has_allow_dead_code || symbol.has_cfg_test {
                continue;
            }
            // Per-symbol guard: a non-blank `// G-allow:` marker on the
            // declaration (design §5 P1; mirrors the orphan-script regex).
            if is_g_allow_suppressed(&symbol) {
                continue;
            }
            // A non-test workspace caller proves the symbol is consumed —
            // suppress (design §5 P1: refs filtered to non-`*/tests/*`).
            let has_non_test_caller = ctx
                .jcodemunch
                .find_references(&symbol)
                .iter()
                .any(|r| !crate::is_test_path(&r.file));
            if has_non_test_caller {
                continue;
            }

            let age = now_secs.saturating_sub(done_at);
            // Strict `>` per design §5 P1 line 83 (">14 days"): at exactly
            // the boundary the finding stays Low (still inside the window).
            let (severity, summary) = if age > GRACE_WINDOW_SECS {
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
