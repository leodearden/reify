//! Whether the ENGINE lane is busy, answered without ever waiting for it.
//!
//! This is the one place that question is answered without blocking. Every
//! engine user holds the one engine mutex while it works — the queue's
//! drainer, the debug server's `run_on_engine`, setup — so "the mutex is held"
//! is exact for all of them with no instrumentation at their call sites. The
//! queue's own progress adds the work it has accepted but not yet started,
//! which the mutex cannot see.

use std::sync::{Mutex, TryLockError};

use serde::{Serialize, Serializer};

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
