//! Tests for [`crate::engine_activity`].

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::MockGeometryKernel;

use crate::engine::EngineSession;
use crate::engine_activity::{EngineActivity, SettleVerdict, probe, verdict};
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
