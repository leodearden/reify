//! The executor: works out the paths a request demands and runs every body
//! that is ready, on the thread that calls it. What one answer sets off is
//! worked through a queue, never by recursion.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::Poll;

use crate::answer::Answer;
use crate::content::Content;
use crate::core_id::CoreId;
use crate::counters::Counters;
use crate::declaration::Declaration;
use crate::demand::Demand;
use crate::event::{Event, RequestError, RequestId};
use crate::failure::{Failure, OutsideFailure, panic_message};
use crate::finish;
use crate::index::{self, Entry, Indexes, NodeKey};
use crate::input::{Input, InputKey};
use crate::job::Report;
use crate::key::Key;
use crate::kinds::Kinds;
use crate::lookup::Lookup;
use crate::mailbox::{Ask, Posted};
use crate::outcome::{Erased, Outcome};
use crate::path::Path;
use crate::path_table::{PathId, PathTable};
use crate::produced::Produced;
use crate::ready::ReadyQueue;
use crate::record::Reading;
use crate::requests::{Request, Requests};
use crate::resolve::declaration_of;
use crate::resource::Resources;
use crate::serve;
use crate::serve::section::{Delivery, InFlight};
use crate::snapshot::{Snapshot, SnapshotId, SnapshotTokens};
use crate::store::{Store, Stored, StoredResult};
use crate::task::Task;
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
    Answered(NodeKey, Entry),
    /// The path at the key can now be worked out: it is a root, or its
    /// parent has its final answer.
    Resolve(NodeKey),
}

pub(crate) struct Executor {
    core: CoreId,
    kinds: Kinds,
    paths: PathTable,
    snapshots: Arc<SnapshotTokens>,
    indexes: Indexes,
    store: Store,
    /// The nodes following the store for a result filed for them.
    lookups: BTreeMap<NodeKey, Lookup>,
    tasks: BTreeMap<NodeKey, Task>,
    /// The paths waiting on their parent's answer.
    resolving: BTreeSet<NodeKey>,
    waits: Waits,
    /// The read each body or lookup waiting on its reads has outstanding,
    /// kept until every node it awaits is answered and dropped with it.
    readings: BTreeMap<NodeKey, Reading>,
    ready: ReadyQueue<NodeKey>,
    requests: Requests,
    counters: Counters,
    events: Vec<Event>,
    in_flight: InFlight,
    resources: Resources,
}

impl Executor {
    pub fn new(kinds: Kinds, store: Store) -> Self {
        let core = CoreId::fresh();
        Self {
            core,
            kinds,
            paths: PathTable::default(),
            snapshots: Arc::default(),
            indexes: Indexes::default(),
            store,
            lookups: BTreeMap::new(),
            tasks: BTreeMap::new(),
            resolving: BTreeSet::new(),
            waits: Waits::default(),
            readings: BTreeMap::new(),
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
        Snapshot::new(self.core, &self.snapshots, inputs)
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

    /// Ends `request`, then drops every node in progress that no live
    /// request still waits on, directly or through what it reads (contract
    /// 10). A running section such a body started is told through
    /// `cancelled`; what it makes is released when it is delivered.
    pub fn end(&mut self, request: Request) {
        self.core.refuse_foreign(request.id().core, "a request");
        self.requests.end(request);
        self.drop_unwanted_work();
        self.prune();
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    /// Takes in what resource threads have reported, without blocking, then
    /// runs every ready body until none is ready, then ends the requests
    /// nothing can now serve. A finished section is delivered only by `wait`
    /// or `drain`, so what a turn does to bodies depends on the order of the
    /// host's calls alone.
    pub fn turn(&mut self) -> Turn {
        self.prune();
        while let Some(report) = self.resources.try_receive() {
            self.take_in(report);
        }
        while let Some(key) = self.ready.pop() {
            self.advance(key);
        }
        match self.in_flight.running() {
            0 => {
                self.end_stuck();
                Turn::Idle
            }
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

    /// The store, taken out before the rest of the executor is dropped.
    pub fn into_store(mut self) -> Store {
        mem::take(&mut self.store)
    }

    pub fn selected_store(&self, keep: impl FnMut(&StoredResult) -> bool) -> Store {
        self.store.selected(keep)
    }

    /// Drops the index of each snapshot that is no longer live: the host
    /// holds it in no value and no live request is on it (contract 4.4).
    ///
    /// # Panics
    ///
    /// If a node in progress is on such a snapshot: work on a snapshot
    /// keeps its index.
    fn prune(&mut self) {
        let (requests, snapshots) = (&self.requests, &self.snapshots);
        snapshots.forget_unheld();
        let dropped = self
            .indexes
            .prune(|snapshot| snapshots.is_held(snapshot) || requests.any_on(snapshot));
        if let Some(key) = self
            .nodes_in_progress()
            .find(|key| dropped.contains(&key.snapshot))
        {
            index::lost(key.snapshot);
        }
    }

    /// Tells a path already answered in the snapshot at once; otherwise
    /// the request wants it until it is answered.
    fn demand(&mut self, request: RequestId, snapshot: SnapshotId, path: &Path) {
        let key = NodeKey {
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

    /// Every node in progress: a lookup, a body, or a path waiting on its
    /// parent's answer.
    fn nodes_in_progress(&self) -> impl Iterator<Item = &NodeKey> {
        self.lookups
            .keys()
            .chain(self.tasks.keys())
            .chain(&self.resolving)
    }

    /// Whether `key` is among `nodes_in_progress`, found without walking
    /// them.
    fn in_progress(&self, key: NodeKey) -> bool {
        self.reads(key) || self.resolving.contains(&key)
    }

    /// Whether `key` is a lookup or a body: a node in progress that reads.
    fn reads(&self, key: NodeKey) -> bool {
        self.lookups.contains_key(&key) || self.tasks.contains_key(&key)
    }

    fn answered(&self, key: NodeKey) -> bool {
        self.indexes.of(key.snapshot).entry(key.path).is_some()
    }

    /// Sees that `key` will be answered: walks up to the nearest ancestor
    /// that is answered or in progress, each path on the way waiting on its
    /// parent, and works out the topmost path that can be worked out now.
    fn ensure(&mut self, key: NodeKey) {
        if self.answered(key) || self.in_progress(key) {
            return;
        }
        let mut at = key;
        while let Some((parent, _)) = self.paths.parent(at.path) {
            let parent = NodeKey {
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
                Work::Resolve(key) if self.unwanted(key) => {}
                Work::Resolve(key) => queue.extend(self.resolve(key)),
            }
        }
    }

    /// Whether no live request wants `key` and nothing awaits it: a path
    /// its parent's answer released after the reader that awaited it was
    /// dropped.
    fn unwanted(&self, key: NodeKey) -> bool {
        self.requests.wanting(&key).next().is_none() && !self.waits.is_awaited(key)
    }

    /// Works out what the path at `key` is: a root from its snapshot input,
    /// read with the typed read a body uses (decision D11); any other path
    /// from its parent's final answer. A declaration starts a lookup; any
    /// other outcome is the path's answer.
    fn resolve(&mut self, key: NodeKey) -> Option<Work> {
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
                self.look_up(key, declaration);
                None
            }
            settled => Some(Work::Answered(
                key,
                Entry {
                    content: Content::of_settled(&settled),
                    outcome: settled.erase(),
                    declaration: None,
                },
            )),
        }
    }

    /// Removes the lookup or body at `key`, with the read it waits on, if
    /// any, returning the body.
    fn remove_reader(&mut self, key: NodeKey) -> Option<Task> {
        self.readings.remove(&key);
        self.lookups.remove(&key);
        self.tasks.remove(&key)
    }

    /// Queues a lookup for the node at `key`, which starts at its recipe's
    /// key; a kind the core was not built with fails outside the inputs.
    fn look_up(&mut self, key: NodeKey, declaration: Arc<Declaration>) {
        let kind_id = declaration.kind();
        let Some(kind) = self.kinds.get(kind_id) else {
            let unknown = OutsideFailure::UnknownKind { kind: kind_id.0 };
            return self.end_outside(key, unknown);
        };
        let start = Key::of_recipe(kind.recipe());
        self.lookups.insert(key, Lookup::new(declaration, start));
        self.queue(key);
    }

    /// Starts the body of the node at `key`, whose lookup found nothing.
    fn start(&mut self, key: NodeKey, declaration: Arc<Declaration>) {
        let kind_id = declaration.kind();
        let kind = self
            .kinds
            .get(kind_id)
            .expect("a lookup is made only for a kind the core was built with");
        self.counters.count_body_run(kind_id);
        self.tasks.insert(key, Task::start(kind, declaration));
        self.queue(key);
    }

    /// Queues the lookup or body at `key` to be stepped, once however often
    /// it is queued before its step.
    fn queue(&mut self, key: NodeKey) {
        let owed = match (self.lookups.get_mut(&key), self.tasks.get_mut(&key)) {
            (Some(lookup), _) => lookup.queue(),
            (None, Some(task)) => task.owe_poll(),
            (None, None) => false,
        };
        if owed {
            self.ready.push(key);
        }
    }

    /// Drops every node in progress that no live request still waits on,
    /// directly or through the wait relation (contract 10), in key order.
    /// A dropped body is abandoned, so a running section it started is told
    /// through `cancelled`.
    fn drop_unwanted_work(&mut self) {
        let kept = self.waits.reach(self.requests.wanted());
        let unwanted: BTreeSet<_> = self
            .nodes_in_progress()
            .filter(|key| !kept.contains(key))
            .copied()
            .collect();
        for key in unwanted {
            if let Some(task) = self.remove_reader(key) {
                task.abandon();
            }
            self.resolving.remove(&key);
            self.waits.forget(key);
        }
    }

    /// With no body ready and no section in flight nothing can make
    /// progress, so every node in progress is stuck. Ends each live request
    /// that still wants a node with an error naming the bodies those nodes
    /// wait on, every error worked out before any request ends (PRD 12.2 β;
    /// ε replaces this with the cycle rule), then drops the work no live
    /// request still waits on.
    fn end_stuck(&mut self) {
        let stuck: Vec<_> = self
            .requests
            .wanting_any()
            .into_iter()
            .map(|(request, snapshot, wanted)| {
                let waiting = self.bodies_waited_on(wanted);
                (request, snapshot, RequestError::NothingCanRun { waiting })
            })
            .collect();
        for (request, snapshot, error) in stuck {
            self.events.push(Event::Ended {
                request,
                snapshot,
                error,
            });
            self.requests.end_id(request);
        }
        self.drop_unwanted_work();
    }

    /// The bodies reachable from `wanted` through the wait relation, in path
    /// order, a lookup standing for its body. A path waiting on its parent
    /// passes through to it; only bodies are named.
    fn bodies_waited_on(&self, wanted: Vec<NodeKey>) -> Vec<Path> {
        let mut bodies: Vec<_> = self
            .waits
            .reach(wanted)
            .into_iter()
            .filter(|key| self.reads(*key))
            .map(|key| self.paths.path(key.path))
            .collect();
        bodies.sort();
        bodies
    }

    /// Steps the lookup at `key` if it is queued, or polls the body there
    /// if it is owed a poll.
    fn advance(&mut self, key: NodeKey) {
        if self.lookups.contains_key(&key) {
            return self.step_lookup(key);
        }
        let Some(task) = self.tasks.get_mut(&key) else {
            return;
        };
        if !task.take_poll() {
            return;
        }
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
                let declaration = task.declaration();
                let reading =
                    serve::read::serve(key.path, declaration, refs.into(), &mut self.paths, index);
                let awaited = reading.awaited();
                if awaited.is_empty() {
                    task.answer_read(reading.reply(index));
                    return self.queue(key);
                }
                self.readings.insert(key, reading);
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

    /// Follows the store for the lookup at `key` if it is queued: first
    /// past the request it awaited, if any, then from key to key while
    /// each request stored can be answered at once. A result found is the
    /// node's answer, with the node's own declaration, and no body runs; a
    /// request that awaits a node makes the lookup wait as a body would;
    /// a key where nothing is filed starts the body from its beginning.
    fn step_lookup(&mut self, key: NodeKey) {
        let Some(lookup) = self.lookups.get_mut(&key) else {
            return;
        };
        if !lookup.take_queued() {
            return;
        }
        let index = self.indexes.of(key.snapshot);
        if let Some(reading) = self.readings.remove(&key) {
            lookup.advance(&reading.reply(index));
        }
        loop {
            self.counters.count_store_lookup();
            let refs = match self.store.probe(lookup.key()) {
                Some(Stored::Next(refs)) => Arc::clone(refs),
                Some(Stored::Result(result)) => {
                    let entry = Entry {
                        outcome: result.outcome.clone(),
                        declaration: Some(Arc::clone(lookup.declaration())),
                        content: result.content,
                    };
                    self.lookups.remove(&key);
                    return self.work(Work::Answered(key, entry));
                }
                None => {
                    let declaration = Arc::clone(lookup.declaration());
                    self.lookups.remove(&key);
                    return self.start(key, declaration);
                }
            };
            let reading =
                serve::read::serve(key.path, lookup.declaration(), refs, &mut self.paths, index);
            let awaited = reading.awaited();
            if !awaited.is_empty() {
                self.readings.insert(key, reading);
                return self.await_nodes(key, awaited);
            }
            lookup.advance(&reading.reply(index));
        }
    }

    /// Makes the reader at `key` wait on each node in `awaited` before any
    /// of them is worked out, so that none can answer the reading early.
    /// Stops once the reader is dropped: a node that fails outside the
    /// inputs as it is worked out drops it at once.
    fn await_nodes(&mut self, key: NodeKey, awaited: BTreeSet<PathId>) {
        let nodes: Vec<_> = awaited
            .into_iter()
            .map(|path| NodeKey {
                snapshot: key.snapshot,
                path,
            })
            .collect();
        for node in &nodes {
            self.waits.add(key, *node);
        }
        for node in nodes {
            if !self.reads(key) {
                return;
            }
            self.ensure(node);
        }
    }

    /// Queues the lookup at `key`, whose reading awaits nothing more, to
    /// move on when it is next stepped; or replies to the body there and
    /// queues it.
    fn resume_reader(&mut self, key: NodeKey) {
        if self.lookups.contains_key(&key) {
            return self.queue(key);
        }
        let Some(task) = self.tasks.get_mut(&key) else {
            return;
        };
        if let Some(reading) = self.readings.remove(&key) {
            task.answer_read(reading.reply(self.indexes.of(key.snapshot)));
            self.queue(key);
        }
    }

    /// Ends a node whose body produced `outcome`, filing it in the store.
    fn finish(&mut self, key: NodeKey, outcome: Outcome<Erased>) {
        let Some(task) = self.remove_reader(key) else {
            return;
        };
        let (declaration, chain) = task.finish();
        let entry = finish::filed(declaration, chain, outcome, &self.kinds, &mut self.store);
        self.work(Work::Answered(key, entry));
    }

    /// Ends a node with a failure outside the inputs, stored nowhere, and
    /// with it every node that waited on it, in turn: each is dropped and
    /// pending to every live request that wanted it. Then, if any of them
    /// awaited something, drops the work no live request still waits on; a
    /// node that awaits nothing keeps no other work.
    fn end_outside(&mut self, key: NodeKey, failure: OutsideFailure) {
        let mut released_a_wait = self.waits.awaits(key).next().is_some();
        let mut queue = VecDeque::from([key]);
        while let Some(key) = queue.pop_front() {
            if let Some(task) = self.remove_reader(key) {
                task.discard();
            }
            self.resolving.remove(&key);
            let failed = finish::failed_outside(key, &mut self.waits, &mut self.requests);
            for request in failed.tell {
                self.tell(request, key, Answer::Pending(failure.clone()));
            }
            released_a_wait |= !failed.dropped.is_empty();
            queue.extend(failed.dropped);
        }
        if released_a_wait {
            self.drop_unwanted_work();
        }
    }

    fn tell(&mut self, request: RequestId, key: NodeKey, answer: Answer<Erased>) {
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
                self.queue(key);
            }
            Err(failure) => self.end_outside(key, failure),
        }
    }

    /// The task still waiting on section `serial`: the one that started it
    /// may have been dropped, and another started in its place.
    fn waiting_on(&self, serial: u64) -> Option<NodeKey> {
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
    /// Bodies, lookups, the reads they wait on, indexes, the store and
    /// untaken answers go before the resources, so what they hold is queued
    /// for release before each resource thread stops.
    fn drop(&mut self) {
        for (_, task) in mem::take(&mut self.tasks) {
            task.discard();
        }
        self.lookups.clear();
        self.readings.clear();
        self.indexes.clear();
        self.store.clear();
        self.events.clear();
    }
}
