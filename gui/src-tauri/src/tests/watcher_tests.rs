use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::watcher::{FileEvent, FileWatcher};

/// Poll `sink` until `predicate` holds or `timeout` elapses.
///
/// The lock is dropped before each sleep so a concurrent producer (e.g. the
/// watcher's callback thread) can still push into `sink` while we wait.
fn wait_for<T>(
    sink: &Arc<Mutex<Vec<T>>>,
    timeout: Duration,
    predicate: impl Fn(&[T]) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let guard = sink.lock().unwrap();
            if predicate(&guard) {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Try to create a FileWatcher, returning None if OS resources (e.g. inotify
/// instances) are exhausted. Tests should skip rather than fail in that case.
fn try_watcher<F>(
    dir: &std::path::Path,
    target_file: Option<PathBuf>,
    callback: F,
) -> Option<FileWatcher>
where
    F: Fn(FileEvent) + Send + 'static,
{
    match FileWatcher::new(dir, target_file, callback) {
        Ok(w) => Some(w),
        Err(e)
            if e.contains("Too many open files")
                || e.contains("OS file watch limit reached")
                || e.contains("watch limit reached")
                || e.contains("No space left on device") =>
        {
            eprintln!("SKIP: inotify resources exhausted: {e}");
            None
        }
        Err(e) => panic!("unexpected watcher error: {e}"),
    }
}

#[test]
fn wait_for_returns_true_promptly_when_condition_already_satisfied() {
    let sink: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![42]));
    let start = Instant::now();

    let found = wait_for(&sink, Duration::from_secs(10), |v: &[u32]| v.contains(&42));

    assert!(found, "predicate should be satisfied on the first check");
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "should return promptly when already satisfied, took {:?}",
        start.elapsed()
    );
}

#[test]
fn wait_for_detects_value_set_by_another_thread() {
    let sink: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![]));
    let producer_sink = sink.clone();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        producer_sink.lock().unwrap().push(7);
    });

    let found = wait_for(&sink, Duration::from_secs(5), |v: &[u32]| v.contains(&7));

    assert!(
        found,
        "should observe the value pushed by the producer thread"
    );
}

#[test]
fn wait_for_returns_false_after_timeout_when_never_satisfied() {
    let sink: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![]));
    let start = Instant::now();

    let found = wait_for(&sink, Duration::from_millis(150), |v: &[u32]| {
        v.contains(&99)
    });

    assert!(!found, "predicate is never satisfied, should time out");
    assert!(
        start.elapsed() >= Duration::from_millis(150),
        "should wait out the full timeout, took {:?}",
        start.elapsed()
    );
}

#[test]
fn watcher_detects_ri_file_modification() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("test.ri");
    std::fs::write(&ri_file, "structure Bracket {}").unwrap();

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();

    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            changed_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // Give the watcher time to register
    std::thread::sleep(Duration::from_millis(200));

    // Modify the .ri file
    std::fs::write(&ri_file, "structure Bracket { param width = 80mm }").unwrap();

    // Wait for the event to propagate (with debounce)
    wait_for(&changed_paths, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("test.ri"))
    });

    let paths = changed_paths.lock().unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("test.ri")),
        "should have detected test.ri change, got: {:?}",
        *paths
    );
}

#[test]
fn watcher_ignores_non_ri_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let txt_file = dir.path().join("notes.txt");
    std::fs::write(&txt_file, "initial content").unwrap();

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();

    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            changed_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // Give the watcher time to register
    std::thread::sleep(Duration::from_millis(200));

    // Modify a .txt file (should be ignored)
    std::fs::write(&txt_file, "updated content").unwrap();

    // Wait long enough that we'd see the event if it weren't filtered
    std::thread::sleep(Duration::from_millis(500));

    let paths = changed_paths.lock().unwrap();
    assert!(
        paths.is_empty(),
        "should NOT have detected .txt file change, but got: {:?}",
        *paths
    );
}

#[test]
fn watcher_with_target_file_only_fires_for_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("project.ri");
    let other_file = dir.path().join("other.ri");
    std::fs::write(&project_file, "structure Project {}").unwrap();
    std::fs::write(&other_file, "structure Other {}").unwrap();

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();

    let Some(_watcher) =
        try_watcher(dir.path(), Some(PathBuf::from("project.ri")), move |event| {
            if let FileEvent::Changed(path) = event {
                changed_clone.lock().unwrap().push(path);
            }
        })
    else {
        return;
    };

    // Give the watcher time to register (increased for loaded CI systems)
    std::thread::sleep(Duration::from_millis(500));

    // Modify the other .ri file (should be ignored due to target_file filter)
    std::fs::write(&other_file, "structure Other { param x = 10mm }").unwrap();
    std::thread::sleep(Duration::from_millis(500));

    // Modify the target file (should trigger)
    std::fs::write(&project_file, "structure Project { param y = 20mm }").unwrap();
    // Wait for the event to propagate (with debounce)
    wait_for(&changed_paths, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("project.ri"))
    });

    let paths = changed_paths.lock().unwrap();
    // Should have fired for project.ri only
    assert!(
        paths.iter().any(|p| p.ends_with("project.ri")),
        "should have detected project.ri change, got: {:?}",
        *paths
    );
    assert!(
        !paths.iter().any(|p| p.ends_with("other.ri")),
        "should NOT have detected other.ri change, got: {:?}",
        *paths
    );
}

/// Watcher emits a `FileEvent::Removed` event when a `.ri` file is deleted
/// from the watched directory (no target_file filter on Remove events).
#[test]
fn watcher_detects_ri_file_removal() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("scratch.ri");
    std::fs::write(&ri_file, "structure Scratch {}").unwrap();

    let removed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let removed_clone = removed_paths.clone();

    // Watch with no target_file so all .ri events reach the callback.
    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Removed(path) = event {
            removed_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // Give the watcher time to register
    std::thread::sleep(Duration::from_millis(200));

    // Delete the .ri file
    std::fs::remove_file(&ri_file).unwrap();

    // Wait for the Remove event to propagate (with debounce)
    wait_for(&removed_paths, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("scratch.ri"))
    });

    let paths = removed_paths.lock().unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("scratch.ri")),
        "should have received FileEvent::Removed for scratch.ri, got: {:?}",
        *paths
    );
}

/// Even when `target_file` is set (Changed-only filter), Remove events for
/// OTHER .ri files in the watched directory are still emitted.
#[test]
fn watcher_emits_remove_event_even_when_target_file_filter_excludes_other_files() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("target.ri");
    let scratch_file = dir.path().join("scratch.ri");
    std::fs::write(&target_file, "structure Target {}").unwrap();
    std::fs::write(&scratch_file, "structure Scratch {}").unwrap();

    let events: Arc<Mutex<Vec<FileEvent>>> = Arc::new(Mutex::new(vec![]));
    let events_clone = events.clone();

    // Watch with target_file="target.ri" — Changed for non-target should be filtered,
    // but Removed should still fire for any .ri file.
    let Some(_watcher) = try_watcher(
        dir.path(),
        Some(PathBuf::from("target.ri")),
        move |event| {
            events_clone.lock().unwrap().push(event);
        },
    ) else {
        return;
    };

    // Give the watcher time to register
    std::thread::sleep(Duration::from_millis(200));

    // Delete the scratch file (not the target) — should produce Removed event
    std::fs::remove_file(&scratch_file).unwrap();

    // Wait for event propagation
    wait_for(&events, Duration::from_secs(10), |evts| {
        evts.iter()
            .any(|e| matches!(e, FileEvent::Removed(p) if p.ends_with("scratch.ri")))
    });

    let evts = events.lock().unwrap();
    let has_removed = evts.iter().any(|e| {
        if let FileEvent::Removed(p) = e {
            p.ends_with("scratch.ri")
        } else {
            false
        }
    });
    assert!(
        has_removed,
        "FileEvent::Removed for scratch.ri should fire even with target_file filter, got: {:?}",
        evts.iter()
            .map(|e| format!("{:?}", e))
            .collect::<Vec<_>>()
    );
}
