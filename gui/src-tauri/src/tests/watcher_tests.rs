use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::watcher::{ChangeKind, Debouncer, FileEvent, FileWatcher};

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

// --- Debouncer unit tests (deterministic, clock-injected, no filesystem/threads) ---
//
// These pin the trailing-edge + per-path coalescing contract that
// `Debouncer` implements, using synthetic `Instant`s so the assertions are
// exact and never depend on wall-clock scheduling.

#[test]
fn debouncer_lone_record_becomes_ready_only_after_the_window_elapses() {
    let t0 = Instant::now();
    let path = PathBuf::from("a.ri");
    let mut deb = Debouncer::new(Duration::from_millis(100));

    deb.record(path.clone(), ChangeKind::Changed, t0);

    // Not yet ready: only 50ms of quiet has elapsed.
    assert_eq!(deb.drain_ready(t0 + Duration::from_millis(50)), vec![]);

    // Ready: 150ms of quiet has elapsed (>= the 100ms window).
    assert_eq!(
        deb.drain_ready(t0 + Duration::from_millis(150)),
        vec![(path.clone(), ChangeKind::Changed)]
    );

    // Draining removes the entry -- a second drain finds nothing pending.
    assert_eq!(deb.drain_ready(t0 + Duration::from_millis(500)), vec![]);
}

#[test]
fn debouncer_second_record_resets_the_quiet_window() {
    let t0 = Instant::now();
    let path = PathBuf::from("a.ri");
    let mut deb = Debouncer::new(Duration::from_millis(100));

    deb.record(path.clone(), ChangeKind::Changed, t0);
    deb.record(
        path.clone(),
        ChangeKind::Changed,
        t0 + Duration::from_millis(50),
    );

    // Only 70ms since the LATEST event (120 - 50) -- still not ready, even
    // though 120ms have elapsed since the FIRST event.
    assert_eq!(deb.drain_ready(t0 + Duration::from_millis(120)), vec![]);

    // 110ms since the latest event (160 - 50) -- now ready.
    assert_eq!(
        deb.drain_ready(t0 + Duration::from_millis(160)),
        vec![(path.clone(), ChangeKind::Changed)]
    );
}

#[test]
fn debouncer_coalesces_rapid_records_to_one_emission_with_the_latest_kind() {
    let t0 = Instant::now();
    let path = PathBuf::from("a.ri");
    let mut deb = Debouncer::new(Duration::from_millis(100));

    deb.record(path.clone(), ChangeKind::Changed, t0);
    deb.record(
        path.clone(),
        ChangeKind::Changed,
        t0 + Duration::from_millis(10),
    );
    deb.record(
        path.clone(),
        ChangeKind::Changed,
        t0 + Duration::from_millis(20),
    );
    deb.record(
        path.clone(),
        ChangeKind::Removed,
        t0 + Duration::from_millis(30),
    );

    let ready = deb.drain_ready(t0 + Duration::from_millis(200));
    assert_eq!(
        ready,
        vec![(path, ChangeKind::Removed)],
        "rapid same-path records should coalesce into a single emission carrying the latest kind"
    );
}

#[test]
fn debouncer_next_wait_reports_remaining_time_or_none_when_empty() {
    let t0 = Instant::now();
    let path = PathBuf::from("a.ri");
    let mut deb = Debouncer::new(Duration::from_millis(100));

    assert_eq!(
        deb.next_wait(t0),
        None,
        "empty debouncer has nothing to wait for"
    );

    deb.record(path.clone(), ChangeKind::Changed, t0);
    assert_eq!(deb.next_wait(t0), Some(Duration::from_millis(100)));
    assert_eq!(
        deb.next_wait(t0 + Duration::from_millis(40)),
        Some(Duration::from_millis(60))
    );

    // Draining clears pending state, so next_wait goes back to None.
    deb.drain_ready(t0 + Duration::from_millis(150));
    assert_eq!(deb.next_wait(t0 + Duration::from_millis(150)), None);
}

#[test]
fn debouncer_paths_are_coalesced_and_drained_independently() {
    let t0 = Instant::now();
    let a = PathBuf::from("a.ri");
    let b = PathBuf::from("b.ri");
    let mut deb = Debouncer::new(Duration::from_millis(100));

    // `a`'s quiet window starts at t0. `b`'s starts later and is reset
    // again at t0+60ms, so it stays pending well after `a` is due alone.
    deb.record(a.clone(), ChangeKind::Changed, t0);
    deb.record(
        b.clone(),
        ChangeKind::Changed,
        t0 + Duration::from_millis(30),
    );
    deb.record(
        b.clone(),
        ChangeKind::Removed,
        t0 + Duration::from_millis(60),
    );

    // At t0+110ms, `a`'s window (started at t0) has elapsed (110 >= 100),
    // but `b`'s window (last reset at t0+60ms) has only seen 50ms of quiet
    // -- so only `a` drains, and `b`'s pending entry is untouched.
    assert_eq!(
        deb.drain_ready(t0 + Duration::from_millis(110)),
        vec![(a.clone(), ChangeKind::Changed)],
        "only the path whose OWN window has elapsed should drain; a \
         different pending path's window is tracked independently"
    );
    assert_eq!(
        deb.next_wait(t0 + Duration::from_millis(110)),
        Some(Duration::from_millis(50)),
        "b is due at t0+160ms (last_seen 60ms + 100ms window); 50ms remain at t0+110ms"
    );

    // At t0+160ms, `b`'s window has elapsed too and it drains independently
    // of `a` (already drained 50ms earlier), carrying its LATEST kind
    // (Removed) from the second record at t0+60ms.
    assert_eq!(
        deb.drain_ready(t0 + Duration::from_millis(160)),
        vec![(b.clone(), ChangeKind::Removed)]
    );
    assert_eq!(deb.next_wait(t0 + Duration::from_millis(160)), None);
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
    // Dedicated inotify-free coverage of wait_for's cross-thread poll path:
    // this must keep passing even on hosts where every watcher test below
    // skips (e.g. "OS file watch limit reached"), since that skip would
    // otherwise remove all exercise of a producer thread pushing into the
    // same kind of sink wait_for polls.
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

    // Wait for the event to propagate (with debounce). Bind the result so a
    // genuine regression fails via the assert below with a clear message,
    // rather than the boolean being silently discarded.
    let found = wait_for(&changed_paths, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("test.ri"))
    });

    let paths = changed_paths.lock().unwrap();
    assert!(
        found,
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
    // Wait for the event to propagate (with debounce). Bind the result so a
    // genuine regression fails via the assert below with a clear message,
    // rather than the boolean being silently discarded.
    let found = wait_for(&changed_paths, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("project.ri"))
    });

    // The negative check below is an immediate snapshot, not a poll:
    // asserting an event's absence can only ever false-PASS under a
    // condition-poll (there's no positive condition to wait for), so
    // polling here would just add latency on every green run for no
    // correctness benefit. It is race-free by construction, not by
    // timeout: other.ri was modified 500ms before project.ri (above) — 5x
    // the production debounce window (100ms, watcher.rs:15) — and the
    // watcher's debounce only suppresses duplicate same-path events; it
    // does not delay or reorder emission across distinct paths. So a
    // broken target_file filter's other.ri push would already be sitting
    // in `changed_paths` well before project.ri's push makes `found` true
    // above. That's a wall-clock ordering argument sized to catch "the
    // filter is simply broken" (what this test exists to catch), not a
    // formal guarantee against an adversarially delayed event.
    let paths = changed_paths.lock().unwrap();
    assert!(
        found,
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

    // Wait for the Remove event to propagate (with debounce). Bind the
    // result so a genuine regression fails via the assert below with a
    // clear message, rather than the boolean being silently discarded.
    let found = wait_for(&removed_paths, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("scratch.ri"))
    });

    let paths = removed_paths.lock().unwrap();
    assert!(
        found,
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

    // Wait for event propagation. Bind the result so a genuine regression
    // fails via the assert below with a clear message, rather than the
    // boolean being silently discarded.
    let found = wait_for(&events, Duration::from_secs(10), |evts| {
        evts.iter()
            .any(|e| matches!(e, FileEvent::Removed(p) if p.ends_with("scratch.ri")))
    });

    let evts = events.lock().unwrap();
    assert!(
        found,
        "FileEvent::Removed for scratch.ri should fire even with target_file filter, got: {:?}",
        evts.iter()
            .map(|e| format!("{:?}", e))
            .collect::<Vec<_>>()
    );
}

/// Reproduces the non-atomic-write papercut: a writer that truncates a file
/// and then appends to it in two separate syscalls (e.g.
/// `printf 'module bottom_deck\n\n' > f && cat other.ri >> f`) must not
/// leave the watcher stuck on the partially-written (truncated) content.
/// The callback re-reads the file from disk on every fire, so once
/// debouncing correctly coalesces the truncate+append into a single
/// trailing-edge emission, that re-read observes the FINAL on-disk content
/// rather than the transient partial buffer.
///
/// This is a best-effort, real-filesystem SMOKE TEST, not the authoritative
/// coalescing guarantee. It depends on the OS delivering both writes' notify
/// events within the same real-time debounce window; a sufficiently
/// slow/loaded host could in principle violate that (e.g. if the test
/// thread's `sleep` between the two writes gets descheduled long enough to
/// blow past `DEBOUNCE_DURATION`, the two writes legitimately become two
/// separate debounce cycles rather than one coalesced cycle, and the
/// "never delivers the partial content" assertion below could fail on a
/// correct implementation). The exact, deterministic trailing-edge +
/// per-path coalescing contract is pinned with synthetic `Instant`s --
/// immune to scheduling jitter -- by the `debouncer_*` unit tests above;
/// this test only adds end-to-end confirmation that the production wiring
/// (notify closure + worker thread) actually exercises that contract on a
/// real filesystem.
#[test]
fn watcher_rereads_final_content_after_nonatomic_truncate_then_append() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("bottom_deck.ri");
    std::fs::write(&target, "module bottom_deck\n\nvalue a = 0\n").unwrap();

    let contents: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let contents_clone = contents.clone();

    let Some(_watcher) = try_watcher(
        dir.path(),
        Some(PathBuf::from("bottom_deck.ri")),
        move |event| {
            if let FileEvent::Changed(path) = event
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                contents_clone.lock().unwrap().push(text);
            }
        },
    ) else {
        return;
    };

    // Give the watcher time to register.
    std::thread::sleep(Duration::from_millis(200));

    // Simulate a non-atomic write: truncate first, then append moments
    // later in a SEPARATE syscall -- e.g. `printf '...' > f && cat other >> f`.
    std::fs::write(&target, "module bottom_deck\n\n").unwrap();

    // Sub-window pause: 40ms is comfortably less than the production 100ms
    // debounce window (DEBOUNCE_DURATION, watcher.rs:15), so the append
    // below lands inside the SAME quiet-window cycle as the truncation
    // rather than starting a fresh one -- exercising per-path coalescing,
    // not two independent debounce cycles. It's also ample separation for
    // the two writes to land as two distinct filesystem events rather than
    // merging into one at the OS level.
    std::thread::sleep(Duration::from_millis(40));

    let partial_content = "module bottom_deck\n\n";
    let full_content = "module bottom_deck\n\nvalue a = 1\n";
    std::fs::write(&target, full_content).unwrap();

    // Poll for the sink's TERMINAL entry to equal the full (post-append)
    // content -- i.e. the watcher's emission re-read the file after both
    // writes settled, rather than firing early on the truncation and
    // getting suppressed for the trailing append.
    let found = wait_for(&contents, Duration::from_secs(10), |texts| {
        texts.last().is_some_and(|t| t == full_content)
    });

    let texts = contents.lock().unwrap();
    assert!(
        found,
        "expected the watcher's terminal read to equal the fully-appended \
         content {:?}, got: {:?}",
        full_content, *texts
    );

    // The terminal-read check above would also pass for a watcher that
    // fires on EVERY event (no coalescing) as long as the second read
    // happens to observe the settled file -- it doesn't actually pin
    // coalescing. Assert directly that the transient partial (truncated)
    // content was never delivered: that's only possible if the truncate
    // and append events coalesced into a single trailing-edge emission
    // rather than the truncation getting its own early callback firing.
    assert!(
        !texts.iter().any(|t| t == partial_content),
        "the watcher should coalesce the truncate+append into a single \
         trailing-edge emission and never deliver the transient partial \
         content {:?}, got emissions: {:?}",
        partial_content,
        *texts
    );
}

/// A callback that panics on its first invocation must not permanently kill
/// event delivery: the worker thread catches the unwind and keeps draining
/// the debouncer, so a later filesystem event still reaches the callback.
///
/// The panic is gated on the FIRST write's distinct content (read back
/// inside the callback) rather than a call counter, and the survival
/// assertion checks for the SECOND write's distinct content specifically.
/// This means the test cannot be satisfied by a double-fire of the first
/// write alone: if the worker (incorrectly) delivered the first write's
/// event twice, both deliveries would observe the first-write content and
/// panic, and `received` would still end up empty. Only a genuine delivery
/// of the second write proves the worker kept draining after the panic.
#[test]
fn watcher_survives_a_panicking_callback_and_keeps_delivering_later_events() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("flaky.ri");
    std::fs::write(&ri_file, "structure Flaky {}").unwrap();

    let first_write_content = "structure Flaky { param x = 1mm }";
    let second_write_content = "structure Flaky { param x = 2mm }";

    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = received.clone();

    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            // Simulate a callback bug specifically on the first write's
            // content -- see the doc comment above for why gating on
            // content (not a call counter) matters here.
            if text == first_write_content {
                panic!("simulated callback panic on first-write content");
            }
            if text == second_write_content {
                received_clone.lock().unwrap().push(text);
            }
        }
    }) else {
        return;
    };

    // Give the watcher time to register.
    std::thread::sleep(Duration::from_millis(200));

    // First modification: reaches the callback, which panics. If the
    // worker thread didn't catch that unwind, it would terminate here and
    // no further event would ever be delivered.
    std::fs::write(&ri_file, first_write_content).unwrap();

    // Let the debounce window elapse so the panicking call is fully
    // processed (and, if uncaught, the worker fully dead) before the
    // second write.
    std::thread::sleep(Duration::from_millis(300));

    // Second modification, with content DISTINCT from the first: only
    // observable if the worker survived the first callback's panic and is
    // still draining the debouncer.
    std::fs::write(&ri_file, second_write_content).unwrap();

    let found = wait_for(&received, Duration::from_secs(10), |texts| {
        texts.iter().any(|t| t == second_write_content)
    });

    let texts = received.lock().unwrap();
    assert!(
        found,
        "watcher should keep delivering events after a callback panic, got: {:?}",
        *texts
    );
}

/// `Drop` must cleanly shut down and join the worker thread even while a
/// change is still pending in the `Debouncer` (i.e. its quiet window hasn't
/// elapsed yet) -- this is the subtlest part of the trailing-edge rewrite:
/// the shutdown flag is set and the condvar notified under the SAME mutex
/// the worker checks it under, specifically to avoid a lost wakeup where
/// the worker re-checks the flag, finds it unset, and goes back to sleep
/// with no one left to wake it. If that were broken, dropping a
/// `FileWatcher` shortly after a write would hang.
#[test]
fn watcher_drop_joins_worker_promptly_even_with_a_pending_event() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("closing.ri");
    std::fs::write(&ri_file, "structure Closing {}").unwrap();

    let Some(watcher) = try_watcher(dir.path(), None, |_event| {}) else {
        return;
    };

    // Give the watcher time to register.
    std::thread::sleep(Duration::from_millis(200));

    // Trigger an event and drop almost immediately -- well within the
    // 100ms debounce window, so a not-yet-drained entry is very likely
    // still sitting in the Debouncer when Drop runs below.
    std::fs::write(&ri_file, "structure Closing { param x = 1mm }").unwrap();
    std::thread::sleep(Duration::from_millis(10));

    let start = Instant::now();
    drop(watcher);
    let elapsed = start.elapsed();

    // Generous bound: a correct Drop wakes the worker via the condvar and
    // joins almost instantly. This is only guarding against a hang (a
    // lost-wakeup regression would block here indefinitely), not timing
    // the happy path precisely.
    assert!(
        elapsed < Duration::from_secs(2),
        "Drop should join the worker thread promptly even with a pending \
         event, took {:?}",
        elapsed
    );
}

/// Pins the "pending-on-shutdown is dropped, not flushed" contract
/// documented on `FileWatcher::new`: a change still sitting in the
/// `Debouncer` when `Drop` runs (its quiet window hasn't elapsed yet) is
/// silently discarded rather than delivered to the callback. This is a
/// deliberate design choice, not an oversight -- this test pins it
/// explicitly so a future change that decided to flush pending events on
/// shutdown instead would fail here rather than silently altering
/// observable behavior with nothing to catch it.
///
/// A fixed sleep between the write and the drop can't distinguish "the
/// event was recorded and then correctly discarded" from "the event never
/// reached the notify closure in time" (e.g. on a slow/loaded host) -- both
/// look identical from here (`received` ends up empty either way), and the
/// latter would let this test pass vacuously without exercising the
/// discard-on-drop contract it claims to pin. So this polls
/// `FileWatcher::pending_paths` (a test-only hook into the debouncer's
/// internal state) to positively confirm the event was recorded as pending
/// *before* dropping, and fails loudly if that confirmation never arrives.
#[test]
fn watcher_drop_discards_a_pending_event_rather_than_delivering_it() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("abandoned.ri");
    std::fs::write(&ri_file, "structure Abandoned {}").unwrap();

    let received: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = received.clone();

    let Some(watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            received_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // Give the watcher time to register.
    std::thread::sleep(Duration::from_millis(200));

    // Trigger an event.
    std::fs::write(&ri_file, "structure Abandoned { param x = 1mm }").unwrap();

    // Confirm the notify closure actually recorded this event into the
    // debouncer before we drop. Once recorded, the entry is guaranteed to
    // stay pending for the full 100ms debounce window before the worker
    // could drain it, so polling at a much finer grain than that window
    // reliably observes it while it's still pending -- this is what makes
    // the drop below race against a genuinely-pending entry rather than an
    // empty debouncer.
    let deadline = Instant::now() + Duration::from_secs(5);
    let recorded = loop {
        if watcher
            .pending_paths()
            .iter()
            .any(|p| p.ends_with("abandoned.ri"))
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(
        recorded,
        "the event was never recorded as pending in the debouncer within \
         the deadline -- can't exercise the discard-on-drop contract \
         without it (this would otherwise let the test pass vacuously, \
         see doc comment above)"
    );

    // `Drop` joins the worker thread before returning, so once this call
    // is back, the worker has already exited -- there's no race to poll
    // for below; a direct snapshot is enough.
    drop(watcher);

    let paths = received.lock().unwrap();
    assert!(
        paths.is_empty(),
        "a change still pending in the Debouncer when Drop runs should be \
         discarded, not delivered -- got: {:?}",
        *paths
    );
}

/// Constructing and immediately dropping a `FileWatcher` in a loop must
/// always return -- a smoke test that `Drop` never hangs or leaks its
/// worker thread across repeated rapid create/destroy cycles, as happens
/// in the GUI when the user switches files and `create_watcher` runs again
/// (main.rs re-creates the watcher on `open_file`).
///
/// `try_watcher` returning `None` mid-loop (OS inotify resources exhausted)
/// is a legitimate environment skip, same as every other test in this
/// file. But if it returns `None` on the very FIRST iteration, bailing out
/// silently would make this test report green while never having run a
/// single create/destroy cycle -- exercising nothing and masking exactly
/// the kind of Drop-hang regression it exists to catch. So completion is
/// tracked explicitly and asserted, rather than trusting a silent early
/// return to mean success.
#[test]
fn watcher_construct_and_drop_in_a_loop_never_hangs() {
    let dir = tempfile::tempdir().unwrap();

    let mut constructed = 0;
    for _ in 0..20 {
        let Some(watcher) = try_watcher(dir.path(), None, |_event| {}) else {
            break;
        };
        drop(watcher);
        constructed += 1;
    }

    assert!(
        constructed > 0,
        "inotify resources were exhausted before a single create/destroy \
         cycle could run -- this test would otherwise pass vacuously \
         without exercising Drop at all (see the SKIP message above for \
         the underlying try_watcher error)"
    );
}
