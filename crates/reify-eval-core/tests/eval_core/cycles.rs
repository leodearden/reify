//! A ring of waits answers "cycle" on every node of it, with one value
//! wherever the ring was entered, and nothing is stored for it (contract
//! 7.7). A chain of waits that passes through a lookup is not a ring: that
//! lookup misses and its body runs.

use std::sync::Arc;

use reify_eval_core::{
    Answer, Binding, Budget, Core, Cx, Declaration, Event, Failure, Hash128, Input, InputKey, Kind,
    KindId, Outcome, Path, Produced, Ref, Snapshot, Step, Turn,
};

use crate::toy::{self, Cell, Members, ToyKernel, Watch};

/// The answer every node on a ring has.
fn cycle() -> Answer<i64> {
    Answer::Final(Outcome::Failure(Failure::Cycle))
}

/// The answer of a cell whose first term is a node on a ring.
fn blocked_by_its_first_term() -> Answer<i64> {
    Answer::Final(Outcome::Failure(Failure::BlockedBy {
        at: Ref::binding("0"),
    }))
}

/// The answers at `paths`, in order, to a fresh request for them on
/// `snapshot`, which the core runs until no section is running.
fn run_for(core: &mut Core, snapshot: &Snapshot, paths: &[&Path]) -> Vec<Answer<i64>> {
    let request = core.evaluate(
        snapshot,
        toy::demand(paths.iter().copied()),
        Budget::unlimited(),
    );
    core.run();
    let events = core.take_events();
    assert!(
        events
            .iter()
            .all(|event| matches!(event, Event::Answered { .. })),
        "the core tells answers and nothing else: {events:?}"
    );
    let answers = paths
        .iter()
        .map(|path| toy::answer_at(&events, &request, path).downcast::<i64>())
        .collect();
    core.end(request);
    answers
}

/// The inputs of root scope R with `members` and the inputs `named`.
fn scope_r(
    members: Members,
    named: impl IntoIterator<Item = (InputKey, Input)>,
) -> Vec<(InputKey, Input)> {
    let mut inputs: Vec<_> = named.into_iter().collect();
    inputs.extend(toy::root_scope("R", members));
    inputs
}

/// R/`name`.
fn r(name: &str) -> Path {
    toy::below("R", &[name])
}

/// Cells a = b + one and b = a + one, members of R, and `more` beside them.
fn two_cell_ring(more: impl IntoIterator<Item = (&'static str, Declaration)>) -> Members {
    let over = |other: &str| toy::cell_over([toy::sibling(&[other]), toy::input("one")]);
    let mut members = toy::members([("a", over("b")), ("b", over("a"))]);
    members.0.extend(toy::members(more).0);
    members
}

fn ring_inputs(
    more: impl IntoIterator<Item = (&'static str, Declaration)>,
) -> Vec<(InputKey, Input)> {
    scope_r(two_cell_ring(more), [toy::named("one", 1_i64)])
}

#[test]
fn two_cell_ring_answers_cycle_on_both_whatever_is_demanded_first() {
    let (a, b) = (r("a"), r("b"));
    let orders: [&[&[&Path]]; 3] = [&[&[&a], &[&b]], &[&[&b], &[&a]], &[&[&a, &b]]];
    for order in orders {
        let mut core = toy::with_scope().build();
        let snapshot = core.snapshot(ring_inputs([]));
        let answers: Vec<_> = order
            .iter()
            .flat_map(|paths| run_for(&mut core, &snapshot, paths))
            .collect();
        assert_eq!(answers, [cycle(), cycle()], "order {order:?}");
        assert_eq!(core.counters().body_runs(Cell::ID), 2, "order {order:?}");
    }
}

/// Reads the bool at "if"; true reads the i64 at "then", false the one at
/// "else", and answers what it read. A read that fails makes it blocked by
/// the ref it read; one that reads absent makes it absent.
struct Choose;

impl Choose {
    fn of(condition: Binding, then: Binding, otherwise: Binding) -> Declaration {
        Declaration::new(Self::ID)
            .bind("if", condition)
            .bind("then", then)
            .bind("else", otherwise)
    }
}

/// What a read gives a body that answers a failed read as blocked by it.
fn or_blocked<T>(read: Outcome<T>, at: &Ref) -> Result<Arc<T>, Outcome<i64>> {
    match read {
        Outcome::Value(value) => Ok(value),
        Outcome::Absent => Err(Outcome::Absent),
        Outcome::Failure(_) => Err(Outcome::Failure(Failure::BlockedBy { at: at.clone() })),
    }
}

impl Kind for Choose {
    type Out = i64;
    const ID: KindId = KindId("test.choose");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let condition = Ref::binding("if");
        let holds = match or_blocked(cx.read::<bool>(&condition).await, &condition) {
            Ok(holds) => *holds,
            Err(answer) => return Produced::Outcome(answer),
        };
        let branch = Ref::binding(if holds { "then" } else { "else" });
        Produced::Outcome(match or_blocked(cx.read::<i64>(&branch).await, &branch) {
            Ok(value) => Outcome::of(*value),
            Err(answer) => answer,
        })
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(toy::Contented::content(out))
    }
}

fn with_choose() -> Core {
    toy::with_scope()
        .kind::<Choose>()
        .expect("distinct kinds")
        .build()
}

fn f_is(holds: bool) -> (InputKey, Option<Input>) {
    (InputKey::named("f"), Some(toy::input_of(holds)))
}

#[test]
fn conditional_mutual_reference_without_a_ring_of_running_bodies_is_not_a_cycle() {
    let f = || toy::input("f");
    let members = toy::members([
        (
            "a",
            Choose::of(f(), toy::sibling(&["b"]), toy::input("one")),
        ),
        (
            "b",
            Choose::of(f(), toy::input("two"), toy::sibling(&["a"])),
        ),
    ]);
    let inputs = scope_r(
        members,
        [
            toy::named("f", true),
            toy::named("one", 1_i64),
            toy::named("two", 2_i64),
        ],
    );
    let (a, b) = (r("a"), r("b"));
    let is = |value: i64| Answer::Final(Outcome::of(value));

    let mut core = with_choose();
    let s1 = core.snapshot(inputs.clone());
    assert_eq!(run_for(&mut core, &s1, &[&a, &b]), [is(2), is(2)]);

    let mut fresh = with_choose();
    let fresh_s1 = fresh.snapshot(inputs);
    let one_at_a_time = [&b, &a].map(|path| run_for(&mut fresh, &fresh_s1, &[path]));
    assert_eq!(one_at_a_time, [[is(2)], [is(2)]]);

    let s2 = s1.edit([f_is(false)]);
    assert_eq!(run_for(&mut core, &s2, &[&a, &b]), [is(1), is(1)]);
    let s3 = s2.edit([f_is(true)]);
    assert_eq!(run_for(&mut core, &s3, &[&a, &b]), [is(2), is(2)]);
}

/// Reads the members at "of" and answers how many there are.
struct Plan;

impl Kind for Plan {
    type Out = i64;
    const ID: KindId = KindId("test.plan");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let of = Ref::binding("of");
        Produced::Outcome(match or_blocked(cx.read::<Members>(&of).await, &of) {
            Ok(members) => Outcome::of(i64::try_from(members.0.len()).expect("a few members")),
            Err(answer) => answer,
        })
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(toy::Contented::content(out))
    }
}

/// Reads the i64 at "plan" before it reads its members at "members", then
/// declares them as a scope does.
struct PlanFirst;

impl Kind for PlanFirst {
    type Out = Members;
    const ID: KindId = KindId("test.plan_first");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Members> {
        let plan = Ref::binding("plan");
        if let Outcome::Failure(_) = cx.read::<i64>(&plan).await {
            return Produced::Outcome(Outcome::Failure(Failure::BlockedBy { at: plan }));
        }
        Produced::Outcome(cx.read::<Members>(&Ref::binding("members")).await)
    }

    fn content(out: &Members) -> Option<Hash128> {
        Some(toy::Contented::content(out))
    }

    fn declares(out: &Members, step: &Step) -> Option<Declaration> {
        out.0.get(step).cloned()
    }
}

/// R's one member `s`, declared `s`, which declares the members `plan`,
/// counting the members of s; `handdown`, a scope whose binding "src" is
/// the place s/plan and whose one member `a` answers what "src" names; and
/// `a`, answering what s/handdown/a does.
fn handdown_inputs(s: Declaration) -> Vec<(InputKey, Input)> {
    let plan = Declaration::new(Plan::ID).bind("of", Binding::Declarer(Vec::new()));
    let handdown = toy::scope("handdown.members").bind("src", toy::sibling(&["plan"]));
    let s_members = toy::members([
        ("plan", plan),
        ("handdown", handdown),
        ("a", toy::cell_over([toy::sibling(&["handdown", "a"])])),
    ]);
    let handdown_a = toy::cell_over([Binding::Passed(Ref::binding("src"))]);
    scope_r(
        toy::members([("s", s)]),
        [
            toy::named("s.members", s_members),
            toy::named("handdown.members", toy::members([("a", handdown_a)])),
        ],
    )
}

fn with_plans() -> Core {
    toy::with_scope()
        .kind::<Plan>()
        .and_then(|builder| builder.kind::<PlanFirst>())
        .expect("distinct kinds")
        .build()
}

#[test]
fn a_binding_to_a_sibling_place_is_not_a_read_by_the_declaring_scope() {
    let s_a = toy::below("R", &["s", "a"]);

    let mut core = with_plans();
    let snapshot = core.snapshot(handdown_inputs(toy::scope("s.members")));
    assert_eq!(
        run_for(&mut core, &snapshot, &[&s_a]),
        [Answer::Final(Outcome::of(3))]
    );

    let reads_plan_first = Declaration::new(PlanFirst::ID)
        .bind("plan", toy::sibling(&["s", "plan"]))
        .bind("members", toy::input("s.members"));
    let mut core = with_plans();
    let snapshot = core.snapshot(handdown_inputs(reads_plan_first));
    let s = toy::below("R", &["s"]);
    let s_plan = toy::below("R", &["s", "plan"]);
    let answers: Vec<_> = [&s_a, &s, &s_plan]
        .into_iter()
        .flat_map(|path| run_for(&mut core, &snapshot, &[path]))
        .collect();
    assert_eq!(answers, [cycle(), cycle(), cycle()]);
}

#[test]
fn a_body_that_reads_its_own_path_answers_cycle() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(scope_r(
        toy::members([("a", toy::cell_over([toy::sibling(&["a"])]))]),
        [],
    ));

    assert_eq!(run_for(&mut core, &snapshot, &[&r("a")]), [cycle()]);
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}

#[test]
fn a_reader_outside_a_ring_is_blocked_by_it() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(ring_inputs([("c", toy::cell_over([toy::sibling(&["a"])]))]));

    assert_eq!(
        run_for(&mut core, &snapshot, &[&r("c")]),
        [blocked_by_its_first_term()]
    );
    assert_eq!(
        run_for(&mut core, &snapshot, &[&r("a"), &r("b")]),
        [cycle(), cycle()]
    );
}

#[test]
fn a_cell_reading_below_itself_and_that_path_answer_cycle() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(scope_r(
        toy::members([("a", toy::cell_over([toy::sibling(&["a", "x"])]))]),
        [],
    ));
    let below_a = toy::below("R", &["a", "x"]);

    assert_eq!(run_for(&mut core, &snapshot, &[&r("a")]), [cycle()]);
    assert_eq!(run_for(&mut core, &snapshot, &[&below_a]), [cycle()]);
}

#[test]
fn a_request_that_wants_no_node_on_a_ring_is_answered() {
    let mut core = toy::with_scope().build();
    let mut inputs = ring_inputs([("c", toy::cell(&["two"]))]);
    inputs.push(toy::named("two", 2_i64));
    let snapshot = core.snapshot(inputs);
    let (a, c) = (r("a"), r("c"));

    let on_ring = core.evaluate(&snapshot, toy::demand([&a]), Budget::unlimited());
    let beside = core.evaluate(&snapshot, toy::demand([&c]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    assert!(
        events
            .iter()
            .all(|event| matches!(event, Event::Answered { .. })),
        "{events:?}"
    );
    let answer = toy::the_answer(&events, &beside, &c);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(2)));
    let answer = toy::the_answer(&events, &on_ring, &a);
    assert_eq!(answer.downcast::<i64>(), cycle());
}

#[test]
fn a_ring_does_not_stop_turn_wait_or_run_from_returning() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(ring_inputs([]));
    let a = r("a");

    let request = core.evaluate(&snapshot, toy::demand([&a]), Budget::unlimited());
    core.run();
    assert_eq!(core.wait(), Turn::Idle);
    assert_eq!(core.turn(), Turn::Idle);

    let events = core.take_events();
    assert!(
        events
            .iter()
            .all(|event| matches!(event, Event::Answered { .. })),
        "{events:?}"
    );
    assert_eq!(
        toy::the_answer(&events, &request, &a).downcast::<i64>(),
        cycle()
    );
}

#[test]
fn nothing_is_stored_for_a_ring_and_its_reader_is_found_in_the_store() {
    let c = || ("c", toy::cell_over([toy::sibling(&["a"])]));
    let (a, b) = (r("a"), r("b"));

    let mut first = toy::with_scope().build();
    let snapshot = first.snapshot(ring_inputs([c()]));
    assert_eq!(
        run_for(&mut first, &snapshot, &[&r("c")]),
        [blocked_by_its_first_term()]
    );
    assert_eq!(
        run_for(&mut first, &snapshot, &[&a, &b]),
        [cycle(), cycle()]
    );

    let mut core = toy::with_scope().on_store(first.into_store()).build();
    let snapshot = core.snapshot(ring_inputs([c()]));
    assert_eq!(
        run_for(&mut core, &snapshot, &[&r("c"), &a, &b]),
        [blocked_by_its_first_term(), cycle(), cycle()]
    );
    assert_eq!(
        core.counters().body_runs(Cell::ID),
        2,
        "a and b run again; c's result is found"
    );
}

/// A pair over `first` and `second`, read in one read.
fn pair(first: Binding, second: Binding) -> Declaration {
    Declaration::new(toy::Pair::ID)
        .bind("0", first)
        .bind("1", second)
}

/// R's members `a`, a pair over its siblings `b` and `l`; `b`, a pair over
/// `a` and the input x; and `l`, a cell over `a`.
fn pairs_and_l() -> Members {
    toy::members([
        ("a", pair(toy::sibling(&["b"]), toy::sibling(&["l"]))),
        ("b", pair(toy::sibling(&["a"]), toy::input("x"))),
        ("l", toy::cell_over([toy::sibling(&["a"])])),
    ])
}

fn with_pair() -> Core {
    toy::with_scope()
        .kind::<toy::Pair>()
        .expect("distinct kinds")
        .build()
}

#[test]
fn a_ring_found_through_a_waiting_lookup_answers_cycle_on_every_node_of_it() {
    let x = || [toy::named("x", 5_i64)];
    let (a, b, l) = (r("a"), r("b"), r("l"));
    let s0_members = toy::members([
        ("a", toy::cell(&["x"])),
        ("l", toy::cell_over([toy::sibling(&["a"])])),
    ]);

    let mut core = with_pair();
    let s0 = core.snapshot(scope_r(s0_members, x()));
    assert_eq!(
        run_for(&mut core, &s0, &[&l]),
        [Answer::Final(Outcome::of(5))]
    );
    let s1 = s0.edit([(
        InputKey::named("R.members"),
        Some(toy::input_of(pairs_and_l())),
    )]);
    let mut answers = run_for(&mut core, &s1, &[&l]);
    answers.extend(run_for(&mut core, &s1, &[&a, &b]));
    assert_eq!(answers, [cycle(), cycle(), cycle()]);

    let mut fresh = with_pair();
    let s1 = fresh.snapshot(scope_r(pairs_and_l(), x()));
    let mut answers = run_for(&mut fresh, &s1, &[&l]);
    answers.extend(run_for(&mut fresh, &s1, &[&a, &b]));
    assert_eq!(answers, [cycle(), cycle(), cycle()]);
}

/// Runs one section on the toy kernel, which makes a box it drops, then
/// reads the i64 at "then" and answers it. A read that fails makes it
/// blocked by "then".
struct AfterASection;

impl Kind for AfterASection {
    type Out = i64;
    const ID: KindId = KindId("test.after_a_section");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let _ = cx
            .exclusive(|kernel: &mut ToyKernel, section| kernel.make_box(1, section))
            .await;
        let then = Ref::binding("then");
        Produced::Outcome(match or_blocked(cx.read::<i64>(&then).await, &then) {
            Ok(value) => Outcome::of(*value),
            Err(answer) => answer,
        })
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(toy::Contented::content(out))
    }
}

/// R's members `a`, a pair over its siblings `b` and `c`; `b`, a cell over
/// `a`; and `c`, which reads `a` once its section has run.
fn pair_and_late_reader() -> Members {
    let late = Declaration::new(AfterASection::ID).bind("then", toy::sibling(&["a"]));
    toy::members([
        ("a", pair(toy::sibling(&["b"]), toy::sibling(&["c"]))),
        ("b", toy::cell_over([toy::sibling(&["a"])])),
        ("c", late),
    ])
}

/// The answers at the paths of `requests`, in order, on a gated core of
/// `pair_and_late_reader`. Each request is made and given a turn, which
/// leaves c's section running and answers nothing; then the section is
/// let through and the core runs.
fn answers_once_the_section_has_run(requests: &[&[&Path]]) -> Vec<Answer<i64>> {
    let builder = toy::with_scope()
        .kind::<toy::Pair>()
        .and_then(|builder| builder.kind::<AfterASection>())
        .expect("distinct kinds");
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(builder, &watch);
    let snapshot = core.snapshot(scope_r(pair_and_late_reader(), []));
    let made: Vec<_> = requests
        .iter()
        .map(|paths| {
            let request = core.evaluate(
                &snapshot,
                toy::demand(paths.iter().copied()),
                Budget::unlimited(),
            );
            assert_eq!(core.turn(), Turn::SectionsRunning(1));
            request
        })
        .collect();
    keeper.entered();
    assert!(
        core.take_events().is_empty(),
        "no ring is broken while c's section runs"
    );

    keeper.open();
    core.run();
    let events = core.take_events();
    assert_eq!(core.counters().body_runs(AfterASection::ID), 1);
    let mut answers = Vec::new();
    for (request, paths) in made.iter().zip(requests) {
        answers.extend(
            paths
                .iter()
                .map(|path| toy::answer_at(&events, request, path).downcast::<i64>()),
        );
    }
    for request in made {
        core.end(request);
    }
    answers
}

#[test]
fn rings_are_found_at_rest_so_a_node_that_joins_one_after_a_section_answers_cycle() {
    let (a, b, c) = (r("a"), r("b"), r("c"));
    assert_eq!(
        answers_once_the_section_has_run(&[&[&a, &b, &c]]),
        [cycle(), cycle(), cycle()]
    );
    assert_eq!(
        answers_once_the_section_has_run(&[&[&c], &[&a, &b]]),
        [cycle(), cycle(), cycle()]
    );
}

#[test]
fn a_ring_of_100000_cells_answers_cycle_on_the_default_test_thread_stack() {
    const CELLS: usize = 100_000;
    let names: Vec<String> = (0..CELLS).map(|i| format!("m{i}")).collect();
    let cells = names.iter().enumerate().map(|(i, name)| {
        let next = &names[(i + 1) % CELLS];
        (name.as_str(), toy::cell_over([toy::sibling(&[next])]))
    });
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(scope_r(toy::members(cells), []));

    assert_eq!(run_for(&mut core, &snapshot, &[&r(&names[0])]), [cycle()]);
    let cells = u64::try_from(CELLS).expect("the ring is short enough");
    assert_eq!(core.counters().body_runs(Cell::ID), cells);
}
