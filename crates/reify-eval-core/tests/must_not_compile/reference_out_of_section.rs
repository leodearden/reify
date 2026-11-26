use reify_eval_core::Cx;

pub async fn body(cx: &Cx) -> i64 {
    *cx.exclusive(|numbers: &mut Vec<i64>, _| &numbers[0]).await
}

fn main() {}
