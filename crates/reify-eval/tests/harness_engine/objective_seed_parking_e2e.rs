//! TRIAGE PROBE P7 (task #6756) — the seed parking reproduced at the **`.ri` driver
//! level**, and the silence that hides it.
//!
//! # Provenance
//!
//! * Task: **#6756** (triage-investigation; deliverable = discriminating probe set
//!   + mechanism verdict, *not* a fix).
//! * Date: **2026-08-28**
//! * Measured at HEAD: **`9c1bed42a7cb949cfe15dcee67052c84d4d41ff3`** (`9c1bed42a7`,
//!   "Merge task/6341 into main", 2026-08-28T19:52:47+01:00), branch `task/6756`.
//! * Instrument: this file —
//!   `cargo test -p reify-eval --test harness_engine objective_seed_parking_e2e`.
//! * Solver-level probe set (P1–P6, P8):
//!   `crates/reify-constraints/tests/objective_seed_parking_triage.rs`.
//! * Write-up: `docs/notes/objective-seed-parking-triage-2026-08-27.md`.
//!
//! **Citation convention.** Anchors lead with the **symbol**; the line range is only a
//! parenthetical hint, point-in-time and valid at the HEAD above. That is the house
//! convention (`docs/prds/v0_6/solution-set-completeness.md:5` — "Main moves fast —
//! cite-by-symbol; re-locate lines at implementation time"), and it matters here because
//! this file is written to be read later, by the owners of `#5711`/`#6678`/`#6654`.
//! Grep the symbol first; a range that no longer matches it is stale, not a finding.
//!
//! # What this probe adds over P1–P6
//!
//! P1–P6 build a [`reify_ir::ResolutionProblem`] directly. P7 goes through the real
//! driver — `compile_source_with_stdlib` → `Engine::eval` — so it proves the parking is
//! reachable from ordinary `.ri` source, not an artefact of a hand-built problem. It
//! reuses the inline-`&str` harness at
//! `crates/reify-eval/tests/solver_optimality_unproven.rs:79-131` verbatim rather than
//! adding a checked-in `.ri` file under the prd-gate fixtures directory, which would
//! pull in a `_RUST_COUPLED_RI_FIXTURES` pinning obligation in
//! `scripts/verify.sh:1050-1081` (and the PG-DRIFT scenario in
//! `tests/infra/test_verify_scope.sh`) for no triage gain.
//!
//! It asserts two things:
//!
//! 1. the resolved value is the **seed**, not the bound the objective points at; and
//! 2. **no** diagnostic carries [`DiagnosticCode::SolverOptimalityUnproven`] — the wrong
//!    answer comes back silently. That silence is why loudness is a real, separate
//!    deliverable, owned by `#6654` arm 3.
//!
//! # Measured result (HEAD `9c1bed42a7`)
//!
//! `SeedParking.x` = `2.40000000000000005e-2` m = **24.000000 mm**, bits
//! `0x3f989374bc6a7efa` — bit-identical to the solver-level P2 measurement, and 16mm
//! away from the 8mm bound `minimize x` points at. Total diagnostic count: **0**.
//!
//! The contrast the probe set establishes reads cleanly:
//!
//! | shape | clamp wall inside the constraint region? | outcome |
//! |---|---|---|
//! | P1–P4, P7 (**production**, `bounds: None`) | no | parks at the seed, bit-exactly |
//! | P6 (the shape of `optimize_with_feasible_initial_point`, `solver_integration.rs:498`) | yes (5mm–100mm) | real progress to the 5mm floor |
//!
//! # Candidate (c) needs no probe of its own
//!
//! The Money robustness floor and the centrality blend are ruled out **by construction**,
//! and the two anchors are cited here rather than duplicated into a new fixture:
//!
//! * The floor is Money-gated — `objective_is_money`
//!   (`crates/reify-constraints/src/solver.rs:820`) with its gate in
//!   `solve_core_with_sd_tolerance` at `:1755-1760` — and
//!   `crates/reify-constraints/tests/robustness_floor.rs:397`
//!   (`non_money_objective_unchanged`) already pins that a non-Money objective is
//!   untouched. Every probe here uses a `Length` objective.
//! * `build_centrality_objective` is synthesised only when `problem.objective.is_none()`
//!   (`build_centrality_objective`'s call site in `solve_core_with_sd_tolerance`,
//!   `solver.rs:1847`), so it cannot fire when the author wrote `minimize`/`maximize`.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, compile_source_with_stdlib};

/// P7 source: the two-sided production shape, at the driver level.
///
/// `minimize x` under `8mm <= x <= 40mm`. A correct `minimize` returns 8mm; the
/// derived-box midpoint SEED is 24mm (`extract_initial_point` arm 3 — doc
/// `crates/reify-constraints/src/solver.rs:402-419`, body `:420-440`; the line range is a
/// point-in-time hint, the symbol is the durable cite).
///
/// The two-sided form is used deliberately: a one-sided-only auto can trip the separate
/// one-sided-auto path owned by `#6692`/`#6655`, which would confound the reading.
const TWO_SIDED_SOURCE: &str = r#"
structure SeedParking {
    param x: Length = auto
    constraint x >= 8mm
    constraint x <= 40mm
    minimize x
}
"#;

/// **P7 — driver-level reproduction: the answer is the seed, and nothing says so.**
///
/// PREDICTION: `SeedParking.x` resolves to **24mm** (the derived-box midpoint seed),
/// NOT the 8mm bound that `minimize x` points at — and no
/// `DiagnosticCode::SolverOptimalityUnproven` is emitted, because that warning is gated
/// on `BestFoundReason::IterationLimit` by the γ-gate in `Engine::eval`
/// (`crates/reify-eval/src/engine_eval.rs:6120-6136`, the "γ (task #4804)" comment) and
/// this solve converges within budget (measured by P5).
///
/// MEASURED at HEAD `9c1bed42a7`: `SeedParking.x` = `2.40000000000000005e-2` m =
/// **24.000000 mm**, bits `0x3f989374bc6a7efa` — **bit-identical** to the solver-level
/// P2 result, so the driver adds nothing and subtracts nothing. `minimize x` moved the
/// answer **16mm in the wrong direction** from the 8mm bound it points at.
///
/// And `result.diagnostics.len()` == **0**. Not merely "no `SolverOptimalityUnproven`":
/// the whole eval emitted **no diagnostic of any kind**. An author gets 24mm back with
/// nothing at all to indicate the objective was never honoured.
#[test]
fn p7_ri_driver_minimize_parks_at_seed_silently() {
    let compiled = compile_source_with_stdlib(TWO_SIDED_SOURCE);

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "SeedParking fixture should compile without errors: {:#?}",
        compile_errors
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    let x_id = ValueCellId::new("SeedParking", "x");
    let x_si = match result.values.get(&x_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected Scalar for SeedParking.x, got {:?}", other),
    };

    // (i) The answer is the SEED (24mm), not the 8mm bound `minimize x` points at.
    assert!(
        (x_si - 0.024).abs() <= 1e-9,
        "P7 `minimize x` s.t. `8mm <= x <= 40mm` at the .ri driver level: expected the \
         derived-box midpoint SEED 0.024 m; got {x_si} m ({} mm). A correct minimize \
         returns 8mm — the objective moved the answer by 0.",
        x_si * 1000.0
    );
    assert!(
        (x_si - 0.008).abs() > 1e-9,
        "P7: if x reached the 8mm bound, the defect is fixed — re-measure this whole \
         probe set and the write-up rather than reverting (owners: #5711 / #6678)."
    );

    // (ii) …and nothing warns. The wrong number is returned SILENTLY.
    let optimality_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SolverOptimalityUnproven))
        .collect();
    assert!(
        optimality_warnings.is_empty(),
        "P7: expected NO SolverOptimalityUnproven diagnostic (the gate at \
         the gamma-gate in Engine::eval (engine_eval.rs:6120-6136) requires \
         BestFoundReason::IterationLimit, and this \
         solve converges within budget — see P5). Getting one means the loudness work \
         in #6654 arm 3 has landed; update this probe rather than reverting. Got: {:#?}",
        optimality_warnings
    );
}
