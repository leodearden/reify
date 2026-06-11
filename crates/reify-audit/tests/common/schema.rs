//! Shared SQLite schema helpers for reify-audit integration tests.
//!
//! These items are shared between `p5.rs`, `audit_integration.rs`, and any
//! future test binary that needs the runs.db events-table schema.
//!
//! The helpers carry `#[allow(dead_code)]` because each test binary
//! consumes only a subset — mirrors the reify-cli/reify-eval convention.

use rusqlite::Connection;
use std::path::Path;

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

// -----------------------------------------------------------------------
// PTODO liveness lane — `.taskmaster/tasks/tasks.db` `tasks` table
// -----------------------------------------------------------------------

/// Minimal `tasks`-table schema, mirroring the live `.taskmaster/tasks/tasks.db`
/// (`PRIMARY KEY(tag, id)`, `id INTEGER`, `status TEXT NOT NULL`). Only the
/// columns the PTODO liveness query reads (`tag`, `id`, `status`) are pinned;
/// the production table has more columns the detector ignores.
#[allow(dead_code)]
pub const TASKS_DB_SCHEMA: &str = r#"
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    PRIMARY KEY (tag, id)
);
"#;

/// Open an in-memory SQLite connection and seed the `tasks`-table schema.
/// Rows are added with [`insert_task`]. Used by `resolve_liveness` unit tests.
#[allow(dead_code)]
pub fn seed_tasks_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(TASKS_DB_SCHEMA).expect("create tasks schema");
    conn
}

/// Seed a single `(tag, id, status)` row into the `tasks` table.
#[allow(dead_code)]
pub fn insert_task(conn: &Connection, tag: &str, id: i64, status: &str) {
    conn.execute(
        "INSERT INTO tasks (tag, id, status) VALUES (?, ?, ?)",
        rusqlite::params![tag, id, status],
    )
    .unwrap();
}

/// Create an on-disk `tasks.db` at `path` (creating parent dirs), seed the
/// `tasks` schema, and insert each `(tag, id, status)` row. Used by `check()`
/// / CLI tests to seed the default `<root>/.taskmaster/tasks/tasks.db` so the
/// liveness lane resolves cites against it without any env override.
#[allow(dead_code)]
pub fn seed_tasks_db_at(path: &Path, rows: &[(&str, i64, &str)]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all tasks.db parent");
    }
    let conn = Connection::open(path).expect("open on-disk tasks.db");
    conn.execute_batch(TASKS_DB_SCHEMA).expect("create tasks schema");
    for (tag, id, status) in rows {
        insert_task(&conn, tag, *id, status);
    }
}
