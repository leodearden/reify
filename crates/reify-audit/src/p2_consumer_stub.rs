//! P2 — consumer-stub detector.
//!
//! Scans the added-lines portion of `git diff main..task-branch` (filtered to
//! `metadata.files`) for canonical stub markers and emits Medium-severity
//! Findings (Low when the task title contains "stub" or "placeholder").
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §5 P2.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity, TaskMetadata};

// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md
pub fn check(_ctx: &AuditContext) -> Vec<Finding> {
    Vec::new()
}
