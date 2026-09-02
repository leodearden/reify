//! P5 — phantom-done detector.
//!
//! A task is "phantom-done" when `metadata.status == "done"` but its claimed
//! provenance commit cannot be corroborated against runs.db / `git log main`.
//! Slice-1 (T-1) ships the corroboration core only; T-4 will wire the CLI
//! that loads `tasks.json` into [`crate::TaskMetadata`] and invokes [`check`]
//! and [`check_pre_done`].
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §10 (T-1) and §11
//! (D-1 dependency row).

use crate::{AuditContext, ChangedSymbol, EvidenceRef, Finding, GitCommit, Pattern, Severity, TaskMetadata};
use std::collections::HashMap;

// Empty/vacuous assertion patterns scanned for by H1 (gate b).
// Each is matched as a substring of added lines within a fn body.
const EMPTY_ASSERTION_PATTERNS: &[&str] = &[
    ".is_empty()",
    "vec![]",
    "Vec::new()",
    "assert_eq!(result, 0)",
    "assert_eq!(result, [])",
    "assert_eq!(0,",
    "assert_eq!([], ",
    "assert_eq!(vec![]",
    "assert_eq!(Vec::new()",
];

// Placeholder/stub markers for H1 fn-name gate (gate a, case-insensitive).
// A test fn name containing any of these signals a deliberately-placeholder
// test rather than a legitimate empty-result test (design caveat task 4140).
//
// NOTE: "empty" is intentionally NOT in this list. Many legitimate test names
// contain the word 'empty' as a domain noun (e.g. `handles_empty_input`,
// `returns_error_on_empty_list`, `empty_collection_is_valid`). Including it
// would generate false positives for tests that correctly assert an empty result
// for an empty input — exactly the class of legitimate test the double-gate is
// designed to spare. The concrete incident fn name
// `activate_expands_geometric_params_placeholder_to_empty_list` still triggers
// via the stronger "placeholder" marker. Per task 4140 §FP-control.
const PLACEHOLDER_MARKERS: &[&str] = &[
    "placeholder",
    "not_yet",
    "notyet",
    "stub",
    "todo",
    "unimplemented",
];

// Empty-intent tokens for H1 fn-name gate (gate c, the third signal added in
// task 4141 to harden against domain-noun false positives).
//
// A test fn name must contain at least one of these tokens in addition to a
// PLACEHOLDER_MARKERS match before the body-empty-assertion gate (gate b) is
// armed. This three-signal gate suppresses domain-noun FPs observed in the
// live corpus during task 4141's validation sweep:
//
//   - `tessellate_sentinel_placeholder_continues_independent_ops`: carries
//     "placeholder" as a geometry-sentinel noun; no empty-intent token → NOT
//     flagged.
//   - `stub_kernel_export_returns_error`: carries "stub" as a kernel-module
//     noun; no empty-intent token → NOT flagged.
//
// The genuine incident pattern still fires:
//   - `activate_expands_geometric_params_placeholder_to_empty_list`: carries
//     both "placeholder" (marker, gate a) AND "empty" (empty-intent, gate c)
//     in its name → still flagged.
//
// Token design rationale:
// - "empty", "none", "nil", "zero", "vacuous", "nothing" are chosen as
//   unambiguous empty-result-intent indicators that do not collide as
//   substrings with common identifiers (e.g. "nil" is not in "until";
//   "none" is not in "independent" or "continues").
// - "no_" (with trailing underscore) is included to match `no_results`,
//   `no_items`, `no_warnings` etc. while excluding common fragments like
//   "independent", "canonical", "not_yet" that don't contain the "no_" bigram.
//
// Precision/recall tradeoff: a masking test whose name carries a marker but
// lacks any empty-intent noun would be missed by this gate. This is the
// correct bias for a visibility-only Medium signal — low-confidence signals
// stay suppressable. Broader tuning (word-boundary marker matching, etc.) is
// filed as a follow-up. Per task 4141 live-corpus FP validation; see
// docs/prds/p5-h1-h2-live-corpus-fp-validation.md.
const EMPTY_INTENT_NAME_TOKENS: &[&str] = &[
    "empty",
    "none",
    "nil",
    "zero",
    "vacuous",
    "nothing",
    "no_",
];

/// Returns `true` when `line` contains a vacuous empty-assertion pattern
/// (gate b of the H1 double-gate), with one exception: a `.is_empty()` that is
/// part of a negated expression (e.g. `assert!(!result.is_empty())`) does NOT
/// satisfy the gate — asserting non-empty is not a vacuous assertion.
///
/// Negation detection: strip identifier characters (word chars) from the end
/// of the text before `.is_empty()`; if what remains ends with `!`, the call
/// is negated. This correctly handles `!result.is_empty()` (where `!` precedes
/// the receiver, not the dot) while not mistaking `assert!(result.is_empty()`
/// (which ends with `(` after stripping `result`) for a negation.
fn line_has_empty_assertion(line: &str) -> bool {
    for pat in EMPTY_ASSERTION_PATTERNS {
        let Some(pos) = line.find(pat) else {
            continue;
        };
        // Special-case: detect negated `.is_empty()` — asserting NON-empty.
        // Strip word characters from the end of the text before `.is_empty()`.
        // If what remains ends with `!`, the receiver was negated (e.g.
        // `!result.is_empty()`). Does not catch chained calls like
        // `!x.to_vec().is_empty()` (rare in tests; accepted limitation).
        if *pat == ".is_empty()" {
            let before_trimmed = line[..pos].trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
            if before_trimmed.ends_with('!') {
                continue; // negated: asserting non-empty
            }
        }
        return true;
    }
    false
}

/// Extract the function name from a line that is a Rust `fn` declaration.
/// Returns `None` when the line is not a function declaration. Anchors to the
/// start of the non-whitespace content so that `fn ` occurring inside a doc
/// comment, string literal, or another identifier context is NOT mistakenly
/// treated as a declaration boundary.
///
/// Accepted leading patterns (after stripping leading whitespace):
/// - `fn ` / `pub fn ` / `async fn ` / `pub async fn `
/// - `pub(<vis>) fn ` (e.g. `pub(crate) fn`, `pub(super) fn`)
///
/// Suggestion 4 from the code review (task 4140 amendment pass).
fn extract_fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    // Determine whether this trimmed line begins with a fn declaration.
    let is_fn_decl = trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("async fn ")
        || trimmed.starts_with("pub async fn ")
        || (trimmed.starts_with("pub(") && trimmed.contains(") fn "));
    if !is_fn_decl {
        return None;
    }
    // Find `fn ` within the (already-anchored) trimmed line and extract the name.
    let fn_kw_pos = trimmed.find("fn ")?;
    let after_fn = &trimmed[fn_kw_pos + 3..];
    let name = after_fn.split('(').next()?.trim().to_lowercase();
    if name.is_empty() { None } else { Some(name) }
}

/// The git ref the detector diffs claimed commits *against*. Production runs
/// against `main`; the integration tests configure their `MockGitOps` with
/// this exact string so the keys line up.
const MAIN_BASE: &str = "main";

/// Which caller a per-task pass is running for.
///
/// The two callers observe genuinely different task states, so a handful of
/// guards must diverge — see [`check_task`] (status) and [`check_one`]
/// (provenance). Threaded as a parameter rather than added to
/// [`AuditContext`] deliberately: a context field would force a mechanical
/// edit at every one of the ~40 test construction sites for no behavioural
/// gain, and would make the mode look like ambient configuration rather than
/// a property of the call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CheckMode {
    /// The periodic sweep ([`check`] / `check_with_target`). Reads persisted,
    /// post-transition state: `status == "done"` and `done_provenance` written.
    Sweep,
    /// The D-1 pre-done hook ([`check_pre_done`]). Reads pre-transition state:
    /// the flip has not been written yet.
    PreDone,
}

/// Production SQL used by [`has_task_completed_event`] to corroborate a
/// merged task's `task_completed` event in runs.db. Hoisted to a `pub const`
/// so the integration test `p5::tests::runs_db_schema_pin` can pin the test
/// schema against the exact string the detector executes — preventing schema
/// and query drift.
///
/// # Visibility note
/// This constant is exposed as `pub` solely to allow the integration test
/// `p5::tests::runs_db_schema_pin` (a separate compilation unit) to reference
/// it. It is **not** part of the stable public API of this crate;
/// `#[doc(hidden)]` removes it from rendered rustdoc and IDE autocomplete
/// while keeping it linkable from the separate-crate integration test.
#[doc(hidden)]
pub const PRODUCTION_QUERY: &str =
    "SELECT 1 FROM events WHERE task_id = ? AND event_type = 'task_completed' LIMIT 1";

/// Run the P5 detector across every `status="done"` task in
/// `ctx.task_metadata`. Returns one [`Finding`] per phantom-done task.
///
/// Slice-1 corroboration logic, per `f-infra-design.md` §10:
/// 1. **Primary**: `git diff main..<claimed_commit>` must cover every path
///    in `metadata.files`. For `kind="merged"`, runs.db must additionally
///    contain a `task_completed` event for the task.
/// 2. **Cargo.lock-only guard** (memory:
///    `project_post_merge_equivalence_false_positive_cargo_lock.md`):
///    if the lone missing entry is `Cargo.lock`, downgrade to Low.
/// 3. **Convergent-FF rescue** (memory:
///    `project_unblock_convergent_ff_worktree_reap.md`): if
///    `git log main --grep <task_id>` returns sibling commits whose
///    aggregated diff covers the entire missing set, downgrade to Low and
///    cite each contributing sibling SHA via `EvidenceRef::Commit`.
///
/// Mismatches that survive both guards produce `Severity::High`.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    check_with_target(ctx, ctx.target_task_id.as_deref())
}

/// Single-task entry point for the D-1 dark-factory pre-done hook
/// (`docs/architecture-audit/f-infra-design.md` §3 + §11). Scopes the
/// detector to one `task_id` so the orchestrator can call us synchronously
/// before flipping a task to `done` without auditing its entire backlog.
///
/// Hot path: D-1 fires on every status flip. Direct HashMap lookup keeps
/// this wrapper O(1) rather than the O(n) linear scan that `check_with_target`
/// does across all rows.
///
/// # Two-mode contract
///
/// **With persisted `done_provenance`** (a direct caller, or a task whose
/// provenance was already written) it corroborates exactly as the sweep does —
/// `check_pre_done_equivalent_to_scoped_check` pins full `Finding` equality.
///
/// **Without it** — which at real hook time is EVERY invocation — it
/// corroborates landing from `task_id` + `metadata.files` via
/// [`check_pre_done_landing`]. Two upstream facts force that:
///
///   - fused-memory's `task_interceptor.py` accumulates `done_provenance` in an
///     in-memory `audit_fields` dict and persists it only at write time, which
///     happens AFTER this hook returns;
///   - the hook command template (`middleware/pre_done_hook.py`) substitutes
///     only `{id}` — no `{provenance}`/`{commit}`/`{files}` placeholder exists,
///     and the subprocess is launched with no env injection and no stdin.
///
/// So the subprocess receives no task state beyond the id. For the same reason
/// [`check_task`] skips its `status == "done"` gate in this mode: the hook fires
/// before the status write, so the live row still reads "in-progress"/"review".
///
/// Slice-1 ships the wrapper; T-4 will host the CLI subprocess that the
/// hook actually invokes.
pub fn check_pre_done(ctx: &AuditContext, task_id: &str) -> Vec<Finding> {
    let Some(meta) = ctx.task_metadata.get(task_id) else {
        return vec![];
    };
    check_task(ctx, meta, CheckMode::PreDone)
}

/// Inner loop for the [`check`] periodic-sweep entry point. Iterates all
/// `status="done"` tasks in `ctx.task_metadata`, optionally restricted to
/// `target_task_id` when the caller supplies a scoped sweep.
///
/// [`check_pre_done`] deliberately does NOT route through this function — it
/// uses a direct O(1) `ctx.task_metadata.get(task_id)` HashMap lookup so the
/// D-1 hot path stays constant-time rather than paying the O(n) iteration cost
/// of this loop. Borrows the context (no clone of `task_metadata`).
fn check_with_target(ctx: &AuditContext, target_task_id: Option<&str>) -> Vec<Finding> {
    let mut findings = Vec::new();

    for meta in ctx.task_metadata.values() {
        if let Some(target) = target_task_id
            && meta.task_id != target
        {
            continue;
        }

        findings.extend(check_task(ctx, meta, CheckMode::Sweep));
    }

    findings
}

/// Per-task pass set shared by [`check_pre_done`] (D-1 hot path, O(1) lookup)
/// and the inner loop of [`check_with_target`] (periodic sweep, O(n) iteration).
/// Centralising the pass list here prevents drift when future per-task detectors
/// join the per-task pass set — they get added in exactly one place.
fn check_task(ctx: &AuditContext, meta: &TaskMetadata, mode: CheckMode) -> Vec<Finding> {
    // Sweep only. The sweep audits tasks that are ALREADY done, so a non-done
    // row has nothing to corroborate.
    //
    // The pre-done hook deliberately skips this gate: it fires at fused-memory
    // `task_interceptor.py` step "2d", BEFORE the status write, so the live
    // `get_task` it reads still returns the pre-transition status
    // ("in-progress"/"review"). Requiring `status == "done"` here made the gate
    // structurally unable to fire on the one transition it exists to guard —
    // every pre-done invocation returned `[]` unconditionally.
    if mode == CheckMode::Sweep && meta.status != "done" {
        return vec![];
    }
    // ONE `git check-ignore` fork per declared file for the whole task. Both
    // `check_gitignored` (which needs the full set as its finding payload) and
    // the pre-done landing leg (which subtracts it from the declared set) ask
    // the same question about the same paths; computing it here makes the
    // second consumer free. That matters on the pre-done path specifically —
    // it runs inside fused-memory's per-project write lock under a 30 s hard
    // timeout, so a duplicated per-file fork is paid by every task mutation
    // for the project.
    let gitignored: Vec<String> = meta
        .files
        .iter()
        .filter(|p| ctx.git.is_gitignored(p))
        .cloned()
        .collect();

    let mut findings = Vec::new();
    if let Some(f) = check_one(ctx, meta, mode, &gitignored) {
        findings.push(f);
    }
    if let Some(f) = check_gitignored(meta, &gitignored) {
        findings.push(f);
    }
    findings.extend(check_tests_assert_empty(ctx, meta));
    findings.extend(check_live_path_stranded(ctx, meta));
    findings
}

/// H1 — tests-assert-empty pass (three-signal gate, task 4141 precision hardening).
///
/// For each test-path entry in `metadata.files`, reads the added lines via
/// `GitOps::diff_added_lines_in_commit(commit, path)` and emits a
/// `P5TestsAssertEmpty` `Medium` finding ONLY when an added test fn satisfies
/// ALL THREE signals:
///
/// (a) carries a placeholder/not_yet/notyet/stub/todo/unimplemented marker in
///     its fn name (case-insensitive substring match; see `PLACEHOLDER_MARKERS`);
/// (c) carries an empty-intent token (e.g. "empty", "none", "nil", "zero",
///     "no_") in its fn name (see `EMPTY_INTENT_NAME_TOKENS`);
/// (b) has an empty/vacuous assertion within that fn's added lines
///     (see `EMPTY_ASSERTION_PATTERNS` and [`line_has_empty_assertion`]).
///
/// The three-signal gate suppresses the live-corpus domain-noun false positives
/// identified in task 4141's validation sweep (e.g. `tessellate_sentinel_
/// placeholder_continues_independent_ops`, `stub_kernel_export_returns_error`)
/// while preserving recall for the genuine incident pattern
/// `activate_expands_geometric_params_placeholder_to_empty_list` (carries BOTH
/// "placeholder" and "empty" in its name). Design caveat: task 4140 §FP-control;
/// task 4141 live-corpus validation; see
/// docs/prds/p5-h1-h2-live-corpus-fp-validation.md.
///
/// ## 4141 live-corpus validation note
///
/// Task 4141 ran a live H1 sweep and found a non-zero, partly-irreducible FP
/// rate from domain-noun marker usages in the corpus (53 test fns containing
/// "stub" and 25 containing "placeholder" as domain nouns). The third signal
/// (gate c: name-empty-intent) reduces this FP class substantially while
/// preserving the genuine incident pattern. H1 remains at `Severity::Medium`
/// (non-blocking for the D-1 hook) pending a fresh post-refinement NON-vacuous
/// validation sweep; see docs/prds/p5-h1-h2-live-corpus-fp-validation.md §6
/// for the promotion criteria a future task must meet.
///
/// Fn-declaration detection is anchored to the start of the non-whitespace
/// content of the line (via [`extract_fn_name`]) to avoid spurious matches on
/// `fn ` inside doc comments, string literals, or other non-declaration
/// contexts.
///
/// Skipped when `done_provenance.commit` is absent (no commit to diff).
///
/// # Known limitation
///
/// H1 only fires when the `fn <name>(` declaration line itself appears among
/// the commit's added lines. If a developer adds assertion lines into a
/// pre-existing placeholder fn (signature unchanged), `current_fn_name` remains
/// `None` and the heuristic silently misses the case. This is an accepted
/// limitation: closing the gap would require a `GitOps::read_file_at_commit`
/// seam not currently available. The incident fixtures all add the whole fn,
/// confirming the heuristic covers the target pattern. Per task 4140
/// §H1-known-limitations.
fn check_tests_assert_empty(ctx: &AuditContext, meta: &TaskMetadata) -> Vec<Finding> {
    let Some(commit) = meta.done_provenance.as_ref().and_then(|p| p.commit.as_deref()) else {
        return vec![];
    };

    let mut findings = Vec::new();
    for path in &meta.files {
        if !crate::is_test_path(path) {
            continue;
        }
        let added = ctx.git.diff_added_lines_in_commit(commit, path);
        // Walk added lines tracking the current fn name.
        // State machine: once we see an anchored fn declaration, we record the
        // lowercased fn name until the next declaration, accumulating the fn's
        // added lines. A placeholder-named fn whose accumulated lines contain a
        // vacuous assertion (and is not a negated non-empty assertion) fires the
        // finding. Uses extract_fn_name to anchor detection to declaration lines
        // only, and line_has_empty_assertion to exclude negated .is_empty() calls.
        let mut current_fn_name: Option<String> = None;
        let mut fn_has_placeholder = false;
        let mut fn_has_empty_intent = false;
        let mut fn_has_empty_assertion = false;
        let mut found_in_file = false;

        for (_, line) in &added {
            // Detect a new fn declaration (anchored to declaration start).
            if let Some(fn_name) = extract_fn_name(line) {
                // Flush the previous fn if it triggered all three gates.
                if fn_has_placeholder && fn_has_empty_intent && fn_has_empty_assertion {
                    found_in_file = true;
                }
                fn_has_placeholder = PLACEHOLDER_MARKERS.iter().any(|m| fn_name.contains(m));
                fn_has_empty_intent = EMPTY_INTENT_NAME_TOKENS.iter().any(|t| fn_name.contains(t));
                fn_has_empty_assertion = false;
                current_fn_name = Some(fn_name);
            }

            // Within a fn, check for vacuous assertions (excluding negated !is_empty()).
            // Gate c (empty-intent name check) must also pass before we arm gate b.
            if current_fn_name.is_some()
                && fn_has_placeholder
                && fn_has_empty_intent
                && line_has_empty_assertion(line)
            {
                fn_has_empty_assertion = true;
            }
        }
        // Flush the last fn.
        if fn_has_placeholder && fn_has_empty_intent && fn_has_empty_assertion {
            found_in_file = true;
        }

        if found_in_file {
            findings.push(Finding {
                pattern: Pattern::P5TestsAssertEmpty,
                severity: Severity::Medium,
                task_id: meta.task_id.clone(),
                summary: format!(
                    "added test in {} carries a placeholder fn name AND empty-intent token \
                     AND asserts an empty/vacuous result — possible placeholder test masking \
                     a not-yet-implemented capability (task 4141 H1 three-signal gate)",
                    path
                ),
                evidence: vec![EvidenceRef::File { path: path.clone() }],
            });
        }
    }
    findings
}

/// Independent pre-pass: any metadata.files entry that's gitignored gets
/// flagged with one consolidated `Severity::Medium` finding per task. The
/// corroboration check above doesn't filter these out because the
/// gitignored path may legitimately appear in the diff (e.g. tree-sitter
/// generated `parser.c` is committed at vendor sync time but ignored in
/// normal workflow). Memory: project_steward_metadata_files_gitignore_falsepositive.md.
///
/// Takes the gitignored subset precomputed once per task by [`check_task`]
/// rather than re-forking `git check-ignore` per file — see the comment at that
/// call site.
fn check_gitignored(meta: &TaskMetadata, ignored: &[String]) -> Option<Finding> {
    if ignored.is_empty() {
        return None;
    }
    Some(Finding {
        pattern: Pattern::P5MetadataFilesGitignored,
        severity: Severity::Medium,
        task_id: meta.task_id.clone(),
        summary:
            "metadata.files contains gitignored entry — strip per \
             project_steward_metadata_files_gitignore_falsepositive.md"
                .to_string(),
        evidence: vec![EvidenceRef::MetadataFiles {
            entries: ignored.to_vec(),
        }],
    })
}

/// Returns `true` when every `meta.files` entry is tracked on `MAIN_BASE`
/// (via [`crate::GitOps::path_tracked_on`]) AND the files list is non-empty.
///
/// The non-empty guard prevents false-low downgrades for tasks with no declared
/// deliverables. Used by both the merged-arm (b) rescue and the git-diff-leg
/// deliverable-presence rescue so the corroboration predicate lives in one place
/// and cannot drift between the two sites.
fn all_files_tracked_on_main(ctx: &AuditContext, meta: &TaskMetadata) -> bool {
    !meta.files.is_empty() && meta.files.iter().all(|p| ctx.git.path_tracked_on(MAIN_BASE, p))
}

/// Verdict of the substring-collision filter for ONE `log_grep` hit.
///
/// Three-valued on purpose. `git log --grep` matches the WHOLE commit message
/// (subject + body + trailers), but [`crate::LOG_GREP_FORMAT`] (`%H%x09%s`)
/// carries only the subject — so for a hit whose subject does not contain the
/// id at all, the match necessarily came from text this process never sees and
/// the collision question is simply **unanswerable** from the data at hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubjectMatch {
    /// The subject contains the id with at least one non-ASCII-digit boundary.
    WholeNumber,
    /// The subject contains the id, but EVERY occurrence is digit-adjacent —
    /// i.e. it is part of a different, longer number ("5937" for id "593").
    DigitCollision,
    /// The subject does not contain the id at all: git matched on the body or a
    /// trailer, which `log_grep` does not return.
    NotInSubject,
}

/// Classify how `subject` references `task_id`, for the substring-collision
/// filter. See [`SubjectMatch`] for the three outcomes.
///
/// Filtering Rust-side (rather than changing the `--grep` pattern) keeps the
/// [`crate::GitOps::log_grep`] contract unchanged, works identically for
/// `MockGitOps`, and avoids depending on git's BRE/ERE word-boundary support.
///
/// Only DIGIT neighbours disqualify a match, so non-numeric ids ("H1T1",
/// "REGR1", "6345D") behave exactly as before — the criterion is "is this a
/// different, longer number", not "is this a word boundary".
fn classify_subject_match(subject: &str, task_id: &str) -> SubjectMatch {
    if task_id.is_empty() {
        return SubjectMatch::NotInSubject;
    }
    let mut seen_any = false;
    for (pos, m) in subject.match_indices(task_id) {
        seen_any = true;
        let before_ok = subject[..pos]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_digit());
        let after_ok = subject[pos + m.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_digit());
        if before_ok && after_ok {
            return SubjectMatch::WholeNumber;
        }
    }
    if seen_any {
        SubjectMatch::DigitCollision
    } else {
        SubjectMatch::NotInSubject
    }
}

/// True iff a `log_grep` hit should still count as task-referencing.
///
/// Drops ONLY the hits that are demonstrably collisions: the subject contains
/// the id and every occurrence sits inside a longer number. A hit whose subject
/// does not mention the id at all is KEPT — git matched it on the body or a
/// trailer, and dropping it would silently narrow the filter from "reject
/// digit-collisions" into "reject every body-only reference", turning a
/// legitimate landing commit into a false High (sweep) or a wrongful refusal
/// (pre-done gate). MEASURED on the live repo: `git log main --grep=6200`
/// returns `1881ede9ac docs(6211): …` and `09de21ab8e Merge main into
/// task/6211`, neither of whose subjects contains `6200`.
///
/// # Known limitation (deliberate)
///
/// The residual case — a body-only reference that is ALSO a digit collision —
/// is not adjudicable here, because [`crate::LOG_GREP_FORMAT`] never carries
/// the body. Such a hit is kept, exactly as it was before the collision filter
/// existed. Closing it would mean widening `GitCommit` with the message body
/// (a public-struct change) or pushing an ERE at the `--grep` seam; both were
/// judged out of proportion to a rescue leg that only ever downgrades or
/// corroborates.
fn hit_references_task(subject: &str, task_id: &str) -> bool {
    classify_subject_match(subject, task_id) != SubjectMatch::DigitCollision
}

/// `git log <MAIN_BASE> --grep=<task_id>`, with substring collisions removed.
///
/// The single entry point for every `log_grep`-based rescue, so the sites
/// cannot drift on what counts as a task-referencing commit.
///
/// Fail-safe: an empty vec on a git error, which in the SWEEP simply means no
/// rescue is available. The pre-done gate must call
/// [`try_task_referencing_commits`] instead — there, an empty candidate list is
/// the first half of a blocking refusal.
fn task_referencing_commits(ctx: &AuditContext, task_id: &str) -> Vec<GitCommit> {
    try_task_referencing_commits(ctx, task_id).unwrap_or_default()
}

/// [`task_referencing_commits`] preserving the git error, for callers that
/// must not read "git failed" as "no commit references this task".
fn try_task_referencing_commits(
    ctx: &AuditContext,
    task_id: &str,
) -> Result<Vec<GitCommit>, String> {
    Ok(ctx
        .git
        .try_log_grep(MAIN_BASE, task_id)?
        .into_iter()
        .filter(|c| hit_references_task(&c.subject, task_id))
        .collect())
}

/// Per-invocation memo for `git merge-base --is-ancestor <commit> <MAIN_BASE>`.
///
/// The single place the `is_ancestor` fork is issued within one [`check_one`],
/// so the same SHA is never re-forked: the merged-arm rescue tests
/// `prov.commit` and then the primary git-diff leg (via
/// [`changed_paths_for_claim`], which remains the sole ancestry *policy*
/// decision point) tests it again, and a sibling SHA can repeat across legs.
/// One `HashMap` lookup replaces a ~57 ms fork, which matters most on the
/// pre-done path (held inside fused-memory's per-project write lock).
///
/// Deliberately NOT pre-seeded BY DEFAULT. `git log <MAIN_BASE> --grep=…`
/// lists only commits reachable from `MAIN_BASE`, so a `log_grep`-derived SHA
/// is an ancestor by construction — but baking that in here would move the
/// ancestry decision out of [`changed_paths_for_claim`] and silently change
/// behaviour for every caller (including `MockGitOps`) whose `log_grep`
/// answers are not real ancestors. Instead, a call site that can prove the
/// invariant LOCALLY asserts it via [`AncestryCache::prime`]; the only such
/// caller is [`check_pre_done_landing`], where the whole candidate list is
/// `log_grep`-derived and the fork cost is paid inside a held write lock.
#[derive(Default)]
struct AncestryCache(HashMap<String, bool>);

impl AncestryCache {
    /// Record a known ancestry answer without forking.
    ///
    /// For a caller that can prove the answer from where the SHA came from.
    /// Misuse is a correctness bug, not a performance one: a wrong `true`
    /// sends [`changed_paths_for_claim`] down the `<commit>^1..<commit>` arm
    /// for a commit that is not on main. See the invariant named at the one
    /// call site in [`check_pre_done_landing`].
    fn prime(&mut self, commit: &str, is_ancestor: bool) {
        self.0.insert(commit.to_string(), is_ancestor);
    }

    fn is_ancestor_of_main(&mut self, ctx: &AuditContext, commit: &str) -> bool {
        if let Some(&known) = self.0.get(commit) {
            return known;
        }
        let answer = ctx.git.is_ancestor(commit, MAIN_BASE);
        self.0.insert(commit.to_string(), answer);
        answer
    }
}

/// The set of paths a claimed commit contributes, choosing the diff base by
/// ancestry.
///
/// `main..<commit>` is a two-point TREE diff. Once `<commit>` is an ancestor of
/// main the two trees agree on exactly the paths the commit introduced, so the
/// task's own files are EXCLUDED by construction and the leg can never
/// corroborate a landed task — what comes back is the reverse-delta of whatever
/// landed afterwards. MEASURED on the live repo: for merge `bc8f74a4d4`,
/// `main..M` returned 6 paths and all six of that task's own files were absent
/// from the set. Post-merge the correct question is "what did this commit
/// change", i.e. `<commit>^1..<commit>`.
///
/// The ancestry answer goes through `ancestry` ([`AncestryCache`]) so a SHA
/// tested by an earlier leg of the same `check_one` costs a map lookup rather
/// than a second `git merge-base` fork.
fn changed_paths_for_claim(
    ctx: &AuditContext,
    commit: &str,
    ancestry: &mut AncestryCache,
) -> Vec<String> {
    if ancestry.is_ancestor_of_main(ctx, commit) {
        ctx.git.changed_paths_in_commit(commit)
    } else {
        // Pre-merge D-1 case: `<commit>` is an un-landed branch tip, so
        // `main..<tip>` IS the branch delta and is correct.
        ctx.git.diff_changed_paths(MAIN_BASE, commit)
    }
}

/// Provenance-free landing corroboration for the D-1 pre-done hook.
///
/// Reached only from [`check_one`] under [`CheckMode::PreDone`] when
/// `done_provenance` is absent — which, at hook time, is *every* invocation
/// (the interceptor persists provenance only after the hook returns). With no
/// claimed commit and no persisted status to read, the only evidence available
/// is the task's own id and its declared `metadata.files`.
///
/// Ordered cheapest-first, because the hook runs inside fused-memory's
/// per-project write lock: every leg here delays every task mutation for the
/// project.
///
/// Returns `Some` (a refusal) only when a declared deliverable is genuinely
/// unaccounted for on main.
///
/// # Deliberate divergence from the sweep
///
/// Gitignored entries are dropped from the declared set here, whereas the
/// sweep's `genuinely_absent` computation deliberately KEEPS them (see the
/// comment above the deliverable-presence rescue in [`check_one`]). Do not
/// "unify" the two: a gitignored path can never resolve on main by
/// construction, so blocking a state transition on one is a guaranteed false
/// positive, and refusing a transition is a far costlier error than a Low
/// sweep finding. The gitignored aspect already has its own channel — the
/// `P5MetadataFilesGitignored` Medium from [`check_gitignored`]. Memory:
/// `project_steward_metadata_files_gitignore_falsepositive.md`.
///
/// # Refuse only on evidence actually gathered
///
/// Every git leg in this crate fail-safes to `false` / empty on error. In the
/// sweep that converges on "no finding"; HERE it converges on a High that
/// BLOCKS a state transition, so an infrastructure hiccup would be
/// indistinguishable from a genuine phantom-done. Four guards invert that:
/// [`main_base_resolves`] probes the repo once before any refusal; a sibling
/// scan truncated at [`PRE_DONE_SIBLING_SCAN_CAP`] is treated as incomplete;
/// and the two legs the refusal actually RESTS on are consulted through their
/// fallible variants ([`crate::GitOps::try_path_tracked_on`] and
/// [`try_task_referencing_commits`]) so a per-call git failure is recorded as
/// an unanswered question rather than silently read as evidence. Every one of
/// them still EMITS the finding, but as an advisory `Low` that cannot block
/// the flip.
///
/// The whole-repo [`main_base_resolves`] probe alone was not sufficient: it
/// cannot distinguish a per-call failure from a genuine observation, so with
/// `main` still resolving, one transient `ls-tree` error plus an empty
/// `log_grep` produced a blocking High against a legitimate done-flip.
///
/// # Known residual
///
/// The per-sibling delta seams reached through [`changed_paths_for_claim`]
/// (`changed_paths_in_commit` / `diff_changed_paths` / `is_ancestor`) still
/// fail-safe to empty/false, so a git failure there can leave an entry in
/// `still_absent` that a healthy read would have cleared. That is the same
/// failure direction and is deliberately left open here: those seams are on
/// the rescue leg rather than on the two legs the refusal rests on, and
/// widening them means four more fallible trait methods threaded through the
/// sweep's two call sites as well.
///
/// `gitignored` is the task's precomputed gitignored subset (see
/// [`check_task`]), so this leg costs no `git check-ignore` forks of its own.
fn check_pre_done_landing(
    ctx: &AuditContext,
    meta: &TaskMetadata,
    gitignored: &[String],
) -> Option<Finding> {
    // Nothing corroboratable → nothing to refuse. Covers both the research /
    // ops / escalation task that legitimately lands no files, and the task
    // whose declared entries are all gitignored (equally uncorroboratable).
    // Pure in-memory `retain` against the shared set — no fork here.
    let declared: Vec<String> = meta
        .files
        .iter()
        .filter(|p| !gitignored.iter().any(|g| g == *p))
        .cloned()
        .collect();
    if declared.is_empty() {
        return None;
    }

    // The healthy flip: every declared entry resolves to a tracked file or
    // directory on main. Costs |declared| × `git ls-tree` and no `git log` at
    // all — and, on this path, nothing else: the probe and the sibling scan
    // below run only once something is genuinely absent.
    //
    // `try_path_tracked_on`, not `path_tracked_on`: the infallible seam
    // fail-safes an ls-tree ERROR to `false`, which here reads as "the
    // declared deliverable is absent from main" — evidence this leg never
    // actually gathered. An errored entry is still carried into the refusal
    // set (the rescue leg below may yet account for it, and if it clears
    // everything the finding disappears entirely), but it arms `degraded` so
    // any SURVIVING refusal is emitted as a non-blocking advisory Low.
    let mut degraded: Option<String> = None;
    let mut absent: Vec<String> = Vec::new();
    for p in &declared {
        match ctx.git.try_path_tracked_on(MAIN_BASE, p) {
            Ok(true) => {}
            Ok(false) => absent.push(p.clone()),
            Err(_) => {
                absent.push(p.clone());
                // First failure only: the reason names one concrete entry an
                // operator can re-check by hand, and `RealGitOps` has already
                // printed a per-failure `reify-audit:` breadcrumb carrying
                // git's own stderr for the rest.
                degraded.get_or_insert_with(|| {
                    format!("git degraded: ls-tree errored for declared entry {p}")
                });
            }
        }
    }
    if absent.is_empty() {
        return None;
    }

    // Something is absent, so we are now on the road to a refusal. Probe that
    // git is actually usable before spending the sibling scan on it, and before
    // reading "absent" as evidence rather than as the fail-safe default.
    if !main_base_resolves(ctx) {
        return Some(pre_done_refusal(
            meta,
            &absent,
            &[],
            Some("git degraded: MAIN_BASE did not resolve"),
        ));
    }

    // Deletion / rename rescue. A task whose declared file was REMOVED by its
    // landing commit has path_tracked_on == false, yet the work landed. Only
    // the commit's OWN delta shows a deletion, which is why this leg depends on
    // `changed_paths_for_claim` — `main..<merge>` can never show it, since a
    // path absent from both trees is not in a two-point diff at all.
    //
    // The RENAME half of this rescue is not separable from the seam: git's
    // default rename detection collapses a rename to the destination path, so
    // the vanished source never reaches this loop and the entry declaring it is
    // refused. Both `GitOps` path-listing seams therefore pass `--no-renames`.
    // If that flag is ever dropped, this leg silently degrades to
    // deletion-only and the rename case regresses without a compile error.
    let mut still_absent = absent.clone();
    // `try_…`, for the same reason the presence leg above uses the fallible
    // seam: an empty candidate list from a FAILED `git log --grep` is not
    // evidence that no commit references this task, and reading it as such is
    // the second half of a wrongful refusal.
    let siblings = match try_task_referencing_commits(ctx, &meta.task_id) {
        Ok(s) => s,
        Err(_) => {
            degraded.get_or_insert_with(|| {
                "git degraded: log --grep errored, so no rescue candidate was inspected"
                    .to_string()
            });
            Vec::new()
        }
    };
    let mut contributing: Vec<&GitCommit> = Vec::new();
    let mut ancestry = AncestryCache::default();
    // INVARIANT, locally provable HERE and nowhere else in this module: every
    // candidate in `siblings` came from `git log <MAIN_BASE> --grep=…`, which
    // lists only commits REACHABLE FROM `MAIN_BASE` — the same relation
    // `git merge-base --is-ancestor <sha> <MAIN_BASE>` tests, against the same
    // ref, in the same repo, in the same process. The fork can therefore only
    // answer `true`, and issuing it is pure cost: without this priming each
    // inspected sibling pays TWO forks (ancestry + diff), i.e. up to
    // 2 × PRE_DONE_SIBLING_SCAN_CAP ≈ 5.7 s at the measured ~57 ms/fork,
    // inside fused-memory's per-project write lock against a 30 s hard timeout.
    //
    // Scoped to this call site on purpose: `changed_paths_for_claim` stays the
    // sole ancestry POLICY point, and the sweep's rescue leg — whose
    // candidates arrive the same way, but whose forks are not paid under a
    // lock — keeps asking git, so a `MockGitOps` fixture there still reaches
    // both arms.
    for c in &siblings {
        ancestry.prime(&c.sha, true);
    }
    let mut truncated = false;
    for (scanned, c) in siblings.iter().enumerate() {
        if still_absent.is_empty() {
            break;
        }
        if scanned >= PRE_DONE_SIBLING_SCAN_CAP {
            truncated = true;
            eprintln!(
                "reify-audit: pre-done landing scan capped at {PRE_DONE_SIBLING_SCAN_CAP} \
                 commits for task {} ({} candidates); {} entries left unchecked",
                meta.task_id,
                siblings.len(),
                still_absent.len()
            );
            break;
        }
        let covered = changed_paths_for_claim(ctx, &c.sha, &mut ancestry);
        let before = still_absent.len();
        // Prefix-aware, mirroring `path_tracked_on`'s directory handling
        // (`git ls-tree main -- <dir>` resolves a directory entry). A declared
        // entry naming a DIRECTORY the landing commit removed or renamed away
        // never appears verbatim in a `--name-only` delta, which lists the
        // individual files beneath it — matching by exact string equality alone
        // would refuse that flip.
        still_absent.retain(|p| !covered.iter().any(|c| covers_path(c, p)));
        if still_absent.len() < before {
            contributing.push(c);
        }
    }
    if still_absent.is_empty() {
        return None;
    }

    // A recorded git failure outranks truncation as the reported reason: it is
    // the more actionable of the two, and both downgrade identically.
    let advisory = degraded.as_deref().or(truncated.then_some(
        "incomplete: sibling scan hit PRE_DONE_SIBLING_SCAN_CAP before exhausting candidates",
    ));
    Some(pre_done_refusal(meta, &still_absent, &contributing, advisory))
}

/// True iff the changed path `changed` accounts for the declared entry
/// `declared` — either verbatim, or as a file beneath it when `declared` names
/// a directory.
///
/// Mirrors [`crate::GitOps::path_tracked_on`]'s directory handling
/// (`git ls-tree main -- <dir>` resolves a directory entry). A `--name-only`
/// delta lists the individual files under a removed directory and never the
/// directory itself, so exact string equality alone would miss it. The `/`
/// check anchors the prefix: `crates/x/gone_too/a.rs` must NOT satisfy a
/// declared `crates/x/gone`. Allocation-free on purpose — this runs inside the
/// per-sibling loop on the write-lock-held pre-done path.
///
/// A declared entry may be written WITH a trailing slash (`crates/x/gone/`) —
/// `metadata.files` is hand-authored and nothing normalises it. That form must
/// be trimmed before the prefix compare, or the anchor check indexes the byte
/// AFTER the slash (`'a'` of `a.rs`), never sees `/`, and the entry is reported
/// as still-absent. The healthy `path_tracked_on` leg does NOT mask this:
/// `git ls-tree main -- crates/x/gone/` returns nothing for a directory the
/// landing commit removed, so a removal/rename task declaring a trailing-slash
/// directory reaches this rescue and would be wrongly refused for work that
/// did land.
///
/// An entry that trims to empty names no deliverable, so nothing can cover it
/// — returning `false` there also preserves the pre-normalisation behaviour
/// (a repo-relative `changed` never begins with `/`).
fn covers_path(changed: &str, declared: &str) -> bool {
    let declared = declared.trim_end_matches('/');
    if declared.is_empty() {
        return false;
    }
    changed == declared
        || (changed.len() > declared.len()
            && changed.starts_with(declared)
            && changed.as_bytes()[declared.len()] == b'/')
}

/// One-fork probe that `MAIN_BASE` resolves in this repository:
/// `git merge-base --is-ancestor main main` is exit-0 iff `main` names a
/// commit, and exit-128 (mapped to `false` by [`crate::GitOps::is_ancestor`]'s
/// fail-safe) when the ref, the repo, or `git` itself is unavailable.
///
/// Used only on the pre-done refusal road — see the "refuse only on evidence
/// actually gathered" section of [`check_pre_done_landing`].
fn main_base_resolves(ctx: &AuditContext) -> bool {
    ctx.git.is_ancestor(MAIN_BASE, MAIN_BASE)
}

/// Build the pre-done refusal finding.
///
/// `advisory` carries the reason the evidence is incomplete (degraded git, a
/// truncated scan). When it is `Some`, the finding is forced to `Low` and
/// labelled — it is emitted for operator visibility but must not block a
/// done-flip, because the corroborating commit may simply be one we never
/// looked at. When it is `None` the severity is the armed one, subject to the
/// [`pre_done_refusal_severity`] break-glass.
fn pre_done_refusal(
    meta: &TaskMetadata,
    still_absent: &[String],
    contributing: &[&GitCommit],
    advisory: Option<&str>,
) -> Finding {
    // Cite the commits we DID inspect alongside the absent set, so an operator
    // reading the refusal payload can inspect immediately rather than
    // re-deriving the candidate list by hand.
    let mut evidence: Vec<EvidenceRef> = contributing
        .iter()
        .map(|c| EvidenceRef::Commit {
            sha: c.sha.clone(),
            subject: c.subject.clone(),
        })
        .collect();
    evidence.push(EvidenceRef::MetadataFiles {
        entries: still_absent.to_vec(),
    });

    // Mark a downgraded refusal so an operator reading a Low in a log can tell
    // it apart from a naturally-Low rescue, and can tell the two downgrade
    // reasons (break-glass vs incomplete evidence) apart from each other.
    let (severity, prefix) = match advisory {
        Some(reason) => (Severity::Low, format!("[advisory — {reason}] ")),
        None => {
            let armed = pre_done_refusal_severity();
            let prefix = if armed == Severity::High {
                String::new()
            } else {
                "[warn-only] ".to_string()
            };
            (armed, prefix)
        }
    };

    Finding {
        pattern: Pattern::P5PhantomDone,
        severity,
        task_id: meta.task_id.clone(),
        summary: format!(
            "{}pre-done gate: {} declared metadata.files entr{} neither tracked on main \
             nor covered by a task-referencing commit's own delta — refusing the \
             done-flip for task {}",
            prefix,
            still_absent.len(),
            if still_absent.len() == 1 { "y is" } else { "ies are" },
            meta.task_id
        ),
        evidence,
    }
}

/// How many task-referencing commits the pre-done landing scan will inspect.
///
/// The hook runs INSIDE fused-memory's per-project write lock under a 30 s hard
/// timeout (`middleware/pre_done_hook.py`), so an unbounded scan would
/// head-of-line block every task mutation for that project. Measured on the
/// live repo: `git log main --grep=<id>` ≈ 73 ms and `git ls-tree main -- <p>`
/// ≈ 57 ms/path, so the cheap legs dominate the healthy case and this leg only
/// runs when something is genuinely absent. Truncation emits a breadcrumb
/// rather than silently reporting partial coverage as complete.
///
/// An inspected sibling costs exactly ONE fork — the diff — because the
/// ancestry answer is primed from the `log_grep` reachability invariant (see
/// the call site in [`check_pre_done_landing`]). The worst case is therefore
/// 50 × ~57 ms ≈ 2.9 s, not the ~5.7 s two forks per sibling would cost.
const PRE_DONE_SIBLING_SCAN_CAP: usize = 50;

/// Break-glass: `REIFY_AUDIT_PREDONE_WARN_ONLY=1` downgrades the pre-done
/// landing refusal from High to Low, so it can no longer block a done-flip;
/// the finding is still produced, and only the exit code — which counts Highs
/// — changes.
///
/// Why this exists: the gate is fail-closed production infrastructure that had
/// never emitted a finding until this task. Without it, backing a misfire out
/// means reinstalling an older binary or unsetting the hook command entirely.
/// Mirrors the house `REIFY_MAIN_GATE_BYPASS` / `REIFY_STASH_GUARD_BYPASS`
/// convention. Default is ARMED.
///
/// # Two limits to know BEFORE relying on it
///
/// 1. It is not cheaper than the outage it prevents. The value is read from
///    the hook subprocess's environment, which it inherits from fused-memory
///    (`create_subprocess_exec`, no `env=` kwarg), so setting it means editing
///    `~/.config/systemd/user/fused-memory.service` and restarting
///    fused-memory — the same red-tier restart. It has to be decided before
///    exposure, not reached for mid-incident.
/// 2. On the LIVE hook path it makes the gate silent, not advisory.
///    dark-factory's `pre_done_hook.py` launches with `stdout=PIPE,
///    stderr=PIPE` and surfaces the captured stderr only on a NON-zero exit;
///    warn-only exits 0 by construction, so the `[warn-only]` line is captured
///    and discarded. An observational soak has to run this binary out-of-band.
///    Rollout sequence: `docs/architecture-audit/f-infra-design.md` §11.1.4.
///
/// Deliberately scoped to THIS finding only. It must not widen into a general
/// P5 mute: no sweep finding and no pre-existing High path in [`check_one`]
/// consults it.
fn pre_done_refusal_severity() -> Severity {
    if std::env::var("REIFY_AUDIT_PREDONE_WARN_ONLY").is_ok_and(|v| v == "1") {
        Severity::Low
    } else {
        Severity::High
    }
}

/// Per-task corroboration. Returns `Some(Finding)` if the task is
/// phantom-done, `None` if the provenance corroborates cleanly.
///
/// `gitignored` is the task's gitignored subset, computed once by
/// [`check_task`]; only the [`CheckMode::PreDone`] arm consumes it.
fn check_one(
    ctx: &AuditContext,
    meta: &TaskMetadata,
    mode: CheckMode,
    gitignored: &[String],
) -> Option<Finding> {
    // One ancestry answer per SHA for the whole invocation: the merged-arm
    // rescue and the primary git-diff leg below both test `prov.commit`.
    let mut ancestry = AncestryCache::default();
    let Some(prov) = meta.done_provenance.as_ref() else {
        return match mode {
            // Sweep: unchanged (guard A1). A provenance-less `done` row is the
            // norm for tasks predating provenance capture; emitting on every
            // one of them is exactly the 4075/4464 false-positive storm.
            CheckMode::Sweep => None,
            // Pre-done: provenance is NOT missing, it is merely not yet
            // written. `task_interceptor.py` accumulates it in the in-memory
            // `audit_fields` dict and persists it only after this hook
            // returns, and the hook command template substitutes just `{id}`
            // (no env injection, no stdin), so the subprocess receives no task
            // state beyond the id. Landing must therefore be corroborated from
            // `task_id` + `metadata.files` alone.
            CheckMode::PreDone => check_pre_done_landing(ctx, meta, gitignored),
        };
    };
    let kind = prov.kind.as_deref().unwrap_or("");

    // Corroboration (a) — runs.db trail. For kind="merged", absence of a
    // task_completed event means the orchestrator never recorded the
    // completion at all — definitive phantom-done, no sibling rescue.
    // (Memory: procedural_runs_db_forensics.md.)
    //
    // Three states:
    //   Ok(true)  — event exists, proceed to git corroboration
    //   Ok(false) — event genuinely missing → High, evidence=RunsDb row
    //   Err(e)    — runs.db is unreadable (table missing, db locked,
    //               permission denied, etc.). Operators need to distinguish
    //               this from a real phantom-done, so emit a Medium finding
    //               citing the unreadable runs.db rather than mass-flagging
    //               every merged task as High.
    if kind == "merged" {
        match has_task_completed_event(ctx, &meta.task_id) {
            Ok(true) => {}
            Ok(false) => {
                // Ancestor-corroboration rescue. If the claimed commit is a
                // valid ancestor of main, the work is literally on main — a
                // sufficient corroboration regardless of the runs.db gap (e.g.
                // rebuild coverage gap, recycled task ID). Downgrade to Low.
                // Ancestry alone (not file-presence) is the corroboration
                // signal here; we stay Low/inspectable rather than
                // suppressing entirely.
                if let Some(commit) = prov.commit.as_deref()
                    && ancestry.is_ancestor_of_main(ctx, commit)
                {
                    return Some(Finding {
                        pattern: Pattern::P5PhantomDone,
                        severity: Severity::Low,
                        task_id: meta.task_id.clone(),
                        summary:
                            "deliverable present (claimed commit is an ancestor of main); \
                             no task_completed event in runs.db — stale/rebuilt provenance, \
                             not phantom-done"
                                .to_string(),
                        // Cite both the missing-event RunsDb row and the
                        // corroborating ancestor commit. Subject left empty
                        // to avoid an extra `git log` round-trip; the sha
                        // alone is the inspectable corroboration locator.
                        evidence: vec![
                            EvidenceRef::RunsDb {
                                table: "events".to_string(),
                                key: format!(
                                    "task_id={} AND event_type=task_completed",
                                    meta.task_id
                                ),
                            },
                            EvidenceRef::Commit {
                                sha: commit.to_string(),
                                subject: String::new(),
                            },
                        ],
                    });
                }

                // (a) Task-id-referencing commit reachable on main rescue.
                // `git log main --grep=<task_id>` only returns commits reachable
                // from main, so a non-empty result means a "Merge task/<id>
                // into main" or task-id-referencing commit is on main.
                // The branch ref may have been reaped and the claimed commit
                // unresolvable, but the work is demonstrably on main.
                // Downgrade to Low so the operator can inspect without it
                // escalating as a genuine phantom-done.
                //
                // Substring-match note: `--grep` is a bare substring/regex
                // match, so `--grep=593` also matches "Merge task/5937 into
                // main". That collision was an ACCEPTED risk under task 4464's
                // bias toward fewer false-Highs; it is now FILTERED, because
                // the failure it produces is a false NEGATIVE — a genuine
                // phantom-done silently downgraded to Low. The criterion is a
                // non-ASCII-digit boundary (see `hit_references_task`), so
                // "Merge task/5937 into main" no longer rescues task 593.
                {
                    let siblings = task_referencing_commits(ctx, &meta.task_id);
                    if !siblings.is_empty() {
                        let mut evidence: Vec<EvidenceRef> = siblings
                            .iter()
                            .map(|c| EvidenceRef::Commit {
                                sha: c.sha.clone(),
                                subject: c.subject.clone(),
                            })
                            .collect();
                        evidence.push(EvidenceRef::RunsDb {
                            table: "events".to_string(),
                            key: format!(
                                "task_id={} AND event_type=task_completed",
                                meta.task_id
                            ),
                        });
                        return Some(Finding {
                            pattern: Pattern::P5PhantomDone,
                            severity: Severity::Low,
                            task_id: meta.task_id.clone(),
                            summary: format!(
                                "task-id-referencing commit reachable on main (landed, not \
                                 phantom-done); no task_completed event in runs.db — \
                                 stale/rebuilt provenance or reaped branch (task {})",
                                meta.task_id
                            ),
                            evidence,
                        });
                    }
                }

                // (b) Deliverable-presence rescue for the merged Ok(false) arm.
                // Uses all_files_tracked_on_main (shared with the git-diff leg)
                // so the two sites cannot drift: if every metadata.files entry
                // resolves to a tracked path on main, the work landed even
                // though the runs.db row is missing (stale or rebuilt
                // provenance). Downgrade to Low.
                if all_files_tracked_on_main(ctx, meta) {
                    return Some(Finding {
                        pattern: Pattern::P5PhantomDone,
                        severity: Severity::Low,
                        task_id: meta.task_id.clone(),
                        summary: format!(
                            "deliverable present on main (every metadata.files entry \
                             resolves to a tracked path); no task_completed event in \
                             runs.db — stale/rebuilt provenance, not phantom-done (task {})",
                            meta.task_id
                        ),
                        evidence: vec![
                            EvidenceRef::MetadataFiles {
                                entries: meta.files.clone(),
                            },
                            EvidenceRef::RunsDb {
                                table: "events".to_string(),
                                key: format!(
                                    "task_id={} AND event_type=task_completed",
                                    meta.task_id
                                ),
                            },
                        ],
                    });
                }

                return Some(Finding {
                    pattern: Pattern::P5PhantomDone,
                    severity: Severity::High,
                    task_id: meta.task_id.clone(),
                    summary:
                        "metadata.status=done but no task_completed event in runs.db".to_string(),
                    evidence: vec![EvidenceRef::RunsDb {
                        table: "events".to_string(),
                        key: format!(
                            "task_id={} AND event_type=task_completed",
                            meta.task_id
                        ),
                    }],
                });
            }
            Err(e) => {
                // Surface a low-noise breadcrumb so operators aren't left
                // wondering why nothing flagged — but only emit one finding
                // per task, not a torrent of stderr lines.
                eprintln!(
                    "reify-audit: runs.db unreadable while checking task {}: {}",
                    meta.task_id, e
                );
                return Some(Finding {
                    pattern: Pattern::P5PhantomDone,
                    severity: Severity::Medium,
                    task_id: meta.task_id.clone(),
                    summary: format!(
                        "runs.db unreadable — cannot corroborate merged provenance for task {}: {}",
                        meta.task_id, e
                    ),
                    evidence: vec![EvidenceRef::RunsDb {
                        table: "events".to_string(),
                        key: format!(
                            "task_id={} AND event_type=task_completed",
                            meta.task_id
                        ),
                    }],
                });
            }
        }
    }

    // No files claimed → no git provenance to corroborate; treat as clean for
    // the git-diff leg only. The runs.db check above was already decisive for
    // kind="merged" tasks: if that check passed (Ok(true)), the task is
    // corroborated by the orchestrator record even without a file-list. Only
    // gate the expensive git-diff work that follows.
    if meta.files.is_empty() {
        return None;
    }

    // Corroboration (b) — primary git check. The claimed commit's diff
    // against main must cover every metadata.files entry. For
    // kind="found_on_main" with no `commit` field (the work was discovered
    // on main rather than merged through), the primary check yields
    // "everything missing" and the sibling-rescue path takes over.
    let primary_covered = match prov.commit.as_deref() {
        Some(commit) => changed_paths_for_claim(ctx, commit, &mut ancestry),
        None => Vec::new(),
    };
    let missing = files_missing_from(&meta.files, &primary_covered);
    if missing.is_empty() {
        return None;
    }

    // Cargo.lock-only divergence guard. When the lone missing entry is
    // Cargo.lock — and every other metadata.files path was corroborated by
    // the primary diff — main has merely absorbed an unrelated dependency
    // bump after our task wrote its lockfile. Not phantom-done.
    // Precondition: meta.files must have more than one entry so that "every
    // other entry corroborates" is a meaningful claim. When the task claims
    // only Cargo.lock (no other entries), the precondition is violated and
    // we fall through to sibling-FF rescue, then High (erring on the side of
    // operator visibility for an unverifiable claim).
    // Memory: project_post_merge_equivalence_false_positive_cargo_lock.md.
    if is_cargo_lock_only(&missing, meta.files.len()) {
        return Some(Finding {
            pattern: Pattern::P5PhantomDone,
            severity: Severity::Low,
            task_id: meta.task_id.clone(),
            summary:
                "Cargo.lock-only divergence: every other metadata.files entry corroborates; \
                 main absorbed an unrelated lockfile change after this task merged"
                    .to_string(),
            evidence: vec![EvidenceRef::MetadataFiles {
                entries: missing.clone(),
            }],
        });
    }

    // Convergent fast-forward / sibling-absorbed rescue. The task's branch
    // may have been reaped after a sibling FF; `git log main --grep <id>`
    // surfaces the actual landing commit(s). If the union of those sibling
    // diffs covers every missing path, downgrade to Low and cite each
    // contributing sibling SHA. Memory: project_unblock_convergent_ff_worktree_reap.md.
    //
    // Substring-match note: the pure-reachability fallback (below) fires
    // whenever siblings is non-empty, which intercepts before the
    // deliverable-presence rescue — so a colliding task_id produced a false-low
    // on a genuine phantom-done by matching an unrelated commit. That was an
    // ACCEPTED risk under task 4464's bias toward fewer false-Highs; it is now
    // FILTERED, because the failure it produces is a false NEGATIVE. The
    // criterion is a non-ASCII-digit boundary (see `hit_references_task`),
    // so "Merge task/5937 into main" no longer rescues task 593.
    let siblings = task_referencing_commits(ctx, &meta.task_id);
    if !siblings.is_empty() {
        let mut sibling_covered: Vec<String> = Vec::new();
        let mut contributing: Vec<&GitCommit> = Vec::new();
        for c in &siblings {
            // Via the ancestry-selecting helper, not changed_paths_in_commit
            // directly: log_grep(main, …) only ever returns ancestors of main, but
            // stating that as an unwritten invariant at this call site would make
            // the code silently wrong the day a caller passes a non-ancestor.
            let diff = changed_paths_for_claim(ctx, &c.sha, &mut ancestry);
            // Only cite siblings that contribute to closing the missing set.
            if diff.iter().any(|p| missing.contains(p)) {
                contributing.push(c);
            }
            sibling_covered.extend(diff);
        }
        let still_missing = files_missing_from(&missing, &sibling_covered);
        if still_missing.is_empty() {
            let mut evidence: Vec<EvidenceRef> = contributing
                .iter()
                .map(|c| EvidenceRef::Commit {
                    sha: c.sha.clone(),
                    subject: c.subject.clone(),
                })
                .collect();
            evidence.push(EvidenceRef::MetadataFiles {
                entries: missing.clone(),
            });
            return Some(Finding {
                pattern: Pattern::P5PhantomDone,
                severity: Severity::Low,
                task_id: meta.task_id.clone(),
                summary:
                    "convergent fast-forward: claimed commit not reachable but sibling commit(s) \
                     on main cover every missing metadata.files entry"
                        .to_string(),
                evidence,
            });
        }

        // Pure-reachability fallback: coverage check failed (still_missing
        // non-empty) but siblings are non-empty, which means a task-id-referencing
        // commit IS reachable on main. The claimed commit may be unresolvable /
        // the diff unreliable, but the work demonstrably landed. Downgrade to Low
        // so the operator can inspect without it escalating as phantom-done.
        // Cite all contributing siblings (or all siblings if none contributed).
        let cite_commits: Vec<EvidenceRef> = if !contributing.is_empty() {
            contributing
                .iter()
                .map(|c| EvidenceRef::Commit {
                    sha: c.sha.clone(),
                    subject: c.subject.clone(),
                })
                .collect()
        } else {
            siblings
                .iter()
                .map(|c| EvidenceRef::Commit {
                    sha: c.sha.clone(),
                    subject: c.subject.clone(),
                })
                .collect()
        };
        let mut evidence = cite_commits;
        evidence.push(EvidenceRef::MetadataFiles {
            entries: still_missing.clone(),
        });
        return Some(Finding {
            pattern: Pattern::P5PhantomDone,
            severity: Severity::Low,
            task_id: meta.task_id.clone(),
            summary: format!(
                "task-id-referencing commit reachable on main (not phantom-done); \
                 claimed commit unresolvable / diff unreliable — {} metadata.files \
                 entries not confirmed in sibling diffs (task {})",
                still_missing.len(),
                meta.task_id
            ),
            evidence,
        });
    }

    // Deliverable-presence rescue. If every metadata.files entry resolves to a
    // tracked file or directory on main (via path_tracked_on), the work landed
    // — only the done_provenance.commit pointer is stale (e.g. recycled task
    // ID or runs.db rebuild whose squashed commit was later gc'd). Downgrade
    // to Low so the operator can inspect without it escalating as a genuine
    // phantom-done.
    //
    // Scope: applies only to the git-diff leg. The merged/Ok(false) arm above
    // has its own ancestry-corroboration rescue; a non-ancestor merged task
    // with a missing runs.db event correctly stays High and does NOT fall
    // through here.
    //
    // Note: file-presence is necessary but NOT sufficient (a file can exist
    // yet lack the wired symbol, e.g. task 3803's unwired resolve_unit_expr),
    // so we stay Low / inspectable rather than suppressing entirely.
    //
    // Only path_tracked_on is checked — not is_gitignored — so that a
    // gitignored entry that is also absent from main stays in genuinely_absent
    // and keeps the finding High. Excluding gitignored entries from
    // genuinely_absent would incorrectly downgrade to Low for tasks whose sole
    // missing file happens to be gitignored (check_gitignored handles the
    // separate Medium breadcrumb for the gitignored aspect).
    //
    // all_files_tracked_on_main is called first so genuinely_absent is only
    // computed (and path_tracked_on called per-file a second time) on the High
    // path, where the absent list is needed as evidence.
    if all_files_tracked_on_main(ctx, meta) {
        return Some(Finding {
            pattern: Pattern::P5PhantomDone,
            severity: Severity::Low,
            task_id: meta.task_id.clone(),
            summary:
                "deliverable present on main (every metadata.files entry resolves to a tracked \
                 file or directory) but claimed provenance commit not reachable — \
                 stale-provenance, not phantom-done"
                    .to_string(),
            // Cite `missing` (files not in the claimed commit's diff, all
            // verified present on main via path_tracked_on) as the stale-
            // provenance locator; `genuinely_absent` is empty here so citing
            // it would produce an uninformative empty list.
            evidence: vec![EvidenceRef::MetadataFiles {
                entries: missing.clone(),
            }],
        });
    }

    let genuinely_absent: Vec<String> = meta
        .files
        .iter()
        .filter(|p| !ctx.git.path_tracked_on(MAIN_BASE, p))
        .cloned()
        .collect();

    Some(build_high_finding(
        meta,
        &genuinely_absent,
        "metadata.files mismatch / commit not reachable from main",
    ))
}

/// Run the runs.db existence query: returns `Ok(true)` if at least one
/// `task_completed` event exists for `task_id`, `Ok(false)` if no row
/// matches, and `Err` if the database itself can't be queried (missing
/// table, locked file, permission denied, etc.).
///
/// The three-way return is load-bearing for [`check_one`]: a missing row
/// is genuine evidence of phantom-done (High), but an unreadable database
/// is a different operator-actionable signal (Medium "runs.db unreadable")
/// — earlier versions collapsed both into `false` and risked mass-flagging
/// every merged task on a malformed runs.db.
fn has_task_completed_event(
    ctx: &AuditContext,
    task_id: &str,
) -> Result<bool, rusqlite::Error> {
    let mut stmt = ctx.conn.prepare(PRODUCTION_QUERY)?;
    match stmt.query_row::<i64, _, _>(rusqlite::params![task_id], |row| row.get(0)) {
        Ok(_) => Ok(true),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Returns the subset of `files` not present in `covered`.
fn files_missing_from(files: &[String], covered: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| !covered.contains(f))
        .cloned()
        .collect()
}

/// True iff the sole missing entry is a `Cargo.lock` file (top-level or
/// nested — e.g. `fuzz/Cargo.lock`, `examples/foo/Cargo.lock`). Matches by
/// the path's final segment so nested lockfiles still benefit from the
/// downgrade.
///
/// Precondition: `total_files > 1`. At least one other `metadata.files` entry
/// must exist for the "every other entry corroborates" justification to hold.
/// Pass `meta.files.len()` at the call site; when the task claims only
/// Cargo.lock, this returns `false` and the caller falls through to the
/// sibling-FF rescue path.
fn is_cargo_lock_only(missing: &[String], total_files: usize) -> bool {
    total_files > 1
        && missing.len() == 1
        && std::path::Path::new(&missing[0]).file_name()
            == Some(std::ffi::OsStr::new("Cargo.lock"))
}

/// Construct a `Severity::High` phantom-done finding listing the missing
/// metadata.files entries as the primary evidence.
fn build_high_finding(meta: &TaskMetadata, missing: &[String], summary: &str) -> Finding {
    Finding {
        pattern: Pattern::P5PhantomDone,
        severity: Severity::High,
        task_id: meta.task_id.clone(),
        summary: summary.to_string(),
        evidence: vec![EvidenceRef::MetadataFiles {
            entries: missing.to_vec(),
        }],
    }
}

/// H2 — live-path-stranded pass (with cross-crate gate + suppression guards, step-10).
///
/// Emits `P5LivePathStranded` `Medium` only when ALL of:
///
/// 1. **Cross-crate gate**: `metadata.files` span >=2 distinct `crates/<name>/`
///    roots (computed by [`crate_root_count`]). Single-crate orphans are P1's
///    grace-windowed domain; H2 scopes to the documented cross-crate relocation
///    pattern to avoid duplicating noisy P1 findings.
/// 2. **No commit**: skipped when `done_provenance.commit` is absent.
/// 3. **Per-symbol suppression guards** (reuses P1's opt-out set):
///    - Symbol file starts with `crates/reify-stdlib/` (scope-exclude).
///    - `has_allow_dead_code` or `has_cfg_test` (intentional-orphan opt-outs).
///    - Non-blank `// G-allow:` marker (mirrors `p1_producer_orphan::is_g_allow_suppressed`).
/// 4. **No non-test workspace caller**: `find_references` returns only test-path
///    refs (or none) for the symbol.
///
/// Design rationale: cross-crate gate keeps H2 off of P1's single-crate turf;
/// suppression guards keep H2 and P1 semantically consistent. Task 4140 §H2.
///
/// ## 4141 live-corpus validation note
///
/// Task 4141 confirmed that H2 **cannot be live-validated** against the current
/// corpus. `needs_jcodemunch()` in `bin/reify-audit.rs:433–443` returns `false`
/// for `--pattern P5` (and for `--pre-done`) → the binary always wires
/// `NoopJCodemunchOps` for P5 runs. A default sweep attempts
/// `RealJCodemunchOps` but fail-softs to `NoopJCodemunchOps` because
/// `jcodemunch-serve` is not yet deployed in reify
/// (`bin/reify-audit.rs:547–567`). With `NoopJCodemunchOps`,
/// `get_changed_symbols` returns `vec![]` → this function iterates nothing →
/// zero H2 findings regardless of real cross-crate stranding. A zero-finding
/// H2 sweep is therefore **vacuous** and cannot justify a `Medium → High`
/// promotion. H2 remains at `Severity::Medium` (non-blocking for the D-1
/// hook) pending the real jcodemunch JCodemunchOps implementation. Future
/// promotion task must meet the criteria in
/// `docs/prds/p5-h1-h2-live-corpus-fp-validation.md` §6 (H2 promotion
/// criteria): real jcodemunch substrate wired, non-vacuous live sweep,
/// measured FP rate ≤ 5%. Per task 4141 live-corpus FP validation.
///
/// When `get_changed_symbols` returns an empty slice a stderr vacuous
/// breadcrumb is emitted via [`h2_vacuous_breadcrumb`] (task 4144).
fn check_live_path_stranded(ctx: &AuditContext, meta: &TaskMetadata) -> Vec<Finding> {
    // Cross-crate gate: requires >=2 distinct crates/<name>/ roots.
    if crate_root_count(&meta.files) < 2 {
        return vec![];
    }

    let Some(commit) = meta.done_provenance.as_ref().and_then(|p| p.commit.as_deref()) else {
        return vec![];
    };
    let since_sha = format!("{commit}^1");
    let until_sha = commit;

    let symbols = ctx.jcodemunch.get_changed_symbols(&since_sha, until_sha);
    if let Some(msg) = h2_vacuous_breadcrumb(&symbols, &meta.task_id, &since_sha, until_sha) {
        eprintln!("{msg}");
    }
    let mut findings = Vec::new();
    for symbol in symbols {
        // Per-symbol guards: stdlib scope-exclude, intentional-orphan opt-outs
        // (#[allow(dead_code)], #[cfg(test)]), and non-blank G-allow marker.
        // Delegated to crate::is_symbol_suppressed so that P1 and P5 H2 share
        // the same opt-out semantics and cannot drift independently.
        if crate::is_symbol_suppressed(&symbol) {
            continue;
        }
        let has_non_test_caller = ctx
            .jcodemunch
            .find_references(&symbol)
            .iter()
            .any(|r| !crate::is_test_path(&r.file));
        if !has_non_test_caller {
            findings.push(Finding {
                pattern: Pattern::P5LivePathStranded,
                severity: Severity::Medium,
                task_id: meta.task_id.clone(),
                summary: format!(
                    "changed symbol `{}` at {}:{} has no non-test workspace caller — \
                     possible live-path stranding from a cross-crate relocation \
                     (task 4140 H2)",
                    symbol.name, symbol.file, symbol.line
                ),
                evidence: vec![EvidenceRef::File { path: symbol.file.clone() }],
            });
        }
    }
    findings
}

/// Returns a `reify-audit:` prefixed stderr breadcrumb message when the H2
/// `get_changed_symbols` call returned an empty slice, so operators can
/// distinguish a vacuous sweep from a legitimately clean corpus.
///
/// Returns `None` when `symbols` is non-empty (normal sweep; no annotation
/// needed). Mirrors the `Option<String>`-diagnostic pattern from
/// `jcodemunch_client.rs::read_source_lines_for_enrichment`.
fn h2_vacuous_breadcrumb(
    symbols: &[ChangedSymbol],
    task_id: &str,
    since_sha: &str,
    until_sha: &str,
) -> Option<String> {
    if symbols.is_empty() {
        Some(format!(
            "reify-audit: H2 (live-path-stranded) vacuous for task {task_id}: \
             get_changed_symbols returned empty for {since_sha}..{until_sha} \
             — H2 produced no findings (corpus clean OR jcodemunch not wired / NoopJCodemunchOps)"
        ))
    } else {
        None
    }
}

/// Count the number of distinct `crates/<name>/` roots referenced by `files`.
///
/// A path contributes a root if it starts with `crates/` and has at least one
/// more path component (the crate name). For example:
/// - `crates/reify-eval/src/lib.rs` → root `reify-eval`
/// - `crates/reify-compiler/src/compile.rs` → root `reify-compiler`
/// - `gui/src/main.rs` → no root (not under `crates/`)
/// - `Cargo.lock` → no root
///
/// Used by H2 to enforce the cross-crate gate (>=2 roots required).
fn crate_root_count(files: &[String]) -> usize {
    let roots: std::collections::HashSet<&str> = files
        .iter()
        .filter_map(|f| {
            let rest = f.strip_prefix("crates/")?;
            rest.split('/').next()
        })
        .collect();
    roots.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DoneProvenance, MockGitOps, MockJCodemunchOps};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Asserts `h2_vacuous_breadcrumb` returns `Some` (with task-id and the word
    /// "vacuous") for an empty symbols slice and `None` for a non-empty slice.
    #[test]
    fn h2_vacuous_breadcrumb_fires_only_when_empty() {
        // Empty slice → Some(msg) containing the task id and "vacuous".
        let result = h2_vacuous_breadcrumb(&[], "4144", "abc123^1", "abc123");
        let msg = result.expect("expected Some for empty symbols slice");
        assert!(
            msg.contains("4144"),
            "breadcrumb message must contain task_id '4144'; got: {msg}"
        );
        assert!(
            msg.contains("vacuous"),
            "breadcrumb message must contain 'vacuous'; got: {msg}"
        );

        // Non-empty slice → None.
        let sym = ChangedSymbol {
            name: "my_fn".to_string(),
            file: "crates/foo/src/lib.rs".to_string(),
            line: 42,
            has_allow_dead_code: false,
            has_cfg_test: false,
            g_allow_marker: None,
        };
        let result = h2_vacuous_breadcrumb(&[sym], "4144", "abc123^1", "abc123");
        assert!(
            result.is_none(),
            "expected None for non-empty symbols slice; got: {result:?}"
        );
    }

    /// Integration: `check_live_path_stranded` with `MockJCodemunchOps`
    /// (which returns `vec![]` by default for all `get_changed_symbols` calls)
    /// and >=2 distinct crate roots returns no findings.
    ///
    /// This pins the empty-symbols branch: once both the cross-crate gate
    /// (>=2 roots) and the commit gate pass, the vacuous path is reached and
    /// the function exits cleanly without producing spurious
    /// `Pattern::P5LivePathStranded` findings.  Capturing the stderr
    /// breadcrumb itself is not necessary — the absence of findings is the
    /// observable contract.
    #[test]
    fn h2_vacuous_path_returns_no_findings() {
        let conn = Connection::open_in_memory().expect("open in-memory runs.db");
        let git = MockGitOps::new();
        // Default MockJCodemunchOps: get_changed_symbols returns vec![] for
        // any (since_sha, until_sha) pair not explicitly seeded — mirrors
        // NoopJCodemunchOps behaviour.
        let jc = MockJCodemunchOps::new();

        let meta = TaskMetadata {
            task_id: "4144".to_string(),
            status: "done".to_string(),
            // Two distinct crates/<name>/ roots → cross-crate gate passes.
            files: vec![
                "crates/reify-eval/src/lib.rs".to_string(),
                "crates/reify-compiler/src/compile.rs".to_string(),
            ],
            done_provenance: Some(DoneProvenance {
                kind: Some("merged".to_string()),
                commit: Some("deadbeef".to_string()),
                note: None,
            }),
            title: "multi-crate vacuous H2 test task".to_string(),
            prd: None,
            consumer_ref: None,
            audit_foundation: None,
            done_at: None,
        };

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = check_live_path_stranded(&ctx, &meta);
        assert!(
            findings.is_empty(),
            "vacuous H2 sweep (empty get_changed_symbols, >=2 crate roots) \
             must yield no findings; got {findings:?}"
        );
    }

    /// Pins the empty-files short-circuit at `p5_phantom_done.rs:215`.
    ///
    /// A `done`/`merged` task whose `metadata.files` is empty has no git
    /// provenance to corroborate beyond the runs.db `task_completed` row. The
    /// short-circuit returns `None` from `check_one`; this test asserts that
    /// `check_pre_done` emits zero findings (no panic, no spurious High).
    ///
    /// The runs.db `task_completed` row is required: without it, the runs.db
    /// leg returns `Ok(false)` and emits a High before reaching the empty-files
    /// guard, which would mask the invariant being pinned here.
    #[test]
    fn empty_files_returns_no_findings() {
        let conn = Connection::open_in_memory().expect("open in-memory runs.db");
        conn.execute_batch("CREATE TABLE events (task_id TEXT, event_type TEXT);")
            .expect("create events table");
        conn.execute(
            "INSERT INTO events (task_id, event_type) VALUES ('9001', 'task_completed')",
            [],
        )
        .expect("insert task_completed event");

        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "9001".to_string(),
            TaskMetadata {
                task_id: "9001".to_string(),
                status: "done".to_string(),
                files: vec![],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("deadbeef".to_string()),
                    note: None,
                }),
                title: "empty-files done task".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = check_pre_done(&ctx, "9001");
        assert!(
            findings.is_empty(),
            "empty-files done task must yield no findings; got {findings:?}"
        );
    }
}
