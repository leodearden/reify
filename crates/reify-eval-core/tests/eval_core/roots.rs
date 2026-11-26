//! Where a root's declaration comes from (contract 7.5, decision D11): a
//! snapshot input under the root's name.

use std::any::type_name;

use reify_eval_core::{Answer, Declaration, Failure, KindId, Outcome, OutsideFailure, Root, Turn};

use crate::toy;

#[test]
fn a_demanded_root_with_no_input_answers_absent() {
    let mut core = toy::core();
    let snapshot = core.snapshot([]);

    let request = core.evaluate(&snapshot, [Root::named("missing")]);
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, "missing");
    assert!(matches!(answer, Answer::Final(Outcome::Absent)), "{answer:?}");
}

#[test]
fn a_root_input_that_is_not_a_declaration_answers_a_failure_naming_both_types() {
    let mut core = toy::core();
    let snapshot = core.snapshot([toy::root("seven", 7_i64)]);

    let request = core.evaluate(&snapshot, [Root::named("seven")]);
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, "seven");
    let wrong_type = Failure::WrongType {
        expected: type_name::<Declaration>(),
        found: type_name::<i64>(),
    };
    assert_eq!(toy::failed(answer), wrong_type);
}

#[test]
fn a_root_declaring_a_kind_the_core_was_not_built_with_is_pending_with_that_failure() {
    let mut core = toy::core();
    let nobody = KindId("toy.nobody");
    let snapshot = core.snapshot([toy::root("lost", Declaration::new(nobody))]);

    let request = core.evaluate(&snapshot, [Root::named("lost")]);
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, "lost");
    assert_eq!(toy::pending(answer), OutsideFailure::UnknownKind { kind: "toy.nobody" });
    assert_eq!(core.counters().body_runs(nobody), 0);
}
