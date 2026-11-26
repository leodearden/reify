//! The two-places test of contract 5.4, over one table with an entry for
//! every toy kind: a result must not contain where it is.

use std::collections::BTreeSet;
use std::thread::ThreadId;

use reify_eval_core::{
    Answer, Builder, Cx, Declaration, Erased, Held, Input, InputKey, Kind, KindId, Outcome,
    Produced, Ref, Value,
};

use crate::toy::{self, BoxHandle, Cell, Collection, Guard, Measure, Scope, Shape, ToyKernel};

/// One kind's case: the subject's declaration, the members beside it it
/// reads, the snapshot inputs it needs, and how its kind compares two of
/// its answers.
pub struct Entry {
    pub kind: KindId,
    pub subject: Declaration,
    pub siblings: Vec<(&'static str, Declaration)>,
    pub inputs: Vec<(InputKey, Input)>,
    pub same: fn(Answer<Erased>, Answer<Erased>) -> bool,
}

/// Two answers compared by `T`'s own equality on its values.
fn same_as<T: Value + PartialEq>(first: Answer<Erased>, second: Answer<Erased>) -> bool {
    first.downcast::<T>() == second.downcast::<T>()
}

/// The table: one entry per toy kind in `toy::TOY_KINDS`.
fn table() -> Vec<Entry> {
    vec![
        Entry {
            kind: Cell::ID,
            subject: toy::cell_over([toy::sibling(&["x"]), toy::input("v")]),
            siblings: vec![("x", toy::cell(&["v"]))],
            inputs: vec![toy::named("v", 3_i64)],
            same: same_as::<i64>,
        },
        Entry {
            kind: Scope::ID,
            subject: toy::scope("inner"),
            siblings: Vec::new(),
            inputs: vec![toy::named(
                "inner",
                toy::members([("m", toy::cell(&["v"]))]),
            )],
            same: same_as::<toy::Members>,
        },
        Entry {
            kind: Guard::ID,
            subject: toy::guard(toy::input("holds"), "inner"),
            siblings: Vec::new(),
            inputs: vec![
                toy::named("holds", true),
                toy::named("inner", toy::members([("m", toy::cell(&["v"]))])),
            ],
            same: same_as::<toy::Members>,
        },
        Entry {
            kind: Collection::ID,
            subject: toy::collection(toy::input("n"), toy::input("element")),
            siblings: Vec::new(),
            inputs: vec![
                toy::named("n", 3_i64),
                toy::named("element", toy::cell(&["v"])),
            ],
            same: same_as::<toy::Elements>,
        },
        shape_entry(),
        Entry {
            kind: Measure::ID,
            subject: toy::measure(toy::sibling(&["s"])),
            siblings: vec![("s", toy::shape("side"))],
            inputs: vec![toy::named("side", 2_i64)],
            same: same_as::<i64>,
        },
    ]
}

fn shape_entry() -> Entry {
    Entry {
        kind: Shape::ID,
        subject: toy::shape("side"),
        siblings: Vec::new(),
        inputs: vec![toy::named("side", 2_i64)],
        same: same_as::<toy::BoxShape>,
    }
}

#[test]
fn every_toy_kind_has_a_two_places_entry() {
    let entries = table();
    let covered: BTreeSet<KindId> = entries.iter().map(|entry| entry.kind).collect();
    let listed: BTreeSet<KindId> = toy::TOY_KINDS.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(covered, listed);
    assert_eq!(entries.len(), listed.len(), "one entry per kind");
}

#[test]
fn the_two_places_test_holds_for_every_toy_kind() {
    for entry in table() {
        assert_eq!(
            check(&entry, toy::with_every_kind),
            Ok(()),
            "{:?}",
            entry.kind
        );
    }
}

/// A box that remembers the handle number the toy kernel minted for it,
/// which its equality compares: it contains where it was made.
#[derive(Debug)]
pub struct LeakyBox {
    _handle: Held<ToyKernel, BoxHandle>,
    minted: u32,
    volume: i64,
    _built_on: ThreadId,
}

impl PartialEq for LeakyBox {
    fn eq(&self, other: &Self) -> bool {
        (self.minted, self.volume) == (other.minted, other.volume)
    }
}

/// The toy shape, with the minted handle number copied into its result.
struct LeakyShape;

impl Kind for LeakyShape {
    type Out = LeakyBox;
    const ID: KindId = KindId("test.leaky_shape");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<LeakyBox> {
        let side = match cx.read::<i64>(&Ref::binding("side")).await {
            Outcome::Value(side) => *side,
            Outcome::Failure(failure) => return Produced::Outcome(Outcome::Failure(failure)),
            Outcome::Absent => return Produced::Outcome(Outcome::Absent),
        };
        let (handle, built_on) = cx
            .exclusive(move |kernel: &mut ToyKernel, section| kernel.make_box(side, section))
            .await;
        let minted = handle.0;
        Produced::Outcome(Outcome::of(LeakyBox {
            _handle: handle,
            minted,
            volume: side * side * side,
            _built_on: built_on,
        }))
    }
}

fn with_leaky_shape() -> Builder {
    toy::with_every_kind()
        .kind::<LeakyShape>()
        .expect("distinct kinds")
}

#[test]
fn two_places_check_catches_resource_minted_id_in_result() {
    let leaky = Entry {
        kind: LeakyShape::ID,
        subject: Declaration::new(LeakyShape::ID).bind("side", toy::input("side")),
        siblings: Vec::new(),
        inputs: vec![toy::named("side", 2_i64)],
        same: same_as::<LeakyBox>,
    };
    assert_eq!(
        check(&leaky, with_leaky_shape),
        Err(Mismatch::Differs {
            kind: LeakyShape::ID
        })
    );
    assert_eq!(check(&shape_entry(), with_leaky_shape), Ok(()));
}
