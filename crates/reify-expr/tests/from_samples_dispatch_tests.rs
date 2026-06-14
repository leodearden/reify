//! Dispatch tests for the `from_samples` intercepting-builtin (task 4221 γ,
//! PRD docs/prds/v0_6/std-fields-api.md §D3/D5).
//!
//! Boundary tests covered:
//!   B2 — `sample(from_samples([0.0,1.0,2.0],[0.0,10.0,20.0], InterpolationMethod.Linear), 0.5)`
//!        evaluates to `Value::Real(5.0)` (IEEE-754 exact: lerp at fraction 0.5)
//!   B3 — non-uniform spacing → `Value::Undef` + `DiagnosticCode::FieldSamplesNotGrid` error
//!        (added in step-5 after the diagnostic variant lands in step-6)
//!   B4 — unsupported method (RBF) → `Value::Undef` + `DiagnosticCode::InterpMethodUnsupported`
//!        (added in step-7 after the diagnostic variant lands in step-8)
//!
//! Model: `fn_field_dispatch_tests.rs` — same direct-Value construction +
//! `eval_expr(&expr, &EvalContext::simple(&ValueMap::new()))` pattern.
//!
//! B2 is RED before step-4 (the eval_from_samples arm lands):
//!   - `from_samples(...)` falls through to `reify_stdlib::eval_builtin` (no
//!     binding) → `Value::Undef`
//!   - `sample(Undef, 0.5)` → strict Undef propagation → `Value::Undef`

use std::cell::RefCell;

use reify_core::{ContentHash, DiagnosticCode, Severity, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build `Value::List([Value::Real(a), Value::Real(b), Value::Real(c)])`.
fn real_list(a: f64, b: f64, c: f64) -> Value {
    Value::List(vec![Value::Real(a), Value::Real(b), Value::Real(c)])
}

/// Build a `Value::Enum { type_name: "InterpolationMethod", variant: v }`.
fn interp_method(variant: &str) -> Value {
    Value::Enum {
        type_name: "InterpolationMethod".to_string(),
        variant: variant.to_string(),
    }
}

/// Build a `from_samples(points, values, method)` FunctionCall `CompiledExpr`
/// whose `result_type = Field<Real, Real>` (what α stamps).
///
/// Takes pre-built `Value` args — wrapped in `CompiledExpr::literal`.
pub fn make_from_samples_call(points: Value, values: Value, method: Value) -> CompiledExpr {
    let hash = ContentHash::of(b"from_samples_dispatch_test");
    let list_type = Type::List(Box::new(Type::dimensionless_scalar()));
    let enum_type = Type::Enum("InterpolationMethod".to_string());
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "from_samples".to_string(),
                qualified_name: "std::from_samples".to_string(),
            },
            args: vec![
                CompiledExpr::literal(points, list_type.clone()),
                CompiledExpr::literal(values, list_type),
                CompiledExpr::literal(method, enum_type),
            ],
        },
        result_type: Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        },
        content_hash: hash,
    }
}

/// Build a `sample(field_expr, at)` FunctionCall `CompiledExpr` where the
/// field argument is itself a `CompiledExpr` (allowing from_samples to be
/// nested directly rather than pre-evaluated to a literal).
fn make_sample_of_expr(field_expr: CompiledExpr, at: f64) -> CompiledExpr {
    let hash = ContentHash::of(b"sample_of_from_samples_test");
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "sample".to_string(),
                qualified_name: "std::sample".to_string(),
            },
            args: vec![
                field_expr,
                CompiledExpr::literal(Value::Real(at), Type::dimensionless_scalar()),
            ],
        },
        result_type: Type::dimensionless_scalar(),
        content_hash: hash,
    }
}

// ── B2 tests ─────────────────────────────────────────────────────────────────

/// `from_samples([0,1,2],[0,10,20], Linear)` must evaluate to a
/// `Value::Field { source: Sampled, .. }` wrapping a `Value::SampledField`.
///
/// **RED before step-4**: no arm → falls through → `Value::Undef`.
/// **GREEN after step-4**: arm constructs and returns `Value::Field { source: Sampled, .. }`.
#[test]
fn from_samples_evaluates_to_sampled_field() {
    let expr = make_from_samples_call(
        real_list(0.0, 1.0, 2.0),
        real_list(0.0, 10.0, 20.0),
        interp_method("Linear"),
    );

    let result = eval_expr(&expr, &EvalContext::simple(&ValueMap::new()));

    assert!(
        matches!(
            &result,
            Value::Field {
                source: FieldSourceKind::Sampled,
                ..
            }
        ),
        "from_samples(..., Linear) must yield Value::Field {{ source: Sampled, .. }}; got {:?}",
        result
    );
}

/// B2: `sample(from_samples([0,1,2],[0,10,20], Linear), 0.5)` must equal 5.0
/// (exact: lerp between node0=0 and node1=10 at fraction 0.5).
///
/// **RED before step-4**: `from_samples` → Undef → `sample(Undef, 0.5)` → Undef.
/// **GREEN after step-4**: `from_samples` → SampledField → sample → 5.0.
#[test]
fn sample_from_samples_evaluates_to_real_b2() {
    let from_samples_expr = make_from_samples_call(
        real_list(0.0, 1.0, 2.0),
        real_list(0.0, 10.0, 20.0),
        interp_method("Linear"),
    );
    let sample_expr = make_sample_of_expr(from_samples_expr, 0.5);

    let result = eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()));

    assert!(
        matches!(&result, Value::Real(v) if (v - 5.0).abs() < 1e-12),
        "sample(from_samples([0,1,2],[0,10,20],Linear), 0.5) must be 5.0 (B2); got {:?}",
        result
    );
}

// ── B3 tests (non-uniform spacing → FieldSamplesNotGrid) ─────────────────────

/// B3: `from_samples([0,1,5],[0,10,20], Linear)` — non-uniform spacing
/// [1.0, 4.0] — must return `Value::Undef` and push a
/// `DiagnosticCode::FieldSamplesNotGrid` Error into the diagnostics sink.
///
/// **RED before step-6**: `DiagnosticCode::FieldSamplesNotGrid` variant does
/// not exist (E0599 compile error) + eval returns Undef silently (no code pushed).
/// **GREEN after step-6**: variant exists; returns Undef + pushes Error.
#[test]
fn from_samples_non_grid_emits_field_samples_not_grid_b3() {
    let sink: RefCell<Vec<reify_core::Diagnostic>> = RefCell::new(Vec::new());
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

    let expr = make_from_samples_call(
        real_list(0.0, 1.0, 5.0), // spacing [1.0, 4.0] — non-uniform
        real_list(0.0, 10.0, 20.0),
        interp_method("Linear"),
    );

    let result = eval_expr(&expr, &ctx);

    assert_eq!(
        result,
        Value::Undef,
        "from_samples with non-uniform spacing must return Undef (B3); got {:?}",
        result
    );

    let diags = sink.borrow();
    assert!(
        diags
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::FieldSamplesNotGrid)
                && d.severity == Severity::Error),
        "from_samples with non-uniform spacing must push FieldSamplesNotGrid Error (B3); \
         diagnostics: {:?}",
        *diags
    );
}

// ── B4 tests (unsupported method → InterpMethodUnsupported) ──────────────────

/// B4: `from_samples([0,1,2],[0,10,20], InterpolationMethod.RBF)` — valid
/// 1-D regular grid but unsupported RBF method — must return `Value::Undef`
/// and push a `DiagnosticCode::InterpMethodUnsupported` Error.
///
/// **RED before step-8**: `DiagnosticCode::InterpMethodUnsupported` variant
/// does not exist (E0599 compile error) + eval returns Undef silently.
/// **GREEN after step-8**: variant exists; returns Undef + pushes Error.
#[test]
fn from_samples_rbf_method_emits_interp_method_unsupported_b4() {
    let sink: RefCell<Vec<reify_core::Diagnostic>> = RefCell::new(Vec::new());
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

    let expr = make_from_samples_call(
        real_list(0.0, 1.0, 2.0), // valid uniform grid
        real_list(0.0, 10.0, 20.0),
        interp_method("RBF"), // unsupported method
    );

    let result = eval_expr(&expr, &ctx);

    assert_eq!(
        result,
        Value::Undef,
        "from_samples with RBF method must return Undef (B4); got {:?}",
        result
    );

    let diags = sink.borrow();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(DiagnosticCode::InterpMethodUnsupported)
                && d.severity == Severity::Error
        }),
        "from_samples with RBF method must push InterpMethodUnsupported Error (B4); \
         diagnostics: {:?}",
        *diags
    );
}

/// B4 (Kriging): confirm the Kriging variant also emits
/// `DiagnosticCode::InterpMethodUnsupported`.
#[test]
fn from_samples_kriging_method_emits_interp_method_unsupported_b4() {
    let sink: RefCell<Vec<reify_core::Diagnostic>> = RefCell::new(Vec::new());
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

    let expr = make_from_samples_call(
        real_list(0.0, 1.0, 2.0),
        real_list(0.0, 10.0, 20.0),
        interp_method("Kriging"), // unsupported method
    );

    let result = eval_expr(&expr, &ctx);

    assert_eq!(
        result,
        Value::Undef,
        "from_samples with Kriging method must return Undef (B4); got {:?}",
        result
    );

    let diags = sink.borrow();
    assert!(
        diags.iter().any(|d| {
            d.code == Some(DiagnosticCode::InterpMethodUnsupported)
                && d.severity == Severity::Error
        }),
        "from_samples with Kriging method must push InterpMethodUnsupported Error (B4); \
         diagnostics: {:?}",
        *diags
    );
}
