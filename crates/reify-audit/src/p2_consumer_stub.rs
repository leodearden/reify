//! P2 — consumer-stub detector.
//!
//! Scans the added-lines portion of `git diff main..task-branch` (filtered to
//! `metadata.files`) for canonical stub markers and emits Medium-severity
//! Findings (Low when the task title contains "stub" or "placeholder").
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §5 P2.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

/// Returns `Some(label)` if the line matches any canonical stub-pattern family,
/// or `None` if the line is clean.
///
/// Six families (hand-rolled `&str` checks — `regex` is intentionally NOT a
/// dependency per design §12):
/// 1. TODO variants: `TODO(…pending)`, `TODO(post-…)`, `TODO(…later)`,
///    `TODO(task_N)` — substring scans on a lowercase copy.
/// 2. `unimplemented!(` — hard panic placeholder.
/// 3. `panic!(` + later `not yet` — explicit "not yet implemented" panic.
/// 4. `tracing::warn!(` + `reason="task_` + `_pending"` — structured warning.
/// 5. `Value::Undef` + comment substring `pending`, `stub`, or `placeholder`.
/// 6. Bare line-comments: `// stub`, `// placeholder`, `// fixme` (case-insensitive).
fn line_matches_stub(line: &str) -> Option<&'static str> {
    let lower = line.to_lowercase();

    // Family 1 — TODO variants.  Sub-checks run only on the content INSIDE
    // the TODO(...) parens to prevent cross-talk with unrelated tokens on the
    // same line (e.g. `// TODO(refactor) // see task_123` must NOT match
    // TODO(task_N) because `task_123` lives outside the parens).
    if let Some(paren_start) = lower.find("todo(") {
        let inner_start = paren_start + 5; // skip "todo("
        let inner = if let Some(paren_end) = lower[inner_start..].find(')') {
            &lower[inner_start..inner_start + paren_end]
        } else {
            &lower[inner_start..]
        };
        if inner.contains("pending") {
            return Some("TODO(pending)");
        }
        if inner.contains("post-") {
            return Some("TODO(post-)");
        }
        if inner.contains("later") {
            return Some("TODO(later)");
        }
        // Numeric task reference: "task_" followed by at least one digit.
        if let Some(idx) = inner.find("task_") {
            let after = &inner[idx + 5..];
            if after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some("TODO(task_N)");
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
    if lower.contains("value::undef")
        && (lower.contains("pending") || lower.contains("stub") || lower.contains("placeholder"))
    {
        return Some("Value::Undef(pending/stub/placeholder)");
    }

    // Family 6 — bare line-comment markers (case-insensitive).
    // Three independent checks rather than an outer guard + inner ladder to
    // avoid evaluating each substring twice (maintenance hazard if a fourth
    // marker is added later).
    if lower.contains("// stub") { return Some("// stub"); }
    if lower.contains("// placeholder") { return Some("// placeholder"); }
    if lower.contains("// fixme") { return Some("// fixme"); }

    None
}


/// Returns `true` when the task title itself signals that the task is
/// intentionally a stub or placeholder (case-insensitive substring match).
/// Used to downgrade finding severity from Medium to Low.
fn title_signals_stub(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("stub") || t.contains("placeholder")
}

/// Threshold above which `check()` warns about an unbounded backlog
/// when both `target_task_id` and `window` are unset (sweep mode
/// without pre-narrowing — see the `# Callers` rustdoc on `check`).
/// 50 is well above every existing test fixture (max 7 tasks in
/// `seven_prepd_legacy_tasks_produce_no_false_positives` /
/// audit_integration.rs) and well below the unbounded-backlog
/// scenario (hundreds of tasks loaded from fused-memory). The
/// comparison is strict `>` so a backlog of exactly 50 does not warn.
const SWEEP_BACKLOG_WARN_THRESHOLD: usize = 50;

/// Scan all tasks in `ctx.task_metadata` for canonical stub markers in their
/// added-line diff and return [`Pattern::P2ConsumerStub`] findings.
///
/// # Callers
///
/// **Pre-done hook** (`check_pre_done`-style): set `ctx.target_task_id` to the
/// single closing task.  The task's `status` will still be `"in_progress"` at
/// call time — the orchestrator has not yet flipped it to `"done"` — so P2
/// omits a `status != "done"` filter (see the `NOTE` comment inside the body).
///
/// **Periodic sweep** (`--mode sweep`): narrow `ctx.task_metadata` to the
/// closing-window tasks **before** calling this function.  Passing the full
/// backlog will surface every in-progress `TODO(... pending)` as a finding,
/// because P2 has no internal status filter to suppress legitimate WIP markers.
///
/// Reference: `docs/architecture-audit/f-infra-design.md` §5 P2 and §10.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    // Sweep-mode contract enforcement (esc-3752-365). The # Callers rustdoc
    // above requires sweep-mode callers to narrow `ctx.task_metadata` to
    // closing-window tasks before invoking check(). A caller that forgets —
    // runs --mode sweep with no --task and no --since against the full
    // fused-memory backlog — would silently surface every in-progress WIP
    // `TODO(... pending)` as a Medium-severity finding. Make the contract
    // explicit per project convention "contract in production code is made
    // explicit rather than relying on test coverage".
    //
    // We use BOTH debug_assert! and eprintln!:
    //   - debug_assert! panics in dev/test (loud fail-fast; the
    //     `sweep_mode_unbounded_backlog_panics_in_debug` integration test
    //     pins this signal via #[should_panic(expected = "unbounded backlog")]).
    //   - eprintln! emits a `reify-audit:` breadcrumb so production release
    //     builds (debug_assert compiled out) still surface the warning on
    //     stderr alongside the spurious findings. Joins the existing breadcrumb
    //     convention from lib.rs (git check-ignore) and p5_phantom_done.rs
    //     (runs.db unreadable).
    if ctx.target_task_id.is_none()
        && ctx.window.is_none()
        && ctx.task_metadata.len() > SWEEP_BACKLOG_WARN_THRESHOLD
    {
        eprintln!(
            "reify-audit: p2::check called with unbounded backlog \
             (target_task_id=None, window=None, {} tasks > threshold {}); \
             callers MUST pre-narrow ctx.task_metadata to closing-window \
             tasks per the # Callers rustdoc — else every in-progress WIP \
             `TODO(... pending)` will surface as a Medium-severity finding",
            ctx.task_metadata.len(),
            SWEEP_BACKLOG_WARN_THRESHOLD,
        );
        debug_assert!(
            false,
            "p2::check called with unbounded backlog \
             (target_task_id=None, window=None, {} tasks): callers MUST \
             pre-narrow ctx.task_metadata to closing-window tasks per the \
             # Callers rustdoc",
            ctx.task_metadata.len(),
        );
    }

    let mut findings = Vec::new();

    // NOTE: unlike `p5_phantom_done::check_task` (which filters `meta.status != "done"`
    //   to skip non-`done` tasks), P2 deliberately iterates EVERY task regardless of
    //   status. Reason: the D-1 pre-done hook calls into P2 *before* the orchestrator
    //   flips `status` from `in_progress` to `done`, so a `status != "done"` filter
    //   would suppress every finding on the primary call path (`check_pre_done`-style
    //   single-task narrowing via `target_task_id`).
    //
    //   Constraint on periodic-sweep callers (e.g. T-4 CLI in `--mode sweep`): they
    //   MUST narrow `ctx.task_metadata` to closing-window tasks themselves before
    //   calling `check`. Passing the full backlog will surface every in-progress
    //   task carrying a legitimate WIP `TODO(... pending)` as a finding — the marker
    //   is what P2 looks for, and there is no further filter inside this function.
    //
    //   Reference: `docs/architecture-audit/f-infra-design.md` §5 P2 and §10.
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

        // TODO(perf): coalesce paths per task into a single
        //   `git diff main..task/<id> -- <p1> <p2> ...` invocation instead of
        //   one subprocess per (task, file) — avoids N×M cost in production
        //   sweeps over hundreds of tasks. The `+++ b/path` hunk headers
        //   already delimit per-file sections in a multi-path diff output.
        //   Reference: docs/architecture-audit/f-infra-design.md §5 P2.
        //
        //   Additionally: `line_matches_stub` allocates a fresh `String` per
        //   added line via `to_lowercase()` (see line_matches_stub, top of
        //   function). When the per-task coalescing follow-up lands, consider
        //   reusing a per-task `String` scratch buffer via `clear();
        //   push_str(line); make_ascii_lowercase()` guarded by
        //   `line.is_ascii()`, falling back to `to_lowercase()` for the
        //   non-ASCII tail. This collapses the ASCII fast-path and
        //   scratch-buffer goals into one coherent strategy: zero allocations
        //   on all-ASCII input (the common case for Rust stub markers), one
        //   allocation per non-ASCII line (same cost as today). Note: do NOT
        //   call `make_ascii_lowercase()` on non-ASCII input without the
        //   `is_ascii()` guard — it silently skips non-ASCII bytes rather than
        //   case-folding them, changing semantics vs. `to_lowercase()`.
        for path in &meta.files {
            // Skip test-shaped paths to avoid false positives on intentional
            // stubs inside test helpers (design §5 P2 false-positive guards).
            if crate::is_test_path(path) {
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
                        // Use char-boundary-safe truncation: count Unicode scalar
                        // values rather than bytes so a multi-byte character that
                        // straddles byte 60 never causes a panic.
                        let snip = if snippet.chars().count() > 60 {
                            let head: String = snippet.chars().take(60).collect();
                            format!("{head}…")
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
