//! Collection sub-structure evaluation tests (task 64).
//!
//! Tests for evaluating collection sub-components (`sub bolts : List<Bolt>`),
//! count-based elaboration, and count re-elaboration.

use std::collections::HashSet;

use reify_eval::cache::{EvalOutcome, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{ConcurrentEditResult, ConcurrentNodeResult, Engine};
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

// ─── task-826 step-6: non-Int non-Undef count emits a warning instead of silently treating as 0 ───

#[test]
fn edit_param_non_int_non_undef_count_emits_warning() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n
    // We'll set n to Value::Real(3.5) to simulate a non-Int non-Undef count
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

    // Edit n to Real(3.5): count cell becomes Real(3.5) (non-Int non-Undef)
    // Instead of silently treating as count=0, a warning should be emitted
    let result = engine
        .edit_param(n_id, Value::Real(3.5))
        .expect("edit_param should succeed");

    // A warning diagnostic should be emitted for the non-Int non-Undef count
    let has_non_int_warning = result.diagnostics.iter().any(|d| {
        d.severity == reify_types::Severity::Warning
            && d.message.contains("__count_bolts")
    });
    assert!(
        has_non_int_warning,
        "expected a warning diagnostic for non-Int non-Undef count cell '__count_bolts', got: {:?}",
        result.diagnostics
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

// ─── task-826 step-5: Undef→Int(4) via edit_param creates instances (regression) ───

#[test]
fn edit_param_count_from_undef_to_int_creates_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n has NO default (Undef), so __count_bolts is Undef initially
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let n_id = ValueCellId::new("Parent", "n");
    let parent = TopologyTemplateBuilder::new("Parent")
        .param("Parent", "n", Type::Int, None) // no default → Undef initially
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

    // Initial eval: count is Undef → no instances
    let initial = engine.eval(&module);
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    assert_eq!(
        initial.values.get(&count_id),
        Some(&Value::Undef),
        "count cell should be Undef initially when n has no default"
    );
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            initial.values.get(&scoped_id).is_none(),
            "no bolt instances should exist initially when count is Undef",
        );
    }

    // Edit n from Undef to Int(4): old_count=Undef(→0), new_count=Int(4) → create 4 instances
    // The guard only fires when new_count is Undef; this path must NOT be blocked.
    let result = engine
        .edit_param(n_id, Value::Int(4))
        .expect("edit_param should succeed");

    // 4 bolt instances should now exist
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should be 10mm after count changes from Undef to Int(4)",
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

// ─── task-826 step-8: Int(4)→Undef→Int(2) must NOT leak stale instances [2..4) ───

#[test]
fn edit_param_count_int_undef_int_no_stale_leak() {
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

    // (a) Initial eval with n=4 → 4 bolt instances
    let initial = engine.eval(&module);
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            initial.values.get(&scoped_id).is_some(),
            "initially bolts[{}] should exist",
            i
        );
    }

    // (b) Edit n to Undef → Undef guard fires, instances preserved (4 still exist)
    let undef_result = engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("edit_param to Undef should succeed");
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            undef_result.values.get(&scoped_id).is_some(),
            "bolts[{}] should be preserved when count becomes Undef",
            i
        );
    }

    // (c) Edit n to Int(2) → should clean up instances [2..4), keeping only [0..2)
    let result = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit_param to Int(2) should succeed");

    // bolts[0] and bolts[1] must exist with correct values
    for i in 0..2 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should exist after recovery to Int(2)",
            i
        );
    }

    // bolts[2] and bolts[3] must be absent (no stale leak)
    for i in 2..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}] must be removed after Int(4)→Undef→Int(2) — stale instances must not leak",
            i
        );
    }
}

// ─── task-826 step-10: Int(4)→Undef→Int(6) removes 4 old instances then creates 6 ───

#[test]
fn edit_param_count_int_undef_int_expand_no_stale() {
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

    // (a) eval with n=4 → 4 instances
    engine.eval(&module);

    // (b) edit to Undef → guard fires, 4 instances preserved
    engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("edit_param to Undef should succeed");

    // (c) edit to Int(6) → preserved count (4) used as old_count; remove [0..4) then create [0..6)
    let result = engine
        .edit_param(n_id, Value::Int(6))
        .expect("edit_param to Int(6) should succeed");

    // All 6 instances must exist with correct values
    for i in 0..6 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should exist after Int(4)→Undef→Int(6)",
            i
        );
    }

    // No bolts[6] should exist
    let no_instance = ValueCellId::new("Parent.bolts[6]", "diameter");
    assert!(
        result.values.get(&no_instance).is_none(),
        "bolts[6] must not exist after Int(4)→Undef→Int(6)"
    );
}

// ─── task-826 step-11: Int(4)→Undef→Int(2)→Undef→Int(1) multi-cycle recovery ───

#[test]
fn edit_param_count_multi_cycle_recovery() {
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

    // (a) eval with n=4 → 4 instances
    engine.eval(&module);

    // (b) Int(4)→Undef → guard fires, preserved_counts[__count_bolts]=4
    engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("edit_param to Undef should succeed");

    // (c) Undef→Int(2) → old_count from preserved_counts=4, removes [2..4), creates [0..2)
    // After this, preserved_counts[__count_bolts] is cleared.
    let after_first_recovery = engine
        .edit_param(n_id.clone(), Value::Int(2))
        .expect("edit_param to Int(2) should succeed");

    // Verify first recovery: bolts[0..2) exist, bolts[2..4) absent
    for i in 0..2 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            after_first_recovery.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "after first recovery: bolts[{}] should exist",
            i
        );
    }
    for i in 2..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            after_first_recovery.values.get(&scoped_id).is_none(),
            "after first recovery: bolts[{}] must be gone",
            i
        );
    }

    // (d) Int(2)→Undef → guard fires again, preserved_counts[__count_bolts]=2
    engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("second edit_param to Undef should succeed");

    // (e) Undef→Int(1) → old_count from preserved_counts=2, removes [1..2), creates [0..1)
    let after_second_recovery = engine
        .edit_param(n_id, Value::Int(1))
        .expect("edit_param to Int(1) should succeed");

    // Only bolts[0] should exist
    let bolt0 = ValueCellId::new("Parent.bolts[0]", "diameter");
    assert_eq!(
        after_second_recovery.values.get(&bolt0),
        Some(&Value::length(0.01)),
        "after second recovery: bolts[0] should exist"
    );

    // bolts[1] and bolts[2..4) must all be gone
    for i in 1..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            after_second_recovery.values.get(&scoped_id).is_none(),
            "after second recovery: bolts[{}] must be absent",
            i
        );
    }
}

// ─── task-826 step-13: Int(4)→Undef→Undef→Int(2): second Undef must not overwrite preserved_counts ───

#[test]
fn edit_param_count_int_undef_undef_int_no_overwrite() {
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

    // (a) eval with n=4 → 4 instances, preserved_counts empty
    engine.eval(&module);

    // (b) edit n→Undef: Undef guard fires, preserved_counts[__count_bolts]=4
    engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("first edit to Undef should succeed");

    // (c) edit n→Undef again: new_count==old_count==Undef → equality check fires,
    //     loop iteration is skipped entirely, preserved_counts must NOT be touched.
    //     If there were a regression (e.g. unconditional insert in the guard), this
    //     would overwrite preserved_counts[__count_bolts] with 0.
    engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("second edit to Undef should succeed");

    // (d) edit n→Int(2): old_count must be recovered from preserved_counts as 4,
    //     so instances [2..4) are removed and only [0..2) remain.
    let result = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit to Int(2) should succeed");

    // bolts[0] and bolts[1] must exist
    for i in 0..2 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should exist after Int(4)→Undef→Undef→Int(2)",
            i
        );
    }

    // bolts[2] and bolts[3] must be absent (old_count was 4, not 0)
    for i in 2..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}] must be removed — preserved_counts must not be overwritten by second Undef",
            i
        );
    }
}

// ─── task-826 step-14: concurrent Int→Undef does NOT populate preserved_counts, causing stale leak ───
// This test is expected to FAIL until step-16 wires re_elaborate_collections into apply_concurrent_edit.

#[test]
fn concurrent_edit_count_to_undef_then_sync_int_no_stale_leak() {
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
    let count_bolts_id = ValueCellId::new("Parent", "__count_bolts");
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

    // (a) eval with n=4 → 4 instances
    engine.eval(&module);

    // (b) concurrent edit: n→Undef via prepare+apply.
    //     The external scheduler evaluates __count_bolts→Undef.
    //     apply_concurrent_edit (pre-fix) does NOT run re_elaborate_collections,
    //     so preserved_counts[__count_bolts] is never set.
    let setup = engine
        .prepare_concurrent_edit(n_id.clone(), Value::Undef)
        .expect("prepare_concurrent_edit should succeed");

    // Simulate scheduler: __count_bolts evaluates to Undef (depends on n=Undef)
    let count_node = NodeId::Value(count_bolts_id.clone());
    let mut snapshot_values = setup.snapshot_values.clone();
    snapshot_values.insert(
        count_bolts_id.clone(),
        (Value::Undef, DeterminacyState::Determined),
    );
    let mut values = setup.values.clone();
    values.insert(count_bolts_id.clone(), Value::Undef);

    let result = ConcurrentEditResult {
        values,
        snapshot_values,
        node_results: vec![ConcurrentNodeResult {
            node: count_node.clone(),
            value: Value::Undef,
            determinacy: DeterminacyState::Determined,
            trace: DependencyTrace {
                reads: vec![n_id.clone()],
            },
            outcome: EvalOutcome::Changed,
        }],
        actual_eval_set: vec![count_node],
        skipped: HashSet::new(),
        resolved_params: std::collections::HashMap::new(),
        diagnostics: Vec::new(),
    };

    engine.apply_concurrent_edit(&setup, result);

    // (c) sync edit_param(n, Int(2)): should clean up instances [2..4), keeping [0..2).
    //     Without the fix, old_count resolves to 0 (preserved_counts empty, Undef fallback),
    //     so removal loop runs 0..0 → stale instances [2..4) remain.
    let final_result = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit_param to Int(2) should succeed");

    // bolts[0..2) must exist
    for i in 0..2 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            final_result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}] should exist after concurrent Undef → sync Int(2)",
            i
        );
    }

    // bolts[2..4) must be absent (no stale leak)
    for i in 2..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            final_result.values.get(&scoped_id).is_none(),
            "bolts[{}] must be gone — concurrent Undef path must populate preserved_counts",
            i
        );
    }
}
