//! Collection sub-structure evaluation tests (task 64).
//!
//! Tests for evaluating collection sub-components (`sub bolts : List<Bolt>`),
//! count-based elaboration, and count re-elaboration.

use reify_eval::Engine;
use reify_eval::graph::EvaluationGraph;
use reify_test_support::builders::value_ref_typed;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::*;

// ─── step-7: collection sub elaboration in from_templates ───

#[test]
fn from_templates_creates_collection_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let graph = EvaluationGraph::from_templates(&[parent, bolt]);

    // Verify 4 scoped instances exist: Parent.bolts[0].diameter through Parent.bolts[3].diameter
    for i in 0..4 {
        let scoped_entity = format!("Parent.bolts[{}]", i);
        let scoped_id = ValueCellId::new(&scoped_entity, "diameter");
        assert!(
            graph.value_cells.contains_key(&scoped_id),
            "expected scoped value cell {} to exist in graph",
            scoped_id,
        );
    }

    // Verify no instance [4] exists
    let no_instance = ValueCellId::new("Parent.bolts[4]", "diameter");
    assert!(
        !graph.value_cells.contains_key(&no_instance),
        "should not have bolts[4]",
    );
}

// ─── step-9: Engine.eval() with collection count=4 ───

#[test]
fn eval_collection_sub_produces_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Verify count cell evaluated to Int(4)
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    let count_val = result.values.get(&count_id);
    assert_eq!(
        count_val,
        Some(&Value::Int(4)),
        "count cell should evaluate to Int(4)"
    );

    // Verify 4 instances have diameter = 0.01 (10mm)
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        let val = result.values.get(&scoped_id);
        assert_eq!(
            val,
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should be 10mm = 0.01m",
            i
        );
    }

    // No bolts[4]
    let no_instance = ValueCellId::new("Parent.bolts[4]", "diameter");
    assert!(
        result.values.get(&no_instance).is_none(),
        "should not have bolts[4]"
    );
}

// ─── step-11: count Undef means no instances ───

#[test]
fn eval_collection_sub_undef_count_no_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: count cell depends on an Undef param (no default)
    // __count_bolts = ValueRef(Parent.n), but n has no default -> Undef
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param("Parent", "n", Type::Int, None) // no default -> Undef
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Count cell should be Undef
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    let count_val = result.values.get(&count_id);
    assert_eq!(
        count_val,
        Some(&Value::Undef),
        "count cell should be Undef when param n has no default"
    );

    // No instances should exist
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}] should not exist when count is Undef",
            i
        );
    }
}

// ─── step-13: count change 4->6 triggers re-elaboration ───

#[test]
fn edit_param_count_change_re_elaborates_collection() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let n_id = ValueCellId::new("Parent", "n");
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with n=4
    let initial_result = engine.eval(&module);
    let initial_fingerprint = engine.snapshot().unwrap().topology_fingerprint;

    // Verify 4 instances exist initially
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            initial_result.values.get(&scoped_id).is_some(),
            "initially bolts[{}] should exist",
            i
        );
    }

    // Edit n from 4 to 6
    let edit_result = engine
        .edit_param(n_id, Value::Int(6))
        .expect("edit_param should succeed");

    // (1) topology_fingerprint should change
    let new_fingerprint = engine.snapshot().unwrap().topology_fingerprint;
    assert_ne!(
        initial_fingerprint, new_fingerprint,
        "topology_fingerprint should change when count changes"
    );

    // (2) 6 instances should now exist
    for i in 0..6 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        let val = edit_result.values.get(&scoped_id);
        assert_eq!(
            val,
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should be 10mm after count change to 6",
            i
        );
    }

    // (3) No bolts[6] should exist
    let no_instance = ValueCellId::new("Parent.bolts[6]", "diameter");
    assert!(
        edit_result.values.get(&no_instance).is_none(),
        "should not have bolts[6]"
    );
}

// ─── step-21: count change 4→2, stale instances removed ───

#[test]
fn edit_param_count_decrease_removes_stale_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let n_id = ValueCellId::new("Parent", "n");
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with n=4
    let _initial = engine.eval(&module);

    // Edit n from 4 to 2
    let result = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit_param should succeed");

    // bolts[0] and bolts[1] should still have values
    for i in 0..2 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should remain after count decrease",
            i
        );
    }

    // bolts[2] and bolts[3] should be gone (not just overwritten)
    for i in 2..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}].diameter should be removed after count decreased from 4 to 2",
            i
        );
    }
}

// ─── step-19: collection value aggregation ───

#[test]
fn eval_collection_list_aggregation() {
    // Bolt template: param grade : Real = 8.8
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "grade",
            Type::Real,
            Some(CompiledExpr::literal(Value::Real(8.8), Type::Real)),
        )
        .build();

    // Parent template: param n=3, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(3), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        // Let binding that references the per-member synthetic list of bolt grades
        .let_binding(
            "Parent",
            "grades",
            Type::List(Box::new(Type::Real)),
            CompiledExpr::value_ref(
                ValueCellId::new("Parent", "__list_bolts__grade"),
                Type::List(Box::new(Type::Real)),
            ),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // The per-member synthetic __list_bolts__grade cell should exist as a List of instance grade values.
    // Since Bolt has one param (grade=8.8), the list is [Real(8.8), Real(8.8), Real(8.8)].
    let list_id = ValueCellId::new("Parent", "__list_bolts__grade");
    let list_val = result.values.get(&list_id);
    assert!(
        list_val.is_some(),
        "should have synthetic __list_bolts__grade cell with a List value"
    );

    match list_val.unwrap() {
        Value::List(items) => {
            assert_eq!(items.len(), 3, "should have 3 items in bolt grade list");
            for item in items {
                assert_eq!(item, &Value::Real(8.8), "each bolt grade should be 8.8");
            }
        }
        other => panic!("expected List, got {:?}", other),
    }

    // The grades let binding (which references __list_bolts__grade) should have the same value
    let grades_id = ValueCellId::new("Parent", "grades");
    let grades_val = result.values.get(&grades_id);
    assert!(grades_val.is_some(), "should have grades value cell");
    match grades_val.unwrap() {
        Value::List(items) => {
            assert_eq!(items.len(), 3, "grades should have 3 items");
            for item in items {
                assert_eq!(item, &Value::Real(8.8), "each grade should be 8.8");
            }
        }
        other => panic!("expected grades to be a List, got {:?}", other),
    }
}

// ─── step-27: dynamic index e2e eval ───

#[test]
fn eval_dynamic_index_collection_member_access_from_source() {
    // End-to-end: compile source with bolts[idx].diameter and eval.
    // If the compiler emits Literal(Undef) for the collection base, the result will be Undef.
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S {
            param idx : Int = 0
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[idx].diameter
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // d = bolts[0].diameter should be 10mm = 0.01m, NOT Undef
    let d_id = ValueCellId::new("S", "d");
    let d_val = result.values.get(&d_id);
    assert!(
        d_val.is_some() && d_val != Some(&Value::Undef),
        "bolts[idx].diameter should evaluate to the actual diameter, not Undef. Got: {:?}",
        d_val
    );
    assert_eq!(
        d_val,
        Some(&Value::length(0.01)),
        "bolts[0].diameter should be 10mm = 0.01m"
    );
}

// ─── step-29: collection sub as standalone identifier e2e eval ───

#[test]
fn eval_collection_aggregate_from_source() {
    // End-to-end: compile and eval `let grades = bolts` where bolts is a collection sub.
    // This tests the compiler path for bare collection sub identifier resolution,
    // NOT the builder API that step-19 uses.
    let source = r#"
        structure Bolt { param grade : Scalar = 8.8 }
        structure S {
            sub bolts : List<Bolt>
            constraint bolts.count == 3
            let grades = bolts
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // grades should be a List of 3 values (the bolt grades), not Undef
    let grades_id = ValueCellId::new("S", "grades");
    let grades_val = result.values.get(&grades_id);
    assert!(
        grades_val.is_some() && grades_val != Some(&Value::Undef),
        "grades should be a List of bolt values, not Undef. Got: {:?}",
        grades_val
    );

    match grades_val.unwrap() {
        Value::List(items) => {
            assert_eq!(items.len(), 3, "should have 3 items in grades list");
        }
        other => panic!("expected List, got {:?}", other),
    }
}

// ─── task-826 step-3: warning emitted when Undef guard skips re-elaboration ───

#[test]
fn edit_param_count_to_undef_emits_warning() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let n_id = ValueCellId::new("Parent", "n");
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with n=4
    let _initial = engine.eval(&module);

    // Edit n to Undef — count cell becomes Undef, guard fires
    let result = engine
        .edit_param(n_id, Value::Undef)
        .expect("edit_param should succeed");

    // A warning diagnostic should be emitted describing the skip
    let has_undef_count_warning = result.diagnostics.iter().any(|d| {
        d.severity == reify_types::Severity::Warning
            && d.message.contains("__count_bolts")
    });
    assert!(
        has_undef_count_warning,
        "expected a warning diagnostic mentioning '__count_bolts' when count becomes Undef, got: {:?}",
        result.diagnostics
    );
}

// ─── task-826 step-1: setting count param to Undef preserves existing instances ───

#[test]
fn edit_param_count_to_undef_preserves_existing_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let n_id = ValueCellId::new("Parent", "n");
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with n=4 → 4 bolt instances
    let initial = engine.eval(&module);
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            initial.values.get(&scoped_id).is_some(),
            "initially bolts[{}] should exist",
            i
        );
    }

    // Edit n to Undef: count cell becomes Undef after re-evaluation.
    // The guard should skip re-elaboration and preserve the existing 4 instances.
    let result = engine
        .edit_param(n_id, Value::Undef)
        .expect("edit_param should succeed");

    // Existing instances should be preserved, not destroyed
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_some(),
            "bolts[{}] should be preserved when count cell becomes Undef (not destroyed by treating Undef as 0)",
            i
        );
    }
}
