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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_no_match"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Should produce exactly one error
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error, got: {:?}",
        errors
    );

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

/// step-5: Define fn f(x: Int)->Int and fn f(x: Int, y: Int)->Int, call f(5).
/// Assert resolves to the 1-arity overload (UserFunctionCall with return type Int).
/// Verifies arity-based disambiguation works.
#[test]
fn overload_arity_disambiguation_selects_one_arg_overload() {
    let source = r#"
fn f(x: Int) -> Int { x }
fn f(x: Int, y: Int) -> Int { x + y }
structure S { let v = f(5) }
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_arity_disam"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");

    match &v_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "f");
            assert_eq!(args.len(), 1, "should select the 1-arity overload");
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(
        v_expr.result_type,
        reify_core::Type::Int,
        "1-arity overload returns Int"
    );
}

/// step-6 (additional): Define fn f(x: Int)->Int and fn f(x: Int, y: Int)->Int,
/// call f(1, 2). Assert resolves to the 2-arity overload.
#[test]
fn overload_arity_disambiguation_selects_two_arg_overload() {
    let source = r#"
fn f(x: Int) -> Int { x }
fn f(x: Int, y: Int) -> Int { x + y }
structure S { let v = f(1, 2) }
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_arity_disam2"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");

    match &v_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "f");
            assert_eq!(args.len(), 2, "should select the 2-arity overload");
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(
        v_expr.result_type,
        reify_core::Type::Int,
        "2-arity overload returns Int"
    );
}

/// step-7: No user function named 'sqrt', call sqrt(4.14) in a structure let binding.
/// Assert compiles to FunctionCall (stdlib) with qualified_name 'std::sqrt', NOT an error.
/// Verifies stdlib fallback is preserved when no user functions have the name.
#[test]
fn stdlib_fallback_when_no_user_function_with_name() {
    let source = r#"
structure S { let v = sqrt(4.14) }
"#;
    let parsed = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("test_stdlib_fallback"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Should compile without errors
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");

    // Should be stdlib FunctionCall, not UserFunctionCall
    match &v_expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, .. } => {
            assert_eq!(function.qualified_name, "std::sqrt");
            assert_eq!(function.name, "sqrt");
        }
        other => panic!("expected stdlib FunctionCall, got {:?}", other),
    }
}

/// step-9: Define fn f(x: Int)->Int and fn f(x: Int)->Real (same param types,
/// different return types — duplicate signatures). Call f(3).
/// Assert produces an ambiguity error diagnostic that lists both candidate signatures.
#[test]
fn ambiguous_call_lists_candidate_signatures() {
    let source = r#"
fn f(x: Int) -> Int { x }
fn f(x: Int) -> Real { x + 0.0 }
structure S { let v = f(3) }
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ambiguous"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Should produce at least one error
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error (ambiguous or duplicate sig)"
    );

    // Find the ambiguous-call error (there may also be a duplicate-sig error)
    let ambiguous_error = errors.iter().find(|d| d.message.contains("ambiguous"));
    assert!(
        ambiguous_error.is_some(),
        "expected an 'ambiguous' error, got: {:?}",
        errors
    );

    let msg = &ambiguous_error.unwrap().message;
    // Should list both candidate signatures
    assert!(
        msg.contains("f(Int) -> Int"),
        "ambiguous error should list 'f(Int) -> Int', got: {:?}",
        msg
    );
    assert!(
        msg.contains("f(Int) -> Real"),
        "ambiguous error should list 'f(Int) -> Real', got: {:?}",
        msg
    );
}

/// step-11: E2E test for evaluator disambiguation.
/// Define fn double(x: Int)->Int { x * 2 } and fn double(x: Real)->Real { x * 2.0 }.
/// Call both in structure: let a = double(3) (Int arg) and let b = double(1.5) (Real arg).
/// Note: 3.0 compiles as Int because it's a whole number. Use 1.5 for a guaranteed Real literal.
/// Compile (assert no errors), then evaluate and verify a==Int(6) and b==Real(3.0).
///
/// FAILS because the evaluator matches by name+arity only, so both double(3) and double(1.5)
/// resolve to the FIRST 'double' function — producing wrong results for double(1.5).
#[test]
fn e2e_evaluator_disambiguates_int_vs_real_overload() {
    let source = r#"
fn double(x: Int) -> Int { x * 2 }
fn double(x: Real) -> Real { x * 2.0 }
structure S {
    let a = double(3)
    let b = double(1.5)
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_e2e_double"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Compiler should produce no errors — exact type matching picks Int for 3, Real for 1.5
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no compile errors, got: {:?}",
        errors
    );

    // Get the template
    let template = &compiled.templates[0];

    // Find value cells for 'a' and 'b'
    let a_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "a")
        .expect("should have 'a' value cell");
    let b_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "b")
        .expect("should have 'b' value cell");

    let a_expr = a_cell
        .default_expr
        .as_ref()
        .expect("'a' let should have expr");
    let b_expr = b_cell
        .default_expr
        .as_ref()
        .expect("'b' let should have expr");

    // Evaluate with EvalContext that includes the compiled functions
    let values = reify_ir::ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &compiled.functions);

    let a_val = reify_expr::eval_expr(a_expr, &ctx);
    let b_val = reify_expr::eval_expr(b_expr, &ctx);

    // a = double(3) should call the Int overload → Int(6)
    assert_eq!(
        a_val,
        reify_ir::Value::Int(6),
        "double(3) should return Int(6), got {:?}",
        a_val
    );

    // b = double(1.5) should call the Real overload → Real(3.0)
    assert_eq!(
        b_val,
        reify_ir::Value::Real(3.0),
        "double(1.5) should return Real(3.0), got {:?}",
        b_val
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
    let parsed = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("test_overload_int_real"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Should compile without any errors
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

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
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "f");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(
        v_expr.result_type,
        reify_core::Type::Int,
        "f(3) should select the Int overload, returning Int"
    );
}
