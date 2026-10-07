//! Task #5469 (PRD2 γ): the user-observable leaf for default-ON discrete
//! solving. Evaluating the PRD's let-indirected hexagon balance must yield
//! exact balancing Bools, with no misleading residual error.
//!
//! Registered from `harness_engine.rs` with an explicit `#[path]` — see the
//! anti-re-accretion rationale there. The fixture is read FROM DISK, as in
//! `let_tracing_transitive_e2e`, so the test tracks the PRD's literal artifact
//! instead of pinning a paraphrase.

use reify_core::ValueCellId;
use reify_eval::EvalResult;
use reify_ir::Value;

use crate::underdetermined_support::{eval_through_production_registry, workspace_root};

/// Repo-relative path of the PRD2 fixture.
const BALANCE_FIXTURE_PATH: &str = "docs/prds/v0_6/fixtures/discrete_balance_lets.ri";

const WHAT: &str = "discrete_balance_lets";

fn balance_fixture_source() -> String {
    let path = workspace_root().join(BALANCE_FIXTURE_PATH);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "the PRD2 fixture must be readable at {} ({e}). It is read from disk \
             on purpose; a docs-only move of it lands without running this suite. \
             Restore the path or update `BALANCE_FIXTURE_PATH` — do not inline a copy.",
            path.display()
        )
    })
}

fn n2(member: &str) -> ValueCellId {
    ValueCellId::new("N2", member)
}

/// The six `N2.up<i>`, each required to be an exact `Value::Bool`.
fn resolved_ups(result: &EvalResult) -> [bool; 6] {
    std::array::from_fn(|k| {
        let id = n2(&format!("up{}", k + 1));
        match result.values.get(&id) {
            Some(Value::Bool(up)) => *up,
            other => panic!("{id:?} in the {WHAT} eval must be an exact Bool; got {other:?}"),
        }
    })
}

/// The six let forces `N2.f<i>`, each required to have resolved to a number.
fn resolved_forces(result: &EvalResult) -> [f64; 6] {
    std::array::from_fn(|k| {
        let id = n2(&format!("f{}", k + 1));
        let value = result.values.get(&id);
        value.and_then(Value::as_f64).unwrap_or_else(|| {
            panic!("{id:?} in the {WHAT} eval must resolve to a number; got {value:?}")
        })
    })
}

#[test]
fn discrete_balance_lets_fixture_resolves_to_exact_balancing_bools() {
    let result = eval_through_production_registry(&balance_fixture_source(), WHAT);
    let ups = resolved_ups(&result);
    let forces = resolved_forces(&result);

    for (k, (up, force)) in ups.iter().zip(forces).enumerate() {
        let expected = if *up { 1.0 } else { -1.0 };
        assert_eq!(force, expected, "N2.f{} must follow N2.up{}", k + 1, k + 1);
    }
    let [f1, f2, f3, f4, f5, f6] = forces;
    assert_eq!(f1 + f2 + f3 + f4 + f5 + f6, 0.0, "force sum for {ups:?}");
    assert_eq!(
        0.8660254 * (f2 + f3 - f5 - f6),
        0.0,
        "vertical balance for {ups:?}"
    );
    assert_eq!(
        f1 + 0.5 * f2 - 0.5 * f3 - f4 - 0.5 * f5 + 0.5 * f6,
        0.0,
        "horizontal balance for {ups:?}"
    );
}

/// B11 end to end (PRD2 D4): the engine hands the solver its autos in
/// declaration order, so every fresh eval returns the same balance model —
/// the true-first one, (T,F,T,F,T,F). The registry-seam twin in
/// `reify-constraints`' `registry_tests` is the reliable detector of an order
/// leak; this locks that the engine layer adds none of its own.
#[test]
fn discrete_balance_lets_fixture_resolves_identically_across_evals() {
    const DECLARATION_ORDER_FIRST_MODEL: [bool; 6] = [true, false, true, false, true, false];
    const EVALS: usize = 6;

    let source = balance_fixture_source();
    let observed: Vec<[bool; 6]> = (0..EVALS)
        .map(|_| resolved_ups(&eval_through_production_registry(&source, WHAT)))
        .collect();

    assert!(
        observed
            .iter()
            .all(|ups| *ups == DECLARATION_ORDER_FIRST_MODEL),
        "every eval must return the declaration-order model \
         {DECLARATION_ORDER_FIRST_MODEL:?}; observed {observed:?}"
    );
}
