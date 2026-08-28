//! TRIAGE PROBES (task #6756) — `minimize`/`maximize` park at the SEED instead of
//! seeking the constraint bound.
//!
//! # Provenance
//!
//! * Task: **#6756** (triage-investigation; deliverable = discriminating probe set
//!   + mechanism verdict, *not* a fix).
//! * Date: **2026-08-28**
//! * Measured at HEAD: **`9c1bed42a7cb949cfe15dcee67052c84d4d41ff3`** (`9c1bed42a7`,
//!   "Merge task/6341 into main", 2026-08-28T19:52:47+01:00), branch `task/6756`.
//! * Instrument: this file —
//!   `cargo test -p reify-constraints --test objective_seed_parking_triage`.
//! * Companion driver-level probe: `crates/reify-eval/tests/objective_seed_parking_e2e.rs`.
//! * Write-up: `docs/notes/objective-seed-parking-triage-2026-08-27.md`.
//!
//! **All `file:line` anchors in this file are point-in-time, valid at the HEAD above —
//! re-verify against current `main` before relying on them.**
//!
//! # What these probes are (and are not)
//!
//! Every probe asserts the **current, characterised** value, and each predicted value
//! is *derived* from a named in-tree constant — never a threshold tuned to match an
//! unknown output. No probe asserts the *desired* behaviour ("minimize reaches 8mm"):
//! the fixes are owned elsewhere (`#5711` for the `floor_applied` clamp gate, `#6678`
//! for retiring `PENALTY_WEIGHT`), so a desired-behaviour assertion would be a doomed
//! RED that no in-scope change can turn GREEN.
//!
//! # Production shape
//!
//! Every probe builds its auto param with `bounds: None` — the PRODUCTION shape.
//! `AutoParam.bounds` is *always* `None` in production: all three construction sites
//! hardcode it (`crates/reify-constraints/src/solver.rs:993-997` names them —
//! `reify-eval/src/engine_eval.rs:1436`, `engine_edit.rs:1470`, `:3635`) and no `.ri`
//! surface sets it. `free: true` keeps `verify_uniqueness` out of the probe (same
//! rationale as the 8+ existing `free: true` sites in `solver_integration.rs`), so a
//! seed-parking probe is not confounded with a uniqueness verdict.

use reify_constraints::DimensionalSolver;
use reify_core::Type;
use reify_ir::{
    AutoParam, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet, ResolutionProblem,
    SolveResult, ValueMap,
};
use reify_test_support::*;

/// Entity/member the probes resolve.
const ENTITY: &str = "Probe";
const MEMBER: &str = "x";

/// Mirror of the private `SEED_NUDGE_REL` at
/// `crates/reify-constraints/src/solver.rs:239`. Re-declared here (the solver const is
/// private) so every prediction below is *derived* from the shipped constant rather
/// than guessed.
const SEED_NUDGE_REL: f64 = 0.1;

/// Absolute tolerance for every probe assertion, in metres (1 nm).
///
/// Deliberately tight: the drift fallback returns the **exact** initial point (pinned
/// by `solver_integration.rs:1483`, `warm_start_fallback_returns_exact_initial_values`),
/// so a loose tolerance would hide the whole point of these probes.
const TOL_M: f64 = 1e-9;

/// Build a probe problem in the PRODUCTION shape (`bounds: None`, `free: true`).
///
/// `seed_mm` populates `current_values`, exercising arm 1 of `extract_initial_point`
/// (`solver.rs:402-419`); `None` leaves `current_values` empty so arm 3 (the
/// constraint-derived box, task #5618) supplies the seed.
fn probe_problem(
    constraints: Vec<CompiledExpr>,
    objective: Option<ObjectiveSense>,
    seed_mm: Option<f64>,
) -> ResolutionProblem {
    let x_id = vcid(ENTITY, MEMBER);
    let mut current_values = ValueMap::new();
    if let Some(seed) = seed_mm {
        current_values.insert(x_id.clone(), mm(seed));
    }
    ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![AutoParam {
            id: x_id,
            param_type: Type::length(),
            // PRODUCTION shape — see module doc. NOT `Some((lo, hi))`: an explicit
            // wall strictly inside the constraint region is exactly what makes the
            // existing suite blind to this defect.
            bounds: None,
            free: true,
        }],
        constraints: constraints
            .into_iter()
            .enumerate()
            .map(|(i, c)| (cnid(ENTITY, i as u32), c))
            .collect(),
        current_values,
        objective: objective.map(|sense| ObjectiveSet::single(sense, value_ref(ENTITY, MEMBER))),
        functions: vec![].into(),
    }
}

/// Solve a probe problem and return the resolved value in **metres (SI)**.
fn probe_si(
    constraints: Vec<CompiledExpr>,
    objective: ObjectiveSense,
    seed_mm: Option<f64>,
) -> f64 {
    let problem = probe_problem(constraints, Some(objective), seed_mm);
    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, .. } => values
            .get(&vcid(ENTITY, MEMBER))
            .expect("solved values must contain the probe auto param")
            .as_f64()
            .expect("probe auto param must resolve to a numeric value"),
        other => panic!(
            "probe expected SolveResult::Solved (the seed is feasible by construction), got {:?}",
            other
        ),
    }
}

/// `x >= 8mm` — the one-sided lower bound shared by P1/P4/P5.
fn ge_8mm() -> CompiledExpr {
    ge(value_ref(ENTITY, MEMBER), literal(mm(8.0)))
}

/// `x <= 40mm` — the one-sided upper bound shared by P3.
fn le_40mm() -> CompiledExpr {
    le(value_ref(ENTITY, MEMBER), literal(mm(40.0)))
}

// ─────────────────────────────────────────────────────────────────────────────
// P1/P2/P3 — reproduce the two REPORTED numbers, and discriminate which
// constraint shape produced the reported 24mm.
// ─────────────────────────────────────────────────────────────────────────────

/// **P1 — reported-number probe, one-sided `minimize`.**
///
/// `minimize x` s.t. `x >= 8mm`, production shape, no seed.
///
/// PREDICTION (derived, not guessed): `8mm × (1 + SEED_NUDGE_REL)` = **8.8mm**, the
/// one-sided inward nudge of `extract_initial_point` arm 3
/// (`solver.rs:402-419` doc, `:420-440` body; `SEED_NUDGE_REL = 0.1` at `solver.rs:239`).
/// The objective never moves it: the answer IS the seed.
#[test]
fn p1_minimize_one_sided_lower_bound_parks_at_nudged_seed() {
    let got = probe_si(vec![ge_8mm()], ObjectiveSense::Minimize, None);
    let predicted = 0.008 * (1.0 + SEED_NUDGE_REL);

    assert!(
        (got - predicted).abs() <= TOL_M,
        "P1 `minimize x` s.t. `x >= 8mm`: expected the one-sided nudged SEED \
         {predicted} m (= 8mm × (1 + SEED_NUDGE_REL), SEED_NUDGE_REL = 0.1 at \
         solver.rs:239), got {got} m ({} mm). Minimizing would reach 8mm.",
        got * 1000.0
    );
}

/// **P2 — reported-number probe, two-sided `maximize`.**
///
/// `maximize x` s.t. `8mm <= x <= 40mm`, production shape, no seed.
///
/// PREDICTION: **24mm** = the midpoint of the constraint-derived box, i.e.
/// `extract_initial_point` arm 3 with BOTH sides derived (`solver.rs:402-419`).
/// This is the reported number — and it is the *same* mechanism as P1, not a separate
/// unexplained effect.
#[test]
fn p2_maximize_two_sided_parks_at_derived_box_midpoint() {
    let got = probe_si(
        vec![ge_8mm(), le_40mm()],
        ObjectiveSense::Maximize,
        None,
    );
    let predicted = (0.008 + 0.040) / 2.0;

    assert!(
        (got - predicted).abs() <= TOL_M,
        "P2 `maximize x` s.t. `8mm <= x <= 40mm`: expected the two-sided derived-box \
         MIDPOINT seed {predicted} m (extract_initial_point arm 3, solver.rs:402-419), \
         got {got} m ({} mm). Maximizing would reach 40mm.",
        got * 1000.0
    );
}

/// **P3 — one-sided control that DISCRIMINATES which shape produced the reported 24mm.**
///
/// `maximize x` s.t. `x <= 40mm` only, production shape, no seed.
///
/// PREDICTION: **36mm** = `40mm − SEED_NUDGE_REL × 40mm`, the one-sided inward nudge
/// from the single derived UPPER bound (`solver.rs:402-419`, `:239`).
///
/// If P3 returns 36mm while P2 returns 24mm, the reported 24mm can only have come from
/// a TWO-SIDED shape — a genuinely one-sided `<= 40mm` does not produce it.
#[test]
fn p3_maximize_one_sided_upper_bound_parks_at_nudged_seed() {
    let got = probe_si(vec![le_40mm()], ObjectiveSense::Maximize, None);
    let predicted = 0.040 - SEED_NUDGE_REL * 0.040;

    assert!(
        (got - predicted).abs() <= TOL_M,
        "P3 `maximize x` s.t. `x <= 40mm`: expected the one-sided nudged SEED \
         {predicted} m (= 40mm − 0.1 × 40mm, SEED_NUDGE_REL at solver.rs:239), \
         got {got} m ({} mm). This is the discriminator against P2's 24mm.",
        got * 1000.0
    );
}
