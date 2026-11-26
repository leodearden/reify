//! Structure by declaration (contract 7.1-7.4): a path below a root is
//! worked out by reading its parent's answer, and a path nobody declares
//! answers absent.

use std::any::type_name;

use reify_eval_core::{Answer, Declaration, Failure, Kind, Outcome, Turn};

use crate::toy::{self, Cell};

#[test]
fn a_path_below_a_node_whose_kind_declares_no_children_answers_absent() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);
    let (total, below) = (toy::at("total"), toy::below("total", &["x"]));

    let request = core.evaluate(&snapshot, toy::demand([&total, &below]));
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    let answer = toy::answer_at(&events, &request, &total);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    let answer = toy::answer_at(&events, &request, &below);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
    assert_eq!(core.counters().paths_resolved(), 2);
}

#[test]
fn a_path_below_a_root_with_no_input_answers_absent() {
    let mut core = toy::core();
    let snapshot = core.snapshot([]);
    let below = toy::below("missing", &["x"]);

    let request = core.evaluate(&snapshot, toy::demand([&below]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &below);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
}

#[test]
fn a_path_below_a_root_whose_input_is_not_a_declaration_answers_that_failure() {
    let mut core = toy::core();
    let snapshot = core.snapshot([toy::root("seven", 7_i64)]);
    let below = toy::below("seven", &["x"]);

    let request = core.evaluate(&snapshot, toy::demand([&below]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &below);
    let wrong_type = Failure::WrongType {
        expected: type_name::<Declaration>(),
        found: type_name::<i64>(),
    };
    assert_eq!(toy::failed(answer), wrong_type);
}

#[test]
fn a_demanded_path_already_answered_in_the_snapshot_is_told_without_resolving_it_again() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
    ]);
    let below = toy::below("total", &["x"]);

    let first = core.evaluate(&snapshot, toy::demand([&below]));
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &first, &below);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
    assert_eq!(core.counters().paths_resolved(), 2);

    let second = core.evaluate(&snapshot, toy::demand([&below]));
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &second, &below);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
    assert_eq!(core.counters().paths_resolved(), 2);
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}
