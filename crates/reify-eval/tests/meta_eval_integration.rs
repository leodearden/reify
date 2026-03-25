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
