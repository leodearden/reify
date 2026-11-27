//! The catalogue's T model (docs/prds/evaluation-core.counter-examples.md,
//! leaf η), shared by the limit and budget tests. T is a toy guard on its
//! binding "condition" (the model's live), over the one members input
//! `MEMBERS`, with a binding "d". Its members are m = d - 1, l = (d > 0), a
//! reader that folds the readers below it, and the T or Ts one level down,
//! each with d = the place m and condition = the place l. Every kind here
//! hashes its value by content, so every T whose condition reads true has
//! one key.

use reify_eval_core::{Binding, Builder, Cx, Declaration, Failure, Hash128, Input, InputKey};
use reify_eval_core::{Kind, KindId, Outcome, Produced, Ref};

use crate::toy::{self, Contented, Guard, Members};

/// The host-named input every T's members are read from.
pub const MEMBERS: &str = "t.members";

/// Reads the i64 at "0" and answers whether it is above zero; a failed
/// read makes it blocked by "0".
pub struct Positive;

impl Kind for Positive {
    type Out = bool;
    const ID: KindId = KindId("t.positive");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<bool> {
        let at = Ref::binding("0");
        Produced::Outcome(match cx.read::<i64>(&at).await {
            Outcome::Value(d) => Outcome::of(*d > 0),
            Outcome::Absent => Outcome::Absent,
            Outcome::Failure(_) => Outcome::Failure(Failure::BlockedBy { at }),
        })
    }

    fn content(out: &bool) -> Option<Hash128> {
        Some(out.content())
    }
}

/// Reads the bool at "l" first: false answers 1; true answers the sum of
/// the i64s at "0", "1", … read one at a time until one reads absent. A
/// failed read makes it blocked by the ref it read.
pub struct Fold;

async fn fold(cx: &Cx) -> Outcome<i64> {
    let live = Ref::binding("l");
    match cx.read::<bool>(&live).await {
        Outcome::Value(holds) if *holds => {}
        Outcome::Value(_) => return Outcome::of(1),
        Outcome::Absent => return Outcome::Absent,
        Outcome::Failure(_) => return Outcome::Failure(Failure::BlockedBy { at: live }),
    }
    let mut sum = 0;
    let mut index = 0_u32;
    loop {
        let at = Ref::binding(index.to_string());
        match cx.read::<i64>(&at).await {
            Outcome::Value(value) => sum += *value,
            Outcome::Absent => return Outcome::of(sum),
            Outcome::Failure(_) => return Outcome::Failure(Failure::BlockedBy { at }),
        }
        index += 1;
    }
}

impl Kind for Fold {
    type Out = i64;
    const ID: KindId = KindId("t.fold");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        Produced::Outcome(fold(cx).await)
    }

    fn content(out: &i64) -> Option<Hash128> {
        Some(out.content())
    }
}

/// A T whose d is what `d` names and whose condition is what `live` names.
pub fn t(d: Binding, live: Binding) -> Declaration {
    Declaration::new(Guard::ID)
        .bind("condition", live)
        .bind("members", toy::input(MEMBERS))
        .bind("d", d)
}

/// The members every T declares besides its reader and the Ts below it:
/// m = d - 1 and l = (d > 0).
fn arithmetic() -> [(&'static str, Declaration); 2] {
    let d = || Binding::Passed(Ref::binding("d"));
    [
        ("m", toy::cell_over([d(), toy::input("minus_one")])),
        ("l", Declaration::new(Positive::ID).bind("0", d())),
    ]
}

/// A fold whose "l" is the sibling l, over `terms` bound at "0", "1", ….
fn folding(terms: impl IntoIterator<Item = Binding>) -> Declaration {
    terms.into_iter().enumerate().fold(
        Declaration::new(Fold::ID).bind("l", toy::sibling(&["l"])),
        |declaration, (index, term)| declaration.bind(index.to_string(), term),
    )
}

/// The T one level down: d is the sibling m, its condition the sibling l.
fn next_t() -> Declaration {
    t(toy::sibling(&["m"]), toy::sibling(&["l"]))
}

/// The inputs every T reads besides its own bindings: `members` under
/// `MEMBERS`, and the -1 that m adds.
fn shared(members: Members) -> [(InputKey, Input); 2] {
    [
        toy::named(MEMBERS, members),
        toy::named("minus_one", -1_i64),
    ]
}

/// The chain model: each T declares m, l, `leaf` = (if l then next/leaf
/// else 1) and one T, `next`.
pub fn chain() -> [(InputKey, Input); 2] {
    let below = [
        ("leaf", folding([toy::sibling(&["next", "leaf"])])),
        ("next", next_t()),
    ];
    shared(toy::members(arithmetic().into_iter().chain(below)))
}

/// The tree model: each T declares m, l, `sum` = (if l then left/sum +
/// right/sum else 1) and two Ts, `left` and `right`.
pub fn tree() -> [(InputKey, Input); 2] {
    let sum = folding([
        toy::sibling(&["left", "sum"]),
        toy::sibling(&["right", "sum"]),
    ]);
    let below = [("sum", sum), ("left", next_t()), ("right", next_t())];
    shared(toy::members(arithmetic().into_iter().chain(below)))
}

/// A builder with the kinds the T model uses: cells, scopes, guards,
/// `Positive` and `Fold`.
pub fn with_t_kinds() -> Builder {
    toy::with_scope()
        .kind::<Guard>()
        .and_then(|builder| builder.kind::<Positive>())
        .and_then(|builder| builder.kind::<Fold>())
        .expect("the T model's kind ids are distinct")
}
