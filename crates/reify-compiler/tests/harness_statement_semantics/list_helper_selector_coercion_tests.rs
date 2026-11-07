//! Compiler integration tests for **task 4118 γ**, ResolveSelector insertion
//! site #2: **`single()` / list-helper coercion**.
//!
//! A `Selector(k)` argument fed to a list-helper whose first parameter is a
//! `List<_>` (`single`) must coerce one-directionally (PRD §4.4 / task 4117 β
//! `type_compatible`) by wrapping the compiled argument in a
//! `CompiledExprKind::ResolveSelector` coercion node
//! (`result_type == List<Geometry>`). The helper's element-type inference then
//! collapses `single(List<Geometry>)` → `Geometry`.
//!
//! Ground truth (verified before step-10): the re-typed constructors (step-4)
//! make `faces_by_normal(...)` a `Selector(Face)`, so without the coercion the
//! `single(...)` cell mis-types as `Selector(Face)` (first-arg fallback) and the
//! argument stays a bare `FunctionCall`. These tests are RED until step-10
//! inserts the coercion at the list-helper call site.

use reify_core::Type;
use reify_ir::{CompiledExpr, CompiledExprKind};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// `single(faces_by_normal(b, [0,0,1], 1deg))` — a `Selector(Face)` arg fed to
/// the `single` list-helper. After step-10 this compiles cleanly, the argument
/// is wrapped in `ResolveSelector`, and the cell type collapses to `Geometry`.
const SOURCE_SINGLE_FACE_SELECTOR: &str = r#"
structure def SingleFaceByNormal {
    let b = box(10mm, 10mm, 10mm)
    let top = single(faces_by_normal(b, [0, 0, 1], 1deg))
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

/// (a) `single(selector)` compiles WITHOUT any error-severity diagnostic.
#[test]
fn single_face_selector_arg_compiles_without_errors() {
    let compiled = compile_source_with_stdlib(SOURCE_SINGLE_FACE_SELECTOR);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "single(Selector(Face)) must coerce cleanly, got errors: {errors:#?}"
    );
}

/// (b) The compiled `single` argument is wrapped in `ResolveSelector` with
/// `result_type == List<Geometry>`, the inner selector stays `Selector(k)`, and
/// `single(...)` infers return type `Geometry`.
#[test]
fn single_face_selector_arg_is_wrapped_and_returns_geometry() {
    let compiled = compile_source_with_stdlib(SOURCE_SINGLE_FACE_SELECTOR);
    let top_expr = cell_default_expr(&compiled, "top");

    let CompiledExprKind::FunctionCall { function, args } = &top_expr.kind else {
        panic!(
            "expected `top` to compile to a stdlib FunctionCall, got {:?}",
            top_expr.kind
        );
    };
    assert_eq!(function.name, "single", "the helper must be `single`");
    assert_eq!(args.len(), 1, "single takes exactly one argument");

    let arg = &args[0];
    let CompiledExprKind::ResolveSelector { selector } = &arg.kind else {
        panic!(
            "expected the single() argument to be wrapped in ResolveSelector, \
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

    // single(List<Geometry>) -> Geometry (the element-type collapse).
    assert_eq!(
        top_expr.result_type,
        Type::Geometry,
        "single() over a coerced selector must infer element type Geometry"
    );
}

/// (c) Non-coercible case is UNAFFECTED: `single` over a `List<Int>` literal
/// stays `single(List<Int>) -> Int` with NO ResolveSelector wrapper, proving the
/// coercion fires only for `Selector` first-args.
const SOURCE_SINGLE_NON_SELECTOR: &str = r#"
structure def SingleNonSelector {
    let only = single([42])
}
"#;

#[test]
fn single_non_selector_arg_is_not_wrapped() {
    let compiled = compile_source_with_stdlib(SOURCE_SINGLE_NON_SELECTOR);
    let only_expr = cell_default_expr(&compiled, "only");

    let CompiledExprKind::FunctionCall { function, args } = &only_expr.kind else {
        panic!(
            "expected `only` to compile to a stdlib FunctionCall, got {:?}",
            only_expr.kind
        );
    };
    assert_eq!(function.name, "single");
    assert_eq!(args.len(), 1);
    assert!(
        !matches!(args[0].kind, CompiledExprKind::ResolveSelector { .. }),
        "a non-selector single() argument must NOT be wrapped in ResolveSelector, \
         got {:?}",
        args[0].kind
    );
    assert_eq!(
        only_expr.result_type,
        Type::Int,
        "single(List<Int>) must still infer Int"
    );
}
