use reify_eval_core::{Cx, Ref};

pub async fn body(cx: &Cx) {
    cx.exclusive(|_: &mut Vec<i64>, _| drop(cx.read::<i64>(&Ref::binding("x"))))
        .await
}

fn main() {}
