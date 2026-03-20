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

/// Watches a directory for .ri file changes and invokes a callback.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Create a new FileWatcher that monitors `dir` for .ri file changes.
    ///
    /// The `callback` is invoked with the path of each changed .ri file,
    /// debounced to avoid rapid duplicate notifications.
    pub fn new<F>(dir: &Path, callback: F) -> Result<Self, String>
    where
        F: Fn(PathBuf) + Send + 'static,
    {
        let last_seen: Arc<Mutex<HashMap<PathBuf, Instant>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    // Only process create/modify events
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {}
                        _ => return,
                    }

                    for path in event.paths {
                        // Filter to .ri files only
                        if path.extension().is_some_and(|ext| ext == "ri") {
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

                            callback(path);
                        }
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
