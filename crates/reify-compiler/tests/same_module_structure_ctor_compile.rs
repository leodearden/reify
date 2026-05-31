//! Integration test: a fn body can construct a structure_def from the SAME module.
//!
//! This file is the acceptance gate for task 3895: after the same-module skeleton
//! pre-pass lands in `phase_functions`, `Widget()` inside `make_widget()` must lower
//! to `CompiledExprKind::StructureInstanceCtor` (not a generic `UserFunctionCall`),
//! and eval must produce a `Value::StructureInstance` with the structure_def defaults.
//!
//! Today (step-1, RED) this fails: `Widget()` in a same-module fn body is not in the
//! prelude_template_registry, so it lowers to a `UserFunctionCall` with no matching
//! overload → poison diagnostic + error.  Step-2 (GREEN) adds the skeleton pre-pass.

use reify_ir::*;
use reify_compiler::*;
use reify_core::*;

// ─── source fixture ───────────────────────────────────────────────────────────

/// Single ad-hoc module containing both a structure_def and pub fns that
/// construct it — the minimal same-module ctor test case.
const WIDGET_SRC: &str = r#"
module test.widget

structure def Widget {
    param width : Real = 3.5
    param tag   : Bool = true
}

pub fn make_widget() -> Widget { Widget() }
pub fn make_widget_partial() -> Widget { Widget(5.5) }
"#;

// ─── helper ───────────────────────────────────────────────────────────────────

fn load_widget_module() -> reify_compiler::CompiledModule {
    reify_test_support::helpers::compile_source_with_stdlib(WIDGET_SRC)
}

// ─── step-1: zero errors ──────────────────────────────────────────────────────

/// The module must compile with zero `Severity::Error` diagnostics once the
/// same-module skeleton pre-pass (step-2) is in place.  RED today: `Widget()` in
/// a same-module fn body is not in the prelude_template_registry, so it lowers
/// to a `UserFunctionCall` with no matching overload → error diagnostic.
#[test]
fn same_module_widget_zero_errors() {
    let module = load_widget_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "same-module Widget module should compile with zero errors; \
         got: {:#?}",
        errors
    );
}

// ─── step-2: make_widget body is StructureInstanceCtor ────────────────────────

/// `make_widget()`'s result_expr must lower to
/// `CompiledExprKind::StructureInstanceCtor` with `type_name == "Widget"`.
/// RED today: `Widget()` lowers to `UserFunctionCall` instead.
#[test]
fn make_widget_body_is_structure_instance_ctor() {
    let module = load_widget_module();
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_widget")
        .unwrap_or_else(|| {
            panic!(
                "expected `make_widget` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "Widget",
                "make_widget body StructureInstanceCtor.type_name should be \
                 \"Widget\"; got: {}",
                type_name
            );
        }
        other => panic!(
            "make_widget body result_expr.kind should be \
             CompiledExprKind::StructureInstanceCtor; got: {:?}",
            other
        ),
    }
}

// ─── step-3: eval make_widget() → Value::StructureInstance with defaults ──────

/// Evaluating `make_widget()` must yield `Value::StructureInstance` with
/// `type_name == "Widget"`, `width == Value::Real(3.5)`, `tag == Value::Bool(true)`.
#[test]
fn eval_make_widget_returns_struct_with_defaults() {
    let module = load_widget_module();

    let call_expr = CompiledExpr::user_function_call(
        "make_widget".to_string(),
        vec![],
        Type::StructureRef("Widget".to_string()),
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    let data = match &result {
        Value::StructureInstance(data) => data,
        other => panic!(
            "make_widget() should return Value::StructureInstance; got: {:?}",
            other
        ),
    };

    assert_eq!(
        data.type_name, "Widget",
        "make_widget() StructureInstance.type_name should be \"Widget\"; got: {}",
        data.type_name
    );

    assert_eq!(
        data.fields.len(),
        2,
        "make_widget() StructureInstance.fields should have 2 entries; got: {:?}",
        data.fields.keys().collect::<Vec<_>>()
    );

    // width default = 3.5
    let width = data
        .fields
        .get(&"width".to_string())
        .expect("make_widget().width missing");
    assert_eq!(
        *width,
        Value::Real(3.5),
        "make_widget().width should be Real(3.5) (structure_def default); got: {:?}",
        width
    );

    // tag default = true
    let tag = data
        .fields
        .get(&"tag".to_string())
        .expect("make_widget().tag missing");
    assert_eq!(
        *tag,
        Value::Bool(true),
        "make_widget().tag should be Bool(true) (structure_def default); got: {:?}",
        tag
    );
}

// ─── step-4: eval make_widget_partial() → covered positional arg + default ───

/// `make_widget_partial()` passes `5.5` as the first positional arg (`width`).
/// Eval must yield `width == Value::Real(5.5)` (covered) and
/// `tag == Value::Bool(true)` (default).
#[test]
fn eval_make_widget_partial_covers_first_param() {
    let module = load_widget_module();

    let call_expr = CompiledExpr::user_function_call(
        "make_widget_partial".to_string(),
        vec![],
        Type::StructureRef("Widget".to_string()),
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    let data = match &result {
        Value::StructureInstance(data) => data,
        other => panic!(
            "make_widget_partial() should return Value::StructureInstance; got: {:?}",
            other
        ),
    };

    // width covered by positional arg 5.5
    let width = data
        .fields
        .get(&"width".to_string())
        .expect("make_widget_partial().width missing");
    assert_eq!(
        *width,
        Value::Real(5.5),
        "make_widget_partial().width should be Real(5.5) (positional arg); got: {:?}",
        width
    );

    // tag still defaults to true
    let tag = data
        .fields
        .get(&"tag".to_string())
        .expect("make_widget_partial().tag missing");
    assert_eq!(
        *tag,
        Value::Bool(true),
        "make_widget_partial().tag should be Bool(true) (default); got: {:?}",
        tag
    );
}
