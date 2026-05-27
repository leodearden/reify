//! Compiler-level tests for fn_param default consumption (task 3688, step-3).
//!
//! Tests E and F pin the call-site behavior at the compilation layer:
//! Test E — defaulted call compiles without errors and emits UserFunctionCall.
//! Test F — param without a default still produces the unchanged NoMatch error.

use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_ir::{CompiledExprKind, Value};

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
            // Suggestion 2: pin the content of the padded default, not just its presence.
            // A bug that inserted the wrong default (e.g., from a different candidate or
            // a zero literal) would still pass an args.len() check alone.
            match &args[0].kind {
                CompiledExprKind::Literal(Value::Real(v)) => assert!(
                    (*v - 1.0).abs() < f64::EPSILON,
                    "padded default should be 1.0, got {v}"
                ),
                other => panic!(
                    "expected Literal(Value::Real(1.0)) as padded default, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(
        v_expr.result_type,
        reify_core::Type::Real,
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

/// Test G: a default expression that references a sibling param (`fn f(a: Real, b: Real = a)`)
/// produces a diagnostic during compilation — defaults are compiled in a neutral scope
/// (no params registered) so sibling-param references are unresolved.
///
/// The current diagnostic is the generic "unresolved name: a" from compile_expr.
/// A future refinement may emit a more specific message; update this test accordingly.
#[test]
fn fn_param_default_sibling_param_ref_errors() {
    let source = r#"
fn f(a : Real, b : Real = a) -> Real { b }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_g"));
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
    assert!(
        !errors.is_empty(),
        "expected at least one error for sibling-param reference in default expression"
    );
    // Current behavior: generic "unresolved name: a" because the neutral scope has no params.
    assert!(
        errors.iter().any(|e| e.message.contains("unresolved name")),
        "expected 'unresolved name' diagnostic, got: {:?}",
        errors
    );
}

/// Test I (regression for task 3700): a function param with a declared type of `Int`
/// but a default expression that produces `Real` must be caught at definition time.
///
/// Bug (suggestion #7 gap 1 from task 3688): `compile_function` compiled the default
/// expression in a neutral scope but never compared its `result_type` against the
/// resolved param type. So `fn f(x: Int = 1.5) -> Int { x }` compiled silently;
/// the divergence only surfaced at eval time.
///
/// The check must use strict equality (matching `resolve_function_overload` and
/// `try_default_padding`'s prefix-check) — a definition-site check cannot be more
/// permissive than the call-site check that the synthesized default is inserted into.
#[test]
fn fn_param_default_int_param_real_default_type_mismatch_errors() {
    let source = r#"
fn f(x : Int = 1.5) -> Int { x }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_i"));
    // The type mismatch is a compile-time error, not a parse-time error.
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
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::FnParamDefaultTypeMismatch)),
        "expected at least one FnParamDefaultTypeMismatch error, got: {:?}",
        errors
    );
}

/// Test J: a function param declared as `Real` but given an integer-literal default (`1`)
/// must produce a `FnParamDefaultTypeMismatch` error under the strict-equality policy.
///
/// This documents the deliberate divergence from let-binding semantics: `let x: Real = 42`
/// is accepted via Int→Real widening (see `field_codomain_compatible`), but fn-param defaults
/// use strict equality (matching `resolve_function_overload` / `try_default_padding`'s
/// prefix check) because a default is conceptually inserted at the padded call site, and
/// `f(1)` is already rejected for `fn f(x: Real)`.
#[test]
fn fn_param_default_real_param_int_literal_default_type_mismatch_errors() {
    let source = r#"
fn f(x : Real = 1) -> Real { x }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_j"));
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
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::FnParamDefaultTypeMismatch)),
        "expected a FnParamDefaultTypeMismatch error (Int literal default for Real param \
         diverges from let-binding widening — strict policy matches call-site check), \
         got: {:?}",
        errors
    );
}

/// Test K (negative control): a function param declared as `Int` with an integer-literal
/// default (`1`) must NOT produce a `FnParamDefaultTypeMismatch` error — the types match.
///
/// This guards against the check over-firing on correct code.
#[test]
fn fn_param_default_int_param_int_literal_default_no_type_mismatch() {
    let source = r#"
fn f(x : Int = 1) -> Int { x }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_k"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let mismatch_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FnParamDefaultTypeMismatch))
        .collect();
    assert!(
        mismatch_errors.is_empty(),
        "expected no FnParamDefaultTypeMismatch for matching Int param / Int default, \
         got: {:?}",
        mismatch_errors
    );
}

/// Test H (ambiguous-padding regression): when two or more same-name candidates are
/// both satisfiable via default-padding for the same call, `try_default_padding` returns
/// `None` and the caller falls through to the generic "no matching overload" error.
///
/// This documents the current UX gap: the message says "no matching overload" rather
/// than something like "ambiguous default-padding". A future enhancement may route
/// the `satisfiable.len() > 1` case through a dedicated diagnostic mirroring
/// `OverloadResolution::Ambiguous`; when that lands, update or remove this test.
#[test]
fn fn_param_default_ambiguous_padding_pins_no_match_error() {
    // Both overloads are satisfiable from f() with 0 provided args:
    //   f(x:Real=1.0)             — 1 param, all-defaulted → satisfiable
    //   f(x:Real=3.0, y:Real=2.0) — 2 params, all-defaulted → satisfiable
    // Multiple satisfiable → try_default_padding returns None → NoMatch error.
    let source = r#"
fn f(x : Real = 1.0) -> Real { x }
fn f(x : Real = 3.0, y : Real = 2.0) -> Real { x + y }

structure S {
    let v = f()
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_h"));
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
    assert!(
        !errors.is_empty(),
        "expected an error for ambiguous default-padding"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("no matching overload")),
        "expected 'no matching overload' (ambiguous default-padding pins current behavior), got: {:?}",
        errors
    );
}

/// Test L (cascade-suppression regression for task 3718): when a function param's declared
/// type fails to resolve (e.g. `Bogus`), the root-cause "unresolved type" diagnostic must
/// be emitted, but the fn_param default type-check must NOT also fire a spurious
/// `FnParamDefaultTypeMismatch` against the `Type::Real` fallback type.
///
/// Guards the `if !type_ok { continue; }` gate in `compile_function`
/// (crates/reify-compiler/src/functions.rs:80-82). Without the gate, the String default
/// expression's `result_type` would mismatch the `Type::Real` fallback param type and
/// produce a cascading diagnostic on top of the unresolved-type root cause.
#[test]
fn fn_param_default_unresolved_param_type_no_cascade() {
    let source = r#"
fn f(x : Bogus = "hi") -> Int { 0 }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_l"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // (a) Zero FnParamDefaultTypeMismatch errors — the param_type_resolved gate must
    // suppress the cascade. Asserted via DiagnosticCode for narrowness.
    let mismatch_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FnParamDefaultTypeMismatch))
        .collect();
    assert!(
        mismatch_errors.is_empty(),
        "expected NO FnParamDefaultTypeMismatch errors when declared type is unresolved \
         (param_type_resolved gate must suppress cascade), got: {:?}",
        mismatch_errors
    );

    // (b) At least one diagnostic with DiagnosticCode::UnresolvedType must be present —
    // this is the root-cause "unresolved type: Bogus" error from functions.rs:33-36.
    //
    // Asserting on the typed code (rather than severity or a message substring) pins the
    // cascade-suppression invariant to the specific root cause: a future refactor that
    // accidentally suppresses the unresolved-type diagnostic but emits some unrelated error
    // would still satisfy a loose severity check but will fail this typed assertion.
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected at least one DiagnosticCode::UnresolvedType diagnostic (root cause for \
         unresolved 'Bogus' type at functions.rs:33-36), got: {:?}",
        compiled.diagnostics
    );
}

/// Test M (cascade-suppression regression for task 3718): when a function param's default
/// expression fails to compile (e.g. references an undefined name), the root-cause
/// "unresolved name" diagnostic must be emitted with the default poisoned to `Type::Error`,
/// but the fn_param default type-check must NOT also fire a spurious
/// `FnParamDefaultTypeMismatch` on top.
///
/// Guards the `default_ty.is_error()` short-circuit in `fn_param_default_compatible`
/// (crates/reify-compiler/src/type_compat.rs:265-269). Without the short-circuit,
/// `param_ty == Type::Int` compared against `default_ty == Type::Error` would be unequal
/// under the strict-equality policy and cascade an `Int vs Error` mismatch.
#[test]
fn fn_param_default_undefined_default_expr_no_cascade() {
    let source = r#"
fn f(x : Int = undefined_name) -> Int { x }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_consume_m"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // (a) Zero FnParamDefaultTypeMismatch errors — the default_ty.is_error()
    // short-circuit in fn_param_default_compatible must suppress the cascade.
    // Asserted via DiagnosticCode for narrowness.
    let mismatch_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FnParamDefaultTypeMismatch))
        .collect();
    assert!(
        mismatch_errors.is_empty(),
        "expected NO FnParamDefaultTypeMismatch errors when default expression has Type::Error \
         (is_error() short-circuit in fn_param_default_compatible must suppress cascade), \
         got: {:?}",
        mismatch_errors
    );

    // (b) At least one diagnostic with DiagnosticCode::UnresolvedName must be present —
    // this is the root-cause "unresolved name: undefined_name" error from expr.rs:670-682.
    //
    // Asserting on the typed code (rather than severity or a message substring) pins the
    // cascade-suppression invariant to the specific root cause: a future refactor that
    // accidentally suppresses the unresolved-name diagnostic but emits some unrelated error
    // would still satisfy a loose severity check but will fail this typed assertion.
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedName)),
        "expected at least one DiagnosticCode::UnresolvedName diagnostic (root cause for \
         undefined 'undefined_name' at expr.rs:670-682), got: {:?}",
        compiled.diagnostics
    );
}
