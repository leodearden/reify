//! What a body knows of its declaration: only what its reads return; and
//! what a binding passed along from a declarer names (contract 2.1, 2.5).

use std::fmt::Debug;
use std::sync::Arc;

use reify_eval_core::{
    Answer, Binding, Budget, Core, Cx, Declaration, Event, Input, InputKey, Kind, KindId, Outcome,
    Path, Produced, Ref, Step, Turn, Value,
};

use crate::toy::{self, Cell};

/// The i64s at "arg0", "arg1", … until one reads absent.
struct List;

impl Kind for List {
    type Out = Vec<i64>;
    const ID: KindId = KindId("toy.list");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Vec<i64>> {
        let mut list = Vec::new();
        for index in 0_u32.. {
            match cx.read::<i64>(&Ref::binding(format!("arg{index}"))).await {
                Outcome::Value(value) => list.push(*value),
                Outcome::Absent => break,
                Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            }
        }
        Produced::Outcome(Outcome::of(list))
    }
}

/// The i64 at "arg", or where that reads absent, the one at "default".
struct Fallback;

impl Kind for Fallback {
    type Out = i64;
    const ID: KindId = KindId("toy.fallback");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        match cx.read::<i64>(&Ref::binding("arg")).await {
            Outcome::Absent => Produced::Outcome(cx.read::<i64>(&Ref::binding("default")).await),
            read => Produced::Outcome(read),
        }
    }
}

/// The one value the core told for `at` since events were last taken.
fn value_at<T: Value + Debug>(core: &mut Core, at: &Path) -> Arc<T> {
    let events = core.take_events();
    let mut values = Vec::new();
    for event in &events {
        if let Event::Answered { path, answer, .. } = event
            && path == at
        {
            values.push(answer.clone().downcast::<T>());
        }
    }
    match values.as_slice() {
        [Answer::Final(Outcome::Value(value))] => Arc::clone(value),
        other => panic!("expected one value at {at:?}, got {other:?}"),
    }
}

#[test]
fn a_body_learns_its_declaration_only_through_recorded_reads() {
    let list = |args: &[&str]| {
        args.iter()
            .enumerate()
            .fold(Declaration::new(List::ID), |list, (index, arg)| {
                list.bind(format!("arg{index}"), toy::sibling(&[arg]))
            })
    };
    let members = toy::members([
        ("a", toy::cell(&["1"])),
        ("b", toy::cell(&["2"])),
        ("c", toy::cell(&["3"])),
        ("x", toy::cell(&["10"])),
        ("y", toy::cell(&["20"])),
        ("P1", list(&["a", "b"])),
        ("P2", list(&["a", "b", "c"])),
        (
            "A",
            Declaration::new(Fallback::ID).bind("arg", toy::sibling(&["x"])),
        ),
        (
            "B",
            Declaration::new(Fallback::ID).bind("default", toy::sibling(&["y"])),
        ),
    ]);
    let order = ["P1", "P2", "A", "B"];

    for order in [order.to_vec(), order.iter().rev().copied().collect()] {
        let mut core = toy::with_scope()
            .kind::<List>()
            .and_then(|builder| builder.kind::<Fallback>())
            .expect("distinct kinds")
            .build();
        let mut inputs: Vec<_> = [1_i64, 2, 3, 10, 20]
            .map(|value| toy::named(&value.to_string(), value))
            .into();
        inputs.extend(toy::root_scope("R", members.clone()));
        let snapshot = core.snapshot(inputs);

        for name in &order {
            let at = toy::below("R", &[name]);
            let _request = core.evaluate(&snapshot, toy::demand([&at]), Budget::unlimited());
            assert_eq!(core.turn(), Turn::Idle);
            match *name {
                "P1" => assert_eq!(*value_at::<Vec<i64>>(&mut core, &at), [1, 2]),
                "P2" => assert_eq!(*value_at::<Vec<i64>>(&mut core, &at), [1, 2, 3]),
                "A" => assert_eq!(*value_at::<i64>(&mut core, &at), 10),
                _ => assert_eq!(*value_at::<i64>(&mut core, &at), 20),
            }
        }
        assert_eq!(core.counters().body_runs(List::ID), 2, "order {order:?}");
        assert_eq!(
            core.counters().body_runs(Fallback::ID),
            2,
            "order {order:?}"
        );
    }
}

/// A binding to what the declarer's binding `name` names.
fn passed(name: &str) -> Binding {
    Binding::Passed(Ref::binding(name))
}

/// The inputs of root scope R with members `x`, a cell over the input
/// five, and `holder`, a scope whose binding "src" is `src` and whose
/// members are `members`.
fn with_holder(src: Binding, members: toy::Members) -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        toy::named("five", 5_i64),
        toy::named("holder.members", members),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("x", toy::cell(&["five"])),
            ("holder", toy::scope("holder.members").bind("src", src)),
        ]),
    ));
    inputs
}

/// The answer at `at` to a fresh request for it alone on a snapshot of
/// `inputs`, on `core`.
fn i64_at(core: &mut Core, inputs: Vec<(InputKey, Input)>, at: &Path) -> Answer<i64> {
    let snapshot = core.snapshot(inputs);
    toy::answer_alone(core, &snapshot, at).downcast()
}

#[test]
fn a_child_bound_to_its_declarer_s_binding_reads_the_place_that_binding_names_and_the_declarer_reads_nothing()
 {
    let mut core = toy::with_scope().build();
    let members = toy::members([("a", toy::cell_over([passed("src")]))]);
    let snapshot = core.snapshot(with_holder(toy::sibling(&["x"]), members));
    let (holder, a) = (
        toy::below("R", &["holder"]),
        toy::below("R", &["holder", "a"]),
    );

    let first = core.evaluate(&snapshot, toy::demand([&holder]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &first, &holder);
    assert!(
        matches!(answer, Answer::Final(Outcome::Value(_))),
        "{answer:?}"
    );
    assert_eq!(
        core.counters().body_runs(Cell::ID),
        0,
        "the place the holder binds is not read by binding it"
    );

    let second = core.evaluate(&snapshot, toy::demand([&a]), Budget::unlimited());
    assert_eq!(core.turn(), Turn::Idle);
    let answer = toy::the_answer(&core.take_events(), &second, &a);
    assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(5)));
    assert_eq!(core.counters().body_runs(Cell::ID), 2);
}

#[test]
fn a_binding_passed_along_two_declarers_reads_the_first_place() {
    let mut core = toy::with_scope().build();
    let holder2 = toy::scope("holder2.members").bind("src2", passed("src"));
    let mut inputs = with_holder(toy::sibling(&["x"]), toy::members([("holder2", holder2)]));
    inputs.push(toy::named(
        "holder2.members",
        toy::members([("b", toy::cell_over([passed("src2")]))]),
    ));
    let b = toy::below("R", &["holder", "holder2", "b"]);

    assert_eq!(i64_at(&mut core, inputs, &b), Answer::Final(Outcome::of(5)));
}

#[test]
fn a_passed_binding_with_steps_descends_from_the_place_it_names() {
    let mut core = toy::with_scope().build();
    let down = Binding::Passed(Ref::binding("src").step(Step::named("x")));
    let members = toy::members([("a", toy::cell_over([down]))]);
    let inputs = with_holder(toy::sibling(&[]), members);
    let a = toy::below("R", &["holder", "a"]);

    assert_eq!(i64_at(&mut core, inputs, &a), Answer::Final(Outcome::of(5)));
}

#[test]
fn a_passed_binding_that_names_the_declarer_s_input_reads_that_input() {
    let mut core = toy::with_scope().build();
    let members = toy::members([("a", toy::cell_over([passed("src")]))]);
    let inputs = with_holder(toy::input("five"), members);
    let a = toy::below("R", &["holder", "a"]);

    assert_eq!(i64_at(&mut core, inputs, &a), Answer::Final(Outcome::of(5)));
}

#[test]
fn a_place_binding_on_a_root_names_nothing_and_reads_absent() {
    for binding in [toy::sibling(&["x"]), passed("src")] {
        let mut core = toy::with_scope().build();
        let inputs = vec![
            toy::named("five", 5_i64),
            toy::root(
                "lone",
                toy::cell_over([binding.clone()]).bind("src", toy::input("five")),
            ),
        ];

        let answer = i64_at(&mut core, inputs, &toy::at("lone"));
        assert_eq!(answer, Answer::Final(Outcome::of(0)), "{binding:?}");
    }
}

#[test]
fn a_ref_with_steps_below_a_snapshot_input_names_nothing_and_reads_absent() {
    let mut core = toy::with_cell().build();
    let inputs = vec![
        toy::named("zero", 0_i64),
        toy::named("five", 5_i64),
        toy::root("lone", toy::picker(toy::input("zero"), toy::input("five"))),
    ];

    let answer = i64_at(&mut core, inputs, &toy::at("lone"));
    assert_eq!(answer, Answer::Final(Outcome::Absent));
}

/// A binding to the constant `value`, held in the declaration itself.
fn constant(value: i64) -> Binding {
    Binding::Constant(toy::input_of(value))
}

#[test]
fn a_read_through_a_constant_binding_answers_its_value() {
    let mut core = toy::with_cell().build();
    let inputs = vec![toy::root("lone", toy::cell_over([constant(5)]))];

    let answer = i64_at(&mut core, inputs, &toy::at("lone"));
    assert_eq!(answer, Answer::Final(Outcome::of(5)));
}

#[test]
fn a_ref_with_steps_below_a_constant_reads_absent() {
    let mut core = toy::with_cell().build();
    let inputs = vec![
        toy::named("zero", 0_i64),
        toy::root("lone", toy::picker(toy::input("zero"), constant(5))),
    ];

    let answer = i64_at(&mut core, inputs, &toy::at("lone"));
    assert_eq!(answer, Answer::Final(Outcome::Absent));
}

/// The inputs of root scopes R1 and R2, whose binding "y" is the constant
/// each is given, over one members input: a cell `c` over what the
/// declarer's "y" names.
fn scopes_with_constants(r1: i64, r2: i64) -> Vec<(InputKey, Input)> {
    let members = toy::members([("c", toy::cell_over([passed("y")]))]);
    let scope = |y| toy::scope("shared.members").bind("y", constant(y));
    vec![
        toy::named("shared.members", members),
        toy::root("R1", scope(r1)),
        toy::root("R2", scope(r2)),
    ]
}

#[test]
fn children_given_an_equal_constant_by_two_parents_share_one_stored_result() {
    let mut core = toy::with_scope().build();
    let snapshot = core.snapshot(scopes_with_constants(5, 5));

    for root in ["R1", "R2"] {
        let c = toy::below(root, &["c"]);
        let answer = toy::answer_alone(&mut core, &snapshot, &c).downcast::<i64>();
        assert_eq!(answer, Answer::Final(Outcome::of(5)), "{root}");
    }
    assert_eq!(core.counters().body_runs(Cell::ID), 1);
}

#[test]
fn changing_a_constant_runs_the_body_of_the_node_that_reads_it_and_no_other() {
    let mut core = toy::with_scope().build();
    let c = toy::below("R1", &["c"]);
    assert_eq!(
        i64_at(&mut core, scopes_with_constants(5, 5), &c),
        Answer::Final(Outcome::of(5))
    );
    let (cells, scopes) = (
        core.counters().body_runs(Cell::ID),
        core.counters().body_runs(toy::Scope::ID),
    );

    assert_eq!(
        i64_at(&mut core, scopes_with_constants(6, 5), &c),
        Answer::Final(Outcome::of(6))
    );
    assert_eq!(core.counters().body_runs(Cell::ID), cells + 1);
    assert_eq!(core.counters().body_runs(toy::Scope::ID), scopes);
}
