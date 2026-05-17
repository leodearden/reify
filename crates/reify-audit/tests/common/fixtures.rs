//! Canonical TaskMetadata fixture builders for reify-audit integration tests.
//!
//! The `legacy_meta` helper produces the pre-`/prd` "all-None" shape used as
//! a base by tests that want the canonical legacy fixture. Varied fixtures
//! construct `TaskMetadata` inline (or use struct update syntax `..legacy_meta(id)`)
//! to customize individual fields.
//!
//! Items carry `#[allow(dead_code)]` because each test binary consumes only a
//! subset — mirrors the reify-cli/reify-eval convention.

use reify_audit::TaskMetadata;

/// Builder for the pre-`/prd` legacy shape: status=done, files=vec![],
/// no done_provenance, no prd/consumer_ref/audit_foundation/done_at, benign
/// title. This shape clears all three detectors without triggering any false
/// positive (P5 early-returns on `done_provenance.as_ref()?`, P1 on
/// `done_at=None`, P2 on the empty `files` loop).
#[allow(dead_code)]
pub fn legacy_meta(task_id: &str) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        status: "done".to_string(),
        files: vec![],
        done_provenance: None,
        title: "Wire foo into bar".to_string(),
        prd: None,
        consumer_ref: None,
        audit_foundation: None,
        done_at: None,
    }
}
