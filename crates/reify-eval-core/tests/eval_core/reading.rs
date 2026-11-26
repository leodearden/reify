//! A body's reads: of the snapshot inputs its declaration binds, and of
//! several places in one read (contract 2.5).

use std::any::type_name;

use std::sync::Arc;

use reify_eval_core::{
    Answer, Cx, Declaration, Failure, Kind, KindId, Outcome, Produced, Ref, Turn,
};

use crate::toy::{self, Cell, Members};

#[test]
fn a_root_cell_adds_the_snapshot_inputs_its_declaration_binds() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("total", toy::cell(&["a", "b"])),
    ]);

    let request = core.evaluate(&snapshot, toy::roots(&["total"]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("total"));
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

    let request = core.evaluate(&snapshot, toy::roots(&["total"]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &toy::at("total"));
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

    let first = core.evaluate(&snapshot, toy::roots(&["total"]));
    let second = core.evaluate(&snapshot, toy::roots(&["total"]));
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    for request in [&first, &second] {
        let answer = toy::the_answer(&events, request, &toy::at("total"));
        assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    }
    assert_eq!(
        core.counters().body_runs(Cell::ID),
        1,
        "the running body is shared"
    );
}

/// Reads "input", "unset", "node", "node" again and "wrong" in one read.
struct Gather;

impl Kind for Gather {
    type Out = Vec<Outcome<i64>>;
    const ID: KindId = KindId("toy.gather");

    async fn run(cx: &Cx) -> Produced<Self::Out> {
        let refs = ["input", "unset", "node", "node", "wrong"].map(Ref::binding);
        Produced::Outcome(Outcome::of(cx.read_all::<i64>(&refs).await))
    }
}

#[test]
fn read_all_gives_each_outcome_in_the_order_of_its_refs() {
    let mut core = toy::with_scope()
        .kind::<Gather>()
        .expect("distinct kinds")
        .build();
    let gather = Declaration::new(Gather::ID)
        .bind("input", toy::input("a"))
        .bind("node", toy::sibling(&["b"]))
        .bind("wrong", toy::sibling(&["s"]));
    let mut inputs = vec![
        toy::named("a", 1_i64),
        toy::named("two", 2_i64),
        toy::named("none", Members::default()),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("gather", gather),
            ("b", toy::cell(&["two"])),
            ("s", toy::scope("none")),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    let at = toy::below("R", &["gather"]);

    let request = core.evaluate(&snapshot, toy::demand([&at]));
    assert_eq!(core.turn(), Turn::Idle);

    let read = toy::the_value::<Vec<Outcome<i64>>>(&core.take_events(), &request, &at);
    let wrong_type = Failure::WrongType {
        expected: type_name::<i64>(),
        found: type_name::<Members>(),
    };
    assert_eq!(
        *read,
        [
            Outcome::of(1),
            Outcome::Absent,
            Outcome::of(2),
            Outcome::of(2),
            Outcome::Failure(wrong_type),
        ]
    );
    match (&read[2], &read[3]) {
        (Outcome::Value(first), Outcome::Value(again)) => assert!(Arc::ptr_eq(first, again)),
        other => panic!("expected the node's value twice, got {other:?}"),
    }
}

/// Reads no refs at all, and counts the outcomes it was given.
struct ReadsNothing;

impl Kind for ReadsNothing {
    type Out = usize;
    const ID: KindId = KindId("toy.reads_nothing");

    async fn run(cx: &Cx) -> Produced<usize> {
        Produced::Outcome(Outcome::of(cx.read_all::<i64>(&[]).await.len()))
    }
}

#[test]
fn read_all_of_no_refs_returns_an_empty_list() {
    let mut core = toy::with_cell()
        .kind::<ReadsNothing>()
        .expect("distinct kinds")
        .build();
    let snapshot = core.snapshot([toy::root("idle", Declaration::new(ReadsNothing::ID))]);

    let request = core.evaluate(&snapshot, toy::roots(&["idle"]));
    assert_eq!(core.turn(), Turn::Idle);

    let read = toy::the_value::<usize>(&core.take_events(), &request, &toy::at("idle"));
    assert_eq!(*read, 0);
}
