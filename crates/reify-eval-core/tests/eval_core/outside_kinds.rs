//! What the first kinds outside the crate need of its data (contract 9,
//! 5.2, 7.3; PRD D14): a host takes a path and a ref apart and puts them
//! together again, a kind declares a child at a step in a space of its
//! own, and a kind's failure carries a description its readers and the
//! host read back.

use std::collections::BTreeMap;
use std::fmt::Debug;

use reify_eval_core::{
    Answer, Binding, Core, Cx, Declaration, Description, Failure, Hash128, Input, InputKey, Kind,
    KindId, Outcome, Path, Produced, Ref, Root, Snapshot, Space, Step, StepKey, Value,
};

use crate::toy::{self, Contented, Members};

/// A space of the presence kind's own, beside the default one.
const OWN: Space = Space::Named("test.presence.own");

/// The code of a presence's failure when its condition is no bool.
const PRESENCE_UNDETERMINED: &str = "presence undetermined";

/// What reading a presence's condition gave instead of a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    Absent,
    Failure,
}

/// A presence's account of its failure: the ref it read, and what it
/// found there. The ref is relative, so it names no path (contract 5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Undetermined {
    at: Ref,
    found: Found,
}

impl Contented for Undetermined {
    fn content(&self) -> Hash128 {
        let name = self.at.binding_name();
        let steps = self.at.steps().iter().flat_map(toy::step_bytes);
        let found = match self.found {
            Found::Absent => b'a',
            Found::Failure => b'f',
        };
        let bytes: Vec<u8> = name
            .len()
            .to_le_bytes()
            .into_iter()
            .chain(name.bytes())
            .chain(steps)
            .chain([found])
            .collect();
        Hash128::of_bytes(&bytes)
    }
}

/// A presence's failure: `PRESENCE_UNDETERMINED`, described.
fn undetermined(at: Ref, found: Found) -> Failure {
    let description = Undetermined { at, found };
    let content = description.content();
    Failure::Described {
        kind: Presence::ID,
        code: PRESENCE_UNDETERMINED,
        description: Description::of(description, content),
    }
}

/// Reads the bool at "condition": true makes it declare the members it
/// reads at "members", as a scope does; false makes it absent; anything
/// else is its own failure, `PRESENCE_UNDETERMINED`, described.
struct Presence;

impl Kind for Presence {
    type Out = Members;
    const ID: KindId = KindId("test.presence");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Members> {
        let condition = Ref::binding("condition");
        Produced::Outcome(match cx.read::<bool>(&condition).await {
            Outcome::Value(holds) if *holds => cx.read::<Members>(&Ref::binding("members")).await,
            Outcome::Value(_) => Outcome::Absent,
            Outcome::Absent => Outcome::Failure(undetermined(condition, Found::Absent)),
            Outcome::Failure(_) => Outcome::Failure(undetermined(condition, Found::Failure)),
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

/// A presence on `condition`, none where it is unbound, over the
/// host-named input `members`.
fn presence(condition: Option<Binding>, members: &str) -> Declaration {
    let declaration = Declaration::new(Presence::ID).bind("members", toy::input(members));
    match condition {
        Some(condition) => declaration.bind("condition", condition),
        None => declaration,
    }
}

/// Reads the members at "subject" and answers the `Undetermined` its
/// failure describes; absent where it holds none.
struct Why;

impl Kind for Why {
    type Out = Undetermined;
    const ID: KindId = KindId("test.why");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Undetermined> {
        Produced::Outcome(match cx.read::<Members>(&Ref::binding("subject")).await {
            Outcome::Failure(Failure::Described { description, .. }) => description
                .read::<Undetermined>()
                .cloned()
                .map_or(Outcome::Absent, Outcome::of),
            _ => Outcome::Absent,
        })
    }

    fn content(out: &Undetermined) -> Option<Hash128> {
        Some(out.content())
    }
}

/// A why of the member `p` beside it.
fn why() -> Declaration {
    Declaration::new(Why::ID).bind("subject", toy::sibling(&["p"]))
}

fn core() -> Core {
    toy::with_scope()
        .kind::<Presence>()
        .and_then(|builder| builder.kind::<Why>())
        .expect("test kind ids are distinct from the toy ones")
        .build()
}

/// A presence that fails: its condition, input "seven", holds an i64.
fn failing() -> Declaration {
    presence(Some(toy::input("seven")), "p.members")
}

/// The inputs a failing presence reads, and its members: "note", a cell
/// over "seven".
fn failing_inputs() -> Vec<(InputKey, Input)> {
    vec![
        toy::named("seven", 7_i64),
        toy::named("p.members", toy::members([("note", toy::cell(&["seven"]))])),
    ]
}

/// The presence description `failure` holds.
fn described(failure: &Failure) -> &Undetermined {
    match failure {
        Failure::Described { description, .. } => description
            .read::<Undetermined>()
            .expect("a presence describes its failure as Undetermined"),
        other => panic!("expected a described failure, got {other:?}"),
    }
}

/// The value at `at` in a request for it alone, as `T`.
fn value_at<T: Value + Clone + Debug>(core: &mut Core, snapshot: &Snapshot, at: &Path) -> T {
    match toy::answer_alone(core, snapshot, at).downcast::<T>() {
        Answer::Final(Outcome::Value(value)) => (*value).clone(),
        other => panic!("expected a value at {at:?}, got {other:?}"),
    }
}

#[test]
fn a_host_can_take_a_path_apart_and_put_it_together_again() {
    let path = Path::root(Root::named("R"))
        .child(Step::named("p"))
        .child(Step::index(2))
        .child(Step::new(OWN, StepKey::Name("note".into())));

    assert_eq!(path.root_name().name(), "R");
    let parts: Vec<(Space, StepKey)> = path
        .steps()
        .iter()
        .map(|step| (step.space(), step.key().clone()))
        .collect();
    assert_eq!(
        parts,
        [
            (Space::Default, StepKey::Name("p".into())),
            (Space::Default, StepKey::Index(2)),
            (OWN, StepKey::Name("note".into())),
        ]
    );
    let rebuilt = parts.into_iter().fold(
        Path::root(Root::named(path.root_name().name())),
        |rebuilt, (space, key)| rebuilt.child(Step::new(space, key)),
    );
    assert_eq!(rebuilt, path);

    let r = Ref::binding("subject")
        .step(Step::named("p"))
        .step(Step::new(OWN, StepKey::Index(0)));
    let rebuilt = r.steps().iter().fold(
        Ref::binding(r.binding_name()),
        |rebuilt, step| rebuilt.step(Step::new(step.space(), step.key().clone())),
    );
    assert_eq!(rebuilt, r);

    assert_eq!(
        Step::new(Space::Default, StepKey::Name("p".into())),
        Step::named("p")
    );
    assert_eq!(
        Step::new(Space::Default, StepKey::Index(2)),
        Step::index(2)
    );
}

#[test]
fn a_kind_failure_carries_a_description_the_host_reads_back() {
    let mut core = core();
    let mut inputs = failing_inputs();
    inputs.push(toy::root("P", failing()));
    let snapshot = core.snapshot(inputs);

    let failure = toy::failed(toy::answer_alone(&mut core, &snapshot, &toy::at("P")));
    assert_eq!(
        failure,
        undetermined(Ref::binding("condition"), Found::Failure)
    );
    let Failure::Described {
        kind,
        code,
        description,
    } = &failure
    else {
        panic!("expected a described failure, got {failure:?}");
    };
    assert_eq!(*kind, Presence::ID);
    assert_eq!(*code, PRESENCE_UNDETERMINED);
    assert_eq!(
        description.read::<Undetermined>(),
        Some(&Undetermined {
            at: Ref::binding("condition"),
            found: Found::Failure,
        })
    );
    assert_eq!(description.read::<i64>(), None);
    assert_eq!(description.read::<Members>(), None);
}

/// The inputs of root scopes P1 and P2, each with members `p`, the
/// presence `p2` gives P2's, and `w`, a why of `p`.
fn two_places(p2: Declaration) -> Vec<(InputKey, Input)> {
    let mut inputs = failing_inputs();
    inputs.extend(toy::root_scope(
        "P1",
        toy::members([("p", failing()), ("w", why())]),
    ));
    inputs.extend(toy::root_scope("P2", toy::members([("p", p2), ("w", why())])));
    inputs
}

/// On a snapshot of `inputs`, the failure at P1/p and the value at P1/w,
/// then those at P2/p and P2/w, each path in a request of its own.
fn answer_both_places(
    core: &mut Core,
    inputs: Vec<(InputKey, Input)>,
) -> [(Failure, Undetermined); 2] {
    let snapshot = core.snapshot(inputs);
    ["P1", "P2"].map(|place| {
        let failure = toy::failed(toy::answer_alone(
            core,
            &snapshot,
            &toy::below(place, &["p"]),
        ));
        let why = value_at::<Undetermined>(core, &snapshot, &toy::below(place, &["w"]));
        (failure, why)
    })
}

/// The body runs of presence, then of why.
fn runs(core: &Core) -> (u64, u64) {
    (
        core.counters().body_runs(Presence::ID),
        core.counters().body_runs(Why::ID),
    )
}

#[test]
fn equal_failure_descriptions_at_two_places_are_one_stored_result() {
    let mut core = core();
    let [first, second] = answer_both_places(&mut core, two_places(failing()));

    let expected = Undetermined {
        at: Ref::binding("condition"),
        found: Found::Failure,
    };
    assert_eq!(
        first,
        (
            undetermined(Ref::binding("condition"), Found::Failure),
            expected
        )
    );
    assert_eq!(second, first);
    assert_eq!(runs(&core), (1, 1));
}

#[test]
fn failures_with_different_descriptions_are_not_shared() {
    let mut core = core();
    let [first, second] =
        answer_both_places(&mut core, two_places(presence(None, "p.members")));

    let condition = Ref::binding("condition");
    assert_eq!(
        first.0,
        undetermined(condition.clone(), Found::Failure)
    );
    assert_eq!(
        second,
        (
            undetermined(condition.clone(), Found::Absent),
            Undetermined {
                at: condition,
                found: Found::Absent,
            }
        )
    );
    assert_eq!(runs(&core), (2, 2));
}

#[test]
fn a_path_below_a_failed_node_answers_that_failure_with_its_description() {
    let mut core = core();
    let mut inputs = failing_inputs();
    inputs.extend(toy::root_scope("R", toy::members([("p", failing())])));
    let snapshot = core.snapshot(inputs);

    let by_default = toy::below("R", &["p", "note"]);
    let by_own = toy::below("R", &["p"]).child(Step::new(OWN, StepKey::Name("note".into())));
    let answers = toy::answers_to(&mut core, &snapshot, &[&by_default, &by_own]);
    for answer in answers {
        let failure = toy::failed(answer);
        assert_eq!(
            failure,
            undetermined(Ref::binding("condition"), Found::Failure)
        );
        assert_eq!(
            described(&failure),
            &Undetermined {
                at: Ref::binding("condition"),
                found: Found::Failure,
            }
        );
    }
}

#[test]
fn a_reader_of_a_failed_node_can_reach_its_description() {
    let mut core = core();
    let mut inputs = failing_inputs();
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("p", failing()),
            ("r", toy::cell_over([toy::sibling(&["p"])])),
        ]),
    ));
    let snapshot = core.snapshot(inputs);

    let failure = toy::failed(toy::answer_alone(
        &mut core,
        &snapshot,
        &toy::below("R", &["r"]),
    ));
    let Failure::BlockedBy { at } = failure else {
        panic!("expected a reader blocked by what it read, got {failure:?}");
    };
    assert_eq!(at, Ref::binding("0"));

    // r's binding "0" names its declarer's member p, so the place `at`
    // names is R/p, then `at`'s steps.
    assert_eq!(at.binding_name(), "0");
    let cause = at
        .steps()
        .iter()
        .cloned()
        .fold(toy::below("R", &["p"]), |path, step| path.child(step));
    let failure = toy::failed(toy::answer_alone(&mut core, &snapshot, &cause));
    assert_eq!(
        described(&failure),
        &Undetermined {
            at: Ref::binding("condition"),
            found: Found::Failure,
        }
    );
}

#[test]
fn steps_with_equal_keys_in_different_spaces_are_different_children() {
    let own_note = Step::new(OWN, StepKey::Name("note".into()));
    let mut core = core();
    let mut inputs = vec![
        toy::named("yes", true),
        toy::named("one", 1_i64),
        toy::named("two", 2_i64),
        toy::named(
            "p.members",
            Members(BTreeMap::from([
                (Step::named("note"), toy::cell(&["one"])),
                (own_note.clone(), toy::cell(&["two"])),
            ])),
        ),
    ];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("p", presence(Some(toy::input("yes")), "p.members")),
            (
                "by_default",
                toy::cell_over([Binding::Declarer(vec![
                    Step::named("p"),
                    Step::named("note"),
                ])]),
            ),
            (
                "by_own",
                toy::cell_over([Binding::Declarer(vec![Step::named("p"), own_note.clone()])]),
            ),
        ]),
    ));
    let snapshot = core.snapshot(inputs);

    let cases = [
        (toy::below("R", &["p", "note"]), 1),
        (toy::below("R", &["p"]).child(own_note), 2),
        (toy::below("R", &["by_default"]), 1),
        (toy::below("R", &["by_own"]), 2),
    ];
    for (at, expected) in cases {
        assert_eq!(value_at::<i64>(&mut core, &snapshot, &at), expected, "{at:?}");
    }
}
