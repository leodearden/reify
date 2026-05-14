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

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity, TaskMetadata};

/// The git ref the detector diffs claimed-merge commits *against*. Production
/// runs against `main`; the integration tests configure their `MockGitOps`
/// with this exact string so the keys line up.
const MAIN_BASE: &str = "main";

/// Run the P5 detector across every `status="done"` task in
/// `ctx.task_metadata`. Returns one [`Finding`] per phantom-done task.
///
/// Slice-1 corroboration logic, per `f-infra-design.md` §10:
/// 1. For `kind="merged"`: confirm a `task_completed` event exists in
///    runs.db AND `git diff main..<claimed_commit>` covers every path in
///    `metadata.files`.
/// 2. For `kind="found_on_main"`: confirm `git log main --grep <task_id>`
///    returns ≥1 commit (the path coverage is checked by the same logic).
///
/// Mismatches produce `Severity::High` findings. The false-positive guards
/// (steps 6/8/10 of T-1) downgrade to `Low` or emit a separate
/// `Severity::Medium` gitignore-flag finding.
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

    // Compute "missing" — the metadata.files entries NOT covered by the
    // primary corroborating diff. Empty missing = clean done.
    let missing: Vec<String> = match kind {
        "merged" => {
            let commit = prov.commit.as_deref().unwrap_or("");
            // Corroboration (a): runs.db has a task_completed event.
            // (Memory: procedural_runs_db_forensics.md.) When the row is
            // absent, ALL metadata.files count as un-corroborated — the
            // phantom-done has no orchestrator-side trail at all.
            if !has_task_completed_event(ctx, &meta.task_id) {
                return Some(build_high_finding(meta, &meta.files));
            }
            // Corroboration (b): the claimed commit's diff covers
            // every metadata.files path.
            let diff = ctx.git.diff_changed_paths(MAIN_BASE, commit);
            files_missing_from(&meta.files, &diff)
        }
        "found_on_main" => {
            // Per design doc §10: a sibling commit must exist mentioning
            // the task_id, and its diff must touch ≥1 metadata.files path.
            let hits = ctx.git.log_grep(MAIN_BASE, &meta.task_id);
            if hits.is_empty() {
                return Some(build_high_finding(meta, &meta.files));
            }
            // Aggregate sibling diffs — any path covered by any hit counts.
            let mut covered: Vec<String> = Vec::new();
            for c in &hits {
                covered.extend(ctx.git.diff_changed_paths(MAIN_BASE, &c.sha));
            }
            files_missing_from(&meta.files, &covered)
        }
        // Unknown / absent kind — leave to T-2/T-3 to refine. For slice-1,
        // we don't second-guess; treat as corroborated.
        _ => return None,
    };

    if missing.is_empty() {
        return None;
    }

    // Cargo.lock-only divergence guard. When the lone missing entry is
    // Cargo.lock — and every other metadata.files path was corroborated by
    // the claimed commit's diff — main has merely absorbed an unrelated
    // dependency bump after our task wrote its lockfile. Not phantom-done.
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

    // TODO step-8: convergent-fast-forward guard
    //   (memory: project_unblock_convergent_ff_worktree_reap.md)
    //   — if `git.log_grep("main", task_id)` finds sibling commits whose
    //   diffs cover the entire missing set, downgrade to Severity::Low and
    //   add EvidenceRef::Commit per contributing sibling.

    Some(build_high_finding(meta, &missing))
}

fn is_cargo_lock_only(missing: &[String]) -> bool {
    missing.len() == 1 && missing[0] == "Cargo.lock"
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

/// Construct a `Severity::High` phantom-done finding listing the missing
/// metadata.files entries as the primary evidence.
fn build_high_finding(meta: &TaskMetadata, missing: &[String]) -> Finding {
    Finding {
        pattern: Pattern::P5PhantomDone,
        severity: Severity::High,
        task_id: meta.task_id.clone(),
        summary: "metadata.files mismatch / commit not reachable from main".to_string(),
        evidence: vec![EvidenceRef::MetadataFiles {
            entries: missing.to_vec(),
        }],
    }
}
