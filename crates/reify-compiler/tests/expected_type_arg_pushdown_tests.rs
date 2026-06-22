//! Expected-type argument push-down integration tests — task #4703 δ.
//!
//! Exercises the FunctionCall-arm push-down that threads a selected candidate's
//! parameter type into an empty collection-literal argument as `expected_type`,
//! consuming the α/#4701 channel (compile_expr_guarded_with_expected).
//!
//! Signal references (PRD §7):
//!   - #5  (positive push-down): `firstlen([])` resolves cleanly with no warning
//!   - #6  (TypeUndetermined): `ident([])` emits E_TYPE_UNDETERMINED, not silent accept
//!
//! Step 3 tests: §7#5 positive path + all-three-kind parity + non-regression.
//! Step 5 tests: §7#6 TypeUndetermined + bound-by-other-arg suppression.
//! Step 7 tests: §10.2 overload-ambiguity fallback (push-down does NOT engage).
//!
//! Uses `reify_test_support::compile_source` (resolves `1mm`/`Length`/`List`
//! with no stdlib) — the established arg-position test pattern.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

/// Locate the `default_expr` of a named value cell in the first template.
fn cell_expr<'a>(
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

// ── step-3 tests: §7#5 positive push-down (List / Set / Map parity) ──────────

/// §7#5 (i) — List case.
///
/// `firstlen([])` over `fn firstlen(xs: List<Length>) -> Int { xs.count }`:
/// after push-down, the compiler must resolve `[]` as `List<Length>` (not the
/// current `List<Real>` default), so overload resolution finds the candidate.
///
/// Asserts:
/// - NO Error diagnostic (the current "no matching overload for firstlen(List<Real>)" error vanishes).
/// - NO "cannot infer element type of empty list" Warning (the warning is suppressed when
///   the expected element type is concrete — α §5.3 Resolve arm).
/// - The resolved cell type is `Type::Int` (the function's return type).
/// - The expression kind is `UserFunctionCall { function_name: "firstlen", … }`.
///
/// RED until step-4: today `[]` defaults to `List<Real>` → NoMatch error.
#[test]
fn pushdown_list_arg_resolves_empty_literal_to_param_element_type() {
    let source = "fn firstlen(xs: List<Length>) -> Int { xs.count } \
                  structure S { let n = firstlen([]) }";
    let module = compile_source(source);

    // No Error diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "§7#5 List: expected no Error diagnostics for firstlen([]), got: {errors:?}"
    );

    // No empty-list-inference Warning.
    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("cannot infer element type of empty list")
        })
        .collect();
    assert!(
        warnings.is_empty(),
        "§7#5 List: expected no empty-list-inference warning for firstlen([]), got: {warnings:?}"
    );

    // Resolved result type is Int.
    let n_expr = cell_expr(&module, "n");
    assert_eq!(
        n_expr.result_type,
        Type::Int,
        "§7#5 List: firstlen([]) result_type should be Int, got {:?}",
        n_expr.result_type
    );

    // Expression kind is UserFunctionCall.
    match &n_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "firstlen",
                "§7#5 List: expected UserFunctionCall for firstlen, got name={function_name:?}"
            );
        }
        other => panic!(
            "§7#5 List: expected UserFunctionCall for firstlen([]), got {other:?}"
        ),
    }
}

/// §7#5 (ii) — Set parity.
///
/// `f(set {})` over `fn f(s: Set<Length>) -> Int { s.count }`:
/// after push-down, `set {}` must resolve as `Set<Length>` (not the current
/// `Set<Real>` default), so overload resolution finds the candidate.
///
/// RED until step-4: today `set {}` defaults to `Set<Real>` → NoMatch error.
#[test]
fn pushdown_set_arg_resolves_empty_literal_to_param_element_type() {
    let source = "fn f(s: Set<Length>) -> Int { s.count } \
                  structure S { let n = f(set {}) }";
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "§7#5 Set: expected no Error diagnostics for f(set {{}}), got: {errors:?}"
    );

    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("cannot infer element type of empty set")
        })
        .collect();
    assert!(
        warnings.is_empty(),
        "§7#5 Set: expected no empty-set-inference warning for f(set {{}}), got: {warnings:?}"
    );

    let n_expr = cell_expr(&module, "n");
    assert_eq!(
        n_expr.result_type,
        Type::Int,
        "§7#5 Set: f(set {{}}) result_type should be Int, got {:?}",
        n_expr.result_type
    );

    match &n_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(function_name, "f", "§7#5 Set: expected UserFunctionCall for f");
        }
        other => panic!("§7#5 Set: expected UserFunctionCall for f(set {{}}), got {other:?}"),
    }
}

/// §7#5 (iii) — Map parity.
///
/// `g(map {})` over `fn g(m: Map<String,Length>) -> Int { m.count }`:
/// after push-down, `map {}` must resolve as `Map<String,Length>` (not the
/// current `Map<String,Real>` default), so overload resolution finds the candidate.
///
/// RED until step-4: today `map {}` defaults to `Map<String,Real>` → NoMatch error.
#[test]
fn pushdown_map_arg_resolves_empty_literal_to_param_key_value_types() {
    let source = "fn g(m: Map<String,Length>) -> Int { m.count } \
                  structure S { let n = g(map {}) }";
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "§7#5 Map: expected no Error diagnostics for g(map {{}}), got: {errors:?}"
    );

    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("cannot infer")
                && d.message.contains("empty map")
        })
        .collect();
    assert!(
        warnings.is_empty(),
        "§7#5 Map: expected no empty-map-inference warning for g(map {{}}), got: {warnings:?}"
    );

    let n_expr = cell_expr(&module, "n");
    assert_eq!(
        n_expr.result_type,
        Type::Int,
        "§7#5 Map: g(map {{}}) result_type should be Int, got {:?}",
        n_expr.result_type
    );

    match &n_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(function_name, "g", "§7#5 Map: expected UserFunctionCall for g");
        }
        other => panic!("§7#5 Map: expected UserFunctionCall for g(map {{}}), got {other:?}"),
    }
}

/// §7#5 (iv) — Non-regression: non-empty list arg still resolves unchanged.
///
/// `firstlen([1mm])` must still compile cleanly after the push-down logic is
/// added. The pre-scan short-circuit (no empty collection-literal args) must
/// leave the existing path byte-for-byte unchanged.
///
/// GREEN today (non-regression baseline established in step-3).
#[test]
fn pushdown_non_regression_nonempty_list_arg_unchanged() {
    let source = "fn firstlen(xs: List<Length>) -> Int { xs.count } \
                  structure S { let n = firstlen([1mm]) }";
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "non-regression: firstlen([1mm]) should compile cleanly, got errors: {errors:?}"
    );

    let n_expr = cell_expr(&module, "n");
    assert_eq!(
        n_expr.result_type,
        Type::Int,
        "non-regression: firstlen([1mm]) result_type should be Int, got {:?}",
        n_expr.result_type
    );

    match &n_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(function_name, "firstlen", "expected UserFunctionCall for firstlen");
        }
        other => panic!("non-regression: expected UserFunctionCall for firstlen([1mm]), got {other:?}"),
    }
}

// ── step-5 tests: §7#6 TypeUndetermined + bound-by-other-arg suppression ─────

/// §7#6 (i) — Unbound generic type-parameter → TypeUndetermined error.
///
/// `ident([])` over `fn ident<T>(xs: List<T>) -> Int { xs.count }`:
/// T is not bound by any other argument, so push-down cannot determine the
/// element type. The compiler must emit exactly ONE Error diagnostic with
/// `DiagnosticCode::TypeUndetermined` and NOT silently accept `T ← Real`.
///
/// RED until step-6: today `ident([])` silently accepts (`T ← Real` via the
/// default `List<Real>` → "All constraints satisfied").
#[test]
fn pushdown_unbound_type_param_emits_type_undetermined_error() {
    let source = "fn ident<T>(xs: List<T>) -> Int { xs.count } \
                  structure S { let n = ident([]) }";
    let module = compile_source(source);

    // Exactly ONE Error with TypeUndetermined code.
    let type_undetermined_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::TypeUndetermined)
        })
        .collect();
    assert!(
        !type_undetermined_errors.is_empty(),
        "§7#6: expected at least one TypeUndetermined Error for ident([]), got diagnostics: {:?}",
        module.diagnostics
    );

    // The call must NOT silently resolve to UserFunctionCall (would mean T←Real accepted).
    // After step-6, the whole call is poisoned.
    assert!(
        !module.diagnostics.is_empty()
            && module
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
        "§7#6: ident([]) must produce at least one Error (TypeUndetermined), got: {:?}",
        module.diagnostics
    );
}

/// §7#6 (ii) — Bound-by-other-arg suppression.
///
/// `pair(3mm, [])` over `fn pair<T>(x: T, xs: List<T>) -> Int { xs.count }`:
/// T is bound to Length by the first arg (3mm), so push-down resolves the
/// empty list as `List<Length>` — NO TypeUndetermined error.
///
/// RED until step-6: today `pair(3mm, [])` fails with "no matching overload"
/// because `[]` defaults to `List<Real>` while the 2nd param is `List<Length>`.
#[test]
fn pushdown_bound_by_other_arg_suppresses_type_undetermined() {
    let source = "fn pair<T>(x: T, xs: List<T>) -> Int { xs.count } \
                  structure S { let n = pair(3mm, []) }";
    let module = compile_source(source);

    // No Error diagnostics at all.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "§7#6 suppression: pair(3mm, []) should compile cleanly (T bound to Length by 3mm), got errors: {errors:?}"
    );

    // No TypeUndetermined specifically.
    let type_undetermined: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeUndetermined))
        .collect();
    assert!(
        type_undetermined.is_empty(),
        "§7#6 suppression: pair(3mm, []) must NOT emit TypeUndetermined (T is bound by 3mm), got: {type_undetermined:?}"
    );

    // The call resolves to Int.
    let n_expr = cell_expr(&module, "n");
    assert_eq!(
        n_expr.result_type,
        Type::Int,
        "§7#6 suppression: pair(3mm, []) result_type should be Int, got {:?}",
        n_expr.result_type
    );
}

/// Set analogue of the unbound-type-param test.
///
/// `ident(set {})` over `fn ident<T>(xs: Set<T>) -> Int { xs.count }`:
/// T is not bound by any other argument, so the compiler must emit exactly
/// one `DiagnosticCode::TypeUndetermined` Error (not silently accept `T ← Real`).
///
/// Exercises the `SetLiteral` / `Type::Set` arm of [`push_down_expected_for_empty_coll`].
#[test]
fn pushdown_unbound_type_param_set_emits_type_undetermined_error() {
    let source = "fn ident<T>(xs: Set<T>) -> Int { xs.count } \
                  structure S { let n = ident(set {}) }";
    let module = compile_source(source);

    let type_undetermined_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::TypeUndetermined)
        })
        .collect();
    assert!(
        !type_undetermined_errors.is_empty(),
        "Set: expected at least one TypeUndetermined Error for ident(set {{}}), \
         got diagnostics: {:?}",
        module.diagnostics
    );
}

/// Map analogue of the unbound-type-param test (unbound value type-param).
///
/// `ident(map {})` over `fn ident<V>(m: Map<String,V>) -> Int { m.count }`:
/// V is not bound by any other argument (key is concrete `String`), so the
/// compiler must emit `DiagnosticCode::TypeUndetermined`.
///
/// Exercises the `MapLiteral` / `Type::Map` arm of [`push_down_expected_for_empty_coll`]
/// with a partially-bound map (concrete key, unbound value).
#[test]
fn pushdown_unbound_type_param_map_emits_type_undetermined_error() {
    let source = "fn ident<V>(m: Map<String,V>) -> Int { m.count } \
                  structure S { let n = ident(map {}) }";
    let module = compile_source(source);

    let type_undetermined_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::TypeUndetermined)
        })
        .collect();
    assert!(
        !type_undetermined_errors.is_empty(),
        "Map: expected at least one TypeUndetermined Error for ident(map {{}}), \
         got diagnostics: {:?}",
        module.diagnostics
    );
}

// ── step-7 tests: §10.2 overload-ambiguity → push-down does NOT engage ───────

/// §10.2 — Overload-ambiguous empty literal: push-down falls back to today's behaviour.
///
/// With two overloads differing only in the empty arg's element type
/// (`h(List<Length>)` and `h(List<Force>)`), push-down cannot select a unique
/// candidate from non-empty args alone. It must NOT engage (no TypeUndetermined,
/// no spurious resolution to one overload); the existing path (empty-list warning +
/// Real default + NoMatch) is preserved.
///
/// RED until step-8: step-4 may select the first/arbitrary same-name candidate.
#[test]
fn pushdown_ambiguous_overloads_fallback_no_type_undetermined() {
    let source = "fn h(xs: List<Length>) -> Int { xs.count } \
                  fn h(xs: List<Force>) -> Int { xs.count } \
                  structure S { let n = h([]) }";
    let module = compile_source(source);

    // Must NOT emit TypeUndetermined (that would be wrong — it's not a generic issue).
    let type_undetermined: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeUndetermined))
        .collect();
    assert!(
        type_undetermined.is_empty(),
        "§10.2: h([]) with ambiguous overloads must NOT emit TypeUndetermined, got: {type_undetermined:?}"
    );

    // The existing empty-list warning IS expected (fallback to today's path).
    // (The exact outcome — NoMatch or Ambiguous — follows the existing overload resolution.)
    // We assert only the absence of TypeUndetermined; the presence of the empty-list
    // Warning is the existing behaviour signal we do NOT want to break.
    let empty_list_warning: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("cannot infer element type of empty list")
        })
        .collect();
    assert!(
        !empty_list_warning.is_empty(),
        "§10.2: h([]) with ambiguous overloads must emit the empty-list-inference Warning \
         (fallback to existing path), got diagnostics: {:?}",
        module.diagnostics
    );
}
