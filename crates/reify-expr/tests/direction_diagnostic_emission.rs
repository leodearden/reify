//! Pins that `eval_expr`'s post-Undef builtin hook drains reify-stdlib's `dir`
//! classifier: a degenerate `dir` emits `E_DegenerateDirection` (keeping only
//! the generic undef cause), a dimensioned one `DimensionedArgRejected` (also
//! recorded as the cell's undef cause), and a valid one nothing.

use std::cell::RefCell;

use reify_core::{ContentHash, Diagnostic, DiagnosticCode, DimensionVector, Severity, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, UndefCause, Value, ValueMap};

fn make_fn_call(name: &str, args: Vec<CompiledExpr>) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{name}"),
            },
            args,
        },
        result_type: Type::Direction,
        content_hash: ContentHash::of(name.as_bytes()),
    }
}

fn lit(v: Value) -> CompiledExpr {
    CompiledExpr::literal(v, Type::dimensionless_scalar())
}

fn dir_call(args: Vec<Value>) -> CompiledExpr {
    make_fn_call("dir", args.into_iter().map(lit).collect())
}

fn mm(x: f64) -> Value {
    Value::Scalar {
        si_value: x * 1e-3,
        dimension: DimensionVector::LENGTH,
    }
}

/// Evaluates `expr` with both sinks attached; returns the value, the emitted
/// diagnostics and the recorded undef causes.
fn eval_with_sinks(expr: &CompiledExpr) -> (Value, Vec<Diagnostic>, Vec<UndefCause>) {
    let values = ValueMap::new();
    let diags = RefCell::new(Vec::new());
    let causes = RefCell::new(Vec::new());
    let ctx = EvalContext::new(&values, &[])
        .with_runtime_diagnostics(&diags)
        .with_undef_cause_sink(&causes);
    let value = eval_expr(expr, &ctx);
    (value, diags.into_inner(), causes.into_inner())
}

fn op_contract_codes(causes: &[UndefCause]) -> Vec<DiagnosticCode> {
    causes
        .iter()
        .filter_map(|c| match c {
            UndefCause::OpContractFailed { code, .. } => Some(*code),
            _ => None,
        })
        .collect()
}

/// Degeneracy is not a dimension fault, so the hook records no specific cause:
/// the cell keeps only the generic `OpContractViolation` every Undef builtin
/// result carries.
#[test]
fn degenerate_dir_emits_e_degenerate_direction_and_keeps_the_generic_cause() {
    let expr = dir_call(vec![Value::Int(0), Value::Int(0), Value::Int(0)]);
    let (value, diags, causes) = eval_with_sinks(&expr);

    assert_eq!(value, Value::Undef);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(diags[0].code, Some(DiagnosticCode::DegenerateDirection));
    assert!(
        diags[0].message.contains("E_DegenerateDirection"),
        "{}",
        diags[0].message
    );
    assert_eq!(
        op_contract_codes(&causes),
        vec![DiagnosticCode::OpContractViolation],
        "a degenerate dir cell must carry only the generic cause; got {causes:?}"
    );
}

#[test]
fn dimensioned_dir_records_dimensioned_arg_rejected() {
    let expr = dir_call(vec![mm(1.0), Value::Int(0), Value::Int(0)]);
    let (value, diags, causes) = eval_with_sinks(&expr);

    assert_eq!(value, Value::Undef);
    assert!(
        diags
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::DimensionedArgRejected)),
        "expected a DimensionedArgRejected diagnostic, got {diags:?}"
    );
    assert_eq!(
        op_contract_codes(&causes).first(),
        Some(&DiagnosticCode::DimensionedArgRejected),
        "the first OpContractFailed cause must be the specific dimension fault; got {causes:?}"
    );
}

#[test]
fn valid_dir_emits_nothing() {
    let expr = dir_call(vec![Value::Int(0), Value::Int(3), Value::Int(0)]);
    let (value, diags, _) = eval_with_sinks(&expr);

    match value {
        Value::Direction(u) => assert_eq!(u.components(), [0.0, 1.0, 0.0]),
        other => panic!("expected a Direction, got {other:?}"),
    }
    assert!(
        diags.is_empty(),
        "a valid dir must emit nothing; got {diags:?}"
    );
}
