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

use crate::{AuditContext, EvidenceRef, Finding, GitCommit, Pattern, Severity, TaskMetadata};

/// The git ref the detector diffs claimed commits *against*. Production runs
/// against `main`; the integration tests configure their `MockGitOps` with
/// this exact string so the keys line up.
const MAIN_BASE: &str = "main";

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
// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md — defensive; current caller count is incidental (script name-collision heuristic may shadow this via stdlib `check`)
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
// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md
pub fn check_pre_done(ctx: &AuditContext, task_id: &str) -> Vec<Finding> {
    let Some(meta) = ctx.task_metadata.get(task_id) else {
        return vec![];
    };
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
    findings
}

/// Shared inner routine for [`check`] and [`check_pre_done`]. Borrows the
/// context (no clone of `task_metadata`) and threads the optional
/// single-task override through the filter so both entry points share one
/// code path for the P5 invariant.
fn check_with_target(ctx: &AuditContext, target_task_id: Option<&str>) -> Vec<Finding> {
    let mut findings = Vec::new();

    for meta in ctx.task_metadata.values() {
        if meta.status != "done" {
            continue;
        }
        if let Some(target) = target_task_id
            && meta.task_id != target
        {
            continue;
        }

        if let Some(finding) = check_one(ctx, meta) {
            findings.push(finding);
        }
        if let Some(finding) = check_gitignored(ctx, meta) {
            findings.push(finding);
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
        pattern: Pattern::MetadataFilesGitignored,
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

    Some(build_high_finding(
        meta,
        &missing,
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
    let mut stmt = ctx.conn.prepare(
        "SELECT 1 FROM events WHERE task_id = ? AND event_type = 'task_completed' LIMIT 1",
    )?;
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
