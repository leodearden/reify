//! What the tests share: the toy kinds, and host-side helpers for building
//! a core, making snapshot inputs and reading the answers a request received.

use reify_eval_core::{
    Answer, Binding, Builder, Core, Cx, Declaration, Erased, Event, Failure, Input, InputKey, Kind,
    KindId, Outcome, OutsideFailure, Path, Produced, Ref, Request, Root, Value,
};

/// Adds the i64s bound at "0", "1", … until a binding reads absent. A read
/// that fails ends the body with that failure as its outcome.
pub struct Cell;

impl Kind for Cell {
    type Out = i64;
    const ID: KindId = KindId("toy.cell");

    async fn run(cx: &Cx) -> Produced<i64> {
        let mut sum = 0;
        for index in 0_u32.. {
            match cx.read::<i64>(&Ref::binding(index.to_string())).await {
                Outcome::Value(value) => sum += *value,
                Outcome::Absent => break,
                Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            }
        }
        Produced::Outcome(Outcome::of(sum))
    }
}

/// A cell over the host-named inputs `names`, bound at "0", "1", … in order.
pub fn cell(names: &[&str]) -> Declaration {
    names.iter().enumerate().fold(Declaration::new(Cell::ID), |declaration, (index, name)| {
        declaration.bind(index.to_string(), Binding::Input(InputKey::named(*name)))
    })
}

pub fn with_cell() -> Builder {
    Builder::new().kind::<Cell>().expect("a fresh builder has no kinds")
}

pub fn core() -> Core {
    Builder::new().build()
}

/// The input that declares root `name`.
pub fn root<T: Value>(name: &str, declaration: T) -> (InputKey, Input) {
    (InputKey::root(name), Input::of(declaration))
}

/// The host-named input `name`.
pub fn named<T: Value>(name: &str, value: T) -> (InputKey, Input) {
    (InputKey::named(name), Input::of(value))
}

/// Every answer among `events` that went to `request`, in the order given.
pub fn answers(events: &[Event], request: &Request) -> Vec<(Path, Answer<Erased>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Answered { request: to, path, answer } if *to == request.id() => {
                Some((path.clone(), answer.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The one answer `request` received, which must be for root `name`.
pub fn the_answer(events: &[Event], request: &Request, name: &str) -> Answer<Erased> {
    let mut answers = answers(events, request);
    assert_eq!(answers.len(), 1, "one answer to the request: {answers:?}");
    let (path, answer) = answers.remove(0);
    assert_eq!(path, Path::root(Root::named(name)));
    answer
}

/// The failure a final answer holds.
pub fn failed(answer: Answer<Erased>) -> Failure {
    match answer {
        Answer::Final(Outcome::Failure(failure)) => failure,
        other => panic!("expected a final failure, got {other:?}"),
    }
}

/// The failure outside the inputs a pending answer carries.
pub fn pending(answer: Answer<Erased>) -> OutsideFailure {
    match answer {
        Answer::Pending(failure) => failure,
        other => panic!("expected a pending answer, got {other:?}"),
    }
}
