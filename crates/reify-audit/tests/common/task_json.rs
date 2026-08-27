//! The `tasks.json` record fixtures, and P1's eligibility rules, in ONE place.
//!
//! These build the WIRE shape — a `serde_json::Value` matching the on-disk
//! `tasks.json` record the `reify-audit` BINARY parses — which is why they are
//! not in `common/fixtures.rs`: that module builds the in-process
//! `reify_audit::TaskMetadata` struct for library-level tests. The two are
//! complementary, not redundant; a binary-level test cannot use the struct.
//!
//! # Why they live here rather than in either consumer
//!
//! [`done_task_fixture`] encodes P1's eligibility rules, and a fixture that
//! gets them wrong does not fail — it makes the run VACUOUS, because the
//! detector skips the record before reaching `get_changed_symbols` at all.
//! Two test binaries need a P1-eligible record for two different reasons:
//!
//! - `tests/cli.rs` (`freshness_gate::write_p1_done_task`) drives the real
//!   binary against a mock MCP endpoint, hermetically, on the merge gate.
//! - `tests/jcodemunch_live.rs` (`write_synthetic_done_task`) drives the real
//!   binary against a REAL serve over a throwaway corpus.
//!
//! When each binary hand-spelled the same nine-field record, the rules — and
//! the paragraph explaining them — lived in two places. A future field that
//! P1 begins to require would then have to be found and fixed twice, and the
//! copy that was missed would go quietly inert rather than red: the same
//! lockstep hazard `common/breadcrumbs.rs` was created to close, applied to
//! fixtures. Here it is one edit, and both consumers move.
//!
//! # How it is wired in
//!
//! Declared by each consuming test binary as
//! `#[path = "common/task_json.rs"] mod task_json;` rather than through
//! `common/mod.rs`, matching `common/breadcrumbs.rs`. Cargo only promotes
//! top-level `tests/*.rs` files (and subdirectories carrying a `main.rs`) to
//! test targets, so a plain module file in `tests/common/` compiles into its
//! consumers and never becomes a test binary of its own.
//!
//! Items carry `#[allow(dead_code)]` because each consumer uses a subset.

/// Minimal tasks.json fixture object with all 9 required TaskMetadata fields.
/// Returns a serde_json::Value so callers can override fields as needed.
#[allow(dead_code)]
pub fn task_fixture(
    task_id: &str,
    status: &str,
    kind: Option<&str>,
    commit: Option<&str>,
) -> serde_json::Value {
    let done_provenance = match kind {
        Some(k) => serde_json::json!({
            "kind": k,
            "commit": commit,
            "note": null
        }),
        None => serde_json::Value::Null,
    };
    serde_json::json!({
        "task_id": task_id,
        "status": status,
        "files": ["crates/reify-audit/src/lib.rs"],
        "done_provenance": done_provenance,
        "title": format!("Task {}", task_id),
        "prd": null,
        "consumer_ref": null,
        "audit_foundation": null,
        "done_at": null
    })
}

/// A [`task_fixture`] that P1 will actually CONSIDER: `status: "done"`, a
/// `done_provenance.commit`, and a NON-NULL `done_at`.
///
/// P1 skips a task whose `done_at` is null, and one whose
/// `done_provenance.commit` is absent — both guards sit at the top of
/// `p1_producer_orphan::check`'s per-task loop, before it derives a range.
/// `task_fixture` hardcodes `done_at: null` because most of its callers are
/// exercising other detectors. A P1 fixture that inherited that null would be
/// vacuous no matter what the code under test did: the detector would never
/// reach `get_changed_symbols`.
///
/// Callers supply only what is scenario-specific — the `commit` P1 derives
/// `{commit}^1..{commit}` from, and typically an override of `files` naming a
/// path that exists where the binary is pointed.
#[allow(dead_code)]
pub fn done_task_fixture(task_id: &str, commit: &str, done_at: u64) -> serde_json::Value {
    let mut task = task_fixture(task_id, "done", Some("merged"), Some(commit));
    task["done_at"] = serde_json::json!(done_at);
    task
}
