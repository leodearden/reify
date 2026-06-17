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
