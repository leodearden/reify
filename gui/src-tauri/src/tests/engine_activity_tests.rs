//! Tests for [`crate::engine_activity`].

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::MockGeometryKernel;

use crate::engine::EngineSession;
use crate::engine_activity::{
    EngineActivity, SettleRequest, SettleVerdict, WaitOutcome, probe, stamp_generation, verdict,
    wait_until_settled,
};
use crate::eval_queue::{EvalOutcome, EvalProgress, EvalRequest};
use crate::tests::make_test_engine;
use crate::tests::test_helpers::{ANTI_WEDGE, ManualQueue};

type Engine = Arc<Mutex<EngineSession>>;

/// Another thread holding the engine mutex until [`LockHolder::release`].
struct LockHolder {
    release: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}

impl LockHolder {
    /// Returns once the other thread holds the lock.
    fn hold(engine: &Engine) -> Self {
        let engine = Arc::clone(engine);
        let (held_tx, held_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let _guard = engine.lock().unwrap_or_else(|p| p.into_inner());
            held_tx
                .send(())
                .expect("the test waits for the lock to be held");
            let _ = release_rx.recv_timeout(ANTI_WEDGE);
        });
        held_rx
            .recv_timeout(ANTI_WEDGE)
            .expect("the holder thread must take the engine lock");
        Self { release, thread }
    }

    fn release(self) {
        let _ = self.release.send(());
        self.thread
            .join()
            .expect("the holder thread must not panic");
    }
}

fn never_loaded_engine() -> Engine {
    Arc::new(Mutex::new(EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    )))
}

fn poison(engine: &Engine) {
    let engine = Arc::clone(engine);
    let panicked = std::thread::spawn(move || {
        let _guard = engine.lock().unwrap_or_else(|p| p.into_inner());
        panic!("poisoning the engine mutex on purpose");
    })
    .join();
    assert!(panicked.is_err(), "the poisoning thread must have panicked");
}

fn unrun_evaluation(queue: &ManualQueue) -> crate::eval_queue::EvalTicket<()> {
    queue
        .queue
        .submit(EvalRequest::evaluation(|| EvalOutcome::succeeded(None, ())))
}

// ── probe: the engine lane's busy-ness, read without blocking ────────────────

#[test]
fn an_idle_loaded_engine_and_idle_queue_probe_as_not_busy() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();

    let activity = probe(&engine, &queue.queue);

    assert!(!activity.busy(), "got {activity:?}");
    assert!(!activity.engine_lock_held);
    assert_eq!(activity.engine_started, Some(true));
    assert_eq!(
        activity.queue,
        EvalProgress {
            generation: 0,
            outstanding: 0
        }
    );
}

/// If the probe took the lock with `lock()` it would wait for the holder, which
/// releases only after the probe's result has been asserted: a deadlock.
#[test]
fn a_held_engine_lock_probes_as_busy_without_waiting_for_it() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();
    let holder = LockHolder::hold(&engine);

    let activity = probe(&engine, &queue.queue);
    holder.release();

    assert!(activity.engine_lock_held, "got {activity:?}");
    assert_eq!(
        activity.engine_started, None,
        "unknown while another thread holds the lock"
    );
    assert!(activity.busy());
}

#[test]
fn an_unrun_queued_evaluation_probes_as_busy_with_the_lock_free() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();
    let _ticket = unrun_evaluation(&queue);

    let activity = probe(&engine, &queue.queue);

    assert!(activity.busy(), "got {activity:?}");
    assert!(!activity.engine_lock_held);
    assert_eq!(activity.queue.outstanding, 1);
}

#[test]
fn a_never_loaded_session_probes_as_not_started() {
    let engine = never_loaded_engine();
    let queue = ManualQueue::new();

    let activity = probe(&engine, &queue.queue);

    assert_eq!(activity.engine_started, Some(false), "got {activity:?}");
    assert!(!activity.busy());
}

#[test]
fn a_poisoned_engine_mutex_is_not_read_as_busy() {
    let engine = make_test_engine();
    poison(&engine);
    assert!(
        engine.is_poisoned(),
        "the fixture must really poison the lock"
    );
    let queue = ManualQueue::new();

    let activity = probe(&engine, &queue.queue);

    assert!(!activity.engine_lock_held, "got {activity:?}");
    assert_eq!(activity.engine_started, Some(true));
    assert!(!activity.busy());
}

// ── verdict: whether a waiter may stop waiting ───────────────────────────────

fn activity(
    engine_lock_held: bool,
    engine_started: Option<bool>,
    generation: u64,
    outstanding: usize,
) -> EngineActivity {
    EngineActivity {
        engine_lock_held,
        engine_started,
        queue: EvalProgress {
            generation,
            outstanding,
        },
    }
}

#[test]
fn the_verdict_follows_busy_then_awaiting_then_not_started_then_settled() {
    let held = activity(true, None, 4, 0);
    let queued = activity(false, Some(true), 4, 1);
    let idle = activity(false, Some(true), 4, 0);
    let not_started = activity(false, Some(false), 4, 0);
    let cases: [(&str, &EngineActivity, Option<u64>, SettleVerdict); 12] = [
        ("lock held", &held, None, SettleVerdict::Busy),
        ("queue outstanding", &queued, None, SettleVerdict::Busy),
        ("busy beats awaiting", &held, Some(4), SettleVerdict::Busy),
        (
            "queued beats awaiting",
            &queued,
            Some(9),
            SettleVerdict::Busy,
        ),
        (
            "since the current generation",
            &idle,
            Some(4),
            SettleVerdict::AwaitingGeneration,
        ),
        (
            "since a generation not yet issued",
            &idle,
            Some(7),
            SettleVerdict::AwaitingGeneration,
        ),
        (
            "awaiting beats not started",
            &not_started,
            Some(4),
            SettleVerdict::AwaitingGeneration,
        ),
        ("not started", &not_started, None, SettleVerdict::NotStarted),
        (
            "not started with the generation reached",
            &not_started,
            Some(3),
            SettleVerdict::NotStarted,
        ),
        (
            "idle",
            &idle,
            None,
            SettleVerdict::Settled { generation: 4 },
        ),
        (
            "a newer generation settled",
            &idle,
            Some(3),
            SettleVerdict::Settled { generation: 4 },
        ),
        (
            "without since_generation nothing is awaited",
            &activity(false, Some(true), 0, 0),
            None,
            SettleVerdict::Settled { generation: 0 },
        ),
    ];
    for (case, activity, since, expected) in cases {
        assert_eq!(
            verdict(activity, since),
            expected,
            "{case}: {activity:?} since {since:?}"
        );
    }
}

// ── wire shape ───────────────────────────────────────────────────────────────

#[test]
fn an_activity_serializes_to_the_flat_engine_status_object() {
    let json = serde_json::to_value(activity(true, None, 7, 2)).expect("serializes");

    assert_eq!(
        json,
        serde_json::json!({
            "busy": true,
            "engine_lock_held": true,
            "engine_started": null,
            "generation": 7,
            "queue_outstanding": 2,
        })
    );
}

// ── SettleRequest: wait_for_idle's params ────────────────────────────────────

const TIMEOUT_REFUSAL: &str = "timeout_ms must be a positive integer";
const GENERATION_REFUSAL: &str = "since_generation must be a non-negative integer";

fn settle_request(params: serde_json::Value) -> Result<SettleRequest, String> {
    SettleRequest::from_params(&params)
}

#[test]
fn absent_params_wait_thirty_seconds_for_whatever_is_in_hand() {
    let request = settle_request(serde_json::json!({})).expect("empty params are valid");

    assert_eq!(request.timeout, Duration::from_millis(30_000));
    assert_eq!(request.since_generation, None);
}

#[test]
fn given_params_set_the_timeout_and_the_generation_waited_past() {
    let request = settle_request(serde_json::json!({"timeout_ms": 250, "since_generation": 7}))
        .expect("valid params");

    assert_eq!(request.timeout, Duration::from_millis(250));
    assert_eq!(request.since_generation, Some(7));
    assert_eq!(
        settle_request(serde_json::json!({"since_generation": 0}))
            .expect("generation 0 is valid")
            .since_generation,
        Some(0)
    );
}

#[test]
fn a_timeout_that_is_not_a_positive_integer_is_refused_with_the_in_band_message() {
    for bad in [
        serde_json::json!(0),
        serde_json::json!(-5),
        serde_json::json!(1.5),
        serde_json::json!("100"),
    ] {
        assert_eq!(
            settle_request(serde_json::json!({"timeout_ms": bad}))
                .err()
                .as_deref(),
            Some(TIMEOUT_REFUSAL),
            "timeout_ms {bad}"
        );
    }
}

#[test]
fn a_generation_that_is_not_a_non_negative_integer_is_refused() {
    for bad in [
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!("3"),
    ] {
        assert_eq!(
            settle_request(serde_json::json!({"since_generation": bad}))
                .err()
                .as_deref(),
            Some(GENERATION_REFUSAL),
            "since_generation {bad}"
        );
    }
}

// ── wait_until_settled: an async wait that never blocks on the engine lock ───

fn waiting(timeout: Duration, since_generation: Option<u64>) -> SettleRequest {
    SettleRequest {
        timeout,
        since_generation,
    }
}

/// Far beyond every request timeout below: only a wait that ignored its own
/// deadline trips it.
const GUARD: Duration = Duration::from_secs(10);

#[tokio::test]
async fn an_idle_loaded_engine_settles_at_the_current_generation() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();

    let outcome = wait_until_settled(&engine, &queue.queue, &waiting(GUARD, None)).await;

    assert!(
        matches!(outcome, WaitOutcome::Settled { generation: 0, .. }),
        "got {outcome:?}"
    );
}

/// A wait that took the engine lock with `lock()` would park until the holder
/// releases, which it does only after the wait has been asserted.
#[tokio::test]
async fn a_lock_held_throughout_times_out_reporting_the_held_lock() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();
    let holder = LockHolder::hold(&engine);

    let guarded = tokio::time::timeout(
        GUARD,
        wait_until_settled(
            &engine,
            &queue.queue,
            &waiting(Duration::from_millis(100), None),
        ),
    )
    .await;
    holder.release();

    let outcome = guarded.expect("the wait must honour its own 100 ms deadline");
    assert!(
        matches!(
            outcome,
            WaitOutcome::TimedOut {
                last: EngineActivity {
                    engine_lock_held: true,
                    ..
                },
                awaiting_generation: false,
            }
        ),
        "got {outcome:?}"
    );
}

#[tokio::test]
async fn a_lock_released_during_the_wait_settles_it() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();
    let holder = LockHolder::hold(&engine);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        holder.release();
    });

    let outcome = wait_until_settled(&engine, &queue.queue, &waiting(GUARD, None)).await;
    releaser
        .join()
        .expect("the releasing thread must not panic");

    assert!(
        matches!(outcome, WaitOutcome::Settled { generation: 0, .. }),
        "got {outcome:?}"
    );
}

#[tokio::test]
async fn waiting_past_the_current_generation_on_an_idle_queue_times_out_awaiting_it() {
    let engine = make_test_engine();
    let queue = ManualQueue::new();
    let current = queue.queue.progress().generation;

    let outcome = wait_until_settled(
        &engine,
        &queue.queue,
        &waiting(Duration::from_millis(100), Some(current)),
    )
    .await;

    assert!(
        matches!(
            outcome,
            WaitOutcome::TimedOut {
                awaiting_generation: true,
                ..
            }
        ),
        "got {outcome:?}"
    );
}

#[tokio::test]
async fn an_evaluation_submitted_after_the_wait_began_settles_it_at_its_generation() {
    let engine = make_test_engine();
    let ManualQueue {
        queue, executor, ..
    } = ManualQueue::new();
    let request = waiting(GUARD, Some(0));
    let wait = wait_until_settled(&engine, &queue, &request);
    tokio::pin!(wait);
    let short = Duration::from_millis(100);

    assert!(
        tokio::time::timeout(short, &mut wait).await.is_err(),
        "nothing newer than generation 0 has been issued yet"
    );
    let _ticket = queue.submit(EvalRequest::evaluation(|| EvalOutcome::succeeded(None, ())));
    assert!(
        tokio::time::timeout(short, &mut wait).await.is_err(),
        "a queued but unrun evaluation must keep the wait pending"
    );
    executor.run_pending();
    let outcome = tokio::time::timeout(GUARD, wait)
        .await
        .expect("the wait must settle once the evaluation has run");

    assert!(
        matches!(outcome, WaitOutcome::Settled { generation: 1, .. }),
        "got {outcome:?}"
    );
}

#[tokio::test]
async fn a_never_loaded_session_is_reported_not_started_rather_than_timed_out() {
    let engine = never_loaded_engine();
    let queue = ManualQueue::new();

    let outcome = wait_until_settled(&engine, &queue.queue, &waiting(GUARD, None)).await;

    assert!(
        matches!(outcome, WaitOutcome::NotStarted),
        "got {outcome:?}"
    );
}

// ── Replies: what wait_for_idle and health say ───────────────────────────────

#[test]
fn a_not_started_wait_replies_the_engine_not_started_token() {
    assert_eq!(
        WaitOutcome::NotStarted.settled_or_early_reply(),
        Err(serde_json::json!({"error": "engine_not_started"}))
    );
}

#[test]
fn a_timed_out_wait_replies_the_timeout_token_with_its_last_reading() {
    let busy = WaitOutcome::TimedOut {
        last: activity(true, None, 5, 0),
        awaiting_generation: false,
    };
    let awaiting = WaitOutcome::TimedOut {
        last: activity(false, Some(true), 3, 0),
        awaiting_generation: true,
    };

    assert_eq!(
        busy.settled_or_early_reply(),
        Err(serde_json::json!({
            "error": "timeout",
            "engine_busy": true,
            "generation": 5,
            "awaiting_generation": false,
        }))
    );
    assert_eq!(
        awaiting.settled_or_early_reply(),
        Err(serde_json::json!({
            "error": "timeout",
            "engine_busy": false,
            "generation": 3,
            "awaiting_generation": true,
        }))
    );
}

#[test]
fn a_settled_wait_yields_its_generation_and_wait_so_the_frontend_is_asked_next() {
    let settled = WaitOutcome::Settled {
        generation: 2,
        waited: Duration::from_millis(40),
    };

    assert_eq!(
        settled.settled_or_early_reply(),
        Ok((2, Duration::from_millis(40)))
    );
}

#[test]
fn stamping_adds_the_generation_to_an_object_reply_and_keeps_its_keys() {
    assert_eq!(
        stamp_generation(serde_json::json!({"ok": true, "idle_after_ms": 12}), 4),
        serde_json::json!({"ok": true, "idle_after_ms": 12, "generation": 4})
    );
    assert_eq!(
        stamp_generation(serde_json::json!("not an object"), 4),
        serde_json::json!("not an object")
    );
}

#[test]
fn health_replies_ok_and_whether_the_engine_is_busy() {
    assert_eq!(
        activity(false, Some(true), 0, 0).health_reply(),
        serde_json::json!({"ok": true, "engine_busy": false})
    );
    assert_eq!(
        activity(true, None, 0, 0).health_reply(),
        serde_json::json!({"ok": true, "engine_busy": true})
    );
    assert_eq!(
        activity(false, Some(true), 1, 1).health_reply(),
        serde_json::json!({"ok": true, "engine_busy": true})
    );
}
