use std::sync::Mutex;

use reify_eval_core::{Cx, Kind, KindId, Outcome, Produced, Ref};

static SHARED: Mutex<i64> = Mutex::new(0);

pub struct Hoarder;

impl Kind for Hoarder {
    type Out = i64;
    const ID: KindId = KindId("negative.hoarder");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let shared = SHARED.lock().unwrap();
        let read = cx.read::<i64>(&Ref::binding("x")).await;
        drop(read);
        Produced::Outcome(Outcome::of(*shared))
    }
}

fn main() {}
