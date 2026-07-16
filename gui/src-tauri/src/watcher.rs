// File watcher for .ri source files.
//
// Monitors a directory for changes to .ri files and invokes a callback
// with the changed file path. Debounces rapid filesystem events.
// No tauri dependency — fully testable as pure Rust.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
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
///
/// `record`, `drain_ready`, and `next_wait` all scan the full `pending` map
/// (O(n) per call). That's an accepted tradeoff, not an oversight: this
/// GUI watches one target file (plus a handful of open scratch tabs), so
/// `pending` holds at most a few entries and a linear scan is simpler and
/// easier to reason about than a min-heap keyed by deadline. Revisit if
/// this is ever pointed at a directory with many concurrently-changing
/// `.ri` files.
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
        // Single pass: `retain` visits each entry once and removes it
        // in-place, so a ready entry is neither re-hashed nor looked up a
        // second time (unlike a filter-then-remove-by-key two-pass).
        let mut ready = Vec::new();
        let window = self.window;
        self.pending.retain(|path, pending| {
            if now.duration_since(pending.last_seen) >= window {
                ready.push((path.clone(), pending.kind));
                false
            } else {
                true
            }
        });
        ready
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
///
/// Filesystem events are trailing-edge debounced (see [`Debouncer`]) by a
/// single background worker thread that owns `callback`. `Drop` shuts that
/// worker down and joins it so no thread outlives the watcher.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    worker: Option<JoinHandle<()>>,
    shared: Arc<(Mutex<Debouncer>, Condvar)>,
    shutdown: Arc<AtomicBool>,
}

impl FileWatcher {
    /// Create a new FileWatcher that monitors `dir` for .ri file changes.
    ///
    /// When `target_file` is `Some`, only **`Changed`** events for the file
    /// with that name trigger the callback; `Removed` events bypass this
    /// filter and fire for any `.ri` file in the directory.
    /// When `None`, all `.ri` `Changed` events trigger the callback.
    ///
    /// The `callback` is invoked with a [`FileEvent`] after filesystem
    /// events for a path go quiet for `DEBOUNCE_DURATION` (trailing-edge
    /// debounce), so a non-atomic write (e.g. truncate followed by a
    /// separate append) coalesces into a single callback firing that
    /// observes the final on-disk content rather than a transient partial
    /// buffer. This is a deliberate tradeoff: it adds a fixed
    /// ~`DEBOUNCE_DURATION` minimum latency to *every* change notification,
    /// including ordinary atomic saves that a leading-edge throttle would
    /// have fired instantly. For this GUI's live-reload use case that
    /// ~100ms delay is imperceptible next to the correctness win of never
    /// getting stuck on a partially-written file; a hybrid leading+trailing
    /// scheme (fire immediately, then also coalesce the tail) was considered
    /// and rejected to keep the debounce/coalescing contract in one
    /// deterministically-testable code path (see [`Debouncer`]).
    ///
    /// `callback` runs on a dedicated worker thread and **must not block
    /// for an unbounded time**: [`Drop`] joins that worker to shut it down
    /// cleanly, so a callback invocation that never returns will make
    /// dropping the `FileWatcher` hang indefinitely. A *panicking* callback
    /// is caught and logged rather than killing the worker (see the worker
    /// loop), so `callback` only needs to be non-blocking, not panic-free.
    pub fn new<F>(dir: &Path, target_file: Option<PathBuf>, callback: F) -> Result<Self, String>
    where
        F: Fn(FileEvent) + Send + 'static,
    {
        let shared: Arc<(Mutex<Debouncer>, Condvar)> = Arc::new((
            Mutex::new(Debouncer::new(DEBOUNCE_DURATION)),
            Condvar::new(),
        ));
        let shutdown = Arc::new(AtomicBool::new(false));

        let notify_shared = shared.clone();

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

                        let kind = if is_remove {
                            ChangeKind::Removed
                        } else {
                            ChangeKind::Changed
                        };

                        let (lock, cvar) = &*notify_shared;
                        // Recover from a poisoned mutex rather than propagating
                        // the panic: `Debouncer`'s methods are simple and not
                        // expected to panic, but if one ever did while holding
                        // this lock, letting every subsequent `.lock()` here
                        // and in the worker panic too would permanently and
                        // silently kill event delivery (this closure would
                        // keep recording into the debouncer with no live
                        // consumer left to drain it). `into_inner()` just
                        // recovers the guard; the original panic still prints
                        // via the default panic hook.
                        lock.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .record(path, kind, Instant::now());
                        cvar.notify_one();
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| format!("Failed to create file watcher: {}", e))?;

        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;

        let worker_shared = shared.clone();
        let worker_shutdown = shutdown.clone();
        let worker = std::thread::spawn(move || {
            let (lock, cvar) = &*worker_shared;
            // Poison-recovering lock, same rationale as the notify closure above.
            let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if worker_shutdown.load(Ordering::SeqCst) {
                    return;
                }

                // Only the `callback` invocation further down is isolated
                // with `catch_unwind`. A hypothetical panic inside
                // `drain_ready`/`next_wait` here (or in the poison-recovery
                // closures used throughout this function) would NOT be
                // caught: it would unwind and terminate this worker thread,
                // and since nothing else drains the debouncer, every
                // subsequent event would silently stop being delivered
                // until `Drop` joins the dead thread and logs the failure.
                // This asymmetry is accepted rather than also wrapped,
                // because these are pure, simple operations over an
                // in-memory map with no I/O (see `Debouncer`'s doc comment)
                // -- unlike `callback`, which is arbitrary caller-provided
                // code and is explicitly isolated below because it can do
                // anything.
                let now = Instant::now();
                let ready = guard.drain_ready(now);

                if ready.is_empty() {
                    guard = match guard.next_wait(now) {
                        Some(wait) => cvar
                            .wait_timeout(guard, wait)
                            .unwrap_or_else(|e| e.into_inner())
                            .0,
                        None => cvar.wait(guard).unwrap_or_else(|e| e.into_inner()),
                    };
                    continue;
                }

                // Release the lock before invoking the callback so a slow
                // or reentrant callback never blocks the notify thread from
                // recording new events.
                //
                // Note: `shutdown` is intentionally not re-checked here.
                // This batch was already returned by `drain_ready` above,
                // so every event in it is still delivered to `callback`
                // even if `Drop` concurrently requests shutdown while this
                // loop runs -- `shutdown` is only consulted at the top of
                // the loop, before the *next* `drain_ready` call. Only
                // entries that are still pending (not yet ready) at that
                // point are discarded on shutdown; anything already drained
                // into `ready` always fires.
                drop(guard);
                for (path, kind) in ready {
                    let file_event = match kind {
                        ChangeKind::Changed => FileEvent::Changed(path),
                        ChangeKind::Removed => FileEvent::Removed(path),
                    };
                    // Isolate a panicking callback: without this, a single
                    // bad event would unwind and terminate this worker
                    // thread, and since nothing else drains the debouncer,
                    // every future event would silently stop being
                    // delivered (the notify closure would keep recording
                    // into it with no consumer left).
                    if let Err(payload) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            callback(file_event);
                        }))
                    {
                        let message = payload
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "<non-string panic payload>".to_string());
                        eprintln!("FileWatcher callback panicked (continuing to watch): {message}");
                    }
                }
                guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            }
        });

        Ok(FileWatcher {
            _watcher: watcher,
            worker: Some(worker),
            shared,
            shutdown,
        })
    }

    /// Test-only observability hook: a snapshot of the paths currently
    /// pending in the debouncer (recorded by the notify closure but not yet
    /// drained/fired). Lets a test confirm an event was actually recorded
    /// before racing it against something else (e.g. dropping the
    /// watcher), rather than assuming a fixed `sleep` was long enough —
    /// otherwise a slow/loaded host could make the race pass vacuously
    /// (nothing recorded yet) rather than by exercising the intended
    /// behavior.
    #[cfg(test)]
    pub(crate) fn pending_paths(&self) -> Vec<PathBuf> {
        let (lock, _cvar) = &*self.shared;
        lock.lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .keys()
            .cloned()
            .collect()
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        // Set the shutdown flag while holding the same mutex the worker
        // checks it under, so a concurrent check-then-wait in the worker
        // can't race this store and go back to sleep with no one left to
        // wake it (a classic condvar lost-wakeup).
        {
            // Poison-recovering lock, same rationale as the notify closure
            // in `FileWatcher::new`.
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            self.shutdown.store(true, Ordering::SeqCst);
        }
        cvar.notify_all();
        if let Some(handle) = self.worker.take()
            && handle.join().is_err()
        {
            // The worker wraps `callback` in `catch_unwind`, so it should
            // never itself panic -- but `drain_ready`/`next_wait` or the
            // poison-recovery path could in principle. Surface that rather
            // than swallowing it silently, mirroring the callback-panic
            // logging above.
            eprintln!("FileWatcher worker thread panicked during shutdown");
        }
    }
}
