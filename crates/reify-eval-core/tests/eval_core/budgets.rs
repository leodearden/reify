//! The paths budget a host gives with a request (contract 7.6, 9): each
//! path resolved is charged to every live request that needs it, and the
//! request that would go over its budget is ended with one error, while
//! work another live request waits on goes on and nothing is stored for
//! the end.

use reify_eval_core::{
    Answer, Budget, Core, Event, Input, InputKey, Kind, Outcome, Path, Request, RequestError,
    Snapshot, Turn,
};

use crate::t_model::{self, Fold, Positive};
use crate::toy::{self, Cell, Guard};

/// The tree model's inputs with root T, d = `d`, live.
fn tree(d: i64) -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        toy::named("d", d),
        toy::named("live", true),
        toy::root("root", t_model::t(toy::input("d"), toy::input("live"))),
    ];
    inputs.extend(t_model::tree());
    inputs
}

fn root_sum() -> Path {
    toy::below("root", &["sum"])
}

/// A fresh request for `at` on `snapshot` under `budget`, run to idle, and
/// the events of that turn.
fn evaluate_within(
    core: &mut Core,
    snapshot: &Snapshot,
    at: &Path,
    budget: Budget,
) -> (Request, Vec<Event>) {
    let request = core.evaluate(snapshot, toy::demand([at]), budget);
    assert_eq!(core.turn(), Turn::Idle);
    (request, core.take_events())
}

/// The errors among `events` that ended `request`.
fn endings(events: &[Event], request: &Request) -> Vec<RequestError> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Ended {
                request: to, error, ..
            } if *to == request.id() => Some(error.clone()),
            _ => None,
        })
        .collect()
}

/// `request` was ended by its paths budget `budget`, with one error and no
/// answer.
fn assert_ended_by_budget(events: &[Event], request: &Request, budget: u64) {
    assert_eq!(
        endings(events, request),
        [RequestError::PathsBudgetExhausted { budget }]
    );
    let answers = toy::answers(events, request.id());
    assert!(answers.is_empty(), "{answers:?}");
}

/// `request` was not ended and was told the final value `value` at `at`.
fn assert_answered(events: &[Event], request: &Request, at: &Path, value: i64) {
    let ended = endings(events, request);
    assert!(ended.is_empty(), "{ended:?}");
    let answer = toy::answer_at(events, request, at).downcast::<i64>();
    assert_eq!(answer, Answer::Final(Outcome::of(value)));
}

#[test]
fn shared_exponential_tree_ends_by_paths_budget_not_node_budget() {
    let mut core = t_model::with_t_kinds().build();
    let snapshot = core.snapshot(tree(40));

    let (first, events) = evaluate_within(&mut core, &snapshot, &root_sum(), Budget::paths(10_000));
    assert_ended_by_budget(&events, &first, 10_000);
    assert_eq!(core.counters().paths_resolved(), 10_000);
    assert_eq!(
        core.counters().body_runs(Fold::ID),
        41,
        "one fold per level of the leftmost descent; every other is found by content"
    );
    for kind in [Cell::ID, Guard::ID, Positive::ID] {
        let runs = core.counters().body_runs(kind);
        assert!(runs <= 41, "{kind:?} ran {runs} times");
    }

    let (second, events) =
        evaluate_within(&mut core, &snapshot, &root_sum(), Budget::paths(20_000));
    assert_ended_by_budget(&events, &second, 20_000);
    assert_eq!(core.counters().paths_resolved(), 30_000);

    let mut seeded = t_model::with_t_kinds()
        .on_store(core.selected_store(|_| true))
        .build();
    let on_seeded = seeded.snapshot(tree(40));
    let (third, events) =
        evaluate_within(&mut seeded, &on_seeded, &root_sum(), Budget::paths(10_000));
    assert_ended_by_budget(&events, &third, 10_000);

    let small = core.snapshot(tree(8));
    let ran = [Fold::ID, Positive::ID, Cell::ID].map(|kind| core.counters().body_runs(kind));
    let (fourth, events) = evaluate_within(&mut core, &small, &root_sum(), Budget::unlimited());
    assert_answered(&events, &fourth, &root_sum(), 256);
    assert_eq!(
        [Fold::ID, Positive::ID, Cell::ID].map(|kind| core.counters().body_runs(kind)),
        ran,
        "the descent stored correct values"
    );
}

/// A root cell over two inputs, and a root scope R whose member c is a cell
/// over the same inputs.
fn cells() -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("total", toy::cell(&["a", "b"])),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([("c", toy::cell(&["a", "b"]))]),
    ));
    inputs
}

#[test]
fn a_demand_that_needs_n_paths_answers_within_n_and_ends_within_one_fewer() {
    for (at, needed) in [(toy::at("total"), 1), (toy::below("R", &["c"]), 2)] {
        let mut core = toy::with_scope().build();
        let snapshot = core.snapshot(cells());
        let (within, events) = evaluate_within(&mut core, &snapshot, &at, Budget::paths(needed));
        assert_answered(&events, &within, &at, 5);
        assert_eq!(core.counters().paths_resolved(), needed);

        let mut core = toy::with_scope().build();
        let snapshot = core.snapshot(cells());
        let (short, events) = evaluate_within(&mut core, &snapshot, &at, Budget::paths(needed - 1));
        assert_ended_by_budget(&events, &short, needed - 1);
        assert_eq!(core.counters().paths_resolved(), needed - 1);
    }
}

#[test]
fn a_path_already_answered_in_the_snapshot_is_answered_within_no_budget() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(cells());
    let c = toy::below("R", &["c"]);
    let (first, events) = evaluate_within(&mut core, &snapshot, &c, Budget::unlimited());
    assert_answered(&events, &first, &c, 5);

    let (again, events) = evaluate_within(&mut core, &snapshot, &c, Budget::paths(0));
    assert_answered(&events, &again, &c, 5);
    assert_eq!(core.counters().paths_resolved(), 2);
}

#[test]
fn a_budget_end_stops_no_work_another_live_request_on_the_snapshot_waits_on() {
    let mut core = t_model::with_t_kinds().build();
    let snapshot = core.snapshot(tree(3));
    let bounded = core.evaluate(&snapshot, toy::demand([&root_sum()]), Budget::paths(5));
    let unbounded = core.evaluate(&snapshot, toy::demand([&root_sum()]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    assert_ended_by_budget(&events, &bounded, 5);
    assert_answered(&events, &unbounded, &root_sum(), 8);
}

#[test]
fn a_budget_end_stops_no_request_on_another_snapshot() {
    let mut core = t_model::with_t_kinds().build();
    let (narrow, wide) = (core.snapshot(tree(3)), core.snapshot(tree(4)));
    let bounded = core.evaluate(&narrow, toy::demand([&root_sum()]), Budget::paths(5));
    let unbounded = core.evaluate(&wide, toy::demand([&root_sum()]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    assert_ended_by_budget(&events, &bounded, 5);
    assert_answered(&events, &unbounded, &root_sum(), 16);
}

#[test]
fn ending_a_request_its_budget_ended_does_nothing() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(cells());
    let c = toy::below("R", &["c"]);
    let (short, events) = evaluate_within(&mut core, &snapshot, &c, Budget::paths(1));
    assert_ended_by_budget(&events, &short, 1);

    core.end(short);
    assert_eq!(core.turn(), Turn::Idle);
    let events = core.take_events();
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(core.counters().paths_resolved(), 1);
}
