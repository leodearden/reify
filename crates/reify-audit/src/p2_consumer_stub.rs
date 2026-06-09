//! P2 — consumer-stub detector.
//!
//! Scans the added lines of a task's contribution to `metadata.files` for
//! canonical stub markers and emits Medium-severity findings (Low when the
//! task title contains "stub" or "placeholder").
//!
//! ## Per-task diff routing (reaped-branch resilient)
//!
//! The source of the `(line_no, content)` stream fed into
//! [`scan_file_added_lines`] is determined per-task as follows:
//!
//! 1. **`done_provenance.commit` present AND `is_ancestor(commit, "main")` →
//!    `diff_added_lines_in_commit(commit, path)`** — the surviving merge
//!    commit's first-parent diff (`<commit>^1..<commit>`) recovers the task's
//!    exact added lines even after the orchestrator has reaped the `task/N`
//!    branch.
//!
//! 2. **`done_provenance.commit` present but NOT reachable from `main`**
//!    (gc'd / recycled SHA) → `file_lines_on("main", path)` — last-resort
//!    recall via a full-file content scan on the current `main`. This is
//!    noisier (re-surfaces pre-existing stubs) but is gated strictly to the
//!    rare unreachable-commit case so it does not reintroduce the
//!    false-positive volume that dependency 4076 suppressed on the common path.
//!
//! 3. **No provenance commit** (in-progress / pre-done-hook task whose
//!    `task/N` branch is still alive) → `diff_added_lines("main", "task/N",
//!    path)` — the original behaviour, preserved unchanged so all existing
//!    tests (every fixture uses `done_provenance: None`) stay green and the
//!    dark-factory pre-done hook continues to work.
//!
//! The reachability gate (`is_ancestor`) is evaluated once per task (not per
//! path) to bound the extra `git merge-base --is-ancestor` invocation.
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
    // Doc-comment lines (/// and //!) describe API and never execute — suppress all families.
    let t = line.trim_start();
    if t.starts_with("///") || t.starts_with("//!") {
        return None;
    }

    // A pure `//` comment line (trimmed start begins with `//`). The
    // executable-code-token families (2 unimplemented!, 3 panic!(not yet),
    // 4 tracing::warn!, 5 Value::Undef) match executable syntax that should
    // not appear in a `//` comment context — such lines are prose *about* the
    // tokens, not executable arms. Family 1 (TODO variants) and Family 6 (bare
    // labels) are intentionally unchanged because they are definitionally comment
    // markers and are expected to appear in `//` lines.
    let is_comment_line = t.starts_with("//");

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

    // Families 2–5 are executable-code-token families. Skip them when the line
    // is a pure `//` comment (prose ABOUT the token, not an executable arm).
    if !is_comment_line {
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
    }

    // Family 6 — bare line-comment markers (case-insensitive, label-anchored).
    // A match requires that after "// word" the remainder is empty/whitespace or
    // starts with ':' — distinguishing a stub LABEL (`// stub`, `// placeholder: …`)
    // from PROSE where the word is a sentence subject (`// Placeholder is a leaf`).
    for (needle, label) in &[("// stub", "// stub"), ("// placeholder", "// placeholder"), ("// fixme", "// fixme")] {
        if let Some(pos) = lower.find(needle) {
            let after = &lower[pos + needle.len()..];
            if after.is_empty() || after.trim_start().is_empty() || after.trim_start().starts_with(':') {
                return Some(label);
            }
        }
    }

    None
}


/// Returns `true` when `path` has an executable-code extension that P2 scans.
/// Non-code files (.ri, .yaml, .md, .toml, etc.) carry design prose, not
/// consumer stubs, so they are excluded from P2 scanning.
fn is_code_ext(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".rs")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
}

/// Returns `true` when `trimmed_lower` (already lowercased, leading whitespace
/// stripped) is a `#[cfg(...)]` attribute that enables a **test-only** code path.
///
/// Matches:
/// - `#[cfg(test)]`
/// - `#[cfg(any(test, ...))]`
/// - `#[cfg(feature = "test-...")]` (feature name starting with "test")
///
/// Does **not** match:
/// - `#[cfg(not(test))]` — production-only guard; explicitly excluded so that
///   genuine production stubs following such an attribute are still flagged.
/// - Attributes where "test" appears only as a substring of another word in a
///   feature name, e.g. `#[cfg(feature = "fastest")]`: the token-boundary check
///   requires `test` to be preceded by `(`, `,`, or space on the left and
///   followed by `)`, `,`, or space on the right.
fn is_test_cfg_attr(trimmed_lower: &str) -> bool {
    if !trimmed_lower.starts_with("#[cfg(") {
        return false;
    }
    // #[cfg(not(test))] guards production-only code — must NOT suppress.
    if trimmed_lower.contains("not(test") {
        return false;
    }
    // Token-boundary check: "test" must appear as a standalone cfg-predicate
    // identifier, not as a substring of another word (e.g. "fastest").
    // Valid left boundaries: '(', ',', ' '.
    // Valid right boundaries: ')', ',', ' '.
    let b = trimmed_lower.as_bytes();
    let pat = b"test";
    let mut i = 0;
    while i + 4 <= b.len() {
        if &b[i..i + 4] == pat {
            let left_ok = i == 0 || matches!(b[i - 1], b'(' | b',' | b' ');
            let right_ok = i + 4 == b.len() || matches!(b[i + 4], b')' | b',' | b' ');
            if left_ok && right_ok {
                return true;
            }
        }
        i += 1;
    }
    // Also match feature="test-..." (feature name starting with "test", with or
    // without spaces around `=`), e.g. #[cfg(feature="test-support")].
    trimmed_lower.contains("feature = \"test") || trimmed_lower.contains("feature=\"test")
}

/// Scan a single file's added-line stream for stub markers, returning
/// `(line_no, content, label)` for each match.
///
/// Implements positional `#[cfg(test)]` gating: once a line whose trimmed
/// form is recognised as a test-cfg attribute (see [`is_test_cfg_attr`]), all
/// subsequent lines in that file's stream are suppressed. Lines before the gate
/// are still flagged (genuine production stubs). Safe because `diff_added_lines`
/// returns lines in file order and Rust convention places test modules last.
///
/// `#[cfg(not(test))]` (production-only guard) does **not** trigger the gate,
/// so genuine production stubs following it remain detected.
///
/// # Known limitation
///
/// The gate fires only when the `#[cfg(test)]` attribute line itself appears
/// among the *added* lines. If a task adds stub lines inside a **pre-existing**
/// inline test module (the `#[cfg(test)]` was already on `main` and thus not
/// in the diff), the gate never engages and those test-only stubs may be
/// reported as production stubs. `is_test_path` only protects dedicated test
/// files, not inline test modules in production `.rs` files. Accepted v1
/// behaviour; a full fix would require scanning the unmodified file head.
fn scan_file_added_lines(added: &[(usize, String)]) -> Vec<(usize, String, &'static str)> {
    let mut result = Vec::new();
    let mut in_cfg_test = false;
    for (line_no, content) in added {
        if !in_cfg_test {
            let trimmed_lower = content.trim_start().to_lowercase();
            if is_test_cfg_attr(&trimmed_lower) {
                in_cfg_test = true;
                continue;
            }
            if let Some(label) = line_matches_stub(content) {
                result.push((*line_no, content.clone(), label));
            }
        }
    }
    result
}

/// Returns `true` when the task title itself signals that the task is
/// intentionally a stub or placeholder (case-insensitive substring match).
/// Used to downgrade finding severity from Medium to Low.
fn title_signals_stub(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("stub") || t.contains("placeholder")
}

/// The detector's own source path, excluded from P2 scanning. The live code
/// `return Some("TODO(post-)")` at line 41 lowercases to contain `todo(post-)`
/// and self-matches Family 1 — excluding the file is the minimal correct fix.
const SELF_SOURCE_PATH: &str = "crates/reify-audit/src/p2_consumer_stub.rs";

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
///
/// Key: `(task_id, path)` → `Vec<(line_no, content, label)>`
type GroupMap = std::collections::HashMap<(String, String), Vec<(usize, String, &'static str)>>;

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
    // KNOWN LIMITATION (esc-3752-365 review, suggestion 1): The `window.is_none()`
    // conjunct is intended as a proxy for "no sweep scoping was requested", but
    // `ctx.window` is NOT consumed by P2 — the AuditContext::window rustdoc says
    // "None of the slice-1 detector paths consume this yet". The CLI also loads the
    // full fused-memory backlog regardless of --since (it only builds `window`,
    // never filters `task_metadata`). Consequently, a --since-scoped sweep
    // (window=Some but task_metadata still contains the full backlog) BYPASSES this
    // guard and still surfaces spurious findings. This guard therefore only catches
    // the zero-scoping-flag case (--mode sweep with neither --task nor --since).
    // Complete protection requires the CLI or loader to narrow `ctx.task_metadata`
    // at load time before calling check().
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

    // Phase 1 — collect all surviving (path, line_no, content, label) per task.
    //
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

    // Raw match: all fields needed for dedup and finding emission.
    struct RawEntry {
        path: String,
        line_no: usize,
        content: String,
        label: &'static str,
        task_id: String,
        /// Ordering key: (done_at_or_max, numeric_task_id_or_max).
        /// Smaller = introduced earlier = the introducer.
        sort_key: (i64, u64),
    }

    let mut raw: Vec<RawEntry> = Vec::new();

    for meta in ctx.task_metadata.values() {
        // Optional single-task narrowing (mirrors p5_phantom_done::check_with_target).
        if let Some(target) = ctx.target_task_id.as_deref()
            && meta.task_id != target
        {
            continue;
        }

        let task_branch = format!("task/{}", meta.task_id);
        let sort_key = (
            meta.done_at.unwrap_or(i64::MAX),
            meta.task_id.parse::<u64>().unwrap_or(u64::MAX),
        );

        // Resolve the per-task routing mode once (not per path) to bound the
        // extra `git merge-base --is-ancestor` invocation.
        //
        // `provenance_commit` = raw commit SHA from done_provenance (may be
        // Some even for gc'd SHAs).
        // `resolved_commit`   = Some(sha) only when the SHA is actually
        // reachable from "main" (is_ancestor gate).
        //
        // NOTE (transient-error routing): `is_ancestor` is fail-safe — it
        // returns `false` on ANY git error (lock contention, corrupted index,
        // spawn failure) in addition to the legitimate "not an ancestor" case.
        // When `provenance_commit` is Some but `is_ancestor` transiently
        // errors, `resolved_commit` becomes `None` and routing falls to branch
        // (2) — full-file content scan — rather than branch (3) — branch diff.
        // Branch (2) is noisier: it re-surfaces pre-existing stubs that may be
        // mis-attributed to this task. The accepted mitigation is that branch
        // (2) is gated strictly to tasks with a provenance commit (the rare
        // gc'd-SHA case plus transient errors), so it cannot reintroduce
        // 4076's false-positive volume on the common reachable-commit path.
        // A more robust fix — having `is_ancestor` return `Option<bool>` to
        // distinguish an error from a genuine "not an ancestor" — would route
        // transient errors to branch (3) instead, preserving the original
        // branch-diff behaviour rather than escalating to the noisy content
        // scan. That change is recorded as a known limitation; it requires a
        // trait-breaking refactor that is out of scope for this task.
        let provenance_commit: Option<&str> =
            meta.done_provenance.as_ref().and_then(|p| p.commit.as_deref());
        let resolved_commit: Option<&str> = provenance_commit
            .filter(|c| ctx.git.is_ancestor(c, "main"));

        // TODO(perf): coalesce paths per task into a single
        //   `git diff main..task/<id> -- <p1> <p2> ...` invocation.
        //   Reference: docs/architecture-audit/f-infra-design.md §5 P2.
        for path in &meta.files {
            if crate::is_test_path(path) { continue; }
            if !is_code_ext(path) { continue; }
            if path == SELF_SOURCE_PATH { continue; }

            // Three-way routing — see module-level doc for rationale.
            let added = match (resolved_commit, provenance_commit) {
                // (1) Merge commit reachable from main → first-parent diff.
                (Some(commit), _) => ctx.git.diff_added_lines_in_commit(commit, path),
                // (2) Provenance commit present but not reachable (gc'd / recycled)
                //     → full-file content scan as last-resort recall.
                //     Documented precision/recall tradeoff; gated to this rare branch
                //     so it does not reintroduce 4076's false-positive volume on the
                //     common reachable-commit path.
                (None, Some(_)) => ctx.git.file_lines_on("main", path),
                // (3) No provenance commit (in-progress / pre-done-hook; task/N alive)
                //     → original branch diff. Preserves all existing behaviour.
                (None, None) => ctx.git.diff_added_lines("main", &task_branch, path),
            };
            for (line_no, content, label) in scan_file_added_lines(&added) {
                raw.push(RawEntry {
                    path: path.clone(),
                    line_no,
                    content,
                    label,
                    task_id: meta.task_id.clone(),
                    sort_key,
                });
            }
        }
    }

    // Phase 2 — de-dup by (path, line_no, content), keeping the introducer
    // (smallest sort_key = earliest done_at; tie-break ascending numeric task_id).
    // Window-wide attribution re-reports a shared location under every task whose
    // diff surfaces it; collapsing here attributes each location exactly once.
    let mut dedup: std::collections::HashMap<(String, usize, String), RawEntry> =
        std::collections::HashMap::new();
    for entry in raw {
        let key = (entry.path.clone(), entry.line_no, entry.content.clone());
        let keep = match dedup.get(&key) {
            None => true,
            Some(existing) => entry.sort_key < existing.sort_key,
        };
        if keep {
            dedup.insert(key, entry);
        }
    }

    // Phase 3 — group winners by (task_id, path).
    let mut groups: GroupMap = std::collections::HashMap::new();
    for (_, entry) in dedup {
        groups
            .entry((entry.task_id, entry.path))
            .or_default()
            .push((entry.line_no, entry.content, entry.label));
    }

    // Phase 4 — emit one Finding per (task_id, path); sort for determinism.
    let mut findings = Vec::new();
    let mut group_keys: Vec<(String, String)> = groups.keys().cloned().collect();
    group_keys.sort();

    for (task_id, path) in group_keys {
        let mut matches = groups.remove(&(task_id.clone(), path.clone())).unwrap();
        matches.sort_by_key(|(ln, _, _)| *ln);

        // Phase-4 severity: Low when (a) the task title signals stub/placeholder,
        // OR (b) every match in the group is a `// fixme` maintenance label
        // (documented permanent maintenance trap, weaker signal than `// stub`
        // or `// placeholder`). Mixed groups (fixme + other families) stay Medium.
        let severity = ctx.task_metadata
            .get(&task_id)
            .map(|m| {
                if title_signals_stub(&m.title) {
                    Severity::Low
                } else if matches.iter().all(|(_, _, label)| *label == "// fixme") {
                    Severity::Low
                } else {
                    Severity::Medium
                }
            })
            .unwrap_or(Severity::Medium);

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
            task_id,
            summary,
            evidence: vec![EvidenceRef::File { path }],
        });
    }

    findings
}
