//! The signal of leaf γ: a wrong rule in the toy cell, registered in the
//! core under test in the cell's place, is caught by check 1, shrunk until
//! the shrinker can remove nothing more, and printed as a test whose text
//! is the committed printed_faulty_cell.rs.

use reify_eval_core::{Builder, Core, Cx, Hash128, Kind, KindId, Outcome, Produced};

use super::case::Case;
use super::generator::{Features, GATE_EVENTS, GATE_SEEDS, generate};
use super::print::{Expectation, print_test};
use super::reduce::{candidates, size};
use super::session::{MismatchKind, check, toy_core};
use super::shrink::shrink;
use crate::toy;

/// The toy cell with a wrong rule: a value whose bytes hash to a multiple
/// of ten is one too many.
struct FaultyCell;

impl Kind for FaultyCell {
    type Out = i64;
    const ID: KindId = toy::Cell::ID;
    const VERSION: u32 = toy::Cell::VERSION;

    async fn run(cx: &Cx) -> Produced<i64> {
        match <toy::Cell as Kind>::run(cx).await {
            Produced::Outcome(Outcome::Value(value)) if hashes_to_a_tenth(*value) => {
                Produced::Outcome(Outcome::of(*value + 1))
            }
            honest => honest,
        }
    }
}

fn hashes_to_a_tenth(value: i64) -> bool {
    Hash128::of_bytes(&value.to_le_bytes())
        .as_u128()
        .is_multiple_of(10)
}

/// A core with the faulty cell in the toy cell's place: the only builder
/// that registers it, so no other test sees it.
fn faulty_core() -> Core {
    Builder::new()
        .kind::<FaultyCell>()
        .and_then(|builder| builder.kind::<toy::Scope>())
        .and_then(|builder| builder.kind::<toy::Guard>())
        .and_then(|builder| builder.kind::<toy::Collection>())
        .expect("the kind ids are distinct")
        .build()
}

/// Whether check 1 reports the faulty cell on `case` while the honest
/// kinds pass it.
pub fn caught(case: &Case) -> bool {
    let faulty = check(case, &faulty_core);
    let reported = faulty.is_err_and(|mismatch| mismatch.kind() == MismatchKind::Reference);
    reported && check(case, &toy_core).is_ok()
}

#[test]
fn a_wrong_rule_in_the_cell_is_caught_shrunk_and_printed() {
    let (seed, case) = GATE_SEEDS
        .map(|seed| (seed, generate(seed, Features::structure(), GATE_EVENTS)))
        .find(|(_, case)| caught(case))
        .expect("a gate case catches the faulty cell");
    let shrunk = shrink(case, candidates, size, caught);
    let still: Vec<_> = candidates(&shrunk).into_iter().filter(caught).collect();
    assert!(still.is_empty(), "{shrunk:?} shrinks further to {still:?}");
    let name = format!("faulty_cell_case_from_seed_{seed}");
    let printed = print_test(&shrunk, seed, &name, Expectation::CaughtWithFaultyCell);
    assert_eq!(
        printed,
        include_str!("printed_faulty_cell.rs"),
        "re-bless printed_faulty_cell.rs with exactly this text:\n{printed}"
    );
}
