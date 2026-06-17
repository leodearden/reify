//! Resolution + type-check tests for the `std.option_recovery` stdlib module —
//! task α of PRD docs/prds/v0_6/result-and-fallback.md §8 Phase 1.
//!
//! The 7 generic combinators (unwrap_or / or_else / or_default / fallback /
//! is_some / is_none / get_or) are declared in `stdlib/option_recovery.ri` and
//! become prelude-callable without an import.  Resolution and return-type
//! substitution are delivered free by the existing generic-fn resolver
//! (expr.rs `resolve_function_overload` → `type_compat::unify` →
//! `substitute_type_params`); no new resolver code is introduced.
//!
//! Tests use `reify_test_support::compile_source_with_stdlib` — NOT the bare
//! `compile_source` — because the combinators live in a stdlib module and are
//! only prelude-callable via `compile_with_stdlib`.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source_with_stdlib;

// ── helper ───────────────────────────────────────────────────────────────────

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

// ── (a) module loads clean ───────────────────────────────────────────────────

/// The `std/option_recovery` module must be registered in the stdlib loader and
/// compile with zero Severity::Error diagnostics.
///
/// RED: `std/option_recovery` does not exist yet — the `find` returns None.
#[test]
fn option_recovery_module_loads_clean() {
    let module = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/option_recovery")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/option_recovery module; available paths: {:?}",
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
        "unexpected error diagnostics in option_recovery.ri: {:?}",
        errors
    );
}

// ── (b) unwrap_or resolves and substitutes return type to element type ────────

/// [CORE SIGNAL] `unwrap_or(o, 6mm)` where `o : Option<Length>` must resolve to
/// a `UserFunctionCall` and the result type must be substituted to `Type::length()`
/// (not `TypeParam("T")`). Zero Error diagnostics.
///
/// RED: std/option_recovery does not exist → `unwrap_or` is an unresolved name
/// (NoMatch) → Error diagnostic + poison result_type.
#[test]
fn unwrap_or_resolves_and_substitutes_to_element_type() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = unwrap_or(o, 6mm)
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
        "expected no Error diagnostics for unwrap_or(o, 6mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "unwrap_or(o, 6mm) result_type should be substituted to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (c) is_some resolves to Bool ─────────────────────────────────────────────

/// [CORE SIGNAL] `is_some(o)` where `o : Option<Length>` must resolve and the
/// result type must be `Type::Bool`. Zero Error diagnostics.
///
/// RED: std/option_recovery does not exist → unresolved name.
#[test]
fn is_some_resolves_to_bool() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let b = is_some(o)
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
        "expected no Error diagnostics for is_some(o), got: {:?}",
        errors
    );

    let b_expr = cell_expr_stdlib(&module, "b");
    assert_eq!(
        b_expr.result_type,
        Type::Bool,
        "is_some(o) result_type should be Bool, got {:?}",
        b_expr.result_type
    );
}

// ── (d) or_else resolves to Option<element_type> ─────────────────────────────

/// `or_else(o, o)` where `o : Option<Length>` must resolve and the result type
/// must be `Type::Option(Box::new(Type::length()))`. Zero Error diagnostics.
///
/// RED: std/option_recovery does not exist → unresolved name.
#[test]
fn or_else_resolves_to_option_element() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let w = or_else(o, o)
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
        "expected no Error diagnostics for or_else(o, o), got: {:?}",
        errors
    );

    let w_expr = cell_expr_stdlib(&module, "w");
    assert_eq!(
        w_expr.result_type,
        Type::Option(Box::new(Type::length())),
        "or_else(o, o) result_type should be Option<Scalar<LENGTH>>, got {:?}",
        w_expr.result_type
    );
}

// ── (e) get_or resolves to value type V ──────────────────────────────────────

/// `get_or(m, "key", 0mm)` where `m : Map<String, Length>` must resolve and the
/// result type must be `Type::length()` (the map's V type). Zero Error diagnostics.
///
/// Uses a non-empty map literal default `map{"k" => 1mm}` so the inferred type
/// is exactly `Map<String, Length>` with no empty-map warning and no
/// default-vs-annotation mismatch.
///
/// RED: std/option_recovery does not exist → unresolved name.
#[test]
fn get_or_resolves_to_value_type() {
    let source = r#"
structure S {
    param m : Map<String, Length> = map{"k" => 1mm}
    let v = get_or(m, "key", 0mm)
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
        "expected no Error diagnostics for get_or(m, \"key\", 0mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "get_or(m, \"key\", 0mm) result_type should be Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (f) E_FALLBACK_TYPE on default/element type mismatch ─────────────────────

/// `unwrap_or(o, "x")` where `o : Option<Length>` binds T=Length via the
/// first arg, then the second arg "x" (String) conflicts: the call yields
/// exactly one Error diagnostic with code == DiagnosticCode::FallbackType
/// AND the message contains the mnemonic "E_FALLBACK_TYPE", and the
/// result cell is poisoned (default_expr.result_type == Type::Error).
///
/// RED (after step-2): the conflict arm already fires
/// (type_compat::unify returns Err(TypeArgConflict), T=Length vs T=String)
/// but emits DiagnosticCode::FnTypeArgConflict — NOT FallbackType, which
/// does not yet exist → this test fails to compile / the code assertion
/// fails.
#[test]
fn unwrap_or_default_element_type_mismatch_emits_e_fallback_type() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = unwrap_or(o, "x")
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
        "expected exactly 1 Error diagnostic for unwrap_or(o, \"x\"), got: {:?}",
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

// ── (g) or_default resolves and substitutes return type ───────────────────────

/// `or_default(o, 6mm)` where `o : Option<Length>` must resolve and the
/// result type must be substituted to `Type::length()`. Zero Error diagnostics.
///
/// or_default is an alias of unwrap_or (PRD fork F2-a); this test ensures its
/// signature is identical and the generic resolver handles it correctly.
#[test]
fn or_default_resolves_and_substitutes_to_element_type() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = or_default(o, 6mm)
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
        "expected no Error diagnostics for or_default(o, 6mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "or_default(o, 6mm) result_type should be substituted to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (h) fallback resolves and substitutes return type ─────────────────────────

/// `fallback(o, 6mm)` where `o : Option<Length>` must resolve and the result
/// type must be substituted to `Type::length()`. Zero Error diagnostics.
///
/// fallback is the free-function alias of unwrap_or (PRD fork F2-a / D6).
#[test]
fn fallback_resolves_and_substitutes_to_element_type() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = fallback(o, 6mm)
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
        "expected no Error diagnostics for fallback(o, 6mm), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "fallback(o, 6mm) result_type should be substituted to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

// ── (i) is_none resolves to Bool ─────────────────────────────────────────────

/// `is_none(o)` where `o : Option<Length>` must resolve and the result type
/// must be `Type::Bool`. Zero Error diagnostics.
///
/// Complements the is_some test (c); ensures the Bool return type is correct
/// for the negative predicate.
#[test]
fn is_none_resolves_to_bool() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let b = is_none(o)
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
        "expected no Error diagnostics for is_none(o), got: {:?}",
        errors
    );

    let b_expr = cell_expr_stdlib(&module, "b");
    assert_eq!(
        b_expr.result_type,
        Type::Bool,
        "is_none(o) result_type should be Bool, got {:?}",
        b_expr.result_type
    );
}

// ── (j) or_default E_FALLBACK_TYPE on mismatch ───────────────────────────────

/// `or_default(o, "x")` where `o : Option<Length>` binds T=Length via the
/// first arg, then "x" (String) conflicts as T.  Must emit exactly one Error
/// diagnostic with code == DiagnosticCode::FallbackType and message containing
/// "E_FALLBACK_TYPE".
///
/// Ensures or_default is genuinely in `FALLBACK_COMBINATORS` — a regression
/// that dropped it would emit FnTypeArgConflict instead.
#[test]
fn or_default_default_element_type_mismatch_emits_e_fallback_type() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = or_default(o, "x")
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
        "expected exactly 1 Error diagnostic for or_default(o, \"x\"), got: {:?}",
        errors
    );

    let diag = &errors[0];
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::FallbackType),
        "expected DiagnosticCode::FallbackType for or_default conflict, got: {:?}",
        diag.code
    );
    assert!(
        diag.message.contains("E_FALLBACK_TYPE"),
        "expected diagnostic message to contain \"E_FALLBACK_TYPE\", got: {:?}",
        diag.message
    );
}

// ── (k) fallback E_FALLBACK_TYPE on mismatch ─────────────────────────────────

/// `fallback(o, "x")` where `o : Option<Length>` emits E_FALLBACK_TYPE.
/// Ensures fallback is in `FALLBACK_COMBINATORS`.
#[test]
fn fallback_default_element_type_mismatch_emits_e_fallback_type() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = fallback(o, "x")
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
        "expected exactly 1 Error diagnostic for fallback(o, \"x\"), got: {:?}",
        errors
    );

    let diag = &errors[0];
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::FallbackType),
        "expected DiagnosticCode::FallbackType for fallback conflict, got: {:?}",
        diag.code
    );
    assert!(
        diag.message.contains("E_FALLBACK_TYPE"),
        "expected diagnostic message to contain \"E_FALLBACK_TYPE\", got: {:?}",
        diag.message
    );
}

// ── (l) get_or E_FALLBACK_TYPE on value-type mismatch ────────────────────────

/// `get_or(m, "k", "x")` where `m : Map<String, Length>` binds V=Length via
/// the map type, then "x" (String) conflicts as V.  Must emit exactly one Error
/// diagnostic with code == DiagnosticCode::FallbackType.
///
/// get_or has a distinct code path (Map<K,V>, three arguments, K+V binding),
/// so this test independently verifies E_FALLBACK_TYPE emission for it.
/// A regression that dropped get_or from `FALLBACK_COMBINATORS` would emit
/// FnTypeArgConflict instead.
#[test]
fn get_or_default_value_type_mismatch_emits_e_fallback_type() {
    let source = r#"
structure S {
    param m : Map<String, Length> = map{"k" => 1mm}
    let v = get_or(m, "k", "x")
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
        "expected exactly 1 Error diagnostic for get_or(m, \"k\", \"x\"), got: {:?}",
        errors
    );

    let diag = &errors[0];
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::FallbackType),
        "expected DiagnosticCode::FallbackType for get_or conflict, got: {:?}",
        diag.code
    );
    assert!(
        diag.message.contains("E_FALLBACK_TYPE"),
        "expected diagnostic message to contain \"E_FALLBACK_TYPE\", got: {:?}",
        diag.message
    );
}

// ── (m) map_or present: std/option_recovery still loads clean ─────────────────
//
// task 4595 step-7.  Adding `map_or<T, U>(o: Option<T>, dflt: U, f: (T) -> U)`
// must keep the std/option_recovery module compiling with ZERO Severity::Error
// diagnostics, and map_or must appear among the module's resolved functions
// (proving the new arrow-type production parses + lowers + resolves inside a
// real stdlib module, not just an isolated unit test).
//
// RED: map_or is not yet declared in option_recovery.ri (step-8 adds it), so
// the `any` for "map_or" is false and the assertion fails.
#[test]
fn map_or_present_and_module_loads_clean() {
    let module = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/option_recovery")
        .expect("stdlib should contain std/option_recovery module");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "option_recovery.ri (with map_or) should load with zero Error diagnostics, got: {:?}",
        errors
    );

    assert!(
        module.functions.iter().any(|f| f.name == "map_or"),
        "map_or should be declared in std/option_recovery; functions present: {:?}",
        module
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
    );
}

// ── (n) map_or resolved signature: third param is a Type::Function ────────────
//
// task 4595 step-7 [CORE SIGNAL].  map_or<T, U>(o: Option<T>, dflt: U,
// f: (T) -> U) -> U — the arrow-type third parameter must resolve to
// `Type::Function { params: [TypeParam("T")], return_type: TypeParam("U") }`.
// This is the proof that the grammar → lowering → resolution chain (steps
// 2/4/6) carries an arrow type all the way to a resolved `Type::Function` in a
// real stdlib signature.
//
// RED: map_or is not yet declared (step-8), so the function is absent.
#[test]
fn map_or_third_param_resolves_to_type_function() {
    let module = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/option_recovery")
        .expect("stdlib should contain std/option_recovery module");

    let map_or = module
        .functions
        .iter()
        .find(|f| f.name == "map_or")
        .expect("map_or should be declared in std/option_recovery");

    assert_eq!(
        map_or.params.len(),
        3,
        "map_or should have 3 params (o, dflt, f), got: {:?}",
        map_or.params
    );

    let (f_name, f_ty) = &map_or.params[2];
    assert_eq!(f_name, "f", "third param should be named f, got: {:?}", f_name);
    assert_eq!(
        *f_ty,
        Type::Function {
            params: vec![Type::TypeParam("T".to_string())],
            return_type: Box::new(Type::TypeParam("U".to_string())),
        },
        "map_or's third param `f` should resolve to (T) -> U as Type::Function, got: {:?}",
        f_ty
    );
}

// ── (o) map_or call type-checks clean and resolves result type to U ───────────
//
// task 4595 step-7 [CORE SIGNAL].  A user call `map_or(o, 6mm, |x: Length| x)`
// where `o : Option<Length>` binds T=Length (arg0) and U=Length (arg1 `dflt`
// plus the lambda body); passing the lambda to the `(T) -> U` parameter is
// accepted by the existing generic-fn unify path and the call's result type
// substitutes to U = Length.  Zero Error diagnostics.
//
// The lambda param is explicitly typed `|x: Length|` so its inferred type is
// `(Length) -> Length`; an untyped `|x|` would default to a dimensionless Real
// param and conflict with T=Length — reify lambdas infer their param types
// locally rather than from the expected `(T) -> U` (see the list_helpers
// flat_map note).  Untyped lambdas over the inner type are exercised by the
// eval intercept (step-9), which constructs the IR directly and bypasses this
// front-end inference.
//
// RED: map_or is not declared yet (step-8) → unresolved name → Error.
#[test]
fn map_or_call_typechecks_clean_and_resolves_to_u() {
    let source = r#"
structure S {
    param o : Option<Length> = none
    let v = map_or(o, 6mm, |x: Length| x)
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
        "expected no Error diagnostics for map_or(o, 6mm, |x: Length| x), got: {:?}",
        errors
    );

    let v_expr = cell_expr_stdlib(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "map_or(o, 6mm, |x: Length| x) result_type should substitute U to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}
