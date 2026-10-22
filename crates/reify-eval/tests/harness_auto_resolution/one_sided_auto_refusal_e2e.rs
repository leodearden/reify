//! Eval-level e2e for the one-sided strict-auto refusal (task #6692, P1-ε) —
//! the "no driver reports 10 m" half of PRD
//! `docs/prds/v0_6/solver-driver-parity.md` §5 B4, plus B5 and the F2 twin.
//!
//! The engine is wired with `SolverRegistry::production()` (PRD I2), so this
//! exercises the solver set every driver installs rather than a bare solver.
//! Harness otherwise mirrors
//! `cost_robustness_tradeoff_underdetermined_diagnostic.rs`.

use reify_constraints::SolverRegistry;
use reify_core::{Diagnostic, DiagnosticCode, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, compile_source_with_stdlib};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../tests/prd-gate/fixtures/{name}.ri",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {path}: {e}"))
}

fn eval_with_production_registry(compiled: &reify_compiler::CompiledModule) -> EvalResult {
    let mut engine = Engine::new(
        reify_eval::SolverPosture::Solve(reify_ir::ResolutionProfile::FRESH),
        Box::new(MockConstraintChecker::new()),
        None,
    )
    .with_solver(Box::new(SolverRegistry::production()));
    engine.eval(compiled)
}

fn non_unique(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique))
        .collect()
}

fn rod_length(result: &EvalResult, entity: &str) -> Option<Value> {
    result
        .values
        .get(&ValueCellId::new(format!("{entity}.rod"), "length"))
        .cloned()
}

/// B4: the static arm refuses the fixture at compile time, and an eval run
/// anyway (the GUI and LSP evaluate modules that carry compile errors) never
/// reports the 10 m corner: the value is Undef and the runtime arm names the
/// missing bound.
#[test]
fn b4_one_sided_strict_auto_never_resolves_to_the_default_edge() {
    const CLAUSE: &str = "no constraint bounds `OneSided.rod.length` above";
    let compiled = compile_source_with_stdlib(&fixture("driver_parity_one_sided_auto"));

    let compile_refusals = non_unique(&compiled.diagnostics);
    assert!(
        compile_refusals.iter().any(|d| d.message.contains(CLAUSE)),
        "B4: the compile diagnostics must carry the static ConstraintNonUnique naming \
         {CLAUSE:?}; got {:#?}",
        compiled.diagnostics
    );

    let result = eval_with_production_registry(&compiled);
    let value = rod_length(&result, "OneSided");
    assert!(
        matches!(value, Some(Value::Undef)),
        "B4: OneSided.rod.length must be Undef, never the 10 m default edge; got {value:?}"
    );
    assert!(
        non_unique(&result.diagnostics)
            .iter()
            .any(|d| d.message.contains(CLAUSE)),
        "B4: eval must carry the runtime ConstraintNonUnique naming {CLAUSE:?}; got {:#?}",
        result.diagnostics
    );
}

/// B5: two-sided bounds resolve to the analytic centre with no refusal at
/// either layer.
#[test]
fn b5_two_sided_strict_auto_resolves_to_the_centre() {
    let compiled = compile_source_with_stdlib(&fixture("driver_parity_two_sided_auto"));
    let result = eval_with_production_registry(&compiled);
    assert!(
        non_unique(&compiled.diagnostics).is_empty() && non_unique(&result.diagnostics).is_empty(),
        "B5: no ConstraintNonUnique anywhere; compile {:#?}, eval {:#?}",
        compiled.diagnostics,
        result.diagnostics
    );
    let value = rod_length(&result, "TwoSided").and_then(|v| v.as_f64());
    assert!(
        value.is_some_and(|v| (v - 0.010).abs() < 1e-9),
        "B5: TwoSided.rod.length must be the centre 0.010 m; got {value:?}"
    );
}

/// F2: a one-sided auto(free) is never refused. Its value rule belongs to
/// #6709, so only finiteness is pinned here, plus the existing free warning.
#[test]
fn f2_one_sided_auto_free_is_not_refused() {
    let compiled = compile_source_with_stdlib(&fixture("driver_parity_one_sided_auto_free"));
    let result = eval_with_production_registry(&compiled);
    assert!(
        non_unique(&compiled.diagnostics).is_empty() && non_unique(&result.diagnostics).is_empty(),
        "F2: no ConstraintNonUnique anywhere; compile {:#?}, eval {:#?}",
        compiled.diagnostics,
        result.diagnostics
    );
    let value = rod_length(&result, "OneSidedFree");
    assert!(
        matches!(value, Some(Value::Scalar { si_value, .. }) if si_value.is_finite()),
        "F2: OneSidedFree.rod.length must be a finite Scalar; got {value:?}"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("resolved via auto(free)")),
        "F2: the existing auto(free) warning must still be present; got {:#?}",
        result.diagnostics
    );
}
