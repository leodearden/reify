//! A body's reads of the snapshot inputs its declaration binds.

use std::any::type_name;

use reify_eval_core::{Answer, Failure, Kind, Outcome, Root, Turn};

use crate::toy::{self, Cell};

#[test]
fn a_root_cell_adds_the_snapshot_inputs_its_declaration_binds() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);

    let request = core.evaluate(&snapshot, [Root::named("total")]);
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, "total");
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}

#[test]
fn a_read_at_the_wrong_type_gives_the_reader_a_failure_naming_both_types() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", String::from("two")),
        toy::root("total", toy::cell(&["a"])),
    ]);

    let request = core.evaluate(&snapshot, [Root::named("total")]);
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, "total");
    let wrong_type = Failure::WrongType {
        expected: type_name::<i64>(),
        found: type_name::<String>(),
    };
    assert_eq!(toy::failed(answer), wrong_type);
}

#[test]
fn two_live_requests_on_one_snapshot_each_receive_the_answer_of_a_root_they_both_demand() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);

    let first = core.evaluate(&snapshot, [Root::named("total")]);
    let second = core.evaluate(&snapshot, [Root::named("total")]);
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    for request in [&first, &second] {
        let answer = toy::the_answer(&events, request, "total");
        assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    }
    assert_eq!(core.counters().body_runs(Cell::ID), 1, "the running body is shared");
}
