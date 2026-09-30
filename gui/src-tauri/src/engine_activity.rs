//! Whether the ENGINE lane is busy, answered without ever waiting for it.
//!
//! This is the one place that question is answered without blocking. Every
//! engine user holds the one engine mutex while it works — the queue's
//! drainer, the debug server's `run_on_engine`, setup — so "the mutex is held"
//! is exact for all of them with no instrumentation at their call sites. The
//! queue's own progress adds the work it has accepted but not yet started,
//! which the mutex cannot see. Waiters poll the same reading through
//! [`wait_until_settled`].

use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};

use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::engine::EngineSession;
use crate::eval_queue::{EvalProgress, EvalQueue};

/// One lock-free reading of the engine lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineActivity {
    /// Another thread holds the engine mutex right now.
    pub engine_lock_held: bool,
    /// Whether a design is loaded and checked; `None` while the lock is held,
    /// since reading it would mean waiting.
    pub engine_started: Option<bool>,
    pub queue: EvalProgress,
}

impl EngineActivity {
    pub fn busy(&self) -> bool {
        self.engine_lock_held || self.queue.outstanding > 0
    }
}

/// The flat wire object `engine_status` replies.
#[derive(Serialize)]
struct EngineActivityWire {
    busy: bool,
    engine_lock_held: bool,
    engine_started: Option<bool>,
    generation: u64,
    queue_outstanding: usize,
}

impl Serialize for EngineActivity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineActivityWire {
            busy: self.busy(),
            engine_lock_held: self.engine_lock_held,
            engine_started: self.engine_started,
            generation: self.queue.generation,
            queue_outstanding: self.queue.outstanding,
        }
        .serialize(serializer)
    }
}

/// Read the engine lane without blocking: `try_lock` never waits, and the
/// queue's progress takes only the queue's own short-lived lock.
///
/// A poisoned mutex reads as free, on the grounds `with_engine_lock` documents
/// for recovering its guard (`crate::engine_lock`).
pub fn probe(engine: &Mutex<EngineSession>, queue: &EvalQueue) -> EngineActivity {
    let engine_started = match engine.try_lock() {
        Ok(guard) => Some(guard.is_idle()),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner().is_idle()),
        Err(TryLockError::WouldBlock) => None,
    };
    EngineActivity {
        engine_lock_held: engine_started.is_none(),
        engine_started,
        queue: queue.progress(),
    }
}

/// Whether a waiter may stop waiting, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleVerdict {
    /// Engine work is running or queued.
    Busy,
    /// Idle, but no generation newer than the one waited past has been issued.
    AwaitingGeneration,
    /// Idle with no design loaded.
    NotStarted,
    /// Idle, with `generation` the newest one issued.
    Settled { generation: u64 },
}

/// Judge `activity` for a waiter who wants work newer than `since_generation`
/// finished, or — without it — whatever is in hand. The first that applies
/// wins: busy, awaiting a generation, not started, settled.
pub fn verdict(activity: &EngineActivity, since_generation: Option<u64>) -> SettleVerdict {
    let generation = activity.queue.generation;
    if activity.busy() {
        SettleVerdict::Busy
    } else if since_generation.is_some_and(|since| generation <= since) {
        SettleVerdict::AwaitingGeneration
    } else if activity.engine_started == Some(false) {
        SettleVerdict::NotStarted
    } else {
        SettleVerdict::Settled { generation }
    }
}

/// How long `wait_for_idle` waits when its caller names no `timeout_ms`.
const DEFAULT_SETTLE_TIMEOUT_MS: u64 = 30_000;

/// How often [`wait_until_settled`] re-reads the lane.
const SETTLE_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// What a waiter asked to wait for, and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleRequest {
    pub timeout: Duration,
    /// Wait for work newer than this generation; `None` waits for whatever is
    /// in hand.
    pub since_generation: Option<u64>,
}

impl SettleRequest {
    /// Read `timeout_ms` and `since_generation` from a tool's params. A
    /// malformed value is refused with the message the tool replies in band.
    pub fn from_params(params: &Value) -> Result<Self, String> {
        let timeout_ms = match params.get("timeout_ms") {
            None => DEFAULT_SETTLE_TIMEOUT_MS,
            Some(value) => value
                .as_u64()
                .filter(|&ms| ms > 0)
                .ok_or("timeout_ms must be a positive integer")?,
        };
        let since_generation = params
            .get("since_generation")
            .map(|value| {
                value
                    .as_u64()
                    .ok_or("since_generation must be a non-negative integer")
            })
            .transpose()?;
        Ok(Self {
            timeout: Duration::from_millis(timeout_ms),
            since_generation,
        })
    }
}

/// How a wait for the engine lane ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// Idle at `generation`, reached after `waited`.
    Settled { generation: u64, waited: Duration },
    /// Idle with no design loaded, so there is nothing to wait for.
    NotStarted,
    /// Still pending at the deadline: `last` is the final reading, and
    /// `awaiting_generation` says the lane was idle but no newer generation had
    /// been issued.
    TimedOut {
        last: EngineActivity,
        awaiting_generation: bool,
    },
}

/// Poll [`probe`] until [`verdict`] lets the waiter go or `request.timeout`
/// passes. Sleeps between readings and never waits on the engine lock, so a
/// long evaluation cannot park the calling runtime.
pub async fn wait_until_settled(
    engine: &Mutex<EngineSession>,
    queue: &EvalQueue,
    request: &SettleRequest,
) -> WaitOutcome {
    let started = Instant::now();
    loop {
        let activity = probe(engine, queue);
        match verdict(&activity, request.since_generation) {
            SettleVerdict::Settled { generation } => {
                return WaitOutcome::Settled {
                    generation,
                    waited: started.elapsed(),
                };
            }
            SettleVerdict::NotStarted => return WaitOutcome::NotStarted,
            pending @ (SettleVerdict::Busy | SettleVerdict::AwaitingGeneration) => {
                if started.elapsed() >= request.timeout {
                    return WaitOutcome::TimedOut {
                        last: activity,
                        awaiting_generation: pending == SettleVerdict::AwaitingGeneration,
                    };
                }
            }
        }
        tokio::time::sleep(SETTLE_POLL_INTERVAL).await;
    }
}
