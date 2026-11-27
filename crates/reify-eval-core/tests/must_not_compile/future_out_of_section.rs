use reify_eval_core::Cx;

pub async fn body(cx: &Cx) -> usize {
    cx.exclusive(|numbers: &mut Vec<i64>, _| async move { numbers.len() })
        .await
        .await
}

fn main() {}
