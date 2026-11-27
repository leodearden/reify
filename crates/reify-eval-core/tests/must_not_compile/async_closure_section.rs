use reify_eval_core::Cx;

pub async fn body(cx: &Cx) {
    let _ = cx.exclusive(async |numbers: &mut Vec<i64>, _| {
        std::future::ready(()).await;
        numbers.len()
    });
}

fn main() {}
