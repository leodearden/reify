//! Tests for function overload resolution logic (Task 171).
//!
//! These tests verify exact-type matching during resolution, proper error
//! messages with candidate listing, arity disambiguation, and evaluator
//! disambiguation of same-name/same-arity/different-type overloads.

/// step-3: Define only fn f(x: Real)->Real, call f(3) where 3 is Int.
/// Assert produces a "no matching overload" error that lists the candidate.
/// Verifies: Int→Real widening is NOT used during resolution, and zero-match
/// errors list the available candidates with full signatures.
#[test]
fn no_match_error_lists_candidates_when_int_arg_misses_real_param() {
    let source = r#"
fn f(x: Real) -> Real { x }
structure S { let v = f(3) }
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_no_match"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // Should produce exactly one error
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "expected exactly 1 error, got: {:?}", errors);

    let msg = &errors[0].message;
    assert!(
        msg.contains("no matching overload"),
        "error should say 'no matching overload', got: {:?}",
        msg
    );
    // Candidate should be listed with full signature
    assert!(
        msg.contains("f(Real) -> Real"),
        "error should list candidate 'f(Real) -> Real', got: {:?}",
        msg
    );
}

/// step-1: Define fn f(x: Int)->Int and fn f(x: Real)->Real, call f(3)
/// where 3 is Int. Assert resolves to UserFunctionCall with return type Int.
///
/// Fails with current code: type_compatible matches BOTH overloads (Int→Real
/// widening), producing an ambiguity error instead of resolving to the Int overload.
#[test]
fn overload_int_vs_real_selects_int_for_int_arg() {
    let source = r#"
fn f(x: Int) -> Int { x }
fn f(x: Real) -> Real { x }
structure S { let v = f(3) }
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_overload_int_real"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // Should compile without any errors
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors, got: {:?}",
        errors
    );

    // Find the 'v' value cell
    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");

    // Should be UserFunctionCall with return type Int (not Real, not error)
    match &v_expr.kind {
        reify_types::CompiledExprKind::UserFunctionCall { function_name, args } => {
            assert_eq!(function_name, "f");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(
        v_expr.result_type,
        reify_types::Type::Int,
        "f(3) should select the Int overload, returning Int"
    );
}
