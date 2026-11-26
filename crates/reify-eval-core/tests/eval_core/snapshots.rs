//! The index of a snapshot: within a snapshot a path is answered once, and
//! a result is kept while a live snapshot's index points at it (contract
//! 4.4, 5.5); an edit makes a new snapshot (contract 2.4). Every count of
//! live boxes is read after a drain.

use reify_eval_core::{
    Answer, Core, Erased, Event, Input, InputKey, Kind, Outcome, Path, Snapshot, SnapshotId, Turn,
};

use crate::toy::{self, BoxShape, Cell, Watch};

/// The answer at `at` to a fresh request for it alone on `snapshot`, and
/// the snapshot the answer is tagged with.
fn answer_alone(core: &mut Core, snapshot: &Snapshot, at: &Path) -> (Answer<Erased>, SnapshotId) {
    let request = core.evaluate(snapshot, toy::demand([at]));
    assert_eq!(core.turn(), Turn::Idle);
    let events = core.take_events();
    let tagged = events
        .iter()
        .find_map(|event| match event {
            Event::Answered { snapshot, .. } => Some(*snapshot),
            _ => None,
        })
        .expect("an answer was given");
    (toy::the_answer(&events, &request, at), tagged)
}

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

#[test]
fn an_edit_makes_a_new_snapshot_whose_answers_follow_the_edited_input() {
    let mut core = toy::with_cell().build();
    let first = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
    ]);
    let second = first.edit([(InputKey::named("a"), Some(Input::of(3_i64)))]);
    assert_ne!(first.id(), second.id());
    let total = toy::at("total");

    let (answer, tagged) = answer_alone(&mut core, &first, &total);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
    assert_eq!(tagged, first.id());
    let (answer, tagged) = answer_alone(&mut core, &second, &total);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(3)));
    assert_eq!(tagged, second.id());
}

#[test]
fn the_snapshot_an_edit_came_from_keeps_its_answers_and_its_index() {
    let mut core = toy::with_cell().build();
    let first = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
    ]);
    let total = toy::at("total");
    let (answer, _) = answer_alone(&mut core, &first, &total);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
    let runs = core.counters().body_runs(Cell::ID);

    let _second = first.edit([(InputKey::named("a"), Some(Input::of(3_i64)))]);
    let (answer, _) = answer_alone(&mut core, &first, &total);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
    assert_eq!(core.counters().body_runs(Cell::ID), runs);
}

#[test]
fn an_edit_that_removes_an_input_makes_its_reads_absent() {
    let mut core = toy::with_cell().build();
    let first = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
    ]);
    let total = toy::at("total");

    let without_a = first.edit([(InputKey::named("a"), None)]);
    let (answer, _) = answer_alone(&mut core, &without_a, &total);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(0)));

    let without_root = first.edit([(InputKey::root("total"), None)]);
    let (answer, _) = answer_alone(&mut core, &without_root, &total);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
}

#[test]
fn an_edit_can_attach_an_input_to_a_path() {
    let mut core = toy::with_scope().build();
    let mut inputs = vec![toy::named("v", 9_i64)];
    inputs.extend(toy::root_scope("R", toy::members([])));
    let first = core.snapshot(inputs);
    let extra = toy::members([("x", toy::cell(&["v"]))]);
    let second = first.edit([(
        InputKey::attached(toy::at("R"), "extra"),
        Some(Input::of(extra)),
    )]);
    let x = toy::below("R", &["x"]);

    let (answer, _) = answer_alone(&mut core, &second, &x);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(9)));
    let (answer, _) = answer_alone(&mut core, &first, &x);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
}
