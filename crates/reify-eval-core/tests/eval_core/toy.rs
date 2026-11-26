//! What the tests share: host-side helpers for building a core, making
//! snapshot inputs and reading the answers a request received.

use reify_eval_core::{
    Answer, Builder, Core, Erased, Event, Failure, Input, InputKey, Outcome, OutsideFailure, Path,
    Request, Root, Value,
};

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
