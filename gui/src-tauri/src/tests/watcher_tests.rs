use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::watcher::{ChangeKind, Debouncer, FileEvent, FileWatcher};

/// Clock seam for the `wait_*` helpers below, so their retry/poll contract
/// can be pinned deterministically instead of against the host scheduler.
/// Mirrors `Debouncer`'s existing convention in `watcher.rs` (all methods
/// take an explicit `now: Instant` rather than reading the clock
/// themselves) -- see #5709.
trait WaitClock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, d: Duration);
}

/// Real clock: reads [`Instant::now`] and actually blocks the thread.
struct WallClock;
impl WaitClock for WallClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d)
    }
}

/// Virtual clock: `sleep` advances `now` by exactly `d` and never blocks,
/// so a whole retry budget elapses in microseconds and the attempt count
/// is a function of the loop's arithmetic, not of host scheduling.
struct VirtualClock {
    now: Instant,
}
impl VirtualClock {
    fn new(now: Instant) -> Self {
        Self { now }
    }
}
impl WaitClock for VirtualClock {
    fn now(&self) -> Instant {
        self.now
    }
    fn sleep(&mut self, d: Duration) {
        self.now += d;
    }
}

/// Poll `condition` every 20ms until it holds or `timeout` elapses.
/// Returns immediately, before any sleep, if `condition` already holds.
///
/// The 20ms poll interval is clamped to whatever time remains before
/// `deadline`, so the final sleep of a call never overshoots `timeout` by
/// a full interval -- without this, a caller chaining many short windows
/// (e.g. `wait_until_with_retry`'s per-attempt windows) would accumulate
/// up to ~20ms of drift per window.
fn wait_until_on(
    clock: &mut dyn WaitClock,
    timeout: Duration,
    condition: impl Fn() -> bool,
) -> bool {
    let deadline = clock.now() + timeout;
    loop {
        if condition() {
            return true;
        }
        let remaining = deadline.saturating_duration_since(clock.now());
        if remaining.is_zero() {
            return false;
        }
        clock.sleep(Duration::from_millis(20).min(remaining));
    }
}

/// See [`wait_until_on`] -- this wrapper runs against the real wall clock.
fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    wait_until_on(&mut WallClock, timeout, condition)
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

/// Like [`wait_until_on`], but also invokes `attempt` before each poll window,
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
fn wait_until_with_retry_on(
    clock: &mut dyn WaitClock,
    mut attempt: impl FnMut(),
    retry_every: Duration,
    timeout: Duration,
    condition: impl Fn() -> bool,
) -> bool {
    // A zero-length window never advances a `VirtualClock` (`sleep` adds
    // `d` to `now`, and `0` is a no-op), so a caller passing `retry_every
    // == Duration::ZERO` with a false condition would spin forever on the
    // virtual clock instead of failing -- a silent hang rather than a
    // panic. `WallClock` doesn't have this failure mode (real time
    // advances regardless), so nothing in this file hits it today; this
    // guards the shared seam for future callers. This is `assert!`, not
    // `debug_assert!`, because the verify pipeline's release-profile test
    // pass builds with `-C debug-assertions=off`: a `debug_assert!` here
    // would be compiled out in exactly that pass, so the failure mode this
    // guards against would degrade from a loud panic to a silent hang
    // burning the whole release-test budget instead of failing fast. See
    // #5709.
    assert!(
        !retry_every.is_zero(),
        "retry_every must be non-zero: a zero window never advances a VirtualClock"
    );
    let deadline = clock.now() + timeout;
    loop {
        attempt();
        let remaining = deadline.saturating_duration_since(clock.now());
        if wait_until_on(clock, remaining.min(retry_every), &condition) {
            return true;
        }
        if clock.now() >= deadline {
            return false;
        }
    }
}

/// See [`wait_until_with_retry_on`] -- this wrapper runs against the real wall clock.
fn wait_until_with_retry(
    attempt: impl FnMut(),
    retry_every: Duration,
    timeout: Duration,
    condition: impl Fn() -> bool,
) -> bool {
    wait_until_with_retry_on(&mut WallClock, attempt, retry_every, timeout, condition)
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

/// A positive-progress barrier (added by #6462): waits for TWO deliveries
/// of a control path -- one whose callback the filter under test is
/// expected to PASS -- in `sink`, calling `write_control` before each wait.
/// Delivery of a control event is positive evidence that an EARLIER write
/// the filter is expected to DROP has already been through the notify
/// closure, replacing a fixed sleep, which only ever proved that time
/// passed.
///
/// WHY TWO deliveries and not one: the debouncer is keyed by path, so one
/// batch holds at most one entry per path -- two deliveries of the same
/// control path are therefore necessarily two DISTINCT batches.
/// `drain_ready` (watcher.rs:98-124) is a `HashMap::retain`, so intra-batch
/// order is unspecified, and a one-delivery barrier could snapshot between
/// a same-batch straggler and the control (see
/// `wait_for_control_drain_does_not_return_until_a_same_batch_straggler_has_landed`
/// above for the discriminating case). The single worker thread runs a
/// batch to completion before draining the next (watcher.rs:307-332), so
/// observing the second delivery proves every callback of the first
/// delivery's batch has already returned. A filtered write issued BEFORE
/// the first control write can never land in a later batch than it
/// (recorded earlier => earlier debounce deadline), so it is visible by
/// then either way.
///
/// WHY this is not subject to the retry-cadence constraint documented on
/// `wait_until_with_retry` (:110-119): the second write is gated on the
/// FIRST DELIVERY, which can only have happened after that entry was
/// drained out of `pending`, so `Debouncer::record`'s insert-or-update can
/// never perpetually reset a pending entry here. There is no cadence to
/// tune -- which is why this is built on `wait_for` rather than on
/// `wait_until_with_retry`.
///
/// PRECONDITION: the caller must already have confirmed the watch is live
/// (`wait_for_watch_registration` / `wait_for_watch_registration_via_removal`);
/// this helper assumes a write produces an event and cannot recover a
/// write issued before registration.
///
/// `timeout` bounds EACH of the two waits, not the pair. `write_control`
/// should vary the bytes it writes across calls (mirroring
/// `wait_for_watch_registration_inner`'s `probe_attempt` counter, above),
/// so successive control writes differ on disk instead of relying on a
/// write of byte-identical content emitting its own Modify.
fn wait_for_control_drain(
    sink: &Arc<Mutex<Vec<PathBuf>>>,
    control_name: &str,
    mut write_control: impl FnMut(),
    timeout: Duration,
) -> bool {
    let count = |paths: &[PathBuf]| paths.iter().filter(|p| p.ends_with(control_name)).count();
    write_control();
    if !wait_for(sink, timeout, |paths| count(paths) >= 1) {
        return false;
    }
    write_control();
    wait_for(sink, timeout, |paths| count(paths) >= 2)
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

/// PREMISE PIN for the far-future-stamp idiom that
/// `watcher_drop_discards_a_pending_event_rather_than_delivering_it` below
/// rests on: it injects via `FileWatcher::record_pending_for_test` at the
/// stamp from `far_future_stamp()`, then asserts the entry is STILL pending.
///
/// The identity that makes a future-stamped entry structurally un-drainable
/// -- how `Instant::duration_since` saturates, and what that does to
/// `drain_ready` and `next_wait` -- is stated ONCE, at the seam it belongs
/// to: `FileWatcher::record_pending_for_test` in `watcher.rs`. This test is
/// the EXECUTABLE half of that statement (#6438 review: the argument had been
/// written out in four places, so a change to `drain_ready` would have needed
/// four prose edits while only this test went red).
///
/// So a change that breaks the identity -- `checked_duration_since`,
/// reordered comparison operands, signed deltas -- turns THIS test red at the
/// identity, instead of silently converting the discard test back into the
/// wall-clock flake it was.
#[test]
fn debouncer_a_record_stamped_after_now_is_never_ready_and_reports_a_full_window() {
    let t0 = Instant::now();
    let path = PathBuf::from("frozen.ri");
    let mut deb = Debouncer::new(Duration::from_millis(100));

    // Stamped an hour into the future relative to every `now` queried below.
    deb.record(
        path.clone(),
        ChangeKind::Changed,
        t0 + Duration::from_secs(3600),
    );

    // Never ready -- not at the stamp's own reference point, not once a full
    // window has elapsed, not once 600x the window has elapsed.
    assert_eq!(
        deb.drain_ready(t0),
        vec![],
        "an entry stamped in the future is not ready at t0"
    );
    assert_eq!(
        deb.drain_ready(t0 + Duration::from_millis(100)),
        vec![],
        "a full debounce window of real time does not make a future-stamped entry ready"
    );
    assert_eq!(
        deb.drain_ready(t0 + Duration::from_secs(600)),
        vec![],
        "no amount of elapsed time makes a future-stamped entry ready: \
         `duration_since` saturates to zero, so `>= window` is unreachable"
    );

    // And the worker's park budget stays a FULL window rather than
    // collapsing to zero, so an injected entry costs no spin.
    assert_eq!(
        deb.next_wait(t0),
        Some(Duration::from_millis(100)),
        "next_wait saturates the same way and reports the whole window"
    );
}

// Assertions over these helpers must be MONOTONE UNDER DESCHEDULING.
// A saturated host can deschedule this thread for an entire timeout
// budget, so: lower bounds on `start.elapsed()` and `>=`-style
// "at least once" counts are safe (they only grow under load), while
// upper bounds on elapsed and "more than N attempts" claims invert
// and become flakes. Sharp count/promptness claims belong on the
// `VirtualClock` tests below, which have no wall-clock dependency
// at all. See #5709.
//
// ---------------------------------------------------------------------
// REAL-CLOCK LEDGER for this file. EXTENDS the invariant above; does not
// restate it.
//
// This file has now produced FOUR separate merge-blocking flakes of one
// class (#5143, #5422, #5709, #6438). Each earlier round fixed the line
// that happened to fail and left every other real-clock site implicit,
// which is exactly how instances two, three and four each survived the
// round before them. So this ledger enumerates the real-clock sites in
// the file, including -- especially -- the ones judged safe, with the
// reason, and names them by test function so it survives line drift.
// Every cited name is kept on ONE physical line for that reason: a name
// wrapped across a comment break is invisible to `grep <test_name>`, which
// is the only mechanism by which a rename or deletion can surface a stale
// row here at all. If a name no longer fits the prose, reflow the PROSE.
//
// READ THIS AS A SNAPSHOT, NOT AS A CHECKED INVARIANT. It was accurate
// when written (#6438) and nothing verifies it since; the enforced half
// is the guard named at the bottom of this block. Where the two ever
// disagree, the guard is right. Deliberately count-free: a ledger that
// says "the six barriered polls" is wrong the first time a test is added
// or renamed, and a stale ledger that sounds precise is worse than none,
// because the next maintainer trusts it instead of re-deriving. If you
// add a real-clock site, add its row -- and if a row here no longer
// matches the code, fix the row rather than trusting it.
//
// FIXED BY #6438 (all three deleted, none widened). These rows are
// anchored to deletions, so they cannot drift:
//   * watcher_drop_discards_a_pending_event_rather_than_delivering_it --
//     a hand-rolled 5s deadline off the raw clock, polled every 2ms,
//     hunting for a pending entry that is only observable during its
//     100ms debounce window. Descheduling past that window hard-failed a
//     watcher that had behaved perfectly. Now injects the entry via
//     `FileWatcher::record_pending_for_test` at `far_future_stamp()`.
//   * watcher_drop_wakes_and_joins_a_worker_parked_indefinitely (landed as
//     `watcher_drop_joins_worker_without_hanging_even_with_a_pending_event`,
//     renamed and rewritten in the #6438 review pass) -- an upper bound on
//     real elapsed time around `drop`, plus a `sleep(10ms)` that was its only
//     (and never-confirmed) "an entry is pending" precondition. The bound is
//     gone; see the tombstone in its body. The precondition is now a
//     confirmed delivery, which parks the worker INDEFINITELY -- the one park
//     shape in which `Drop`'s notify is load-bearing, and the reason this
//     test is no longer a subset of the discard test below it.
//   * wait_for_returns_true_when_the_condition_is_already_satisfied
//     (renamed here off `..._returns_true_promptly_...`, so the name cited
//     resolves to a live symbol) -- an upper bound on `start.elapsed()`;
//     see the tombstone just below.
//
// FIXED BY #6462 (both deleted; replaced by a two-delivery
// `wait_for_control_drain` barrier, not widened):
//   * watcher_ignores_non_ri_file_changes -- a fixed 500ms sleep gating the
//     `paths.is_empty()` NEGATIVE assertion on wall-clock separation alone.
//     Now: two deliveries of a dedicated control.ri write. Two deliveries
//     of the SAME path are necessarily two distinct debouncer batches (the
//     debouncer is keyed by path), and the single worker thread completes
//     batch N's callbacks before draining batch N+1, so observing the
//     second delivery proves every callback of the first batch -- including
//     any filtered write issued before it, whose earlier record implies an
//     earlier debounce deadline -- has already run. Race-free by
//     construction, not by timeout.
//   * watcher_with_target_file_only_fires_for_that_file -- the same fixed
//     500ms sleep and the same fix, using the test's own project.ri write
//     as the control: it's the only path the target_file filter ever
//     passes for a Changed event, so it doubles as its own barrier.
//
// JUDGED SAFE, and why:
//   * The sub-debounce-window sleep in
//     watcher_rereads_final_content_after_nonatomic_truncate_then_append.
//     If load stretches it past DEBOUNCE_DURATION the two writes merely
//     split into separate debounce cycles; the ASSERTED claim (terminal
//     content) holds either way, and the coalescing-fidelity claim is
//     `eprintln!`-downgraded rather than asserted, precisely so this
//     cannot fail on load.
//   * The sleep in that same test's bounded watcher-construction retry
//     loop: retry-with-cap terminating in an environment skip, with no
//     timing assertion riding on it at all.
//   * The LOWER bounds on `start.elapsed()` in
//     wait_for_returns_false_after_timeout_when_never_satisfied and
//     wait_until_with_retry_returns_false_after_the_timeout_when_never_satisfied.
//     Monotone under descheduling, i.e. the safe direction, by the invariant
//     above -- and load-bearing in the other: they are what proves
//     `WallClock::sleep` really blocks.
//   * The budget in
//     wait_for_watch_registration_via_removal_confirms_a_watch_behind_a_target_file_filter,
//     spent on a NEGATIVE claim (no Changed event behind the filter) --
//     safe direction, as above.
//   * The budget over an in-process producer thread in
//     wait_for_detects_value_set_by_another_thread: two orders of
//     magnitude of margin, no filesystem and no inotify involved.
//   * The two budgets, and the settle sleep between them, in
//     watcher_delivers_an_injected_pending_entry_whose_window_has_already_elapsed
//     -- generous `wait_for` budgets over hard POSITIVE asserts, on entries
//     stamped in the PAST and therefore already ready when they land.
//     Descheduling only makes such an entry READIER, so there is no window
//     to miss -- which is the precise property the pre-#6438 form of
//     watcher_drop_discards_a_pending_event_rather_than_delivering_it
//     lacked. The settle sleep is not a budget and nothing is asserted
//     about it: it only makes the worker more certainly parked, so load
//     STRENGTHENS what that phase catches instead of inverting it. Added
//     by the #6438 review pass to pin the notify wiring the discard test
//     below silently depends on -- see its doc comment for the measurement
//     that showed a one-phase version could not.
//   * The delivery budget and the settle sleep in
//     watcher_drop_wakes_and_joins_a_worker_parked_indefinitely, which are
//     the same two constructs in the same two safe directions, used there to
//     establish an indefinite park rather than to observe a second delivery.
//   * The registration-barriered condition-polls that pair a generous
//     `wait_for` / `wait_until_with_retry` budget with a hard POSITIVE
//     assert (the watcher_detects_*, watcher_emits_*, watcher_rereads_*,
//     watcher_survives_* and watcher_with_target_file_* tests). This IS
//     the shape #5143 blessed: a condition-poll standing behind a
//     positively confirmed live watch, so the budget is slack rather
//     than a claim. Kept as-is.
//   * The shared helpers those polls run on: `wait_until_on`'s poll
//     interval (clamped to `remaining`) and
//     `wait_for_watch_registration_inner`'s retry cadence and budget.
//     Both are driven through the `WaitClock` seam, and the cadence
//     exceeding DEBOUNCE_DURATION is deliberate -- see that helper's doc.
//   * `wait_for_control_drain`'s two `wait_for` budgets (added by #6462,
//     used at both converted sites above). Each is a generous `wait_for`
//     budget over a hard POSITIVE claim -- that a control delivery
//     arrived -- the same shape the registration-barriered condition-polls
//     row above already blesses: slack, not a claim, and monotone under
//     descheduling.
//   * The debouncer_* / VirtualClock tests. `Instant::now()` there is
//     only a seed for synthetic arithmetic; no real time is consumed and
//     none is asserted on.
//   * watcher_construct_and_drop_in_a_loop_never_hangs -- no timing
//     construct of any kind.
//
// MACHINE-CHECKED HALF -- the part that does NOT rot, and the reason
// this prose can stay a snapshot. Two mechanical rules are enforced over
// every .rs file in this directory by
// tests/infra/test_no_new_wallclock_rust_deadlines.sh: a real-clock
// deadline built by hand off `Instant::now()`, and an UPPER bound
// compared against a `Duration` (lower bounds are monotone-safe and are
// not matched). That script's header is the canonical statement of the
// rules, of the same-line escape comment, of the two shapes a lexical
// guard cannot see (an upper bound against a named constant, and a
// construct hand-wrapped across lines), and of the three sanctioned
// fixes to try before reaching for an escape. Do not restate them here
// -- read them there, so there is one copy to keep true.
//
// Exactly ONE escape exists in tree: `far_future_stamp()` below, argued
// at the site. A second one is a deliberate, reviewable act rather than
// pre-existing noise.
// ---------------------------------------------------------------------

// The upper-bound half of the test below -- `start.elapsed()` compared
// against a one-second `Duration`, asserted under the name
// `..._returns_true_promptly_...` -- was removed here (#6438).
// It was the last live contradiction of the invariant stated directly above:
// an upper bound on elapsed time is starvation-invertible, so a host that
// deschedules this thread for a second fails a `wait_for` that did exactly
// the right thing. Same shape as the
// `wait_until_with_retry_returns_true_without_waiting_when_already_satisfied`
// tombstone further down.
//
// The PROMPTNESS claim is not lost, and is not even weakened: it is already
// pinned exactly, and deterministically, by
// `wait_until_on_does_not_advance_the_clock_when_the_condition_already_holds`
// below, which asserts `clock.now() == t0` on a `VirtualClock`. "The clock
// did not advance AT ALL" is strictly stronger than "under a second of real
// time elapsed", and it has no wall-clock dependency whatsoever. `wait_for`
// reaches that same already-holds fast path via `wait_until` -> `wait_until_on`,
// so the behaviour under test here is the behaviour pinned there.
//
// What remains below is this test's real content: that the ARRIVAL claim
// holds -- a predicate already satisfied is observed on the first check --
// which is monotone under descheduling and therefore safe. The name lost
// "promptly" to match.
#[test]
fn wait_for_returns_true_when_the_condition_is_already_satisfied() {
    let sink: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![42]));
    let found = wait_for(&sink, Duration::from_secs(10), |v: &[u32]| v.contains(&42));
    assert!(found, "predicate should be satisfied on the first check");
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

// Synthetic-sink, inotify-free meta-tests pinning the contract of
// `wait_for_control_drain` (added by #6462; the helper itself is defined
// beside the other `wait_*` helpers above). No FileWatcher, no tempdir, no
// inotify -- these keep passing even on hosts where every watcher test in
// this file skips. `write_control` here plays the role the debouncer's
// worker thread plays in production: each of these tests drives it
// directly rather than through a real watch, so the barrier's two-delivery
// contract is pinned deterministically.

#[test]
fn wait_for_control_drain_does_not_return_until_a_same_batch_straggler_has_landed() {
    // THE DISCRIMINATING TEST. write_control simulates a worker draining
    // two debouncer batches: call 1 pushes ONLY control.ri (a batch whose
    // control was pushed first and whose straggler has not been pushed yet
    // -- exactly the window a one-delivery barrier would snapshot in);
    // call 2 pushes straggler.ri THEN control.ri (batch 1's callbacks all
    // returned before batch 2's push, because one worker thread runs a
    // batch to completion before draining the next). A barrier that
    // returns as soon as it observes the FIRST control delivery would
    // return before straggler.ri ever lands, leaving a negative assertion
    // gated on it vacuous -- a one-delivery implementation fails this test.
    let sink: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let write_sink = sink.clone();
    let call = Rc::new(Cell::new(0u32));
    let call_counter = call.clone();

    let drained = wait_for_control_drain(
        &sink,
        "control.ri",
        move || {
            let n = call_counter.get() + 1;
            call_counter.set(n);
            let mut guard = write_sink.lock().unwrap();
            if n == 1 {
                guard.push(PathBuf::from("control.ri"));
            } else {
                guard.push(PathBuf::from("straggler.ri"));
                guard.push(PathBuf::from("control.ri"));
            }
        },
        Duration::from_secs(10),
    );

    assert!(drained, "two control deliveries should satisfy the barrier");
    let paths = sink.lock().unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("straggler.ri")),
        "the barrier returned before the same-batch straggler landed -- a \
         negative assertion gated on this barrier would be vacuous, got: {:?}",
        *paths
    );
}

#[test]
fn wait_for_control_drain_returns_false_when_the_control_is_never_delivered() {
    // write_control is a no-op: the FIRST wait (count >= 1) never
    // succeeds, so the barrier must time out and return false after that
    // one wait -- it must not block on a second window once the first has
    // already failed.
    let sink: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));

    let drained = wait_for_control_drain(&sink, "control.ri", || {}, Duration::from_millis(150));

    assert!(
        !drained,
        "control is never delivered, barrier should time out"
    );
}

#[test]
fn wait_for_control_drain_returns_false_when_only_the_first_delivery_ever_lands() {
    // write_control pushes control.ri on the FIRST call only; a second
    // call (if the implementation makes one) is a no-op. Pins that the
    // SECOND wait is load-bearing: a one-delivery implementation that
    // returns as soon as the first wait succeeds would return true here --
    // exactly the vacuity risk this helper exists to close.
    let sink: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let write_sink = sink.clone();
    let call = Rc::new(Cell::new(0u32));
    let call_counter = call.clone();

    let drained = wait_for_control_drain(
        &sink,
        "control.ri",
        move || {
            let n = call_counter.get() + 1;
            call_counter.set(n);
            if n == 1 {
                write_sink.lock().unwrap().push(PathBuf::from("control.ri"));
            }
        },
        Duration::from_millis(150),
    );

    assert!(
        !drained,
        "only one control delivery ever lands, barrier should time out \
         waiting for the second"
    );
}

// `wait_until_with_retry_returns_true_without_waiting_when_already_satisfied`
// (a real-clock "found + elapsed < 200ms" test) was removed here: its
// upper-bound-on-elapsed claim was starvation-invertible like the count
// assertion above, and once widened to a non-discriminating 5s bound its
// two remaining claims were each already covered elsewhere -- `found` is
// proven deterministically by
// `wait_until_with_retry_does_not_sleep_when_the_condition_already_holds_on_a_virtual_clock`
// below (which also proves the sharp "never sleeps" form no wall-clock
// bound can), and that the real wrapper is actually wired to `WallClock`
// is proven by `..._returns_false_after_the_timeout_when_never_satisfied`
// just below (`elapsed >= 200ms` only holds if `WallClock::sleep` really
// blocks). See #5709.

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
    // `>= 1`, not `> 1`: a thread descheduled for the whole 200ms budget
    // between the first `attempt()` and the deadline check legitimately
    // issues exactly one attempt, so a "more than once" claim here is
    // starvation-invertible (this was the reported flake). The exact
    // "reissued more than once" claim -- deterministically 10 attempts --
    // now lives on the virtual clock in
    // `wait_until_with_retry_reissues_the_attempt_for_every_window_until_the_deadline_on_a_virtual_clock`
    // below. `>= 1` is starvation-proof by construction: `attempt()` is
    // called unconditionally before the loop ever computes `remaining` or
    // checks the deadline. See #5709.
    assert!(
        counter.get() >= 1,
        "attempt should be issued unconditionally at least once, even when \
         the condition never holds, got {}",
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
    assert_eq!(
        counter.get(),
        10,
        "attempt should have been reissued for every window until the \
         deadline (200ms budget / 20ms windows = 10 attempts), got {}",
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
fn wait_until_with_retry_on_clamps_its_final_window_to_remaining_on_a_non_multiple_timeout() {
    // The 200ms/20ms case above (and the 2s/20ms case below) can't
    // distinguish the retry loop's OWN `remaining.min(retry_every)` clamp
    // from a regression that used a bare `retry_every`, because both land
    // on the same total either way when the timeout is an exact multiple
    // of retry_every -- this is the retry loop's own clamp, one layer
    // above the (separately covered) clamp inside `wait_until_on` itself.
    // A non-multiple budget can distinguish them: unclamped, a third 20ms
    // window would overshoot 50ms to 60ms; clamped, it's cut to 10ms and
    // the call lands on exactly 50ms (windows of 20 + 20 + 10). See #5709.
    let t0 = Instant::now();
    let mut clock = VirtualClock::new(t0);
    let counter = Rc::new(Cell::new(0u32));
    let attempt_counter = counter.clone();

    let found = wait_until_with_retry_on(
        &mut clock,
        move || attempt_counter.set(attempt_counter.get() + 1),
        Duration::from_millis(20),
        Duration::from_millis(50),
        || false,
    );

    assert!(!found, "condition is never satisfied, should time out");
    assert_eq!(
        counter.get(),
        3,
        "three windows (20 + 20 + 10ms) should fit in a 50ms budget, got {} attempts",
        counter.get()
    );
    assert_eq!(
        clock.now() - t0,
        Duration::from_millis(50),
        "the final window should be clamped to the remaining budget rather \
         than overshooting to 60ms, got {:?}",
        clock.now() - t0
    );
}

#[test]
fn wait_until_with_retry_stops_reissuing_once_the_condition_holds_on_a_virtual_clock() {
    // Deterministic replacement for the former (now-deleted) wall-clock test
    // `wait_until_with_retry_reissues_the_attempt_until_the_condition_holds`.
    // Preserved rationale: `attempt` only flips the shared counter;
    // `condition` reads the SAME counter and is satisfied once it reaches 3.
    // If `wait_until_with_retry` only invoked `attempt` once (like a plain
    // poll), the condition would never hold and this would time out.
    // Reissuing it is exactly the property the de-flake depends on: a
    // stimulus (e.g. a write) lost to a not-yet-live watcher must be
    // re-issued, not just waited on.
    //
    // On the virtual clock, the condition is re-checked at the head of each
    // poll window, so the 3rd attempt short-circuits before any further
    // sleep -- the count is deterministically exactly 3 rather than merely
    // "eventually >= 3" within a wall-clock budget. See #5709.
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
fn wait_until_with_retry_attempts_exactly_once_when_the_condition_already_holds_on_a_virtual_clock()
 {
    // This test's subject is the attempt count, not the clock: "the clock
    // never advances when the condition already holds" is pinned once, at
    // the layer that actually implements it, by
    // `wait_until_on_does_not_advance_the_clock_when_the_condition_already_holds`
    // below -- duplicating that assertion here added scaffolding without a
    // distinct failure mode. What IS unique to the retry wrapper is that it
    // calls `attempt` exactly once (not zero, not more) before the first,
    // immediately-successful poll window. See #5709.
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
}

#[test]
#[should_panic(expected = "retry_every must be non-zero")]
fn wait_until_with_retry_on_panics_when_retry_every_is_zero() {
    // The guard in `wait_until_with_retry_on` is an `assert!`, not a
    // `debug_assert!`, precisely so this stays a loud, profile-independent
    // panic instead of degrading to a hang that only reproduces in debug
    // builds -- see the comment on the guard itself. See #5709.
    wait_until_with_retry_on(
        &mut VirtualClock::new(Instant::now()),
        || {},
        Duration::ZERO,
        Duration::from_millis(200),
        || false,
    );
}

#[test]
fn wait_until_on_clamps_the_final_sleep_to_remaining_on_a_virtual_clock() {
    // The doc comment above `wait_until_on` claims the 20ms poll interval
    // is clamped to whatever time remains before `deadline`, so the final
    // sleep of a call never overshoots `timeout` by a full interval. A
    // budget that's an exact multiple of the poll interval -- like the
    // 200ms/20ms cases above -- can't distinguish clamped from unclamped
    // behaviour, since both land on exactly 200ms. A non-multiple budget
    // can: unclamped, the third 20ms window would overshoot 50ms to 60ms;
    // clamped, that window is cut short and it lands on exactly 50ms. See
    // #5709.
    let t0 = Instant::now();
    let mut clock = VirtualClock::new(t0);

    let found = wait_until_on(&mut clock, Duration::from_millis(50), || false);

    assert!(!found, "condition is never satisfied, should time out");
    assert_eq!(
        clock.now() - t0,
        Duration::from_millis(50),
        "the final poll window should be clamped to the remaining budget \
         rather than the full 20ms interval, got {:?}",
        clock.now() - t0
    );
}

#[test]
fn wait_until_on_does_not_advance_the_clock_when_the_condition_already_holds() {
    // Companion to the clamping test above, pinned at the same layer:
    // proves `wait_until_on` itself checks `condition` before ever
    // sleeping, directly rather than through the `wait_until_with_retry_on`
    // wrapper. This is the ONLY place that claim is pinned -- the
    // retry-layer test
    // (`wait_until_with_retry_attempts_exactly_once_when_the_condition_already_holds_on_a_virtual_clock`)
    // asserts only its own attempt count now, since duplicating this clock
    // assertion there added no distinct failure mode. See #5709.
    let t0 = Instant::now();
    let mut clock = VirtualClock::new(t0);

    let found = wait_until_on(&mut clock, Duration::from_secs(10), || true);

    assert!(found, "already-satisfied condition should return true");
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
    // Created lazily by the first control write below, same as probe.ri --
    // the notify wiring collapses an initial Create and a later Modify to
    // the same `FileEvent::Changed`, so there is nothing to gain from
    // pre-creating it.
    let control_file = dir.path().join("control.ri");

    let changed_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let changed_clone = changed_paths.clone();
    let control_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let control_clone = control_paths.clone();
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
            // MANDATORY for the same reason as the probe.ri arm above:
            // control.ri IS a .ri file, and this test asserts
            // `paths.is_empty()` below. Routing it into its own sink
            // (rather than `changed_paths`) is what lets control.ri serve
            // as a positive-progress barrier (#6462) while keeping that
            // assertion meaning "no event of any kind reached the
            // callback for a filtered file" instead of weakening it to a
            // `.txt`-specific check.
            if path.ends_with("control.ri") {
                control_clone.lock().unwrap().push(path);
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

    // #6462: a fixed 500ms sleep stood here, gating the absence assertion
    // below on wall-clock separation alone. Replaced with a
    // positive-progress barrier: two deliveries of control.ri (which the
    // extension filter passes) prove, by construction rather than by
    // timeout, that the .txt write above has already been through the
    // notify closure and dropped -- see `wait_for_control_drain`'s doc
    // comment for why two deliveries, not one, are required.
    let mut control_attempt = 0u32;
    let drained = wait_for_control_drain(
        &control_paths,
        "control.ri",
        || {
            control_attempt += 1;
            std::fs::write(
                &control_file,
                format!("structure Control {{ param n = {control_attempt} }}"),
            )
            .unwrap();
        },
        Duration::from_secs(10),
    );
    assert!(
        drained,
        "control.ri should have been delivered twice -- without that, the \
         absence assertion below proves nothing about the extension filter"
    );

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

    // Modify the other .ri file (should be ignored due to target_file
    // filter). This write must stay strictly BEFORE the project.ri control
    // writes below: the batch argument in the comment further down depends
    // on other.ri having been recorded first, so its debounce deadline is
    // no later than the first control write's.
    std::fs::write(&other_file, "structure Other { param x = 10mm }").unwrap();

    // #6462: a fixed 500ms sleep stood here, separating the other.ri write
    // above from the project.ri write below by 5x the debounce window.
    // Replaced with a positive-progress barrier -- project.ri (the target
    // file) doubles as its own control, since it's the only path the
    // target_file filter passes for Changed events. Bind the result so a
    // genuine regression fails via the assert below with a clear message,
    // rather than the boolean being silently discarded.
    let mut project_attempt = 0u32;
    let found = wait_for_control_drain(
        &changed_paths,
        "project.ri",
        || {
            project_attempt += 1;
            std::fs::write(
                &project_file,
                format!("structure Project {{ param y = {project_attempt}mm }}"),
            )
            .unwrap();
        },
        Duration::from_secs(10),
    );

    // The negative check below is an immediate snapshot, not a poll:
    // asserting an event's absence can only ever false-PASS under a
    // condition-poll (there's no positive condition to wait for), so
    // polling here would just add latency on every green run for no
    // correctness benefit. It is race-free by CONSTRUCTION now, not by
    // wall-clock separation: the debouncer is keyed by path, so the two
    // project.ri deliveries `found` waits for above are necessarily two
    // distinct debouncer batches, and the single worker thread completes
    // batch N's callbacks before draining batch N+1 (see
    // `wait_for_control_drain`'s doc comment). other.ri was recorded
    // before the FIRST project.ri write, so a broken target_file filter's
    // other.ri entry has a debounce deadline no later than that first
    // write's, and is therefore pushed in a batch no later than the first
    // project.ri delivery's -- which has necessarily already run by the
    // time the SECOND delivery (what `found` actually observes) lands. No
    // wall-clock separation is assumed anywhere, which is why the 500ms
    // sleep could go.
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

/// An `Instant` far enough past the present that a `Debouncer` entry stamped
/// with it can never become ready, for as long as any test could plausibly
/// run. The single seam that takes real time out of
/// `watcher_drop_discards_a_pending_event_rather_than_delivering_it` below.
///
/// WHY an entry stamped here is structurally un-drainable, rather than merely
/// unlikely to be drained: stated once, at the seam, in
/// `FileWatcher::record_pending_for_test` in `watcher.rs`. Pinned executably
/// by `debouncer_a_record_stamped_after_now_is_never_ready_and_reports_a_full_window`
/// above, which goes red at the identity if it ever stops holding.
///
/// THIS FILE'S ONE ESCAPE, and why it is argued rather than dodged (#6438
/// review). The line below is the only real-`Instant` offset in the file, and
/// `tests/infra/test_no_new_wallclock_rust_deadlines.sh` (Rule A) matches it:
/// a deadline built off the raw clock instead of taken through the
/// `WaitClock` seam. Being matched is the CORRECT outcome -- the rule keys on
/// the shape, and this line genuinely has that shape. What makes the site
/// legitimate is its DIRECTION, which no lexical rule can see: the offset
/// exists to take real time OUT of the discard test below, not to hand it a
/// budget a loaded host can blow. Nothing here can expire; a LARGER
/// offset is strictly more un-drainable, never flakier. So the site carries
/// the guard's same-line escape annotation, with that reason attached. (The
/// token itself is written only on the line it annotates: a second contiguous
/// copy in prose would silently mark THAT line escaped too, and would inflate
/// any "count the escapes in tree" audit.)
///
/// `checked_add` is kept for stating the no-overflow intent outright, NOT as a
/// way around the rule: an earlier draft of this task spelled it that way
/// precisely because Rule A then keyed on `+` alone, which hid this site and
/// advertised an undetectable spelling to every future author of a real
/// deadline. Rule A now matches both spellings, and the escape does the
/// arguing.
fn far_future_stamp() -> Instant {
    // The offset is bound on one line so the escape sits on the matched line:
    // both of the guard's rules are single-physical-line by design.
    let stamp = Instant::now().checked_add(Duration::from_secs(3600)); // wallclock:allow -- see above
    stamp.expect("an hour past now is representable as an Instant")
}

/// An `Instant` far enough BEFORE the present that a `Debouncer` entry
/// stamped with it is ALREADY ready to drain -- the mirror of
/// [`far_future_stamp`], and the seam the drive-half test just below rests
/// on.
///
/// No escape is needed here and none is taken: subtracting from the raw clock
/// is not a deadline. `Debouncer::drain_ready` asks
/// `now.duration_since(last_seen) >= window`, so moving `last_seen` FURTHER
/// into the past only makes that hold sooner. The direction is monotone under
/// descheduling -- the safe one -- exactly like the lower bounds
/// `tests/infra/test_no_new_wallclock_rust_deadlines.sh` deliberately does
/// not match, and unlike the future offset above, which has to argue its case
/// with an escape.
///
/// ONE SECOND, not the hour an earlier draft used, and the fallback is LOUD
/// (#6438 review). On Linux an `Instant` is CLOCK_MONOTONIC -- time since boot
/// -- so an offset larger than the host's uptime is not representable, and an
/// hour is a realistic uptime for a freshly booted CI VM or container. With a
/// silent `unwrap_or(now)` the stamp there was not already-elapsed at all: the
/// caller quietly degraded into an ordinary 100ms-debounce delivery test,
/// still green, testing something other than its name -- the same silent
/// vacuity this task exists to remove elsewhere in this file. A second is an
/// order of magnitude past DEBOUNCE_DURATION, which is all the callers need,
/// and is representable on any host that has been up long enough to run a
/// test at all; `expect` rather than a fallback so that if it somehow is not,
/// the run says so instead of quietly testing less.
///
/// `checked_sub` rather than `-` for the same reason: `Instant - Duration`
/// panics on underflow with no message worth reading.
fn already_elapsed_stamp() -> Instant {
    let now = Instant::now();
    now.checked_sub(Duration::from_secs(1))
        .expect("a second before now is representable as an Instant (host uptime < 1s?)")
}

/// THE DRIVE HALF of `FileWatcher::record_pending_for_test` -- the half
/// `watcher_drop_discards_a_pending_event_rather_than_delivering_it` below
/// depends on and cannot exercise (#6438 review).
///
/// That test injects a future-stamped, structurally un-drainable entry and
/// then asserts NON-delivery, and `pending_paths()` only proves the entry
/// landed in the shared map. So the hook's documented claim to record
/// "exactly as if the notify closure had observed a change ... same lock,
/// same `Debouncer::record` call, same `notify_one` afterwards" was untested
/// on its second half: delete the `cvar.notify_one()`, or wire the hook to a
/// different `Condvar`, and every test in this file still passed green --
/// while the equivalence that test's vacuity argument rests on had quietly
/// stopped holding.
///
/// This test closes that by injecting an ALREADY-ELAPSED stamp and asserting
/// the callback fires with that path.
///
/// WHY THERE ARE TWO PHASES, and why the first one alone is not enough. The
/// notify only matters when the worker is ALREADY blocked in `cvar.wait`: if
/// it has not reached its park yet, its next `drain_ready` finds the injected
/// entry regardless and delivers it. A freshly constructed watcher is exactly
/// that case -- the worker thread has just been spawned and, on a loaded
/// host, may not have run a single iteration by the time the test injects. So
/// a one-phase version of this test passes with `cvar.notify_one()` DELETED
/// from the hook, which was measured, not assumed (#6438 review: the deleted
/// -notify build passed the one-phase form in 0.02s).
///
/// The second phase closes it. Once the first delivery has been observed the
/// worker has drained the map and is on its way back to an INDEFINITE park --
/// `next_wait` returns `None` on an empty debouncer, so that park has no
/// timeout that could paper over a missing wire. The settle delay before the
/// second injection is not a budget and nothing is asserted about it: a
/// LONGER delay only makes the worker more certainly parked, so the mutation
/// -catching power of this test grows under load rather than inverting, and
/// the delivery assertion itself passes either way while the hook is correct.
/// With `notify_one()` removed the second phase hangs and fails on its
/// budget, which is what makes this test the drive half's real pin.
///
/// LOAD DIRECTION, per this file's REAL-CLOCK LEDGER: this is the blessed
/// shape -- a generous budget paired with a hard POSITIVE assert, with no
/// upper bound on elapsed time anywhere. Descheduling only makes an
/// already-elapsed entry MORE ready, never less, so there is no window to
/// miss here (which is exactly what made the pre-#6438 form of the test below
/// flaky) and nothing that inverts under load.
#[test]
fn watcher_delivers_an_injected_pending_entry_whose_window_has_already_elapsed() {
    let dir = tempfile::tempdir().unwrap();

    let received: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = received.clone();

    // Nothing is written to disk in this test either, so -- as in the discard
    // test below -- the notify closure cannot produce an event of its own and
    // every path that reaches `received` came through the injection hook.
    let Some(watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            received_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // PHASE 1: the record -> drain -> callback path itself. The worker may
    // still be starting up here, so this phase does not depend on the notify.
    watcher.record_pending_for_test(
        dir.path().join("first.ri"),
        ChangeKind::Changed,
        already_elapsed_stamp(),
    );
    let first_delivered = wait_for(&received, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("first.ri"))
    });
    assert!(
        first_delivered,
        "an injected entry whose debounce window has already elapsed should be \
         drained and delivered to the callback; got: {:?}",
        *received.lock().unwrap()
    );

    // PHASE 2: the notify. The first delivery is proof the worker drained the
    // map, so it is now heading for an indefinite `cvar.wait` -- see the doc
    // comment for why this phase, and not the first, is what fails when
    // `record_pending_for_test` stops waking the worker.
    std::thread::sleep(Duration::from_millis(50));
    watcher.record_pending_for_test(
        dir.path().join("second.ri"),
        ChangeKind::Changed,
        already_elapsed_stamp(),
    );
    let second_delivered = wait_for(&received, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("second.ri"))
    });
    assert!(
        second_delivered,
        "an entry injected once the worker had already parked was never \
         delivered -- the injection hook recorded into the debouncer without \
         waking the worker (or woke a different condvar), which would also let \
         the discard test below pass vacuously; got: {:?}",
        *received.lock().unwrap()
    );
}

/// `Drop` must WAKE and join a worker that is parked INDEFINITELY -- the one
/// park shape in which `Drop`'s `notify_all` is load-bearing, and the only
/// failure mode this test can have is a hang.
///
/// WHY THE PARK SHAPE IS THE WHOLE POINT (#6438 review). This test landed as
/// `watcher_drop_joins_worker_without_hanging_even_with_a_pending_event`,
/// which injected a future-stamped entry and dropped. That made it a strict
/// subset of `watcher_drop_discards_a_pending_event_rather_than_delivering_it`
/// just below -- same construction, same injection, same drop, and no
/// assertion of its own -- so every failure it could see, that test saw first.
/// Worse, its stated lost-wakeup rationale held in NEITHER test: with an entry
/// pending, the worker parks in `wait_timeout` with a full debounce window as
/// its budget, so a `Drop` that never notified would still be joined about a
/// window later and nothing would notice.
///
/// A worker with an EMPTY debouncer parks in `Condvar::wait` with no timeout
/// at all (`next_wait` returns `None`), so there is no timeout to paper over a
/// missing wire: delete `notify_all` from `Drop` and this test hangs. Getting
/// the worker there is what the first half does -- an already-elapsed entry is
/// injected and its delivery awaited, and that delivery is the proof the
/// worker ran, drained the map, and is on its way back to the indefinite park.
///
/// WHAT THIS OWNS THAT NOTHING ELSE DOES: the attribution. The scope-exit drop
/// ending `watcher_delivers_an_injected_pending_entry_whose_window_has_already_elapsed`
/// leaves a worker parked the same way and so would hang too -- but
/// incidentally, as a side effect of a test about delivery, and one edit to
/// that test's ending would remove the coverage with nothing to say so. Here
/// the contract is named, the precondition is established deliberately, and a
/// hang reads as what it is.
///
/// RETURNING AT ALL IS THE ASSERTION -- the honest statement of "`Drop` must
/// not hang", and what the deleted `elapsed` upper bound was really guarding
/// (see the tombstone in the body).
///
/// LOAD DIRECTION, per this file's REAL-CLOCK LEDGER: the delivery budget is
/// the blessed shape (generous budget, hard POSITIVE assert, on an
/// already-elapsed entry that descheduling only makes readier), and the settle
/// delay is not a budget -- a longer one only makes the worker more certainly
/// parked, so load strengthens this test rather than inverting it.
#[test]
fn watcher_drop_wakes_and_joins_a_worker_parked_indefinitely() {
    let dir = tempfile::tempdir().unwrap();

    let delivered: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let delivered_clone = delivered.clone();

    let Some(watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            delivered_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    // Drive one full record -> drain -> deliver cycle. The delivery is this
    // test's precondition: it proves the worker emptied the debouncer, which
    // is what sends it into an indefinite park rather than a timed one.
    watcher.record_pending_for_test(
        dir.path().join("parked.ri"),
        ChangeKind::Changed,
        already_elapsed_stamp(),
    );
    let drained = wait_for(&delivered, Duration::from_secs(10), |paths| {
        paths.iter().any(|p| p.ends_with("parked.ri"))
    });
    assert!(
        drained,
        "the injected entry should be delivered, leaving the debouncer empty \
         and the worker heading for an indefinite park; got: {:?}",
        *delivered.lock().unwrap()
    );

    // Settle, so the worker has reached that park before the drop below.
    // Nothing is asserted about this delay; see the doc comment.
    std::thread::sleep(Duration::from_millis(50));

    drop(watcher);

    // A real-clock upper bound around the `drop` above -- `elapsed` compared
    // against a two-second `Duration` -- was removed here (#6438). It
    // contradicted this file's own
    // monotone-under-descheduling invariant stated above the `wait_*` tests:
    // an upper bound on elapsed time INVERTS under load -- a host that
    // deschedules this thread for two seconds fails a correct `Drop` -- which
    // is precisely the shape already deleted in the
    // `wait_until_with_retry_returns_true_without_waiting_when_already_satisfied`
    // tombstone. Widening it would only have traded a flake for a
    // non-discriminating bound.
    //
    // Nothing is lost: the assertion's only real job was catching a
    // lost-wakeup HANG, and a hang is still caught loudly, by the harness
    // rather than by this thread. .config/nextest.toml sets
    // `slow-timeout = { period = "120s", terminate-after = 10 }`, so a `drop`
    // that never returns is reported as a slow test and then terminated,
    // instead of silently passing. Please do not "restore" the bound.
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
/// HOW THE PENDING ENTRY GETS THERE, and why that is no longer a race
/// (#6438). This test used to write a real file, wait for the directory
/// watch to be confirmed live, then poll `pending_paths()` on a 2ms cadence
/// trying to catch the entry inside its 100ms debounce window. That window
/// is the ONLY interval in which the entry is observable, so a loaded host
/// that descheduled the polling thread straight past it made the test
/// hard-fail on its 5s deadline even though the watcher had behaved
/// perfectly and the event had been delivered normally. Nothing was lost and
/// nothing was wrong; the test simply lost a 100ms real-time race. Widening
/// the deadline could never have helped -- the thing being polled for was
/// already gone. That is the flake this task exists to kill, and the fourth
/// instance of the class in this file.
///
/// The pending entry is now injected directly, through
/// `FileWatcher::record_pending_for_test`, stamped at `far_future_stamp()`.
/// A future-stamped entry is structurally un-drainable, so "an entry is
/// pending when `Drop` runs" is an invariant rather than a race, and the
/// discard assertion below is UNCONDITIONAL.
///
/// WHAT THAT IS, AND IS NOT, STRONGER THAN (#6438 review). On the axis that
/// produced the flake it is strictly stronger: the precondition is checked
/// rather than raced, and the assertion can no longer degrade to the pre-fix
/// form's `eprintln!` skip when its re-snapshot went stale. On REGRESSION
/// COVERAGE it is narrower, and saying otherwise would mislead the next
/// reader into thinking this test owns more than it does:
///   * A `Drop` that delivered pending entries unconditionally, ignoring the
///     debounce window, still fails here. That is the shape the contract
///     forbids most directly, and it is the shape this test owns.
///   * A `Drop` that flushed via `drain_ready(Instant::now())` before joining
///     would find a future-stamped entry NOT ready, deliver nothing, and pass
///     this test unchanged. The load-bearing half of that shape does have an
///     owner, just not here:
///     `debouncer_lone_record_becomes_ready_only_after_the_window_elapses`
///     pins that a drain at a `now` inside the quiet window returns nothing,
///     so a `drain_ready` that began handing back not-yet-ready entries goes
///     red there, deterministically. What is genuinely uncovered is only the
///     WIRING -- a shutdown path that calls a flush at all -- and no test in
///     this file can cover that without re-introducing the 100ms real-time
///     race this task exists to remove.
///   * A `Drop` that WAITED for the debouncer to drain would hang rather than
///     fail with the message below. That is reported by the harness, not by
///     this thread: `.config/nextest.toml` sets `slow-timeout` with
///     `terminate-after`, the same backstop the two tombstones in this file
///     rely on in place of their deleted upper bounds.
///
/// The contract exercised is still exactly the real one: `Drop` never reads
/// `last_seen`. It sets `shutdown` under the mutex, notifies the condvar and
/// joins; the worker returns at the top of its loop without draining. The
/// future stamp changes only how the entry got into the map, never what
/// `Drop` does with it.
///
/// Nothing is lost by dropping the real write either -- the full
/// inotify -> debouncer -> callback path stays covered end to end by
/// `watcher_detects_ri_file_modification`, `watcher_detects_ri_file_removal`,
/// `watcher_emits_remove_event_even_when_target_file_filter_excludes_other_files`,
/// `watcher_rereads_final_content_after_nonatomic_truncate_then_append` and
/// `watcher_survives_a_panicking_callback_and_keeps_delivering_later_events`.
#[test]
fn watcher_drop_discards_a_pending_event_rather_than_delivering_it() {
    let dir = tempfile::tempdir().unwrap();

    let received: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = received.clone();

    // No probe filter and no registration barrier any more: this test writes
    // NOTHING to disk, so the notify closure cannot produce an event at all.
    // Any path that reaches `received` is therefore a real violation of the
    // discard contract rather than incidental probe traffic to be screened
    // out -- which is why the callback can now push unconditionally.
    let Some(watcher) = try_watcher(dir.path(), None, move |event| {
        if let FileEvent::Changed(path) = event {
            received_clone.lock().unwrap().push(path);
        }
    }) else {
        return;
    };

    let ri_file = dir.path().join("abandoned.ri");
    watcher.record_pending_for_test(ri_file.clone(), ChangeKind::Changed, far_future_stamp());
    assert!(
        watcher
            .pending_paths()
            .iter()
            .any(|p| p.ends_with("abandoned.ri")),
        "the injected entry should be pending before Drop -- otherwise this \
         run does not exercise the discard-on-drop contract at all"
    );

    // `Drop` joins the worker thread before returning, so once this call is
    // back the worker has already exited -- there's no race to poll for
    // below; a direct snapshot is enough.
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
