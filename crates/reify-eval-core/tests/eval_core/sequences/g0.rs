//! The G0 gate at leaf γ: random sequences over cells, scopes, guards and
//! collections at the gate's fixed seeds pass check 1 and the order check,
//! and together reach every shape the census names, so the run is not
//! vacuous. REIFY_EVAL_CORE_SEQUENCES=N widens the run to seeds 1..=N
//! outside the gate (PRD 5.5).

use std::env::{self, VarError};
use std::ops::RangeInclusive;

use super::case::Case;
use super::generator::{GATE_SEEDS, gate_case};
use super::print::{Expectation, print_test};
use super::reduce::{candidates, size};
use super::session::{Mismatch, Reach, check, toy_core};
use super::shrink::shrink;

/// Widens the run to seeds 1..=N outside the gate.
const WIDEN: &str = "REIFY_EVAL_CORE_SEQUENCES";

/// The seeds to run: the gate's, unless `WIDEN` names a number of seeds.
fn seeds() -> RangeInclusive<u64> {
    match env::var(WIDEN) {
        Err(VarError::NotPresent) => GATE_SEEDS,
        Ok(text) => {
            let last = text
                .parse()
                .unwrap_or_else(|_| panic!("{WIDEN}={text:?} is not a number of seeds"));
            1..=last
        }
        Err(unreadable) => panic!("{WIDEN} cannot be read: {unreadable}"),
    }
}

/// `case`, failing with `mismatch`, shrunk while the honest core fails it
/// the same way and printed as a named regression to paste in and fix.
fn regression(seed: u64, case: Case, mismatch: &Mismatch) -> String {
    let kind = mismatch.kind();
    let fails_alike = |case: &Case| check(case, &toy_core).is_err_and(|found| found.kind() == kind);
    let shrunk = shrink(case, candidates, size, fails_alike);
    let name = format!("g0_case_from_seed_{seed}");
    let printed = print_test(&shrunk, seed, &name, Expectation::PassesWithToyKinds);
    format!("shrunk and printed as a regression to paste in and fix:\n{printed}")
}

#[test]
fn random_sequences_over_cells_scopes_guards_and_collections_satisfy_g0() {
    let mut reach = Reach::default();
    for seed in seeds() {
        let case = gate_case(seed);
        match check(&case, &toy_core) {
            Ok(found) => reach.add(&found),
            Err(mismatch) => panic!(
                "seed {seed}: {mismatch}\n{}",
                regression(seed, case, &mismatch)
            ),
        }
    }
    assert_eq!(
        reach.missing(),
        [],
        "shapes the seeds never reached: {reach:?}"
    );
}
