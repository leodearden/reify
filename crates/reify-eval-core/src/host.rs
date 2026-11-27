//! The core as a host drives it: snapshots in, requests opened and ended,
//! events out. One thread drives it at a time, through `&mut self`.

use crate::budget::Budget;
use crate::counters::Counters;
use crate::demand::Demand;
use crate::event::Event;
use crate::executor::{Executor, Turn};
use crate::input::{Input, InputKey};
use crate::requests::Request;
use crate::snapshot::Snapshot;
use crate::store::{Store, StoredResult};

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
    /// If another core made `snapshot`, or if a limit setting of `snapshot`
    /// is not a u64.
    pub fn evaluate(&mut self, snapshot: &Snapshot, demand: Demand, budget: Budget) -> Request {
        self.executor.evaluate(snapshot, demand, budget)
    }

    /// Ends `request`, dropping every node in progress that no live request
    /// still waits on, directly or through what it reads; a running section
    /// such a body started is told through `cancelled`. Ending a request
    /// the core has already ended does nothing.
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
    /// section is running either, every ring of waits is answered "cycle"
    /// (contract 7.7) and what that makes ready is run, until no body is
    /// ready and no ring is left.
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

    /// The core's results, as a store another core can be built on
    /// (`Builder::on_store`); the core is gone. A result that holds a thing
    /// inside a resource stays good only beside this core's resource, which
    /// goes with it.
    pub fn into_store(self) -> Store {
        self.executor.into_store()
    }

    /// A new store holding each result `keep` accepts, with what leads to
    /// it, sharing those results with this core, which is left as it was.
    /// `keep` sees every stored result once.
    pub fn selected_store(&self, keep: impl FnMut(&StoredResult) -> bool) -> Store {
        self.executor.selected_store(keep)
    }
}
