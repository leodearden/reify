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
//! - Per symbol: the declaration could not be located, so whether its author
//!   opted out is UNKNOWN ([`crate::ChangedSymbol::decl_located`]) — SKIPPED outright
//!   rather than downgraded, since unknown is not "no opt-out"; an opt-out the
//!   located declaration carries — `#[allow(dead_code)]` / `#[cfg(test)]` /
//!   a non-blank `// G-allow:` marker ([`crate::ChangedSymbol::opts_out`]);
//!   a non-test workspace caller.
//! - Surviving symbols: severity is Medium only once *strictly more than*
//!   14 days have elapsed since the done-flip (design §5 P1, line 83:
//!   ">14 days"); at exactly the boundary and anywhere inside the window it
//!   is Low ("log only").
//!
//! When the FIRST of those per-symbol guards eats a task's entire symbol list,
//! zero findings means "examined nothing", not "corpus clean", so
//! [`unexamined_sweep_breadcrumb`] annotates it on stderr.

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

        let symbols = ctx.jcodemunch.get_changed_symbols(&since_sha, until_sha);
        if let Some(msg) =
            unexamined_sweep_breadcrumb(&symbols, &meta.task_id, &since_sha, until_sha)
        {
            eprintln!("{msg}");
        }

        for symbol in symbols {
            // Per-symbol guard: the declaration could not be located, so whether
            // its author opted out is UNKNOWN, not "no". Reporting here is what
            // turns a jcodemunch grammar drift (a release that stops emitting
            // `line`, as 1.108.54 already did for `find_references`), a stale
            // index, or an unreadable file into a false-positive storm over
            // every intentionally suppressed symbol. The enrichment pass has
            // already told the operator.
            if !symbol.decl_located() {
                continue;
            }
            // Per-symbol guard: an intentional-orphan opt-out the located
            // declaration carries — `#[allow(dead_code)]` / `#[cfg(test)]` /
            // a non-blank `// G-allow:` marker, whose shared rule (and the
            // orphan-script regex it mirrors) lives in `DeclSuppression`
            // (design §5 P1).
            if symbol.opts_out() {
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

/// Returns a `reify-audit:` prefixed stderr breadcrumb when every symbol this
/// task's `get_changed_symbols` returned had an unlocatable declaration, so P1
/// skipped all of them and its zero findings report a degraded jcodemunch
/// substrate rather than a clean corpus.
///
/// Returns `None` otherwise, including for an EMPTY result: a done task that
/// introduced no public symbol has always been an ordinary no-op here, and P1
/// visits every done task, so annotating that would be one line per task on
/// the (common) unwired-jcodemunch path. The vacuity rule itself lives in
/// [`crate::wholly_unlocatable_count`], shared with P5 H2's breadcrumb.
///
/// Mirrors `p5_phantom_done::h2_vacuous_breadcrumb`'s "return the diagnostic,
/// let the caller `eprintln!` it" idiom — an in-process test cannot read its
/// own process's stderr, so a message printed from inside here could be
/// deleted with the whole suite still green.
fn unexamined_sweep_breadcrumb(
    symbols: &[ChangedSymbol],
    task_id: &str,
    since_sha: &str,
    until_sha: &str,
) -> Option<String> {
    let unlocatable = crate::wholly_unlocatable_count(symbols)?;
    Some(format!(
        "reify-audit: P1 (producer-orphan) vacuous for task {task_id}: all \
         {unlocatable} symbol(s) from {since_sha}..{until_sha} had an unlocatable \
         declaration and were skipped unexamined — P1 produced no findings for \
         this task (jcodemunch substrate degraded, not a clean corpus)"
    ))
}

#[cfg(test)]
mod tests {
    use super::unexamined_sweep_breadcrumb;
    use crate::{ChangedSymbol, DeclSuppression};

    fn symbol(name: &str, suppression: Option<DeclSuppression>) -> ChangedSymbol {
        ChangedSymbol {
            name: name.to_string(),
            file: "crates/foo/src/lib.rs".to_string(),
            line: 42,
            suppression,
        }
    }

    /// The quiet-degradation case this breadcrumb exists for: a NON-empty
    /// symbol list in which not one declaration was locatable. P1 skips every
    /// row, so a zero-finding sweep would otherwise read as "corpus clean"
    /// — the same jcodemunch grammar drift that was loud (a false-positive
    /// storm) before unlocatable symbols were skipped.
    #[test]
    fn unexamined_sweep_breadcrumb_fires_when_no_declaration_was_locatable() {
        let symbols = vec![symbol("alpha", None), symbol("beta", None)];
        let msg = unexamined_sweep_breadcrumb(&symbols, "7600", "abc123^1", "abc123")
            .expect("an all-unlocatable sweep must produce a breadcrumb");
        assert!(
            msg.contains("7600"),
            "breadcrumb must name the task id; got: {msg}"
        );
        assert!(
            msg.contains("vacuous"),
            "breadcrumb must name the sweep as vacuous; got: {msg}"
        );
        assert!(
            msg.contains('2'),
            "breadcrumb must name how many symbols went unexamined; got: {msg}"
        );
        assert_eq!(
            msg.lines().count(),
            1,
            "breadcrumb must stay one line; got: {msg:?}"
        );
    }

    /// One examinable symbol is enough to make the sweep real: the detector
    /// applied its guards to something, so zero findings is a genuine result
    /// and an operator must not be told otherwise.
    #[test]
    fn unexamined_sweep_breadcrumb_is_silent_when_any_declaration_was_located() {
        let symbols = vec![
            symbol("alpha", None),
            symbol("beta", Some(DeclSuppression::default())),
        ];
        assert_eq!(
            unexamined_sweep_breadcrumb(&symbols, "7600", "abc123^1", "abc123"),
            None,
            "a sweep that examined even one declaration is not vacuous"
        );
    }

    /// An empty result is P1's ordinary no-op — a done task can legitimately
    /// introduce no public symbol, and P1 visits every done task, so this
    /// branch must not annotate.
    #[test]
    fn unexamined_sweep_breadcrumb_is_silent_for_an_empty_sweep() {
        assert_eq!(
            unexamined_sweep_breadcrumb(&[], "7600", "abc123^1", "abc123"),
            None,
            "no symbols at all is not the same fact as none examinable"
        );
    }
}
