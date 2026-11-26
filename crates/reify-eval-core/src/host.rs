//! The core as a host drives it: snapshots in, requests opened and ended,
//! events out. One thread drives it at a time, through `&mut self`.

use crate::counters::Counters;
use crate::demand::Demand;
use crate::event::Event;
use crate::executor::{Executor, Turn};
use crate::input::{Input, InputKey};
use crate::requests::Request;
use crate::snapshot::Snapshot;

pub struct Core {
    executor: Executor,
}

impl Core {
    pub(crate) fn new(executor: Executor) -> Self {
        Self { executor }
    }

    pub fn snapshot(&mut self, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Snapshot {
        self.executor.snapshot(inputs)
    }

    /// Opens a request on `snapshot` for the answers at the paths `demand`
    /// names.
    ///
    /// # Panics
    ///
    /// If another core made `snapshot`.
    pub fn evaluate(&mut self, snapshot: &Snapshot, demand: Demand) -> Request {
        self.executor.evaluate(snapshot, demand)
    }

    /// Ends `request`, dropping the work it wanted that no other live
    /// request wants and nothing awaits. Ending a request the core has
    /// already ended does nothing.
    ///
    /// # Panics
    ///
    /// If another core opened `request`.
    pub fn end(&mut self, request: Request) {
        self.executor.end(request);
    }

    /// The answers, progress notes and request endings given since the last
    /// call, in the order they were given.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.executor.take_events()
    }

    /// Runs every body that is ready until none is. Never blocks. If then no
    /// section is running either, each live request that still wants a path
    /// is ended with `RequestError::NothingCanRun`.
    pub fn turn(&mut self) -> Turn {
        self.executor.turn()
    }

    /// Blocks until one running section is delivered to its body, then
    /// takes a turn.
    pub fn wait(&mut self) -> Turn {
        self.executor.wait()
    }

    /// Turns and waits until no section is running.
    pub fn run(&mut self) {
        self.executor.run();
    }

    /// Runs until no section is running, nothing a section produced is
    /// waiting to be delivered, and no release is waiting to run.
    pub fn drain(&mut self) {
        self.executor.drain();
    }

    /// Adds resource `R` on a thread of its own; `make` runs there, so `R`
    /// need not be `Send`.
    ///
    /// # Panics
    ///
    /// If the core already has a resource of type `R`.
    pub fn add_resource<R: 'static>(&mut self, make: impl FnOnce() -> R + Send + 'static) {
        self.executor.add_resource(make);
    }

    pub fn counters(&self) -> &Counters {
        self.executor.counters()
    }
}
