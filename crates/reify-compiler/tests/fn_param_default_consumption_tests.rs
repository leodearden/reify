//! Compiler-level tests for fn_param default consumption (task 3688, step-3).
//!
//! Tests E and F pin the call-site behavior at the compilation layer:
//! Test E — defaulted call compiles without errors and emits UserFunctionCall.
//! Test F — param without a default still produces the unchanged NoMatch error.

use reify_types::{CompiledExprKind, ModulePath, Severity};

/// Test E: a call that omits all defaulted params compiles without errors
/// and the resulting expression is a UserFunctionCall with the full arg count.
///
/// `fn f(x : Real = 1.0) -> Real { x }`
/// `structure S { let v = f() }`
///
/// Expects: no Error-severity diagnostics; `v` cell holds a `UserFunctionCall`
/// with 1 arg (the padded default).
#[test]
fn fn_param_default_defaulted_call_no_error() {
    let source = r#"
fn f(x : Real = 1.0) -> Real { x }

structure S {
    let v = f()
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let 'v' should have an expression");

    match &v_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, args } => {
            assert_eq!(function_name, "f");
            assert_eq!(
                args.len(),
                1,
                "padded call should carry 1 arg (the compiled default)"
            );
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(
        v_expr.result_type,
        reify_types::Type::Real,
        "f() -> Real"
    );
}

/// Test F: a call to a function whose param has NO default still produces
/// the unchanged "no matching overload" error — default-padding must not
/// over-pad params lacking a default.
///
/// `fn h(x : Real) -> Real { x }`
/// `structure S { let v = h() }`
///
/// Expects: exactly one Error-severity diagnostic containing "no matching overload".
#[test]
fn fn_param_no_default_still_errors() {
    let source = r#"
fn h(x : Real) -> Real { x }

structure S {
    let v = h()
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_f"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error (h() missing required arg), got: {:?}",
        errors
    );
    assert!(
        errors[0].message.contains("no matching overload"),
        "error should mention 'no matching overload', got: {:?}",
        errors[0].message
    );
}
