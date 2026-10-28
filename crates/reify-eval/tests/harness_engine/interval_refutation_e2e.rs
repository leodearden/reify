//! BT5 end to end (task #6655, solution-set-completeness PRD §3.3): a model
//! the interval pre-check can refute is reported as a PROOF, naming the
//! narrowing constraint, rather than as a search failure.
//!
//! `tests/prd-gate/fixtures/ssc_refuted_pair.ri` pins `L == 10mm` and
//! `L == 20mm` on one auto. It is evaluated through the production registry,
//! as `underdetermined_support::eval_through_production_registry` wires it,
//! but without that helper's no-errors assertion: the error IS the signal.

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

use super::underdetermined_support::workspace_root;

#[test]
fn ssc_refuted_pair_is_refuted_naming_the_second_constraint() {
    let path = workspace_root().join("tests/prd-gate/fixtures/ssc_refuted_pair.ri");
    let src = std::fs::read_to_string(&path).expect("read ssc_refuted_pair.ri");
    let compiled = compile_source_with_stdlib(&src);
    assert!(
        collect_errors(&compiled.diagnostics).is_empty(),
        "the fixture must compile cleanly; got {:#?}",
        compiled.diagnostics
    );
    let mut engine = Engine::new(
        reify_eval::SolverPosture::Solve(reify_ir::ResolutionProfile::FRESH),
        Box::new(MockConstraintChecker::new()),
        None,
    );
    let result = engine.eval(&compiled);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let refuted: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintRefuted))
        .collect();
    eprintln!("MEASURE refuted={refuted:#?} errors={errors:#?}");
    assert_eq!(refuted.len(), 1, "exactly one refutation; got {errors:#?}");
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("could not be satisfied")),
        "a proof must not also render as a search failure (C3); got {errors:#?}"
    );
    let length = result
        .values
        .get(&ValueCellId::new("SscRefutedPair.rod", "length"));
    assert!(
        matches!(length, None | Some(reify_ir::Value::Undef)),
        "a refuted auto stays unresolved; got {length:?}"
    );
}
