//! Views (contract 6): a body supposes outcomes at places and reads through
//! the view it opens, each read telling whether its answer depended on the
//! substitution, while the surrounding state is left as it was.

use std::marker::PhantomData;

use reify_eval_core::{
    Answer, Binding, Builder, Core, Cx, Declaration, Erased, Failure, Input, InputKey, Kind,
    KindId, Outcome, OutsideFailure, Produced, Ref, Snapshot, Step, Substitution,
};

use crate::toy::{self, Members};

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
