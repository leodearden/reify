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

use crate::{AuditContext, Finding};

/// Run the P5 detector across every `status="done"` task in
/// `ctx.task_metadata`. Returns one [`Finding`] per phantom-done task.
///
/// The slice-1 implementation returns the empty vec — step-4 onward layers
/// in the actual corroboration logic and false-positive guards.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let _ = ctx
        .task_metadata
        .values()
        .filter(|m| m.status == "done")
        .count();
    // Step-4 onward populates this.
    Vec::new()
}
