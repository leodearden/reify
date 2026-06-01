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
/// Slice-1 ships the wrapper; T-4 will host the CLI subprocess that the
/// hook actually invokes.
pub fn check_pre_done(ctx: &AuditContext, task_id: &str) -> Vec<Finding> {
    let Some(meta) = ctx.task_metadata.get(task_id) else {
        return vec![];
    };
    check_task(ctx, meta)
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

        findings.extend(check_task(ctx, meta));
    }

    findings
}

/// Per-task pass set shared by [`check_pre_done`] (D-1 hot path, O(1) lookup)
/// and the inner loop of [`check_with_target`] (periodic sweep, O(n) iteration).
/// Centralising the pass list here prevents drift when future per-task detectors
/// join the per-task pass set — they get added in exactly one place.
fn check_task(ctx: &AuditContext, meta: &TaskMetadata) -> Vec<Finding> {
    if meta.status != "done" {
        return vec![];
    }
    let mut findings = Vec::new();
    if let Some(f) = check_one(ctx, meta) {
        findings.push(f);
    }
    if let Some(f) = check_gitignored(ctx, meta) {
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
fn check_gitignored(ctx: &AuditContext, meta: &TaskMetadata) -> Option<Finding> {
    let ignored: Vec<String> = meta
        .files
        .iter()
        .filter(|p| ctx.git.is_gitignored(p))
        .cloned()
        .collect();
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
        evidence: vec![EvidenceRef::MetadataFiles { entries: ignored }],
    })
}

/// Per-task corroboration. Returns `Some(Finding)` if the task is
/// phantom-done, `None` if the provenance corroborates cleanly.
fn check_one(ctx: &AuditContext, meta: &TaskMetadata) -> Option<Finding> {
    let prov = meta.done_provenance.as_ref()?;
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
                    && ctx.git.is_ancestor(commit, MAIN_BASE)
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
        Some(commit) => ctx.git.diff_changed_paths(MAIN_BASE, commit),
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
    let siblings = ctx.git.log_grep(MAIN_BASE, &meta.task_id);
    if !siblings.is_empty() {
        let mut sibling_covered: Vec<String> = Vec::new();
        let mut contributing: Vec<&GitCommit> = Vec::new();
        for c in &siblings {
            let diff = ctx.git.diff_changed_paths(MAIN_BASE, &c.sha);
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
    let genuinely_absent: Vec<String> = meta
        .files
        .iter()
        .filter(|p| !ctx.git.path_tracked_on(MAIN_BASE, p))
        .cloned()
        .collect();

    if genuinely_absent.is_empty() {
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
            // provenance locator. `genuinely_absent` is empty here so citing
            // it would produce an uninformative empty list; the operator
            // instead sees which paths the stale commit was supposed to cover.
            evidence: vec![EvidenceRef::MetadataFiles {
                entries: missing.clone(),
            }],
        });
    }

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
    use crate::{ChangedSymbol, DoneProvenance, MockGitOps, MockJCodemunchOps};
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
