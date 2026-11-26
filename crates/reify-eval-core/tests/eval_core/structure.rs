//! Structure by declaration (contract 7.1-7.4): a path below a root is
//! worked out by reading its parent's answer, and a path nobody declares
//! answers absent.

use std::any::type_name;

use reify_eval_core::{
    Answer, Binding, Core, Declaration, Erased, Failure, Input, InputKey, Kind, Outcome, Path,
    Snapshot, Step, Turn,
};

use crate::toy::{self, Cell, Scope};

/// The answer at `at` to a fresh request for it alone on `snapshot`.
fn answer_alone(core: &mut Core, snapshot: &Snapshot, at: &Path) -> Answer<Erased> {
    let request = core.evaluate(snapshot, toy::demand([at]));
    assert_eq!(core.turn(), Turn::Idle);
    toy::the_answer(&core.take_events(), &request, at)
}

fn is_absent(answer: &Answer<Erased>) -> bool {
    matches!(answer, Answer::Final(Outcome::Absent))
}

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

#[test]
fn a_scope_s_member_resolves_by_reading_the_scope_s_result() {
    let mut core = toy::with_scope().build();
    let mut inputs = vec![
        toy::named("four", 4_i64),
        toy::named("s.members", toy::members([("m", toy::cell(&["four"]))])),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("s", toy::scope("s.members")),
            ("reader", toy::cell_over([toy::sibling(&["s", "m"])])),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    let (reader, m) = (toy::below("R", &["reader"]), toy::below("R", &["s", "m"]));

    let first = core.evaluate(&snapshot, toy::demand([&reader]));
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &first, &reader);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(4)));

    let second = core.evaluate(&snapshot, toy::demand([&m]));
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &second, &m);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(4)));
    assert_eq!(core.counters().body_runs(Scope::ID), 2);
    assert_eq!(core.counters().body_runs(Cell::ID), 2);
}

#[test]
fn a_body_that_reads_a_node_not_yet_computed_runs_once() {
    let mut core = toy::with_scope().build();
    let mut inputs = vec![toy::named("two", 2_i64), toy::named("three", 3_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            (
                "a",
                toy::cell_over([toy::sibling(&["b"]), toy::input("three")]),
            ),
            ("b", toy::cell(&["two"])),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    let a = toy::below("R", &["a"]);

    let request = core.evaluate(&snapshot, toy::demand([&a]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &a);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    assert_eq!(core.counters().body_runs(Cell::ID), 2);
}

#[test]
fn a_member_the_scope_does_not_declare_answers_absent() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(toy::root_scope("R", toy::members([("b", toy::cell(&[]))])));
    let nope = toy::below("R", &["nope"]);

    let request = core.evaluate(&snapshot, toy::demand([&nope]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &nope);
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
    assert_eq!(core.counters().body_runs(Cell::ID), 0);
}

#[test]
fn a_chain_of_100000_dependent_cells_is_evaluated_on_the_default_test_thread_stack() {
    const CELLS: usize = 100_000;
    let mut core = toy::with_scope().build();
    let names: Vec<String> = (0..CELLS).map(|i| format!("c{i}")).collect();
    let cells = names.iter().enumerate().map(|(i, name)| {
        let cell = match i {
            0 => toy::cell(&["seed"]),
            _ => toy::cell_over([toy::sibling(&[&names[i - 1]]), toy::input("one")]),
        };
        (name.as_str(), cell)
    });
    let mut inputs = vec![toy::named("seed", 7_i64), toy::named("one", 1_i64)];
    inputs.extend(toy::root_scope("R", toy::members(cells)));
    let snapshot = core.snapshot(inputs);
    let last = toy::below("R", &[&names[CELLS - 1]]);

    let request = core.evaluate(&snapshot, toy::demand([&last]));
    assert_eq!(core.turn(), Turn::Idle);

    let answer = toy::the_answer(&core.take_events(), &request, &last);
    let expected = 7 + i64::try_from(CELLS - 1).expect("the chain is short enough");
    assert_eq!(
        answer.downcast::<i64>(),
        Answer::Final(Outcome::of(expected))
    );
    let cells = u64::try_from(CELLS).expect("the chain is short enough");
    assert_eq!(core.counters().body_runs(Cell::ID), cells);
    assert_eq!(core.counters().body_runs(Scope::ID), 1);
}

#[test]
fn a_path_under_an_absent_guard_answers_absent() {
    let (guard, m) = (toy::at("g"), toy::below("g", &["m"]));
    for holds in [false, true] {
        let mut core = toy::with_structure().build();
        let snapshot = core.snapshot([
            toy::named("holds", holds),
            toy::named("four", 4_i64),
            toy::named("g.members", toy::members([("m", toy::cell(&["four"]))])),
            toy::root("g", toy::guard(toy::input("holds"), "g.members")),
        ]);

        let guarded = answer_alone(&mut core, &snapshot, &guard);
        let below = answer_alone(&mut core, &snapshot, &m);
        if holds {
            assert!(!is_absent(&guarded), "{guarded:?}");
            assert_eq!(below.downcast::<i64>(), Answer::Final(Outcome::of(4)));
        } else {
            assert!(is_absent(&guarded), "{guarded:?}");
            assert!(is_absent(&below), "{below:?}");
        }
    }
}

#[test]
fn a_guard_whose_condition_is_not_a_bool_fails_and_a_path_under_it_answers_that_failure() {
    let mut core = toy::with_structure().build();
    let snapshot = core.snapshot([
        toy::named("holds", 7_i64),
        toy::named("g.members", toy::members([("m", toy::cell(&[]))])),
        toy::root("g", toy::guard(toy::input("holds"), "g.members")),
    ]);
    let wrong_type = Failure::WrongType {
        expected: type_name::<bool>(),
        found: type_name::<i64>(),
    };

    let guarded = answer_alone(&mut core, &snapshot, &toy::at("g"));
    assert_eq!(toy::failed(guarded), wrong_type);
    let below = answer_alone(&mut core, &snapshot, &toy::below("g", &["m"]));
    assert_eq!(toy::failed(below), wrong_type);
}

#[test]
fn a_collection_declares_as_many_elements_as_its_count_reads() {
    let mut core = toy::with_structure().build();
    let snapshot = core.snapshot([
        toy::named("three", 3_i64),
        toy::named("v", 6_i64),
        toy::named("element", toy::cell(&["v"])),
        toy::root(
            "elems",
            toy::collection(toy::input("three"), toy::input("element")),
        ),
    ]);
    let element = |i| toy::at("elems").child(Step::index(i));

    for i in 0..3 {
        let answer = answer_alone(&mut core, &snapshot, &element(i));
        assert_eq!(
            answer.downcast::<i64>(),
            Answer::Final(Outcome::of(6)),
            "[{i}]"
        );
    }
    let beyond = answer_alone(&mut core, &snapshot, &element(3));
    assert!(is_absent(&beyond), "{beyond:?}");
}

#[test]
fn an_input_attached_to_a_scope_s_path_adds_a_member() {
    let x = toy::below("R", &["x"]);
    for attached in [false, true] {
        let mut core = toy::with_structure().build();
        let mut inputs = vec![toy::named("v", 9_i64)];
        inputs.extend(toy::root_scope("R", toy::members([])));
        if attached {
            let extra = toy::members([("x", toy::cell(&["v"]))]);
            inputs.push((InputKey::attached(toy::at("R"), "extra"), Input::of(extra)));
        }
        let snapshot = core.snapshot(inputs);

        let answer = answer_alone(&mut core, &snapshot, &x);
        if attached {
            assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(9)));
        } else {
            assert!(is_absent(&answer), "{answer:?}");
        }
    }
}

#[test]
fn a_ref_computed_from_an_earlier_answer_reads_the_element_it_names() {
    let pick = toy::below("R", &["pick"]);
    for (index, expected) in [(1_i64, Some(10_i64)), (5, None)] {
        let mut core = toy::with_structure().build();
        let element = toy::cell_over([Binding::Attached("value".into())]);
        let mut inputs = vec![
            toy::named("three", 3_i64),
            toy::named("index", index),
            toy::named("element", element),
        ];
        for i in 0..3_u64 {
            let at = toy::below("R", &["elems"]).child(Step::index(i));
            let value = 10 * i64::try_from(i).expect("small");
            inputs.push((InputKey::attached(at, "value"), Input::of(value)));
        }
        inputs.extend(toy::root_scope(
            "R",
            toy::members([
                (
                    "elems",
                    toy::collection(toy::input("three"), toy::input("element")),
                ),
                ("which", toy::cell(&["index"])),
                (
                    "pick",
                    toy::picker(toy::sibling(&["which"]), toy::sibling(&["elems"])),
                ),
            ]),
        ));
        let snapshot = core.snapshot(inputs);

        let answer = answer_alone(&mut core, &snapshot, &pick);
        match expected {
            Some(value) => assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(value))),
            None => assert!(is_absent(&answer), "{answer:?}"),
        }
    }
}
