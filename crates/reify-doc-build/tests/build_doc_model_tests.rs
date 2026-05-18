//! Integration tests for `reify_doc_build::build_doc_model`.
//!
//! Each test compiles a small `.ri` source string via
//! `reify_test_support::compile_source_with_stdlib`, then calls
//! `reify_doc_build::build_doc_model(&compiled, source)` and asserts the
//! structure of the returned `DocModel`.

use reify_doc::model::{DocModel, ItemKind};
use reify_doc_build::build_doc_model;
use reify_test_support::compile_source_with_stdlib;

// ---------------------------------------------------------------------------
// step-1: structure with params and constraints
// ---------------------------------------------------------------------------

/// Compile a small source with one `pub structure` that has params (with defaults)
/// and named constraints.  Asserts that `build_doc_model` returns a
/// `DocModel` with the correct module path and the expected `ItemKind::Structure`
/// payload.
#[test]
fn structure_with_params_and_constraints() {
    let source = r#"
pub structure Widget {
    param width: Scalar = 10mm
    param height: Scalar = 20mm
    param depth: Scalar
    constraint depth > 0mm
    constraint width >= height
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let model: DocModel = build_doc_model(&compiled, source);

    // There should be exactly one ModuleDoc.
    assert_eq!(model.modules.len(), 1, "expected one module");
    let module = &model.modules[0];

    // Module path should match the compiled module's path (empty for unnamed source).
    assert_eq!(module.path, compiled.path.to_string());

    // There should be exactly one item: Widget.
    assert_eq!(module.items.len(), 1, "expected one item");
    let item = &module.items[0];

    // Header fields.
    assert_eq!(item.header.name, "Widget");
    assert!(item.header.is_pub, "Widget is pub");

    // It should be a Structure.
    let (params, constraints) = match &item.kind {
        ItemKind::Structure {
            params,
            constraints,
            ..
        } => (params, constraints),
        other => panic!("expected Structure, got {other:?}"),
    };

    // Params: width, height, depth (in source order).
    assert_eq!(params.len(), 3, "expected 3 params; got {params:?}");
    assert_eq!(params[0].name, "width");
    assert!(
        params[0].default_repr.is_some(),
        "width has a default (10mm)"
    );
    assert_eq!(params[1].name, "height");
    assert!(
        params[1].default_repr.is_some(),
        "height has a default (20mm)"
    );
    assert_eq!(params[2].name, "depth");
    assert!(
        params[2].default_repr.is_none(),
        "depth has no default"
    );

    // Constraints: two entries.
    assert_eq!(constraints.len(), 2, "expected 2 constraints; got {constraints:?}");

    // constraint 0: expr_repr is the span-sliced text of "depth > 0mm"
    assert!(
        constraints[0].expr_repr.contains("depth"),
        "first constraint expr_repr must mention 'depth', got: {:?}",
        constraints[0].expr_repr
    );
    // constraint 1: expr_repr is "width >= height"
    assert!(
        constraints[1].expr_repr.contains("width"),
        "second constraint expr_repr must mention 'width', got: {:?}",
        constraints[1].expr_repr
    );

    // line numbers: both constraints must have Some(line) and be >= 1.
    for (i, c) in constraints.iter().enumerate() {
        assert!(
            c.line.is_some(),
            "constraint[{i}].line must be Some, got None"
        );
        assert!(
            c.line.unwrap() >= 1,
            "constraint[{i}].line must be >= 1, got {:?}",
            c.line
        );
    }
}
