//! B2+B3 tests — Option/Result overload disambiguation and E_FALLBACK_TYPE
//! reachability over a Result subject — task result-fallback Layer-B
//! substrate (PRD docs/prds/v0_6/result-and-fallback.md).
//!
//! Every source here declares a user-defined generic
//! `unwrap_or<T,E>(r: Result<T,E>, ..)` alongside the stdlib
//! `unwrap_or<T>(o: Option<T>, dflt: T) -> T` from `option_recovery.ri`
//! (pulled in via `compile_source_with_stdlib`), so every call has TWO
//! wildcard-matching generic candidates and must be disambiguated by
//! constructor head rather than exact-equality.

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

// ── (a) Signal 2: Option vs Result overload disambiguation ──────────────────

/// [CORE SIGNAL] A user `unwrap_or<T,E>(r: Result<T,E>, d: T) -> Bool` competes
/// with the stdlib `unwrap_or<T>(o: Option<T>, dflt: T) -> T`. A call over an
/// (erased) `Ok { value: 5mm }` subject must resolve to the user Result
/// overload (`a`'s result type is the distinctive `Bool`, proving direction);
/// a call over a `some(5mm)` subject must resolve to the stdlib Option
/// overload (`b`'s result type substitutes to the element type
/// `Scalar<LENGTH>`). Zero Error diagnostics.
///
/// RED (after B1, before B2): both overloads' Result/Option-typed first
/// param carry a type param, so BOTH wildcard-match every subject —
/// `matches` has 2 elements and `exact_matches` is empty (a declared generic
/// param never equals a concrete arg type) → `Ambiguous` → poison for both
/// `a` and `b` (neither is `Bool` nor `Scalar<LENGTH>`).
#[test]
fn unwrap_or_disambiguates_option_vs_result_by_head() {
    let source = r#"
fn unwrap_or<T, E>(r: Result<T, E>, d: T) -> Bool { true }

structure S {
    let a = unwrap_or(Ok { value: 5mm }, 0mm)
    let b = unwrap_or(some(5mm), 0mm)
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
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    let a_expr = cell_expr_stdlib(&module, "a");
    assert_eq!(
        a_expr.result_type,
        Type::Bool,
        "unwrap_or(Ok {{ value: 5mm }}, 0mm) should resolve to the user Result \
         overload (result_type Bool), got {:?}",
        a_expr.result_type
    );

    let b_expr = cell_expr_stdlib(&module, "b");
    assert_eq!(
        b_expr.result_type,
        Type::length(),
        "unwrap_or(some(5mm), 0mm) should resolve to the stdlib Option overload \
         (result_type substituted to Scalar<LENGTH>), got {:?}",
        b_expr.result_type
    );
}

// ── (b) Signal 3: E_FALLBACK_TYPE reachable over a Result subject ───────────

/// [CORE SIGNAL] With the same competing-overload setup, a call whose
/// Result-typed subject is a CONCRETELY-typed structure param (`r: Result<Length,
/// String>`, so `unify` can bind `T=Length` from it) and whose default arg
/// conflicts (`"y" : String` against `T=Length`) must emit
/// `DiagnosticCode::FallbackType` — proving the pre-existing E_FALLBACK_TYPE
/// emission (expr.rs, `is_fallback_combinator`) is reachable for a Result
/// subject once B1+B2 land.
///
/// The subject `r` is a STRUCTURE param (not a sibling fn's param): a fn
/// body's own calls resolve only against `ctx.functions` (prior sibling user
/// fns in source order), NOT the prelude-merged `ctx.resolution_functions` —
/// so a call inside a fn body would never see the competing stdlib Option
/// overload and this signal would be vacuously green regardless of B2. A
/// structure's value cells resolve calls against `ctx.resolution_functions`
/// (entities_phase.rs), so both the user Result overload and the stdlib
/// Option overload are visible candidates here, exactly like signal 2 above.
///
/// RED (after B1, before B2): `unwrap_or(r, "y")` wildcard-matches both
/// overloads → `Ambiguous` → poison; the call never reaches the
/// single-winner `Resolved` arm, so `FallbackType` never fires.
#[test]
fn unwrap_or_over_result_param_emits_e_fallback_type() {
    let source = r#"
fn unwrap_or<T, E>(r: Result<T, E>, d: T) -> T { d }

structure S {
    param r : Result<Length, String> = Ok { value: 5mm }
    let v = unwrap_or(r, "y")
}
"#;
    let module = compile_source_with_stdlib(source);

    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::FallbackType)),
        "expected a DiagnosticCode::FallbackType diagnostic for unwrap_or(r, \"y\") \
         over r: Result<Length, String>, got: {:?}",
        module.diagnostics
    );
}
