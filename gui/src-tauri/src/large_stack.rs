//! Run engine and compiler work on OS threads with an explicit LARGE stack.
//!
//! Deeply-nested geometry can drive `reify_compiler`'s recursive compile past
//! the ~2 MiB stack of a tokio worker or a default `std` thread (task 5357).
//! Running that work on a [`COMPILE_STACK_SIZE`] stack adds headroom on top of
//! task 5337's `stacker::maybe_grow` growth and recursion-depth cap. A plain
//! `std` thread is also never a tokio runtime context, which the real OCCT
//! kernel needs: `OcctKernelHandle::execute()` uses `blocking_send`, which
//! panics inside one.
//!
//! # Two tiers
//!
//! A 256 MiB stack is far above glibc's thread-stack cache ceiling, so it is
//! never recycled: a per-call spawn pays a fresh `mmap`, guard-page `mprotect`
//! and `munmap` every time. Negligible against a compile, pure overhead per
//! keystroke. Hence two tiers:
//!
//! * **Per-call — [`spawn_on_large_stack`].** For work that does not go through
//!   a lane: `debug_server::run_on_engine`'s engine closures, which still bypass
//!   the evaluation queue (tkt_0RV0J0HK8TK4WRS6YJVYEFP93C), and a job that
//!   [`post`] or [`dispatch_async`] got back from a dead lane.
//! * **Persistent lanes — [`ENGINE_LANE`], [`LSP_LANE`] and [`LSP_POOL`].** A
//!   lane's consumer threads live for the process, so the per-call cost becomes
//!   a queue push; see [`Lane`].
//!   - The ENGINE lane is fed only by [`post_to_worker`], whose sole production
//!     caller is [`crate::eval_queue::EvalQueue`]. Posting never waits, so no
//!     submitter — least of all the GTK main thread — parks on engine work.
//!   - The LSP lanes serve `main.rs::lsp_request` →
//!     `lsp_bridge::lsp_request_on_worker`, which
//!     [`crate::lsp_bridge::lane_for_method`] routes to the single-consumer
//!     ORDERED [`LSP_LANE`] or to the [`LSP_POOL_SIZE`]-consumer QUERY POOL
//!     [`LSP_POOL`], both through [`dispatch_async`].
//!
//! # What is NOT covered
//!
//! 1. **Concurrency within the LSP lanes is BOUNDED, not unlimited.**
//!    Head-of-line blocking among queries is bounded at [`LSP_POOL_SIZE`]: the
//!    fifth simultaneous in-flight query queues. Notifications serialize
//!    against each other, which is a requirement; see [`Lane`]'s ORDERING.
//! 2. **No drop-cancellation.** An abandoned submission still runs to
//!    completion; see [`dispatch_async`]'s "Abandonment".
//! 3. **Same-document queries share one parse.**
//!    `crates/reify-lsp/src/document.rs`'s `DocumentState::parsed_module` holds a
//!    `std::sync::Mutex` across the whole `parse_with_stdlib` call, and every
//!    `didChange` replaces the document's parse cache. So the hover,
//!    `documentHighlight` and completion that one cursor move issues against ONE
//!    uri serialize on that lock right after a keystroke, each parking a pool
//!    consumer; item 1's bound holds exactly only across DIFFERENT documents.
//!    The fix, computing the parse outside the lock, is a `reify-lsp` change
//!    tracked as task #7272.
//!
//! # The degradation invariant
//!
//! Every submission that cannot get its large stack still RESOLVES, and no
//! degraded arm needs a resource the triggering condition would have denied:
//!
//! * No lane (the OS refused the 256 MiB mapping): [`post`] runs the job on a
//!   spawned default-stack thread — never inline, because a poster may be a
//!   tokio worker where OCCT's synchronous handle panics — and
//!   [`dispatch_async`] `.await`s the future in place.
//! * Dead lane (the queue hands the job back unrun): the job goes to
//!   [`spawn_on_large_stack`]. If that spawn fails too, [`post`] returns `Err`
//!   and [`dispatch_async`]'s awaiter gets a loud panic.
//!
//! The one shape that would not resolve — a job WAITING on work it queued to its
//! own lane — is rejected by [`assert_not_reentrant`]. The worst outcome
//! anywhere in this module is therefore a loud panic or an `Err`, never a silent
//! hang and never a nested runtime.

/// Stack size for every thread this module spawns: 256 MiB.
///
/// A thread stack is a virtual-address reservation committed page-by-page on
/// first touch, so this costs the pages actually used, not 256 MiB resident.
/// It is ~128x a tokio worker's 2 MiB default.
pub const COMPILE_STACK_SIZE: usize = 256 * 1024 * 1024;

/// Thread name for [`spawn_on_large_stack`]'s per-call thread, and for the
/// default-stack thread [`post`] falls back to when there is no lane.
///
/// The names in this module say which TIER and which LANE a backtrace,
/// `top -H` row or profiler capture is on. Each is held to Linux's 15-byte
/// `pthread_setname_np` limit by a `const` assertion, because `std` silently
/// drops a longer name.
pub const ENGINE_THREAD_NAME: &str = "reify-engine";
const _: () = assert!(
    ENGINE_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread name for [`ENGINE_LANE`]'s consumer.
pub const WORKER_THREAD_NAME: &str = "reify-engine-w";
const _: () = assert!(
    WORKER_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread name for [`LSP_LANE`]'s consumer — distinct from the engine lane's, so
/// a keystroke-path stall and a geometry-evaluation stall can be told apart.
pub const LSP_WORKER_THREAD_NAME: &str = "reify-lsp-w";
const _: () = assert!(
    LSP_WORKER_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread-name PREFIX for [`LSP_POOL`]'s consumers, `reify-lsp-p0` ..
/// `reify-lsp-p{LSP_POOL_SIZE-1}`. The assertion allows a two-digit index, so
/// raising [`LSP_POOL_SIZE`] cannot silently overrun the 15-byte limit.
pub const LSP_POOL_THREAD_PREFIX: &str = "reify-lsp-p";
const _: () = assert!(
    LSP_POOL_THREAD_PREFIX.len() + 2 <= 15,
    "the pool prefix plus a two-digit consumer index must fit Linux's 15-byte \
     pthread_setname_np limit"
);

/// How many consumers the LSP query pool runs: 4.
///
/// Enough for one slow workspace-wide `references` or `rename` to overlap the
/// hover / completion / documentHighlight / definition traffic of a single
/// cursor move. A fixed constant rather than
/// [`std::thread::available_parallelism`], so the head-of-line bound is the same
/// number on every machine.
pub(crate) const LSP_POOL_SIZE: usize = 4;
const _: () = assert!(
    LSP_POOL_SIZE >= 2,
    "a query pool of one consumer serializes every LSP query again"
);

/// Spawn `f` on a dedicated OS thread with a [`COMPILE_STACK_SIZE`] stack, named
/// [`ENGINE_THREAD_NAME`], WITHOUT blocking the caller.
///
/// The per-call tier. `f` is `'static`, so deliver any result through a channel
/// it captures. Thread-creation failure comes back as `Err` rather than running
/// `f` inline, so an async caller can map it to a structured error.
pub fn spawn_on_large_stack<F>(f: F) -> std::io::Result<std::thread::JoinHandle<()>>
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name(ENGINE_THREAD_NAME.to_string())
        .stack_size(COMPILE_STACK_SIZE)
        .spawn(f)
}

// ── Persistent large-stack worker (task 5772) ────────────────────────────────

/// A type-erased unit of work queued to a persistent lane. `'static` because the
/// queue outlives every submitter; any result travels over a channel the job
/// captures.
///
/// `pub(crate)` so tests can build a SYNTHETIC queue.
pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;

/// What a job sends back to its submitter: the value, or the panic payload its
/// body raised, so the submitter can [`std::panic::resume_unwind`] the ORIGINAL
/// panic.
type JobReply<T> = Result<T, Box<dyn std::any::Any + Send>>;

/// The submit end of a lane's job queue, TAGGED with the lane it feeds so
/// [`assert_not_reentrant`] can tell a self-submission (a wedge) from a
/// cross-lane one (legal).
///
/// The [`std::sync::Mutex`] is held only across a `send` of an already-boxed
/// job, so no user code runs under it.
pub(crate) struct JobSender {
    /// Which lane this queue feeds — the same `&'static str` that lane's
    /// consumers publish in [`CURRENT_LANE`].
    lane: &'static str,
    tx: std::sync::Mutex<std::sync::mpsc::Sender<Job>>,
}

impl JobSender {
    /// Wrap a lane's `Sender`, tagging it with that lane's name.
    ///
    /// `pub(crate)` so a test can build a SYNTHETIC sender (typically over an
    /// already-dropped `Receiver`) to provoke the `SendError` arms of
    /// [`post`] / [`dispatch_async`] deterministically.
    pub(crate) fn new(lane: &'static str, tx: std::sync::mpsc::Sender<Job>) -> Self {
        Self {
            lane,
            tx: std::sync::Mutex::new(tx),
        }
    }

    /// Push `job` onto the queue, or hand it BACK inside `SendError` when the
    /// consumer is gone. A poisoned guard is recovered: a `Sender` holds no
    /// invariant a panic could break.
    fn send(&self, job: Job) -> Result<(), std::sync::mpsc::SendError<Job>> {
        self.tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(job)
    }

    /// True when the CALLING thread is this queue's own lane thread.
    fn is_own_lane_thread(&self) -> bool {
        CURRENT_LANE.with(std::cell::Cell::get) == Some(self.lane)
    }
}

thread_local! {
    /// The name of the lane whose consumer this thread is — `None` on every
    /// other thread. Set once by the receive loop; a consumer drains its own
    /// queue for the process lifetime.
    static CURRENT_LANE: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// Panic if this submission would enqueue work onto the lane whose thread is
/// making it.
///
/// A job that submits to its own lane and WAITS for the reply can never be
/// answered by the consumer running it, which dequeues again only once the
/// outer job returns. On a single-consumer lane that wedges the lane and every
/// later submitter in the process. The rule is blanket for pools too: `size`
/// simultaneous self-submissions wedge a size-`size` pool, so a permissive rule
/// would be safe only for as long as few enough callers were in flight.
///
/// The panic fires inside the running job, so that job's own `catch_unwind`
/// re-raises it on its submitter and the lane survives. Cross-lane submission is
/// allowed: it lands on a different consumer.
fn assert_not_reentrant(sender: &JobSender) {
    assert!(
        !sender.is_own_lane_thread(),
        "re-entrant submission to the `{}` large-stack lane: a job running ON \
         that lane submitted to it again. The consumer running the outer job \
         cannot answer the inner one — it only returns to its dequeue loop after \
         the outer job returns, and the outer job is waiting for the inner. Run \
         the inner work inline, or submit it to another lane.",
        sender.lane
    );
}

/// One persistent large-stack lane: a NAME, a SIZE, and the queue feeding it.
///
/// Created lazily on first use and kept for the process. Every lane is an
/// instance of one mechanism — a 256 MiB stack per consumer, one FIFO queue,
/// `None` when no consumer started, a never-dropped `Sender` — and `size` is a
/// value it carries, not a second design.
///
/// # Why more than one lane
///
/// LSP dispatch never takes the engine mutex, so serializing it behind engine
/// work buys nothing, and would make a `textDocument/hover` queue behind an
/// in-flight `set_parameter` evaluation on the GUI's highest-frequency path.
///
/// # The LSP split: an ordered lane and a query pool
///
/// [`LSP_LANE`] keeps ONE consumer for the state-mutating and lifecycle
/// methods; [`LSP_POOL`] runs the read-only queries on [`LSP_POOL_SIZE`]
/// consumers. A workspace-wide `references` or `rename`, whose parse and compile
/// run ON the consumer driving it (see [`crate::lsp_bridge::LspBridge`]'s
/// "Where blocking work runs"), therefore holds one of N consumers rather than
/// the only one.
///
/// * **ORDERING.** Notifications are order-sensitive against each other:
///   applying `didChange` N+1 before N yields text neither side ever had. So
///   [`LSP_LANE`] is size 1 by requirement, and
///   [`crate::lsp_bridge::lane_for_method`] defaults every unrecognised method
///   to it.
/// * **REENTRANCY.** Every consumer of a pool publishes the same
///   [`CURRENT_LANE`] name, so [`assert_not_reentrant`] treats a pool exactly
///   like a single-consumer lane.
///
/// The ordering given up is query-versus-notification: a query may read text
/// older than a concurrently-processing `didChange`. That is staleness, never
/// corruption — `reify-lsp`'s own locks serialise the accesses — and it is
/// reachable in the shipped app. `gui/src/editor/Editor.tsx` debounces
/// `didChange`, and only rename (F2) and find-uses (Shift-F12) wait for it
/// (`flushPendingLspChange`); completion, hover, go-to-definition and occurrence
/// highlights fire independently, and a stale answer self-corrects on the next
/// request. A `rename` edit cannot land stale: the server version-stamps it and
/// `gui/src/editor/rename.ts` refuses a mismatched version. A client that awaits
/// each request reads its own writes (`lsp_lane_routing_tests`' (n)).
///
/// # One consumer or N
///
/// [`Lane::sender`] spawns `size` consumers over one shared
/// `Arc<Mutex<Receiver<Job>>>`; everything else — [`Job`], [`JobSender`],
/// [`post`], [`dispatch_async`], [`assert_not_reentrant`] — is the same for
/// every size.
///
/// * Dequeue is FIFO, because [`std::sync::mpsc`] delivers in send order. The
///   mutex only serialises `recv()` and is released before the job body runs,
///   so `size` bodies run at once. It is not fair, so on a size-N lane job
///   BODIES may start out of arrival order; only a size-1 lane gives strict
///   start order.
/// * A size-1 lane pays one uncontended lock per job, in exchange for a single
///   receive loop.
/// * A size-1 lane's consumer is named exactly `name`; a size-N lane's are
///   `{name}{i}`.
pub(crate) struct Lane {
    /// The consumer thread name; for a size-N lane, the PREFIX of `{name}{i}`.
    name: &'static str,
    /// How many consumers drain this lane's queue — its head-of-line bound.
    size: usize,
    /// How many consumers actually started; see [`Lane::started`].
    started: std::sync::atomic::AtomicUsize,
    /// The lazily-created queue. `None` records that no consumer could be
    /// spawned.
    queue: std::sync::OnceLock<Option<JobSender>>,
}

impl Lane {
    /// Declare a SINGLE-consumer lane. `const` so lanes can be `static`s; no
    /// thread is spawned until [`Lane::sender`] is first called.
    const fn new(name: &'static str) -> Self {
        Self::pool(name, 1)
    }

    /// Declare a lane with `size` consumers, bounding head-of-line blocking
    /// among the work routed to it at `size` rather than serializing it.
    ///
    /// Consumer `i` is named `{name}{i}` (exactly `name` when `size` is 1), so
    /// `name.len()` plus the widest index must fit Linux's 15-byte
    /// `pthread_setname_np` limit — `std` silently drops a longer name. Each
    /// production prefix carries a `const` assertion for that.
    ///
    /// A zero-size lane would be a queue nobody drains. Every call site is a
    /// `static` initialiser, so the assertion below is const-evaluated and
    /// fails the build.
    ///
    /// `pub(crate)` so a test can declare its own instance instead of parking
    /// consumers of a process-wide `static`.
    pub(crate) const fn pool(name: &'static str, size: usize) -> Self {
        assert!(
            size >= 1,
            "a lane needs at least one consumer; a size-0 lane's queue would \
             never be drained"
        );
        Self {
            name,
            size,
            started: std::sync::atomic::AtomicUsize::new(0),
            queue: std::sync::OnceLock::new(),
        }
    }

    /// How many consumers this lane DECLARES.
    ///
    /// Read only by tests (`lsp_lane_routing_tests`' (p)); the allow is scoped
    /// to `not(test)` so the lint still fires if the tests stop reading it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn size(&self) -> usize {
        self.size
    }

    /// How many consumers this lane actually STARTED: 0 until [`Lane::sender`]
    /// has run, then fixed. It falls short of [`Lane::size`] exactly when
    /// [`Lane::sender`] hit a partial spawn failure, which narrows the lane's
    /// concurrency bound without failing anything else.
    ///
    /// The `queue.get()` is the synchronisation, not a fast path: the store
    /// happens inside the `OnceLock` initialiser, so `Some` means this thread
    /// happens-after it and the `Relaxed` load sees the stored count, and `None`
    /// means 0 is the truth. Read only by tests, like [`Lane::size`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn started(&self) -> usize {
        if self.queue.get().is_none() {
            return 0;
        }
        self.started.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Lazily create this lane's consumers, yielding its queue — or `None` if
    /// not one consumer could be spawned.
    ///
    /// `None` is recorded EXPLICITLY rather than inferred from a dead channel: a
    /// leaked `Receiver` would make every `send` succeed into a queue nobody
    /// drains, which is a hang. A PARTIAL spawn failure still yields `Some`: the
    /// queue has a drainer, so every submission still completes on a large
    /// stack, with a smaller concurrency bound that [`Lane::started`] reports.
    /// The `Sender` lives in the `OnceLock` forever, so consumers park on an
    /// empty queue rather than exiting.
    ///
    /// The warnings use `eprintln!` because nothing in this workspace installs a
    /// `tracing` subscriber, so `tracing::warn!` would print nowhere.
    pub(crate) fn sender(&'static self) -> Option<&'static JobSender> {
        self.queue
            .get_or_init(|| {
                let (tx, rx) = std::sync::mpsc::channel::<Job>();
                let name = self.name;
                let size = self.size;
                // Shared so `size` consumers can drain ONE queue. The lock is
                // taken only across a dequeue, never across a job body.
                let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));

                let mut started = 0usize;
                for index in 0..size {
                    let thread_name = if size == 1 {
                        name.to_owned()
                    } else {
                        format!("{name}{index}")
                    };
                    let rx = std::sync::Arc::clone(&rx);
                    match std::thread::Builder::new()
                        .name(thread_name)
                        .stack_size(COMPILE_STACK_SIZE)
                        .spawn(move || {
                            // Every consumer of a lane publishes the SAME name,
                            // which is what `assert_not_reentrant` compares.
                            CURRENT_LANE.with(|l| l.set(Some(name)));
                            loop {
                                // Dequeue under the lock, then RELEASE it before
                                // running the job, so `size` bodies run at once.
                                // A job cannot poison this lock: its
                                // `catch_unwind` is inside the job, and the lock
                                // is not held while it runs.
                                let dequeued = {
                                    let guard = rx
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    guard.recv()
                                };
                                // `recv` errs only once the `Sender` in the
                                // `OnceLock` drops, which never happens.
                                let Ok(job) = dequeued else { break };
                                job();
                            }
                        }) {
                        Ok(_handle) => started += 1,
                        Err(e) => {
                            eprintln!(
                                "Warning: failed to spawn {name} lane consumer \
                                 {index} of {size} ({e})"
                            );
                        }
                    }
                }

                // Stored before either exit, so the degraded `None` and a
                // partial pool alike leave a truthful count behind.
                self.started
                    .store(started, std::sync::atomic::Ordering::Relaxed);

                if started == 0 {
                    // Nothing will drain the queue: record the degrade signal.
                    eprintln!(
                        "Warning: failed to spawn any {name} thread; that lane's \
                         work will run on a default-size stack instead"
                    );
                    return None;
                }
                if started < size {
                    eprintln!(
                        "Warning: the {name} lane started {started} of {size} \
                         consumers; its work still runs on a large stack, with a \
                         smaller concurrency bound"
                    );
                }
                Some(JobSender::new(name, tx))
            })
            .as_ref()
    }
}

/// The ENGINE lane, fed only by [`post_to_worker`]. Named [`WORKER_THREAD_NAME`].
pub(crate) static ENGINE_LANE: Lane = Lane::new(WORKER_THREAD_NAME);

/// The ORDERED LSP lane: the state-mutating and lifecycle methods, plus any
/// method `InProcessLsp::handle_request` does not recognise. Size 1 is a
/// correctness requirement; see [`Lane`]'s ORDERING. Which methods is
/// [`crate::lsp_bridge::lane_for_method`]'s to say.
pub(crate) static LSP_LANE: Lane = Lane::new(LSP_WORKER_THREAD_NAME);

/// The LSP QUERY POOL: the read-only query methods, on [`LSP_POOL_SIZE`]
/// consumers. They may run concurrently because every one is read-only against
/// server-side state. Which methods is [`crate::lsp_bridge::lane_for_method`]'s
/// to say.
pub(crate) static LSP_POOL: Lane = Lane::pool(LSP_POOL_THREAD_PREFIX, LSP_POOL_SIZE);

/// Queue `job` on the persistent ENGINE lane WITHOUT waiting for it, so the
/// calling thread is never parked on engine work. Deliver any result through a
/// channel the job captures. See [`post`] for the degraded arms.
pub fn post_to_worker(job: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    post(ENGINE_LANE.sender(), Box::new(job))
}

/// Queue `job` on `sender`'s lane without waiting for it — the lane-agnostic
/// seam behind [`post_to_worker`], shaped like [`dispatch_async`] so the
/// degraded arms are testable.
///
/// The job never runs inline on the caller, which may be a tokio worker where
/// OCCT's synchronous kernel handle panics. With no lane (the OS refused the
/// 256 MiB mapping) it runs on a spawned default-stack thread, since a second
/// 256 MiB request would be refused the same way; a job handed back by a dead
/// lane runs on [`spawn_on_large_stack`]. `Err` means no thread could be spawned
/// and the job was dropped unrun.
///
/// A panic in the job is caught and logged inside the job, so it never unwinds a
/// lane's receive loop. There is no reentrancy guard: posting never waits, so a
/// job may post to its own lane.
pub(crate) fn post(sender: Option<&JobSender>, job: Job) -> std::io::Result<()> {
    let contained: Job = Box::new(move || {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
            eprintln!(
                "Warning: a posted large-stack job panicked: {}",
                panic_payload_message(&*payload)
            );
        }
    });
    match sender {
        Some(sender) => match sender.send(contained) {
            Ok(()) => Ok(()),
            Err(std::sync::mpsc::SendError(job)) => spawn_on_large_stack(job).map(drop),
        },
        None => std::thread::Builder::new()
            .name(ENGINE_THREAD_NAME.to_string())
            .spawn(contained)
            .map(drop),
    }
}

/// The message a caught panic carried: `panic!("literal")` yields a `&str`
/// payload, a formatted `panic!` a `String`.
pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

/// Submit `fut` to `sender`'s lane and AWAIT its output, releasing the calling
/// tokio worker while a lane consumer computes — or, given `None`, simply
/// `.await` it here.
///
/// The LSP lanes' submission seam. `lsp_request` fires on effectively every
/// keystroke, and awaiting a [`tokio::sync::oneshot`] reply keeps it from
/// pinning a tokio worker for the whole round trip. The lane is a parameter so a
/// test can reach the degraded arms without a real `pthread_create` failure.
///
/// # Why a FUTURE, driven by the submitter's `Handle`
///
/// The degraded arms run in the submitting async frame, which is inside the
/// tauri runtime: a future can be `.await`ed there, but a closure with a
/// [`tokio::runtime::Handle::block_on`] baked in would panic "Cannot start a
/// runtime from within a runtime". On the lane, a consumer is a plain `std`
/// thread with no ambient runtime, and the future may need one —
/// `InProcessLsp`'s default `BlockingPool` placement calls
/// [`tokio::task::spawn_blocking`]. So the submitter's `Handle` is captured here
/// and moved into the job, and `Handle::block_on` installs the runtime context
/// on the consumer.
///
/// # Degradation: never lose a result, never hang an `.await`, never nest a runtime
///
/// * `None` lane (the OS refused the 256 MiB mapping), or no ambient runtime
///   ([`tokio::runtime::Handle::try_current`] is `Err`): `.await` the future
///   here. Neither arm needs a resource the triggering condition denies.
/// * `SendError(job)`: the queue handed the job back unrun. It carries a
///   `Handle::block_on`, so it must not run in this frame; it goes to
///   [`spawn_on_large_stack`]. If that spawn also fails the job is dropped,
///   `reply_rx` resolves `Err` at once, and the awaiter gets a loud panic.
///
/// A panic in `fut` is caught inside the job and re-raised with its ORIGINAL
/// payload on the awaiter, so a consumer's receive loop never unwinds.
/// Submitting to the lane the caller is itself running on panics; see
/// [`assert_not_reentrant`].
///
/// # Abandonment
///
/// Dropping this future drops only the `oneshot` receiver. The job still runs to
/// completion, its side effects happen, and its answer is discarded. Nothing in
/// the shipped app abandons a submission short of runtime teardown: an abandoned
/// tauri `invoke` detaches its command future rather than dropping it.
pub(crate) async fn dispatch_async<Fut, T>(sender: Option<&JobSender>, fut: Fut) -> T
where
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let Some(sender) = sender else {
        // No lane (already warned once, in the lane initialiser). Await the
        // future right here — a panic in it propagates naturally, so this arm
        // needs no forwarding of its own.
        return fut.await;
    };

    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        // Polled outside any tokio runtime, so there is nothing to hand the
        // lane thread as a driver — and equally nothing to nest. Await here.
        return fut.await;
    };

    // Rejected loudly rather than enqueued: a future submitted from a job
    // already running on this lane could only be driven after that job
    // returned (see `assert_not_reentrant`).
    assert_not_reentrant(sender);

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<JobReply<T>>();
    let job: Job = Box::new(move || {
        // The catch lives INSIDE the job, so the lane's receive loop can never
        // observe an unwind and cannot be killed by user code.
        // `AssertUnwindSafe` is sound because the job OWNS its captures and is
        // consumed by this call — nothing observes them after a panic — and the
        // payload is re-raised on the submitter below rather than swallowed.
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle.block_on(fut)));
        // A dropped receiver means the awaiting task is gone; nothing to report.
        let _ = reply_tx.send(outcome);
    });

    // The queue lock is released before the `.await` below — it is a std
    // `Mutex` held only across this push, inside `JobSender::send`.
    let send_result = sender.send(job);

    if let Err(std::sync::mpsc::SendError(job)) = send_result {
        // The lane's worker is gone and the job came back unrun. It carries a
        // `Handle::block_on`, so running it in THIS frame would panic — give it
        // a plain `std` thread, which is never a runtime context.
        if let Err(e) = spawn_on_large_stack(job) {
            // Nothing left that can legally run it. Dropping the job drops its
            // `reply_tx`, so the `.await` below resolves `Err` immediately and
            // reports loudly instead of hanging.
            eprintln!(
                "Warning: a large-stack lane's queue was dead and the recovery \
                 thread could not be spawned ({e}); the submitted job cannot be \
                 run"
            );
        }
    }

    match reply_rx.await {
        Ok(Ok(value)) => value,
        // Re-raise the ORIGINAL payload on the awaiting task, exactly as if the
        // future had been awaited inline.
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        // Reached only when the job was dropped unrun — i.e. both the lane and
        // the recovery thread were unavailable. A disconnected `oneshot`
        // resolves AT ONCE, so this is a loud failure, not a hung future.
        Err(_) => panic!(
            "a large-stack lane dropped a job without answering: its oneshot \
             reply channel disconnected before a result arrived"
        ),
    }
}
