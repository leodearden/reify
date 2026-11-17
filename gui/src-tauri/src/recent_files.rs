//! The Start view's Recent list (task 9730): the documents most recently opened,
//! newest first, persisted as JSON at a store path the caller chooses (the bin
//! puts it under the Tauri app-data dir).

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One opened document: its canonical document key and when it was opened, in
/// unix milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: String,
    pub opened_at: u64,
}

pub const RECENT_FILES_LIMIT: usize = 10;

/// The stored list, newest first. A missing store is an empty list; an
/// unreadable or corrupt one is an empty list too, with a warning naming it.
pub fn list_recent(store: &Path) -> Vec<RecentEntry> {
    let bytes = match std::fs::read(store) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(store = %store.display(), %error, "recent files: unreadable store; listing none");
            return Vec::new();
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        tracing::warn!(store = %store.display(), %error, "recent files: corrupt store; listing none");
        Vec::new()
    })
}

/// Put `path` at the front of the stored list, under the same canonical key
/// [`crate::commands::open_file_impl`] returns, dropping any older entry for it
/// and keeping at most [`RECENT_FILES_LIMIT`]. Returns the list as written.
pub fn record_recent(store: &Path, path: &str, opened_at: u64) -> Result<Vec<RecentEntry>, String> {
    let key = crate::path_key::canonicalize_document_key(path);
    let mut entries = list_recent(store);
    entries.retain(|e| e.path != key);
    entries.insert(
        0,
        RecentEntry {
            path: key,
            opened_at,
        },
    );
    entries.truncate(RECENT_FILES_LIMIT);
    write_store(store, &entries)
        .map_err(|e| format!("Error writing recent files {}: {e}", store.display()))?;
    Ok(entries)
}

/// Replace `store` with `entries` atomically (a sibling temp file, then a rename),
/// creating its directory first.
fn write_store(store: &Path, entries: &[RecentEntry]) -> std::io::Result<()> {
    let dir = match store.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&serde_json::to_vec_pretty(entries)?)?;
    tmp.persist(store).map_err(|e| e.error)?;
    Ok(())
}
