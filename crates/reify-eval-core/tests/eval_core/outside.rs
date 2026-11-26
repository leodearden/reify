//! A failure outside the inputs reaches whatever needed it: a body that
//! read it is dropped, and nothing of it is kept (contract 9, PRD 13.2
//! point 13).

use reify_eval_core::{
    Answer, Core, Cx, Declaration, Kind, KindId, Outcome, OutsideFailure, Produced, Snapshot, Turn,
};

use crate::toy::{self, BoxShape, Cell, Pair, Scope, Shape, Watch};

const SPENT: OutsideFailure = OutsideFailure::OutOfMemory { resource: "toy" };

/// Fails in its own work, outside the inputs.
struct Spent;

impl Kind for Spent {
    type Out = i64;
    const ID: KindId = KindId("toy.spent");
    const VERSION: u32 = 1;

    async fn run(_: &Cx) -> Produced<i64> {
        Produced::Outside(SPENT)
    }
}

/// A core with the toy kinds, `Spent` and `Pair`, and a snapshot of root
/// scope R with members `bad` (spent), `reader` (a cell over `bad`),
/// `chained` (a cell over `reader`), `fine` (a cell over an input) and
/// `pair` (a pair over `fine` and `bad`).
fn model() -> (Core, Snapshot) {
    let mut core = toy::with_scope()
        .kind::<Spent>()
        .and_then(|builder| builder.kind::<Pair>())
        .expect("distinct kinds")
        .build();
    let pair = Declaration::new(Pair::ID)
        .bind("0", toy::sibling(&["fine"]))
        .bind("1", toy::sibling(&["bad"]));
    let mut inputs = vec![toy::named("two", 2_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("bad", Declaration::new(Spent::ID)),
            ("reader", toy::cell_over([toy::sibling(&["bad"])])),
            ("chained", toy::cell_over([toy::sibling(&["reader"])])),
            ("fine", toy::cell(&["two"])),
            ("pair", pair),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    (core, snapshot)
}

#[test]
fn a_body_that_reads_a_node_that_failed_outside_the_inputs_is_dropped_and_each_demanded_path_that_needed_it_is_pending()
 {
    let (mut core, snapshot) = model();
    let [reader, chained, fine] =
        ["reader", "chained", "fine"].map(|name| toy::below("R", &[name]));

    let answers = toy::answers_to(&mut core, &snapshot, &[&reader, &chained, &fine]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
    assert_eq!(toy::pending(answers[1].clone()), SPENT);
    assert_eq!(
        answers[2].clone().downcast::<i64>(),
        Answer::Final(Outcome::of(2))
    );
    assert_eq!(core.turn(), Turn::Idle);
}

#[test]
fn nothing_is_kept_for_a_body_that_read_a_failure_outside_the_inputs() {
    let (mut core, snapshot) = model();
    let reader = toy::below("R", &["reader"]);
    let answers = toy::answers_to(&mut core, &snapshot, &[&reader]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
    let runs = |core: &Core| {
        (
            core.counters().body_runs(Spent::ID),
            core.counters().body_runs(Cell::ID),
        )
    };
    let (spent, cells) = runs(&core);

    let answers = toy::answers_to(&mut core, &snapshot, &[&reader]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
    assert_eq!(runs(&core), (spent + 1, cells + 1));
}

#[test]
fn a_path_below_a_node_that_failed_outside_the_inputs_is_pending_with_that_failure() {
    let (mut core, snapshot) = model();
    let below = toy::below("R", &["bad", "x"]);
    let answers = toy::answers_to(&mut core, &snapshot, &[&below]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);

    let nobody = KindId("toy.nobody");
    let lost = core.snapshot([toy::root("lost", Declaration::new(nobody))]);
    let below = toy::below("lost", &["x"]);
    let answers = toy::answers_to(&mut core, &lost, &[&below]);
    assert_eq!(
        toy::pending(answers[0].clone()),
        OutsideFailure::UnknownKind { kind: "toy.nobody" }
    );
}

#[test]
fn a_read_all_with_one_target_failing_outside_the_inputs_drops_the_reader() {
    let (mut core, snapshot) = model();
    let pair = toy::below("R", &["pair"]);

    let answers = toy::answers_to(&mut core, &snapshot, &[&pair]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
}

/// A core with the toy kinds and `Pair`, and a snapshot of root scope R
/// whose member `p` is a pair over the places `lost` (a kind the core was
/// not built with) and `fine` (a cell over an input) under `R/prefix`;
/// with a prefix, scope `s` declares them.
fn unknown_beside_fine(prefix: &[&str]) -> (Core, Snapshot) {
    let mut core = toy::with_scope()
        .kind::<Pair>()
        .expect("distinct kinds")
        .build();
    let place = |name| toy::sibling(&[prefix, &[name]].concat());
    let pair = Declaration::new(Pair::ID)
        .bind("0", place("lost"))
        .bind("1", place("fine"));
    let under = toy::members([
        ("lost", Declaration::new(KindId("toy.nobody"))),
        ("fine", toy::cell(&["two"])),
    ]);
    let mut inputs = vec![toy::named("two", 2_i64)];
    let members = if prefix.is_empty() {
        let mut members = under;
        members.0.extend(toy::members([("p", pair)]).0);
        members
    } else {
        inputs.push(toy::named("s.members", under));
        toy::members([("s", toy::scope("s.members")), ("p", pair)])
    };
    inputs.extend(toy::root_scope("R", members));
    let snapshot = core.snapshot(inputs);
    (core, snapshot)
}

#[test]
fn a_reader_dropped_because_a_node_it_reads_has_an_unknown_kind_starts_none_of_its_other_reads() {
    for prefix in [&[][..], &["s"]] {
        let (mut core, snapshot) = unknown_beside_fine(prefix);
        let p = toy::below("R", &["p"]);

        let answers = toy::answers_to(&mut core, &snapshot, &[&p]);
        assert_eq!(
            toy::pending(answers[0].clone()),
            OutsideFailure::UnknownKind { kind: "toy.nobody" },
            "{prefix:?}"
        );
        assert_eq!(
            core.counters().body_runs(Cell::ID),
            0,
            "nobody wants `fine` once its only reader is dropped: {prefix:?}"
        );
    }
}

#[test]
fn work_only_a_reader_dropped_for_a_failure_outside_the_inputs_awaited_is_dropped_with_it() {
    let watch = Watch::default();
    let builder = toy::with_toys()
        .kind::<Scope>()
        .and_then(|builder| builder.kind::<Spent>())
        .and_then(|builder| builder.kind::<Pair>())
        .expect("distinct kinds");
    let mut core = toy::ungated(builder, &watch);
    let pair = Declaration::new(Pair::ID)
        .bind("0", toy::sibling(&["slow"]))
        .bind("1", toy::sibling(&["bad"]));
    let mut inputs = vec![toy::named("side", 2_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("bad", Declaration::new(Spent::ID)),
            ("slow", toy::shape("side")),
            ("pair", pair),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    let [pair, slow] = ["pair", "slow"].map(|name| toy::below("R", &[name]));

    let first = core.evaluate(&snapshot, toy::demand([&pair]));
    core.drain();
    let answer = toy::the_answer(&core.take_events(), &first, &pair);
    assert_eq!(toy::pending(answer), SPENT);
    assert_eq!(core.counters().body_runs(Shape::ID), 1);

    let second = core.evaluate(&snapshot, toy::demand([&slow]));
    core.run();
    let events = core.take_events();
    assert_eq!(
        toy::the_value::<BoxShape>(&events, &second, &slow).volume,
        8
    );
    assert_eq!(
        core.counters().body_runs(Shape::ID),
        2,
        "the shape only the dropped pair awaited was dropped unfinished, so it runs again"
    );
    core.end(first);
    core.end(second);
}
