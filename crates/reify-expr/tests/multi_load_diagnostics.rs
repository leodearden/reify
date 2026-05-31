//! Pins that the eval seam at `crates/reify-expr/src/lib.rs` (the `eval_builtin`
//! `Undef`-site wiring) emits a `Severity::Error` diagnostic into the
//! `EvalContext` diagnostics sink when a `linear_combine` call returns
//! `Value::Undef` for a task-#10 multi-load-case failure mode.
//!
//! Mirrors `stackup_diagnostic_emission.rs`. Empty-weights is used as the
//! wiring trigger; the per-mode message/code logic for all three diagnosed
//! `linear_combine` modes is covered exhaustively by the `fea.rs` unit tests
//! (`diagnose_linear_combine_*`), so exercising a single mode end-to-end here
//! is sufficient to prove the engine invokes `fea_diagnose` and pushes its
//! result — it is intentional, not a silent cap.
//!
//! RED until step-12 wires the `fea_diagnose` call at lib.rs:502-513.

#![allow(clippy::mutable_key_type)]

use std::cell::RefCell;
use std::collections::BTreeMap;

use reify_core::{ContentHash, DiagnosticCode, Severity, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value, ValueMap};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a FunctionCall `CompiledExpr` that calls `name` with the given `args`.
fn make_fn_call(name: &str, args: Vec<CompiledExpr>) -> CompiledExpr {
    let hash = ContentHash::of(name.as_bytes());
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{name}"),
            },
            args,
        },
        result_type: Type::Real,
        content_hash: hash,
    }
}

/// Wrap a `Value` in a literal `CompiledExpr`.
fn lit(v: Value) -> CompiledExpr {
    CompiledExpr::literal(v, Type::Real)
}

/// Build a minimal `MultiCaseResult` (`Value::Map { "cases" -> Value::Map }`)
/// holding a single case. The per-case value is irrelevant to the empty-weights
/// path — `fea::diagnose` reports `MultiLoadEmptyWeights` before it inspects any
/// per-case fields — so a degenerate empty ElasticResult Map suffices while
/// still satisfying `extract_cases_map`'s `cases -> Value::Map` shape contract.
fn make_multi_case_result_one_case() -> Value {
    let mut cases: BTreeMap<Value, Value> = BTreeMap::new();
    cases.insert(Value::String("operating".into()), Value::Map(BTreeMap::new()));
    let mut outer: BTreeMap<Value, Value> = BTreeMap::new();
    outer.insert(Value::String("cases".into()), Value::Map(cases));
    Value::Map(outer)
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// `linear_combine(mcr, {})` → `Value::Undef` AND exactly one `Error` diagnostic
/// with `DiagnosticCode::MultiLoadEmptyWeights` pushed into the sink. This proves
/// the engine invokes `fea_diagnose` on an `Undef` result and pushes it.
///
/// RED until step-12 wires the `fea_diagnose` call at lib.rs:502-513 (today only
/// `stackup_diagnose` runs there, and it returns `None` for `linear_combine`).
#[test]
fn linear_combine_empty_weights_emits_diagnostic() {
    let mcr = make_multi_case_result_one_case();
    let empty_weights = Value::Map(BTreeMap::new());
    let expr = make_fn_call("linear_combine", vec![lit(mcr), lit(empty_weights)]);

    let values = ValueMap::new();
    let sink = RefCell::new(Vec::new());
    let ctx = EvalContext::new(&values, &[]).with_runtime_diagnostics(&sink);

    let result = eval_expr(&expr, &ctx);

    assert!(
        result.is_undef(),
        "expected Undef for empty weights, got {:?}",
        result
    );

    let diags = sink.borrow();
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic for empty weights, got {}; diags={:?}",
        diags.len(),
        *diags
    );
    assert_eq!(
        diags[0].severity,
        Severity::Error,
        "diagnostic must be Error severity, got {:?}",
        diags[0].severity
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::MultiLoadEmptyWeights),
        "expected MultiLoadEmptyWeights code, got {:?}",
        diags[0].code
    );
}
