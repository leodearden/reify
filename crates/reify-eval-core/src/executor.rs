//! The executor: works out the paths a request demands and runs every body
//! that is ready, on the thread that calls it. What one answer sets off is
//! worked through a queue, never by recursion. At rest, with no body ready
//! and no section running, every node in progress awaits another, so a
//! request still waiting waits on a ring; each ring is broken by the rule of
//! `cycle` (contract 7.7), and the work goes on.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::Poll;

use crate::answer::Answer;
use crate::budget::Budget;
use crate::content::Content;
use crate::core_id::CoreId;
use crate::counters::Counters;
use crate::cycle;
use crate::declaration::Declaration;
use crate::declared_children::Conflict;
use crate::demand::Demand;
use crate::event::{Event, RequestError, RequestId};
use crate::failure::{Failure, OutsideFailure, panic_message};
use crate::finish;
use crate::index::{self, Declared, Entry, Indexes, NodeKey, StateId};
use crate::input::{Input, InputKey};
use crate::job::Report;
use crate::key::Key;
use crate::kinds::Kinds;
use crate::limit_check;
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
use crate::serve::view_read::Opener;
use crate::snapshot::{Snapshot, SnapshotId, SnapshotTokens};
use crate::store::{Store, Stored, StoredResult};
use crate::substitution::Substitution;
use crate::task::Task;
use crate::waits::Waits;
use crate::wanted::Wanted;

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

impl Work {
    fn key(&self) -> NodeKey {
        match self {
            Work::Answered(key, _) | Work::Resolve(key) => *key,
        }
    }
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
    /// The paths waiting to be worked out: each on its parent's answer,
    /// or, at a view's opener's own path under that view, on the opener's
    /// own node (contract 7.7).
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

    pub fn evaluate(&mut self, snapshot: &Snapshot, demand: Demand, budget: Budget) -> Request {
        self.core.refuse_foreign(snapshot.id().core, "a snapshot");
        self.prune();
        self.indexes.open(snapshot);
        let request = self.requests.open(snapshot.id(), budget);
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
    /// runs every ready body until none is ready. If then no section is
    /// running, breaks the rings of waits and runs what that makes ready,
    /// until no ring is left. A finished section is delivered only by
    /// `wait` or `drain`, so what a turn does to bodies depends on the order
    /// of the host's calls alone.
    pub fn turn(&mut self) -> Turn {
        self.prune();
        while let Some(report) = self.resources.try_receive() {
            self.take_in(report);
        }
        loop {
            while let Some(key) = self.ready.pop() {
                self.advance(key);
            }
            let running = self.in_flight.running();
            if running > 0 {
                return Turn::SectionsRunning(running);
            }
            if !self.break_rings() {
                break;
            }
        }
        self.refuse_stuck();
        Turn::Idle
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
            .find(|key| dropped.contains(&key.state.snapshot))
        {
            index::lost(key.state);
        }
    }

    /// Tells a path already answered in the snapshot at once; otherwise
    /// the request wants it until it is answered. A request its budget has
    /// ended is told nothing more.
    fn demand(&mut self, request: RequestId, snapshot: SnapshotId, path: &Path) {
        if !self.requests.is_live(request) {
            return;
        }
        let key = NodeKey {
            state: StateId::of(snapshot),
            path: self.paths.intern(path),
        };
        if let Some(entry) = self.indexes.of(key.state).entry(key.path) {
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
        self.is_reader(key) || self.resolving.contains(&key)
    }

    /// Whether `key` is a lookup or a body: a node in progress that reads.
    fn is_reader(&self, key: NodeKey) -> bool {
        self.lookups.contains_key(&key) || self.tasks.contains_key(&key)
    }

    fn answered(&self, key: NodeKey) -> bool {
        self.indexes.of(key.state).entry(key.path).is_some()
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
                state: key.state,
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

    /// Works through what `first` sets off, in order. Work on a view that
    /// has closed, as its opener's run ended, is dropped.
    fn work(&mut self, first: Work) {
        let mut queue = VecDeque::from([first]);
        while let Some(work) = queue.pop_front() {
            if !self.indexes.is_open(work.key().state) {
                continue;
            }
            match work {
                Work::Answered(key, entry) => {
                    let limits = self.indexes.of(key.state).limits();
                    let entry = limit_check::children(entry, limits, &self.kinds);
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
    /// from its parent's final answer, on whose dependence on a
    /// substitution its declaration depends.
    fn resolve(&mut self, key: NodeKey) -> Option<Work> {
        let index = self.indexes.of(key.state);
        let declared = match self.paths.parent(key.path) {
            None => {
                let root = self
                    .paths
                    .as_root(key.path)
                    .expect("a path with no parent is a root");
                let input = index.input(&InputKey::Root(root.clone()));
                let declared = input.map_or(Outcome::Absent, Input::outcome);
                Declared {
                    declaration: declared.downcast::<Declaration>(),
                    depended: false,
                }
            }
            Some((parent, step)) => {
                let entry = index
                    .entry(parent)
                    .expect("a path is worked out once its parent is answered");
                Declared {
                    declaration: declaration_of(step, entry, &self.kinds),
                    depended: entry.depended,
                }
            }
        };
        self.work_out(key, declared)
    }

    /// Works out the path at `key`, which resolved to `declared`, or which
    /// a body declared so (contract 7.1). A declaration of a node too deep
    /// for the snapshot's depth setting answers that failure; one at its
    /// view's opener's own path is the opener's, which awaits it; one at a
    /// path its view supposes answers the supposed outcome; any other
    /// declaration starts a lookup; any other outcome is the path's answer.
    /// A path no live request needs once its resolution is charged is not
    /// worked out.
    fn work_out(&mut self, key: NodeKey, declared: Declared<Outcome<Declaration>>) -> Option<Work> {
        if !self.charge_resolution(key) {
            return None;
        }
        self.counters.count_path_resolved();
        let Declared {
            declaration,
            depended,
        } = declared;
        let declaration = match declaration {
            Outcome::Value(_) => {
                let depth = self.paths.depth(key.path);
                let limits = self.indexes.of(key.state).limits();
                limit_check::depth(depth, limits).map_or(declaration, Outcome::Failure)
            }
            other => other,
        };
        let (content, outcome) = match declaration {
            Outcome::Value(_) if self.is_opener_under_its_view(key) => {
                self.await_opener(key);
                return None;
            }
            Outcome::Value(declaration) => {
                if let Some(supposed) = self.indexes.of(key.state).supposed(key.path) {
                    return Some(Work::Answered(key, Entry::supposed(supposed, declaration)));
                }
                let declared = Declared {
                    declaration,
                    depended,
                };
                self.look_up(key, declared);
                return None;
            }
            Outcome::Failure(failure) => (Content::of_failure(&failure), Outcome::Failure(failure)),
            Outcome::Absent => (Content::absent(), Outcome::Absent),
        };
        Some(Work::Answered(
            key,
            Entry::settled(outcome, content, depended),
        ))
    }

    /// Whether `key` is the path of its view's opener, under that view.
    fn is_opener_under_its_view(&self, key: NodeKey) -> bool {
        key.state.view.is_some() && self.indexes.of(key.state).opener() == Some(key.path)
    }

    /// Makes the node at `key`, its view's opener's own path, which the
    /// view's structure declares a node, wait on the opener's own node,
    /// which is running in the snapshot: so a node under the view that
    /// needs it waits on the running opener, a ring (contract 7.7). What
    /// is above it under the view has been worked out there, so where the
    /// view gives it no node, it answers that instead (contract 6,
    /// structure follows). It waits, as a path waiting to be worked out,
    /// until the view closes with the opener's run.
    fn await_opener(&mut self, key: NodeKey) {
        let opener = NodeKey {
            state: StateId::of(key.state.snapshot),
            path: key.path,
        };
        self.resolving.insert(key);
        self.waits.add(key, opener);
    }

    /// Works out the child at `path`, which the reader at `reader`, its
    /// parent, declared `declaration` and awaits; the declaration depends
    /// on a substitution if what the reader read so far did. The reader's
    /// own declaration, `declarer`, is filed first, so that the child's
    /// passed bindings can be read through it while the reader runs. A path
    /// already answered, or already a lookup or a body, is left as it is; a
    /// path waiting on its parent is taken over, its wait on the parent
    /// ended, since the parent's run is what declares it.
    fn declare(
        &mut self,
        reader: NodeKey,
        declarer: &Arc<Declaration>,
        path: PathId,
        declaration: Arc<Declaration>,
    ) {
        self.indexes
            .file_declarer(reader.state, reader.path, declarer);
        let key = NodeKey {
            state: reader.state,
            path,
        };
        if self.answered(key) || self.is_reader(key) {
            return;
        }
        if self.resolving.remove(&key) {
            self.waits.forget(key);
        }
        let declared = Declared {
            declaration: Outcome::Value(declaration),
            depended: self.depended_so_far(reader),
        };
        if let Some(work) = self.work_out(key, declared) {
            self.work(work);
        }
    }

    /// Whether the answer of the node in progress at `key` depends on a
    /// substitution by what it has read so far: a lookup's or a body's
    /// flag; a path waiting on its parent has read nothing.
    fn depended_so_far(&self, key: NodeKey) -> bool {
        match (self.lookups.get(&key), self.tasks.get(&key)) {
            (Some(lookup), _) => lookup.depended(),
            (None, Some(task)) => task.depended(),
            (None, None) => false,
        }
    }

    /// Charges the resolution of `key` to every live request that needs it:
    /// each that wants a node waiting on `key`, directly or through other
    /// waits. Each whose budget allows no more is ended instead, by the
    /// route `end` takes; whether `key` is still needed. Nothing is charged
    /// while no live request on the snapshot has a bounded budget. Finding
    /// the requests walks every node waiting on `key`, so under a bounded
    /// budget a chain of n waiting bodies costs on the order of n² steps.
    fn charge_resolution(&mut self, key: NodeKey) -> bool {
        if !self.requests.any_bounded_on(key.state.snapshot) {
            return true;
        }
        let needing: BTreeSet<RequestId> = self
            .waits
            .reach_waiters(key)
            .iter()
            .flat_map(|node| self.requests.wanting(node))
            .collect();
        let exhausted = self.requests.charge(&needing);
        if exhausted.is_empty() {
            return true;
        }
        for (request, snapshot, budget) in exhausted {
            self.events.push(Event::Ended {
                request,
                snapshot,
                error: RequestError::PathsBudgetExhausted { budget },
            });
            self.requests.end_id(request);
        }
        self.drop_unwanted_work();
        !self.unwanted(key)
    }

    /// Removes the lookup or body at `key`, with the read it waits on, if
    /// any, returning the body. The views the body's run opened close.
    fn remove_reader(&mut self, key: NodeKey) -> Option<Task> {
        self.readings.remove(&key);
        self.lookups.remove(&key);
        let task = self.tasks.remove(&key)?;
        for view in task.views() {
            self.close(view);
        }
        Some(task)
    }

    /// Closes `view`, whose opener's run has ended: every node in progress
    /// on it is withdrawn, each body abandoned, and its index dropped.
    fn close(&mut self, view: StateId) {
        let on_view: Vec<_> = self
            .nodes_in_progress()
            .filter(|key| key.state == view)
            .copied()
            .collect();
        for key in on_view {
            if let Some(task) = self.withdraw(key) {
                task.abandon();
            }
        }
        self.indexes.close(view);
    }

    /// Takes the node at `key` out of the nodes in progress: the lookup or
    /// body there with the read it waits on, or the path waiting on its
    /// parent; and ends every wait of its own. Returns the body, for the
    /// caller to discard or abandon.
    fn withdraw(&mut self, key: NodeKey) -> Option<Task> {
        self.resolving.remove(&key);
        self.waits.forget(key);
        self.remove_reader(key)
    }

    /// Queues a lookup for the node at `key`, `declared` so, which starts
    /// at its recipe's key; a kind the core was not built with fails
    /// outside the inputs.
    fn look_up(&mut self, key: NodeKey, declared: Declared) {
        let kind_id = declared.declaration.kind();
        let Some(kind) = self.kinds.get(kind_id) else {
            let unknown = OutsideFailure::UnknownKind { kind: kind_id.0 };
            return self.end_outside(key, unknown);
        };
        let start = Key::of_recipe(kind.recipe());
        let lookup = Lookup::new(declared, start);
        self.lookups.insert(key, lookup);
        self.queue(key);
    }

    /// Drops the lookup at `key`, with the read it waits on and its waits,
    /// and starts the body of its node from the beginning; whatever waits on
    /// the node waits on that body.
    fn miss(&mut self, key: NodeKey) {
        self.readings.remove(&key);
        self.waits.forget(key);
        let declared = self
            .lookups
            .remove(&key)
            .expect("a lookup that misses is in progress")
            .miss();
        self.start(key, declared);
    }

    /// Starts the body of the node at `key`, `declared` so, whose lookup
    /// found nothing.
    fn start(&mut self, key: NodeKey, declared: Declared) {
        let kind_id = declared.declaration.kind();
        let kind = self
            .kinds
            .get(kind_id)
            .expect("a lookup is made only for a kind the core was built with");
        self.counters.count_body_run(kind_id);
        self.tasks.insert(key, Task::start(kind, declared));
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
            if let Some(task) = self.withdraw(key) {
                task.abandon();
            }
        }
    }

    /// At rest, breaks every ring of waits a live request needs (contract
    /// 7.7): each lookup on a ring that holds one misses and its body
    /// starts; each node of a ring that holds none answers "cycle". Then
    /// drops the work no live request still waits on; whether it broke any.
    /// Every node that answers leaves the nodes in progress before any
    /// answers, so no answer on a ring resumes a node on it or works out a
    /// path on it. Withdrawing a view's opener closes the view, whose nodes
    /// then answer nothing. A ring's nodes in a view depended on its
    /// substitution if any node on the ring had read an answer that did; in
    /// a snapshot nothing does.
    fn break_rings(&mut self) -> bool {
        let lookups = &self.lookups;
        let broken = cycle::broken(&self.waits, self.requests.wanted(), |key| {
            lookups.contains_key(key)
        });
        if broken.is_empty() {
            return false;
        }
        for key in broken.missed {
            self.miss(key);
        }
        let rings: Vec<_> = broken
            .answered
            .into_iter()
            .map(|ring| {
                let depended = ring.iter().any(|key| self.depended_so_far(*key));
                (ring, depended)
            })
            .collect();
        for key in rings.iter().flat_map(|(ring, _)| ring) {
            if let Some(task) = self.withdraw(*key) {
                task.discard();
            }
        }
        for (ring, depended) in rings {
            for key in ring {
                let in_view = key.state.view.is_some();
                self.work(Work::Answered(key, cycle::entry(depended && in_view)));
            }
        }
        self.drop_unwanted_work();
        true
    }

    /// # Panics
    ///
    /// If a live request still wants a node at rest with every ring broken:
    /// every node in progress then awaits another, so the nodes a request
    /// waits on would hold a ring.
    fn refuse_stuck(&self) {
        let stuck: Vec<_> = self
            .requests
            .wanting_any()
            .into_iter()
            .map(|(request, _, wanted)| {
                let paths: Vec<_> = wanted.iter().map(|key| self.paths.path(key.path)).collect();
                (request, paths)
            })
            .collect();
        assert!(
            stuck.is_empty(),
            "at rest with no ring of waits left, live requests still want {stuck:?}"
        );
    }

    /// Steps the lookup at `key` if it is queued, or polls the body there
    /// if it is owed a poll and serves what it asks. A body that opened a
    /// view reads only through its views from then on (O9): a read of a
    /// ref or a child ends its run with that failure, while a section,
    /// which reads nothing, is served.
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
            Posted::Ask(Ask::Read(_)) if task.has_opened_view() => {
                self.finish(key, Outcome::Failure(Failure::ReadOutsideView));
            }
            Posted::Ask(Ask::Read(wanted)) => {
                let index = self.indexes.of(key.state);
                let declaration = task.declaration();
                let children = task.children();
                let served = serve::read::serve(
                    key.path,
                    key.state,
                    declaration,
                    wanted.into(),
                    children,
                    &mut self.paths,
                    index,
                );
                let reading = match served {
                    Ok(reading) => reading,
                    Err(Conflict { step }) => {
                        let conflict = Failure::ConflictingChild { step };
                        return self.finish(key, Outcome::Failure(conflict));
                    }
                };
                let awaited = reading.awaited();
                if awaited.is_empty() {
                    task.answer_read(reading.reply(index));
                    return self.queue(key);
                }
                let declaration = Arc::clone(declaration);
                self.await_reading(key, reading, awaited, &declaration);
            }
            Posted::Ask(Ask::ViewRead {
                ordinal,
                substitution,
                wanted,
            }) => self.read_through_view(key, ordinal, &substitution, wanted),
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

    /// Serves the read of `wanted` that the body at `key` made through its
    /// run's view `ordinal`, which supposes `substitution`, opening the view
    /// on its first read. A substitution that names no place of its own is
    /// the body's answer, and a view opened by a body under a view fails
    /// outside the inputs until views nest (leaf λ, #10563).
    fn read_through_view(
        &mut self,
        key: NodeKey,
        ordinal: u32,
        substitution: &Substitution,
        wanted: Arc<[Wanted]>,
    ) {
        if key.state.view.is_some() {
            return self.end_outside(key, OutsideFailure::ViewInsideView);
        }
        let task = self.tasks.get_mut(&key).expect("a body that asked is held");
        let view = match task.view(ordinal) {
            Some(view) => view,
            None => {
                let opener = Opener {
                    path: key.path,
                    declaration: task.declaration(),
                    index: self.indexes.of(key.state),
                };
                match serve::view_read::open(&opener, substitution, &mut self.paths) {
                    Ok(supposed) => {
                        let snapshot = key.state.snapshot;
                        let view = self.indexes.open_view(snapshot, key.path, supposed);
                        task.opened(ordinal, view);
                        view
                    }
                    Err(failure) => return self.finish(key, Outcome::Failure(failure)),
                }
            }
        };
        let opener = Opener {
            path: key.path,
            declaration: task.declaration(),
            index: self.indexes.of(key.state),
        };
        let (paths, view_index) = (&mut self.paths, self.indexes.of(view));
        let reading =
            serve::view_read::serve(&opener, substitution, wanted, paths, view, view_index);
        let awaited = reading.awaited();
        if awaited.is_empty() {
            task.answer_read(reading.reply(self.indexes.of(view)));
            return self.queue(key);
        }
        let declaration = Arc::clone(task.declaration());
        self.await_reading(key, reading, awaited, &declaration);
    }

    /// Follows the store for the lookup at `key` if it is queued: first
    /// past the request it awaited, if any, then from key to key while
    /// each request stored can be answered at once. A result found is the
    /// node's answer, with the node's own declaration, and no body runs; a
    /// request that awaits a node makes the lookup wait as a body would;
    /// a request that declares a child declares it at the lookup's own path
    /// and awaits it; a key where nothing is filed starts the body from its
    /// beginning, and so would a request in conflict with the children the
    /// lookup declared. Only keys that collide could file one: a key hashes
    /// every child item of the requests on the way to it, so those requests
    /// are one run's, and that run met no conflict among them.
    fn step_lookup(&mut self, key: NodeKey) {
        let Some(lookup) = self.lookups.get_mut(&key) else {
            return;
        };
        if !lookup.take_queued() {
            return;
        }
        let index = self.indexes.of(key.state);
        if let Some(reading) = self.readings.remove(&key) {
            lookup.advance(reading.reply(index));
        }
        loop {
            self.counters.count_store_lookup();
            let wanted = match self.store.probe(lookup.key()) {
                Some(Stored::Next(wanted)) => Arc::clone(wanted),
                Some(Stored::Result(result)) => {
                    let (outcome, content) = (result.outcome.clone(), result.content);
                    let found = self.lookups.remove(&key);
                    let (declaration, children, depended) =
                        found.expect("a lookup stepped is held").found();
                    let entry = Entry {
                        outcome,
                        declaration: Some(declaration),
                        content,
                        children_declared: children,
                        depended,
                    };
                    return self.work(Work::Answered(key, entry));
                }
                None => return self.miss(key),
            };
            let served = serve::read::serve(
                key.path,
                key.state,
                lookup.declaration(),
                wanted,
                lookup.children(),
                &mut self.paths,
                index,
            );
            let Ok(reading) = served else {
                return self.miss(key);
            };
            let awaited = reading.awaited();
            if !awaited.is_empty() {
                let declaration = Arc::clone(lookup.declaration());
                return self.await_reading(key, reading, awaited, &declaration);
            }
            lookup.advance(reading.reply(index));
        }
    }

    /// Keeps the reading of the reader at `key`, declared `declaration`,
    /// until each node in `awaited`, of the reading's state, is answered,
    /// and makes the reader wait on them, declaring the children the
    /// reading lists.
    fn await_reading(
        &mut self,
        key: NodeKey,
        mut reading: Reading,
        awaited: BTreeSet<PathId>,
        declaration: &Arc<Declaration>,
    ) {
        let to_declare = reading.take_to_declare();
        let state = reading.state();
        self.readings.insert(key, reading);
        let nodes = awaited.into_iter().map(|path| NodeKey { state, path });
        self.await_nodes(key, nodes.collect(), to_declare, declaration);
    }

    /// Makes the reader at `key`, declared `declaration`, wait on each of
    /// `nodes` before any of them is worked out, so that none can answer
    /// the reading early; then declares each child in `to_declare`, and
    /// sees that each other node will be answered. Stops once the reader is
    /// dropped: a node that fails outside the inputs as it is worked out
    /// drops it at once.
    fn await_nodes(
        &mut self,
        key: NodeKey,
        nodes: Vec<NodeKey>,
        to_declare: Vec<(PathId, Arc<Declaration>)>,
        declaration: &Arc<Declaration>,
    ) {
        for node in &nodes {
            self.waits.add(key, *node);
        }
        for (path, child) in to_declare {
            if !self.is_reader(key) {
                return;
            }
            self.declare(key, declaration, path, child);
        }
        for node in nodes {
            if !self.is_reader(key) {
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
            let index = self.indexes.of(reading.state());
            task.answer_read(reading.reply(index));
            self.queue(key);
        }
    }

    /// Ends a node whose body produced `outcome`, filing it in the store.
    fn finish(&mut self, key: NodeKey, outcome: Outcome<Erased>) {
        let Some(task) = self.remove_reader(key) else {
            return;
        };
        let entry = finish::filed(task.finish(), outcome, &self.kinds, &mut self.store);
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
            if let Some(task) = self.withdraw(key) {
                task.discard();
            }
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
            snapshot: key.state.snapshot,
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
                        snapshot: key.state.snapshot,
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
