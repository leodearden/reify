//! Integration tests for meta block access in the evaluator.
//!
//! Tests that MetaAccess expressions in let bindings, sub-component contexts,
//! edit_param paths, and cached eval all resolve correctly when the Engine
//! wires meta_map into EvalContext.

use reify_eval::Engine;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::*;

/// step-3: Parent can access child template meta via meta_access("Child", key).
///
/// Build two templates:
///   - 'Part' with meta {"material": "steel"} and a param size
///   - 'Assembly' with sub_component 'part' -> 'Part' and a Let binding
///     using meta_access("Part", "material")
/// After eval(), the Assembly's let cell should hold Value::String("steel").
#[test]
fn eval_meta_access_sub_structure() {
    let label_id = ValueCellId::new("Assembly", "label");
    let meta_expr = CompiledExpr::meta_access("Part".to_string(), "material".to_string());

    let part = TopologyTemplateBuilder::new("Part")
        .meta(
            [("material".to_string(), "steel".to_string())]
                .into_iter()
                .collect(),
        )
        .param(
            "Part",
            "size",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    let assembly = TopologyTemplateBuilder::new("Assembly")
        .sub_component("part", "Part", vec![])
        .let_binding("Assembly", "label", Type::String, meta_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(part)
        .template(assembly)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&label_id),
        Some(&Value::String("steel".to_string())),
        "Assembly let binding with MetaAccess(Part, material) should resolve to 'steel'"
    );
}

/// step-5: meta_access survives edit_param — let binding with MetaAccess is
/// stable after editing an unrelated param.
///
/// Build template 'Box' with meta {"tag": "v1"}, a Param 'size' with default
/// 10.0 and a Let 'label' whose expr is meta_access("Box", "tag").
/// After eval() then edit_param(size, 20.0), 'label' should still hold "v1".
#[test]
fn eval_meta_access_survives_edit_param() {
    let size_id = ValueCellId::new("Box", "size");
    let label_id = ValueCellId::new("Box", "label");
    let meta_expr = CompiledExpr::meta_access("Box".to_string(), "tag".to_string());

    let template = TopologyTemplateBuilder::new("Box")
        .meta(
            [("tag".to_string(), "v1".to_string())]
                .into_iter()
                .collect(),
        )
        .param(
            "Box",
            "size",
            Type::Real,
            Some(CompiledExpr::literal(Value::Real(10.0), Type::Real)),
        )
        .let_binding("Box", "label", Type::String, meta_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&module);

    // edit_param: change size from 10.0 to 20.0
    let result = engine
        .edit_param(size_id, Value::Real(20.0))
        .expect("edit_param should succeed");

    assert_eq!(
        result.values.get(&label_id),
        Some(&Value::String("v1".to_string())),
        "meta label should survive edit_param unchanged"
    );
}

/// step-1: A let binding using MetaAccess resolves to the meta value.
///
/// Build template 'Widget' with meta {"description": "A widget"} and a
/// Let binding whose expr is meta_access("Widget", "description").
/// After eval(), the let cell should hold Value::String("A widget").
#[test]
fn eval_meta_access_in_let_binding() {
    let label_id = ValueCellId::new("Widget", "label");
    let meta_expr = CompiledExpr::meta_access("Widget".to_string(), "description".to_string());

    let template = TopologyTemplateBuilder::new("Widget")
        .meta(
            [("description".to_string(), "A widget".to_string())]
                .into_iter()
                .collect(),
        )
        .let_binding("Widget", "label", Type::String, meta_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&label_id),
        Some(&Value::String("A widget".to_string())),
        "let binding with MetaAccess should resolve to the meta value"
    );
}
