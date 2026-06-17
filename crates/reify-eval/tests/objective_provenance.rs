//! θ integration tests (PRD §10.7, task 4015): ObjectiveProvenance per auto cell
//! is attached to EvalResult after eval().
//!
//! Two RED/GREEN cycles:
//!   Cycle 1 (steps 1-2): record exists, attached, synthetic_centrality + governing
//!     set + combination correct, term_contributions left empty.
//!   Cycle 2 (steps 3-4): per-term realised contributions computed.
//!
//! RED until step-2: reify_ir::ObjectiveProvenance/TermContribution and
//! EvalResult.objective_provenance do not exist yet.

use reify_constraints::DimensionalSolver;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::ObjectiveCombination;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

fn weighted_fixture_source() -> &'static str {
    include_str!("fixtures/objective_set_weighted.ri")
}

fn centrality_fixture_source() -> &'static str {
    include_str!("fixtures/centrality_two_sided_bound.ri")
}

/// [θ Cycle 1a] An explicit objective (WeightedSum) records provenance for each
/// resolved auto cell: governing set present, combination=WeightedSum,
/// synthetic_centrality=false, scope="WeightedObjective".
#[test]
fn explicit_objective_records_provenance() {
    let compiled = compile_source_with_stdlib(weighted_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let mass_id = ValueCellId::new("WeightedObjective", "mass");
    let stiffness_id = ValueCellId::new("WeightedObjective", "stiffness");

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics from eval: {:#?}",
        result.diagnostics
    );

    for cell_id in [&mass_id, &stiffness_id] {
        let prov = result
            .objective_provenance
            .get(cell_id)
            .unwrap_or_else(|| panic!("no ObjectiveProvenance for {:?}", cell_id));

        assert_eq!(
            prov.scope, "WeightedObjective",
            "scope mismatch for {:?}",
            cell_id
        );
        assert!(
            prov.objective.is_some(),
            "objective should be Some for explicit WeightedSum; cell={:?}",
            cell_id
        );
        assert_eq!(
            prov.combination,
            Some(ObjectiveCombination::WeightedSum),
            "combination should be WeightedSum; cell={:?}",
            cell_id
        );
        assert!(
            !prov.synthetic_centrality,
            "synthetic_centrality should be false for explicit objective; cell={:?}",
            cell_id
        );
    }
}

/// [θ Cycle 1b] A synthetic-centrality scope (CentredBar, no objective) records
/// provenance: objective=None, combination=None, term_contributions empty,
/// synthetic_centrality=true.
#[test]
fn synthetic_centrality_records_provenance() {
    let compiled = compile_source_with_stdlib(centrality_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let x_id = ValueCellId::new("CentredBar", "x");

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics from eval: {:#?}",
        result.diagnostics
    );

    let prov = result
        .objective_provenance
        .get(&x_id)
        .expect("no ObjectiveProvenance for CentredBar.x");

    assert_eq!(prov.scope, "CentredBar", "scope mismatch");
    assert!(
        prov.objective.is_none(),
        "objective should be None for synthetic-centrality scope"
    );
    assert!(
        prov.combination.is_none(),
        "combination should be None for synthetic-centrality scope"
    );
    assert!(
        prov.term_contributions.is_empty(),
        "term_contributions should be empty for synthetic-centrality scope"
    );
    assert!(
        prov.synthetic_centrality,
        "synthetic_centrality should be true for CentredBar"
    );
}
