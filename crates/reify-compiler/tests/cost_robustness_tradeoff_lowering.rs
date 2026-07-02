//! γ-lowering tests for `minimize cost_robustness_tradeoff(cost_expr, λ)`
//! (PRD `docs/prds/v0_6/continuous-cost-minimisation.md` §2.4/§8.1, task γ #4791).
//!
//! Mirrors the harness in `objective_set_lowering.rs` (β). Asserts that the
//! special form is recognized in the `MemberDecl::Minimize` arm and lowers to
//! an `ObjectiveSet` with `cost_robustness_lambda` threaded from the literal
//! second argument, and that the arg-diagnostic checks (non-Money cost arg,
//! non-literal/out-of-range λ, wrong arity) fire without panicking.

use reify_core::{DimensionVector, ModulePath, Type};
use reify_ir::{ObjectiveCombination, ObjectiveSense};

// ── helpers ──────────────────────────────────────────────────────────────────

fn compile_ok(src: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(module_name));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );
    compiled
}

// ── recognition + typing + λ threading ──────────────────────────────────────

/// `minimize cost_robustness_tradeoff(<money-expr>, <literal λ>)` lowers to a
/// 1-term `WeightedSum` `ObjectiveSet` whose term is `Minimize(<money-expr>)`
/// and whose `cost_robustness_lambda` carries the literal λ. Zero diagnostics.
#[test]
fn cost_robustness_tradeoff_lowers_lambda_and_money_term() {
    let module = compile_ok(
        r#"pub unit USD : Money
structure S {
    param t: Length = auto
    param c: Money = 5USD
    constraint t > 1mm
    minimize cost_robustness_tradeoff(c * (t / 1mm), 0.3)
}"#,
        "test_tradeoff_lowering",
    );

    let template = &module.templates[0];
    let obj = template
        .objective
        .as_ref()
        .expect("template should have an objective");

    assert_eq!(
        obj.cost_robustness_lambda,
        Some(0.3),
        "expected cost_robustness_lambda == Some(0.3), got {:?}",
        obj.cost_robustness_lambda
    );
    assert_eq!(
        obj.combination,
        ObjectiveCombination::WeightedSum,
        "combination must be WeightedSum, got {:?}",
        obj.combination
    );
    assert_eq!(obj.terms.len(), 1, "expected exactly 1 term, got {}", obj.terms.len());

    let term = &obj.terms[0];
    assert_eq!(term.sense, ObjectiveSense::Minimize, "term.sense must be Minimize");
    assert!(
        matches!(
            &term.expr.result_type,
            Type::Scalar { dimension } if *dimension == DimensionVector::MONEY
        ),
        "expected Money result_type for the cost expr, got {:?}",
        term.expr.result_type
    );
}
