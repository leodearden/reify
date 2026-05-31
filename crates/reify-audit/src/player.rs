//! P-LAYER — import-layer-violation detector.
//!
//! For every violation reported by [`JCodemunchOps::get_layer_violations`]
//! (equivalent to `mcp__jcodemunch__get_layer_violations()`), emits an
//! advisory [`Finding`] with [`Severity::Low`] (never promoted).
//!
//! Reference: `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3/§4-d/§8.
//!
//! ## Honest-bound / Low-only discipline
//!
//! Findings are pinned to `Severity::Low` and no follow-up tasks are
//! auto-filed. jcodemunch's Rust import-layer analysis accuracy has not been
//! validated against a live reify corpus; promotion to Medium / auto-file is
//! deferred until a live-corpus false-positive review (PRD §8 routing table).
//!
//! Each finding summary notes the advisory nature ("advisory; static
//! import-layer analysis, not runtime") so downstream consumers are not
//! misled.
//!
//! ## No detector-side scope excludes
//!
//! Unlike PDEAD/PUNTESTED, PLAYER applies no detector-side file-path
//! excludes (no stdlib filter, no test-path filter). Layer membership and
//! scope filtering are handled entirely at the rule level in
//! `.jcodemunch.jsonc`: only files whose paths match a layer's `paths`
//! prefix are analysed; files in unassigned paths (test dirs, stdlib `.ri`
//! files) are skipped by the upstream tool. A second detector-side filter
//! would be redundant and could silently drop legitimately-reported
//! violations.
//!
//! ## task_id convention
//!
//! `task_id` is left empty (`String::new()`): PLAYER is repo-wide, not
//! task-scoped. Downstream consumers already tolerate empty task_ids (same
//! convention as PDEAD/PUNTESTED).

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

/// Check for import-layer violations across the reify compilation spine.
///
/// Iterates [`JCodemunchOps::get_layer_violations`] and maps each
/// [`LayerViolation`] to a [`Finding`] with severity [`Severity::Low`],
/// empty `task_id`, a human-readable summary citing the from-file, to-file,
/// and rule, and a single [`EvidenceRef::File`] pointing at the importing
/// file. The mapping is a faithful 1:1 pass-through — no filtering, merging,
/// or reordering.
///
/// [`LayerViolation`]: crate::LayerViolation
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let mut findings = Vec::new();

    for v in ctx.jcodemunch.get_layer_violations() {
        let summary = format!(
            "{} imports {} in violation of layer rule '{}' \
            (advisory; static import-layer analysis, not runtime)",
            v.from_file, v.to_file, v.rule,
        );

        findings.push(Finding {
            pattern: Pattern::PLayerViolation,
            severity: Severity::Low,
            // Empty task_id is intentional: PLAYER is repo-wide, not task-scoped.
            task_id: String::new(),
            summary,
            evidence: vec![EvidenceRef::File { path: v.from_file.clone() }],
        });
    }

    findings
}
