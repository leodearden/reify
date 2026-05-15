//! P1 — producer-orphan detector.
//!
//! For every public symbol a `done` task introduced (via
//! [`JCodemunchOps::get_changed_symbols`]), flags a Finding when the symbol
//! has no non-test caller in the workspace and no pending/in-progress
//! consumer task — Medium past the 14-day grace window, Low within it.
//!
//! Reference: `docs/architecture-audit/f-infra-design.md` §5 P1.
//!
//! Guards (each suppresses the finding; firing order added incrementally by
//! later steps): `audit_foundation` → stdlib-scope → symbol attributes
//! (`#[allow(dead_code)]`/`#[cfg(test)]`) → `// G-allow:` marker →
//! pending-consumer task → non-test workspace caller → grace window.

use crate::{AuditContext, Finding};

// G-allow: F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md
pub fn check(_ctx: &AuditContext) -> Vec<Finding> {
    // Stub — step-2 GREEN only pins the public surface. Detector logic is
    // built up across steps 4/6/8/10/12.
    Vec::new()
}
