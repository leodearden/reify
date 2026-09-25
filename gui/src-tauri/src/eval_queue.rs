//! The ENGINE lane's single front door: every engine-touching request is
//! submitted here, and its caller awaits an [`EvalTicket`] without blocking any
//! thread.
//!
//! A single drainer at a time runs the queue in FIFO order on the executor —
//! the ENGINE lane in production. Evaluations and engine calls are ordered
//! barriers. Edits coalesce, per the esc-5215-5 ruling that the newest edit
//! supersedes queued ones: a newly admitted edit resolves every QUEUED older
//! edit of its target unrun, and an edit arriving after a newer one of its
//! target was already admitted is dropped. Two rules decide "older": the newest
//! edit wins within a target, and a durable write never yields to a transient
//! one. A superseding edit takes its own arrival position, so it never jumps a
//! barrier.
//!
//! Edits and evaluations publish: their snapshot becomes a delta before their
//! reply is delivered, and the queue reports [`EvalActivity::Evaluating`] when
//! the first of them is accepted while idle and [`EvalActivity::Idle`] after
//! the last one's delta.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use serde::{Deserialize, Serialize};

use crate::diff::{StateDelta, advance_baseline};
use crate::large_stack::panic_payload_message;
use crate::types::GuiState;

/// Where the frontend stamped an edit: `seq` increases with every edit of one
/// page load, and `epoch` identifies the page load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditOrder {
    pub epoch: u64,
    pub seq: u64,
}

impl EditOrder {
    /// Stamped after `other`, or in another page load — a reload restarts
    /// `seq`, so across epochs the arriving edit counts as the later one.
    fn is_after(self, other: Self) -> bool {
        self.epoch != other.epoch || self.seq > other.seq
    }

    /// Stamped before `other` in the same page load.
    fn is_before(self, other: Self) -> bool {
        self.epoch == other.epoch && self.seq < other.seq
    }
}

/// What an edit changes. Only edits of one target coalesce.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EditTarget {
    Parameter(String),
    EditorSource(PathBuf),
    DiskSource(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum EditStrength {
    Transient,
    Durable,
}

/// The identity by which an edit is coalesced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditIdentity {
    target: EditTarget,
    strength: EditStrength,
    order: Option<EditOrder>,
}

impl EditIdentity {
    /// A slider-drag frame: a transient parameter override.
    pub fn preview(cell_id: impl Into<String>, order: EditOrder) -> Self {
        Self::stamped(
            EditTarget::Parameter(cell_id.into()),
            EditStrength::Transient,
            order,
        )
    }

    /// A durable parameter write back into the `.ri` source.
    pub fn commit(cell_id: impl Into<String>, order: EditOrder) -> Self {
        Self::stamped(
            EditTarget::Parameter(cell_id.into()),
            EditStrength::Durable,
            order,
        )
    }

    /// A sync of the editor's buffer for `path`, which replaces the source
    /// whole.
    pub fn editor_source(path: impl Into<PathBuf>, order: EditOrder) -> Self {
        Self::stamped(
            EditTarget::EditorSource(path.into()),
            EditStrength::Durable,
            order,
        )
    }

    /// A watcher reload of `path`. Unstamped: the reload reads the file when it
    /// runs, so the latest arrival is the one worth running.
    pub fn disk_source(path: impl Into<PathBuf>) -> Self {
        Self {
            target: EditTarget::DiskSource(path.into()),
            strength: EditStrength::Durable,
            order: None,
        }
    }

    fn stamped(target: EditTarget, strength: EditStrength, order: EditOrder) -> Self {
        Self {
            target,
            strength,
            order: Some(order),
        }
    }

    /// Whether this arriving edit makes the queued `older` one redundant.
    pub fn supersedes(&self, older: &EditIdentity) -> bool {
        self.target == older.target
            && self.strength >= older.strength
            && match (self.order, older.order) {
                (Some(newer), Some(older)) => newer.is_after(older),
                _ => true,
            }
    }
}

/// The per-target high-water marks of admitted edits.
#[derive(Debug, Default)]
pub struct EditLedger {
    marks: HashMap<EditTarget, BTreeMap<EditStrength, EditOrder>>,
}

impl EditLedger {
    /// Admit `identity` and record its stamp, or refuse it — recording nothing —
    /// when an edit of its target that is at least as strong and stamped later
    /// in the same page load was already admitted.
    pub fn admit(&mut self, identity: &EditIdentity) -> bool {
        let Some(order) = identity.order else {
            return true;
        };
        let marks = self.marks.entry(identity.target.clone()).or_default();
        let late = marks
            .range(identity.strength..)
            .any(|(_, &mark)| order.is_before(mark));
        if !late {
            marks.insert(identity.strength, order);
        }
        !late
    }
}

/// Whether the queue has edits or evaluations in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalActivity {
    Evaluating,
    Idle,
}

/// What the frontend is told about evaluation.
///
/// [`EvalQueue`] reports activity while holding its own lock, so that status
/// transitions cannot reorder across threads: an observer must never call back
/// into the queue.
pub trait EvalObserver: Send + Sync {
    fn activity(&self, activity: EvalActivity);
    fn delta(&self, delta: &StateDelta);
}

/// The one place evaluation results become frontend-visible deltas.
///
/// The baseline is the same `Arc` the debug server diffs against (INV-GUI-2):
/// otherwise a debug-driven mutation would advance the engine without the
/// deltas published here ever accounting for it.
pub struct SnapshotPublisher {
    baseline: Arc<Mutex<Option<GuiState>>>,
    observer: Arc<dyn EvalObserver>,
    last_generation: Mutex<Option<u64>>,
}

impl SnapshotPublisher {
    pub fn new(baseline: Arc<Mutex<Option<GuiState>>>, observer: Arc<dyn EvalObserver>) -> Self {
        Self {
            baseline,
            observer,
            last_generation: Mutex::new(None),
        }
    }

    /// Advance the baseline to `state` and report the delta — or, when
    /// `generation` is not newer than the last one published, touch nothing and
    /// return `false`, so deltas never go backwards.
    pub fn publish(&self, generation: u64, state: GuiState) -> bool {
        let mut last = self
            .last_generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(last) = *last
            && generation <= last
        {
            tracing::warn!(
                "refused to publish evaluation generation {generation}: generation {last} \
                 was already published"
            );
            return false;
        }
        *last = Some(generation);
        let delta = advance_baseline(&self.baseline, state);
        self.observer.delta(&delta);
        true
    }
}

/// Runs a job on another thread, some time later; `Err` means the job was
/// dropped unrun. It must never run the job inline: [`EvalQueue`] posts while
/// holding its lock.
pub type Executor = Arc<dyn Fn(Box<dyn FnOnce() + Send>) -> std::io::Result<()> + Send + Sync>;

/// What a job produced: the snapshot to publish, if any, and its reply.
pub struct EvalOutcome<T> {
    pub publish: Option<GuiState>,
    pub reply: Result<T, String>,
}

/// A unit of engine work, as the queue will treat it, and the ticket its reply
/// arrives on.
pub struct EvalRequest<T> {
    work: Work,
    ticket: EvalTicket<T>,
}

impl EvalRequest<()> {
    /// A coalescable edit that publishes its snapshot. It replies only whether
    /// it succeeded — its state reaches the frontend as a delta — so an edit a
    /// newer one supersedes resolves `Ok(())` unrun.
    pub fn edit(
        identity: EditIdentity,
        job: impl FnOnce() -> EvalOutcome<()> + Send + 'static,
    ) -> Self {
        Self::new(job, |job| Work::Edit(identity, Box::new(job)))
    }
}

impl<T: Send + 'static> EvalRequest<T> {
    /// An ordered evaluation that publishes its snapshot.
    pub fn evaluation(job: impl FnOnce() -> EvalOutcome<T> + Send + 'static) -> Self {
        Self::new(job, |job| Work::Evaluation(Box::new(job)))
    }

    /// An ordered read or registration that publishes nothing.
    pub fn engine_call(job: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let job = move || EvalOutcome {
            publish: None,
            reply: job(),
        };
        Self::new(job, |job| Work::EngineCall(Box::new(job)))
    }

    /// Join `job` to the sender of its ticket, and make it the work `as_work`
    /// says it is.
    fn new(
        job: impl FnOnce() -> EvalOutcome<T> + Send + 'static,
        as_work: impl FnOnce(TypedJob<T>) -> Work,
    ) -> Self {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        Self {
            work: as_work(TypedJob {
                job: Box::new(job),
                reply,
            }),
            ticket: EvalTicket { reply: receiver },
        }
    }
}

/// Resolves to its request's reply once the request ran, or to `Ok(())` for an
/// edit a newer edit made redundant. A request dropped without a reply resolves
/// `Err`, so a ticket never hangs.
pub struct EvalTicket<T> {
    reply: tokio::sync::oneshot::Receiver<Result<T, String>>,
}

impl<T> Future for EvalTicket<T> {
    type Output = Result<T, String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.reply).poll(cx).map(|received| {
            received.unwrap_or_else(|_| Err("evaluation dropped without a reply".to_string()))
        })
    }
}

/// A request's job and its ticket's sender, with the reply type erased so one
/// queue holds every request.
trait QueuedJob: Send {
    /// Run the job — a panicking job replies `Err` — hand its snapshot, if
    /// any, to `publish`, then deliver the reply.
    fn run(self: Box<Self>, publish: &mut dyn FnMut(GuiState));

    /// Reply `Err(message)` without running the job.
    fn fail(self: Box<Self>, message: String);
}

/// An edit's job, which a newer edit of its target may resolve unrun.
trait QueuedEdit: QueuedJob {
    fn supersede(self: Box<Self>);
}

struct TypedJob<T> {
    job: Box<dyn FnOnce() -> EvalOutcome<T> + Send>,
    reply: tokio::sync::oneshot::Sender<Result<T, String>>,
}

impl<T> TypedJob<T> {
    fn resolve(self, reply: Result<T, String>) {
        // A dropped ticket means nobody is waiting for the reply.
        let _ = self.reply.send(reply);
    }
}

impl<T: Send> QueuedJob for TypedJob<T> {
    fn run(self: Box<Self>, publish: &mut dyn FnMut(GuiState)) {
        let TypedJob { job, reply } = *self;
        let outcome =
            std::panic::catch_unwind(AssertUnwindSafe(job)).unwrap_or_else(|payload| EvalOutcome {
                publish: None,
                reply: Err(format!(
                    "panic in evaluation: {}",
                    panic_payload_message(&*payload)
                )),
            });
        if let Some(state) = outcome.publish {
            publish(state);
        }
        // A dropped ticket means nobody is waiting for the reply.
        let _ = reply.send(outcome.reply);
    }

    fn fail(self: Box<Self>, message: String) {
        self.resolve(Err(message));
    }
}

impl QueuedEdit for TypedJob<()> {
    fn supersede(self: Box<Self>) {
        self.resolve(Ok(()));
    }
}

/// A submitted request before the queue has accepted it.
enum Work {
    Edit(EditIdentity, Box<dyn QueuedEdit>),
    Evaluation(Box<dyn QueuedJob>),
    EngineCall(Box<dyn QueuedJob>),
}

impl Work {
    fn into_job(self) -> Box<dyn QueuedJob> {
        match self {
            Work::Edit(_, job) => job,
            Work::Evaluation(job) | Work::EngineCall(job) => job,
        }
    }
}

/// An accepted request. Edits and evaluations carry the generation their
/// snapshot publishes under.
enum Entry {
    Edit {
        identity: EditIdentity,
        generation: u64,
        job: Box<dyn QueuedEdit>,
    },
    Ordered {
        generation: Option<u64>,
        job: Box<dyn QueuedJob>,
    },
}

impl Entry {
    fn generation(&self) -> Option<u64> {
        match self {
            Entry::Edit { generation, .. } => Some(*generation),
            Entry::Ordered { generation, .. } => *generation,
        }
    }

    fn into_job(self) -> Box<dyn QueuedJob> {
        match self {
            Entry::Edit { job, .. } => job,
            Entry::Ordered { job, .. } => job,
        }
    }
}

#[derive(Default)]
struct QueueState {
    pending: VecDeque<Entry>,
    ledger: EditLedger,
    /// A drainer is posted or running; it clears this when it finds the queue
    /// empty, under the same lock as every push, or when it unwinds.
    draining: bool,
    /// Accepted edits and evaluations not yet finished, queued or running.
    outstanding: usize,
    last_generation: u64,
}

impl QueueState {
    fn issue_generation(&mut self) -> u64 {
        self.last_generation += 1;
        self.last_generation
    }

    /// Queue `work` behind everything accepted before it, first resolving the
    /// queued edits it supersedes.
    fn enqueue(&mut self, work: Work) {
        let entry = match work {
            Work::Edit(identity, job) => {
                self.supersede_queued(&identity);
                Entry::Edit {
                    generation: self.issue_generation(),
                    identity,
                    job,
                }
            }
            Work::Evaluation(job) => Entry::Ordered {
                generation: Some(self.issue_generation()),
                job,
            },
            Work::EngineCall(job) => Entry::Ordered {
                generation: None,
                job,
            },
        };
        if entry.generation().is_some() {
            self.outstanding += 1;
        }
        self.pending.push_back(entry);
    }

    /// Resolve and remove every QUEUED edit `identity` supersedes. The running
    /// entry is never touched: cancelling it on supersede is #5215, whose seam
    /// is this pass.
    fn supersede_queued(&mut self, identity: &EditIdentity) {
        let mut kept = VecDeque::with_capacity(self.pending.len());
        for entry in std::mem::take(&mut self.pending) {
            match entry {
                Entry::Edit {
                    identity: queued,
                    job,
                    ..
                } if identity.supersedes(&queued) => {
                    self.outstanding -= 1;
                    job.supersede();
                }
                entry => kept.push_back(entry),
            }
        }
        self.pending = kept;
    }
}

/// Repairs the queue if its drainer unwinds past `process`'s own catches, as a
/// job's panic payload that panics again when dropped makes it. Otherwise
/// `draining` would stay set with no drainer behind it, and every later request
/// would queue unrun.
struct DrainerUnwind<'a>(&'a Arc<EvalQueue>);

impl Drop for DrainerUnwind<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.recover_from_unwound_drainer();
        }
    }
}

/// The queue in front of the ENGINE lane.
pub struct EvalQueue {
    executor: Executor,
    publisher: SnapshotPublisher,
    observer: Arc<dyn EvalObserver>,
    state: Mutex<QueueState>,
}

impl EvalQueue {
    /// A queue draining on the persistent ENGINE lane.
    pub fn on_engine_lane(
        baseline: Arc<Mutex<Option<GuiState>>>,
        observer: Arc<dyn EvalObserver>,
    ) -> Arc<Self> {
        Self::with_executor(
            Arc::new(crate::large_stack::post_to_worker),
            baseline,
            observer,
        )
    }

    /// A queue draining through `executor`.
    pub fn with_executor(
        executor: Executor,
        baseline: Arc<Mutex<Option<GuiState>>>,
        observer: Arc<dyn EvalObserver>,
    ) -> Arc<Self> {
        Arc::new(Self {
            executor,
            publisher: SnapshotPublisher::new(baseline, Arc::clone(&observer)),
            observer,
            state: Mutex::new(QueueState::default()),
        })
    }

    /// Accept `request` and return the ticket its reply arrives on. Never
    /// blocks and never runs the job itself.
    pub fn submit<T>(self: &Arc<Self>, request: EvalRequest<T>) -> EvalTicket<T> {
        self.accept(request.work);
        request.ticket
    }

    fn accept(self: &Arc<Self>, work: Work) {
        let mut state = self.lock_state();
        let work = match work {
            Work::Edit(identity, job) => {
                if !state.ledger.admit(&identity) {
                    job.supersede();
                    return;
                }
                Work::Edit(identity, job)
            }
            work => work,
        };
        if let Err(error) = self.ensure_drainer(&mut state) {
            work.into_job()
                .fail(format!("the evaluation could not be scheduled: {error}"));
            return;
        }
        let was_idle = state.outstanding == 0;
        state.enqueue(work);
        if was_idle && state.outstanding > 0 {
            self.report(EvalActivity::Evaluating);
        }
    }

    fn ensure_drainer(self: &Arc<Self>, state: &mut QueueState) -> std::io::Result<()> {
        if !state.draining {
            let queue = Arc::clone(self);
            (self.executor)(Box::new(move || queue.drain()))?;
            state.draining = true;
        }
        Ok(())
    }

    /// The drainer: run queued entries one at a time until none are left.
    fn drain(self: Arc<Self>) {
        let _unwind = DrainerUnwind(&self);
        while let Some(entry) = self.next_entry() {
            self.process(entry);
        }
    }

    /// Hand the queue on after its drainer unwound: post a new drainer for what
    /// is still queued — or, when none can be posted, resolve all of it `Err` —
    /// and recount `outstanding`, since nothing is running any more.
    fn recover_from_unwound_drainer(self: &Arc<Self>) {
        let mut state = self.lock_state();
        state.draining = false;
        if !state.pending.is_empty()
            && let Err(error) = self.ensure_drainer(&mut state)
        {
            let message = format!("the evaluation could not be rescheduled: {error}");
            for entry in std::mem::take(&mut state.pending) {
                entry.into_job().fail(message.clone());
            }
        }
        let was_busy = state.outstanding > 0;
        state.outstanding = state
            .pending
            .iter()
            .filter(|entry| entry.generation().is_some())
            .count();
        if was_busy && state.outstanding == 0 {
            self.report(EvalActivity::Idle);
        }
    }

    fn next_entry(&self) -> Option<Entry> {
        let mut state = self.lock_state();
        let entry = state.pending.pop_front();
        if entry.is_none() {
            state.draining = false;
        }
        entry
    }

    fn process(&self, entry: Entry) {
        let generation = entry.generation();
        let ran = std::panic::catch_unwind(AssertUnwindSafe(|| {
            entry.into_job().run(&mut |state| {
                if let Some(generation) = generation {
                    self.publisher.publish(generation, state);
                }
            });
        }));
        if let Err(payload) = ran {
            tracing::warn!(
                "publishing an evaluation panicked: {}",
                panic_payload_message(&*payload)
            );
        }
        if generation.is_some() {
            let mut state = self.lock_state();
            state.outstanding -= 1;
            if state.outstanding == 0 {
                self.report(EvalActivity::Idle);
            }
        }
    }

    fn report(&self, activity: EvalActivity) {
        let reported = std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.observer.activity(activity);
        }));
        if let Err(payload) = reported {
            tracing::warn!(
                "reporting evaluation activity panicked: {}",
                panic_payload_message(&*payload)
            );
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
