//! Collection sub-structure evaluation tests (task 64).
//!
//! Tests for evaluating collection sub-components (`sub bolts : List<Bolt>`),
//! count-based elaboration, and count re-elaboration.

use std::collections::HashMap;

use reify_compiler::{CompiledForallBody, CompiledForallTemplate, TopologyTemplate};
use reify_core::*;
use reify_eval::cache::NodeId;
use reify_eval::graph::EvaluationGraph;
use reify_eval::{Engine, EvalResult};
use reify_ir::*;
use reify_test_support::builders::value_ref_typed;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{
    CompiledModuleBuilder, MultiCallSpyConstraintSolver, TopologyTemplateBuilder, lt,
};

/// Build the canonical Bolt + Parent (collection sub) templates and return
/// `(TopologyTemplate, TopologyTemplate)` in `(parent, bolt)` order.
///
/// - Bolt has a single `diameter: Length = 10mm` param.
/// - Parent has `param n : Int` (default controlled by `n_default`), a
///   `__count_bolts = n` let-binding marked as the structure-controlling
///   cell, and a collection sub-component `bolts : List<Bolt>` whose count
///   tracks `__count_bolts`.
/// - `n_default = Some(n)` gives `n` an Int default; `None` leaves it Undef.
///
/// The return order `(parent, bolt)` matches the argument order expected by
/// `EvaluationGraph::from_templates(&[parent, bolt])` and
/// `CompiledModuleBuilder::template(parent).template(bolt)`.
fn make_bolt_parent_templates(n_default: Option<i64>) -> (TopologyTemplate, TopologyTemplate) {
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    let n_default_expr = n_default.map(|n| CompiledExpr::literal(Value::Int(n), Type::Int));
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param("Parent", "n", Type::Int, n_default_expr)
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    (parent, bolt)
}

/// Build the canonical Bolt + Parent fixture and return a ready-to-eval
/// `(CompiledModule, Engine)`.  Template construction is delegated to
/// [`make_bolt_parent_templates`].
fn make_bolt_parent_engine(n_default: Option<i64>) -> (reify_compiler::CompiledModule, Engine) {
    let (parent, bolt) = make_bolt_parent_templates(n_default);

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let engine = Engine::new(Box::new(checker), None);
    (module, engine)
}

/// Count how many `Parent.bolts[N].diameter` cells exist in `values`.
///
/// Filtering on both the entity prefix **and** the specific `member` name
/// makes this assertion robust: if the engine ever emits additional cells
/// per bolt instance (e.g. a synthetic list cell), the count won't drift.
fn count_bolt_diameter_instances(values: &ValueMap) -> usize {
    values
        .iter()
        .filter(|(id, _)| id.entity.starts_with("Parent.bolts[") && id.member == "diameter")
        .count()
}

// ─── step-7: collection sub elaboration in from_templates ───

#[test]
fn from_templates_creates_collection_instances() {
    let (parent, bolt) = make_bolt_parent_templates(Some(4));
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
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
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
    // n_default=None leaves n without a default, so count evaluates to Undef
    let (module, mut engine) = make_bolt_parent_engine(None);
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
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

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

    // (4) Count cell should reflect the new value of 6
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    assert_eq!(
        edit_result.values.get(&count_id),
        Some(&Value::Int(6)),
        "count cell should reflect new count of 6 after edit"
    );

    // (5) Assert no spurious bolt instances beyond the expected 6
    let bolt_instance_count = count_bolt_diameter_instances(&edit_result.values);
    assert_eq!(
        bolt_instance_count, 6,
        "exactly 6 bolt instances should exist after count change to 6, got {}",
        bolt_instance_count
    );
}

// ─── step-21: count change 4→2, stale instances removed ───

#[test]
fn edit_param_count_decrease_removes_stale_instances() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

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

    // Count cell should reflect the new value
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    assert_eq!(
        result.values.get(&count_id),
        Some(&Value::Int(2)),
        "count cell should reflect new count of 2 after edit"
    );

    // Assert no spurious bolt instances beyond the expected 2
    let bolt_instance_count = count_bolt_diameter_instances(&result.values);
    assert_eq!(
        bolt_instance_count, 2,
        "exactly 2 bolt instances should exist after count decrease to 2, got {}",
        bolt_instance_count
    );
}

// ─── step-19: collection value aggregation ───

#[test]
fn eval_collection_list_aggregation() {
    // Bolt template: param grade : Real = 8.8
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "grade",
            Type::dimensionless_scalar(),
            Some(CompiledExpr::literal(
                Value::Real(8.8),
                Type::dimensionless_scalar(),
            )),
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
            Type::List(Box::new(Type::dimensionless_scalar())),
            CompiledExpr::value_ref(
                ValueCellId::new("Parent", "__list_bolts__grade"),
                Type::List(Box::new(Type::dimensionless_scalar())),
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
        structure Bolt { param diameter : Length = 10mm }
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
        .filter(|d| d.severity == reify_core::Severity::Error)
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
        structure Bolt { param grade : Length = 8.8 }
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
        .filter(|d| d.severity == reify_core::Severity::Error)
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

// --- task-958: consecutive Undef->Undef->Int count transition regression ---

#[test]
fn edit_param_count_int_undef_undef_int_transition() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

    // Initial eval with n=4, creating 4 bolt instances
    let _initial = engine.eval(&module);

    // Transition sequence: Int(4) → Undef → Undef → Int(2)
    let assert_no_bolts = |result: &EvalResult, label: &str| {
        for i in 0..4 {
            let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
            assert!(
                result.values.get(&scoped_id).is_none(),
                "bolts[{}].diameter must be absent after {}",
                i,
                label
            );
        }
        assert_eq!(
            count_bolt_diameter_instances(&result.values),
            0,
            "no bolt diameter instances should exist after {}",
            label
        );
    };

    // Int(4) → Undef must clear all bolt instances
    let r1 = engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("first edit to Undef should succeed");
    assert_no_bolts(&r1, "Int(4)->Undef");

    // Undef → Undef must be a no-op
    let r2 = engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("second edit to Undef should succeed");
    assert_no_bolts(&r2, "Undef->Undef");

    // Undef → Int(2) must elaborate exactly bolts[0..2)
    let result = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit to Int(2) should succeed");

    // bolts[0..2) must exist with diameter = 10mm
    for i in 0..2 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            result.values.get(&scoped_id),
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should be 10mm after Int(4)->Undef->Undef->Int(2) transition",
            i
        );
    }

    // bolts[2..4) must be absent
    for i in 2..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}].diameter must be absent after Int(4)->Undef->Undef->Int(2) transition",
            i
        );
    }

    assert_eq!(
        count_bolt_diameter_instances(&result.values),
        2,
        "exactly 2 bolt diameter instances after Int(2)"
    );
}

// ─── task-1588 step-1: non-Int old_count emits Warning diagnostic ───

#[test]
fn edit_param_non_int_old_count_emits_warning() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

    // Establish baseline: 4 bolt instances in snapshot
    let _initial = engine.eval(&module);

    // Edit n to Real(2.0) — sets snapshot count_cell = Real(2.0)
    // old_count_val = Int(4) → Int arm (no warning from old_count)
    // new_count_val = Real(2.0) → other arm (warning with "new value")
    let r1 = engine
        .edit_param(n_id.clone(), Value::Real(2.0))
        .expect("edit to Real should succeed");

    // r1 must have exactly one warning (from the new_count path only, not old_count)
    let r1_warnings: Vec<_> = r1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        r1_warnings.len(),
        1,
        "expected exactly 1 warning in r1 (new_count path), got: {:?}",
        r1_warnings
    );
    assert!(
        r1_warnings[0].message.contains("new value"),
        "r1 warning should mention 'new value', got: {:?}",
        r1_warnings[0].message
    );

    // Edit n to Int(3) — old_count_val = Real(2.0), new_count_val = Int(3)
    // old_count_val = Real(2.0) should emit a Warning diagnostic about "old value"
    let r2 = engine
        .edit_param(n_id, Value::Int(3))
        .expect("edit to Int(3) should succeed");

    // (a) diagnostics must contain a Warning about non-integer old count, specifically "old value"
    let has_warning = r2.diagnostics.iter().any(|d| {
        d.severity == Severity::Warning
            && d.message.contains("non-integer")
            && d.message.contains("Parent.__count_bolts")
            && d.message.contains("old value")
    });
    assert!(
        has_warning,
        "expected a Warning diagnostic about non-integer old count cell with 'old value', got: {:?}",
        r2.diagnostics
    );

    // (b) exactly 3 bolt instances exist (new_count = Int(3))
    let bolt_count = count_bolt_diameter_instances(&r2.values);
    assert_eq!(
        bolt_count, 3,
        "exactly 3 bolt instances should exist after editing to Int(3), got {}",
        bolt_count
    );

    // (c) no stale instances beyond the 3 expected (no Real(2.0)-era artifacts)
    for i in 3..6 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            r2.values.get(&scoped_id).is_none(),
            "bolts[{}] should not exist (no stale instances expected)",
            i
        );
    }
}

// ─── task-1588 step-3: non-Int new_count emits Warning diagnostic ───

#[test]
fn edit_param_non_int_new_count_emits_warning() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

    // Establish baseline: 4 bolt instances in snapshot
    let _initial = engine.eval(&module);

    // Edit n to Real(2.0) — new_count_val = Real(2.0) should emit a Warning
    // old_count_val = Int(4) → Int arm (no warning from old_count match)
    let r1 = engine
        .edit_param(n_id, Value::Real(2.0))
        .expect("edit to Real should succeed");

    // (a) diagnostics must contain a Warning about non-integer new count, specifically "new value"
    let has_warning = r1.diagnostics.iter().any(|d| {
        d.severity == Severity::Warning
            && d.message.contains("non-integer")
            && d.message.contains("Parent.__count_bolts")
            && d.message.contains("new value")
    });
    assert!(
        has_warning,
        "expected a Warning diagnostic about non-integer new count cell with 'new value', got: {:?}",
        r1.diagnostics
    );

    // (b) all 4 old bolt instances were removed (old_count = 4 from Int arm)
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            r1.values.get(&scoped_id).is_none(),
            "bolts[{}] should be removed when switching away from Int count",
            i
        );
    }

    // (c) 0 new instances created (new_count = 0 because Real(2.0) → _ => 0)
    let bolt_count = count_bolt_diameter_instances(&r1.values);
    assert_eq!(
        bolt_count, 0,
        "zero bolt instances should be created when new count is Real(2.0), got {}",
        bolt_count
    );
}

// ─── task-1588 step-5: Undef count transitions do NOT emit spurious warnings ───

#[test]
fn edit_param_undef_count_transition_no_spurious_warning() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

    // Establish baseline: 4 bolt instances in snapshot
    let _initial = engine.eval(&module);

    // Edit n to Undef — old_count_val=Int(4) (Int arm), new_count_val=Undef (Undef arm)
    // Neither arm should emit a warning.
    let r1 = engine
        .edit_param(n_id.clone(), Value::Undef)
        .expect("edit to Undef should succeed");

    let warnings: Vec<_> = r1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no Warning diagnostics for Int→Undef count transition, got: {:?}",
        warnings
    );
    // After Int→Undef: all instances must be removed (0 instances)
    assert_eq!(
        count_bolt_diameter_instances(&r1.values),
        0,
        "expected 0 bolt instances after Int→Undef count transition"
    );

    // Edit n to Int(2) — old_count_val=Undef (Undef arm), new_count_val=Int(2) (Int arm)
    // Neither arm should emit a warning.
    let r2 = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit to Int(2) should succeed");

    let warnings: Vec<_> = r2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no Warning diagnostics for Undef→Int count transition, got: {:?}",
        warnings
    );
    // After Undef→Int(2): exactly 2 instances must exist
    assert_eq!(
        count_bolt_diameter_instances(&r2.values),
        2,
        "expected 2 bolt instances after Undef→Int(2) count transition"
    );
}

// ─── step-30: count Int→Undef removes all instances (regression for unreachable!() bug) ───

#[test]
fn edit_param_count_to_undef_removes_all_instances() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

    // Initial eval with n=4, creating 4 bolt instances
    let _initial = engine.eval(&module);

    // Edit n to Undef — count cell evaluates to Undef, triggering the Int→Undef path.
    // With the unreachable!() bug this panics; with `_ => 0` it gracefully removes all instances.
    let result = engine
        .edit_param(n_id, Value::Undef)
        .expect("edit_param with Undef count should not panic");

    // All 4 old bolt instances should be removed
    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}].diameter should be removed when count becomes Undef",
            i
        );
    }

    // Assert zero bolt instances remain — not just that old ones are gone,
    // but no spurious new ones appeared either.
    let bolt_instance_count = count_bolt_diameter_instances(&result.values);
    assert_eq!(
        bolt_instance_count, 0,
        "zero bolt instances should remain when count is Undef, got {}",
        bolt_instance_count
    );

    // Count cell itself should reflect the Undef edit — symmetric with the
    // Int→Int sibling tests above. Catches a regression where instances are
    // removed via another path while the count cell is not updated.
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    assert_eq!(
        result.values.get(&count_id),
        Some(&Value::Undef),
        "count cell should be Undef after edit to Undef"
    );
}

// ── Task 2184: Phase 4 cache invalidation for shrunk+regrown collection (mirror of task 2086) ──

#[test]
fn edit_param_phase4_invalidates_cache_for_shrunk_and_regrown_collection_instance() {
    let (module, mut engine) = make_bolt_parent_engine(Some(4));
    let n_id = ValueCellId::new("Parent", "n");

    // Step 1: populate cache at V_A via initial eval
    engine.eval(&module);
    let v_a = engine.snapshot().expect("snapshot after eval").version;

    // Pre-edit assertion: all 4 bolt cache entries must be at V_A.
    // This proves the cache was populated, so the post-edit None-or-fresh check
    // is meaningful rather than trivially satisfied by a never-populated cache.
    for i in 0..4_usize {
        let bolt_node = NodeId::Value(ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter"));
        let entry = engine
            .cache_store()
            .get(&bolt_node)
            .unwrap_or_else(|| panic!("Parent.bolts[{}].diameter must be in cache after eval", i));
        assert_eq!(
            entry.basis_version, v_a,
            "Parent.bolts[{}].diameter must be at V_A before any edits",
            i
        );
    }

    // Step 2: shrink count 4→2 (Phase 4 removes all 4 old instances, creates 0..1).
    // Without the fix, cache entries for indices 0..1 survive the remove loop at V_A.
    engine
        .edit_param(n_id.clone(), Value::Int(2))
        .expect("edit_param shrink 4→2 must succeed");

    // Step 3: re-grow count 2→4 (Phase 4 removes indices 0..1, creates 0..3).
    // Without the fix, the stale V_A entries for 0..1 survive both remove loops.
    let result = engine
        .edit_param(n_id, Value::Int(4))
        .expect("edit_param regrow 2→4 must succeed");

    let current_version = engine
        .snapshot()
        .expect("snapshot must be present after two edit_param calls")
        .version;

    // Post-edit assertion: each cache entry must be absent (None) or fresh at
    // current_version.  A Some(entry) with a prior version is the stale cache
    // artifact this test pins.  None is acceptable — Phase 4's create loop does
    // not call cache.record_evaluation, so invalidated entries remain absent.
    for i in 0..4_usize {
        let bolt_node = NodeId::Value(ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter"));
        if let Some(entry) = engine.cache_store().get(&bolt_node) {
            assert_eq!(
                entry.basis_version, current_version,
                "Parent.bolts[{}].diameter cache entry must be fresh after shrink→regrow; \
                 got basis_version {:?}, expected {:?}",
                i, entry.basis_version, current_version
            );
        }
        // None is acceptable — Phase 4's create loop does not call
        // cache.record_evaluation, so invalidated entries remain absent.
    }

    // Sanity: the re-grown result must have exactly 4 bolt instances
    assert_eq!(
        count_bolt_diameter_instances(&result.values),
        4,
        "exactly 4 bolt instances should exist after regrow to 4"
    );
}

// ─── Task 4530 step-1: grown instances track upstream param edits ───────────

/// Task 4530 (step-1): Pins that collection instances grown by an `edit_param`
/// count increase propagate later upstream param edits through their
/// `default_expr`.
///
/// Root cause: `edit_param`'s collection-count re-elaboration phase never
/// rebuilds `reverse_index` / `demand` after growing the collection, so the
/// next param edit's dirty_cone is computed from a stale index that does not
/// include the grown instances.
///
/// Sequence:
/// 1. Fixture: `Bolt.diameter` has `default = value_ref_typed("Parent","bolt_d",Length)`.
///    `Parent` has `bolt_d` (default `0.01m`), `n` (default `Int(2)`), and count plumbing.
/// 2. `engine.eval()` — creates `bolts[0]`,`bolts[1]` with `diameter = 0.01m`.
/// 3. `edit_param(n, Int(4))` — grows to `bolts[0..3]`.
/// 4. `edit_param(bolt_d, 0.02m)` — must update ALL 4 instances' `diameter`.
///
/// RED today: `bolts[2]` and `bolts[3]` stay at `0.01m` (stale) because
/// `reverse_index` / `demand` were never rebuilt after the grow.
#[test]
fn grown_collection_instances_track_upstream_param_edits() {
    // Bolt template: diameter with default = Parent.bolt_d
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(value_ref_typed("Parent", "bolt_d", Type::length())),
        )
        .build();

    // Parent template: bolt_d + n + count plumbing
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "bolt_d",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding(
            "Parent",
            "__count_bolts",
            Type::Int,
            value_ref_typed("Parent", "n", Type::Int),
        )
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let n_id = ValueCellId::new("Parent", "n");
    let bolt_d_id = ValueCellId::new("Parent", "bolt_d");

    // Step 2: initial eval (n=2 → bolts[0],[1] with diameter=0.01m)
    engine.eval(&module);

    // Step 3: grow n from 2 to 4
    engine
        .edit_param(n_id, Value::Int(4))
        .expect("edit_param(n, Int(4)) should succeed");

    // Step 4: edit bolt_d — ALL 4 instances must track the upstream change
    let r = engine
        .edit_param(bolt_d_id, Value::length(0.02))
        .expect("edit_param(bolt_d, 0.02) should succeed");

    for i in 0..4 {
        let scoped_id = ValueCellId::new(format!("Parent.bolts[{}]", i), "diameter");
        assert_eq!(
            r.values.get(&scoped_id),
            Some(&Value::length(0.02)),
            "bolts[{}].diameter should be 0.02m after bolt_d edit, got {:?}",
            i,
            r.values.get(&scoped_id),
        );
    }
}

// ─── Task 4530 step-2: grown forall constraints trigger solver on upstream edit

/// Task 4530 (step-2): Pins the solver-gate manifestation of the same
/// stale-reverse_index bug: grown forall-emitted constraints are absent from
/// the stale index, so the `constraints_dirty` gate
/// (`engine_edit.rs:1337-1339`) misses them and the solver is never called for
/// grown instances' auto params.
///
/// Observable: `solver.call_count() == 4` after the `bolt_d` edit (one call
/// per instance whose forall constraint reaches the dirty cone).
///
/// RED today: `0` — the stale reverse_index carries no forall constraint
/// edges at all (forall constraints are emitted during `edit_param`'s
/// collection-count re-elaboration, which runs AFTER the resolution phase),
/// so the dirty_cone for the bolt_d edit contains no `Constraint` nodes and
/// the solver is never invoked.
#[test]
fn grown_forall_constraints_trigger_solver_on_later_upstream_edit() {
    // Body constraint: bolts[0].mass < bolts[0].diameter.
    // The runtime rewriter maps bolts[0] → bolts[i] for each instance.
    let body_expr = lt(
        value_ref_typed("Parent.bolts[0]", "mass", Type::length()),
        value_ref_typed("Parent.bolts[0]", "diameter", Type::length()),
    );

    let forall_tmpl = CompiledForallTemplate {
        variable: "b".to_string(),
        parent_entity: "Parent".to_string(),
        collection_sub_name: "bolts".to_string(),
        count_cell: ValueCellId::new("Parent", "__count_bolts"),
        span: SourceSpan::new(0, 0),
        body: CompiledForallBody::Constraint { body_expr },
    };

    // Bolt: diameter (default=Parent.bolt_d) + mass (auto, for solver)
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(value_ref_typed("Parent", "bolt_d", Type::length())),
        )
        .auto_param("Bolt", "mass", Type::length())
        .build();

    // Parent: bolt_d + n + count plumbing + forall template
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "bolt_d",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .param(
            "Parent",
            "n",
            Type::Int,
            // Start at n=0 so the initial eval creates NO bolt instances and
            // avoids the collect_member_list debug_assert for auto cells (Auto
            // cells are not added to `values` by elaborate_child_params_only —
            // they're only added during edit_param's re-elaboration).  Growing
            // 0→4 via edit_param tests the same invariant as 2→4.
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .let_binding(
            "Parent",
            "__count_bolts",
            Type::Int,
            value_ref_typed("Parent", "n", Type::Int),
        )
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .forall_template(forall_tmpl)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    // MultiCallSpyConstraintSolver: always returns Solved with empty values
    // (mass stays Undef — we only care about call_count, not resolved values).
    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: HashMap::new(),
        unique: true,
    }]);
    // Clone the capture Arc BEFORE boxing — the only way to read call_count
    // after the spy is moved into the engine.
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    let n_id = ValueCellId::new("Parent", "n");
    let bolt_d_id = ValueCellId::new("Parent", "bolt_d");

    // Step 1: initial eval (n=0 → no bolt instances; no forall constraints yet —
    //         forall emission only happens in edit_param's re-elaboration phase).
    // NOTE: eval() also calls the solver once for the Bolt template entity (which
    // has an auto param `mass` but no constraints).  Record the baseline count
    // here so the assertion below measures only the edit_param-driven calls.
    engine.eval(&module);
    let calls_before_edits = captured.lock().unwrap().len();

    // Step 2: grow n 0→4 (collection re-elaboration emits forall@b[0..3]).
    // With the fix: reverse_index is rebuilt here so forall@b[0..3] are visible
    // to the next edit_param's dirty_cone traversal.
    engine
        .edit_param(n_id, Value::Int(4))
        .expect("edit_param(n, Int(4)) should succeed");

    // Step 3: edit bolt_d → bolt_i.diameter dirty → forall@b[i] dirty (if in index)
    engine
        .edit_param(bolt_d_id, Value::length(0.02))
        .expect("edit_param(bolt_d, 0.02) should succeed");

    // With the fix: reverse_index rebuilt after the n→4 grow → forall@b[0..3]
    // edges exist → dirty_cone for bolt_d includes all 4 constraint nodes →
    // solver called once per entity group (bolts[0..3]).
    //
    // Without the fix: stale reverse_index has no forall edges → dirty_cone
    // contains only Value nodes → constraints_dirty=false for every group →
    // solver never invoked → edit_param call_count == 0.
    let problems = captured.lock().unwrap();
    // Count only the calls driven by the edit_param sequence (excludes the
    // one eval()-phase call for the Bolt template's empty-constraints problem).
    let call_count = problems.len() - calls_before_edits;
    drop(problems);
    assert_eq!(
        call_count, 4,
        "expected solver called 4 times (one per grown instance with dirty forall constraint), \
         got {} — stale reverse_index likely caused the regression",
        call_count
    );
}
