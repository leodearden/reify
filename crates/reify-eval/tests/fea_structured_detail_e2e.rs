//! B12 e2e emission test for FEA structured diagnostic payloads (task 4802, step-3 RED).
//!
//! Strategy (per plan §B12):
//!   - Reuses the fea_no_supports.ri fixture (existing; proven to yield a
//!     Completed-with-FeaUnderConstrained-Warning in fea_diagnostics_e2e.rs).
//!   - Asserts that eval_result.structured_detail == [Fea(Unconstrained{6 modes})]
//!     and that check_result.structured_detail carries the same payload.
//!
//! RED at step-3: elastic_static.rs does not yet populate structured_detail
//! (the accumulator is declared but the :416 emission site is not wired).
//! GREEN after step-4 wires both emission sites.

use reify_eval::{
    StructuredComputeDetail,
    compute_targets::fea_diagnostics::{DofDirection, FeaDiagnosticDetail},
};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Unconstrained-body solve (no supports) → Completed with FeaUnderConstrained warning.
///
/// eval_result.structured_detail must carry exactly one payload:
///   Fea(FeaDiagnosticDetail::Unconstrained { rigid_body_modes: all 6 })
///
/// check_result.structured_detail must carry the same payload
/// (proves the EvalResult → CheckResult propagation — the R3b-2 read point).
#[test]
fn fea_unconstrained_eval_and_check_carry_structured_detail() {
    let source = include_str!("fixtures/fea_no_supports.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let expected_detail = vec![StructuredComputeDetail::Fea(
        FeaDiagnosticDetail::Unconstrained {
            rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
        },
    )];

    // (1) eval_result.structured_detail carries the Unconstrained payload.
    let eval_result = engine.eval(&compiled);
    assert_eq!(
        eval_result.structured_detail,
        expected_detail,
        "eval_result.structured_detail must carry [Fea(Unconstrained{{6 modes}})];\
         got: {:#?}",
        eval_result.structured_detail
    );

    // (2) check_result.structured_detail carries the same payload (EvalResult→CheckResult handoff).
    let check_result = engine.check(&compiled);
    assert_eq!(
        check_result.structured_detail,
        expected_detail,
        "check_result.structured_detail must carry [Fea(Unconstrained{{6 modes}})];\
         got: {:#?}",
        check_result.structured_detail
    );
}
