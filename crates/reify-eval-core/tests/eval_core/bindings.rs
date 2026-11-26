//! What a body knows of its declaration: only what its reads return
//! (contract 2.1, 2.5).

use std::fmt::Debug;
use std::sync::Arc;

use reify_eval_core::{
    Answer, Core, Cx, Declaration, Event, Kind, KindId, Outcome, Path, Produced, Ref, Turn, Value,
};

use crate::toy;

/// The i64s at "arg0", "arg1", … until one reads absent.
struct List;

impl Kind for List {
    type Out = Vec<i64>;
    const ID: KindId = KindId("toy.list");

    async fn run(cx: &Cx) -> Produced<Vec<i64>> {
        let mut list = Vec::new();
        for index in 0_u32.. {
            match cx.read::<i64>(&Ref::binding(format!("arg{index}"))).await {
                Outcome::Value(value) => list.push(*value),
                Outcome::Absent => break,
                Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            }
        }
        Produced::Outcome(Outcome::of(list))
    }
}

/// The i64 at "arg", or where that reads absent, the one at "default".
struct Fallback;

impl Kind for Fallback {
    type Out = i64;
    const ID: KindId = KindId("toy.fallback");

    async fn run(cx: &Cx) -> Produced<i64> {
        match cx.read::<i64>(&Ref::binding("arg")).await {
            Outcome::Absent => Produced::Outcome(cx.read::<i64>(&Ref::binding("default")).await),
            read => Produced::Outcome(read),
        }
    }
}

/// The one value the core told for `at` since events were last taken.
fn value_at<T: Value + Debug>(core: &mut Core, at: &Path) -> Arc<T> {
    let events = core.take_events();
    let mut values = Vec::new();
    for event in &events {
        if let Event::Answered { path, answer, .. } = event
            && path == at
        {
            values.push(answer.clone().downcast::<T>());
        }
    }
    match values.as_slice() {
        [Answer::Final(Outcome::Value(value))] => Arc::clone(value),
        other => panic!("expected one value at {at:?}, got {other:?}"),
    }
}

#[test]
fn a_body_learns_its_declaration_only_through_recorded_reads() {
    let list = |args: &[&str]| {
        args.iter()
            .enumerate()
            .fold(Declaration::new(List::ID), |list, (index, arg)| {
                list.bind(format!("arg{index}"), toy::sibling(&[arg]))
            })
    };
    let members = toy::members([
        ("a", toy::cell(&["1"])),
        ("b", toy::cell(&["2"])),
        ("c", toy::cell(&["3"])),
        ("x", toy::cell(&["10"])),
        ("y", toy::cell(&["20"])),
        ("P1", list(&["a", "b"])),
        ("P2", list(&["a", "b", "c"])),
        (
            "A",
            Declaration::new(Fallback::ID).bind("arg", toy::sibling(&["x"])),
        ),
        (
            "B",
            Declaration::new(Fallback::ID).bind("default", toy::sibling(&["y"])),
        ),
    ]);
    let order = ["P1", "P2", "A", "B"];

    for order in [order.to_vec(), order.iter().rev().copied().collect()] {
        let mut core = toy::with_scope()
            .kind::<List>()
            .and_then(|builder| builder.kind::<Fallback>())
            .expect("distinct kinds")
            .build();
        let mut inputs: Vec<_> = [1_i64, 2, 3, 10, 20]
            .map(|value| toy::named(&value.to_string(), value))
            .into();
        inputs.extend(toy::root_scope("R", members.clone()));
        let snapshot = core.snapshot(inputs);

        for name in &order {
            let at = toy::below("R", &[name]);
            let _request = core.evaluate(&snapshot, toy::demand([&at]));
            assert_eq!(core.turn(), Turn::Idle);
            match *name {
                "P1" => assert_eq!(*value_at::<Vec<i64>>(&mut core, &at), [1, 2]),
                "P2" => assert_eq!(*value_at::<Vec<i64>>(&mut core, &at), [1, 2, 3]),
                "A" => assert_eq!(*value_at::<i64>(&mut core, &at), 10),
                _ => assert_eq!(*value_at::<i64>(&mut core, &at), 20),
            }
        }
        assert_eq!(core.counters().body_runs(List::ID), 2, "order {order:?}");
        assert_eq!(
            core.counters().body_runs(Fallback::ID),
            2,
            "order {order:?}"
        );
    }
}
