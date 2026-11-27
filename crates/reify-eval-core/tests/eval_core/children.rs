//! Children a running body declares (contract 7.1, 7.5, 7.6): each at the
//! path below its declarer at the step the body chose, answered with its
//! own outcome, shared like any node and reachable by path.

use reify_eval_core::{
    Answer, Binding, Budget, Core, Cx, Declaration, Erased, Event, Failure, Hash128, Input,
    InputKey, Kind, KindId, Limit, Outcome, Path, Produced, Ref, RequestError, Snapshot, Space,
    Step, StepKey, Turn, Wanted,
};

use crate::toy::{self, Call, Calls, Cell, Contented, Members};

/// A cell child given its argument at "0".
fn plain_cell() -> Declaration {
    Declaration::new(Cell::ID)
}

/// The inputs of root scope R with `members`, beside `others`.
fn scope_r(
    members: impl IntoIterator<Item = (&'static str, Declaration)>,
    others: impl IntoIterator<Item = (InputKey, Input)>,
) -> Vec<(InputKey, Input)> {
    let mut inputs: Vec<_> = others.into_iter().collect();
    inputs.extend(toy::root_scope("R", toy::members(members)));
    inputs
}

/// The path of the child of R/`member` at `step`.
fn child_of(member: &str, step: Step) -> Path {
    toy::below("R", &[member]).child(step)
}

/// The answers to a fresh request for `paths` on `snapshot`, as i64s.
fn i64s(core: &mut Core, snapshot: &Snapshot, paths: &[&Path]) -> Vec<Answer<i64>> {
    toy::answers_to(core, snapshot, paths)
        .into_iter()
        .map(Answer::downcast)
        .collect()
}

fn value(value: i64) -> Answer<i64> {
    Answer::Final(Outcome::of(value))
}

fn absent() -> Answer<i64> {
    Answer::Final(Outcome::Absent)
}

fn failure(failure: Failure) -> Answer<i64> {
    Answer::Final(Outcome::Failure(failure))
}

/// The inputs of a call R/call over the loads named `terms`, whose one
/// site [1] declares a cell per term.
fn looping_call(terms: &[&str], loads: [i64; 3]) -> Vec<(InputKey, Input)> {
    let call = toy::call(
        terms.iter().map(|term| toy::input(term)),
        toy::input("calls"),
    );
    let calls = Calls(vec![toy::site(&[1], true, 0, plain_cell())]);
    let [l0, l1, l2] = loads;
    scope_r(
        [("call", call)],
        [
            toy::named("calls", calls),
            toy::named("l0", l0),
            toy::named("l1", l1),
            toy::named("l2", l2),
        ],
    )
}

#[test]
fn children_declared_in_a_loop_get_distinct_paths() {
    let mut core = toy::with_calls().build();
    let call = toy::below("R", &["call"]);
    let children: Vec<Path> = (0..3)
        .map(|index| child_of("call", toy::call_step(&[1], Some(index))))
        .collect();
    let paths: Vec<&Path> = [&call].into_iter().chain(&children).collect();

    let snapshot = core.snapshot(looping_call(&["l0", "l1", "l2"], [10, 20, 30]));
    assert_eq!(
        i64s(&mut core, &snapshot, &paths),
        [value(120), value(10), value(20), value(30)]
    );
    let (cells, calls) = (
        core.counters().body_runs(Cell::ID),
        core.counters().body_runs(Call::ID),
    );
    assert_eq!((cells, calls), (3, 1));

    let edited = core.snapshot(looping_call(&["l0", "l1", "l2"], [10, 25, 30]));
    assert_eq!(
        i64s(&mut core, &edited, &paths),
        [value(130), value(10), value(25), value(30)]
    );
    assert_eq!(core.counters().body_runs(Cell::ID), cells + 1);
    assert_eq!(core.counters().body_runs(Call::ID), calls + 1);

    let reordered = core.snapshot(looping_call(&["l2", "l1", "l0"], [10, 25, 30]));
    assert_eq!(
        i64s(&mut core, &reordered, &paths),
        [value(130), value(30), value(25), value(10)]
    );
    assert_eq!(core.counters().body_runs(Cell::ID), cells + 1);
}

/// The inputs of a call R/call over the loads named `terms`, whose sites
/// [1, 7] and [2, 7] each declare a cell, the second given `offset`.
fn two_sites(terms: &[&str], each: bool, offset: i64) -> Vec<(InputKey, Input)> {
    let call = toy::call(
        terms.iter().map(|term| toy::input(term)),
        toy::input("calls"),
    );
    let calls = Calls(vec![
        toy::site(&[1, 7], each, 0, plain_cell()),
        toy::site(&[2, 7], each, offset, plain_cell()),
    ]);
    scope_r(
        [("call", call)],
        [
            toy::named("calls", calls),
            toy::named("l0", 1_i64),
            toy::named("l1", 2_i64),
            toy::named("l2", 4_i64),
        ],
    )
}

#[test]
fn one_inner_call_site_reached_from_two_outer_calls_gets_two_paths() {
    let mut core = toy::with_calls().build();
    let (first, second) = (
        child_of("call", toy::call_step(&[1, 7], None)),
        child_of("call", toy::call_step(&[2, 7], None)),
    );

    let snapshot = core.snapshot(two_sites(&["l0", "l1"], false, 100));
    assert_eq!(
        i64s(&mut core, &snapshot, &[&first, &second]),
        [value(3), value(103)]
    );
    let cells = core.counters().body_runs(Cell::ID);
    assert_eq!(cells, 2);

    let edited = core.snapshot(two_sites(&["l0", "l1"], false, 200));
    assert_eq!(
        i64s(&mut core, &edited, &[&first, &second]),
        [value(3), value(203)]
    );
    assert_eq!(core.counters().body_runs(Cell::ID), cells + 1);

    let at = |sites: &[u64], index| child_of("call", toy::call_step(sites, Some(index)));
    let looping = [
        at(&[1, 7], 0),
        at(&[1, 7], 2),
        at(&[2, 7], 1),
        at(&[2, 7], 2),
    ];
    let looping: Vec<&Path> = looping.iter().collect();
    let snapshot = core.snapshot(two_sites(&["l0", "l1", "l2"], true, 0));
    assert_eq!(
        i64s(&mut core, &snapshot, &looping),
        [value(1), value(4), value(2), value(4)]
    );
    let filtered = core.snapshot(two_sites(&["l0", "l2"], true, 0));
    assert_eq!(
        i64s(&mut core, &filtered, &looping),
        [value(1), absent(), value(4), absent()]
    );
}

#[test]
fn a_child_a_body_declares_on_one_branch_answers_absent_on_the_other() {
    let mut core = toy::with_calls().build();
    let inputs = |when: bool| {
        let call =
            toy::call([toy::input("five")], toy::input("calls")).bind("when", toy::input("w"));
        let calls = Calls(vec![toy::site(&[1], false, 1, plain_cell())]);
        scope_r(
            [("call", call)],
            [
                toy::named("calls", calls),
                toy::named("five", 5_i64),
                toy::named("w", when),
            ],
        )
    };
    let call = toy::below("R", &["call"]);
    let child = child_of("call", toy::call_step(&[1], None));

    let snapshot = core.snapshot(inputs(false));
    assert_eq!(
        i64s(&mut core, &snapshot, &[&call, &child]),
        [value(5), absent()]
    );
    let snapshot = core.snapshot(inputs(true));
    assert_eq!(
        i64s(&mut core, &snapshot, &[&call, &child]),
        [value(11), value(6)]
    );
}

#[test]
fn a_body_declared_child_demanded_before_its_parent_ran_answers_the_child() {
    let mut core = toy::with_calls().build();
    let call = toy::call([toy::input("five")], toy::input("calls"));
    let calls = Calls(vec![toy::site(&[1], false, 1, plain_cell())]);
    let snapshot = core.snapshot(scope_r(
        [("call", call)],
        [toy::named("calls", calls), toy::named("five", 5_i64)],
    ));
    let child = child_of("call", toy::call_step(&[1], None));

    assert_eq!(i64s(&mut core, &snapshot, &[&child]), [value(6)]);
    assert_eq!(core.counters().body_runs(Call::ID), 1);
}

/// A space of the test's own, beside the default one.
const OWN: Space = Space::Named("test.children.own");

/// Declares the member "a" by its value, a cell over the constant 1, and,
/// by its body, a child at "a" in its own space: a cell over the constant 2.
struct Both;

/// A cell over the constant `value`.
fn constant_cell(value: i64) -> Declaration {
    toy::cell_over([Binding::Constant(toy::input_of(value))])
}

impl Kind for Both {
    type Out = Members;
    const ID: KindId = KindId("test.both");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Members> {
        let own = Step::new(OWN, StepKey::Name("a".into()));
        Produced::Outcome(match cx.child::<i64>(own, constant_cell(2)).await {
            Outcome::Value(_) => Outcome::of(toy::members([("a", constant_cell(1))])),
            Outcome::Failure(failure) => Outcome::Failure(failure),
            Outcome::Absent => Outcome::Absent,
        })
    }

    fn content(out: &Members) -> Option<Hash128> {
        Some(out.content())
    }

    fn declares(out: &Members, step: &Step) -> Option<Declaration> {
        out.0.get(step).cloned()
    }

    fn steps(out: &Members) -> impl Iterator<Item = Step> {
        out.0.keys().cloned()
    }
}

#[test]
fn a_body_declared_child_and_a_member_with_equal_keys_in_two_spaces_are_two_paths() {
    let mut core = toy::with_calls()
        .kind::<Both>()
        .expect("a kind of the test's own")
        .build();
    let snapshot = core.snapshot(scope_r([("both", Declaration::new(Both::ID))], []));
    let (member, child) = (
        child_of("both", Step::named("a")),
        child_of("both", Step::new(OWN, StepKey::Name("a".into()))),
    );

    assert_eq!(
        i64s(&mut core, &snapshot, &[&member, &child]),
        [value(1), value(2)]
    );
}

/// The inputs of a ring through a body-declared child: X reads P/s, P
/// declares s, and the child reads X through P's binding "x".
fn ring_through_a_child() -> Vec<(InputKey, Input)> {
    let s = toy::call_step(&[1], None);
    let x = toy::cell_over([Binding::Declarer(vec![Step::named("P"), s])]);
    let child = toy::cell_over([Binding::Passed(Ref::binding("x"))]);
    let mut site = toy::site(&[1], false, 0, child);
    site.argument = String::from("given");
    let p = toy::call([], toy::input("calls")).bind("x", toy::sibling(&["X"]));
    scope_r(
        [("X", x), ("P", p)],
        [toy::named("calls", Calls(vec![site]))],
    )
}

#[test]
fn a_ring_through_a_body_declared_child_leaves_its_declarer_off_the_ring() {
    let s = toy::call_step(&[1], None);
    let (x, p, child) = (
        toy::below("R", &["X"]),
        toy::below("R", &["P"]),
        child_of("P", s.clone()),
    );
    let expected = [
        failure(Failure::Cycle),
        failure(toy::blocked_by_child(s)),
        failure(Failure::Cycle),
    ];

    for first in [&x, &p] {
        let mut core = toy::with_calls().build();
        let snapshot = core.snapshot(ring_through_a_child());
        i64s(&mut core, &snapshot, &[first]);
        let answers = i64s(&mut core, &snapshot, &[&x, &p, &child]);
        assert_eq!(answers, expected, "{first:?} demanded first");
    }
}

/// The inputs of R/P, a call whose one site [1] declares a cell given the
/// call's sum, five, plus one.
fn one_child() -> Vec<(InputKey, Input)> {
    let p = toy::call([toy::input("five")], toy::input("calls"));
    let calls = Calls(vec![toy::site(&[1], false, 1, plain_cell())]);
    scope_r(
        [("P", p)],
        [toy::named("calls", calls), toy::named("five", 5_i64)],
    )
}

#[test]
fn a_body_declared_child_past_the_depth_setting_answers_too_deep() {
    let mut core = toy::with_calls().build();
    let mut inputs = one_child();
    inputs.push(toy::limit(Limit::Depth, 1));
    let snapshot = core.snapshot(inputs);
    let s = toy::call_step(&[1], None);
    let (p, child) = (toy::below("R", &["P"]), child_of("P", s.clone()));

    assert_eq!(
        i64s(&mut core, &snapshot, &[&p, &child]),
        [
            failure(toy::blocked_by_child(s)),
            failure(Failure::TooDeep { limit: 1 }),
        ]
    );
}

#[test]
fn a_body_declared_child_is_charged_to_the_paths_budget() {
    let p = toy::below("R", &["P"]);
    let ended = |events: &[Event]| -> Vec<RequestError> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Ended { error, .. } => Some(error.clone()),
                _ => None,
            })
            .collect()
    };

    let mut core = toy::with_calls().build();
    let snapshot = core.snapshot(one_child());
    let (request, events) = toy::evaluate_within(&mut core, &snapshot, &[&p], Budget::paths(3));
    assert!(ended(&events).is_empty(), "{events:?}");
    assert_eq!(
        toy::answer_at(&events, &request, &p).downcast::<i64>(),
        value(11)
    );
    assert_eq!(core.counters().paths_resolved(), 3);

    let mut core = toy::with_calls().build();
    let snapshot = core.snapshot(one_child());
    let (request, events) = toy::evaluate_within(&mut core, &snapshot, &[&p], Budget::paths(2));
    assert_eq!(
        ended(&events),
        [RequestError::PathsBudgetExhausted { budget: 2 }]
    );
    assert!(toy::answers(&events, request.id()).is_empty(), "{events:?}");
}

/// A resource of its own for each `N`.
struct Kernel<const N: usize>;

/// Makes, on resource `Kernel<N>`, the i64 at "side" times `N + 1`.
struct Made<const N: usize>;

impl<const N: usize> Kind for Made<N> {
    type Out = i64;
    const ID: KindId = KindId(["test.made.0", "test.made.1", "test.made.2"][N]);
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let side = match cx.read::<i64>(&Ref::binding("side")).await {
            Outcome::Value(side) => *side,
            other => return Produced::Outcome(other),
        };
        let factor = i64::try_from(N + 1).expect("a small factor");
        let made = cx
            .exclusive(move |_: &mut Kernel<N>, _| side * factor)
            .await;
        Produced::Outcome(Outcome::of(made))
    }
}

/// Declares one `Made` child on each of the three kernels, in one
/// `read_all`, and answers the sum of what they make.
struct Three;

impl Kind for Three {
    type Out = i64;
    const ID: KindId = KindId("test.three");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let made = |kind: KindId| Declaration::new(kind).bind("side", toy::input("side"));
        let step = |index| Step::new(Space::Named("test.three"), StepKey::Index(index));
        let children = [
            Wanted::child(step(0), made(Made::<0>::ID)),
            Wanted::child(step(1), made(Made::<1>::ID)),
            Wanted::child(step(2), made(Made::<2>::ID)),
        ];
        let mut sum = 0;
        for read in cx.read_all::<i64>(&children).await {
            match read {
                Outcome::Value(value) => sum += *value,
                other => return Produced::Outcome(other),
            }
        }
        Produced::Outcome(Outcome::of(sum))
    }
}

#[test]
fn three_children_declared_in_one_read_all_on_three_resources_start_before_the_parent_resumes() {
    let builder = reify_eval_core::Builder::new()
        .kind::<Three>()
        .and_then(|builder| builder.kind::<Made<0>>())
        .and_then(|builder| builder.kind::<Made<1>>())
        .and_then(|builder| builder.kind::<Made<2>>())
        .expect("distinct kinds");
    let mut core = builder.build();
    core.add_resource(|| Kernel::<0>);
    core.add_resource(|| Kernel::<1>);
    core.add_resource(|| Kernel::<2>);
    let snapshot = core.snapshot([
        toy::named("side", 2_i64),
        toy::root("T", Declaration::new(Three::ID)),
    ]);
    let at = toy::at("T");

    let request = core.evaluate(&snapshot, toy::demand([&at]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::SectionsRunning(3));
    assert_eq!(core.counters().sections(), 3);
    let told: Vec<(Path, Answer<Erased>)> = toy::answers(&core.take_events(), request.id());
    assert!(told.is_empty(), "{told:?}");

    core.run();
    let answer = toy::the_answer(&core.take_events(), &request, &at);
    assert_eq!(answer.downcast::<i64>(), value(12));
    assert_eq!(core.counters().body_runs(Three::ID), 1);
}

/// The space of `Twice`'s one child step.
const TWICE: Space = Space::Named("test.twice");

/// The step `Twice` declares both its declarations at.
fn twice_step() -> Step {
    Step::new(TWICE, StepKey::Index(0))
}

/// Declares the declaration at "first", then the one at "second", both at
/// `twice_step`: in two requests, or in one `read_all` where "together"
/// reads true. It answers the sum of the two reads.
struct Twice;

impl Kind for Twice {
    type Out = i64;
    const ID: KindId = KindId("test.twice");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let first = cx.read::<Declaration>(&Ref::binding("first")).await;
        let second = cx.read::<Declaration>(&Ref::binding("second")).await;
        let (Outcome::Value(first), Outcome::Value(second)) = (first, second) else {
            return Produced::Outcome(Outcome::Absent);
        };
        let together = cx.read::<bool>(&Ref::binding("together")).await;
        let reads = if matches!(together, Outcome::Value(together) if *together) {
            let both = [
                Wanted::Child(twice_step(), first),
                Wanted::Child(twice_step(), second),
            ];
            cx.read_all::<i64>(&both).await
        } else {
            vec![
                cx.child::<i64>(twice_step(), (*first).clone()).await,
                cx.child::<i64>(twice_step(), (*second).clone()).await,
            ]
        };
        let mut sum = 0;
        for read in reads {
            match read {
                Outcome::Value(value) => sum += *value,
                other => return Produced::Outcome(other),
            }
        }
        Produced::Outcome(Outcome::of(sum))
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(out.content())
    }
}

/// The inputs of two identical `Twice` members of R, P and Q, declaring
/// cells over `first` and `second`, together or not.
fn twice(first: i64, second: i64, together: bool) -> Vec<(InputKey, Input)> {
    let member = || {
        Declaration::new(Twice::ID)
            .bind("first", toy::input("first"))
            .bind("second", toy::input("second"))
            .bind("together", toy::input("together"))
    };
    scope_r(
        [("P", member()), ("Q", member())],
        [
            toy::named("first", constant_cell(first)),
            toy::named("second", constant_cell(second)),
            toy::named("together", together),
        ],
    )
}

fn with_twice() -> Core {
    toy::with_calls()
        .kind::<Twice>()
        .expect("a kind of the test's own")
        .build()
}

#[test]
fn the_same_declaration_at_one_step_twice_in_one_run_is_one_child() {
    for together in [false, true] {
        let mut core = with_twice();
        let snapshot = core.snapshot(twice(7, 7, together));
        let (p, child) = (toy::below("R", &["P"]), child_of("P", twice_step()));

        assert_eq!(
            i64s(&mut core, &snapshot, &[&p, &child]),
            [value(14), value(7)],
            "together: {together}"
        );
        assert_eq!(
            core.counters().body_runs(Cell::ID),
            1,
            "together: {together}"
        );
    }
}

#[test]
fn two_declarations_at_one_step_in_one_run_fail_the_run() {
    let conflict = || failure(Failure::ConflictingChild { step: twice_step() });
    for together in [false, true] {
        let mut core = with_twice();
        let snapshot = core.snapshot(twice(7, 8, together));
        let (p, child) = (toy::below("R", &["P"]), child_of("P", twice_step()));

        let first_declared = if together { conflict() } else { value(7) };
        assert_eq!(
            i64s(&mut core, &snapshot, &[&p, &child]),
            [conflict(), first_declared],
            "together: {together}"
        );
        if together {
            let runs = core.counters().body_runs(Twice::ID);
            let q = toy::below("R", &["Q"]);
            assert_eq!(i64s(&mut core, &snapshot, &[&q]), [conflict()]);
            assert_eq!(core.counters().body_runs(Twice::ID), runs, "stored");
        }
    }
}

/// The inputs of R with identical members c1 and c2, each a scope whose
/// member "result" is a call over the input "a" with `calls`, and of an
/// input nothing reads, "unrelated".
fn shared_calls(a: i64, calls: Calls, unrelated: i64) -> Vec<(InputKey, Input)> {
    let result = toy::call([toy::input("a")], toy::input("calls"));
    let inner = toy::members([("result", result)]);
    scope_r(
        [("c1", toy::scope("inner")), ("c2", toy::scope("inner"))],
        [
            toy::named("inner", inner),
            toy::named("a", a),
            toy::named("calls", calls),
            toy::named("unrelated", unrelated),
        ],
    )
}

fn result_of(member: &str) -> Path {
    toy::below("R", &[member, "result"])
}

#[test]
fn body_declared_child_is_shared_and_resolvable_without_rerunning_its_parent() {
    let calls = || Calls(vec![toy::site(&[1], false, 0, plain_cell())]);
    let mut core = toy::with_calls().build();
    let snapshot = core.snapshot(shared_calls(4, calls(), 0));
    let (first, second) = (result_of("c1"), result_of("c2"));

    assert_eq!(i64s(&mut core, &snapshot, &[&first]), [value(8)]);
    assert_eq!(i64s(&mut core, &snapshot, &[&second]), [value(8)]);
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
    assert_eq!(core.counters().body_runs(Call::ID), 1);

    let child = second.child(toy::call_step(&[1], None));
    assert_eq!(i64s(&mut core, &snapshot, &[&child]), [value(4)]);
    let edited = core.snapshot(shared_calls(4, calls(), 1));
    assert_eq!(i64s(&mut core, &edited, &[&child]), [value(4)]);
    assert_eq!(core.counters().body_runs(Call::ID), 1);
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}

#[test]
fn an_identical_parent_s_lookup_declares_its_children_so_their_paths_hold() {
    let calls = Calls(vec![toy::site(&[1], true, 0, plain_cell())]);
    let mut core = toy::with_calls().build();
    let snapshot = core.snapshot(shared_calls(4, calls, 0));
    let second = result_of("c2");
    let children: Vec<Path> = (0..2)
        .map(|index| second.child(toy::call_step(&[1], Some(index))))
        .collect();

    assert_eq!(i64s(&mut core, &snapshot, &[&result_of("c1")]), [value(8)]);
    assert_eq!(
        i64s(&mut core, &snapshot, &[&children[0], &children[1], &second]),
        [value(4), absent(), value(8)]
    );
    assert_eq!(core.counters().body_runs(Call::ID), 1);
}

#[test]
fn an_identical_parent_of_a_run_that_conflicted_across_two_requests_finds_the_stored_failure() {
    let mut core = with_twice();
    let snapshot = core.snapshot(twice(7, 8, false));
    let (p, q) = (toy::below("R", &["P"]), toy::below("R", &["Q"]));
    let conflict = failure(Failure::ConflictingChild { step: twice_step() });

    assert_eq!(i64s(&mut core, &snapshot, &[&p]), [conflict.clone()]);
    let runs = core.counters().body_runs(Twice::ID);
    assert_eq!(
        i64s(&mut core, &snapshot, &[&q, &child_of("Q", twice_step())]),
        [conflict, value(7)]
    );
    assert_eq!(core.counters().body_runs(Twice::ID), runs);
}
