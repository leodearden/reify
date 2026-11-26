//! The index of a snapshot: within a snapshot a path is answered once, and
//! a result is kept while a live snapshot's index points at it (contract
//! 4.4, 5.5). Every count of live boxes is read after a drain.

use reify_eval_core::{Answer, Kind, Outcome, Turn};

use crate::toy::{self, BoxShape, Cell, Watch};

#[test]
fn a_second_request_on_a_snapshot_is_answered_from_its_index_without_running_the_body_again() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);

    let first = core.evaluate(&snapshot, toy::roots(&["total"]));
    assert_eq!(core.turn(), Turn::Idle);
    let answered = toy::the_answer(&core.take_events(), &first, &toy::at("total"));
    assert_eq!(answered.downcast::<i64>(), Answer::Final(Outcome::of(5)));

    let second = core.evaluate(&snapshot, toy::roots(&["total"]));
    assert_eq!(core.turn(), Turn::Idle);
    let again = toy::the_answer(&core.take_events(), &second, &toy::at("total"));
    assert_eq!(again.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}

#[test]
fn a_result_is_kept_while_a_live_snapshot_s_index_points_at_it() {
    let watch = Watch::default();
    let mut core = toy::ungated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let request = core.evaluate(&snapshot, toy::roots(&["box"]));
    core.run();
    let events = core.take_events();
    assert_eq!(
        toy::the_value::<BoxShape>(&events, &request, &toy::at("box")).volume,
        8
    );

    drop(events);
    core.drain();
    assert_eq!(
        watch.live_boxes(),
        1,
        "the snapshot is held and the request is live"
    );

    core.end(request);
    core.drain();
    assert_eq!(watch.live_boxes(), 1, "the host still holds the snapshot");

    drop(snapshot);
    core.drain();
    assert_eq!(watch.live_boxes(), 0);
}

#[test]
fn a_result_in_an_index_is_released_on_its_kernel_s_thread_when_the_core_is_dropped() {
    let watch = Watch::default();
    let mut core = toy::ungated(toy::with_toys(), &watch);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("box", toy::shape("side")),
    ]);
    let _request = core.evaluate(&snapshot, toy::roots(&["box"]));
    core.run();
    drop(core.take_events());

    drop(core);
    assert_eq!(watch.live_boxes(), 0);
    drop(snapshot);
}
