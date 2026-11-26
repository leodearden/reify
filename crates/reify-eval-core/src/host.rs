//! The core as a host drives it: snapshots in, requests opened and ended,
//! events out. One thread drives it at a time, through `&mut self`.

use crate::counters::Counters;
use crate::event::Event;
use crate::executor::{Executor, Turn};
use crate::input::{Input, InputKey};
use crate::path::Root;
use crate::requests::Request;
use crate::snapshot::{Snapshot, SnapshotId};

pub struct Core {
    executor: Executor,
    snapshots: u64,
}

impl Core {
    pub(crate) fn new(executor: Executor) -> Self {
        Self { executor, snapshots: 0 }
    }

    pub fn snapshot(&mut self, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Snapshot {
        self.snapshots += 1;
        Snapshot::new(SnapshotId(self.snapshots), inputs)
    }

    /// Opens a request on `snapshot` for the answers of `roots`.
    pub fn evaluate(&mut self, snapshot: &Snapshot, roots: impl IntoIterator<Item = Root>) -> Request {
        self.executor.evaluate(snapshot, roots)
    }

    pub fn end(&mut self, request: Request) {
        self.executor.end(request);
    }

    /// The answers given since the last call, in the order they were given.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.executor.take_events()
    }

    /// Runs every body that is ready until none is. Never blocks.
    pub fn turn(&mut self) -> Turn {
        self.executor.turn()
    }

    pub fn counters(&self) -> &Counters {
        self.executor.counters()
    }
}
