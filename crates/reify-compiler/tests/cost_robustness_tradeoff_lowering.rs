//! γ-lowering tests for `minimize cost_robustness_tradeoff(cost_expr, λ)`
//! (PRD `docs/prds/v0_6/continuous-cost-minimisation.md` §2.4/§8.1, task γ #4791).
//!
//! Mirrors the harness in `objective_set_lowering.rs` (β). Asserts that the
//! special form is recognized in the `MemberDecl::Minimize` arm and lowers to
//! an `ObjectiveSet` with `cost_robustness_lambda` threaded from the literal
//! second argument, and that the arg-diagnostic checks (non-Money cost arg,
//! non-literal/out-of-range λ, wrong arity) fire without panicking.

use reify_core::{DiagnosticCode, DimensionVector, ModulePath, Severity, Type};
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

/// Like `compile_ok`, but does NOT assert on diagnostics — used by the
/// negative arg-diagnostic tests below, which expect specific error
/// diagnostics rather than a clean compile. Still asserts the source itself
/// parses (a parse error would signal a malformed test fixture, not the
/// arg-diagnostic behaviour under test).
fn compile_with_diagnostics(src: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(module_name));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
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

// ── arg diagnostics ──────────────────────────────────────────────────────────
//
// PRD §8.2 "Bad override arg" row: `cost_robustness_tradeoff(<non-money>, 2.0)`
// must emit named compile diagnostic(s) — non-Money arg AND λ∉[0,1] — with no
// panic. The two checks (Check 1: arg0 must be Money; Check 2: λ must be a
// compile-time literal in [0,1]) are independent and co-emittable.

/// A non-Money first argument AND an out-of-range λ co-emit BOTH
/// `CostTradeoffNonMoneyArg` and `CostTradeoffInvalidLambda` — the checks are
/// independent, not short-circuiting on the first failure.
#[test]
fn cost_robustness_tradeoff_non_money_and_invalid_lambda_co_emit() {
    let module = compile_with_diagnostics(
        r#"structure S {
    param t: Length = auto
    constraint t > 1mm
    minimize cost_robustness_tradeoff(t * 1.0, 2.0)
}"#,
        "test_tradeoff_non_money_bad_lambda",
    );

    let has_non_money = module.diagnostics.iter().any(|d| {
        d.code == Some(DiagnosticCode::CostTradeoffNonMoneyArg)
            && d.severity == Severity::Error
            && d.message.contains("E_COST_TRADEOFF_NON_MONEY")
    });
    assert!(
        has_non_money,
        "expected a CostTradeoffNonMoneyArg diagnostic with the E_COST_TRADEOFF_NON_MONEY \
         mnemonic, got {:?}",
        module.diagnostics
    );

    let has_invalid_lambda = module.diagnostics.iter().any(|d| {
        d.code == Some(DiagnosticCode::CostTradeoffInvalidLambda)
            && d.severity == Severity::Error
            && d.message.contains("E_COST_TRADEOFF_INVALID_LAMBDA")
    });
    assert!(
        has_invalid_lambda,
        "expected a CostTradeoffInvalidLambda diagnostic with the \
         E_COST_TRADEOFF_INVALID_LAMBDA mnemonic, got {:?}",
        module.diagnostics
    );
}

/// A non-literal λ (e.g. a param-valued expression) is rejected — v1 requires
/// λ to be compile-time-known for the two-anchor solve (PRD §9 Q4).
#[test]
fn cost_robustness_tradeoff_non_literal_lambda_is_invalid() {
    let module = compile_with_diagnostics(
        r#"pub unit USD : Money
structure S {
    param t: Length = auto
    param c: Money = 5USD
    constraint t > 1mm
    minimize cost_robustness_tradeoff(c * (t / 1mm), t / 1mm)
}"#,
        "test_tradeoff_non_literal_lambda",
    );

    let has_invalid_lambda = module.diagnostics.iter().any(|d| {
        d.code == Some(DiagnosticCode::CostTradeoffInvalidLambda) && d.severity == Severity::Error
    });
    assert!(
        has_invalid_lambda,
        "expected a CostTradeoffInvalidLambda diagnostic for a non-literal λ, got {:?}",
        module.diagnostics
    );
}

/// Wrong arity (≠2 args) must not silently lower as an unresolved call
/// (`undef`) — it emits a clear diagnostic naming `cost_robustness_tradeoff`
/// and the argument-count mismatch.
#[test]
fn cost_robustness_tradeoff_wrong_arity_emits_diagnostic() {
    let module = compile_with_diagnostics(
        r#"pub unit USD : Money
structure S {
    param t: Length = auto
    param c: Money = 5USD
    constraint t > 1mm
    minimize cost_robustness_tradeoff(c)
}"#,
        "test_tradeoff_wrong_arity",
    );

    let has_arity_diag = module.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && d.message.contains("cost_robustness_tradeoff")
            && d.message.to_lowercase().contains("argument")
    });
    assert!(
        has_arity_diag,
        "expected a clear arity diagnostic naming cost_robustness_tradeoff, got {:?}",
        module.diagnostics
    );
}

/// The full malformed-arg matrix (non-Money + out-of-range λ, non-literal λ,
/// wrong arity in both directions) must never panic — the compiler always
/// degrades to diagnostics, never a crash.
#[test]
fn cost_robustness_tradeoff_malformed_args_never_panic() {
    let cases = [
        r#"structure S {
    param t: Length = auto
    constraint t > 1mm
    minimize cost_robustness_tradeoff(t * 1.0, 2.0)
}"#,
        r#"pub unit USD : Money
structure S {
    param t: Length = auto
    param c: Money = 5USD
    constraint t > 1mm
    minimize cost_robustness_tradeoff(c * (t / 1mm), t / 1mm)
}"#,
        r#"pub unit USD : Money
structure S {
    param t: Length = auto
    param c: Money = 5USD
    constraint t > 1mm
    minimize cost_robustness_tradeoff(c)
}"#,
        r#"pub unit USD : Money
structure S {
    param t: Length = auto
    param c: Money = 5USD
    constraint t > 1mm
    minimize cost_robustness_tradeoff(c, 0.1, 0.2)
}"#,
    ];
    for (i, src) in cases.iter().enumerate() {
        let _ = compile_with_diagnostics(src, &format!("test_tradeoff_no_panic_{i}"));
    }
}
