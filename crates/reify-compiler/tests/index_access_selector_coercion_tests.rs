//! Compiler integration tests for **task 4118 γ**, ResolveSelector insertion
//! site #3: **`IndexAccess`-object coercion** (`selector(...)[i]`).
//!
//! After step-4 re-typed the constructors, `faces(b)` / `faces_by_normal(...)`
//! are `Selector(k)`, not `List<Geometry>`. Indexing a `Selector` would hit the
//! IndexAccess arm's "cannot index into non-collection type" hard error. The
//! coercion wraps the indexed `object` in a `CompiledExprKind::ResolveSelector`
//! node (`result_type == List<Geometry>`), after which the element type resolves
//! to `Geometry` — preserving the 4315 `selector(...)[i]` shape (the curvature
//! `faces(s)[0]` form) one-directionally (PRD §4.4 / task 4117 β
//! `type_compatible`).
//!
//! RED until step-12 extends the IndexAccess compile arm.

use reify_core::Type;
use reify_ir::{CompiledExpr, CompiledExprKind};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// `faces(b)[0]` (All-leaf face selector) and `faces_by_normal(b,...)[0]`
/// (predicate face selector) — both indexed.
const SOURCE_INDEXED_FACE_SELECTORS: &str = r#"
structure def IndexedFaceSelectors {
    let b = box(10mm, 10mm, 10mm)
    let f0 = faces(b)[0]
    let n0 = faces_by_normal(b, [0, 0, 1], 1deg)[0]
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

/// Assert the named cell is an `IndexAccess` whose `object` is wrapped in
/// `ResolveSelector` (result_type `List<Geometry>`, inner still `Selector(k)`)
/// and whose own result_type is `Geometry`.
fn assert_indexed_selector_coerced(
    compiled: &reify_compiler::CompiledModule,
    member: &str,
) {
    let expr = cell_default_expr(compiled, member);
    let CompiledExprKind::IndexAccess { object, .. } = &expr.kind else {
        panic!(
            "expected `{member}` to compile to an IndexAccess, got {:?}",
            expr.kind
        );
    };
    let CompiledExprKind::ResolveSelector { selector } = &object.kind else {
        panic!(
            "expected the IndexAccess object of `{member}` to be wrapped in \
             ResolveSelector, got {:?}",
            object.kind
        );
    };
    assert_eq!(
        object.result_type,
        Type::List(Box::new(Type::Geometry)),
        "the ResolveSelector object must carry result_type List<Geometry>"
    );
    assert!(
        matches!(selector.result_type, Type::Selector(_)),
        "the inner (wrapped) expression must be a Selector(k), got {:?}",
        selector.result_type
    );
    assert_eq!(
        expr.result_type,
        Type::Geometry,
        "indexing a coerced selector must yield element type Geometry"
    );
}

/// (a) `selector(...)[i]` compiles WITHOUT the non-collection hard error.
#[test]
fn indexed_face_selectors_compile_without_errors() {
    let compiled = compile_source_with_stdlib(SOURCE_INDEXED_FACE_SELECTORS);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "indexing a Selector must coerce cleanly (no `cannot index into \
         non-collection type` error), got: {errors:#?}"
    );
}

/// (b) `faces(b)[0]` — the All-leaf shape — wraps the object and yields Geometry.
#[test]
fn faces_index_object_is_wrapped_in_resolve_selector() {
    let compiled = compile_source_with_stdlib(SOURCE_INDEXED_FACE_SELECTORS);
    assert_indexed_selector_coerced(&compiled, "f0");
}

/// (b') `faces_by_normal(...)[0]` — the predicate-leaf shape — same coercion.
#[test]
fn faces_by_normal_index_object_is_wrapped_in_resolve_selector() {
    let compiled = compile_source_with_stdlib(SOURCE_INDEXED_FACE_SELECTORS);
    assert_indexed_selector_coerced(&compiled, "n0");
}

/// (c) Genuinely non-indexable, non-selector objects still hard-error: indexing
/// a bare scalar must keep emitting the non-collection diagnostic (proves the
/// coercion did not weaken the guard for non-selector types).
const SOURCE_INDEX_NON_COLLECTION: &str = r#"
structure def IndexNonCollection {
    let bad = (3mm)[0]
}
"#;

#[test]
fn indexing_non_selector_non_collection_still_errors() {
    let compiled = compile_source_with_stdlib(SOURCE_INDEX_NON_COLLECTION);
    let errors = errors_only(&compiled);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("cannot index into non-collection type")),
        "indexing a non-collection, non-selector value must still error; got: {errors:#?}"
    );
}
