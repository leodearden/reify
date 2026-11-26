use std::sync::{Arc, Mutex};

use reify_eval_core::{Cx, Kind, KindId, Outcome, Produced, Ref};

type SharedKernel = Arc<Mutex<Vec<i64>>>;

pub struct Keeper;

impl Kind for Keeper {
    type Out = usize;
    const ID: KindId = KindId("loophole.keeper");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<usize> {
        let kernel = cx
            .exclusive(|kernel: &mut SharedKernel, _| Arc::clone(kernel))
            .await;
        let _ = cx.read::<i64>(&Ref::binding("x")).await;
        let held = kernel.lock().unwrap().len();
        Produced::Outcome(Outcome::of(held))
    }
}

fn main() {}
