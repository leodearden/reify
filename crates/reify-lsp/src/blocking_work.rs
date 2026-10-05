//! Where the language server runs the blocking work of its definition,
//! prepareRename, rename and references handlers: parsing, compiling and
//! reading imported files.
//!
//! The embedder chooses the placement. The stdio server keeps
//! [`BlockingWorkPlacement::BlockingPool`], so that work never occupies an
//! async worker it shares with other requests. An embedder that already runs
//! [`InProcessLsp::handle_request`](crate::bridge::InProcessLsp::handle_request)
//! on a thread it chose, such as the GUI's large-stack LSP lane, picks
//! [`BlockingWorkPlacement::CallingThread`] so the work runs on that thread too.
//!
//! Both placements share one failure contract: a panic in the work is logged
//! once and resolves `None`; it never unwinds the awaiting request.

/// Where [`ReifyLanguageServer`](crate::server::ReifyLanguageServer) runs its
/// handlers' blocking work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockingWorkPlacement {
    /// Hand the work to tokio's blocking thread pool.
    #[default]
    BlockingPool,
    /// Run the work inline on the thread polling the handler.
    CallingThread,
}

impl BlockingWorkPlacement {
    /// Run `work` per this placement. `None` means the work panicked; the
    /// panic is logged under `label`.
    pub(crate) async fn run<T: Send + 'static>(
        self,
        label: &'static str,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let outcome = match self {
            Self::BlockingPool => tokio::task::spawn_blocking(work)
                .await
                .map_err(|join_error| join_error.to_string()),
            Self::CallingThread => std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .map_err(|payload| reify_core::panic_payload_to_string(payload.as_ref())),
        };
        outcome
            .inspect_err(|reason| tracing::error!("{label} blocking work failed: {reason}"))
            .ok()
    }
}

/// A test fixture that tells the two placements apart without reaching into
/// the server: a runtime whose blocking pool cannot run anything.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use std::cell::Cell;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::mpsc;
    use std::task::Poll;

    /// A current-thread tokio runtime whose only blocking thread is occupied
    /// until [`SaturatedBlockingPool::release`] (or drop).
    ///
    /// Inside [`SaturatedBlockingPool::block_on`], every `spawn_blocking`
    /// queues instead of running, so a future that completes there before
    /// `release` provably never waited on the blocking pool.
    pub struct SaturatedBlockingPool {
        runtime: tokio::runtime::Runtime,
        release: Cell<Option<mpsc::Sender<()>>>,
    }

    impl SaturatedBlockingPool {
        /// Build the runtime and return once its single blocking thread is
        /// running the occupying task.
        pub fn new() -> Self {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .max_blocking_threads(1)
                .build()
                .expect("build the saturated-pool runtime");
            let (release, released) = mpsc::channel::<()>();
            let (occupied, is_occupied) = mpsc::channel::<()>();
            runtime.spawn_blocking(move || {
                occupied.send(()).expect("the fixture awaits occupation");
                let _ = released.recv();
            });
            is_occupied
                .recv()
                .expect("the occupying blocking task started");
            Self {
                runtime,
                release: Cell::new(Some(release)),
            }
        }

        /// Drive `future` to completion on the saturated runtime.
        pub fn block_on<F: Future>(&self, future: F) -> F::Output {
            self.runtime.block_on(future)
        }

        /// Free the pool's blocking thread. Idempotent, and callable from
        /// inside a future driven by [`SaturatedBlockingPool::block_on`].
        pub fn release(&self) {
            self.release.take();
        }
    }

    impl Default for SaturatedBlockingPool {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for SaturatedBlockingPool {
        /// Release before the runtime drops, so a failing test never hangs
        /// in runtime shutdown waiting for the occupying task.
        fn drop(&mut self) {
            self.release();
        }
    }

    /// Poll `future` exactly once.
    pub async fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
        std::future::poll_fn(|cx| Poll::Ready(Pin::new(&mut *future).poll(cx))).await
    }
}

#[cfg(test)]
mod tests {
    use super::BlockingWorkPlacement;
    use super::test_support::{SaturatedBlockingPool, poll_once};

    #[tokio::test]
    async fn calling_thread_runs_the_work_on_the_polling_thread() {
        let caller = std::thread::current().id();
        let ran_on = BlockingWorkPlacement::CallingThread
            .run("probe", || std::thread::current().id())
            .await;
        assert_eq!(ran_on, Some(caller));
    }

    #[tokio::test]
    async fn blocking_pool_runs_the_work_off_the_polling_thread() {
        let caller = std::thread::current().id();
        let ran_on = BlockingWorkPlacement::BlockingPool
            .run("probe", || std::thread::current().id())
            .await
            .expect("the probe work does not panic");
        assert_ne!(ran_on, caller);
    }

    #[tokio::test]
    async fn a_panicking_work_resolves_none_under_every_placement() {
        for placement in [
            BlockingWorkPlacement::BlockingPool,
            BlockingWorkPlacement::CallingThread,
        ] {
            let outcome = placement
                .run("probe", || -> u32 {
                    panic!("simulated panic in blocking work (expected by this test)")
                })
                .await;
            assert_eq!(outcome, None, "placement {placement:?}");
        }
    }

    /// A first poll of a fresh `spawn_blocking` is Pending even on an idle
    /// pool, so the fixture's claim is only observable after giving an idle
    /// pool ample time to have run the work.
    const IDLE_POOL_WOULD_HAVE_RUN_IT_BY: std::time::Duration =
        std::time::Duration::from_millis(500);

    #[test]
    fn saturated_blocking_pool_runs_no_blocking_work_until_released() {
        let pool = SaturatedBlockingPool::new();
        pool.block_on(async {
            let mut queued = tokio::task::spawn_blocking(|| 7u32);
            assert!(poll_once(&mut queued).await.is_pending());
            tokio::time::sleep(IDLE_POOL_WOULD_HAVE_RUN_IT_BY).await;
            assert!(
                poll_once(&mut queued).await.is_pending(),
                "blocking work ran while the fixture claimed the pool's only thread was occupied"
            );
            pool.release();
            assert_eq!(queued.await.unwrap(), 7);
        });
    }
}
