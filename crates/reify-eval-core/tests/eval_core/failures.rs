//! Failures that follow from the inputs (contract 9; PRD 5.4): a body's own
//! failure, stated in its kind's terms, and a reader blocked by what it
//! read. The toy cell divides, and division by zero is its own failure.

use reify_eval_core::{Answer, Declaration, Failure, Kind, Outcome, Ref};

use crate::toy::{self, Cell};

fn quotient_of(sum: i64, divisor: i64) -> Answer<i64> {
    let mut core = toy::with_cell().build();
    let snapshot = core.snapshot([
        toy::named("a", sum),
        toy::named("d", divisor),
        toy::root("q", toy::divided(toy::cell(&["a"]), toy::input("d"))),
    ]);
    toy::answer_alone(&mut core, &snapshot, &toy::at("q")).downcast()
}

#[test]
fn a_cell_divides_its_sum_by_its_divisor() {
    assert_eq!(quotient_of(7, 2), Answer::Final(Outcome::of(3)));
    assert_eq!(quotient_of(-7, 2), Answer::Final(Outcome::of(-3)));
}

#[test]
fn a_cell_dividing_by_zero_answers_its_own_failure() {
    let by_zero = Failure::OfKind {
        kind: Cell::ID,
        code: toy::DIVISION_BY_ZERO,
    };
    assert_eq!(quotient_of(7, 0), Answer::Final(Outcome::Failure(by_zero)));
}

/// The answers at members `names` of a root scope whose member "z" divides
/// by zero and whose other members are `members`.
fn beside_a_division_by_zero(members: Vec<(&str, Declaration)>, names: &[&str]) -> Vec<Failure> {
    let mut core = toy::with_scope().build();
    let z = toy::divided(toy::cell(&["a"]), toy::input("zero"));
    let mut inputs = vec![toy::named("a", 7_i64), toy::named("zero", 0_i64)];
    let all = std::iter::once(("z", z)).chain(members);
    inputs.extend(toy::root_scope("R", toy::members(all)));
    let snapshot = core.snapshot(inputs);
    names
        .iter()
        .map(|name| {
            toy::failed(toy::answer_alone(
                &mut core,
                &snapshot,
                &toy::below("R", &[name]),
            ))
        })
        .collect()
}

fn blocked_at(binding: &str) -> Failure {
    Failure::BlockedBy {
        at: Ref::binding(binding),
    }
}

#[test]
fn a_cell_that_reads_a_failed_node_answers_blocked_by_that_ref() {
    let failures = beside_a_division_by_zero(
        vec![
            ("r", toy::cell_over([toy::sibling(&["z"])])),
            ("s", toy::divided(toy::cell(&["a"]), toy::sibling(&["z"]))),
        ],
        &["r", "s"],
    );
    assert_eq!(failures, [blocked_at("0"), blocked_at("divisor")]);
}

#[test]
fn a_cell_blocked_by_a_blocked_cell_names_its_own_ref() {
    let failures = beside_a_division_by_zero(
        vec![
            ("r", toy::cell_over([toy::input("a"), toy::sibling(&["z"])])),
            ("t", toy::cell_over([toy::sibling(&["r"])])),
        ],
        &["r", "t"],
    );
    assert_eq!(failures, [blocked_at("1"), blocked_at("0")]);
}
