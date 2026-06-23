//! Compiler integration tests for **task 4118 γ**, ResolveSelector insertion
//! site #1: **param-binding coercion**.
//!
//! A `Selector(k)` argument fed to a `List<Geometry>` function parameter must
//! coerce one-directionally (PRD §4.4 / task 4117 β `type_compatible` rule) by
//! wrapping the compiled argument in a `CompiledExprKind::ResolveSelector`
//! coercion node (`result_type == List<Geometry>`).
//!
//! Ground truth (verified before step-8): `resolve_function_overload` uses exact
//! type equality, so the call is an `OverloadResolution::NoMatch` until the
//! NoMatch-arm selector-coercion retry lands in step-8 (mirrors the existing
//! `try_default_padding` secondary-resolution precedent). These tests are RED
//! until then:
//!   - the positive case currently errors ("no matching overload …
//!     takes_faces(FaceSelector)") and the cell is a poison literal;
//!   - the negative (wrong-kind) case must STAY a no-match both before and after.

use reify_core::Type;
use reify_core::diagnostics::DiagnosticCode;
use reify_ir::{CompiledExpr, CompiledExprKind};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// A user fn `takes_faces(g: List<Geometry>)` called with a `Selector(Face)`
/// argument (`faces_by_normal(...)`). After step-8 this compiles cleanly and the
/// argument is wrapped in `ResolveSelector`.
const SOURCE_FACE_SELECTOR_ARG: &str = r#"
fn takes_faces(g: List<Geometry>) -> Int {
    42
}

structure def ParamCoerceFaceArg {
    let b = box(10mm, 10mm, 10mm)
    let n = takes_faces(faces_by_normal(b, [0, 0, 1], 1deg))
}
"#;

/// Locate the `default_expr` of the named value cell in the first template.
fn cell_default_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a CompiledExpr {
    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("expected '{member}' value cell"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("expected '{member}' cell to have a default_expr"))
}

/// (a) The coerced call compiles WITHOUT any error-severity diagnostic.
#[test]
fn param_binding_face_selector_arg_compiles_without_errors() {
    let compiled = compile_source_with_stdlib(SOURCE_FACE_SELECTOR_ARG);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "Selector(Face) arg fed to a List<Geometry> param must coerce cleanly, \
         got errors: {errors:#?}"
    );
}

/// (b) The compiled argument is wrapped in `ResolveSelector` with
/// `result_type == List<Geometry>`, and the inner selector is the
/// `faces_by_normal` FunctionCall typed `Selector(Face)`.
#[test]
fn param_binding_face_selector_arg_is_wrapped_in_resolve_selector() {
    let compiled = compile_source_with_stdlib(SOURCE_FACE_SELECTOR_ARG);
    let n_expr = cell_default_expr(&compiled, "n");

    let CompiledExprKind::UserFunctionCall { args, .. } = &n_expr.kind else {
        panic!(
            "expected `n` to compile to a UserFunctionCall, got {:?}",
            n_expr.kind
        );
    };
    assert_eq!(args.len(), 1, "takes_faces takes exactly one argument");
    let arg = &args[0];

    let CompiledExprKind::ResolveSelector { selector } = &arg.kind else {
        panic!(
            "expected the List<Geometry> argument to be wrapped in ResolveSelector, \
             got {:?}",
            arg.kind
        );
    };
    assert_eq!(
        arg.result_type,
        Type::List(Box::new(Type::Geometry)),
        "the ResolveSelector node must carry result_type List<Geometry>"
    );
    // The wrapped inner expression is the selector constructor, still typed
    // Selector(Face) — the coercion does not rewrite the constructor.
    assert!(
        matches!(selector.result_type, Type::Selector(_)),
        "the inner (wrapped) expression must be a Selector(k), got {:?}",
        selector.result_type
    );
}

/// (c) Wrong-kind / non-coercible case is UNAFFECTED: a `Selector` arg fed to a
/// `List<Real>` param (NOT List<Geometry>) must remain a no-match error, proving
/// the β coercion is kind-gated and does not over-match.
const SOURCE_WRONG_KIND_PARAM: &str = r#"
fn takes_reals(g: List<Real>) -> Int {
    7
}

structure def ParamCoerceWrongKind {
    let b = box(10mm, 10mm, 10mm)
    let n = takes_reals(faces_by_normal(b, [0, 0, 1], 1deg))
}
"#;

#[test]
fn param_binding_selector_arg_to_non_geometry_list_param_still_errors() {
    let compiled = compile_source_with_stdlib(SOURCE_WRONG_KIND_PARAM);
    let errors = errors_only(&compiled);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("no matching overload")),
        "a Selector arg must NOT coerce to a List<Real> param; expected a \
         no-matching-overload error, got: {errors:#?}"
    );
}

// ── task 4581: param-binding selector-kind mismatch → SelectorKindMismatch ──

/// Source for BT6-style kind mismatch: `needs_face(FaceSelector)` called with
/// an `edges(b)` argument (EdgeSelector). RED until the `is_selector_kind_mismatch_nomatch`
/// classifier is wired in the NoMatch arm (step-2).
const SOURCE_WRONG_SELECTOR_KIND: &str = r#"
fn needs_face(s: FaceSelector) -> Int { 42 }

structure def KindMismatchParam {
    let b = box(10mm, 10mm, 10mm)
    let n = needs_face(edges(b))
}
"#;

/// (task 4581, step-1) A wrong-kind Selector→Selector param mismatch must be
/// tagged with `DiagnosticCode::SelectorKindMismatch` (BT1↔BT6 uniformity).
///
/// RED today: the NoMatch arm emits code = None for this case. Step-2 wires the
/// `is_selector_kind_mismatch_nomatch` classifier which makes it GREEN.
#[test]
fn param_binding_wrong_kind_selector_tagged_with_selector_kind_mismatch() {
    let compiled = compile_source_with_stdlib(SOURCE_WRONG_SELECTOR_KIND);
    let errors = errors_only(&compiled);

    // (a) exactly ONE error
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for wrong-kind selector param, got {} errors:\n{errors:#?}",
        errors.len(),
    );

    let err = errors[0];

    // (b) carries SelectorKindMismatch — RED until step-2
    assert_eq!(
        err.code,
        Some(DiagnosticCode::SelectorKindMismatch),
        "wrong-kind Selector→Selector param must carry DiagnosticCode::SelectorKindMismatch \
         (task 4581 / esc-4120-17), got: {:?}",
        err.code
    );

    // (c) message names BOTH the expected kind and the found kind.
    // Note: "FaceSelector" and "EdgeSelector" are the Display strings for
    // `SelectorKind::Face` and `SelectorKind::Edge` (see reify-core/src/ty.rs).
    // If `SelectorKind`'s Display impl changes (e.g. to "Selector<Face>"), update
    // the substrings here alongside the Display change.
    assert!(
        err.message.contains("FaceSelector"),
        "error message must name FaceSelector (expected kind), got: {:?}",
        err.message
    );
    assert!(
        err.message.contains("EdgeSelector"),
        "error message must name EdgeSelector (found kind), got: {:?}",
        err.message
    );
}

/// (task 4581, over-tag guard) A Selector→`List<Real>` no-match must NOT be
/// tagged with `SelectorKindMismatch` — only the Selector→Selector kind mismatch
/// case qualifies.
///
/// Reuses `SOURCE_WRONG_KIND_PARAM` (takes_reals / `List<Real>` param). GREEN
/// both before and after step-2 because `is_selector_kind_mismatch_nomatch`
/// requires BOTH param and arg to be `Type::Selector`.
#[test]
fn param_binding_selector_to_list_real_no_match_keeps_code_none() {
    let compiled = compile_source_with_stdlib(SOURCE_WRONG_KIND_PARAM);
    let errors = errors_only(&compiled);

    // Find the no-matching-overload error
    let no_match = errors
        .iter()
        .find(|d| d.message.contains("no matching overload"))
        .expect("expected a no-matching-overload error for Selector→List<Real>");

    assert_eq!(
        no_match.code,
        None,
        "Selector→List<Real> no-match must keep code = None (not SelectorKindMismatch); \
         got: {:?}",
        no_match.code
    );
}
