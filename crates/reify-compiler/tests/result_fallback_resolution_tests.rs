//! Resolution + type-check tests for `fallback` over a `Result<T, E>`
//! subject — task result-fallback Layer-B δ #4038 (PRD
//! docs/prds/v0_6/result-and-fallback.md §4.3/§5 D6/§8.B task B-δ).
//!
//! `fallback` is already declared (and covered) for an `Option<T>` subject in
//! `stdlib/option_recovery.ri` (task α) and shares its eval-side dispatch arm
//! with `unwrap_or`/`or_default` (`is_combinator`/`eval_extract_or_default` in
//! `crates/reify-expr/src/option_recovery.rs`, extended to `Result` by task γ
//! #4037). This task adds the missing STDLIB DECLARATION —
//! `fallback<T, E>(Result<T, E>, T) -> T` in `result.ri` — so the compiler can
//! resolve `fallback` calls whose subject is `Result`-typed; no new resolver
//! code is introduced.
//!
//! Tests use `reify_test_support::compile_source_with_stdlib` — NOT the bare
//! `compile_source` — because the combinators live in a stdlib module and are
//! only prelude-callable via `compile_with_stdlib`. Mirrors
//! `result_combinator_resolution_tests.rs` (task γ #4037) and
//! `option_recovery_resolution_tests.rs`'s fallback cases (task α).

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source_with_stdlib;

// ── helper (mirrors result_combinator_resolution_tests.rs) ──────────────────

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

// ── (a) fallback over an Ok-subject Result literal resolves T ───────────────

/// [CORE SIGNAL] `fallback(Ok { value: 5mm }, 0mm)` must resolve to the
/// stdlib Result overload and substitute `result_type` to `Type::length()`
/// (T), not `TypeParam("T")`. Zero Error diagnostics.
///
/// RED: `fallback<T,E>(Result<T,E>, T) -> T` is not yet declared in
/// `result.ri` (only the Option overload exists) → the Result-typed subject
/// has no matching overload → NoMatch → Error diagnostic + poisoned
/// result_type.
#[test]
fn fallback_ok_subject_resolves_to_element_type() {
    let source = r#"
structure S {
    let v = fallback(Ok { value: 5mm }, 0mm)
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
        "expected no Error diagnostics for fallback(Ok {{ value: 5mm }}, 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "fallback(Ok {{ value: 5mm }}, 0mm) result_type should substitute T to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (b) fallback over an Err-subject Result literal resolves T via dflt ─────

/// `fallback(Err { error: "bad" }, 0mm)` must likewise resolve — the Err
/// subject carries no `value` field, so T is bound entirely via the second
/// arg (`0mm`); this pins that path independently of the Ok-subject case
/// above. Zero Error diagnostics.
///
/// RED: same as above — the Result `fallback` overload does not exist yet.
#[test]
fn fallback_err_subject_resolves_to_element_type() {
    let source = r#"
structure S {
    let v = fallback(Err { error: "bad" }, 0mm)
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
        "expected no Error diagnostics for fallback(Err {{ error: \"bad\" }}, 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "fallback(Err {{ .. }}, 0mm) result_type should substitute T to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (c) E_FALLBACK_TYPE reachable over a Result subject via fallback ────────

/// [CORE SIGNAL] `fallback(r, "y")` where `param r : Result<Length, String>`
/// binds T=Length via the subject, then `"y"` (String) conflicts with T as
/// the default arg — must emit exactly one Error diagnostic with code
/// `DiagnosticCode::FallbackType` (E_FALLBACK_TYPE), proving the pre-existing
/// E_FALLBACK_TYPE emission (`expr.rs::is_fallback_combinator`, which already
/// lists `"fallback"` in `FALLBACK_COMBINATORS`) is reachable for the new
/// stdlib Result `fallback` overload.
///
/// RED: the Result `fallback` overload does not exist yet, so `fallback(r,
/// "y")` is an unresolved name — no overload to conflict, so `FallbackType`
/// cannot fire (a generic unresolved-name-style diagnostic fires instead).
#[test]
fn fallback_over_result_param_emits_e_fallback_type() {
    let source = r#"
structure S {
    param r : Result<Length, String> = Ok { value: 5mm }
    let v = fallback(r, "y")
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
        "expected exactly 1 Error diagnostic for fallback(r, \"y\"), got: {:?}",
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

// ── (d) REGRESSION GUARD: Option subject still disambiguates correctly ──────

/// [REGRESSION GUARD] `fallback(some(5mm), 0mm)` must still resolve to the
/// STDLIB OPTION overload (result_type substituted to `Type::length()`) now
/// that a same-named REAL stdlib Result overload also exists. Zero Error
/// diagnostics.
///
/// Guards against the Result `fallback` overload's addition (this task)
/// breaking Option-subject resolution — the head-match disambiguation added
/// by #4991 must keep picking the Option overload for a `some`/`none`
/// subject, exactly as it already does for `unwrap_or`.
#[test]
fn fallback_option_subject_still_resolves_to_option_overload() {
    let source = r#"
structure S {
    let v = fallback(some(5mm), 0mm)
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
        "expected no Error diagnostics for fallback(some(5mm), 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "fallback(some(5mm), 0mm) should still resolve to the stdlib Option overload \
         (result_type substituted to Scalar<LENGTH>), got {:?}",
        v_expr.result_type
    );
}
