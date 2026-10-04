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

    #[test]
    fn saturated_blocking_pool_runs_no_blocking_work_until_released() {
        let pool = SaturatedBlockingPool::new();
        pool.block_on(async {
            let mut queued = tokio::task::spawn_blocking(|| 7u32);
            assert!(
                poll_once(&mut queued).await.is_pending(),
                "blocking work ran while the fixture claimed the pool's only thread was occupied"
            );
            pool.release();
            assert_eq!(queued.await.unwrap(), 7);
        });
    }
}
