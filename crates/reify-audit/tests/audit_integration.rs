//! Integration smoke test for the F-infra slice 1 detector core.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §14 hand-off):
//!   `cargo test -p reify-audit --test audit_integration`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary (the binary's own filename is not
//! part of those paths). To make the substring `audit_integration::tests`
//! resolve, the file body is wrapped in
//! `mod audit_integration { mod tests { ... } }` so each test's path
//! becomes `audit_integration::tests::<name>` — matching the p5.rs/p1.rs/p2.rs
//! convention.
//!
//! Re-exercises the three detectors through the public lib surface with no
//! detector-internal seams. All fixtures are in-memory; no git repo, no
//! runs.db file required.

mod audit_integration {

use reify_audit::{
    AuditContext, ChangedSymbol, DoneProvenance, EvidenceRef, MockGitOps, MockJCodemunchOps,
    Pattern, Severity, TaskMetadata, p1_producer_orphan, p2_consumer_stub, p5_phantom_done,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// Minimal schema — pin reflects only the columns the production
/// `has_task_completed_event` query reads (`events.task_id` and
/// `events.event_type`). Verbatim from p5.rs:32 — intentional duplication;
/// if the schema changes, both p5.rs and this file must be updated, giving
/// two pinning sites that catch missed updates.
const RUNS_DB_SCHEMA: &str = r#"
CREATE TABLE events (task_id TEXT, event_type TEXT);
"#;

/// Open an in-memory SQLite connection and seed the events-table schema.
fn seed_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(RUNS_DB_SCHEMA).expect("create schema");
    conn
}

/// Seed a single `task_completed` event row for `task_id`.
fn insert_task_completed_event(conn: &Connection, task_id: &str) {
    conn.execute(
        "INSERT INTO events (task_id, event_type) VALUES (?, ?)",
        rusqlite::params![task_id, "task_completed"],
    )
    .unwrap();
}

/// Builder for the pre-`/prd` legacy shape used by the false-positive
/// cross-check (step-4): status=done, files=vec![], no done_provenance,
/// no prd/consumer_ref, no done_at, benign title. This shape clears all
/// three detectors without triggering any false positive.
fn legacy_meta(task_id: &str) -> TaskMetadata {
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

/// Fixed synthetic "now" (epoch-seconds) so grace-window boundaries are
/// deterministic across runs. Tests derive `done_at` relative to this.
/// Mirrors `const NOW` in p1.rs:29.
const NOW: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

mod tests {
    use super::*;
}

} // mod audit_integration
