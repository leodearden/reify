use std::sync::mpsc::Receiver;

use reify_eval_core::Cx;

pub async fn body(cx: &Cx, never_filled: Receiver<i64>) -> i64 {
    cx.exclusive(move |_: &mut Vec<i64>, _| never_filled.recv().unwrap())
        .await
}

fn main() {}
