use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::watcher::{ChangeKind, Debouncer, FileEvent, FileWatcher};

/// Poll `condition` every 20ms until it holds or `timeout` elapses.
/// Returns immediately, before any sleep, if `condition` already holds.
///
/// The 20ms poll interval is clamped to whatever time remains before
/// `deadline`, so the final sleep of a call never overshoots `timeout` by
/// a full interval -- without this, a caller chaining many short windows
/// (e.g. `wait_until_with_retry`'s per-attempt windows) would accumulate
/// up to ~20ms of drift per window.
fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20).min(remaining));
    }
}

/// Poll `sink` until `predicate` holds or `timeout` elapses.
///
/// The lock is dropped before each sleep so a concurrent producer (e.g. the
/// watcher's callback thread) can still push into `sink` while we wait: the
/// `MutexGuard` created inside the closure below is a temporary, scoped to
/// the single `predicate(&sink.lock().unwrap())` call, so it's released
/// before `wait_until` ever reaches its deadline check or sleep.
fn wait_for<T>(
    sink: &Arc<Mutex<Vec<T>>>,
    timeout: Duration,
    predicate: impl Fn(&[T]) -> bool,
) -> bool {
    wait_until(timeout, || predicate(&sink.lock().unwrap()))
}

/// Like [`wait_until`], but also invokes `attempt` before each poll window,
/// up to `retry_every` apart, until `condition` holds or the OVERALL
/// `timeout` budget elapses (`retry_every` bounds a single attempt's poll
/// window, not the whole call).
///
/// This is necessary -- rather than just polling harder -- whenever
/// `condition` can only become true as a side effect of re-issuing the
/// stimulus `attempt` performs. For example, a filesystem write issued
/// before an inotify watch is live produces no event at all, not a late
/// one: no amount of polling can recover it, only re-issuing the write
/// after the watch goes live can. Every attempt gets its own `retry_every`
/// window to succeed before the next one is issued.
///
/// When `attempt` is a filesystem write feeding a debounced watcher (as
/// above), `retry_every` MUST be strictly greater than the watcher's
/// debounce window (`DEBOUNCE_DURATION` in `watcher.rs`) -- `Debouncer`'s
/// `record` is insert-or-update and resets a path's quiet window on every
/// call, so a retry cadence faster than the debounce window would
/// perpetually reset the pending entry and the worker would never drain
/// it. That failure mode spins for the full `timeout` and then fails with
/// a message blaming the watcher for never delivering anything, rather
/// than the retry cadence being too fast -- so pick `retry_every` with
/// this in mind rather than by feel.
fn wait_until_with_retry(
    mut attempt: impl FnMut(),
    retry_every: Duration,
    timeout: Duration,
    condition: impl Fn() -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        attempt();
        let remaining = deadline.saturating_duration_since(Instant::now());
        if wait_until(remaining.min(retry_every), &condition) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
    }
}

/// Try to create a FileWatcher, returning None if OS resources (e.g. inotify
/// instances) are exhausted. Tests should skip rather than fail in that case.
///
/// Most callers below make a single attempt and silently `return` on `None`
/// (the `let Some(_watcher) = try_watcher(...) else { return; };` idiom) --
/// in an environment where that exhaustion is genuine and sustained, such a
/// test passes vacuously without exercising anything.
/// `watcher_construct_and_drop_in_a_loop_never_hangs` guards against total,
/// sustained inotify unavailability as a suite-wide canary (it fails
/// loudly, rather than skipping, if EVERY one of 20 quick attempts fails).
/// `watcher_rereads_final_content_after_nonatomic_truncate_then_append`
/// additionally retries a bounded number of times against transient
/// contention (concurrent tests racing for a limited number of inotify
/// instances/watches), since it's the load-bearing regression test for this
/// file's bug (papercut #11). The remaining single-attempt tests accept the
/// residual, suggestion-level vacuous-skip risk rather than each carrying
/// their own retry/canary logic.
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

/// Shared implementation behind [`wait_for_watch_registration`] and
/// [`wait_for_watch_registration_via_removal`]: repeatedly (re)writes a
/// sibling `probe.ri` file inside `dir` -- removing it again immediately
/// when `remove_after_write` is set -- until `probe_seen` reports true,
/// positively confirming the directory watch is live before the caller
/// issues a write it can't afford to lose. See `wait_until_with_retry`'s
/// doc comment for why re-issuing -- not just polling harder -- is
/// required: a write issued before the watch is live produces no inotify
/// event at all, not a late one.
///
/// `retry_every` is fixed at 150ms, which exceeds the production debounce
/// window (`DEBOUNCE_DURATION` in `watcher.rs`, 100ms) so each retry gets
/// its own trailing-edge quiet window instead of perpetually resetting one
/// pending entry -- see `wait_until_with_retry`'s doc comment. `timeout` is
/// a parameter, rather than a fixed budget like the two wrappers below use,
/// so a caller that needs to positively confirm the ABSENCE of registration
/// within a short window can call this directly.
fn wait_for_watch_registration_inner(
    dir: &std::path::Path,
    probe_seen: &Arc<AtomicBool>,
    remove_after_write: bool,
    timeout: Duration,
) -> bool {
    let probe_file = dir.join("probe.ri");
    let mut probe_attempt = 0u32;
    wait_until_with_retry(
        || {
            probe_attempt += 1;
            std::fs::write(
                &probe_file,
                format!("structure Probe {{ param n = {probe_attempt} }}"),
            )
            .unwrap();
            if remove_after_write {
                match std::fs::remove_file(&probe_file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => panic!("failed to remove probe file: {e}"),
                }
            }
        },
        Duration::from_millis(150),
        timeout,
        || probe_seen.load(Ordering::SeqCst),
    )
}

/// Registration barrier for a directory watch: repeatedly (re)writes a
/// sibling `probe.ri` file inside `dir` until `probe_seen` reports true,
/// positively confirming the directory watch is live before the caller
/// issues a write it can't afford to lose.
///
/// The caller must construct its `FileWatcher` (with a callback that sets
/// `probe_seen`, e.g. on any `Changed` event whose path ends with
/// `"probe.ri"`, returning early so probe content can never reach the
/// caller's own content-gated branches) BEFORE calling this. `probe.ri`
/// itself is created lazily by the first attempt below rather than
/// pre-created by the caller: the notify wiring in `watcher.rs` treats a
/// path's initial `Create` the same as a later `Modify` (both collapse to
/// `FileEvent::Changed`), so there's nothing to gain from pre-creating it.
///
/// Use [`wait_for_watch_registration_via_removal`] instead whenever the
/// watcher under test was constructed with `target_file: Some(_)` -- see
/// that function's doc comment for why.
fn wait_for_watch_registration(dir: &std::path::Path, probe_seen: &Arc<AtomicBool>) -> bool {
    wait_for_watch_registration_inner(dir, probe_seen, false, Duration::from_secs(10))
}

/// Registration barrier for a directory watch guarded by a `target_file`
/// filter: repeatedly (re)writes then immediately removes a sibling
/// `probe.ri` file inside `dir` until `probe_seen` reports true, positively
/// confirming the directory watch is live before the caller issues a write
/// it can't afford to lose.
///
/// Use this instead of its Changed-probe sibling
/// [`wait_for_watch_registration`] whenever the watcher under test was
/// constructed with `target_file: Some(_)`; keep using the sibling for
/// `target_file: None` watchers. The `target_file` guard in
/// `FileWatcher::new`'s notify closure applies to **`Changed`** events
/// only, so a watcher filtering out `probe.ri` would never deliver the
/// sibling's `Changed("probe.ri")` probe event -- the sibling would spin
/// its full budget and return `false`, even though the watch is genuinely
/// live. A delivered `Removed` event bypasses that filter (by the same
/// design -- see `watcher.rs`'s module doc), so it can confirm liveness
/// here.
///
/// A delivered `Removed` still proves the watch is live for `Changed`
/// events too: both event kinds ride the same underlying inotify directory
/// watch, and the `target_file` filter is applied downstream, inside our
/// own notify closure -- not by the kernel. The only thing a `Removed`
/// delivery leaves unproven is our own filter logic, which is exactly what
/// the tests using this barrier are testing, and must not be assumed away
/// by the barrier itself.
///
/// The caller must construct its `FileWatcher` (with a callback that sets
/// `probe_seen` on a `FileEvent::Removed(path)` whose path ends with
/// `"probe.ri"`, returning early so probe content can never reach the
/// caller's own content-gated branches) BEFORE calling this.
///
/// Assumes an inotify-style backend that reports a create and a later
/// delete as two separate events -- true of `notify`'s Linux backend,
/// which is the only one this workspace builds against (native deps and CI
/// both pin `x86_64-unknown-linux-gnu`). A coalescing backend (e.g.
/// `notify`'s FSEvents backend on macOS, which can merge a create+delete
/// inside its latency window into a single Create/Modify event) could drop
/// the synthesized `Removed` entirely under a `target_file` filter, and
/// every test relying on this barrier would spin its full budget and fail.
fn wait_for_watch_registration_via_removal(
    dir: &std::path::Path,
    probe_seen: &Arc<AtomicBool>,
) -> bool {
    wait_for_watch_registration_inner(dir, probe_seen, true, Duration::from_secs(10))
}

/// Discriminating test for the constraint that motivates
/// `wait_for_watch_registration_via_removal` (defined above its
/// Changed-probe sibling): a watcher constructed with `Some(target_file)`
/// filters `Changed` events by filename (the guard in `FileWatcher::new`'s
/// notify closure), so the Changed-probe `wait_for_watch_registration`
/// could never confirm registration here -- it would spin its full budget
/// and return `false`. `Removed` events bypass that filter by design, so a
/// removal probe can. Both halves are asserted below: that the removal
/// probe succeeds, and that the Changed probe (run with a short budget
/// against the very same watcher) fails.
#[test]
fn wait_for_watch_registration_via_removal_confirms_a_watch_behind_a_target_file_filter() {
    let dir = tempfile::tempdir().unwrap();

    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();
    let changed_probe_seen = Arc::new(AtomicBool::new(false));
    let changed_probe_seen_clone = changed_probe_seen.clone();

    // A target_file filter that EXCLUDES probe.ri: only Changed("target.ri")
    // would ever pass the filter, so this watcher can only be confirmed
    // live via a Removed probe. The Changed arm below exists purely to make
    // the negative assertion further down discriminating: it WOULD flip
    // `changed_probe_seen` if a Changed("probe.ri") event ever reached this
    // closure, so that flag staying false is evidence the event never
    // arrived (filtered upstream in watcher.rs) rather than evidence this
    // closure merely doesn't look for it.
    let Some(_watcher) =
        try_watcher(
            dir.path(),
            Some(PathBuf::from("target.ri")),
            move |event| match event {
                FileEvent::Removed(path) if path.ends_with("probe.ri") => {
                    probe_seen_clone.store(true, Ordering::SeqCst);
                }
                FileEvent::Changed(path) if path.ends_with("probe.ri") => {
                    changed_probe_seen_clone.store(true, Ordering::SeqCst);
                }
                _ => {}
            },
        )
    else {
        return;
    };

    let registered = wait_for_watch_registration_via_removal(dir.path(), &probe_seen);
    assert!(
        registered,
        "wait_for_watch_registration_via_removal should confirm the watch \
         is live via a Removed probe even though target_file=\"target.ri\" \
         would filter out the probe's Changed events"
    );
    assert!(
        !dir.path().join("probe.ri").exists(),
        "the removal probe should leave no probe.ri behind on disk"
    );

    // Discriminating half: the Changed-probe barrier, run against this same
    // target_file-filtered watcher with a short budget, must fail to
    // confirm registration. Not because this closure can't detect a
    // Changed("probe.ri") -- the arm above proves it can -- but because
    // watcher.rs's target_file filtering drops that event before it ever
    // reaches any callback. Without this assertion, this test would still
    // pass even if target_file filtering of Changed events were deleted
    // from watcher.rs entirely.
    let changed_probe_registered = wait_for_watch_registration_inner(
        dir.path(),
        &changed_probe_seen,
        false,
        Duration::from_secs(1),
    );
    assert!(
        !changed_probe_registered,
        "the Changed-probe barrier should NOT have been able to confirm \
         this target_file-filtered watch -- if it did, target_file \
         filtering of Changed events has regressed"
    );
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
fn wait_until_with_retry_reissues_the_attempt_until_the_condition_holds() {
    // `attempt` only flips the shared counter; `condition` reads the SAME
    // counter and is satisfied once it reaches 3. If `wait_until_with_retry`
    // only invoked `attempt` once (like a plain poll), the condition would
    // never hold and this would time out. Reissuing it is exactly the
    // property the de-flake depends on: a stimulus (e.g. a write) lost to a
    // not-yet-live watcher must be re-issued, not just waited on.
    let counter = Rc::new(Cell::new(0u32));
    let attempt_counter = counter.clone();
    let condition_counter = counter.clone();

    let found = wait_until_with_retry(
        move || attempt_counter.set(attempt_counter.get() + 1),
        Duration::from_millis(20),
        Duration::from_secs(2),
        move || condition_counter.get() >= 3,
    );

    assert!(
        found,
        "condition should be satisfied once attempt has been reissued enough times"
    );
    assert!(
        counter.get() >= 3,
        "attempt should have been reissued (not just invoked once) before \
         the condition held, got {} invocations",
        counter.get()
    );
}

#[test]
fn wait_until_with_retry_returns_true_without_waiting_when_already_satisfied() {
    let start = Instant::now();
    let found = wait_until_with_retry(
        || {},
        Duration::from_millis(150),
        Duration::from_secs(10),
        || true,
    );
    assert!(found, "already-satisfied condition should return true");
    assert!(
        start.elapsed() < Duration::from_millis(200),
        "should return promptly when already satisfied, took {:?}",
        start.elapsed()
    );
}

#[test]
fn wait_until_with_retry_returns_false_after_the_timeout_when_never_satisfied() {
    let counter = Rc::new(Cell::new(0u32));
    let attempt_counter = counter.clone();

    let start = Instant::now();
    let found = wait_until_with_retry(
        move || attempt_counter.set(attempt_counter.get() + 1),
        Duration::from_millis(20),
        Duration::from_millis(200),
        || false,
    );

    assert!(!found, "condition is never satisfied, should time out");
    assert!(
        start.elapsed() >= Duration::from_millis(200),
        "should wait out the full timeout, took {:?}",
        start.elapsed()
    );
    assert!(
        counter.get() > 1,
        "attempt should have been reissued more than once while waiting \
         for the condition, got {}",
        counter.get()
    );
}

#[test]
fn wait_until_with_retry_reissues_the_attempt_for_every_window_until_the_deadline_on_a_virtual_clock()
{
    // Deterministic replacement for the flaky assertion above: on a virtual
    // clock the attempt count is a pure function of the loop's arithmetic
    // (200ms budget / 20ms windows = 10), not of host scheduling, so this
    // makes the same ">1" claim without ever being at the mercy of the
    // scheduler. See #5709.
    let t0 = Instant::now();
    let mut clock = VirtualClock::new(t0);
    let counter = Rc::new(Cell::new(0u32));
    let attempt_counter = counter.clone();

    let found = wait_until_with_retry_on(
        &mut clock,
        move || attempt_counter.set(attempt_counter.get() + 1),
        Duration::from_millis(20),
        Duration::from_millis(200),
        || false,
    );

    assert!(!found, "condition is never satisfied, should time out");
    assert!(
        counter.get() > 1,
        "attempt should have been reissued more than once while waiting \
         for the condition, got {}",
        counter.get()
    );
    assert_eq!(
        clock.now() - t0,
        Duration::from_millis(200),
        "the virtual clock should advance by exactly the timeout budget, \
         neither short nor overshot, got {:?}",
        clock.now() - t0
    );
}

#[test]
fn wait_until_with_retry_stops_reissuing_once_the_condition_holds_on_a_virtual_clock() {
    // Deterministic replacement for
    // `wait_until_with_retry_reissues_the_attempt_until_the_condition_holds`:
    // the condition is re-checked at the head of each poll window, so the
    // 3rd attempt short-circuits before any further sleep. See #5709.
    let t0 = Instant::now();
    let mut clock = VirtualClock::new(t0);
    let counter = Rc::new(Cell::new(0u32));
    let attempt_counter = counter.clone();
    let condition_counter = counter.clone();

    let found = wait_until_with_retry_on(
        &mut clock,
        move || attempt_counter.set(attempt_counter.get() + 1),
        Duration::from_millis(20),
        Duration::from_secs(2),
        move || condition_counter.get() >= 3,
    );

    assert!(
        found,
        "condition should be satisfied once attempt has been reissued enough times"
    );
    assert_eq!(
        counter.get(),
        3,
        "attempt should be reissued exactly until the condition first holds, got {} invocations",
        counter.get()
    );
}

#[test]
fn wait_until_with_retry_does_not_sleep_when_the_condition_already_holds_on_a_virtual_clock() {
    // Sharper, deterministic form of "returns without waiting": the clock
    // does not advance at all, which transitively proves `wait_until`
    // checks its condition before ever sleeping. See #5709.
    let t0 = Instant::now();
    let mut clock = VirtualClock::new(t0);
    let counter = Rc::new(Cell::new(0u32));
    let attempt_counter = counter.clone();

    let found = wait_until_with_retry_on(
        &mut clock,
        move || attempt_counter.set(attempt_counter.get() + 1),
        Duration::from_millis(150),
        Duration::from_secs(10),
        || true,
    );

    assert!(found, "already-satisfied condition should return true");
    assert_eq!(
        counter.get(),
        1,
        "already-satisfied condition should still attempt exactly once, got {}",
        counter.get()
    );
    assert_eq!(
        clock.now(),
        t0,
        "already-satisfied condition should not advance the clock at all, i.e. never sleep"
    );
}

#[test]
fn watcher_detects_ri_file_modification() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("test.ri");
    std::fs::write(&ri_file, "structure Bracket {}").unwrap();

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            if path.ends_with("probe.ri") {
                probe_seen_clone.store(true, Ordering::SeqCst);
                return;
            }
            changed_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    let registered = wait_for_watch_registration(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the test.ri write below could have \
         been lost outright and this run could not exercise change \
         detection"
    );

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
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            // MANDATORY, not just hygiene: probe.ri IS a .ri file, and this
            // test asserts `paths.is_empty()` below. Without this early
            // return, the probe's own Changed event would land in
            // `changed_paths` and fail the test outright.
            if path.ends_with("probe.ri") {
                probe_seen_clone.store(true, Ordering::SeqCst);
                return;
            }
            changed_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    let registered = wait_for_watch_registration(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- this test's absence assertion below \
         would otherwise be a false PASS (a sleep that expired before \
         registration proves nothing about filtering)"
    );

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
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    // The existing Changed arm is untouched: probe.ri's Changed events are
    // filtered out by the watcher itself (target_file = "project.ri"), so
    // they can never reach this arm and pollute changed_paths.
    let Some(_watcher) = try_watcher(
        dir.path(),
        Some(PathBuf::from("project.ri")),
        move |event| match event {
            FileEvent::Changed(path) => {
                changed_clone.lock().unwrap().push(path);
            }
            FileEvent::Removed(path) => {
                if path.ends_with("probe.ri") {
                    probe_seen_clone.store(true, Ordering::SeqCst);
                }
            }
        },
    ) else {
        return;
    };

    // Watcher is target_file-filtered, so the Changed-probe barrier's
    // Changed("probe.ri") event would itself be filtered out and the
    // barrier would spin its full budget and return false -- use the
    // removal-probe variant instead, which bypasses the filter by the same
    // design this test exists to pin.
    let registered = wait_for_watch_registration_via_removal(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the writes below could have been lost \
         outright and this run could not exercise the target_file filter"
    );

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
    // the production debounce window (`DEBOUNCE_DURATION`, 100ms, in watcher.rs) — and the
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
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    // Watch with no target_file so all .ri events reach the callback. A
    // single `match` (rather than two sequential `if let`s) is required
    // here since `event` is owned and non-Copy: a second `if let` on the
    // same `event` after the first has already bound a variant's field by
    // value would not compile ("use of moved value").
    let Some(_watcher) = try_watcher(dir.path(), None, move |event| match event {
        FileEvent::Changed(path) => {
            if path.ends_with("probe.ri") {
                probe_seen_clone.store(true, Ordering::SeqCst);
            }
        }
        FileEvent::Removed(path) => {
            // Defensive: a Removed for probe.ri should never happen (the
            // registration barrier only ever writes/rewrites probe.ri,
            // never removes it), but guard anyway so a future change to
            // the probe mechanism can't silently pollute removed_paths.
            if path.ends_with("probe.ri") {
                return;
            }
            removed_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    let registered = wait_for_watch_registration(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the remove below could have been \
         lost outright and this run could not exercise removal detection"
    );

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
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    // Watch with target_file="target.ri" — Changed for non-target should be filtered,
    // but Removed should still fire for any .ri file.
    let Some(_watcher) = try_watcher(
        dir.path(),
        Some(PathBuf::from("target.ri")),
        move |event| {
            // Leading guard: this callback pushes EVERY event into
            // `events`, so probe traffic of EITHER kind must be caught and
            // returned early here, before it can pollute the sink (and
            // this test's failure-dump formatting below).
            let path = match &event {
                FileEvent::Changed(p) | FileEvent::Removed(p) => p,
            };
            if path.ends_with("probe.ri") {
                probe_seen_clone.store(true, Ordering::SeqCst);
                return;
            }
            events_clone.lock().unwrap().push(event);
        },
    ) else {
        return;
    };

    // Watcher is target_file-filtered, so this relies on the very
    // Removed-bypasses-target_file contract that THIS test exists to pin.
    // That gives the barrier and the test's own assertion below a shared
    // point of failure -- see the barrier's assert message for how a
    // regression here is disambiguated from ordinary inotify flakiness.
    let registered = wait_for_watch_registration_via_removal(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the remove below could have been \
         lost outright and this run could not exercise the \
         Removed-bypasses-target_file contract. NOTE: this barrier probes \
         via a Removed event, so a regression in the \
         Removed-bypasses-target_file contract (the very thing this test \
         pins) presents HERE rather than at the FileEvent::Removed \
         assertion below -- check the target_file guard in \
         FileWatcher::new's notify closure before suspecting inotify \
         registration flakiness"
    );

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
    let probe_seen = Arc::new(AtomicBool::new(false));

    // Unlike the single-attempt `try_watcher` tests elsewhere in this file,
    // this one retries construction a bounded number of times against
    // transient OS resource contention (tests run concurrently by default,
    // so a single attempt can lose a narrow race against sibling tests'
    // watchers even when the environment is perfectly capable of running
    // this test) before conceding a skip -- see the note on `try_watcher`
    // above for how this fits with the other tests' residual vacuous-skip
    // risk. Genuine, sustained exhaustion still skips gracefully once the
    // retries are exhausted, exactly like every other test here; this only
    // shrinks the window, it doesn't change the skip-on-exhaustion
    // contract.
    const MAX_ATTEMPTS: u32 = 10;
    let mut attempts = 0;
    let watcher = loop {
        attempts += 1;
        let contents_clone = contents.clone();
        // Cloned fresh per attempt, same as `contents_clone` above: the
        // callback closure is rebuilt on each of the (up to MAX_ATTEMPTS)
        // iterations, so each attempt's watcher needs its own clone of the
        // shared flag.
        let probe_seen_clone = probe_seen.clone();
        let attempt = try_watcher(
            dir.path(),
            Some(PathBuf::from("bottom_deck.ri")),
            move |event| match event {
                // Existing Changed + read-back arm, untouched: probe.ri's
                // Changed events are filtered out by the watcher itself
                // (target_file = "bottom_deck.ri"), so `contents` can never
                // see probe text.
                FileEvent::Changed(path) => {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        contents_clone.lock().unwrap().push(text);
                    }
                }
                FileEvent::Removed(path) => {
                    if path.ends_with("probe.ri") {
                        probe_seen_clone.store(true, Ordering::SeqCst);
                    }
                }
            },
        );
        match attempt {
            Some(w) => break Some(w),
            None if attempts < MAX_ATTEMPTS => std::thread::sleep(Duration::from_millis(25)),
            None => break None,
        }
    };
    let Some(_watcher) = watcher else {
        eprintln!(
            "SKIP: watcher_rereads_final_content_after_nonatomic_truncate_then_append \
             gave up after {attempts} attempts -- see SKIP reason(s) above"
        );
        return;
    };

    // Watcher is target_file-filtered, so this MUST use the removal-probe
    // barrier -- see wait_for_watch_registration_via_removal's doc comment.
    let registered = wait_for_watch_registration_via_removal(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the writes below could have been lost \
         outright and this run could not exercise the non-atomic \
         truncate-then-append coalescing behavior"
    );

    // Simulate a non-atomic write: truncate first, then append moments
    // later in a SEPARATE syscall -- e.g. `printf '...' > f && cat other >> f`.
    std::fs::write(&target, "module bottom_deck\n\n").unwrap();

    // Sub-window pause: 40ms is comfortably less than the production 100ms
    // debounce window (`DEBOUNCE_DURATION` in watcher.rs), so the append
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
    // coalescing. Ideally we'd also confirm the transient partial
    // (truncated) content was never delivered, since that's only possible
    // if the truncate and append events coalesced into a single
    // trailing-edge emission rather than the truncation getting its own
    // early callback firing. But per the doc comment above, a descheduled
    // test thread that blows past DEBOUNCE_DURATION between the two writes
    // would legitimately split them into two debounce cycles and deliver
    // the partial content on a CORRECT implementation -- so this is a
    // best-effort, logged observation rather than a hard gate. The
    // terminal-content assertion above stays the hard gate for this test;
    // the deterministic `debouncer_*` unit tests are the hard gate for the
    // coalescing contract itself.
    if texts.iter().any(|t| t == partial_content) {
        eprintln!(
            "NOTE: watcher_rereads_final_content_after_nonatomic_truncate_then_append \
             observed the transient partial content {:?} (emissions: {:?}). This is \
             logged rather than asserted because a scheduling stall between the two \
             writes can legitimately produce this on a correct implementation -- see \
             the doc comment above. If this fires routinely (not just occasionally), \
             it likely indicates a real coalescing regression.",
            partial_content, *texts
        );
    }
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
///
/// Two `wait_until_with_retry` barriers stand in for what used to be fixed
/// sleeps, because a fixed sleep can only wait for a stimulus -- it can't
/// recover one that was already lost:
///
/// - **Registration barrier**: a fixed "give the watcher time to register"
///   sleep can still expire before the directory watch is actually live
///   under load -- this is exactly what flaked. A write issued before that
///   happens produces no inotify event AT ALL, not a late one, so once
///   lost, `first_write_content` below could never be recovered by polling
///   harder. Instead, a sibling `probe.ri` is repeatedly rewritten with
///   distinct content until the callback positively reports having seen a
///   probe event, proving the directory watch is live before the write
///   that actually matters is ever issued.
/// - **First-event barrier**: replaces the old "let the debounce window
///   elapse" sleep. `first_write_content` is re-written on each attempt
///   (identical content still yields a fresh `IN_MODIFY`) until the shared
///   `panicked` flag confirms the callback actually observed it and hit
///   the panic branch, recovering the write the same way the registration
///   probe's write is recovered.
///
/// The shared `panicked` flag is what the first-event barrier polls, so its
/// assertion is the single gate on "the panic branch was actually reached"
/// -- without it, a run where the first write's `Changed` event is dropped
/// or coalesced away before ever reaching the callback (e.g. the notify
/// layer only delivers the second, latest content on a slow host) would
/// still pass off the second write's delivery alone: green, but without
/// ever exercising panic survival at all.
#[test]
fn watcher_survives_a_panicking_callback_and_keeps_delivering_later_events() {
    let dir = tempfile::tempdir().unwrap();
    let ri_file = dir.path().join("flaky.ri");
    std::fs::write(&ri_file, "structure Flaky {}").unwrap();

    let first_write_content = "structure Flaky { param x = 1mm }";
    let second_write_content = "structure Flaky { param x = 2mm }";

    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = received.clone();
    let panicked = Arc::new(AtomicBool::new(false));
    let panicked_clone = panicked.clone();
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    let Some(_watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            if path.ends_with("probe.ri") {
                probe_seen_clone.store(true, Ordering::SeqCst);
                return;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                // Simulate a callback bug specifically on the first write's
                // content -- see the doc comment above for why gating on
                // content (not a call counter) matters here.
                if text == first_write_content {
                    panicked_clone.store(true, Ordering::SeqCst);
                    panic!("simulated callback panic on first-write content");
                }
                if text == second_write_content {
                    received_clone.lock().unwrap().push(text);
                }
            }
        }
    }) else {
        return;
    };

    // Registration barrier -- see the doc comment above for why this
    // replaces a fixed "give the watcher time to register" sleep.
    let registered = wait_for_watch_registration(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- this run could not exercise panic \
         survival"
    );

    // First-event barrier -- see the doc comment above for why this
    // replaces a fixed "let the debounce window elapse" sleep. If the
    // worker thread didn't catch the panic's unwind, it would terminate
    // here and no further event would ever be delivered. `retry_every`
    // (300ms) exceeds the production debounce window (100ms) for the same
    // reason as the registration barrier's above: a faster cadence would
    // perpetually reset the pending entry instead of letting it drain --
    // see `wait_until_with_retry`'s doc comment.
    let panic_observed = wait_until_with_retry(
        || std::fs::write(&ri_file, first_write_content).unwrap(),
        Duration::from_millis(300),
        Duration::from_secs(10),
        || panicked.load(Ordering::SeqCst),
    );
    assert!(
        panic_observed,
        "the callback never observed first_write_content and so never hit \
         the panic branch -- this run wouldn't have exercised panic \
         survival at all"
    );

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

    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    let Some(watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event
            && path.ends_with("probe.ri")
        {
            probe_seen_clone.store(true, Ordering::SeqCst);
        }
    }) else {
        return;
    };

    let registered = wait_for_watch_registration(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the write below could have been lost \
         outright and this run could not exercise the pending-drop race"
    );

    // Trigger an event and drop almost immediately -- well within the
    // 100ms debounce window, so a not-yet-drained entry is very likely
    // still sitting in the Debouncer when Drop runs below. (The barrier
    // above may itself leave a still-pending probe.ri entry in the
    // Debouncer at this point -- harmless here, since this test WANTS some
    // pending entry when Drop runs and doesn't care which path it's for.)
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
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = probe_seen.clone();

    let Some(watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            // MANDATORY, not just hygiene: this test asserts
            // `paths.is_empty()` after drop, so any probe event reaching
            // `received` would fail it outright.
            if path.ends_with("probe.ri") {
                probe_seen_clone.store(true, Ordering::SeqCst);
                return;
            }
            received_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // This barrier guards the vacuous-pass hole UPSTREAM (did the write
    // below even reach the notify closure at all); the pending_paths()
    // confirmation loop further down already guards it DOWNSTREAM (was the
    // write recorded into the debouncer). Both are needed: a write issued
    // before the watch is live produces no event at all, which the
    // downstream loop alone couldn't distinguish from "recorded and then
    // legitimately drained before we polled".
    let registered = wait_for_watch_registration(dir.path(), &probe_seen);
    assert!(
        registered,
        "the watcher never delivered a probe event, so the directory watch \
         was never confirmed live -- the write below could have been lost \
         outright and this run could not exercise the discard-on-drop \
         contract"
    );

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

    // One more snapshot immediately before dropping, shrinking the window
    // between "confirmed pending" and Drop as far as possible: on a heavily
    // loaded host, the worker could in principle drain and deliver this
    // event in the gap between the loop above and here (the debounce
    // window happening to elapse right at this instant). That would be the
    // confirmation going stale, not the discard-on-drop contract failing --
    // so gate the hard assertion below on the entry still being observed
    // as pending at this last possible moment.
    let still_pending = watcher
        .pending_paths()
        .iter()
        .any(|p| p.ends_with("abandoned.ri"));

    // `Drop` joins the worker thread before returning, so once this call
    // is back, the worker has already exited -- there's no race to poll
    // for below; a direct snapshot is enough.
    drop(watcher);

    let paths = received.lock().unwrap();
    if still_pending {
        assert!(
            paths.is_empty(),
            "a change still pending in the Debouncer when Drop runs should be \
             discarded, not delivered -- got: {:?}",
            *paths
        );
    } else {
        eprintln!(
            "NOTE: the pending entry was no longer observed immediately before \
             Drop (drained by the worker in a narrow race window) -- skipping \
             the discard-on-drop assertion for this run; got: {:?}",
            *paths
        );
    }
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
