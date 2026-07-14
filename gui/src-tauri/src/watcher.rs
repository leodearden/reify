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

/// The kind of filesystem change recorded by [`Debouncer`] for a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeKind {
    /// The path was created or modified.
    Changed,
    /// The path was removed.
    Removed,
}

/// A pending (not-yet-emitted) change for a single path.
struct Pending {
    kind: ChangeKind,
    last_seen: Instant,
}

/// Pure, clock-injected trailing-edge debouncer with per-path coalescing.
///
/// Every [`record`](Debouncer::record) call for a path resets that path's
/// quiet window and overwrites its kind with the latest one observed. A
/// path only becomes ready — and is returned by
/// [`drain_ready`](Debouncer::drain_ready) — once `window` has elapsed
/// since its *last* record, so rapid bursts of events for the same path
/// (e.g. a non-atomic truncate-then-append write) coalesce into a single
/// emission carrying the most recent kind, fired after things go quiet.
///
/// All methods take an explicit `now: Instant` rather than reading the
/// clock themselves, so the trailing-edge + coalescing contract can be
/// pinned deterministically in tests with synthetic instants.
pub(crate) struct Debouncer {
    window: Duration,
    pending: HashMap<PathBuf, Pending>,
}

impl Debouncer {
    /// Create a new debouncer with the given quiet-window duration.
    pub(crate) fn new(window: Duration) -> Self {
        Debouncer {
            window,
            pending: HashMap::new(),
        }
    }

    /// Record a filesystem change for `path` observed at `now`.
    ///
    /// Insert-or-update: this overwrites any existing pending entry for
    /// `path` with the new `kind` and resets its quiet window to start
    /// counting from `now`.
    pub(crate) fn record(&mut self, path: PathBuf, kind: ChangeKind, now: Instant) {
        self.pending.insert(path, Pending { kind, last_seen: now });
    }

    /// Remove and return every pending path whose quiet window has
    /// elapsed as of `now` (i.e. `now.duration_since(last_seen) >= window`).
    pub(crate) fn drain_ready(&mut self, now: Instant) -> Vec<(PathBuf, ChangeKind)> {
        let ready_paths: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, p)| now.duration_since(p.last_seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect();

        ready_paths
            .into_iter()
            .map(|path| {
                let pending = self.pending.remove(&path).expect("key just observed in iter");
                (path, pending.kind)
            })
            .collect()
    }

    /// The smallest remaining time until some pending path becomes ready,
    /// or `None` if nothing is pending.
    ///
    /// Saturates to `Duration::ZERO` for a path that's already due (i.e.
    /// its window has already elapsed as of `now`).
    pub(crate) fn next_wait(&self, now: Instant) -> Option<Duration> {
        self.pending
            .values()
            .map(|p| {
                let elapsed = now.duration_since(p.last_seen);
                self.window.saturating_sub(elapsed)
            })
            .min()
    }
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
                        if path.extension().is_none_or(|ext| ext != "ri") {
                            continue;
                        }

                        // target_file filter: applies to Changed events only.
                        // Removed events bypass the filter (see module doc).
                        if is_change
                            && let Some(ref target) = target_file
                            && path.file_name() != target.file_name()
                        {
                            continue;
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
