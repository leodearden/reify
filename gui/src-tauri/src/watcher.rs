// File watcher for .ri source files.
//
// Monitors a directory for changes to .ri files and invokes a callback
// with the changed file path. Debounces rapid filesystem events.
// No tauri dependency — fully testable as pure Rust.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Debounce window for filesystem events.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(100);

/// An event emitted by [`FileWatcher`] for each relevant filesystem change.
///
/// The `target_file` filter (set at construction time) applies **only** to
/// `Changed` events — `Removed` events are emitted for any `.ri` file in
/// the watched directory regardless of the filter.  This ensures that sibling
/// scratch files (which DO exist as open tabs in the frontend store) surface
/// their removal even when the engine was launched with a different primary file.
///
/// `Debug` is derived so tests can format events in assertion messages without
/// a custom `Display` impl.
#[derive(Debug)]
pub enum FileEvent {
    /// A `.ri` file was created or modified.
    Changed(PathBuf),
    /// A `.ri` file was deleted from the watched directory.
    Removed(PathBuf),
}

/// Watches a directory for .ri file changes and invokes a callback.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Create a new FileWatcher that monitors `dir` for .ri file changes.
    ///
    /// When `target_file` is `Some`, only **`Changed`** events for the file
    /// with that name trigger the callback; `Removed` events bypass this
    /// filter and fire for any `.ri` file in the directory.
    /// When `None`, all `.ri` `Changed` events trigger the callback.
    ///
    /// The `callback` is invoked with a [`FileEvent`], debounced to avoid
    /// rapid duplicate notifications.
    pub fn new<F>(dir: &Path, target_file: Option<PathBuf>, callback: F) -> Result<Self, String>
    where
        F: Fn(FileEvent) + Send + 'static,
    {
        let last_seen: Arc<Mutex<HashMap<PathBuf, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let is_remove = matches!(event.kind, EventKind::Remove(_));
                    let is_change = matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_)
                    );

                    if !is_remove && !is_change {
                        return;
                    }

                    for path in event.paths {
                        // Filter to .ri files only
                        if !path.extension().is_some_and(|ext| ext == "ri") {
                            continue;
                        }

                        // target_file filter: applies to Changed events only.
                        // Removed events bypass the filter (see module doc).
                        if is_change {
                            if let Some(ref target) = target_file
                                && path.file_name() != target.file_name()
                            {
                                continue;
                            }
                        }

                        // Debounce: skip if we've seen this path recently
                        let mut guard = last_seen.lock().unwrap();
                        let now = Instant::now();
                        if let Some(last) = guard.get(&path)
                            && now.duration_since(*last) < DEBOUNCE_DURATION
                        {
                            continue;
                        }
                        guard.insert(path.clone(), now);
                        drop(guard);

                        let file_event = if is_remove {
                            FileEvent::Removed(path)
                        } else {
                            FileEvent::Changed(path)
                        };
                        callback(file_event);
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| format!("Failed to create file watcher: {}", e))?;

        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;

        Ok(FileWatcher { _watcher: watcher })
    }
}
