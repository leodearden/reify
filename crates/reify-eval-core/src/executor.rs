//! The executor: resolves the roots a request demands and runs what is
//! ready, on the thread that calls it.

use std::collections::BTreeSet;
use std::mem;

use crate::answer::Answer;
use crate::counters::Counters;
use crate::declaration::Declaration;
use crate::event::{Event, RequestId};
use crate::failure::OutsideFailure;
use crate::input::{Input, InputKey};
use crate::outcome::{Erased, Outcome};
use crate::path::{Path, Root};
use crate::requests::{Request, Requests};
use crate::snapshot::Snapshot;

/// What is left after a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Idle,
    SectionsRunning(usize),
}

#[derive(Default)]
pub(crate) struct Executor {
    requests: Requests,
    counters: Counters,
    events: Vec<Event>,
}

impl Executor {
    pub fn evaluate(&mut self, snapshot: &Snapshot, roots: impl IntoIterator<Item = Root>) -> Request {
        let request = self.requests.open(snapshot.clone());
        for root in roots.into_iter().collect::<BTreeSet<_>>() {
            self.demand_root(request.id(), snapshot, root);
        }
        request
    }

    pub fn end(&mut self, request: Request) {
        self.requests.end(request);
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    pub fn turn(&mut self) -> Turn {
        Turn::Idle
    }

    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// Reads the root's declaration from the snapshot with the typed read a
    /// body uses (decision D11).
    fn demand_root(&mut self, request: RequestId, snapshot: &Snapshot, root: Root) {
        let declared = snapshot
            .input(&InputKey::Root(root.clone()))
            .map_or(Outcome::Absent, Input::outcome)
            .downcast::<Declaration>();
        let path = Path::root(root);
        match declared {
            Outcome::Value(declaration) => {
                let unknown = OutsideFailure::UnknownKind { kind: declaration.kind().0 };
                self.tell(request, path, Answer::Pending(unknown));
            }
            settled => self.tell(request, path, Answer::Final(settled.erase())),
        }
    }

    fn tell(&mut self, request: RequestId, path: Path, answer: Answer<Erased>) {
        self.events.push(Event::Answered { request, path, answer });
    }
}
