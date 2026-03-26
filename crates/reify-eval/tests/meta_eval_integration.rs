//! Integration tests for meta block access in the evaluator.
//!
//! Tests that MetaAccess expressions in let bindings, sub-component contexts,
//! edit_param paths, and cached eval all resolve correctly when the Engine
//! wires meta_map into EvalContext.

use reify_compiler::{ValueCellDecl, ValueCellKind, Visibility};
use reify_eval::Engine;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::builders::value_ref_typed;
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

/// step-7: A let member inside a guarded group (guard=true) using meta_access
/// resolves to the meta value.
///
/// Build template 'S' with:
///   - meta {"mode": "active"}
///   - Bool param 'active' (default true)
///   - Guarded group: guard_expr = ValueRef(active), one Let member 'mode_label'
///     whose default_expr is meta_access("S", "mode")
/// After eval(), mode_label should == Value::String("active").
#[test]
fn eval_meta_access_in_guarded_group() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let mode_label_id = ValueCellId::new("S", "mode_label");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let meta_expr = CompiledExpr::meta_access("S".to_string(), "mode".to_string());

    // The guarded member: a Let binding whose expr is meta_access
    let mode_label_decl = ValueCellDecl {
        id: mode_label_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Public,
        cell_type: Type::String,
        default_expr: Some(meta_expr),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .meta(
            [("mode".to_string(), "active".to_string())]
                .into_iter()
                .collect(),
        )
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![mode_label_decl], // members (active when guard=true)
            vec![],                // constraints
            vec![],                // else_members
            vec![],                // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&guard_id),
        Some(&Value::Bool(true)),
        "guard cell should evaluate to true"
    );
    assert_eq!(
        result.values.get(&mode_label_id),
        Some(&Value::String("active".to_string())),
        "guarded Let member with MetaAccess should resolve to 'active' when guard=true"
    );
}

/// step-11: Collection sub-component meta access — parent Let binding can read
/// child template meta via meta_access("Child", key).
///
/// Build two templates:
///   - 'Bolt' with meta {"grade": "A2"} and param 'len'
///   - 'Plate' with:
///       - param 'n' = 3 (count)
///       - let '__count_bolts' = n (count cell)
///       - collection_sub_component 'bolts' -> 'Bolt' (count cell = __count_bolts)
///       - let 'grade_label' = meta_access("Bolt", "grade")
/// After eval(), Plate.grade_label should == Value::String("A2").
#[test]
fn eval_meta_access_collection_sub_component() {
    use reify_test_support::builders::value_ref_typed;

    let grade_label_id = ValueCellId::new("Plate", "grade_label");
    let meta_expr = CompiledExpr::meta_access("Bolt".to_string(), "grade".to_string());

    // Build 'Bolt' template with meta {"grade": "A2"} and param 'len'
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .meta(
            [("grade".to_string(), "A2".to_string())]
                .into_iter()
                .collect(),
        )
        .param(
            "Bolt",
            "len",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.05), Type::length())),
        )
        .build();

    // Build 'Plate' template with collection sub + meta access let binding
    let count_expr = value_ref_typed("Plate", "n", Type::Int);
    let plate = TopologyTemplateBuilder::new("Plate")
        .param(
            "Plate",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(3), Type::Int)),
        )
        .let_binding("Plate", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Plate", "__count_bolts"))
        .collection_sub_component(
            "bolts",
            "Bolt",
            ValueCellId::new("Plate", "__count_bolts"),
        )
        .let_binding("Plate", "grade_label", Type::String, meta_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bolt)
        .template(plate)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&grade_label_id),
        Some(&Value::String("A2".to_string())),
        "Plate let binding with MetaAccess(Bolt, grade) should resolve to 'A2'"
    );
}

/// F6: Both Analytical and Composed field sources evaluate expressions
/// correctly with meta_map wired into EvalContext.
///
/// Build a module with an Analytical field (body = 273.15) and a Composed
/// field (body = 100.0), plus a template with meta and a Let binding using
/// MetaAccess. After eval(), both field values and the meta let binding
/// should resolve correctly.
#[test]
fn eval_field_analytical_and_composed_identical_path() {
    use reify_test_support::CompiledFieldBuilder;

    let analytical_body = CompiledExpr::literal(Value::Real(273.15), Type::Real);
    let composed_body = CompiledExpr::literal(Value::Real(100.0), Type::Real);

    let analytical_field = CompiledFieldBuilder::new("temp_a", Type::Geometry, Type::Real)
        .analytical(analytical_body)
        .build();
    let composed_field = CompiledFieldBuilder::new("temp_c", Type::Geometry, Type::Real)
        .composed(composed_body)
        .build();

    let meta_expr = CompiledExpr::meta_access("Widget".to_string(), "kind".to_string());
    let template = TopologyTemplateBuilder::new("Widget")
        .meta(
            [("kind".to_string(), "sensor".to_string())]
                .into_iter()
                .collect(),
        )
        .let_binding("Widget", "label", Type::String, meta_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .field(analytical_field)
        .field(composed_field)
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Analytical field should evaluate its expression
    let temp_a_id = ValueCellId::new("__field", "temp_a");
    let temp_a = result.values.get(&temp_a_id);
    assert!(temp_a.is_some(), "analytical field should be in values");

    // Composed field should evaluate its expression
    let temp_c_id = ValueCellId::new("__field", "temp_c");
    let temp_c = result.values.get(&temp_c_id);
    assert!(temp_c.is_some(), "composed field should be in values");

    // Meta let binding should resolve
    let label_id = ValueCellId::new("Widget", "label");
    assert_eq!(
        result.values.get(&label_id),
        Some(&Value::String("sensor".to_string())),
        "meta let binding should resolve"
    );
}

/// step-9: eval_cached() resolves MetaAccess expressions correctly.
///
/// Build template 'Widget' with meta {"description": "A widget"} and a
/// Let binding whose expr is meta_access("Widget", "description").
/// Call eval_cached() directly on a fresh engine (no prior eval()).
/// The let cell should resolve to Value::String("A widget").
///
/// This tests that eval_cached() builds meta_map from the module and
/// wires it into EvalContext, just like eval() does.
#[test]
fn eval_meta_access_cached_eval() {
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

    // Call eval_cached directly (no prior eval())
    let result = engine.eval_cached(&module, VersionId(1));

    assert_eq!(
        result.eval_result.values.get(&label_id),
        Some(&Value::String("A widget".to_string())),
        "eval_cached let binding with MetaAccess should resolve to the meta value"
    );
}

/// step-3 (suggestion 13): Guarded group else_members branch with MetaAccess.
///
/// Build a structure 'S' with:
///   - meta {"fallback": "off"}
///   - param 'active' = false (so guard evaluates to false)
///   - guarded group:
///       - members: [mode_label (Let, meta_access "S" "fallback")]
///       - else_members: [alt_label (Let, meta_access "S" "fallback")]
///
/// Since guard=false:
///   - mode_label (member) should be Undef (guard inactive)
///   - alt_label (else_member) should be Value::String("off") (guard active for else)
#[test]
fn eval_meta_access_in_guarded_group_else_branch() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let mode_label_id = ValueCellId::new("S", "mode_label");
    let alt_label_id = ValueCellId::new("S", "alt_label");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let meta_expr_member = CompiledExpr::meta_access("S".to_string(), "fallback".to_string());
    let meta_expr_else = CompiledExpr::meta_access("S".to_string(), "fallback".to_string());

    // Member: active when guard=true
    let mode_label_decl = ValueCellDecl {
        id: mode_label_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Public,
        cell_type: Type::String,
        default_expr: Some(meta_expr_member),
        span: SourceSpan::new(0, 0),
    };

    // Else-member: active when guard=false
    let alt_label_decl = ValueCellDecl {
        id: alt_label_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Public,
        cell_type: Type::String,
        default_expr: Some(meta_expr_else),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .meta(
            [("fallback".to_string(), "off".to_string())]
                .into_iter()
                .collect(),
        )
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![mode_label_decl],   // members (active when guard=true)
            vec![],                   // constraints
            vec![alt_label_decl],     // else_members (active when guard=false)
            vec![],                   // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    assert_eq!(
        result.values.get(&guard_id),
        Some(&Value::Bool(false)),
        "guard cell should evaluate to false"
    );
    assert_eq!(
        result.values.get(&mode_label_id),
        Some(&Value::Undef),
        "members should be Undef when guard=false"
    );
    assert_eq!(
        result.values.get(&alt_label_id),
        Some(&Value::String("off".to_string())),
        "else_members with MetaAccess should resolve to 'off' when guard=false"
    );
}
