//! Limits that change answers, set in a snapshot (contract 7.6): a node
//! deeper below its root than the depth setting allows, or whose value
//! declares more children than the children setting allows, fails whatever
//! the store holds, and the failure is a fact about the path, never stored.

use std::fmt::Debug;
use std::iter;

use reify_eval_core::{
    Answer, Core, Erased, Event, Failure, Input, InputKey, Kind, Limit, Outcome, Path, Ref,
    Request, Snapshot, Step, Value,
};

use crate::t_model::{self, Fold, Positive};
use crate::toy::{self, Cell, Guard, Scope};

/// The path `nexts` Ts below root/deep, then `then`: root/deep is at depth
/// 1, so the T there is at depth `nexts` + 1.
fn under_deep(nexts: usize, then: &[&str]) -> Path {
    let steps: Vec<&str> = iter::once("deep")
        .chain(iter::repeat_n("next", nexts))
        .chain(then.iter().copied())
        .collect();
    toy::below("root", &steps)
}

/// The catalogue's two Ts of the chain model under a root scope: deep (d =
/// 70) and shallow (d = 6), both live, under the depth setting `depth`, if
/// any. The T 64 levels below root/deep is at depth 65 with d = 6, and
/// reads what root/shallow reads.
fn two_ts(depth: Option<u64>) -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        toy::named("deep.d", 70_i64),
        toy::named("shallow.d", 6_i64),
        toy::named("live", true),
    ];
    inputs.extend(depth.map(|depth| toy::limit(Limit::Depth, depth)));
    inputs.extend(t_model::chain());
    inputs.extend(toy::root_scope(
        "root",
        toy::members([
            ("deep", t_model::t(toy::input("deep.d"), toy::input("live"))),
            (
                "shallow",
                t_model::t(toy::input("shallow.d"), toy::input("live")),
            ),
        ]),
    ));
    inputs
}

/// The T at depth 65, then the leaf of each T from root/deep down to the
/// one at depth 64, whose leaf is at depth 65.
fn deep_paths() -> Vec<Path> {
    iter::once(under_deep(64, &[]))
        .chain((0..=63).map(|nexts| under_deep(nexts, &["leaf"])))
        .collect()
}

/// A fresh request for `deep_paths` on `snapshot`, run to idle.
fn evaluate_deep(core: &mut Core, snapshot: &Snapshot) -> (Request, Vec<Event>) {
    let paths = deep_paths();
    toy::evaluate_to_idle(core, snapshot, &paths.iter().collect::<Vec<_>>())
}

/// Under the depth setting 64, the T at depth 65 and the leaf at depth 65
/// fail too deep, and every leaf above them is blocked by the leaf below,
/// which it read at "0".
fn assert_cut_at_64(events: &[Event], request: &Request) {
    let too_deep = Failure::TooDeep { limit: 64 };
    let answer = toy::answer_at(events, request, &under_deep(64, &[]));
    assert_eq!(toy::failed(answer), too_deep, "the T at depth 65");
    let answer = toy::answer_at(events, request, &under_deep(63, &["leaf"]));
    assert_eq!(toy::failed(answer), too_deep, "the leaf at depth 65");
    let blocked = Failure::BlockedBy {
        at: Ref::binding("0"),
    };
    for nexts in 0..=62 {
        let leaf = under_deep(nexts, &["leaf"]);
        let answer = toy::answer_at(events, request, &leaf);
        assert_eq!(toy::failed(answer), blocked, "{leaf:?}");
    }
}

fn shallow_leaf() -> Path {
    toy::below("root", &["shallow", "leaf"])
}

/// Whether `answer` is the final value `value`.
fn assert_value<T: Value + PartialEq + Debug>(answer: Answer<Erased>, value: T) {
    assert_eq!(answer.downcast::<T>(), Answer::Final(Outcome::of(value)));
}

/// How many bodies of the T model's kinds have run.
fn body_runs(core: &Core) -> u64 {
    [Cell::ID, Scope::ID, Guard::ID, Positive::ID, Fold::ID]
        .into_iter()
        .map(|kind| core.counters().body_runs(kind))
        .sum()
}

#[test]
fn depth_limit_failure_is_a_fact_about_the_path_not_shared_by_content() {
    let mut core = t_model::with_t_kinds().build();
    let at_64 = core.snapshot(two_ts(Some(64)));
    let (deep, events) = evaluate_deep(&mut core, &at_64);
    assert_cut_at_64(&events, &deep);
    let (shallow, events) = toy::evaluate_to_idle(&mut core, &at_64, &[&shallow_leaf()]);
    assert_value(toy::the_answer(&events, &shallow, &shallow_leaf()), 1_i64);

    let mut fresh = t_model::with_t_kinds().build();
    let snapshot = fresh.snapshot(two_ts(Some(64)));
    assert_value(
        toy::answer_alone(&mut fresh, &snapshot, &shallow_leaf()),
        1_i64,
    );
    let (fresh_deep, events) = evaluate_deep(&mut fresh, &snapshot);
    assert_cut_at_64(&events, &fresh_deep);

    let (depth, at) = toy::limit(Limit::Depth, 72);
    let at_72 = at_64.edit([(depth.clone(), Some(at))]);
    let at_64_id = at_64.id();
    core.end(deep);
    core.end(shallow);
    drop(at_64);
    let (t_65, leaf) = (under_deep(64, &[]), under_deep(0, &["leaf"]));
    let (at_72_request, events) = toy::evaluate_to_idle(&mut core, &at_72, &[&leaf, &t_65]);
    assert_value(toy::answer_at(&events, &at_72_request, &leaf), 1_i64);
    let answer = toy::answer_at(&events, &at_72_request, &t_65);
    assert!(
        matches!(answer, Answer::Final(Outcome::Value(_))),
        "{answer:?}"
    );

    let ran = body_runs(&core);
    let (_, sixty_four) = toy::limit(Limit::Depth, 64);
    let again = at_72.edit([(depth, Some(sixty_four))]);
    assert_eq!(again.id(), at_64_id, "equal inputs make the same snapshot");
    let (again_request, events) = evaluate_deep(&mut core, &again);
    assert_cut_at_64(&events, &again_request);
    assert_eq!(
        body_runs(&core),
        ran,
        "every result is in the store, and the depth check comes before it"
    );
}

#[test]
fn a_root_is_at_depth_zero_and_answers_under_a_depth_setting_of_zero() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
        toy::limit(Limit::Depth, 0),
    ]);

    assert_value(
        toy::answer_alone(&mut core, &snapshot, &toy::at("total")),
        2_i64,
    );
}

#[test]
fn a_member_below_a_depth_setting_of_zero_answers_too_deep_and_runs_no_body() {
    let mut core = toy::with_scope().build();
    let mut inputs = vec![toy::named("a", 2_i64), toy::limit(Limit::Depth, 0)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([("c", toy::cell(&["a"]))]),
    ));
    let snapshot = core.snapshot(inputs);

    let answer = toy::answer_alone(&mut core, &snapshot, &toy::below("R", &["c"]));
    assert_eq!(toy::failed(answer), Failure::TooDeep { limit: 0 });
    assert_eq!(core.counters().body_runs(Cell::ID), 0);
    assert_eq!(core.counters().body_runs(Scope::ID), 1);
}

/// Root scope R declaring scope inner, which declares the cell c over "a",
/// and reader, a cell over R/inner/c; under the depth setting `depth`.
fn nested(depth: u64) -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        toy::named("a", 2_i64),
        toy::named("inner.members", toy::members([("c", toy::cell(&["a"]))])),
        toy::limit(Limit::Depth, depth),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("inner", toy::scope("inner.members")),
            ("reader", toy::cell_over([toy::sibling(&["inner", "c"])])),
        ]),
    ));
    inputs
}

#[test]
fn a_path_below_a_too_deep_node_answers_that_nodes_failure() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(nested(1));
    let (c, below_c) = (
        toy::below("R", &["inner", "c"]),
        toy::below("R", &["inner", "c", "x"]),
    );

    let answers = toy::answers_to(&mut core, &snapshot, &[&c, &below_c]);
    let too_deep = Failure::TooDeep { limit: 1 };
    assert_eq!(
        answers.into_iter().map(toy::failed).collect::<Vec<_>>(),
        [too_deep.clone(), too_deep]
    );
    assert_eq!(core.counters().body_runs(Cell::ID), 0);
}

#[test]
fn a_reader_of_a_too_deep_node_is_blocked_by_it() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(nested(1));

    let answer = toy::answer_alone(&mut core, &snapshot, &toy::below("R", &["reader"]));
    assert_eq!(
        toy::failed(answer),
        Failure::BlockedBy {
            at: Ref::binding("0")
        }
    );
}

#[test]
fn a_path_past_the_depth_setting_that_no_node_declares_answers_absent() {
    let mut core = toy::with_structure().build();
    let mut inputs = vec![
        toy::named("a", 2_i64),
        toy::named("n", 2_i64),
        toy::named("element", toy::cell(&["a"])),
        toy::limit(Limit::Depth, 0),
        toy::root(
            "list",
            toy::collection(toy::input("n"), toy::input("element")),
        ),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([("c", toy::cell(&["a"]))]),
    ));
    let snapshot = core.snapshot(inputs);
    let list = toy::at("list");
    let (missing, past_count, element) = (
        toy::below("R", &["missing"]),
        list.child(Step::index(5)),
        list.child(Step::index(0)),
    );

    let answers = toy::answers_to(&mut core, &snapshot, &[&missing, &past_count, &element]);
    assert!(
        matches!(
            answers.as_slice(),
            [
                Answer::Final(Outcome::Absent),
                Answer::Final(Outcome::Absent),
                Answer::Final(Outcome::Failure(Failure::TooDeep { limit: 0 })),
            ]
        ),
        "{answers:?}"
    );
}

#[test]
fn with_no_depth_setting_nothing_is_bounded() {
    let mut core = t_model::with_t_kinds().build();
    let snapshot = core.snapshot(two_ts(None));
    let (t_65, leaf) = (under_deep(64, &[]), under_deep(0, &["leaf"]));

    let (request, events) = toy::evaluate_to_idle(&mut core, &snapshot, &[&leaf, &t_65]);
    assert_value(toy::answer_at(&events, &request, &leaf), 1_i64);
    let answer = toy::answer_at(&events, &request, &t_65);
    assert!(
        matches!(answer, Answer::Final(Outcome::Value(_))),
        "{answer:?}"
    );
}

#[test]
#[should_panic(expected = "the limit setting Depth must be a u64")]
fn a_depth_setting_that_is_not_a_u64_is_refused() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
        (InputKey::Limit(Limit::Depth), toy::input_of(64_i64)),
    ]);

    let _ = toy::answer_alone(&mut core, &snapshot, &toy::at("total"));
}

/// The inputs of root scope R with `members`, each member a cell over the
/// input "v", under the children setting `children`.
fn scope_of(children: u64, members: &[&str]) -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        toy::named("v", 1_i64),
        toy::limit(Limit::ChildrenPerNode, children),
    ];
    let cells = members.iter().map(|name| (*name, toy::cell(&["v"])));
    inputs.extend(toy::root_scope("R", toy::members(cells)));
    inputs
}

#[test]
fn a_scope_declaring_more_members_than_the_children_setting_fails_and_so_does_each_member() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(scope_of(2, &["a", "b", "c"]));
    let paths = [
        toy::at("R"),
        toy::below("R", &["a"]),
        toy::below("R", &["b"]),
        toy::below("R", &["c"]),
    ];

    let answers = toy::answers_to(&mut core, &snapshot, &paths.iter().collect::<Vec<_>>());
    let too_many = Failure::TooManyChildren { limit: 2 };
    assert_eq!(
        answers.into_iter().map(toy::failed).collect::<Vec<_>>(),
        [
            too_many.clone(),
            too_many.clone(),
            too_many.clone(),
            too_many
        ]
    );
    assert_eq!(core.counters().body_runs(Cell::ID), 0);
}

#[test]
fn a_scope_declaring_as_many_members_as_the_children_setting_answers_them() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(scope_of(3, &["a", "b", "c"]));

    let answer = toy::answer_alone(&mut core, &snapshot, &toy::below("R", &["c"]));
    assert_value(answer, 1_i64);
}

#[test]
fn a_reader_of_a_member_of_a_node_with_too_many_children_is_blocked_by_it() {
    let mut core = toy::with_scope().build();
    let three = toy::members(["a", "b", "c"].map(|name| (name, toy::cell(&["v"]))));
    let mut inputs = vec![
        toy::named("v", 1_i64),
        toy::named("inner.members", three),
        toy::limit(Limit::ChildrenPerNode, 2),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("inner", toy::scope("inner.members")),
            ("reader", toy::cell_over([toy::sibling(&["inner", "a"])])),
        ]),
    ));
    let snapshot = core.snapshot(inputs);
    let (inner, reader) = (toy::below("R", &["inner"]), toy::below("R", &["reader"]));

    let answers = toy::answers_to(&mut core, &snapshot, &[&inner, &reader]);
    assert_eq!(
        answers.into_iter().map(toy::failed).collect::<Vec<_>>(),
        [
            Failure::TooManyChildren { limit: 2 },
            Failure::BlockedBy {
                at: Ref::binding("0")
            },
        ]
    );
}

/// The answer at root collection "list" of `count` cells over "v", under
/// the children setting `children`.
fn collection_under(children: u64, count: i64) -> Answer<Erased> {
    let mut core = toy::with_structure().build();
    let snapshot = core.snapshot([
        toy::named("v", 1_i64),
        toy::named("n", count),
        toy::named("element", toy::cell(&["v"])),
        toy::limit(Limit::ChildrenPerNode, children),
        toy::root(
            "list",
            toy::collection(toy::input("n"), toy::input("element")),
        ),
    ]);
    toy::answer_alone(&mut core, &snapshot, &toy::at("list"))
}

#[test]
fn a_collection_fails_past_the_children_setting_and_answers_at_it() {
    assert_eq!(
        toy::failed(collection_under(4, 5)),
        Failure::TooManyChildren { limit: 4 }
    );
    for (children, count) in [(5, 5), (0, 0)] {
        let answer = collection_under(children, count);
        assert!(
            matches!(answer, Answer::Final(Outcome::Value(_))),
            "{count} elements under {children}: {answer:?}"
        );
    }
}

#[test]
fn a_guard_under_a_false_condition_is_absent_and_not_checked() {
    let mut core = toy::with_structure().build();
    let three = toy::members(["a", "b", "c"].map(|name| (name, toy::cell(&["v"]))));
    let snapshot = core.snapshot([
        toy::named("v", 1_i64),
        toy::named("off", false),
        toy::named("guarded.members", three),
        toy::limit(Limit::ChildrenPerNode, 0),
        toy::root("guarded", toy::guard(toy::input("off"), "guarded.members")),
    ]);

    let answer = toy::answer_alone(&mut core, &snapshot, &toy::at("guarded"));
    assert!(
        matches!(answer, Answer::Final(Outcome::Absent)),
        "{answer:?}"
    );
}

/// A fresh request for R/a on `snapshot`, run to idle, and its answer.
fn first_member(core: &mut Core, snapshot: &Snapshot) -> (Request, Answer<Erased>) {
    let a = toy::below("R", &["a"]);
    let (request, events) = toy::evaluate_to_idle(core, snapshot, &[&a]);
    let answer = toy::the_answer(&events, &request, &a);
    (request, answer)
}

#[test]
fn the_children_limit_is_never_stored_whether_or_not_the_result_is_in_the_store() {
    let too_many = Failure::TooManyChildren { limit: 2 };
    let mut core = toy::with_scope().build();
    let at_2 = core.snapshot(scope_of(2, &["a", "b", "c"]));
    let (request, answer) = first_member(&mut core, &at_2);
    assert_eq!(toy::failed(answer), too_many);
    assert_eq!(core.counters().body_runs(Scope::ID), 1);
    core.end(request);
    drop(at_2);
    let at_3 = core.snapshot(scope_of(3, &["a", "b", "c"]));
    let (_at_3_request, answer) = first_member(&mut core, &at_3);
    assert_value(answer, 1_i64);
    assert_eq!(
        core.counters().body_runs(Scope::ID),
        1,
        "the scope's value was filed though its snapshot failed it"
    );

    let mut fresh = toy::with_scope().build();
    let at_3 = fresh.snapshot(scope_of(3, &["a", "b", "c"]));
    let (_at_3_request, answer) = first_member(&mut fresh, &at_3);
    assert_value(answer, 1_i64);
    let at_2 = fresh.snapshot(scope_of(2, &["a", "b", "c"]));
    let (_at_2_request, answer) = first_member(&mut fresh, &at_2);
    assert_eq!(toy::failed(answer), too_many);
    assert_eq!(fresh.counters().body_runs(Scope::ID), 1);
}
