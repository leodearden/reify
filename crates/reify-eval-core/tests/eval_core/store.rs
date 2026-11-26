//! Results filed under the recipe and the contents read, in order, and
//! found again wherever the same reads find the same contents (contract
//! 4.2, 5.1 to 5.3): at another place, in another snapshot, after an edit
//! and its reversal.

use reify_eval_core::{
    Answer, Binding, Core, Cx, Declaration, Erased, Failure, Hash128, InputKey, Kind, KindId,
    Outcome, Path, Produced, Ref, Snapshot,
};

use crate::toy::{self, Cell, Scope};

fn int(answer: Answer<Erased>) -> Answer<i64> {
    answer.downcast()
}

fn final_int(value: i64) -> Answer<i64> {
    Answer::Final(Outcome::of(value))
}

/// Body runs of every kind the store tests use.
fn all_runs(core: &Core) -> u64 {
    [Cell::ID, Scope::ID]
        .into_iter()
        .map(|kind| core.counters().body_runs(kind))
        .sum()
}

#[test]
fn an_identical_declaration_at_a_second_place_is_found_without_running_its_body() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::named("b", 3_i64),
        toy::root("first", toy::cell(&["a", "b"])),
        toy::root("second", toy::cell(&["a", "b"])),
    ]);

    let first = toy::answer_alone(&mut core, &snapshot, &toy::at("first"));
    let lookups = core.counters().store_lookups();
    let second = toy::answer_alone(&mut core, &snapshot, &toy::at("second"));

    assert_eq!(int(first), final_int(5));
    assert_eq!(int(second), final_int(5));
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
    assert!(core.counters().store_lookups() > lookups);
}

#[test]
fn a_failure_that_follows_from_the_inputs_is_stored_and_found_again_and_its_reader_is_blocked_by_it()
 {
    let mut core = toy::with_scope().build();
    let zero = toy::cell(&["z"]);
    let quotient = toy::divided(toy::cell(&["a"]), toy::sibling(&["zero"]));
    let reader = toy::cell_over([toy::sibling(&["q"])]);
    let mut inputs = vec![toy::named("a", 7_i64), toy::named("z", 0_i64)];
    inputs.extend(toy::root_scope(
        "R1",
        toy::members([("zero", zero.clone()), ("q", quotient.clone())]),
    ));
    inputs.extend(toy::root_scope(
        "R2",
        toy::members([("zero", zero), ("q", quotient), ("reader", reader)]),
    ));
    let snapshot = core.snapshot(inputs);
    let by_zero = Failure::OfKind {
        kind: Cell::ID,
        code: toy::DIVISION_BY_ZERO,
    };

    let first = toy::answer_alone(&mut core, &snapshot, &toy::below("R1", &["q"]));
    assert_eq!(toy::failed(first), by_zero);
    assert_eq!(core.counters().body_runs(Cell::ID), 2);

    let second = toy::answer_alone(&mut core, &snapshot, &toy::below("R2", &["q"]));
    assert_eq!(toy::failed(second), by_zero);
    assert_eq!(core.counters().body_runs(Cell::ID), 2, "no cell ran again");

    let blocked = toy::answer_alone(&mut core, &snapshot, &toy::below("R2", &["reader"]));
    assert_eq!(
        toy::failed(blocked),
        Failure::BlockedBy {
            at: Ref::binding("0")
        }
    );
}

/// A root scope `R` whose member `t` reads its member `s`, and `u` reads
/// `t`, with `s` adding the inputs "a" and "b".
fn chain_of_three(core: &mut Core, a: i64, b: i64) -> Snapshot {
    let mut inputs = vec![toy::named("a", a), toy::named("b", b)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("s", toy::cell(&["a", "b"])),
            ("t", toy::cell_over([toy::sibling(&["s"])])),
            ("u", toy::cell_over([toy::sibling(&["t"])])),
        ]),
    ));
    core.snapshot(inputs)
}

fn a_set_to(snapshot: &Snapshot, a: i64) -> Snapshot {
    snapshot.edit([(InputKey::named("a"), Some(toy::input_of(a)))])
}

#[test]
fn an_edit_and_its_reversal_cost_no_body_run() {
    let mut core = toy::with_scope().build();
    let s0 = chain_of_three(&mut core, 1, 2);
    let paths = [toy::below("R", &["s"]), toy::below("R", &["u"])];
    let paths: Vec<&Path> = paths.iter().collect();

    let before: Vec<_> = toy::answers_to(&mut core, &s0, &paths)
        .into_iter()
        .map(int)
        .collect();
    let s1 = a_set_to(&s0, 5);
    toy::answers_to(&mut core, &s1, &paths);
    let s2 = a_set_to(&s1, 1);
    let runs = all_runs(&core);
    let after: Vec<_> = toy::answers_to(&mut core, &s2, &paths)
        .into_iter()
        .map(int)
        .collect();

    assert_eq!(after, before);
    assert_eq!(before, [final_int(3), final_int(3)]);
    assert_eq!(all_runs(&core), runs, "no body ran for the reversal");
}

#[test]
fn after_an_edit_body_runs_are_at_most_the_nodes_whose_read_contents_changed() {
    let mut core = toy::with_scope().build();
    let u = toy::below("R", &["u"]);
    let s0 = chain_of_three(&mut core, 1, 2);
    assert_eq!(int(toy::answer_alone(&mut core, &s0, &u)), final_int(3));
    let (cells, scopes) = (
        core.counters().body_runs(Cell::ID),
        core.counters().body_runs(Scope::ID),
    );

    let s1 = s0.edit([
        (InputKey::named("a"), Some(toy::input_of(2_i64))),
        (InputKey::named("b"), Some(toy::input_of(1_i64))),
    ]);
    assert_eq!(int(toy::answer_alone(&mut core, &s1, &u)), final_int(3));
    assert_eq!(core.counters().body_runs(Cell::ID), cells + 1, "only s ran");
    assert_eq!(core.counters().body_runs(Scope::ID), scopes);

    let s2 = a_set_to(&s1, 5);
    assert_eq!(int(toy::answer_alone(&mut core, &s2, &u)), final_int(6));
    assert_eq!(
        core.counters().body_runs(Cell::ID),
        cells + 4,
        "s, t and u ran"
    );
    assert_eq!(core.counters().body_runs(Scope::ID), scopes);
}

/// The body runs of each kind when `n` equal instances, each a scope of
/// two cells the second of which reads the first, are evaluated one after
/// another, each instance's second cell in a request of its own.
fn runs_of_identical_instances(n: usize) -> (u64, u64) {
    let mut core = toy::with_scope().build();
    let instance = toy::members([
        ("x", toy::cell(&["v"])),
        ("y", toy::cell_over([toy::sibling(&["x"]), toy::input("v")])),
    ]);
    let names: Vec<String> = (0..n).map(|index| format!("i{index}")).collect();
    let mut inputs = vec![toy::named("v", 4_i64), toy::named("instance", instance)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members(
            names
                .iter()
                .map(|name| (name.as_str(), toy::scope("instance"))),
        ),
    ));
    let snapshot = core.snapshot(inputs);
    for name in &names {
        let y = toy::below("R", &[name, "y"]);
        assert_eq!(
            int(toy::answer_alone(&mut core, &snapshot, &y)),
            final_int(8)
        );
    }
    (
        core.counters().body_runs(Cell::ID),
        core.counters().body_runs(Scope::ID),
    )
}

#[test]
fn n_identical_instances_evaluated_one_after_another_cost_the_body_runs_of_one() {
    let one = runs_of_identical_instances(1);
    assert_eq!(one, (2, 2));
    for n in [100, 1_000, 10_000] {
        assert_eq!(runs_of_identical_instances(n), one, "N = {n}");
    }
}

/// The bool true, whose content hashes the byte 1.
struct True;

impl Kind for True {
    type Out = bool;
    const ID: KindId = KindId("test.true");
    const VERSION: u32 = 1;

    async fn run(_cx: &Cx) -> Produced<bool> {
        Produced::Outcome(Outcome::of(true))
    }

    fn content(_out: &bool) -> Option<Hash128> {
        Some(Hash128::of_bytes(&[1]))
    }
}

/// The i64 1, whose content hashes the byte 1, as True's does.
struct One;

impl Kind for One {
    type Out = i64;
    const ID: KindId = KindId("test.one");
    const VERSION: u32 = 1;

    async fn run(_cx: &Cx) -> Produced<i64> {
        Produced::Outcome(Outcome::of(1))
    }

    fn content(_out: &i64) -> Option<Hash128> {
        Some(Hash128::of_bytes(&[1]))
    }
}

/// Ten more than the i64 at "x"; a failure read is its answer.
struct TenMore;

impl Kind for TenMore {
    type Out = i64;
    const ID: KindId = KindId("test.ten_more");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        match cx.read::<i64>(&Ref::binding("x")).await {
            Outcome::Value(x) => Produced::Outcome(Outcome::of(*x + 10)),
            other => Produced::Outcome(other),
        }
    }
}

#[test]
fn values_of_different_types_never_share_a_content() {
    let mut core = toy::with_scope()
        .kind::<True>()
        .and_then(|builder| builder.kind::<One>())
        .and_then(|builder| builder.kind::<TenMore>())
        .expect("distinct kinds")
        .build();
    let reader = Declaration::new(TenMore::ID).bind("x", toy::sibling(&["x"]));
    let mut inputs = Vec::new();
    for (place, x) in [("B", True::ID), ("I", One::ID)] {
        inputs.extend(toy::root_scope(
            place,
            toy::members([("x", Declaration::new(x)), ("r", reader.clone())]),
        ));
    }
    let snapshot = core.snapshot(inputs);

    let at_b = toy::answer_alone(&mut core, &snapshot, &toy::below("B", &["r"]));
    assert!(matches!(toy::failed(at_b), Failure::WrongType { .. }));
    let at_i = toy::answer_alone(&mut core, &snapshot, &toy::below("I", &["r"]));
    assert_eq!(int(at_i), final_int(11));
    assert_eq!(core.counters().body_runs(TenMore::ID), 2);
}

#[test]
fn the_same_child_declaration_reached_through_two_declarers_whose_passed_binding_names_equal_contents_runs_once()
 {
    let mut core = toy::with_scope().build();
    let child = toy::members([("c", toy::cell_over([Binding::Passed(Ref::binding("v"))]))]);
    let mut inputs = vec![toy::named("a", 5_i64), toy::named("b", 5_i64)];
    for (declarer, v) in [("first", "a"), ("second", "b")] {
        let members = format!("{declarer}.members");
        inputs.push(toy::named(&members, child.clone()));
        inputs.push(toy::root(
            declarer,
            toy::scope(&members).bind("v", toy::input(v)),
        ));
    }
    let snapshot = core.snapshot(inputs);

    let first = toy::answer_alone(&mut core, &snapshot, &toy::below("first", &["c"]));
    let second = toy::answer_alone(&mut core, &snapshot, &toy::below("second", &["c"]));

    assert_eq!(int(first), final_int(5));
    assert_eq!(int(second), final_int(5));
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}

#[test]
fn a_path_answered_from_its_snapshot_s_index_costs_no_store_lookup() {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", 2_i64),
        toy::root("total", toy::cell(&["a"])),
    ]);
    toy::answer_alone(&mut core, &snapshot, &toy::at("total"));
    let lookups = core.counters().store_lookups();

    let again = toy::answer_alone(&mut core, &snapshot, &toy::at("total"));

    assert_eq!(int(again), final_int(2));
    assert_eq!(core.counters().store_lookups(), lookups);
}
