//! Eval-level e2e test for task #6465 item (2): a γ
//! `cost_robustness_tradeoff` model whose strict auto is DEFAULT-BOUNDS-determined
//! must say so PRECISELY — naming the param, the side no constraint bounded, and
//! the solver-internal bound the solve fell back to.
//!
//! Deliberately a separate file from `cost_robustness_tradeoff_example_e2e.rs`:
//! that file's stated purpose is the λ sweep over the SHIPPED
//! `examples/cost_robustness_tradeoff.ri`, and a prd-gate-fixture diagnostic
//! assertion is not part of it.
//!
//! The fixture under test is `tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri`
//! — the canonical MISSING-UPPER-BOUND γ shape, and the one the task record
//! names. Its `thickness` carries `constraint thickness > 1mm` and nothing
//! above, so `derive_param_intervals` reads `hi: None` and the solve's upper
//! side comes from `default_bounds_for(Length)` = 10 m: a value pinned by a
//! solver-internal default the model never authored, for a mm-scale part.
//! `verify_uniqueness`' γ branch already MEASURES exactly that; before #6465
//! item (2) it collapsed the measurement to a bool, so `finalise_uniqueness`
//! could only emit one generic sentence for three different causes.
//!
//! Harness (`compile_source_with_stdlib` / `MockConstraintChecker` /
//! `collect_errors` / a `CARGO_MANIFEST_DIR`-relative path const) mirrors
//! `cost_robustness_tradeoff_example_e2e.rs`, so both γ eval-layer tests read
//! the same way.

use reify_constraints::DimensionalSolver;
use reify_core::DiagnosticCode;
use reify_eval::Engine;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// The prd-gate fixture, resolved relative to this crate's manifest directory
/// (mirrors `cost_robustness_tradeoff_example_e2e.rs::EXAMPLE_PATH`).
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri"
);

/// The param the fixture leaves unbounded above, as `ValueCellId`'s `Display`
/// renders it (`entity.member`).
const PARAM_FQN: &str = "CostTradeoffPart.thickness";

/// The `ConstraintNonUnique` message must name the param, the missing SIDE, and
/// the bound the solve actually fell back to — and must still carry the
/// `not uniquely determined` diagnosis phrase, which four non-γ tests elsewhere
/// substring-match and which must therefore stay ONE phrase across both
/// branches.
///
/// RED at authoring time, MEASURED on the built release binary against this same
/// fixture: the only error-severity diagnostic is
///
///     error: strict auto parameter resolution is not uniquely determined \
///            — consider using auto(free) for exploration
///
/// which names neither the param, nor the side, nor the bound. (The eval layer
/// then reports `CostTradeoffPart.thickness = undef` with
/// `UndefCause::SolveFailed`, because `finalise_uniqueness` demotes the whole
/// result to `Infeasible` — so the fallback bound is NOT visible in the output
/// as a resolved value, which is precisely why the message has to state it.)
#[test]
fn underdetermined_gamma_model_names_param_side_and_fallback_bound() {
    let src = std::fs::read_to_string(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("could not read {FIXTURE_PATH}: {e}"));

    let compiled = compile_source_with_stdlib(&src);
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "{FIXTURE_PATH} should compile without errors: {compile_errors:#?}"
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    let non_unique: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique))
        .collect();
    assert_eq!(
        non_unique.len(),
        1,
        "expected exactly one ConstraintNonUnique diagnostic from this fixture; \
         all diagnostics: {:#?}",
        result.diagnostics
    );
    let message = &non_unique[0].message;

    assert!(
        message.contains("not uniquely determined"),
        "the diagnosis phrase must stay one phrase across both `Determinedness` \
         branches — four non-γ tests substring-match it. Got: {message}"
    );
    assert!(
        message.contains(PARAM_FQN),
        "the message must NAME the under-determined param ({PARAM_FQN}); the user \
         cannot act on a verdict that does not say which param it is about. \
         Got: {message}"
    );
    assert!(
        message.contains("above"),
        "the message must name the SIDE no constraint bounded — the fixture's \
         `constraint thickness > 1mm` bounds it below, so the missing side is \
         ABOVE, and that is the constraint the user has to add. Got: {message}"
    );
    assert!(
        message.contains("10"),
        "the message must state the bound the solve FELL BACK TO — \
         `default_bounds_for(Length)`'s 10 m ceiling — because the result is \
         demoted to Infeasible and `thickness` prints as `undef`, so that number \
         appears nowhere else in the output the user sees. Got: {message}"
    );
}
