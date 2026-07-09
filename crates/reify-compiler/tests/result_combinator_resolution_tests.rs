//! Resolution + type-check tests for the six Result-specialized recovery
//! combinators — task result-fallback Layer-B γ #4037 (PRD
//! docs/prds/v0_6/result-and-fallback.md §4.3/§8.B task B-γ).
//!
//! `unwrap_or` / `is_ok` / `is_err` / `map_err` / `or_else` / `ok_or` are
//! declared as generic `pub fn`s in `stdlib/result.ri`, co-located with the
//! prelude `Result<T, E>` enum (#4035), and become prelude-callable without
//! an import. Resolution and return-type substitution are delivered free by
//! the existing generic-fn resolver (expr.rs `resolve_function_overload` →
//! `type_compat::unify` → `substitute_type_params`) plus #4991's compiler
//! substrate (enum-typed fn-param resolution + Option/Result overload
//! head-match disambiguation); no new resolver code is introduced by this
//! task.
//!
//! Tests use `reify_test_support::compile_source_with_stdlib` — NOT the bare
//! `compile_source` — because the combinators live in a stdlib module and are
//! only prelude-callable via `compile_with_stdlib`.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source_with_stdlib;

// ── helper (mirrors option_recovery_resolution_tests.rs) ────────────────────

fn cell_expr_stdlib<'a>(
    module: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = &module.templates[0];
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("value cell '{member}' not found"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{member}' has no default_expr"))
}

// ── (h) std/result module still loads clean ──────────────────────────────────

/// The `std/result` module must still compile with zero `Severity::Error`
/// diagnostics once the six recovery combinators are declared alongside the
/// `Result<T, E>` enum.
///
/// RED: not RED by itself today (the bare enum already loads clean), but
/// pinned here so step-2's additions can't silently break it.
#[test]
fn result_module_still_loads_clean() {
    let module = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/result")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/result module; available paths: {:?}",
                reify_compiler::stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in result.ri: {:?}",
        errors
    );
}

// ── (a) unwrap_or over a Result subject resolves T ───────────────────────────

/// [CORE SIGNAL] `unwrap_or(Ok { value: 5mm }, 0mm)` must resolve to the
/// stdlib Result overload and substitute `result_type` to `Type::length()`
/// (T), not `TypeParam("T")`. Zero Error diagnostics.
///
/// RED: `unwrap_or<T,E>(Result<T,E>, T) -> T` is not yet declared in
/// `result.ri` → the Result-typed subject has no matching overload → NoMatch
/// → Error diagnostic + poison result_type.
#[test]
fn unwrap_or_ok_subject_resolves_to_element_type() {
    let source = r#"
structure S {
    let v = unwrap_or(Ok { value: 5mm }, 0mm)
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for unwrap_or(Ok {{ value: 5mm }}, 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "unwrap_or(Ok {{ value: 5mm }}, 0mm) result_type should substitute T to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

/// `unwrap_or(Err { error: "bad" }, 0mm)` must likewise resolve — the Err
/// subject carries no `value` field, so T is bound entirely via the second
/// arg (`0mm`); this pins that path independently of the Ok-subject case
/// above. Zero Error diagnostics.
///
/// RED: same as above — the Result overload does not exist yet.
#[test]
fn unwrap_or_err_subject_resolves_to_element_type() {
    let source = r#"
structure S {
    let v = unwrap_or(Err { error: "bad" }, 0mm)
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for unwrap_or(Err {{ error: \"bad\" }}, 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "unwrap_or(Err {{ .. }}, 0mm) result_type should substitute T to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (b) is_ok / is_err over a Result cell resolve to Bool ────────────────────

/// [CORE SIGNAL] `is_ok(Ok { value: 5mm })` must resolve and the result type
/// must be `Type::Bool`. Zero Error diagnostics.
///
/// RED: `is_ok` is not declared anywhere → unresolved name (NoMatch).
#[test]
fn is_ok_over_result_resolves_to_bool() {
    let source = r#"
structure S {
    let b = is_ok(Ok { value: 5mm })
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for is_ok(Ok {{ value: 5mm }}), got: {:?}",
        errors
    );

    let b_expr = cell_expr_stdlib(&module, "b");
    assert_eq!(
        b_expr.result_type,
        Type::Bool,
        "is_ok(Ok {{ .. }}) result_type should be Bool, got {:?}",
        b_expr.result_type
    );
}

/// `is_err(Err { error: "bad" })` must resolve and the result type must be
/// `Type::Bool`. Zero Error diagnostics.
///
/// RED: `is_err` is not declared anywhere → unresolved name (NoMatch).
#[test]
fn is_err_over_result_resolves_to_bool() {
    let source = r#"
structure S {
    let b = is_err(Err { error: "bad" })
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for is_err(Err {{ .. }}), got: {:?}",
        errors
    );

    let b_expr = cell_expr_stdlib(&module, "b");
    assert_eq!(
        b_expr.result_type,
        Type::Bool,
        "is_err(Err {{ .. }}) result_type should be Bool, got {:?}",
        b_expr.result_type
    );
}

// ── (c) or_else over Result resolves to Result<T,E> ──────────────────────────

/// [CORE SIGNAL] `or_else(r, r)` where `r : Result<Length, String>` must
/// resolve and the result type must be
/// `Type::Applied { name: "Result", args: [Type::length(), Type::String] }`.
/// Zero Error diagnostics.
///
/// A concretely-typed `param r` (rather than a bare `Ok {..}` literal) is
/// used so BOTH T and E are bound from the declared type — the erased
/// construction-inference result type of a bare literal would leave E
/// unbound, which cannot substitute into the `Result<T,E>` return type.
///
/// RED: `or_else<T,E>(Result<T,E>, Result<T,E>) -> Result<T,E>` is not yet
/// declared in `result.ri` → unresolved name (NoMatch).
#[test]
fn or_else_over_result_resolves_to_result_element() {
    let source = r#"
structure S {
    param r : Result<Length, String> = Ok { value: 5mm }
    let w = or_else(r, r)
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for or_else(r, r), got: {:?}",
        errors
    );

    let w_expr = cell_expr_stdlib(&module, "w");
    assert_eq!(
        w_expr.result_type,
        Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::length(), Type::String],
        },
        "or_else(r, r) result_type should be Result<Scalar<LENGTH>, String>, got {:?}",
        w_expr.result_type
    );
}

// ── (d) map_err over Result<Length,String> resolves to Result<T,F> ──────────

/// [CORE SIGNAL] `map_err(r, |e: String| true)` where `r : Result<Length,
/// String>` must resolve: the arrow-type third param `f: (E) -> F` binds
/// E=String (agreeing with r's declared E) and F=Bool (from the lambda's
/// Bool body), and the result type substitutes to
/// `Type::Applied { name: "Result", args: [Type::length(), Type::Bool] }` —
/// proving F is resolved independently of T and E (neither is Bool). Zero
/// Error diagnostics.
///
/// RED: `map_err<T,E,F>(Result<T,E>, (E)->F) -> Result<T,F>` is not yet
/// declared in `result.ri` → unresolved name (NoMatch).
#[test]
fn map_err_over_result_resolves_to_result_with_f() {
    let source = r#"
structure S {
    param r : Result<Length, String> = Ok { value: 5mm }
    let m = map_err(r, |e: String| true)
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for map_err(r, |e: String| true), got: {:?}",
        errors
    );

    let m_expr = cell_expr_stdlib(&module, "m");
    assert_eq!(
        m_expr.result_type,
        Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::length(), Type::Bool],
        },
        "map_err(r, |e: String| true) result_type should be Result<Scalar<LENGTH>, Bool>, got {:?}",
        m_expr.result_type
    );
}

// ── (e) ok_or(some(5mm), "e") resolves to Result<T,E> ────────────────────────

/// [CORE SIGNAL] `ok_or(some(5mm), "e")` — the standard Option→Result bridge
/// — must resolve and the result type must be
/// `Type::Applied { name: "Result", args: [Type::length(), Type::String] }`.
/// Zero Error diagnostics.
///
/// RED: `ok_or<T,E>(Option<T>, E) -> Result<T,E>` is not yet declared in
/// `result.ri` → unresolved name (NoMatch).
#[test]
fn ok_or_resolves_to_result_element() {
    let source = r#"
structure S {
    let o = ok_or(some(5mm), "e")
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for ok_or(some(5mm), \"e\"), got: {:?}",
        errors
    );

    let o_expr = cell_expr_stdlib(&module, "o");
    assert_eq!(
        o_expr.result_type,
        Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::length(), Type::String],
        },
        "ok_or(some(5mm), \"e\") result_type should be Result<Scalar<LENGTH>, String>, got {:?}",
        o_expr.result_type
    );
}

// ── (f) E_FALLBACK_TYPE reachable over a Result subject ──────────────────────

/// [CORE SIGNAL] `unwrap_or(r, "y")` where `param r : Result<Length, String>`
/// binds T=Length via the subject, then `"y"` (String) conflicts with T as
/// the default arg — must emit exactly one Error diagnostic with code
/// `DiagnosticCode::FallbackType` (E_FALLBACK_TYPE), proving the pre-existing
/// E_FALLBACK_TYPE emission (`expr.rs::is_fallback_combinator`) is reachable
/// for the REAL stdlib Result overload (not just a user-declared stand-in, as
/// in `result_combinator_overload_tests.rs`).
///
/// RED: the Result overload does not exist yet, so `unwrap_or(r, "y")` is an
/// unresolved name — no overload to conflict, so `FallbackType` cannot fire
/// (a generic unresolved-name-style diagnostic fires instead).
#[test]
fn unwrap_or_over_result_param_emits_e_fallback_type() {
    let source = r#"
structure S {
    param r : Result<Length, String> = Ok { value: 5mm }
    let v = unwrap_or(r, "y")
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 Error diagnostic for unwrap_or(r, \"y\"), got: {:?}",
        errors
    );

    let diag = &errors[0];
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::FallbackType),
        "expected DiagnosticCode::FallbackType, got: {:?}",
        diag.code
    );
    assert!(
        diag.message.contains("E_FALLBACK_TYPE"),
        "expected diagnostic message to contain \"E_FALLBACK_TYPE\", got: {:?}",
        diag.message
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::Error,
        "poisoned cell result_type should be Type::Error, got {:?}",
        v_expr.result_type
    );
}

// ── (g) REGRESSION GUARD: Option subject still disambiguates correctly ──────

/// [REGRESSION GUARD] `unwrap_or(some(5mm), 0mm)` must still resolve to the
/// STDLIB OPTION overload (result_type substituted to `Type::length()`) now
/// that a same-named REAL stdlib Result overload also exists. Zero Error
/// diagnostics.
///
/// Guards against the Result overload's addition (this task) breaking
/// Option-subject resolution — the head-match disambiguation added by #4991
/// must keep picking the Option overload for a `some`/`none` subject.
#[test]
fn unwrap_or_option_subject_still_resolves_to_option_overload() {
    let source = r#"
structure S {
    let v = unwrap_or(some(5mm), 0mm)
}
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for unwrap_or(some(5mm), 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "unwrap_or(some(5mm), 0mm) should still resolve to the stdlib Option overload \
         (result_type substituted to Scalar<LENGTH>), got {:?}",
        v_expr.result_type
    );
}
