//! The G0 gate at leaf γ: random sequences over cells, scopes, guards and
//! collections at the gate's fixed seeds pass check 1 and the order check,
//! and together reach every shape the census names, so the run is not
//! vacuous. REIFY_EVAL_CORE_SEQUENCES=N widens the run to seeds 1..=N
//! outside the gate (PRD 5.5).

use std::env::{self, VarError};
use std::ops::RangeInclusive;

use reify_eval_core::Core;

use super::generator::{Features, GATE_EVENTS, GATE_SEEDS, generate};
use super::session::{Reach, check};
use crate::toy;

fn honest() -> Core {
    toy::with_structure().build()
}

/// Widens the run to seeds 1..=N outside the gate.
const WIDEN: &str = "REIFY_EVAL_CORE_SEQUENCES";

/// The seeds to run: the gate's, unless `WIDEN` names a number of seeds.
fn seeds() -> RangeInclusive<u64> {
    match env::var(WIDEN) {
        Err(VarError::NotPresent) => GATE_SEEDS,
        Ok(text) => {
            let last = text.parse().unwrap_or_else(|_| {
                panic!("{WIDEN}={text:?} is not a number of seeds")
            });
            1..=last
        }
        Err(unreadable) => panic!("{WIDEN} cannot be read: {unreadable}"),
    }
}

#[test]
fn random_sequences_over_cells_scopes_guards_and_collections_satisfy_g0() {
    let mut reach = Reach::default();
    for seed in seeds() {
        let case = generate(seed, Features::structure(), GATE_EVENTS);
        match check(&case, &honest) {
            Ok(found) => reach.add(&found),
            Err(mismatch) => panic!("seed {seed}: {mismatch}"),
        }
    }
    assert_eq!(reach.missing(), [], "shapes the seeds never reached: {reach:?}");
}
