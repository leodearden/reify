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
//! * Companion driver-level probe: `crates/reify-eval/tests/harness_engine/objective_seed_parking_e2e.rs`.
//! * Write-up: `docs/notes/objective-seed-parking-triage-2026-08-27.md`.
//!
//! **Citation convention.** Anchors here lead with the **symbol** and carry the line
//! range only as a parenthetical hint — the house convention stated in
//! `docs/prds/v0_6/solution-set-completeness.md:5` ("Main moves fast — cite-by-symbol;
//! re-locate lines at implementation time"), which matters doubly for this file: it is
//! written to be read LATER, by the owners of `#5711` / `#6678` / `#6654`, i.e. exactly
//! when the line numbers will have rotted. Symbols survive line drift and are greppable;
//! **every line range below is point-in-time, valid only at the HEAD above.** Grep the
//! symbol first, and treat a range that does not match it as stale, not as a finding.
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
//! # Measured results (HEAD `9c1bed42a7`, `DimensionalSolver::solve`)
//!
//! | Probe | problem | seed | measured | derivation | bit-identical to derivation |
//! |---|---|---|---|---|---|
//! | P1 | `min x` s.t. `x >= 8mm` | none | **8.800000 mm** (`8.80000000000000053e-3` m, bits `0x3f8205bc01a36e2f`) | `8mm × 1.1` | yes |
//! | P2 | `max x` s.t. `8mm <= x <= 40mm` | none | **24.000000 mm** (`2.40000000000000005e-2` m, bits `0x3f989374bc6a7efa`) | `(8mm + 40mm)/2` | yes |
//! | P3 | `max x` s.t. `x <= 40mm` | none | **36.000000 mm** (`3.60000000000000042e-2` m, bits `0x3fa26e978d4fdf3c`) | `40mm − 0.1 × 40mm` | yes |
//! | P4a | `min x` s.t. `x >= 8mm` | 30mm | **30.000000 mm** (bits `0x3f9eb851eb851eb8`) | the seed | yes — bit-exact |
//! | P4b | `min x` s.t. `x >= 8mm` | 12mm | **12.000000 mm** (bits `0x3f889374bc6a7efa`) | the seed | yes — bit-exact |
//! | P4c | `max x` s.t. `8mm <= x <= 40mm` | 11mm | **11.000000 mm** (bits `0x3f86872b020c49ba`) | the seed | yes — bit-exact |
//! | P6 | `min x` s.t. `2mm < x < 50mm`, **wall (5mm, 100mm)** | 25mm | **5.000000 mm** (bits `0x3f747ae147ae147b`) | the clamp floor | — |
//! | P8a | `min x` s.t. `8mm <= x <= 40mm` | none | **24.000000 mm** (bits `0x3f989374bc6a7efa`) | the seed — **bit-identical to P2's `max`** | yes |
//! | P8b | `max x` s.t. `x >= 8mm` | none | **10000.000000 mm** = 10 m (bits `0x4024000000000000`) | `default_bounds_for(Length)` upper corner | yes |
//! | P8c | `min x` s.t. `x <= 40mm` | none | **0.001000 mm** = 1e-6 m (bits `0x3eb0c6f7a0b5ed8d`) | `default_bounds_for(Length)` lower corner | yes |
//!
//! P6 is the single deliberate NON-production row (it sets an `AutoParam.bounds` wall);
//! every other row uses `bounds: None`.
//!
//! Every probe returned `SolveResult::Solved { unique: false }` — never `Infeasible`
//! or `NoProgress`. The `unique: false` is **expected and not a divergence**: the
//! `unique: true` constructed by the `initially_feasible` drift fallback in
//! `solve_core_with_sd_tolerance` (`solver.rs:2029`) is documented as a *placeholder* by
//! that function's own doc comment (`:1691-1693`), and `finalise_uniqueness`
//! (`:2694-2730`) overwrites it — its all-`free` arm skips the uniqueness re-solve
//! entirely and reports `unique: false` (`:2723-2728`).
//!
//! P5 (`solve_ranked` on P1's problem) measured
//! `Ranked { candidates: [ { objective_score: Some(0.0088) } ], optimality: BestFound
//! { reason: ConvergedWithinBudget } }` — 1 candidate, and **not** `IterationLimit`.
//!
//! P2 vs P3 settles the open question from the filing: a genuinely **one-sided**
//! `<= 40mm` returns **36mm**, so the reported 24mm can only have come from a
//! **two-sided** shape. It is the derived-box midpoint seed — the *same* mechanism as
//! P1's 8.8mm, not the separate unexplained effect the filing assumed.
//!
//! # Mechanism — VERDICT: candidate (a), the silent seed-fallback, CONFIRMED
//!
//! Five links, all in `crates/reify-constraints/src/solver.rs`:
//!
//! 1. **SEED.** `extract_initial_point` (`:420-440`, doc `:402-419`) arm 3 — the
//!    constraint-derived box (task #5618): the midpoint when BOTH sides are derived,
//!    otherwise nudged inward from the single derived bound by
//!    `max(SEED_NUDGE_REL × |bound|, SEED_NUDGE_ABS)` (`SEED_NUDGE_REL = 0.1` at
//!    `:239`, `SEED_NUDGE_ABS = 1e-6` at `:244`). Arm 1 takes `current_values` when
//!    present — which is what P4 varies.
//! 2. **NO CLAMP WALL.** The clamp box handed to the optimiser is gated on
//!    `floor_applied` — the `let bounds = if floor_applied` gate in
//!    `solve_core_with_sd_tolerance` (`:1809-1825`): the constraint-derived clamp box is
//!    used ONLY when the Money robustness floor fired. A `Length` objective is not Money
//!    (`objective_is_money` `:820`, its gate in `solve_core_with_sd_tolerance`
//!    `:1755-1760`), so the else-branch takes `effective_bounds` =
//!    `default_bounds_for(Length)` = `(1e-6, 10.0)` (`:1585-1594`). With
//!    `AutoParam.bounds` always `None` in production — the "Constraint-derived parameter
//!    bounds (task #5618)" header comment above `default_bounds_for` (`:993-997`) names
//!    all three construction sites — there is no wall anywhere near the user's bound.
//! 3. **PENALTY UNDERSHOOT.** Cost is `obj + PENALTY_WEIGHT × violation +
//!    PENALTY_WEIGHT × bound_penalty` — `ConstraintCostFunction::cost` (`:1539-1548`) —
//!    with `PENALTY_WEIGHT = 1e6` (`:25`). Minimising `x + 1e6·(b − x)²` is stationary at
//!    `b − 1/(2 × PENALTY_WEIGHT)` = `b − 5e-7`, i.e. 5e-7 OUTSIDE the active bound. The
//!    `#5618` header comment above `default_bounds_for` (`:1017-1021`) and
//!    `solve_core_with_sd_tolerance`'s own penalty-undershoot note (`:1776-1781`) state
//!    this verbatim, and the latter already names the symptom: "a
//!    feasible-but-badly-suboptimal answer (the seed, returned via the drift fallback)".
//! 4. **FEASIBILITY REJECT.** The final check measures the LINEAR residual against
//!    `FEASIBILITY_THRESHOLD` = 1e-12 (the const at `:20`; the
//!    `final_max_residual > FEASIBILITY_THRESHOLD` check in
//!    `solve_core_with_sd_tolerance` at `:1997`). `5e-7 >> 1e-12`, so the converged
//!    optimum is rejected.
//! 5. **SILENT SEED-FALLBACK.** Because the seed *is* feasible, the `initially_feasible`
//!    drift fallback in `solve_core_with_sd_tolerance` (`:1997-2031`) replaces the
//!    rejected optimum with the untouched initial point and returns it as `Solved`. The
//!    objective is ignored, and the only trace is a `tracing::debug!` — no diagnostic.
//!
//! What each probe rules out:
//!
//! * **P4 rules out candidates (b) and (d).** The answer is a bit-exact function of
//!   the seed while constraints, objective and sense are held fixed. No stalling
//!   optimizer and no mis-plumbed objective produces that.
//! * **P5 rules out candidate (b) independently.** Nelder-Mead terminated on its
//!   sd-tolerance, not the iteration cap. It converges fine; its answer is DISCARDED
//!   at link 4.
//! * **P3 corrects the filing's reading of the reported 24mm** (link 1, two-sided arm).
//! * **P6 explains why the suite is blind.** With a clamp wall inside the constraint
//!   region the objective moves the answer 20mm (25mm seed → 5mm floor); without one it
//!   moves it 0mm. Every in-tree progress-asserting fixture sets that wall, and
//!   production never does. P6 is a deliberate LOCAL RESTATEMENT of
//!   `optimize_with_feasible_initial_point` (`solver_integration.rs:498`), kept here so
//!   the contrast reads in one file — it is **not** independent evidence, and adds no new
//!   solver coverage. See its own doc.
//! * **P7** (`reify-eval/tests/harness_engine/objective_seed_parking_e2e.rs`) reproduces link 5 at the
//!   `.ri` driver level and measures the silence: 24.000000 mm returned with **zero**
//!   diagnostics of any kind.
//! * **P8 closes the verdict.** `minimize` and `maximize` over the same two-sided problem
//!   return bit-identical answers, so no soft-penalty, partial-progress or
//!   seed-coincidence story survives. Its one-sided controls also sharpen link 5's
//!   trigger condition: the seed comes back only when the objective points **toward** a
//!   derived bound (penalty active → 5e-7 undershoot → reject → fallback). Pointing
//!   **away**, the optimum is feasible and the optimiser runs to the
//!   `default_bounds_for(Length)` corner instead (10 m / 1e-6 m) — which is this PRD's
//!   §10 **item 3**, owned by `#6655`/`#6692`, not item 4.
//! * Candidate (c), the Money robustness floor / centrality blend, needs no probe:
//!   the floor is Money-gated (`objective_is_money` `:820`, its gate in
//!   `solve_core_with_sd_tolerance` `:1755-1760`) and
//!   `tests/robustness_floor.rs:397` (`non_money_objective_unchanged`) already pins
//!   that a non-Money objective is untouched, while `build_centrality_objective` is
//!   synthesised only when `problem.objective.is_none()` (`:1847`) and so cannot fire
//!   when the author wrote `minimize`/`maximize`.
//!
//! # These probes are a TRIPWIRE, not a specification
//!
//! They characterise CURRENT behaviour and are **expected to go RED when the owning
//! fix lands** — `#5711` (the `floor_applied` clamp gate, bound by
//! `solve_core_with_sd_tolerance`'s gate note (`solver.rs:1801-1809`) and
//! `verify_uniqueness`'s own doc comment (`:2605-2612`) to the `verify_uniqueness`
//! contract: "Revisit both together;
//! neither is actionable in isolation") or `#6678` (retire `PENALTY_WEIGHT`, the 5e-7
//! trigger; `#6688` was cancelled-absorbed into `#6678` on 2026-08-27). A RED here is
//! the signal to RE-MEASURE and update the table, not a regression to revert.
//!
//! # Production shape
//!
//! Every probe builds its auto param with `bounds: None` — the PRODUCTION shape.
//! `AutoParam.bounds` is *always* `None` in production: all three construction sites
//! hardcode it (the "Constraint-derived parameter bounds (task #5618)" header comment
//! above `default_bounds_for`, `crates/reify-constraints/src/solver.rs:993-997`, names
//! them —
//! `reify-eval/src/engine_eval.rs:1436`, `engine_edit.rs:1470`, `:3635`) and no `.ri`
//! surface sets it. `free: true` keeps `verify_uniqueness` out of the probe (same
//! rationale as the 8+ existing `free: true` sites in `solver_integration.rs`), so a
//! seed-parking probe is not confounded with a uniqueness verdict.

use reify_constraints::DimensionalSolver;
use reify_core::Type;
use reify_ir::{
    AutoParam, BestFoundReason, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet,
    OptimalityStatus, RankedSolveResult, ResolutionProblem, SolveResult, ValueMap,
};
use reify_test_support::*;

/// Entity/member the probes resolve.
const ENTITY: &str = "Probe";
const MEMBER: &str = "x";

/// Mirror of the private `SEED_NUDGE_REL` const read by `extract_initial_point`
/// (`crates/reify-constraints/src/solver.rs:239`). Re-declared here (the solver const is
/// private) so every prediction below is *derived* from the shipped constant rather
/// than guessed.
const SEED_NUDGE_REL: f64 = 0.1;

/// Absolute tolerance for every probe assertion, in metres (1 nm).
///
/// Deliberately tight: the drift fallback returns the **exact** initial point (pinned
/// by `solver_integration.rs:1483`, `warm_start_fallback_returns_exact_initial_values`),
/// so a loose tolerance would hide the whole point of these probes.
const TOL_M: f64 = 1e-9;

/// RELATIVE tolerance for the two P8 one-sided corner controls, applied as
/// `TOL_REL × max(|expected|, 1.0)`.
///
/// [`TOL_M`] is the right instrument for every O(1e-2) m target in this file, but against
/// the **10 m** `default_bounds_for(Length)` corner an absolute 1e-9 is a 1e-10 *relative*
/// tolerance — far tighter than anything Nelder-Mead promises. That corner comes back
/// bit-exact today only because the final `val.clamp(lo, hi)` in
/// `solve_core_with_sd_tolerance` (`solver.rs:1971-1980`, whose `clamped` vector is what
/// `build_solved_values` returns at `:2139`) snaps the penalty-method
/// overshoot back onto the box, NOT because NM converged there to 1e-10. If NM ever
/// terminated marginally INSIDE the bound (say 9.9999996 m), an absolute pin would go RED
/// while the CHARACTERISED behaviour — "runs to the default-box corner instead of parking
/// at the seed" — was unchanged, sending a future reader on a false re-measure.
const TOL_REL: f64 = 1e-9;

/// Build a probe problem in the PRODUCTION shape (`bounds: None`, `free: true`).
///
/// `seed_mm` populates `current_values`, exercising arm 1 of `extract_initial_point`
/// (doc `solver.rs:402-419`, body `:420-440`); `None` leaves `current_values` empty so arm 3 (the
/// constraint-derived box, task #5618) supplies the seed.
fn probe_problem(
    constraints: Vec<CompiledExpr>,
    objective: Option<ObjectiveSense>,
    seed_mm: Option<f64>,
) -> ResolutionProblem {
    // PRODUCTION shape — see module doc. NOT `Some((lo, hi))`: an explicit wall
    // strictly inside the constraint region is exactly what makes the existing suite
    // blind to this defect. P6 is the single deliberate exception.
    probe_problem_shaped(constraints, objective, seed_mm, None)
}

/// [`probe_problem`] generalised over `AutoParam.bounds`, so P6 can build the
/// NON-production wall-inside-the-region shape that the existing suite uses.
fn probe_problem_shaped(
    constraints: Vec<CompiledExpr>,
    objective: Option<ObjectiveSense>,
    seed_mm: Option<f64>,
    bounds: Option<(f64, f64)>,
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
            bounds,
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
    solved_si(&probe_problem(constraints, Some(objective), seed_mm))
}

/// Solve and extract the probe auto param's value in **metres (SI)**.
fn solved_si(problem: &ResolutionProblem) -> f64 {
    match DimensionalSolver.solve(problem) {
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
///
/// MEASURED at HEAD `9c1bed42a7`: `Solved { unique: false }`, `8.80000000000000053e-3` m
/// = **8.800000 mm** (bits `0x3f8205bc01a36e2f`) — bit-identical to the derivation.
/// A correct `minimize` would return 8mm; the objective moved the answer by 0.
#[test]
fn p1_minimize_one_sided_lower_bound_parks_at_nudged_seed() {
    let got = probe_si(vec![ge_8mm()], ObjectiveSense::Minimize, None);
    let predicted = 0.008 * (1.0 + SEED_NUDGE_REL);

    assert!(
        (got - predicted).abs() <= TOL_M,
        "P1 `minimize x` s.t. `x >= 8mm`: expected the one-sided nudged SEED \
         {predicted} m (= 8mm × (1 + SEED_NUDGE_REL) — extract_initial_point arm 3, \
         SEED_NUDGE_REL = 0.1, solver.rs:239 at HEAD 9c1bed42a7); MEASURED 8.800000 mm. Got {got} m \
         ({} mm). Minimizing would reach 8mm.",
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
///
/// MEASURED at HEAD `9c1bed42a7`: `Solved { unique: false }`, `2.40000000000000005e-2` m
/// = **24.000000 mm** (bits `0x3f989374bc6a7efa`) — bit-identical to the derivation.
/// A correct `maximize` would return 40mm; the answer sits 16mm away, at the seed.
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
         MIDPOINT seed {predicted} m (extract_initial_point arm 3, doc solver.rs:402-419 \
         at HEAD 9c1bed42a7); MEASURED 24.000000 mm. Got {got} m ({} mm). \
         Maximizing would reach 40mm.",
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
///
/// MEASURED at HEAD `9c1bed42a7`: `Solved { unique: false }`, `3.60000000000000042e-2` m
/// = **36.000000 mm** (bits `0x3fa26e978d4fdf3c`) — bit-identical to the derivation,
/// and DIFFERENT from P2's 24mm. Verdict: the reported 24mm needs both bounds.
#[test]
fn p3_maximize_one_sided_upper_bound_parks_at_nudged_seed() {
    let got = probe_si(vec![le_40mm()], ObjectiveSense::Maximize, None);
    let predicted = 0.040 - SEED_NUDGE_REL * 0.040;

    assert!(
        (got - predicted).abs() <= TOL_M,
        "P3 `maximize x` s.t. `x <= 40mm`: expected the one-sided nudged SEED \
         {predicted} m (= 40mm − 0.1 × 40mm — extract_initial_point arm 3, \
         SEED_NUDGE_REL at solver.rs:239); MEASURED 36.000000 mm at HEAD 9c1bed42a7. Got {got} m ({} mm). \
         This is the discriminator against P2's 24mm.",
        got * 1000.0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// P4/P5 — the DISCRIMINATING probes. P4 rules out candidates (b) and (d);
// P5 rules out candidate (b) independently.
// ─────────────────────────────────────────────────────────────────────────────

/// **P4 — SEED-VARYING: the decisive discriminator for candidate (a).**
///
/// The same `minimize x` s.t. `x >= 8mm` problem as P1, re-solved with `current_values`
/// seeded at 30mm and then at 12mm — plus the two-sided `8mm <= x <= 40mm` `maximize`
/// shape seeded at 11mm. `extract_initial_point` arm 1 takes the current value
/// (`solver.rs:402-419`), so PREDICTION: the answer TRACKS the seed **exactly**.
///
/// Held fixed across all three: the constraints, the objective, and its sense. Only the
/// seed moves. An output that moves *with the seed* under those conditions can only be
/// the seed being returned — which rules out, in one shot:
///
/// * a fixed attractor (the answer is not pinned to any one value),
/// * a bound-seeking failure that merely stops short (12mm and 30mm bracket the
///   8.8mm one-sided seed from both sides — a "stops short of 8mm" story cannot
///   produce 30mm, and 11mm is *below* the two-sided 24mm midpoint),
/// * candidate (d), an objective-plumbing defect (a mis-plumbed objective would still
///   not make the output a function of the seed).
///
/// Asserted bit-exactly (`to_bits()`), because the drift fallback returns the
/// **exact** initial point (the `build_solved_values(&problem.auto_params, initial)` in
/// `solve_core_with_sd_tolerance`'s drift fallback, `solver.rs:2025-2031`; pinned by
/// `solver_integration.rs:1483`).
///
/// MEASURED at HEAD `9c1bed42a7`, all `Solved { unique: false }`: seed 30mm →
/// **30.000000 mm** (bits `0x3f9eb851eb851eb8`), seed 12mm → **12.000000 mm** (bits
/// `0x3f889374bc6a7efa`), two-sided seed 11mm → **11.000000 mm** (bits
/// `0x3f86872b020c49ba`). Each output is bit-identical to `mm(seed)`.
#[test]
fn p4_answer_tracks_the_seed_bit_exactly() {
    for seed_mm in [30.0_f64, 12.0_f64] {
        let got = probe_si(vec![ge_8mm()], ObjectiveSense::Minimize, Some(seed_mm));
        let seed_si = mm(seed_mm).as_f64().expect("mm() builds a numeric Scalar");
        assert_eq!(
            got.to_bits(),
            seed_si.to_bits(),
            "P4 `minimize x` s.t. `x >= 8mm` seeded at {seed_mm}mm: expected the answer to \
             be the SEED, bit-for-bit ({seed_si} m); got {got} m ({} mm). Only the seed \
             moved — constraints, objective and sense were held fixed.",
            got * 1000.0
        );
    }

    // Two-sided variant, opposite sense, seed BELOW the 24mm derived-box midpoint:
    // the answer still tracks the seed rather than the midpoint or the 40mm bound.
    let got = probe_si(
        vec![ge_8mm(), le_40mm()],
        ObjectiveSense::Maximize,
        Some(11.0),
    );
    let seed_si = mm(11.0).as_f64().expect("mm() builds a numeric Scalar");
    assert_eq!(
        got.to_bits(),
        seed_si.to_bits(),
        "P4 `maximize x` s.t. `8mm <= x <= 40mm` seeded at 11mm: expected the SEED \
         bit-for-bit ({seed_si} m); got {got} m ({} mm). Maximizing would reach 40mm, \
         and the unseeded two-sided answer (P2) is 24mm.",
        got * 1000.0
    );
}

/// **P5 — OPTIMALITY STATUS: rules out candidate (b), Nelder-Mead stalling.**
///
/// `solve_ranked` on P1's problem. PREDICTION: `OptimalityStatus::BestFound { reason }`
/// with `reason` **not** `BestFoundReason::IterationLimit` — i.e. Nelder-Mead terminated
/// on its sd-tolerance, not on the iteration cap. The optimiser is not stalling; its
/// answer is computed and then DISCARDED by the feasibility reject + seed fallback.
///
/// Corroborated in-tree by the `SMALL_MM_SOURCE` doc comment ("B6 source") in
/// `reify-eval/tests/solver_optimality_unproven.rs:123-127`, which documents the
/// identical 1-param case.
///
/// Consequence: `W_SOLVER_OPTIMALITY_UNPROVEN` cannot fire — the warning is gated on
/// the `IterationLimit` variant by the γ-gate in `Engine::eval`
/// (`reify-eval/src/engine_eval.rs:6120-6136`, the "γ (task #4804)" comment) — so the
/// wrong number is returned **silently**. That is what makes loudness (#6654 arm 3) a
/// separate, real deliverable from the fix itself.
///
/// Asserted via the **variant**, never a message substring, and deliberately in the weak
/// `!IterationLimit` form rather than pinning `ConvergedWithinBudget`: the
/// budget-exhaustion spelling is actively being minted by whichever of #6654 / #6671 /
/// #6692 lands first, so pinning the positive variant would invite a doomed RED.
///
/// `SolveMeta` (`solver.rs:89`) and `solve_with_meta` (`solver.rs:2802`) are private, so
/// `solve_ranked` is the only public route to this signal.
///
/// MEASURED at HEAD `9c1bed42a7`: `Ranked { candidates: [ { values: {Probe.x: 0.0088},
/// objective_score: Some(0.0088), unique: false } ], optimality: BestFound { reason:
/// ConvergedWithinBudget } }`. Note the ranked candidate's value is the SEED (0.0088 m)
/// too — the fallback happens upstream of the ranking, so even the "best found"
/// candidate the solver reports is the seed.
#[test]
fn p5_optimality_status_is_not_iteration_limited() {
    let problem = probe_problem(vec![ge_8mm()], Some(ObjectiveSense::Minimize), None);
    let ranked = DimensionalSolver.solve_ranked(&problem);

    match &ranked {
        RankedSolveResult::Ranked {
            candidates,
            optimality,
        } => {
            assert_eq!(candidates.len(), 1, "expected exactly 1 candidate");
            match optimality {
                OptimalityStatus::BestFound { reason } => assert!(
                    !matches!(reason, BestFoundReason::IterationLimit),
                    "P5: Nelder-Mead is NOT iteration-limited here, so candidate (b) \
                     (optimizer stalling) cannot explain the seed parking; got {:?}",
                    reason
                ),
                other => panic!(
                    "P5: Nelder-Mead is derivative-free → always BestFound; got {:?}",
                    other
                ),
            }
        }
        other => panic!("P5 expected Ranked, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// P6 — the CONTRAST CONTROL. Why the existing suite is blind to all of the above.
// ─────────────────────────────────────────────────────────────────────────────

/// **P6 — WALL-INSIDE-REGION CONTROL: the objective DOES make progress.**
///
/// The same `minimize x` shape, but with an explicit `bounds: Some((0.005, 0.1))` wall
/// strictly INSIDE a `2mm < x < 50mm` constraint region, seeded at 25mm. This is the
/// shape of `solver_integration.rs:498` (`optimize_with_feasible_initial_point`), which
/// passes on `main` today — and whose own doc comment states the mechanism outright:
/// "Auto param bounds (5mm–100mm) prevent the solver from overshooting the constraint
/// boundary at 2mm, so the optimizer converges at the bounds floor".
///
/// PREDICTION: **≈5mm** — the clamp wall, i.e. real objective-driven progress from the
/// 25mm seed.
///
/// Its role is to isolate the differentiator. The defect appears only when NO clamp
/// wall lies inside the constraint region — which is the PRODUCTION configuration,
/// because `AutoParam.bounds` is always `None` (the `#5618` header comment above
/// `default_bounds_for`, `solver.rs:993-997`). Every in-tree
/// objective fixture that asserts real progress sets such a wall, so the corpus cannot
/// see this defect; and the one fixture that puts the wall OUTSIDE the region
/// (`solver_integration.rs:618`,
/// `warm_start_falls_back_to_initial_when_optimizer_drifts_infeasible`) encodes the
/// symptom as INTENDED behaviour. Hence a real defect with fully green coverage.
///
/// This probe is the deliberate exception to the module-wide production shape.
///
/// # Not independent evidence — a deliberate local restatement
///
/// P6 is a near-verbatim reconstruction of `optimize_with_feasible_initial_point`
/// (`solver_integration.rs:498`): same `2mm < x < 50mm` constraints, the same explicit
/// 5mm–100mm wall, the same 25mm seed, the same `Minimize` sense, the same ≈5mm outcome.
/// It therefore adds **no new solver coverage** — a future reader must not count it as a
/// second, independent observation. It is kept local anyway so the contrast that explains
/// the suite's blindness reads in one file, immediately beside the probes it contrasts
/// with. Candidate (c) got the other treatment for the same trade-off, cited by symbol
/// (`robustness_floor.rs::non_money_objective_unchanged`) with no local restatement,
/// because nothing there needed to be read side-by-side.
///
/// MEASURED at HEAD `9c1bed42a7`: `Solved { unique: false }`, `5.00000000000000010e-3` m
/// = **5.000000 mm** (bits `0x3f747ae147ae147b`) — the clamp floor, i.e. the objective
/// drove the answer 20mm from its 25mm seed. Contrast P1–P4/P7, which move 0mm.
#[test]
fn p6_wall_inside_constraint_region_makes_real_progress() {
    let x = value_ref(ENTITY, MEMBER);
    let problem = probe_problem_shaped(
        vec![
            gt(x.clone(), literal(mm(2.0))),
            lt(x, literal(mm(50.0))),
        ],
        Some(ObjectiveSense::Minimize),
        Some(25.0),
        // NON-production: the wall (5mm-100mm) sits strictly inside `2mm < x < 50mm`.
        Some((0.005, 0.1)),
    );
    let got = solved_si(&problem);

    // One assertion, not two: pinning `got` to the 5mm floor within TOL_M already
    // *implies* it is 20mm from the 25mm seed, so a separate "must not park at the seed"
    // check would be unreachable. The seed contrast lives in the message instead.
    assert!(
        (got - 0.005).abs() <= TOL_M,
        "P6 `minimize x` s.t. `2mm < x < 50mm` with a 5mm-100mm WALL and a 25mm seed: \
         expected the objective to drive the answer to the 5mm clamp floor — i.e. 20mm \
         away from its own seed, which IS the contrast against P1-P4/P7 (those move 0mm). \
         Got {got} m ({} mm). If this parks at the 25mm seed instead, the contrast that \
         explains the suite's blindness is gone and the whole verdict must be re-derived.",
        got * 1000.0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// P8 — SENSE-INVARIANCE. The sharpest single discriminator in the set.
// ─────────────────────────────────────────────────────────────────────────────

/// Mirror of `default_bounds_for(Length)` (`crates/reify-constraints/src/solver.rs:1585-1594`)
/// — "1 micron to 10 meters". This is what `effective_bounds` degrades to in production,
/// because `AutoParam.bounds` is always `None` (the `#5618` header comment above
/// `default_bounds_for`, `:993-997`) and the constraint-derived clamp box is gated on
/// `floor_applied` in `solve_core_with_sd_tolerance` (`:1809-1825`).
const DEFAULT_LENGTH_BOUNDS_M: (f64, f64) = (1e-6, 10.0);

/// **P8 — on a two-sided problem, the objective's SENSE has no effect on the answer.**
///
/// Solve the SAME two-sided problem (`8mm <= x <= 40mm`, production shape, no seed)
/// twice — once `Minimize`, once `Maximize` — and assert the two answers are EQUAL
/// **bit-exactly** (`to_bits()`, the idiom at `registry_tests.rs:386`) and both equal
/// the 24mm derived-box midpoint seed.
///
/// PREDICTION: both **24mm** = `(8mm + 40mm)/2`, `extract_initial_point` arm 3 with both
/// sides derived (`solver.rs:402-419`). A correct solver returns 8mm for one and 40mm
/// for the other — a 32mm spread.
///
/// This is the discriminator no other explanation survives. Minimize and maximize
/// producing the *identical* answer rules out, all at once:
///
/// * a soft-penalty explanation (a soft penalty still pulls the two senses apart),
/// * a partial-progress explanation (partial progress in opposite directions is not
///   bit-identical), and
/// * a seed-coincidence explanation (a coincidence cannot survive negating the
///   objective).
///
/// It is also what settles the severity question: an objective whose sense has **no
/// effect on the answer** is a correctness defect, not a tolerance or solution-quality
/// one.
///
/// # The one-sided controls, and a corrected prediction
///
/// The controls below complete the sense × shape grid. **They contradicted this task's
/// planned prediction** (that a one-sided shape returns its nudged seed under *either*
/// sense), and are pinned here as MEASURED, with the divergence filed as `esc-6756-1`
/// rather than retuned. All values measured at HEAD `9c1bed42a7`:
///
/// | | `x >= 8mm` | `x <= 40mm` | `8mm <= x <= 40mm` |
/// |---|---|---|---|
/// | `Minimize` | **8.800000 mm** — seed (P1) | **0.001000 mm** = 1e-6 m — default corner (**P8**) | **24.000000 mm** — seed (**P8**) |
/// | `Maximize` | **10000.000000 mm** = 10 m — default corner (**P8**) | **36.000000 mm** — seed (P3) | **24.000000 mm** — seed (P2) |
///
/// The two corner values are exactly `default_bounds_for(Length)` = `(1e-6, 10.0)`
/// (`solver.rs:1585-1594`), so they are still *derived*, just from a different constant
/// than planned.
///
/// This SHARPENS the candidate-(a) verdict rather than contradicting it. The seed is
/// returned exactly when the objective points **toward** a derived bound — that is when
/// the penalty term is active, so the optimum lands 5e-7 outside the bound
/// (`PENALTY_WEIGHT` `:25`, `ConstraintCostFunction::cost` `:1539-1548`), is rejected
/// against `FEASIBILITY_THRESHOLD` (`:20`, checked in `solve_core_with_sd_tolerance` at
/// `:1997`), and the `initially_feasible` drift fallback fires (`:1997-2031`). When the objective points **away**, the optimum is
/// feasible, nothing is rejected, no fallback fires, and the optimiser simply runs to the
/// default-box corner. Both reported numbers (8.8mm, 24mm) are the points-toward case.
///
/// The 10 m corner independently reproduces
/// `docs/prds/v0_6/solution-set-completeness.md` §10 **item 3** (owned by `#6655` /
/// P1-ε `#6692`); the `minimize` mirror image — the 1e-6 m LOWER corner — is not named in
/// item 3's wording. Recorded for that item's owner; **not** in scope here.
///
/// Item 4's own wording is unaffected: it describes `maximize` against `<= 40mm`, and a
/// genuinely one-sided `<= 40mm` does return **36mm** under `maximize` (P3). The reported
/// 24mm still requires BOTH bounds.
#[test]
fn p8_objective_sense_has_no_effect_on_the_answer() {
    let minimized = probe_si(vec![ge_8mm(), le_40mm()], ObjectiveSense::Minimize, None);
    let maximized = probe_si(vec![ge_8mm(), le_40mm()], ObjectiveSense::Maximize, None);

    assert_eq!(
        minimized.to_bits(),
        maximized.to_bits(),
        "P8: `minimize x` and `maximize x` over the SAME `8mm <= x <= 40mm` problem \
         returned different values ({minimized} m vs {maximized} m). At HEAD 9c1bed42a7 \
         they were bit-identical — if the sense now matters, the defect is being fixed \
         (owners #5711 / #6678): re-measure this probe set and the write-up rather than \
         reverting."
    );

    let midpoint = (0.008 + 0.040) / 2.0;
    assert!(
        (minimized - midpoint).abs() <= TOL_M,
        "P8: both senses should return the 24mm derived-box midpoint SEED ({midpoint} m); \
         got {minimized} m ({} mm). A correct solver returns 8mm for minimize and 40mm \
         for maximize — a 32mm spread.",
        minimized * 1000.0
    );

    // ── One-sided controls: the objective points AWAY from the single derived bound, so
    // no penalty term is active, nothing is rejected, and the optimiser runs to the
    // `default_bounds_for(Length)` corner instead of parking at the seed.
    let (default_lo, default_hi) = DEFAULT_LENGTH_BOUNDS_M;

    // Corner controls use TOL_REL, not TOL_M — see the `TOL_REL` doc: the bit-exactness
    // here comes from the final clamp, not from NM convergence, so an absolute 1e-9
    // against a 10 m target would be a false tripwire.
    let max_one_sided_lower = probe_si(vec![ge_8mm()], ObjectiveSense::Maximize, None);
    assert!(
        (max_one_sided_lower - default_hi).abs() <= TOL_REL * default_hi.abs().max(1.0),
        "P8 control: `maximize x` s.t. `x >= 8mm` is unbounded above by the constraints, \
         so it runs to the default_bounds_for(Length) UPPER corner {default_hi} m \
         (fn default_bounds_for, solver.rs:1585-1594 at HEAD 9c1bed42a7) — NOT to P1's \
         8.8mm seed. Got {max_one_sided_lower} m \
         ({} mm). This reproduces PRD §10 item 3 (owned by #6655 / #6692).",
        max_one_sided_lower * 1000.0
    );

    let min_one_sided_upper = probe_si(vec![le_40mm()], ObjectiveSense::Minimize, None);
    assert!(
        (min_one_sided_upper - default_lo).abs() <= TOL_REL * default_lo.abs().max(1.0),
        "P8 control: `minimize x` s.t. `x <= 40mm` is unbounded below by the constraints, \
         so it runs to the default_bounds_for(Length) LOWER corner {default_lo} m \
         (fn default_bounds_for, solver.rs:1585-1594) — the mirror image of the 10 m corner, which PRD §10 \
         item 3 does not name. Got {min_one_sided_upper} m ({} mm).",
        min_one_sided_upper * 1000.0
    );
}
