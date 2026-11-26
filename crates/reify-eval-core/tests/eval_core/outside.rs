//! A failure outside the inputs reaches whatever needed it: a body that
//! read it is dropped, and nothing of it is kept (contract 9, PRD 13.2
//! point 13).

use reify_eval_core::{
    Answer, Core, Cx, Declaration, Erased, Kind, KindId, Outcome, OutsideFailure, Path, Produced,
    Snapshot, Turn,
};

use crate::toy::{self, Cell, Pair};

const SPENT: OutsideFailure = OutsideFailure::OutOfMemory { resource: "toy" };

/// Fails in its own work, outside the inputs.
struct Spent;

impl Kind for Spent {
    type Out = i64;
    const ID: KindId = KindId("toy.spent");

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

/// The answers to a fresh request for `paths` on `snapshot`, after a turn
/// that leaves nothing running.
fn answers_to(core: &mut Core, snapshot: &Snapshot, paths: &[&Path]) -> Vec<Answer<Erased>> {
    let request = core.evaluate(snapshot, toy::demand(paths.iter().copied()));
    assert_eq!(core.turn(), Turn::Idle);
    let events = core.take_events();
    paths
        .iter()
        .map(|path| toy::answer_at(&events, &request, path))
        .collect()
}

#[test]
fn a_body_that_reads_a_node_that_failed_outside_the_inputs_is_dropped_and_each_demanded_path_that_needed_it_is_pending()
 {
    let (mut core, snapshot) = model();
    let [reader, chained, fine] =
        ["reader", "chained", "fine"].map(|name| toy::below("R", &[name]));

    let answers = answers_to(&mut core, &snapshot, &[&reader, &chained, &fine]);
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
    let answers = answers_to(&mut core, &snapshot, &[&reader]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
    let runs = |core: &Core| {
        (
            core.counters().body_runs(Spent::ID),
            core.counters().body_runs(Cell::ID),
        )
    };
    let (spent, cells) = runs(&core);

    let answers = answers_to(&mut core, &snapshot, &[&reader]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
    assert_eq!(runs(&core), (spent + 1, cells + 1));
}

#[test]
fn a_path_below_a_node_that_failed_outside_the_inputs_is_pending_with_that_failure() {
    let (mut core, snapshot) = model();
    let below = toy::below("R", &["bad", "x"]);
    let answers = answers_to(&mut core, &snapshot, &[&below]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);

    let nobody = KindId("toy.nobody");
    let lost = core.snapshot([toy::root("lost", Declaration::new(nobody))]);
    let below = toy::below("lost", &["x"]);
    let answers = answers_to(&mut core, &lost, &[&below]);
    assert_eq!(
        toy::pending(answers[0].clone()),
        OutsideFailure::UnknownKind { kind: "toy.nobody" }
    );
}

#[test]
fn a_read_all_with_one_target_failing_outside_the_inputs_drops_the_reader() {
    let (mut core, snapshot) = model();
    let pair = toy::below("R", &["pair"]);

    let answers = answers_to(&mut core, &snapshot, &[&pair]);
    assert_eq!(toy::pending(answers[0].clone()), SPENT);
}
