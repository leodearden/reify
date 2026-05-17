//! Shared SQLite schema helpers for reify-audit integration tests.
//!
//! These items are shared between `p5.rs`, `audit_integration.rs`, and any
//! future test binary that needs the runs.db events-table schema.
//!
//! The helpers carry `#[allow(dead_code)]` because each test binary
//! consumes only a subset — mirrors the reify-cli/reify-eval convention.

use rusqlite::Connection;

/// Minimal schema — pin reflects only the columns the production
/// `has_task_completed_event` query reads (`events.task_id` and
/// `events.event_type`). P1/P2 detectors (landed via T-2/T-3) issue zero SQL,
/// so they add no columns here; future detectors that DO query the DB will add
/// the columns they need when those queries land.
#[allow(dead_code)]
pub const RUNS_DB_SCHEMA: &str = r#"
CREATE TABLE events (task_id TEXT, event_type TEXT);
"#;

/// Open an in-memory SQLite connection and seed the events-table schema.
#[allow(dead_code)]
pub fn seed_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(RUNS_DB_SCHEMA).expect("create schema");
    conn
}

/// Seed a single event row for `task_id` with the given `event_type`.
#[allow(dead_code)]
pub fn insert_event(conn: &Connection, task_id: &str, event_type: &str) {
    conn.execute(
        "INSERT INTO events (task_id, event_type) VALUES (?, ?)",
        rusqlite::params![task_id, event_type],
    )
    .unwrap();
}

/// Seed a single `task_completed` event row for `task_id`.
#[allow(dead_code)]
pub fn insert_task_completed_event(conn: &Connection, task_id: &str) {
    insert_event(conn, task_id, "task_completed");
}
