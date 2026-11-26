//! When no body can run and some are waiting, each request that needs them
//! ends with an error naming them (PRD 12.2 β; ε replaces this with the
//! cycle rule).

use reify_eval_core::{
    Answer, Core, Event, Kind, Outcome, Path, RequestError, RequestId, Snapshot, Turn,
};

use crate::toy::{self, Cell};

/// A core with scopes and a snapshot of root scope R with members `a` and
/// `b`, cells each reading the other, and `c`, a cell over an input.
fn ring() -> (Core, Snapshot) {
    let mut core = toy::with_scope().build();
    let mut inputs = vec![toy::named("two", 2_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("a", toy::cell_over([toy::sibling(&["b"])])),
            ("b", toy::cell_over([toy::sibling(&["a"])])),
            ("c", toy::cell(&["two"])),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    (core, snapshot)
}

/// The errors among `events` that ended `request`, in order.
fn endings(events: &[Event], request: RequestId) -> Vec<RequestError> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Ended {
                request: ended,
                error,
                ..
            } if *ended == request => Some(error.clone()),
            _ => None,
        })
        .collect()
}

fn waiting_on(paths: &[&Path]) -> RequestError {
    RequestError::NothingCanRun {
        waiting: paths.iter().map(|path| (*path).clone()).collect(),
    }
}

#[test]
fn a_request_whose_bodies_wait_on_each_other_ends_with_an_error_naming_them() {
    let (mut core, snapshot) = ring();
    let [a, b, c] = ["a", "b", "c"].map(|name| toy::below("R", &[name]));

    let request = core.evaluate(&snapshot, toy::demand([&a, &c]));
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    let told: Vec<_> = events
        .iter()
        .filter(|event| match event {
            Event::Answered { request: to, .. } | Event::Ended { request: to, .. } => {
                *to == request.id()
            }
            Event::Progress { .. } => false,
        })
        .collect();
    assert_eq!(told.len(), 2, "{told:?}");
    match told[0] {
        Event::Answered { path, answer, .. } => {
            assert_eq!(*path, c);
            assert_eq!(
                answer.clone().downcast::<i64>(),
                Answer::Final(Outcome::of(2))
            );
        }
        other => panic!("expected c's answer first, got {other:?}"),
    }
    match told[1] {
        Event::Ended { snapshot: on, .. } => assert_eq!(*on, snapshot.id()),
        other => panic!("expected the request to end, got {other:?}"),
    }
    assert_eq!(endings(&events, request.id()), [waiting_on(&[&a, &b])]);
    assert_eq!(core.counters().body_runs(Cell::ID), 3);

    core.end(request);
    assert!(core.take_events().is_empty());
}

#[test]
fn a_live_request_that_wants_no_stuck_body_is_not_ended() {
    let (mut core, snapshot) = ring();
    let [a, c] = ["a", "c"].map(|name| toy::below("R", &[name]));

    let stuck = core.evaluate(&snapshot, toy::demand([&a]));
    let fine = core.evaluate(&snapshot, toy::demand([&c]));
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    assert_eq!(endings(&events, stuck.id()).len(), 1);
    assert!(endings(&events, fine.id()).is_empty());
    let answer = toy::the_answer(&events, &fine, &c);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
}

#[test]
fn a_body_that_reads_its_own_path_is_named_waiting() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(toy::root_scope(
        "R",
        toy::members([("a", toy::cell_over([toy::sibling(&["a"])]))]),
    ));
    let a = toy::below("R", &["a"]);

    let request = core.evaluate(&snapshot, toy::demand([&a]));
    assert_eq!(core.turn(), Turn::Idle);

    assert_eq!(
        endings(&core.take_events(), request.id()),
        [waiting_on(&[&a])]
    );
}

#[test]
fn a_stuck_request_does_not_stop_turn_wait_or_run_from_returning() {
    let (mut core, snapshot) = ring();
    let a = toy::below("R", &["a"]);

    let request = core.evaluate(&snapshot, toy::demand([&a]));
    core.run();
    assert_eq!(core.wait(), Turn::Idle);
    assert_eq!(core.turn(), Turn::Idle);

    assert_eq!(endings(&core.take_events(), request.id()).len(), 1);
}
