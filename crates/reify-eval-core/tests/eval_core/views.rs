//! Views (contract 6): a body supposes outcomes at places and reads through
//! the view it opens, each read telling whether its answer depended on the
//! substitution, while the surrounding state is left as it was.

use std::marker::PhantomData;

use std::fmt::Debug;

use reify_eval_core::{
    Answer, Binding, Budget, Builder, Core, Cx, Declaration, Erased, Failure, Hash128, Input,
    InputKey, Kind, KindId, Outcome, OutsideFailure, Path, Produced, Ref, Snapshot, Step,
    Substitution, Turn, Value,
};

use crate::toy::{self, Cell, Contented, Members, Pair, Part, Shape, Solution, Solve, Watch};

/// What one read through a view gave, rendered comparably.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Int(i64),
    /// A value of a scope or guard: its members.
    Present,
    Absent,
    Failed(Failure),
}

/// What a read through a view gave, and whether it depended on the
/// substitution.
#[derive(Debug, Clone, PartialEq)]
struct Saw {
    seen: Seen,
    depended: bool,
}

/// A read a probe makes through a view, with the type it reads as.
enum Read {
    Int(Ref),
    Members(Ref),
}

/// One view a probe opens, and what it reads through it, in order.
struct Trial {
    substitution: Substitution,
    reads: Vec<Read>,
}

/// What a probe does: the views it opens, in order.
trait Plan: 'static {
    const ID: KindId;

    fn trials() -> Vec<Trial>;
}

/// Opens the views its plan gives, reading nothing before, and reads
/// through each what the plan says. Its value is what each read gave, in
/// order.
struct Probe<P>(PhantomData<fn() -> P>);

impl<P: Plan> Kind for Probe<P> {
    type Out = Vec<Saw>;
    const ID: KindId = P::ID;
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Vec<Saw>> {
        let mut saw = Vec::new();
        for trial in P::trials() {
            let view = cx.suppose(trial.substitution);
            for read in &trial.reads {
                saw.push(match read {
                    Read::Int(r) => {
                        let viewed = view.read::<i64>(r).await;
                        let seen = seen(viewed.outcome, |value| Seen::Int(*value));
                        Saw {
                            seen,
                            depended: viewed.depended,
                        }
                    }
                    Read::Members(r) => {
                        let viewed = view.read::<Members>(r).await;
                        let seen = seen(viewed.outcome, |_| Seen::Present);
                        Saw {
                            seen,
                            depended: viewed.depended,
                        }
                    }
                });
            }
        }
        Produced::Outcome(Outcome::of(saw))
    }
}

fn seen<T>(outcome: Outcome<T>, value: impl FnOnce(&T) -> Seen) -> Seen {
    match outcome {
        Outcome::Value(read) => value(&read),
        Outcome::Absent => Seen::Absent,
        Outcome::Failure(failure) => Seen::Failed(failure),
    }
}

/// A probe of plan `P` whose bindings name the root's members `names`, each
/// bound under its own name.
fn probe<P: Plan>(names: &[&str]) -> Declaration {
    names
        .iter()
        .fold(Declaration::new(Probe::<P>::ID), |declaration, name| {
            declaration.bind(*name, toy::sibling(&[name]))
        })
}

/// Reads the i64 at "x" and answers whether it is greater than the i64 at
/// "than".
struct Exceeds;

impl Kind for Exceeds {
    type Out = bool;
    const ID: KindId = KindId("views.exceeds");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<bool> {
        let x = match cx.read::<i64>(&Ref::binding("x")).await {
            Outcome::Value(x) => *x,
            Outcome::Absent => return Produced::Outcome(Outcome::Absent),
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
        };
        let than = match cx.read::<i64>(&Ref::binding("than")).await {
            Outcome::Value(than) => *than,
            Outcome::Absent => return Produced::Outcome(Outcome::Absent),
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
        };
        Produced::Outcome(Outcome::of(x > than))
    }
}

fn constant(value: i64) -> Binding {
    Binding::Constant(toy::input_of(value))
}

fn supposed(value: i64) -> Outcome<Input> {
    Outcome::of(toy::input_of(value))
}

fn int(name: &str) -> Read {
    Read::Int(Ref::binding(name))
}

fn saw(seen: Seen, depended: bool) -> Saw {
    Saw { seen, depended }
}

fn with_probe<P: Plan>(builder: Builder) -> Builder {
    builder
        .kind::<Probe<P>>()
        .expect("a probe's kind id is its own")
}

/// The value the probe at root/probe answers.
fn probed(core: &mut Core, snapshot: &Snapshot) -> Vec<Saw> {
    let at = toy::below("root", &["probe"]);
    match toy::answer_alone(core, snapshot, &at).downcast::<Vec<Saw>>() {
        Answer::Final(Outcome::Value(saw)) => (*saw).clone(),
        other => panic!("the probe answers a value, and answered {other:?}"),
    }
}

/// x is a cell over the input "x0", y = x + 1 and z a cell over the input
/// "five", all members of root scope "root" with `probe`.
fn x_y_z(probe: Declaration) -> [(InputKey, Input); 4] {
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            ("x", toy::cell(&["x0"])),
            ("y", toy::cell_over([toy::sibling(&["x"]), constant(1)])),
            ("z", toy::cell(&["five"])),
            ("probe", probe),
        ]),
    );
    [
        members,
        root,
        toy::named("x0", 10_i64),
        toy::named("five", 5_i64),
    ]
}

struct ThreeForX;

impl Plan for ThreeForX {
    const ID: KindId = KindId("views.probe.three_for_x");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("x"), supposed(3))]),
            reads: vec![int("y"), int("z")],
        }]
    }
}

#[test]
fn view_reports_which_answers_depended_on_substitution() {
    let mut core = with_probe::<ThreeForX>(toy::with_scope()).build();
    let snapshot = core.snapshot(x_y_z(probe::<ThreeForX>(&["x", "y", "z"])));

    assert_eq!(
        probed(&mut core, &snapshot),
        vec![saw(Seen::Int(4), true), saw(Seen::Int(5), false)]
    );
}

#[test]
fn reads_through_a_view_leave_the_surrounding_state_untouched() {
    let mut core = with_probe::<ThreeForX>(toy::with_scope()).build();
    let snapshot = core.snapshot(x_y_z(probe::<ThreeForX>(&["x", "y", "z"])));
    probed(&mut core, &snapshot);

    let (y, x) = (toy::below("root", &["y"]), toy::below("root", &["x"]));
    let answers = toy::answers_to(&mut core, &snapshot, &[&y, &x]);
    let values: Vec<_> = answers
        .into_iter()
        .map(|answer| answer.downcast::<i64>())
        .collect();
    assert_eq!(
        values,
        vec![
            Answer::Final(Outcome::of(11)),
            Answer::Final(Outcome::of(10))
        ]
    );
}

struct ThinTrials;

impl Plan for ThinTrials {
    const ID: KindId = KindId("views.probe.thin_trials");

    fn trials() -> Vec<Trial> {
        let t = Ref::binding("thin").step(Step::named("t"));
        [4, 6]
            .into_iter()
            .map(|x| Trial {
                substitution: Substitution::of([
                    (Ref::binding("x"), supposed(x)),
                    (t.clone(), supposed(3)),
                ]),
                reads: vec![
                    Read::Members(Ref::binding("thin")),
                    Read::Int(t.clone()),
                    int("constraint"),
                ],
            })
            .collect()
    }
}

#[test]
fn a_substitution_under_an_absent_parent_answers_absent() {
    let builder = toy::with_structure()
        .kind::<Exceeds>()
        .expect("toy kind ids are distinct");
    let mut core = with_probe::<ThinTrials>(builder).build();
    let exceeds = Declaration::new(Exceeds::ID)
        .bind("x", toy::sibling(&["x"]))
        .bind("than", constant(5));
    let constraint = toy::cell_over([toy::sibling(&["thin", "t"]), toy::sibling(&["x"])]);
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            ("x", toy::cell(&["x0"])),
            ("exceeds", exceeds),
            (
                "thin",
                toy::guard(toy::sibling(&["exceeds"]), "thin.members"),
            ),
            ("constraint", constraint),
            ("probe", probe::<ThinTrials>(&["x", "thin", "constraint"])),
        ]),
    );
    let snapshot = core.snapshot([
        members,
        root,
        toy::named("x0", 10_i64),
        toy::named("t0", 1_i64),
        toy::named("thin.members", toy::members([("t", toy::cell(&["t0"]))])),
    ]);

    assert_eq!(
        probed(&mut core, &snapshot),
        vec![
            saw(Seen::Absent, true),
            saw(Seen::Absent, true),
            saw(Seen::Int(0), true),
            saw(Seen::Present, true),
            saw(Seen::Int(3), true),
            saw(Seen::Int(9), true),
        ]
    );
}

struct FourElements;

impl Plan for FourElements {
    const ID: KindId = KindId("views.probe.four_elements");

    fn trials() -> Vec<Trial> {
        let element = |index| Read::Int(Ref::binding("items").step(Step::index(index)));
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("n"), supposed(4))]),
            reads: vec![element(3), element(4)],
        }]
    }
}

#[test]
fn a_substituted_count_declares_that_many_elements() {
    let mut core = with_probe::<FourElements>(toy::with_structure()).build();
    let items = toy::collection(toy::sibling(&["n"]), toy::input("element"));
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            ("n", toy::cell(&["n0"])),
            ("items", items),
            ("probe", probe::<FourElements>(&["n", "items"])),
        ]),
    );
    let snapshot = core.snapshot([
        members,
        root,
        toy::named("n0", 2_i64),
        toy::named("seven", 7_i64),
        toy::named("element", toy::cell(&["seven"])),
    ]);

    assert_eq!(
        probed(&mut core, &snapshot),
        vec![saw(Seen::Int(7), true), saw(Seen::Absent, true)]
    );
}

struct SupposesAnInput;

impl Plan for SupposesAnInput {
    const ID: KindId = KindId("views.probe.supposes_an_input");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("five"), supposed(3))]),
            reads: vec![int("y")],
        }]
    }
}

struct SupposesNothing;

impl Plan for SupposesNothing {
    const ID: KindId = KindId("views.probe.supposes_nothing");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("unset"), supposed(3))]),
            reads: vec![int("y")],
        }]
    }
}

struct SupposesOnePlaceTwice;

impl Plan for SupposesOnePlaceTwice {
    const ID: KindId = KindId("views.probe.supposes_one_place_twice");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([
                (Ref::binding("x"), supposed(3)),
                (Ref::binding("also_x"), supposed(4)),
            ]),
            reads: vec![int("y")],
        }]
    }
}

/// The final answer of the probe of plan `P` in the x, y, z design, its
/// binding "five" naming the input "five" and "also_x" naming x again.
fn not_supposable<P: Plan>() -> Answer<Erased> {
    let mut core = with_probe::<P>(toy::with_scope()).build();
    let probe = probe::<P>(&["x", "y"])
        .bind("five", toy::input("five"))
        .bind("also_x", toy::sibling(&["x"]));
    let snapshot = core.snapshot(x_y_z(probe));
    toy::answer_alone(&mut core, &snapshot, &toy::below("root", &["probe"]))
}

#[test]
fn a_substitution_naming_no_place_of_its_own_fails_the_run() {
    let cases = [
        (not_supposable::<SupposesAnInput>(), "five"),
        (not_supposable::<SupposesNothing>(), "unset"),
        (not_supposable::<SupposesOnePlaceTwice>(), "also_x"),
    ];
    for (answer, at) in cases {
        assert_eq!(
            toy::failed(answer),
            Failure::NotSupposable {
                at: Ref::binding(at)
            }
        );
    }
}

struct Inner;

impl Plan for Inner {
    const ID: KindId = KindId("views.probe.inner");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("x"), supposed(1))]),
            reads: vec![int("y")],
        }]
    }
}

struct Outer;

impl Plan for Outer {
    const ID: KindId = KindId("views.probe.outer");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("x"), supposed(3))]),
            reads: vec![int("inner")],
        }]
    }
}

#[test]
fn a_view_opened_under_a_view_ends_outside_the_inputs() {
    let builder = with_probe::<Inner>(toy::with_scope());
    let mut core = with_probe::<Outer>(builder).build();
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            ("x", toy::cell(&["x0"])),
            ("y", toy::cell_over([toy::sibling(&["x"]), constant(1)])),
            ("inner", probe::<Inner>(&["x", "y"])),
            ("probe", probe::<Outer>(&["x", "inner"])),
        ]),
    );
    let snapshot = core.snapshot([members, root, toy::named("x0", 10_i64)]);

    let answer = toy::answer_alone(&mut core, &snapshot, &toy::below("root", &["probe"]));
    assert_eq!(toy::pending(answer), OutsideFailure::ViewInsideView);
}

/// The value of `answer`, which must be a final value of `T`.
fn value_of<T: Value + Clone + Debug>(answer: Answer<Erased>) -> T {
    match answer.downcast::<T>() {
        Answer::Final(Outcome::Value(value)) => (*value).clone(),
        other => panic!("expected a final value, got {other:?}"),
    }
}

fn member(name: &str) -> Path {
    toy::below("root", &[name])
}

/// Scenario K8: a solve over an int unknown n, the count of the collection
/// "items", and a flag unknown f, the condition of the guard g that
/// declares cell m. Constraint c0 holds where items/[2] is present, at a
/// count of 3 or more; c1 holds where g/m is present, where f holds.
fn count_and_guard(problem: toy::Problem) -> Vec<(InputKey, Input)> {
    let solve = toy::sibling(&["solve"]);
    let unknowns = [toy::sibling(&["n"]), toy::sibling(&["f"])];
    let constraints = [toy::sibling(&["c0"]), toy::sibling(&["c1"])];
    let element_2 = Binding::Declarer(vec![Step::named("items"), Step::index(2)]);
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            (
                "solve",
                toy::solve(toy::input("problem"), unknowns, constraints),
            ),
            ("n", toy::solved_int(solve.clone(), 0)),
            ("f", toy::solved_flag(solve, 1)),
            (
                "items",
                toy::collection(toy::sibling(&["n"]), toy::input("element")),
            ),
            ("g", toy::guard(toy::sibling(&["f"]), "g.members")),
            ("c0", toy::cell_over([constant(-7), element_2])),
            (
                "c1",
                toy::cell_over([constant(-5), toy::sibling(&["g", "m"])]),
            ),
        ]),
    );
    vec![
        members,
        root,
        toy::named("problem", problem),
        toy::named("seven", 7_i64),
        toy::named("element", toy::cell(&["seven"])),
        toy::named("g.members", toy::members([("m", toy::cell(&["seven"]))])),
    ]
}

/// Candidates of which only the last holds: the first fails c0 by its
/// count, the second c1 by its guard.
fn count_and_guard_candidates() -> toy::Problem {
    let candidates = [(1, true), (3, false), (3, true)]
        .map(|(count, flag)| vec![Part::Int(count), Part::Flag(flag)]);
    toy::problem(candidates, 2)
}

#[test]
fn a_solve_over_a_cell_a_guard_and_a_count_answers_its_first_holding_candidate() {
    let mut core = toy::with_solve().build();
    let snapshot = core.snapshot(count_and_guard(count_and_guard_candidates()));
    let element_2 = member("items").child(Step::index(2));
    let (solve, n, f) = (member("solve"), member("n"), member("f"));
    let (m, c0, c1) = (toy::below("root", &["g", "m"]), member("c0"), member("c1"));

    let answers = toy::answers_to(&mut core, &snapshot, &[&solve, &n, &f]);
    let [solution, n_answer, f_answer] = <[_; 3]>::try_from(answers).expect("three answers");
    assert_eq!(
        value_of::<Solution>(solution),
        Solution(vec![Part::Int(3), Part::Flag(true)])
    );
    assert_eq!(value_of::<i64>(n_answer), 3);
    assert!(value_of::<bool>(f_answer));

    let real = toy::answers_to(&mut core, &snapshot, &[&element_2, &m, &c0, &c1]);
    let real: Vec<i64> = real.into_iter().map(value_of::<i64>).collect();
    assert_eq!(real, vec![7, 7, 0, 2]);
}

#[test]
fn a_solve_over_a_call_declares_the_call_s_child_under_each_trial() {
    let builder = toy::with_solve()
        .kind::<toy::Call>()
        .expect("toy kind ids are distinct");
    let mut core = builder.build();
    let child = toy::call_step(&[1], None);
    let at_least_three = toy::cell_over([
        constant(-3),
        Binding::Declarer(vec![Step::named("call"), child.clone()]),
    ]);
    let calls = toy::Calls(vec![toy::site(&[1], false, 0, Declaration::new(Cell::ID))]);
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            (
                "solve",
                toy::solve(
                    toy::input("problem"),
                    [toy::sibling(&["k"])],
                    [toy::sibling(&["c0"])],
                ),
            ),
            ("k", toy::solved_int(toy::sibling(&["solve"]), 0)),
            (
                "call",
                toy::call([toy::sibling(&["k"])], toy::input("calls")),
            ),
            ("c0", at_least_three),
        ]),
    );
    let candidates = [1, 2, 3, 4].map(|k| vec![Part::Int(k)]);
    let snapshot = core.snapshot([
        members,
        root,
        toy::named("problem", toy::problem(candidates, 1)),
        toy::named("calls", calls),
    ]);

    let (solve, call) = (member("solve"), member("call"));
    let answers = toy::answers_to(&mut core, &snapshot, &[&solve, &call]);
    let [solution, call_answer] = <[_; 2]>::try_from(answers).expect("two answers");
    assert_eq!(value_of::<Solution>(solution), Solution(vec![Part::Int(3)]));
    assert_eq!(value_of::<i64>(call_answer), 6);
    let child_answer = toy::answer_alone(&mut core, &snapshot, &call.child(child));
    assert_eq!(value_of::<i64>(child_answer), 3);
}

#[test]
fn a_view_opener_runs_again_in_each_new_snapshot() {
    let mut core = toy::with_solve().build();
    let inputs = count_and_guard(count_and_guard_candidates());
    let snapshot = core.snapshot(inputs.clone());
    let (solve, n, f) = (member("solve"), member("n"), member("f"));

    toy::answers_to(&mut core, &snapshot, &[&solve, &n, &f]);
    toy::answer_alone(&mut core, &snapshot, &solve);
    assert_eq!(core.counters().body_runs(Solve::ID), 1);

    let edited = inputs.into_iter().chain([toy::named("unrelated", 1_i64)]);
    let snapshot = core.snapshot(edited);
    toy::answer_alone(&mut core, &snapshot, &solve);
    assert_eq!(core.counters().body_runs(Solve::ID), 2);
}

#[test]
fn results_under_a_view_are_stored_only_when_they_did_not_depend() {
    let builder = toy::with_solve()
        .kind::<Pair>()
        .expect("toy kind ids are distinct");
    let mut core = builder.build();
    let plain = Declaration::new(Pair::ID)
        .bind("0", toy::input("five"))
        .bind("1", toy::input("five"));
    let at_least_two = toy::cell_over([toy::sibling(&["u"]), constant(-2)]);
    let constraints = [toy::sibling(&["plain"]), toy::sibling(&["at_least_two"])];
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            (
                "solve",
                toy::solve(toy::input("problem"), [toy::sibling(&["u"])], constraints),
            ),
            ("u", toy::solved_int(toy::sibling(&["solve"]), 0)),
            ("plain", plain),
            ("at_least_two", at_least_two),
        ]),
    );
    let candidates = [1, 2].map(|u| vec![Part::Int(u)]);
    let inputs = vec![
        members,
        root,
        toy::named("five", 5_i64),
        toy::named("problem", toy::problem(candidates, 2)),
    ];
    let solve = member("solve");

    let snapshot = core.snapshot(inputs.clone());
    let solution = toy::answer_alone(&mut core, &snapshot, &solve);
    assert_eq!(value_of::<Solution>(solution), Solution(vec![Part::Int(2)]));
    let counters = core.counters();
    assert_eq!(
        (counters.body_runs(Pair::ID), counters.body_runs(Cell::ID)),
        (1, 2)
    );

    let edited = inputs.into_iter().chain([toy::named("unrelated", 1_i64)]);
    let snapshot = core.snapshot(edited);
    toy::answer_alone(&mut core, &snapshot, &solve);
    let counters = core.counters();
    assert_eq!(
        (counters.body_runs(Pair::ID), counters.body_runs(Cell::ID)),
        (1, 4),
        "the plain constraint is found in the store; the one that read u runs per trial"
    );
}

/// What `Peek` read through its view. It is not hashed by value.
#[derive(Debug, PartialEq)]
struct Peeked(i64);

/// Opens one view, which supposes "u" to be 0, and reads the i64 at
/// "seen" through it; its value is what it read.
struct Peek;

impl Kind for Peek {
    type Out = Peeked;
    const ID: KindId = KindId("views.peek");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Peeked> {
        let view = cx.suppose(Substitution::of([(Ref::binding("u"), supposed(0))]));
        Produced::Outcome(
            match view.read::<i64>(&Ref::binding("seen")).await.outcome {
                Outcome::Value(seen) => Outcome::of(Peeked(*seen)),
                Outcome::Absent => Outcome::Absent,
                Outcome::Failure(failure) => Outcome::Failure(failure),
            },
        )
    }
}

/// Reads the `Peeked` at "peek" and answers what it holds.
struct Unpeek;

impl Kind for Unpeek {
    type Out = i64;
    const ID: KindId = KindId("views.unpeek");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        Produced::Outcome(match cx.read::<Peeked>(&Ref::binding("peek")).await {
            Outcome::Value(peeked) => Outcome::of(peeked.0),
            Outcome::Absent => Outcome::Absent,
            Outcome::Failure(failure) => Outcome::Failure(failure),
        })
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(out.content())
    }
}

#[test]
fn an_opaque_view_opener_result_has_a_content_covering_what_its_trials_read() {
    let builder = toy::with_scope()
        .kind::<Peek>()
        .and_then(|builder| builder.kind::<Unpeek>())
        .expect("distinct kinds");
    let mut core = builder.build();
    let peek = Declaration::new(Peek::ID)
        .bind("u", toy::sibling(&["u"]))
        .bind("seen", toy::sibling(&["seen"]));
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            ("u", toy::cell(&["u0"])),
            ("seen", toy::cell(&["k"])),
            ("peek", peek),
            (
                "reader",
                Declaration::new(Unpeek::ID).bind("peek", toy::sibling(&["peek"])),
            ),
        ]),
    );
    let inputs = |k: i64| {
        vec![
            members.clone(),
            root.clone(),
            toy::named("u0", 9_i64),
            toy::named("k", k),
        ]
    };
    let reader = member("reader");
    let answer = |core: &mut Core, inputs: Vec<(InputKey, Input)>| {
        let snapshot = core.snapshot(inputs);
        value_of::<i64>(toy::answer_alone(core, &snapshot, &reader))
    };

    assert_eq!(answer(&mut core, inputs(1)), 1);
    assert_eq!(answer(&mut core, inputs(2)), 2);
    assert_eq!(core.counters().body_runs(Unpeek::ID), 2);

    let unrelated = inputs(1)
        .into_iter()
        .chain([toy::named("unrelated", 0_i64)]);
    assert_eq!(answer(&mut core, unrelated.collect()), 1);
    assert_eq!(
        core.counters().body_runs(Unpeek::ID),
        2,
        "the reader is found in the store by the content of what the trial read"
    );
}

struct FirstChild;

impl Plan for FirstChild {
    const ID: KindId = KindId("views.probe.first_child");

    fn trials() -> Vec<Trial> {
        vec![Trial {
            substitution: Substitution::of([(Ref::binding("x"), supposed(3))]),
            reads: vec![int("r")],
        }]
    }
}

/// The probe reads, through its view, cell r over the first child of a
/// call whose second child is a shape on the gated kernel. Reading the
/// child through r lets the call go on to start the shape's section before
/// the probe's read is answered, which reading it directly would not.
#[test]
fn work_under_a_view_ends_with_the_run_that_opened_it() {
    let builder = toy::with_calls()
        .kind::<Shape>()
        .expect("toy kind ids are distinct");
    let watch = Watch::default();
    let (mut core, keeper) = toy::gated(with_probe::<FirstChild>(builder), &watch);
    let shape = toy::CallSite {
        sites: vec![2],
        each: false,
        offset: 0,
        argument: String::from("side"),
        callee: Declaration::new(Shape::ID),
    };
    let calls = toy::Calls(vec![
        toy::site(&[1], false, 0, Declaration::new(Cell::ID)),
        shape,
    ]);
    let first_child = Binding::Declarer(vec![Step::named("call"), toy::call_step(&[1], None)]);
    let [members, root] = toy::root_scope(
        "root",
        toy::members([
            ("x", toy::cell(&["x0"])),
            ("call", toy::call([toy::input("v")], toy::input("calls"))),
            ("r", toy::cell_over([first_child])),
            ("probe", probe::<FirstChild>(&["x", "r"])),
        ]),
    );
    let snapshot = core.snapshot([
        members,
        root,
        toy::named("x0", 1_i64),
        toy::named("v", 2_i64),
        toy::named("calls", calls),
    ]);
    let at = member("probe");

    let request = core.evaluate(&snapshot, toy::demand([&at]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::SectionsRunning(1));
    let probed = value_of::<Vec<Saw>>(toy::the_answer(&core.take_events(), &request, &at));
    assert_eq!(probed, vec![saw(Seen::Int(2), false)]);

    keeper.let_one_through();
    core.drain();
    assert_eq!(watch.live_boxes(), 0);
    assert_eq!(core.turn(), Turn::Idle);
    core.drain();
}
