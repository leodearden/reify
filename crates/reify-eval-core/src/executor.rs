//! The executor: resolves the roots a request demands and runs every body
//! that is ready, on the thread that calls it.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::Poll;

use crate::answer::Answer;
use crate::counters::Counters;
use crate::declaration::Declaration;
use crate::event::{Event, RequestId};
use crate::failure::{Failure, OutsideFailure, panic_message};
use crate::input::{Input, InputKey};
use crate::job::Report;
use crate::kinds::Kinds;
use crate::mailbox::{Ask, Posted, Reply};
use crate::outcome::{Erased, Outcome};
use crate::path::{Path, Root};
use crate::produced::Produced;
use crate::ready::ReadyQueue;
use crate::requests::{Request, Requests};
use crate::resource::Resources;
use crate::serve;
use crate::serve::section::InFlight;
use crate::snapshot::Snapshot;
use crate::task::{Task, TaskKey};

/// What is left after a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Idle,
    SectionsRunning(usize),
}

pub(crate) struct Executor {
    kinds: Kinds,
    tasks: BTreeMap<TaskKey, Task>,
    ready: ReadyQueue<TaskKey>,
    requests: Requests,
    counters: Counters,
    events: Vec<Event>,
    in_flight: InFlight,
    resources: Resources,
}

impl Executor {
    pub fn new(kinds: Kinds) -> Self {
        Self {
            kinds,
            tasks: BTreeMap::new(),
            ready: ReadyQueue::default(),
            requests: Requests::default(),
            counters: Counters::default(),
            events: Vec::new(),
            in_flight: InFlight::default(),
            resources: Resources::new(),
        }
    }

    pub fn add_resource<R: 'static>(&mut self, make: impl FnOnce() -> R + Send + 'static) {
        self.resources.add(make);
    }

    pub fn evaluate(&mut self, snapshot: &Snapshot, roots: impl IntoIterator<Item = Root>) -> Request {
        let request = self.requests.open();
        for root in roots.into_iter().collect::<BTreeSet<_>>() {
            self.demand_root(request.id(), snapshot, root);
        }
        request
    }

    /// Ends `request` and drops the bodies no other live request wants.
    /// A section such a body started keeps running; what it makes is
    /// released when it is delivered.
    pub fn end(&mut self, request: Request) {
        for key in self.requests.end(request) {
            if let Some(task) = self.tasks.remove(&key) {
                task.cancel.set();
                task.discard();
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    /// Takes in what resource threads have reported, without blocking, then
    /// runs every ready body until none is ready. A finished section is
    /// delivered only by `wait` or `drain`, so what a turn does to bodies
    /// depends on the order of the host's calls alone.
    pub fn turn(&mut self) -> Turn {
        while let Some(report) = self.resources.try_receive() {
            self.take_in(report);
        }
        while let Some(key) = self.ready.pop() {
            self.advance(key);
        }
        match self.in_flight.running() {
            0 => Turn::Idle,
            running => Turn::SectionsRunning(running),
        }
    }

    /// Blocks until one running section is delivered, then takes a turn.
    pub fn wait(&mut self) -> Turn {
        self.deliver_one();
        self.turn()
    }

    pub fn run(&mut self) {
        let mut turn = self.turn();
        while turn != Turn::Idle {
            turn = self.wait();
        }
    }

    /// Runs until no section is running, nothing a section produced is
    /// waiting to be delivered, and no release is waiting to run.
    pub fn drain(&mut self) {
        self.turn();
        loop {
            if self.deliver_one() {
                self.turn();
            } else if self.resources.pending_releases() > 0 {
                let report = self.resources.receive();
                self.take_in(report);
            } else {
                return;
            }
        }
    }

    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// Reads the root's declaration from the snapshot with the typed read a
    /// body uses (decision D11), then joins or starts its body.
    fn demand_root(&mut self, request: RequestId, snapshot: &Snapshot, root: Root) {
        let key = (snapshot.id(), Path::root(root.clone()));
        if self.tasks.contains_key(&key) {
            return self.requests.want(request, key);
        }
        let declared = snapshot
            .input(&InputKey::Root(root))
            .map_or(Outcome::Absent, Input::outcome)
            .downcast::<Declaration>();
        match declared {
            Outcome::Value(declaration) => self.start(request, key, declaration, snapshot),
            settled => self.tell(request, key.1, Answer::Final(settled.erase())),
        }
    }

    fn start(&mut self, request: RequestId, key: TaskKey, declaration: Arc<Declaration>, snapshot: &Snapshot) {
        let kind_id = declaration.kind();
        let Some(kind) = self.kinds.get(kind_id) else {
            let unknown = OutsideFailure::UnknownKind { kind: kind_id.0 };
            return self.tell(request, key.1, Answer::Pending(unknown));
        };
        self.counters.count_body_run(kind_id);
        self.tasks.insert(key.clone(), Task::start(kind, declaration, snapshot.clone()));
        self.requests.want(request, key.clone());
        self.ready.push(key);
    }

    fn advance(&mut self, key: TaskKey) {
        let Some(task) = self.tasks.get_mut(&key).filter(|task| task.section.is_none()) else { return };
        let posted = match catch_unwind(AssertUnwindSafe(|| task.poll())) {
            Ok(Poll::Ready(Produced::Outcome(outcome))) => return self.finish(&key, outcome),
            Ok(Poll::Ready(Produced::Outside(failure))) => return self.end_outside(&key, failure),
            Ok(Poll::Pending) => task.mailbox.take_posted(),
            Err(payload) => {
                let message = panic_message(&*payload);
                return self.end_outside(&key, OutsideFailure::BodyPanicked { message });
            }
        };
        match posted {
            Posted::Ask(Ask::Read(r)) => {
                let outcome = serve::read::answer(&task.declaration, &task.snapshot, &r);
                task.mailbox.reply(Reply::Outcome(outcome));
                self.ready.push(key);
            }
            Posted::Ask(Ask::Exclusive { resource, resource_name, job }) => {
                let started = serve::section::start(
                    task,
                    &key,
                    (resource, resource_name),
                    job,
                    &mut self.resources,
                    &mut self.in_flight,
                    &mut self.counters,
                );
                if let Err(failure) = started {
                    self.end_outside(&key, failure);
                }
            }
            Posted::TwoRequestsAtOnce => {
                self.finish(&key, Outcome::Failure(Failure::TwoRequestsAtOnce))
            }
            Posted::Nothing => self.end_outside(&key, OutsideFailure::WaitedOutsideCore),
        }
    }

    /// Ends a node with its outcome, told to every live request that wanted it.
    fn finish(&mut self, key: &TaskKey, outcome: Outcome<Erased>) {
        self.tasks.remove(key);
        for request in self.requests.settle(key) {
            self.tell(request, key.1.clone(), Answer::Final(outcome.clone()));
        }
    }

    /// Ends a node with a failure outside the inputs: pending to every live
    /// request that wanted it, and stored nowhere.
    fn end_outside(&mut self, key: &TaskKey, failure: OutsideFailure) {
        if let Some(task) = self.tasks.remove(key) {
            task.discard();
        }
        for request in self.requests.settle(key) {
            self.tell(request, key.1.clone(), Answer::Pending(failure.clone()));
        }
    }

    fn tell(&mut self, request: RequestId, path: Path, answer: Answer<Erased>) {
        self.events.push(Event::Answered { request, path, answer });
    }

    /// Delivers the earliest finished section, blocking until one has
    /// finished; `false` if no section is running.
    fn deliver_one(&mut self) -> bool {
        if self.in_flight.running() == 0 {
            return false;
        }
        loop {
            if let Some((serial, product)) = self.in_flight.take_finished() {
                serve::section::deliver(serial, product, &mut self.in_flight, &mut self.tasks, &mut self.ready);
                return true;
            }
            let report = self.resources.receive();
            self.take_in(report);
        }
    }

    fn take_in(&mut self, report: Report) {
        match report {
            Report::Progress { serial, note } => {
                let Some(key) = serve::section::waiting_task(serial, &self.in_flight, &self.tasks) else {
                    return;
                };
                for request in self.requests.wanting(key) {
                    let note = Arc::clone(&note);
                    self.events.push(Event::Progress { request, path: key.1.clone(), note });
                }
            }
            Report::Finished { serial, product } => self.in_flight.arrived(serial, product),
            Report::Released => self.resources.acknowledge_release(),
        }
    }
}

impl Drop for Executor {
    /// Bodies and untaken answers go before the resources, so what they
    /// hold is queued for release before each resource thread stops.
    fn drop(&mut self) {
        for (_, task) in mem::take(&mut self.tasks) {
            task.discard();
        }
        self.events.clear();
    }
}
