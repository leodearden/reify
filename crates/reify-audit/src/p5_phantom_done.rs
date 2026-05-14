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
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    for meta in ctx.task_metadata.values() {
        if meta.status != "done" {
            continue;
        }

        if let Some(finding) = check_one(ctx, meta) {
            findings.push(finding);
        }
        // TODO step-10: gitignore pre-pass adds a separate Severity::Medium
        // finding here when any metadata.files entry is gitignored
        // (memory: project_steward_metadata_files_gitignore_falsepositive.md).
    }

    findings
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
    if kind == "merged" && !has_task_completed_event(ctx, &meta.task_id) {
        return Some(build_high_finding(
            meta,
            &meta.files,
            "metadata.status=done but no task_completed event in runs.db",
        ));
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
    // Memory: project_post_merge_equivalence_false_positive_cargo_lock.md.
    if is_cargo_lock_only(&missing) {
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

/// Run the runs.db existence query: returns true iff at least one
/// `task_completed` event exists for `task_id`.
fn has_task_completed_event(ctx: &AuditContext, task_id: &str) -> bool {
    let mut stmt = match ctx
        .conn
        .prepare("SELECT 1 FROM events WHERE task_id = ? AND event_type = 'task_completed' LIMIT 1")
    {
        Ok(s) => s,
        // If the schema is missing, treat as "no corroborating event" — the
        // detector's job is to flag, not to crash on a malformed runs.db.
        Err(_) => return false,
    };
    stmt.query_row::<i64, _, _>(rusqlite::params![task_id], |row| row.get(0))
        .is_ok()
}

/// Returns the subset of `files` not present in `covered`.
fn files_missing_from(files: &[String], covered: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| !covered.contains(f))
        .cloned()
        .collect()
}

fn is_cargo_lock_only(missing: &[String]) -> bool {
    missing.len() == 1 && missing[0] == "Cargo.lock"
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
