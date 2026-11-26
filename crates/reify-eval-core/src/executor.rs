//! The executor: works out the paths a request demands and runs every body
//! that is ready, on the thread that calls it. What one answer sets off is
//! worked through a queue, never by recursion.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::Poll;

use crate::answer::Answer;
use crate::core_id::CoreId;
use crate::counters::Counters;
use crate::declaration::Declaration;
use crate::demand::Demand;
use crate::event::{Event, RequestId};
use crate::failure::{Failure, OutsideFailure, panic_message};
use crate::finish;
use crate::index::{Entry, Indexes};
use crate::input::{Input, InputKey};
use crate::job::Report;
use crate::kinds::Kinds;
use crate::mailbox::{Ask, Posted};
use crate::outcome::{Erased, Outcome};
use crate::path::Path;
use crate::path_table::{PathId, PathTable};
use crate::produced::Produced;
use crate::ready::ReadyQueue;
use crate::requests::{Request, Requests};
use crate::resolve::declaration_of;
use crate::resource::Resources;
use crate::serve;
use crate::serve::section::{Delivery, InFlight};
use crate::snapshot::{Snapshot, SnapshotId};
use crate::task::{Task, TaskKey};
use crate::waits::Waits;

/// What is left after a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Idle,
    SectionsRunning(usize),
}

/// What is left to do after a node's answer, worked in order.
enum Work {
    /// The node at the key has its final answer.
    Answered(TaskKey, Entry),
    /// The path at the key can now be worked out: it is a root, or its
    /// parent has its final answer.
    Resolve(TaskKey),
}

pub(crate) struct Executor {
    core: CoreId,
    snapshots: u64,
    kinds: Kinds,
    paths: PathTable,
    indexes: Indexes,
    tasks: BTreeMap<TaskKey, Task>,
    /// The paths waiting on their parent's answer.
    resolving: BTreeSet<TaskKey>,
    waits: Waits,
    ready: ReadyQueue<TaskKey>,
    requests: Requests,
    counters: Counters,
    events: Vec<Event>,
    in_flight: InFlight,
    resources: Resources,
}

impl Executor {
    pub fn new(kinds: Kinds) -> Self {
        let core = CoreId::fresh();
        Self {
            core,
            snapshots: 0,
            kinds,
            paths: PathTable::default(),
            indexes: Indexes::default(),
            tasks: BTreeMap::new(),
            resolving: BTreeSet::new(),
            waits: Waits::default(),
            ready: ReadyQueue::default(),
            requests: Requests::new(core),
            counters: Counters::default(),
            events: Vec::new(),
            in_flight: InFlight::default(),
            resources: Resources::new(),
        }
    }

    pub fn add_resource<R: 'static>(&mut self, make: impl FnOnce() -> R + Send + 'static) {
        self.resources.add(make);
    }

    pub fn snapshot(&mut self, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Snapshot {
        self.snapshots += 1;
        let id = SnapshotId {
            core: self.core,
            serial: self.snapshots,
        };
        Snapshot::new(id, inputs)
    }

    pub fn evaluate(&mut self, snapshot: &Snapshot, demand: Demand) -> Request {
        self.core.refuse_foreign(snapshot.id().core, "a snapshot");
        self.prune();
        self.indexes.open(snapshot);
        let request = self.requests.open(snapshot.id());
        for path in demand.into_paths() {
            self.demand(request.id(), snapshot.id(), &path);
        }
        request
    }

    /// Ends `request` and drops the work no other live request wants.
    /// A section such a body started keeps running; what it makes is
    /// released when it is delivered.
    pub fn end(&mut self, request: Request) {
        self.core.refuse_foreign(request.id().core, "a request");
        for key in self.requests.end(request) {
            if let Some(task) = self.tasks.remove(&key) {
                task.abandon();
                self.waits.forget(key);
            }
            if self.resolving.remove(&key) {
                self.waits.forget(key);
            }
        }
        self.prune();
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    /// Takes in what resource threads have reported, without blocking, then
    /// runs every ready body until none is ready. A finished section is
    /// delivered only by `wait` or `drain`, so what a turn does to bodies
    /// depends on the order of the host's calls alone.
    pub fn turn(&mut self) -> Turn {
        self.prune();
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

    /// Drops the index of each snapshot that is no longer live: no clone of
    /// it is held and no live request is on it (contract 4.4).
    fn prune(&mut self) {
        let requests = &self.requests;
        self.indexes.prune(|snapshot| requests.any_on(snapshot));
    }

    /// Tells a path already answered in the snapshot at once; otherwise
    /// the request wants it until it is answered.
    fn demand(&mut self, request: RequestId, snapshot: SnapshotId, path: &Path) {
        let key = TaskKey {
            snapshot,
            path: self.paths.intern(path),
        };
        if let Some(entry) = self.indexes.of(snapshot).entry(key.path) {
            let answer = Answer::Final(entry.outcome.clone());
            return self.tell(request, key, answer);
        }
        self.requests.want(request, key);
        self.ensure(key);
    }

    fn in_progress(&self, key: TaskKey) -> bool {
        self.tasks.contains_key(&key) || self.resolving.contains(&key)
    }

    fn answered(&self, key: TaskKey) -> bool {
        self.indexes.of(key.snapshot).entry(key.path).is_some()
    }

    /// Sees that `key` will be answered: walks up to the nearest ancestor
    /// that is answered or in progress, each path on the way waiting on its
    /// parent, and works out the topmost path that can be worked out now.
    fn ensure(&mut self, key: TaskKey) {
        if self.answered(key) || self.in_progress(key) {
            return;
        }
        let mut at = key;
        while let Some((parent, _)) = self.paths.parent(at.path) {
            let parent = TaskKey {
                snapshot: key.snapshot,
                path: parent,
            };
            if self.answered(parent) {
                break;
            }
            self.resolving.insert(at);
            self.waits.add(at, parent);
            if self.in_progress(parent) {
                return;
            }
            at = parent;
        }
        self.work(Work::Resolve(at));
    }

    /// Works through what `first` sets off, in order.
    fn work(&mut self, first: Work) {
        let mut queue = VecDeque::from([first]);
        while let Some(work) = queue.pop_front() {
            match work {
                Work::Answered(key, entry) => {
                    let answer = Answer::Final(entry.outcome.clone());
                    let answered = finish::answered(
                        key,
                        entry,
                        &mut self.indexes,
                        &mut self.waits,
                        &mut self.requests,
                        &mut self.resolving,
                    );
                    for request in answered.tell {
                        self.tell(request, key, answer.clone());
                    }
                    queue.extend(answered.resolve.into_iter().map(Work::Resolve));
                    for reader in answered.resume {
                        self.resume_reader(reader);
                    }
                }
                Work::Resolve(key) => queue.extend(self.resolve(key)),
            }
        }
    }

    /// Works out what the path at `key` is: a root from its snapshot input,
    /// read with the typed read a body uses (decision D11); any other path
    /// from its parent's final answer. A declaration starts the body; any
    /// other outcome is the path's answer.
    fn resolve(&mut self, key: TaskKey) -> Option<Work> {
        self.counters.count_path_resolved();
        let index = self.indexes.of(key.snapshot);
        let declared = match self.paths.parent(key.path) {
            None => {
                let root = self
                    .paths
                    .as_root(key.path)
                    .expect("a path with no parent is a root");
                index
                    .input(&InputKey::Root(root.clone()))
                    .map_or(Outcome::Absent, Input::outcome)
                    .downcast::<Declaration>()
            }
            Some((parent, step)) => {
                let entry = index
                    .entry(parent)
                    .expect("a path is worked out once its parent is answered");
                declaration_of(step, entry, &self.kinds)
            }
        };
        match declared {
            Outcome::Value(declaration) => {
                self.start(key, declaration);
                None
            }
            settled => Some(Work::Answered(
                key,
                Entry {
                    outcome: settled.erase(),
                    declaration: None,
                },
            )),
        }
    }

    fn start(&mut self, key: TaskKey, declaration: Arc<Declaration>) {
        let kind_id = declaration.kind();
        let Some(kind) = self.kinds.get(kind_id) else {
            let unknown = OutsideFailure::UnknownKind { kind: kind_id.0 };
            return self.end_outside(key, unknown);
        };
        self.counters.count_body_run(kind_id);
        self.tasks.insert(key, Task::start(kind, declaration));
        self.ready.push(key);
    }

    fn advance(&mut self, key: TaskKey) {
        let Some(task) = self
            .tasks
            .get_mut(&key)
            .filter(|task| !task.waits_on_a_section())
        else {
            return;
        };
        let posted = match catch_unwind(AssertUnwindSafe(|| task.poll())) {
            Ok(Poll::Ready(Produced::Outcome(outcome))) => return self.finish(key, outcome),
            Ok(Poll::Ready(Produced::Outside(failure))) => return self.end_outside(key, failure),
            Ok(Poll::Pending) => task.take_posted(),
            Err(payload) => {
                let message = panic_message(&*payload);
                return self.end_outside(key, OutsideFailure::BodyPanicked { message });
            }
        };
        match posted {
            Posted::Ask(Ask::Read(refs)) => {
                let index = self.indexes.of(key.snapshot);
                let reading =
                    serve::read::serve(key.path, task.declaration(), &refs, &mut self.paths, index);
                let awaited = reading.awaited();
                if awaited.is_empty() {
                    task.answer_read(reading.reply(index));
                    return self.ready.push(key);
                }
                task.wait_on(reading);
                self.await_nodes(key, awaited);
            }
            Posted::Ask(Ask::Exclusive(ask)) => {
                let cancel = task.cancel_flag();
                match self.in_flight.start(&key, ask, cancel, &mut self.resources) {
                    Ok(serial) => {
                        task.begin_section(serial);
                        self.counters.count_section();
                    }
                    Err(failure) => self.end_outside(key, failure),
                }
            }
            Posted::TwoRequestsAtOnce => {
                self.finish(key, Outcome::Failure(Failure::TwoRequestsAtOnce))
            }
            Posted::Nothing => self.end_outside(key, OutsideFailure::WaitedOutsideCore),
        }
    }

    /// Makes the reader at `key` wait on each node in `awaited` before any
    /// of them is worked out, so that none can answer the reading early.
    fn await_nodes(&mut self, key: TaskKey, awaited: BTreeSet<PathId>) {
        let nodes: Vec<_> = awaited
            .into_iter()
            .map(|path| TaskKey {
                snapshot: key.snapshot,
                path,
            })
            .collect();
        for node in &nodes {
            self.waits.add(key, *node);
        }
        for node in nodes {
            self.ensure(node);
        }
    }

    /// Replies to the body at `key`, whose reading awaits nothing more, and
    /// queues it.
    fn resume_reader(&mut self, key: TaskKey) {
        let Some(task) = self.tasks.get_mut(&key) else {
            return;
        };
        if let Some(reading) = task.take_reading() {
            task.answer_read(reading.reply(self.indexes.of(key.snapshot)));
            self.ready.push(key);
        }
    }

    /// Ends a node whose body produced `outcome`.
    fn finish(&mut self, key: TaskKey, outcome: Outcome<Erased>) {
        let declaration = self.tasks.remove(&key).map(|task| {
            let declaration = Arc::clone(task.declaration());
            task.discard();
            declaration
        });
        self.work(Work::Answered(
            key,
            Entry {
                outcome,
                declaration,
            },
        ));
    }

    /// Ends a node with a failure outside the inputs: pending to every live
    /// request that wanted it, and stored nowhere.
    fn end_outside(&mut self, key: TaskKey, failure: OutsideFailure) {
        if let Some(task) = self.tasks.remove(&key) {
            task.discard();
        }
        for request in self.requests.settle(&key) {
            self.tell(request, key, Answer::Pending(failure.clone()));
        }
    }

    fn tell(&mut self, request: RequestId, key: TaskKey, answer: Answer<Erased>) {
        self.events.push(Event::Answered {
            request,
            snapshot: key.snapshot,
            path: self.paths.path(key.path),
            answer,
        });
    }

    /// Delivers the earliest finished section, blocking until one has
    /// finished; `false` if no section is running.
    fn deliver_one(&mut self) -> bool {
        if self.in_flight.running() == 0 {
            return false;
        }
        loop {
            if let Some(delivery) = self.in_flight.deliver(&mut self.resources) {
                self.hand_over(delivery);
                return true;
            }
            let report = self.resources.receive();
            self.take_in(report);
        }
    }

    /// Gives a finished section's product to the task still waiting on it,
    /// or ends that task's node with the section's failure. With no task
    /// waiting, the product is dropped here, which releases what it holds.
    fn hand_over(&mut self, delivery: Delivery) {
        let Delivery {
            key,
            serial,
            product,
        } = delivery;
        let Some(task) = self
            .tasks
            .get_mut(&key)
            .filter(|task| task.waits_on(serial))
        else {
            return;
        };
        match product {
            Ok(product) => {
                task.resume(product);
                self.ready.push(key);
            }
            Err(failure) => self.end_outside(key, failure),
        }
    }

    /// The task still waiting on section `serial`: the one that started it
    /// may have been dropped, and another started in its place.
    fn waiting_on(&self, serial: u64) -> Option<TaskKey> {
        let key = self.in_flight.starter(serial)?;
        let task = self.tasks.get(key)?;
        task.waits_on(serial).then_some(*key)
    }

    fn take_in(&mut self, report: Report) {
        match report {
            Report::Progress { serial, note } => {
                let Some(key) = self.waiting_on(serial) else {
                    return;
                };
                let path = self.paths.path(key.path);
                for request in self.requests.wanting(&key) {
                    let note = Arc::clone(&note);
                    self.events.push(Event::Progress {
                        request,
                        snapshot: key.snapshot,
                        path: path.clone(),
                        note,
                    });
                }
            }
            Report::Finished { serial, product } => self.in_flight.arrived(serial, product),
            Report::Released => self.resources.acknowledge_release(),
        }
    }
}

impl Drop for Executor {
    /// Bodies, indexes and untaken answers go before the resources, so what
    /// they hold is queued for release before each resource thread stops.
    fn drop(&mut self) {
        for (_, task) in mem::take(&mut self.tasks) {
            task.discard();
        }
        self.indexes.clear();
        self.events.clear();
    }
}
