//! Pins that the eval seam at `crates/reify-expr/src/lib.rs` (the `_ =>` builtin
//! fallthrough arm) emits a `Severity::Error` diagnostic into the `EvalContext`
//! diagnostics sink when a stackup builtin call returns `Value::Undef`.
//!
//! Tests are RED until step-6 wires the `stackup_diagnose` call at lib.rs:495.

#![allow(clippy::mutable_key_type)]

use std::cell::RefCell;

use reify_core::{ContentHash, DiagnosticCode, Severity, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value, ValueMap};
use reify_expr::{EvalContext, eval_expr};

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
        result_type: Type::dimensionless_scalar(),
        content_hash: hash,
    }
}

/// Wrap a `Value` in a literal `CompiledExpr`.
fn lit(v: Value) -> CompiledExpr {
    CompiledExpr::literal(v, Type::dimensionless_scalar())
}

/// Build a single valid contributor Map value (LENGTH scalar nominal/tols, sign=+1).
fn make_contributor() -> Value {
    use std::collections::BTreeMap;
    use reify_core::DimensionVector;

    let len = |si: f64| Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH };
    let mut m: BTreeMap<Value, Value> = BTreeMap::new();
    m.insert(Value::String("nominal".into()),   len(0.010));
    m.insert(Value::String("plus_tol".into()),  len(0.0001));
    m.insert(Value::String("minus_tol".into()), len(0.0001));
    m.insert(Value::String("sign".into()),      Value::Int(1));
    m.insert(
        Value::String("distribution".into()),
        Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() },
    );
    Value::Map(m)
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Case 1: `stackup_rss([])` → Value::Undef AND exactly one Error diagnostic
/// with `DiagnosticCode::StackupEmptyChain` is pushed into the sink.
///
/// RED until step-6 wires the stackup_diagnose call at lib.rs:495.
#[test]
fn stackup_rss_empty_chain_emits_diagnostic() {
    let expr = make_fn_call("stackup_rss", vec![lit(Value::List(vec![]))]);

    let values = ValueMap::new();
    let sink = RefCell::new(Vec::new());
    let ctx = EvalContext::new(&values, &[]).with_runtime_diagnostics(&sink);

    let result = eval_expr(&expr, &ctx);

    assert!(result.is_undef(), "expected Undef for empty chain, got {:?}", result);

    let diags = sink.borrow();
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic for empty chain, got {}; diags={:?}",
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
        Some(DiagnosticCode::StackupEmptyChain),
        "expected StackupEmptyChain code, got {:?}",
        diags[0].code
    );
    assert!(
        diags[0].message.contains("E_StackupEmptyChain"),
        "message must contain 'E_StackupEmptyChain'; got: {:?}",
        diags[0].message
    );
}

/// Case 2: `stackup_rss([valid_contributor])` → NOT Undef AND zero diagnostics.
///
/// This confirms the guard: valid calls must not push spurious diagnostics.
/// Also RED until step-6 (eval returns Undef without the seam fix, but at least
/// the zero-diagnostic assertion would wrongly fire).
#[test]
fn stackup_rss_valid_chain_emits_no_diagnostic() {
    let chain = Value::List(vec![make_contributor()]);
    let expr = make_fn_call("stackup_rss", vec![lit(chain)]);

    let values = ValueMap::new();
    let sink = RefCell::new(Vec::new());
    let ctx = EvalContext::new(&values, &[]).with_runtime_diagnostics(&sink);

    let result = eval_expr(&expr, &ctx);

    // Valid call must return a Map (not Undef)
    assert!(
        matches!(result, Value::Map(_)),
        "expected Map for valid chain, got {:?}",
        result
    );

    let diags = sink.borrow();
    assert_eq!(
        diags.len(),
        0,
        "valid call must push zero diagnostics, got {}; diags={:?}",
        diags.len(),
        *diags
    );
}

/// Case 3: `stackup_rss([])` without a diagnostics sink → still returns Undef,
/// no panic (silently drops the error — the None-sink path is unchanged).
#[test]
fn stackup_rss_empty_chain_no_sink_returns_undef_silently() {
    let expr = make_fn_call("stackup_rss", vec![lit(Value::List(vec![]))]);

    let values = ValueMap::new();
    // EvalContext::simple has diagnostics: None
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert!(result.is_undef(), "expected Undef even without sink, got {:?}", result);
}
