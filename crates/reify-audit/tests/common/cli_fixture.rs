//! The empty substrates a `reify-audit` BINARY run needs, in ONE place.
//!
//! Every CLI test binary pins `--tasks-file` and `--runs-db` so the run never
//! reaches the live fused-memory server or a shared runs.db. Most want both
//! empty: the detector under test reads neither.
//!
//! # How it is wired in
//!
//! Declared by each consuming test binary as
//! `#[path = "common/cli_fixture.rs"] mod cli_fixture;` rather than through
//! `common/mod.rs`, matching `common/task_json.rs`. Cargo only promotes
//! top-level `tests/*.rs` files (and subdirectories carrying a `main.rs`) to
//! test targets, so a plain module file in `tests/common/` compiles into its
//! consumers and never becomes a test binary of its own.
//!
//! Items carry `#[allow(dead_code)]` because each consumer uses a subset.

use std::path::{Path, PathBuf};

/// Write `dir/tasks.json` holding an empty task array. Returns the path.
#[allow(dead_code)]
pub fn write_empty_tasks_json(dir: &Path) -> PathBuf {
    let path = dir.join("tasks.json");
    std::fs::write(&path, "[]").expect("write tasks.json");
    path
}

/// Create `dir/runs.db` with just the `events` table. Returns the path.
///
/// Idempotent, so a test may invoke the binary twice against one directory.
#[allow(dead_code)]
pub fn write_empty_runs_db(dir: &Path) -> PathBuf {
    let path = dir.join("runs.db");
    let conn = rusqlite::Connection::open(&path).expect("open runs.db");
    conn.execute_batch("CREATE TABLE IF NOT EXISTS events (task_id TEXT, event_type TEXT);")
        .expect("create events table");
    path
}
