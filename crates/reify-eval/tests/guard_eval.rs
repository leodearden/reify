//! Guard evaluation tests.
//!
//! Tests for evaluating guarded groups: conditional member activation,
//! else branches, undef guards, and schema re-elaboration.

mod common;

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use reify_eval::Engine;
use reify_test_support::builders::{and, ge, gt, literal, value_ref, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintSolver, SequencedMockConstraintSolver,
    TopologyTemplateBuilder, mm, parse_and_compile, wave2_flip_fixture,
};
use reify_core::*;
use reify_ir::*;

use reify_compiler::{CompiledConstraint, ValueCellDecl, ValueCellKind, Visibility};

use common::ten_bool_guarded_groups;

/// Helper to create a ValueCellDecl for tests.
fn make_param_decl(entity: &str, member: &str, cell_type: Type, default: Value) -> ValueCellDecl {
    ValueCellDecl {
        id: ValueCellId::new(entity, member),
        kind: ValueCellKind::Param,
        visibility: Visibility::Public,
        cell_type: cell_type.clone(),
        default_expr: Some(CompiledExpr::literal(default, cell_type)),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    }
}

/// Step 13: When guard is true, guarded members should be evaluated.
///
/// Build: Bool param 'active' (default=true), guarded_group with
/// guard_expr=ValueRef(active), guard_value_cell='S.__guard_0',
/// member param 'x' (default=5mm). After eval(), 'x' should be 0.005 (5mm SI).
#[test]
fn eval_guard_true_includes_members() {
    let _active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");

    // Guard expression: ValueRef to 'active' (Bool)
    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Member: param x : Scalar = 5mm
    let x_default = CompiledExpr::literal(Value::length(0.005), Type::length());
    let x_decl = ValueCellDecl {
        id: x_id.clone(),
        kind: ValueCellKind::Param,
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: Some(x_default),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members
            vec![],       // constraints
            vec![],       // else_members
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // The guard cell should be evaluated to true
    let guard_val = result.values.get(&guard_id);
    assert_eq!(
        guard_val,
        Some(&Value::Bool(true)),
        "guard cell should evaluate to true"
    );

    // The guarded member 'x' should be evaluated to 5mm = 0.005m
    let x_val = result.values.get(&x_id);
    assert_eq!(
        x_val,
        Some(&Value::length(0.005)),
        "guarded member x should be 0.005 (5mm SI) when guard is true"
    );
}

/// Step 15: When guard is false, else_members should be evaluated and members should be Undef.
///
/// Build: Bool param 'active' (default=false), guarded_group with
/// member param 'x' (default=5mm) and else_member param 'y' (default=10mm).
/// After eval(): 'y' should be 0.01 (10mm SI), 'x' should be Undef.
#[test]
fn eval_guard_false_includes_else() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let y_decl = make_param_decl("S", "y", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members (active when true)
            vec![],       // constraints
            vec![y_decl], // else_members (active when false)
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Guard cell should be false
    assert_eq!(
        result.values.get(&guard_id),
        Some(&Value::Bool(false)),
        "guard cell should evaluate to false"
    );

    // 'y' (else member) should be evaluated to 10mm = 0.01m
    assert_eq!(
        result.values.get(&y_id),
        Some(&Value::length(0.01)),
        "else member y should be 0.01 (10mm SI) when guard is false"
    );

    // 'x' (guard-true member) should be Undef
    assert_eq!(
        result.values.get(&x_id),
        Some(&Value::Undef),
        "guarded member x should be Undef when guard is false"
    );
}

/// Step 17: When guard is Undef (references an Auto param with no solver),
/// all guarded members should have Undef values.
#[test]
fn eval_guard_undef_members_indeterminate() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // Guard expression references an Auto param (starts Undef, no solver)
    let guard_expr = value_ref_typed("S", "flag", Type::Bool);
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let y_decl = make_param_decl("S", "y", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "flag", Type::Bool)
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members
            vec![],       // constraints
            vec![y_decl], // else_members
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Guard cell should be Undef (since flag is Auto/Undef)
    assert_eq!(
        result.values.get(&guard_id),
        Some(&Value::Undef),
        "guard cell should evaluate to Undef when referencing unresolved Auto param"
    );

    // Both members and else_members should be Undef (indeterminate)
    assert_eq!(
        result.values.get(&x_id),
        Some(&Value::Undef),
        "guarded member x should be Undef when guard is Undef"
    );
    assert_eq!(
        result.values.get(&y_id),
        Some(&Value::Undef),
        "else member y should be Undef when guard is Undef"
    );

    // Check DeterminacyState in snapshot
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (_, x_det) = snapshot.values.get(&x_id).expect("x in snapshot");
    assert_eq!(
        *x_det,
        DeterminacyState::Undetermined,
        "guarded member x determinacy should be Undetermined when guard is Undef"
    );
    let (_, y_det) = snapshot.values.get(&y_id).expect("y in snapshot");
    assert_eq!(
        *y_det,
        DeterminacyState::Undetermined,
        "else member y determinacy should be Undetermined when guard is Undef"
    );
}

/// Step 19: Changing a guard parameter via edit_param() triggers re-elaboration.
///
/// Start with guard=true (member x active, else_member y inactive).
/// edit_param 'active' from true to false.
/// Assert: (1) topology_fingerprint changed,
///         (2) x is Undef, (3) y is evaluated,
///         (4) structure_controlling cell is in the graph.
#[test]
fn guard_change_triggers_re_elaboration() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let y_decl = make_param_decl("S", "y", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members (active when true)
            vec![],       // constraints
            vec![y_decl], // else_members (active when false)
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with guard=true
    let initial_result = engine.eval(&module);
    let initial_fingerprint = engine.snapshot().unwrap().topology_fingerprint;

    // Verify initial state: x evaluated, y Undef
    assert_eq!(
        initial_result.values.get(&x_id),
        Some(&Value::length(0.005)),
        "x should be 5mm when guard is true"
    );
    assert_eq!(
        initial_result.values.get(&y_id),
        Some(&Value::Undef),
        "y should be Undef when guard is true"
    );

    // Edit 'active' from true to false
    let edit_result = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param should succeed");

    // (1) Topology fingerprint should change (guard state flipped)
    let new_fingerprint = engine.snapshot().unwrap().topology_fingerprint;
    assert_ne!(
        initial_fingerprint, new_fingerprint,
        "topology_fingerprint should change when guard state changes"
    );

    // (2) x should now be Undef (deactivated)
    assert_eq!(
        edit_result.values.get(&x_id),
        Some(&Value::Undef),
        "x should be Undef after guard changed to false"
    );

    // (3) y (else member) should now be evaluated
    assert_eq!(
        edit_result.values.get(&y_id),
        Some(&Value::length(0.01)),
        "y should be 0.01 (10mm SI) after guard changed to false"
    );

    // (4) Guard cell should be in structure_controlling
    let snapshot = engine.snapshot().unwrap();
    assert!(
        snapshot.graph.structure_controlling.contains(&guard_id),
        "guard_value_cell should be in structure_controlling"
    );
}

/// Step 27: Guarded constraints should only be checked when their guard is active.
///
/// Build: Bool param 'active' (default=true), guarded_group with member param 'x'
/// (default=5mm) and one guarded constraint (x > 10mm, which will be Violated).
/// (1) With active=true: check() should include the constraint result (Violated).
/// (2) With active=false: check() should NOT include the guarded constraint result
///     (it's inactive, should be skipped).
#[test]
fn eval_guarded_constraint_enforced_only_when_active() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let _x_id = ValueCellId::new("S", "x");
    let constraint_id = ConstraintNodeId::new("S", 0);

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Member: param x : Scalar = 5mm
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));

    // Guarded constraint: x > 10mm (will be violated since x=5mm)
    let constraint_expr = gt(
        value_ref_typed("S", "x", Type::length()),
        CompiledExpr::literal(Value::length(0.01), Type::length()),
    );
    let guarded_constraint = CompiledConstraint {
        id: constraint_id.clone(),
        label: Some("x_gt_10mm".to_string()),
        expr: constraint_expr,
        span: SourceSpan::new(0, 0),
        domain: None,
        optimized_target: None,
    };

    // Case 1: active=true — constraint should be checked and show Violated
    let template_true = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr.clone(),
            guard_id.clone(),
            vec![x_decl.clone()],             // members
            vec![guarded_constraint.clone()], // constraints
            vec![],                           // else_members
            vec![],                           // else_constraints
        )
        .build();

    let module_true = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_true)
        .build();

    let checker_true =
        MockConstraintChecker::new().with_result(constraint_id.clone(), Satisfaction::Violated);
    let mut engine_true = Engine::new(Box::new(checker_true), None);
    let check_result_true = engine_true.check(&module_true);

    // When guard=true, the constraint should be in results
    let has_constraint = check_result_true
        .constraint_results
        .iter()
        .any(|cr| cr.id == constraint_id);
    assert!(
        has_constraint,
        "when guard is true, guarded constraint should be checked and appear in results"
    );

    // Case 2: active=false — constraint should NOT be checked
    let guard_expr2 = value_ref_typed("S", "active", Type::Bool);
    let x_decl2 = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let guarded_constraint2 = CompiledConstraint {
        id: constraint_id.clone(),
        label: Some("x_gt_10mm".to_string()),
        expr: gt(
            value_ref_typed("S", "x", Type::length()),
            CompiledExpr::literal(Value::length(0.01), Type::length()),
        ),
        span: SourceSpan::new(0, 0),
        domain: None,
        optimized_target: None,
    };

    let template_false = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .guarded_group(
            guard_expr2,
            guard_id.clone(),
            vec![x_decl2],             // members
            vec![guarded_constraint2], // constraints
            vec![],                    // else_members
            vec![],                    // else_constraints
        )
        .build();

    let module_false = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_false)
        .build();

    let checker_false =
        MockConstraintChecker::new().with_result(constraint_id.clone(), Satisfaction::Violated);
    let mut engine_false = Engine::new(Box::new(checker_false), None);
    let check_result_false = engine_false.check(&module_false);

    // When guard=false, the guarded constraint should NOT be in results
    let has_constraint_false = check_result_false
        .constraint_results
        .iter()
        .any(|cr| cr.id == constraint_id);
    assert!(
        !has_constraint_false,
        "when guard is false, guarded constraint should NOT be checked or appear in results, but got: {:?}",
        check_result_false
            .constraint_results
            .iter()
            .map(|cr| (&cr.id, &cr.satisfaction))
            .collect::<Vec<_>>()
    );
}

/// Bug reproduction: Block B in edit_param() overwrites solver-resolved Auto param values.
///
/// Setup: Bool param 'active' (default=true), guarded group with Auto param 'thickness'
/// as a member, constraint (thickness > 2mm), MockConstraintSolver resolves thickness to 5mm.
/// After eval() (guard=true, solver resolves thickness), call edit_param('active', false).
/// Assert: thickness retains its solver-resolved value (0.005 SI), NOT Undef.
#[test]
fn edit_param_guard_false_preserves_solver_auto_param() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Auto param 'thickness' as a guarded member (kind=Auto, no default_expr)
    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        // Top-level auto_param so eval() resolution phase finds it
        .auto_param("S", "thickness", Type::length())
        // Top-level constraint so eval() resolution phase can match it
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![thickness_decl], // members (active when true) — graph sees kind=Auto
            vec![],               // constraints (already at top level)
            vec![],               // else_members
            vec![],               // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Mock solver: resolves thickness to 5mm = 0.005 SI
    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved_values);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval with guard=true; solver resolves thickness
    let initial_result = engine.eval(&module);
    let thickness_val = initial_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "thickness should be 0.005 SI (5mm) after initial eval, got {:?}",
        thickness_val
    );

    // Edit 'active' from true to false — guard deactivates
    let edit_result = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param should succeed");

    // Regression guard (task 492): Block B in edit_param previously overwrote the
    // solver-resolved Auto param with Undef. With the Auto-skip fix in place,
    // thickness must retain its solver-resolved value through guard deactivation.
    let thickness_after = edit_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_after, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Auto param 'thickness' should retain solver-resolved value (0.005 SI) after guard deactivation, got {:?}",
        thickness_after
    );

    // DeterminacyState in snapshot must remain Determined after guard deactivation
    let snapshot = engine
        .snapshot()
        .expect("snapshot should exist after edit_param");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&thickness_id)
        .expect("thickness in snapshot after deactivation");
    assert!(
        matches!(snap_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "thickness should retain 0.005 SI in snapshot after deactivation, got {:?}",
        snap_val
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Determined,
        "Auto param DeterminacyState must remain Determined after guard deactivation"
    );
}

/// Mirror of the above test but for else_members: Auto param in else branch
/// should survive when guard transitions from false→true (deactivating else branch).
#[test]
fn edit_param_guard_true_preserves_solver_auto_in_else_members() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Auto param 'thickness' as an else_member (kind=Auto, no default_expr)
    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        // Top-level auto_param so eval() resolution phase finds it
        .auto_param("S", "thickness", Type::length())
        // Top-level constraint so eval() resolution phase can match it
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![],               // members
            vec![],               // constraints
            vec![thickness_decl], // else_members (active when false)
            vec![],               // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Mock solver: resolves thickness to 5mm = 0.005 SI
    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved_values);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval with guard=false; solver resolves thickness
    let initial_result = engine.eval(&module);
    let thickness_val = initial_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "thickness should be 0.005 SI (5mm) after initial eval, got {:?}",
        thickness_val
    );

    // Edit 'active' from false to true — else branch deactivates
    let edit_result = engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param should succeed");

    // Auto param in else_members should retain solver-resolved value
    let thickness_after = edit_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_after, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Auto param 'thickness' in else_members should retain solver-resolved value (0.005 SI) after else branch deactivation, got {:?}",
        thickness_after
    );

    // DeterminacyState in snapshot must remain Determined after else branch deactivation
    let snapshot = engine
        .snapshot()
        .expect("snapshot should exist after edit_param");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&thickness_id)
        .expect("thickness in snapshot after else deactivation");
    assert!(
        matches!(snap_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "thickness should retain 0.005 SI in snapshot after else deactivation, got {:?}",
        snap_val
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Determined,
        "Auto param DeterminacyState must remain Determined after else branch deactivation"
    );
}

/// Regression test: regular Param-kind members must still be set to Undef when
/// their guard deactivates. The Auto-skip fix should not affect normal params.
#[test]
fn edit_param_guard_false_still_deactivates_regular_params() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let width_id = ValueCellId::new("S", "width");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Regular Param-kind member 'width' with a default value
    let width_decl = make_param_decl("S", "width", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![width_decl], // members (active when true)
            vec![],           // constraints
            vec![],           // else_members
            vec![],           // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with guard=true; width should be 10mm = 0.01
    let initial_result = engine.eval(&module);
    assert_eq!(
        initial_result.values.get(&width_id),
        Some(&Value::length(0.01)),
        "width should be 0.01 (10mm SI) when guard is true"
    );

    // Edit 'active' from true to false — guard deactivates
    let edit_result = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param should succeed");

    // Regular Param member should be Undef after guard deactivation
    assert_eq!(
        edit_result.values.get(&width_id),
        Some(&Value::Undef),
        "Regular Param 'width' should be Undef after guard changed to false"
    );
}

/// Round-trip test: guard true→false→true preserves Auto param value.
///
/// Uses SequencedMockConstraintSolver with two results (5mm, 8mm). The solver
/// is invoked once during initial eval (5mm). On re-activation, the preserved
/// value keeps constraints out of the dirty cone, so the solver is NOT re-invoked.
/// Asserts the value and DeterminacyState::Determined are preserved at every step.
#[test]
fn guard_round_trip_true_false_true_re_resolves_auto_param() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![thickness_decl],
            vec![],
            vec![],
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Sequenced solver: first solve → 5mm, second solve → 8mm
    let mut solved1 = HashMap::new();
    solved1.insert(thickness_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(thickness_id.clone(), mm(8.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Step 1: eval() with guard=true — solver resolves thickness to 5mm
    let initial_result = engine.eval(&module);
    let thickness_val = initial_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Step 1: thickness should be 5mm (0.005 SI) after initial eval, got {:?}",
        thickness_val
    );
    let snap1 = engine.snapshot().expect("snapshot after eval");
    let (_, det1) = snap1.values.get(&thickness_id).expect("thickness in snap1");
    assert_eq!(
        *det1,
        DeterminacyState::Determined,
        "Step 1: DeterminacyState should be Determined after solver resolution"
    );

    // Step 2: edit_param(active, false) — guard deactivates, thickness preserved at 5mm
    let edit_result1 = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param to false should succeed");
    let thickness_deact = edit_result1.values.get(&thickness_id);
    assert!(
        matches!(thickness_deact, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Step 2: thickness should remain 5mm after deactivation, got {:?}",
        thickness_deact
    );
    let snap2 = engine.snapshot().expect("snapshot after deactivation");
    let (_, det2) = snap2.values.get(&thickness_id).expect("thickness in snap2");
    assert_eq!(
        *det2,
        DeterminacyState::Determined,
        "Step 2: DeterminacyState should remain Determined after deactivation"
    );

    // Step 3: edit_param(active, true) — guard re-activates.
    // The solver is NOT re-invoked because the preserved value (5mm) keeps
    // constraints out of the dirty cone. The Auto param retains its original
    // solver-resolved value through the full round-trip.
    let edit_result2 = engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param to true should succeed");
    let thickness_react = edit_result2.values.get(&thickness_id);
    assert!(
        matches!(thickness_react, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Step 3: thickness should retain preserved value (5mm / 0.005 SI) after re-activation, got {:?}",
        thickness_react
    );
    let snap3 = engine.snapshot().expect("snapshot after re-activation");
    let (_, det3) = snap3.values.get(&thickness_id).expect("thickness in snap3");
    assert_eq!(
        *det3,
        DeterminacyState::Determined,
        "Step 3: DeterminacyState should be Determined after re-activation"
    );
}

/// Round-trip test for else_members: guard false→true→false preserves Auto param value.
///
/// Mirror of guard_round_trip_true_false_true but with Auto param in else_members.
/// The solver is invoked once during initial eval (5mm). On else-branch re-activation,
/// the preserved value keeps constraints out of the dirty cone, so the solver is NOT
/// re-invoked. Asserts the value and DeterminacyState::Determined at every step.
#[test]
fn guard_round_trip_false_true_false_re_resolves_auto_in_else() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![],               // members
            vec![],               // constraints
            vec![thickness_decl], // else_members (active when false)
            vec![],               // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Sequenced solver: first solve → 5mm, second solve → 8mm
    let mut solved1 = HashMap::new();
    solved1.insert(thickness_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(thickness_id.clone(), mm(8.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Step 1: eval() with guard=false — else branch active, solver resolves thickness to 5mm
    let initial_result = engine.eval(&module);
    let thickness_val = initial_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Step 1: thickness should be 5mm (0.005 SI) after initial eval, got {:?}",
        thickness_val
    );
    let snap1 = engine.snapshot().expect("snapshot after eval");
    let (_, det1) = snap1.values.get(&thickness_id).expect("thickness in snap1");
    assert_eq!(
        *det1,
        DeterminacyState::Determined,
        "Step 1: DeterminacyState should be Determined after solver resolution"
    );

    // Step 2: edit_param(active, true) — else branch deactivates, thickness preserved at 5mm
    let edit_result1 = engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param to true should succeed");
    let thickness_deact = edit_result1.values.get(&thickness_id);
    assert!(
        matches!(thickness_deact, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Step 2: thickness should remain 5mm after else deactivation, got {:?}",
        thickness_deact
    );
    let snap2 = engine.snapshot().expect("snapshot after else deactivation");
    let (_, det2) = snap2.values.get(&thickness_id).expect("thickness in snap2");
    assert_eq!(
        *det2,
        DeterminacyState::Determined,
        "Step 2: DeterminacyState should remain Determined after else deactivation"
    );

    // Step 3: edit_param(active, false) — else branch re-activates.
    // The solver is NOT re-invoked because the preserved value keeps
    // constraints out of the dirty cone. Value stays at 5mm.
    let edit_result2 = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param to false should succeed");
    let thickness_react = edit_result2.values.get(&thickness_id);
    assert!(
        matches!(thickness_react, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Step 3: thickness should retain preserved value (5mm / 0.005 SI) after else re-activation, got {:?}",
        thickness_react
    );
    let snap3 = engine
        .snapshot()
        .expect("snapshot after else re-activation");
    let (_, det3) = snap3.values.get(&thickness_id).expect("thickness in snap3");
    assert_eq!(
        *det3,
        DeterminacyState::Determined,
        "Step 3: DeterminacyState should be Determined after else re-activation"
    );
}

/// When eval() runs with guard=false, Auto-kind members in the members list
/// should get DeterminacyState::Auto (not Undetermined) in the snapshot.
/// This makes eval() consistent with edit_param()'s Auto-skip logic.
#[test]
fn eval_guard_false_auto_param_gets_auto_determinacy() {
    let _active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Auto param 'thickness' as a guarded member (kind=Auto, no default_expr)
    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            // Guard defaults to false → members are inactive
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .auto_param("S", "thickness", Type::length())
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![thickness_decl], // members (inactive because guard=false)
            vec![],
            vec![],
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // eval() with guard=false: thickness is in deactivated members
    let _result = engine.eval(&module);

    // Auto-kind cell should have DeterminacyState::Auto even when deactivated
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&thickness_id)
        .expect("thickness should be in snapshot after eval");
    assert_eq!(
        *snap_val,
        Value::Undef,
        "Deactivated Auto param should have Value::Undef"
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Auto,
        "Deactivated Auto param should have DeterminacyState::Auto, not Undetermined"
    );
}

/// Mirror of eval_guard_false_auto_param_gets_auto_determinacy but for else_members:
/// When guard defaults to true, else_members are deactivated. Auto-kind cells
/// in else_members should get DeterminacyState::Auto (not Undetermined).
#[test]
fn eval_guard_true_auto_param_in_else_gets_auto_determinacy() {
    let _active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Auto param 'thickness' as an else_member (kind=Auto, no default_expr)
    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            // Guard defaults to true → else_members are inactive
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .auto_param("S", "thickness", Type::length())
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![],               // members
            vec![],               // constraints
            vec![thickness_decl], // else_members (inactive because guard=true)
            vec![],               // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // eval() with guard=true: thickness is in deactivated else_members
    let _result = engine.eval(&module);

    // Auto-kind cell should have DeterminacyState::Auto even when deactivated
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&thickness_id)
        .expect("thickness should be in snapshot after eval");
    assert_eq!(
        *snap_val,
        Value::Undef,
        "Deactivated Auto param in else_members should have Value::Undef"
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Auto,
        "Deactivated Auto param in else_members should have DeterminacyState::Auto, not Undetermined"
    );
}

/// Regression test: regular Param-kind members must still get Undetermined when
/// their guard is false during eval(). The Auto-kind fix should not affect normal params.
#[test]
fn eval_guard_false_regular_param_still_undetermined() {
    let _active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let width_id = ValueCellId::new("S", "width");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Regular Param-kind member 'width' with a default value
    let width_decl = make_param_decl("S", "width", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            // Guard defaults to false → members are inactive
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![width_decl], // members (inactive because guard=false)
            vec![],
            vec![],
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // eval() with guard=false: width is in deactivated members
    let _result = engine.eval(&module);

    // Regular Param-kind cell should still have DeterminacyState::Undetermined
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&width_id)
        .expect("width should be in snapshot after eval");
    assert_eq!(
        *snap_val,
        Value::Undef,
        "Deactivated regular Param should have Value::Undef"
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Undetermined,
        "Deactivated regular Param should have DeterminacyState::Undetermined"
    );
}

/// Edge case: when the guard expression evaluates to Undef (neither true nor false),
/// both members and else_members are deactivated. Auto-kind cells in this case
/// should get DeterminacyState::Auto, not Undetermined.
#[test]
fn eval_guard_undef_auto_param_gets_auto_determinacy() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");
    let depth_id = ValueCellId::new("S", "depth");

    // Guard expression references an Auto param (starts Undef, no solver)
    let guard_expr = value_ref_typed("S", "flag", Type::Bool);

    // Auto param 'thickness' in members
    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Auto param 'depth' in else_members
    let depth_decl = ValueCellDecl {
        id: depth_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "flag", Type::Bool)
        .auto_param("S", "thickness", Type::length())
        .auto_param("S", "depth", Type::length())
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![thickness_decl], // members (deactivated: guard is Undef)
            vec![],
            vec![depth_decl], // else_members (deactivated: guard is Undef)
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let result = engine.eval(&module);

    // Guard should be Undef
    assert_eq!(
        result.values.get(&guard_id),
        Some(&Value::Undef),
        "guard cell should be Undef when referencing unresolved Auto param"
    );

    let snapshot = engine.snapshot().expect("snapshot should exist after eval");

    // Auto-kind member in members: should get Auto determinacy
    let (thick_val, thick_det) = snapshot
        .values
        .get(&thickness_id)
        .expect("thickness should be in snapshot");
    assert_eq!(*thick_val, Value::Undef);
    assert_eq!(
        *thick_det,
        DeterminacyState::Auto,
        "Auto member deactivated by Undef guard should have DeterminacyState::Auto"
    );

    // Auto-kind member in else_members: should also get Auto determinacy
    let (depth_val, depth_det) = snapshot
        .values
        .get(&depth_id)
        .expect("depth should be in snapshot");
    assert_eq!(*depth_val, Value::Undef);
    assert_eq!(
        *depth_det,
        DeterminacyState::Auto,
        "Auto else_member deactivated by Undef guard should have DeterminacyState::Auto"
    );
}

/// Regression test: topology_fingerprint round-trips correctly through guard state transitions.
///
/// Verifies that `edit_param` re-elaboration correctly reflects guard cell values in the
/// topology_fingerprint. If guard cells were silently defaulting to `Value::Undef` (rather
/// than reading the actual guard value from the values map), all guard states would produce
/// the same fingerprint hash (hash of Undef), causing stale incremental caches.
///
/// Both `eval()` and `edit_param()` now use the same `"guard:{}={:?}"` format string for
/// guard-state hashing, so cross-path fingerprints are directly comparable. F1 (from eval)
/// must equal F3 (from edit_param with the same guard=true state).
///
/// Sequence: eval(true) → F1, edit_param(false) → F2 ≠ F1, edit_param(true) → F3 == F1,
///           edit_param(false) → F4 == F2.
#[test]
fn edit_param_guard_fingerprint_round_trips() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let y_decl = make_param_decl("S", "y", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members (active when guard is true)
            vec![],       // constraints
            vec![y_decl], // else_members (active when guard is false)
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Phase 1: initial eval with guard=true
    let result1 = engine.eval(&module);
    let f1 = engine.snapshot().unwrap().topology_fingerprint;

    // Sanity-check initial state (use EvalResult.values which is HashMap<_, Value>)
    assert_eq!(
        result1.values.get(&x_id),
        Some(&Value::length(0.005)),
        "x should be 5mm when guard is true"
    );
    assert_eq!(
        result1.values.get(&y_id),
        Some(&Value::Undef),
        "y should be Undef when guard is true"
    );

    // Phase 2: edit guard to false → fingerprint F2, must differ from F1
    let result2 = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param(active=false) should succeed");
    let f2 = engine.snapshot().unwrap().topology_fingerprint;

    assert_ne!(
        f1, f2,
        "topology_fingerprint must change when guard transitions true→false"
    );

    // Sanity-check: x deactivated, y active
    assert_eq!(
        result2.values.get(&x_id),
        Some(&Value::Undef),
        "x should be Undef after guard transitions to false"
    );
    assert_eq!(
        result2.values.get(&y_id),
        Some(&Value::length(0.01)),
        "y should be 10mm after guard transitions to false"
    );

    // Phase 3: edit guard back to true → fingerprint F3 must differ from F2.
    // KEY ASSERTION: if guard cells silently fell back to Undef, F3 would equal F2
    // (both would hash Undef), making the fingerprint insensitive to guard state.
    let result3 = engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param(active=true) should succeed");
    let f3 = engine.snapshot().unwrap().topology_fingerprint;

    assert_ne!(
        f2, f3,
        "topology_fingerprint must change when guard transitions false→true (guard state must be reflected in fingerprint)"
    );

    // Cross-path consistency: eval(true) → F1 must equal edit_param(true) → F3.
    // Both use the same "guard:{}={:?}" format string, so same guard state → same fingerprint.
    assert_eq!(
        f1, f3,
        "topology_fingerprint from eval(true) must equal edit_param(true): cross-path consistency"
    );

    // Verify values returned to initial state
    assert_eq!(
        result3.values.get(&x_id),
        Some(&Value::length(0.005)),
        "x should be 5mm again after guard returns to true"
    );
    assert_eq!(
        result3.values.get(&y_id),
        Some(&Value::Undef),
        "y should be Undef again after guard returns to true"
    );

    // Phase 4: edit guard back to false → fingerprint F4 must equal F2 (round-trip).
    // This verifies consistency within the edit_param code path.
    let _result4 = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param(active=false) second time should succeed");
    let f4 = engine.snapshot().unwrap().topology_fingerprint;

    assert_eq!(
        f2, f4,
        "topology_fingerprint must be the same for identical guard states (false==false round-trip)"
    );
}

/// Cross-path consistency: topology_fingerprint from eval() must equal the fingerprint
/// from edit_param() when both represent the same logical guard state.
///
/// If `eval()` and `edit_param()` use different hash format strings for guard state,
/// the same logical guard value (e.g. `Bool(true)`) produces different hashes depending
/// on which code path computed it, causing spurious cache misses or stale incremental
/// caches when switching between paths.
///
/// Sequence: eval(true) → F1, edit_param(false) → F2 ≠ F1, edit_param(true) → F3.
/// Asserts: F1 == F3 (eval and edit_param produce identical fingerprints for same state).
///
/// **Why this test exists alongside `edit_param_guard_fingerprint_round_trips`:**
/// `edit_param_guard_fingerprint_round_trips` comprehensively covers the round-trip
/// property (F2==F4) and validates member values at each phase, but its cross-path
/// assertion (F1==F3) is one of many assertions. This focused test is a minimal
/// regression reproducer for the specific cross-path hash-format bug fixed in task 1112,
/// where `eval()` and `edit_param()` used different format strings for guard-state
/// hashing, causing identical logical states to produce different fingerprints.
#[test]
fn eval_edit_param_guard_fingerprint_cross_path_consistency() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let y_decl = make_param_decl("S", "y", Type::length(), Value::length(0.01));

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members (active when guard is true)
            vec![],       // constraints
            vec![y_decl], // else_members (active when guard is false)
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // F1: initial eval with guard=true
    engine.eval(&module);
    let f1 = engine.snapshot().unwrap().topology_fingerprint;

    // F2: edit guard to false (topology changes, fingerprint must differ from F1)
    engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param(active=false) should succeed");
    let f2 = engine.snapshot().unwrap().topology_fingerprint;
    assert_ne!(f1, f2, "fingerprint must change on guard true→false");

    // F3: edit guard back to true — must equal F1 (same logical state, same code path → same hash)
    engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param(active=true) should succeed");
    let f3 = engine.snapshot().unwrap().topology_fingerprint;

    assert_eq!(
        f1, f3,
        "topology_fingerprint from eval(guard=true) must equal edit_param(guard=true): \
         eval() and edit_param() must use the same guard-state hash format"
    );
}

/// Multi-guarded-group fingerprint disambiguation.
///
/// Verifies that two guard cells with the same *value* but different *identities*
/// produce different topology fingerprints. The hash format `"guard:{}={:?}"` includes
/// the guard cell ID, so `__guard_0=Bool(true), __guard_1=Bool(false)` must hash
/// differently from `__guard_0=Bool(false), __guard_1=Bool(true)` even though both
/// represent "one true, one false" guard states.
///
/// This property prevents spurious cache hits when two guards have swapped values —
/// if the fingerprint only captured the *multiset* of guard values, a swapped-guard
/// topology would incorrectly reuse an incompatible cached evaluation.
///
/// Template: two boolean params `active_a` (controls `__guard_0`) and `active_b`
/// (controls `__guard_1`), each with its own set of members.
///
/// Sequence: eval(a=T, b=T) → F_tt, edit_param(b=F) → F_tf, edit_param(a=F) +
/// edit_param(b=T) → F_ft.
/// Asserts: F_tt ≠ F_tf, F_tt ≠ F_ft, F_tf ≠ F_ft (all three pairwise distinct).
#[test]
fn multi_guard_fingerprint_disambiguates_by_guard_identity() {
    let active_a_id = ValueCellId::new("S", "active_a");
    let active_b_id = ValueCellId::new("S", "active_b");
    let guard_a_id = ValueCellId::new("S", "__guard_0");
    let guard_b_id = ValueCellId::new("S", "__guard_1");

    let guard_a_expr = value_ref_typed("S", "active_a", Type::Bool);
    let guard_b_expr = value_ref_typed("S", "active_b", Type::Bool);

    // Members for guard_0 (controlled by active_a)
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    // Members for guard_1 (controlled by active_b)
    let z_decl = make_param_decl("S", "z", Type::length(), Value::length(0.015));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active_a",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .param(
            "S",
            "active_b",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_a_expr,
            guard_a_id.clone(),
            vec![x_decl], // members: active when guard_0 is true
            vec![],       // constraints
            vec![],       // else_members
            vec![],       // else_constraints
        )
        .guarded_group(
            guard_b_expr,
            guard_b_id.clone(),
            vec![z_decl], // members: active when guard_1 is true
            vec![],       // constraints
            vec![],       // else_members
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // F_tt: initial eval with both guards true (active_a=T, active_b=T)
    engine.eval(&module);
    let f_tt = engine.snapshot().unwrap().topology_fingerprint;

    // F_tf: active_a=T, active_b=F
    engine
        .edit_param(active_b_id.clone(), Value::Bool(false))
        .expect("edit_param(active_b=false) should succeed");
    let f_tf = engine.snapshot().unwrap().topology_fingerprint;

    // Transition to active_a=F, active_b=T: first set a=false (gives FF), then b=true (gives FT)
    engine
        .edit_param(active_a_id.clone(), Value::Bool(false))
        .expect("edit_param(active_a=false) should succeed");
    engine
        .edit_param(active_b_id.clone(), Value::Bool(true))
        .expect("edit_param(active_b=true) should succeed");
    let f_ft = engine.snapshot().unwrap().topology_fingerprint;

    // All three states must produce distinct fingerprints.
    // F_tt vs F_tf: different because guard_1 changed (true → false)
    assert_ne!(
        f_tt, f_tf,
        "F_tt must differ from F_tf: guard_1 changed from true to false"
    );
    // F_tt vs F_ft: different because guard_0 changed (true → false) and guard_1 changed (true → true, no wait...)
    // Actually F_tt: g0=T g1=T; F_ft: g0=F g1=T — guard_0 changed
    assert_ne!(
        f_tt, f_ft,
        "F_tt must differ from F_ft: guard_0 changed from true to false"
    );
    // KEY: F_tf vs F_ft — both have one true and one false guard, but WHICH guard holds WHICH value differs.
    // F_tf: guard_0=Bool(true),  guard_1=Bool(false)
    // F_ft: guard_0=Bool(false), guard_1=Bool(true)
    // The "guard:{}={:?}" format includes guard cell ID, so these hash differently.
    assert_ne!(
        f_tf, f_ft,
        "F_tf must differ from F_ft: guard_0 and guard_1 have swapped values; \
         fingerprint must distinguish which specific guard cell holds which value, \
         not just the multiset of guard values"
    );
}

/// Regression test: regular Param-kind else_members must still be set to Undef when
/// the else branch deactivates via edit_param. The Auto-skip fix must not affect
/// normal params.
///
/// Mirrors `edit_param_guard_false_still_deactivates_regular_params` (members-side)
/// with `members` ↔ `else_members` and the guard transition reversed (false→true).
/// Closes the missing quadrant in the regular-Param × {members, else_members}
/// × {eval, edit_param} matrix.
#[test]
fn edit_param_guard_false_to_true_still_deactivates_regular_params_in_else_members() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let width_id = ValueCellId::new("S", "width");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Regular Param-kind else_member 'width' with a default value
    let width_decl = make_param_decl("S", "width", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            // Guard defaults to false → else_members are active
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![],           // members (active when true, empty here)
            vec![],           // constraints
            vec![width_decl], // else_members (active because guard=false)
            vec![],           // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval with guard=false; width (in else_members) should be 10mm = 0.01
    let initial_result = engine.eval(&module);
    assert_eq!(
        initial_result.values.get(&width_id),
        Some(&Value::length(0.01)),
        "width should be 0.01 (10mm SI) when else branch is active (guard=false)"
    );

    // Edit 'active' from false to true — else branch deactivates
    let edit_result = engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param should succeed");

    // Regular Param else_member should be Undef after else branch deactivation
    assert_eq!(
        edit_result.values.get(&width_id),
        Some(&Value::Undef),
        "Regular Param 'width' in else_members should be Undef after guard changed to true"
    );
}

/// Regression test: regular Param-kind else_members must still get Undetermined
/// when their else branch is inactive at eval() time (guard=true). The Auto-kind
/// fix must not affect normal params.
///
/// Mirrors `eval_guard_false_regular_param_still_undetermined` (members-side) with
/// `members` ↔ `else_members` and guard default flipped to true. Closes the missing
/// quadrant in the regular-Param × {members, else_members} × {eval, edit_param} matrix.
#[test]
fn eval_guard_true_else_member_regular_param_still_undetermined() {
    let _active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let width_id = ValueCellId::new("S", "width");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Regular Param-kind else_member 'width' with a default value
    let width_decl = make_param_decl("S", "width", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            // Guard defaults to true → else_members are inactive
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![],           // members (active when true, empty here)
            vec![],           // constraints
            vec![width_decl], // else_members (inactive because guard=true)
            vec![],           // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // eval() with guard=true: width is in deactivated else_members
    let _result = engine.eval(&module);

    // Regular Param-kind cell should still have DeterminacyState::Undetermined
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&width_id)
        .expect("width should be in snapshot after eval");
    assert_eq!(
        *snap_val,
        Value::Undef,
        "Deactivated regular Param in else_members should have Value::Undef"
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Undetermined,
        "Deactivated regular Param in else_members should have DeterminacyState::Undetermined"
    );
}

/// Integration test: exercises all four `deactivate_if_not_auto` call sites in
/// `edit_param` via a guarded group with mixed Auto+Param members in both
/// `members` and `else_members`.
///
/// Two `edit_param` transitions (true→false, false→true) cover:
/// - Site #1 (Block A members deactivate, guard→false): a1 Auto-skip
/// - Site #2 (Block A else_members deactivate, guard→true): a2 Auto-skip
/// - Site #3 (Block B members, guard changed): mirrors site #1
/// - Site #4 (Block B else_members, guard changed): mirrors site #2
///
/// A refactor that drops any one of the four `deactivate_if_not_auto` calls in
/// `edit_param` (engine_edit.rs lines 472, 492, 723, 746) will break at least
/// one transition's expected state.
///
/// Supersedes the shallow unit test `deactivate_if_not_auto_guard_group_mixed_members`
/// in `engine_edit.rs::tests`, which only iterates the helper over a slice and
/// never reaches the real `edit_param` call sites.
#[test]
fn edit_param_guard_group_mixed_members_via_edit_param() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let a1_id = ValueCellId::new("S", "a1");
    let a2_id = ValueCellId::new("S", "a2");
    let p1_id = ValueCellId::new("S", "p1");
    let p2_id = ValueCellId::new("S", "p2");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Auto param decls (kind=Auto, no default_expr)
    let a1_decl = ValueCellDecl {
        id: a1_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };
    let a2_decl = ValueCellDecl {
        id: a2_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Regular Param decls (default 11mm = 0.011 SI, 13mm = 0.013 SI)
    let p1_decl = make_param_decl("S", "p1", Type::length(), Value::length(0.011));
    let p2_decl = make_param_decl("S", "p2", Type::length(), Value::length(0.013));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        // Top-level Auto params so the resolution phase finds them
        .auto_param("S", "a1", Type::length())
        .auto_param("S", "a2", Type::length())
        // Top-level constraints so the resolution phase can match them
        .constraint(
            "S",
            0,
            Some("a1_gt_2mm"),
            gt(value_ref("S", "a1"), literal(mm(2.0))),
        )
        .constraint(
            "S",
            1,
            Some("a2_gt_3mm"),
            gt(value_ref("S", "a2"), literal(mm(3.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![a1_decl, p1_decl], // members (active when guard=true)
            vec![],                 // constraints
            vec![a2_decl, p2_decl], // else_members (active when guard=false)
            vec![],                 // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Mock solver: always returns a1=5mm, a2=7mm
    let mut solved_values = HashMap::new();
    solved_values.insert(a1_id.clone(), mm(5.0));
    solved_values.insert(a2_id.clone(), mm(7.0));
    let solver = MockConstraintSolver::new_solved(solved_values);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // ── Phase 1: initial eval (guard=true) ──────────────────────────────────
    let result1 = engine.eval(&module);

    // a1: active member, solver resolved to 5mm
    let a1_p1 = result1.values.get(&a1_id);
    assert!(
        matches!(a1_p1, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Phase 1: a1 should be 5mm, got {:?}",
        a1_p1
    );
    // p1: active member, default 11mm
    assert_eq!(
        result1.values.get(&p1_id),
        Some(&Value::length(0.011)),
        "Phase 1: p1 should be 11mm"
    );
    // a2: inactive else_member (guard=true → else branch inactive).
    // Canonical rule: Auto cell lifecycle is owned by the solver. The post-solver pass
    // in engine_eval.rs now skips Auto cells on the inactive branch (same as
    // deactivate_if_not_auto in engine_edit.rs), so a2 retains its solver-resolved
    // value of 7mm with DeterminacyState::Determined.
    let a2_p1 = result1.values.get(&a2_id);
    assert!(
        matches!(a2_p1, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.007).abs() < 1e-10),
        "Phase 1: a2 should retain solver-resolved 7mm (inactive else_member, Auto-skip), got {:?}",
        a2_p1
    );
    let snap1 = engine.snapshot().expect("snapshot after eval");
    let (_, a2_det1) = snap1.values.get(&a2_id).expect("a2 in snapshot after eval");
    assert_eq!(
        *a2_det1,
        DeterminacyState::Determined,
        "Phase 1: a2 DeterminacyState should be Determined (solver resolved, Auto-skip preserves it)"
    );
    // p2: else_member deactivated → Undef
    assert_eq!(
        result1.values.get(&p2_id),
        Some(&Value::Undef),
        "Phase 1: p2 should be Undef (else_member inactive when guard=true)"
    );

    // ── Phase 2: edit_param(active, false) — guard true→false ──────────────
    // Block A members deactivate: a1 Auto-skip (site #1), p1→Undef
    // Block A else_members activate: a2 no default_expr (stays 7mm), p2→13mm
    // Block B fires (guard changed true→false): same via sites #3/#4
    let result2 = engine
        .edit_param(active_id.clone(), Value::Bool(false))
        .expect("edit_param(active=false) should succeed");

    // a1: Auto-skip in members deactivate (site #1) → 5mm preserved
    let a1_p2 = result2.values.get(&a1_id);
    assert!(
        matches!(a1_p2, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Phase 2: a1 should retain 5mm after guard→false (Auto-skip site #1), got {:?}",
        a1_p2
    );
    let snap2 = engine.snapshot().expect("snapshot after guard→false");
    let (_, a1_det2) = snap2
        .values
        .get(&a1_id)
        .expect("a1 in snapshot after guard→false");
    assert_eq!(
        *a1_det2,
        DeterminacyState::Determined,
        "Phase 2: a1 DeterminacyState must remain Determined after Auto-skip"
    );
    // p1: Param in members → Undef after deactivation
    assert_eq!(
        result2.values.get(&p1_id),
        Some(&Value::Undef),
        "Phase 2: p1 should be Undef after members deactivate"
    );
    // a2: else_members activate; Auto with no default_expr, retained 7mm from Phase 1
    // (deactivate_if_not_auto Auto-skip preserved the solver value). Activating an Auto
    // cell with no default_expr leaves the value as-is (no default to restore), so a2
    // retains its 7mm/Determined state.
    let a2_p2 = result2.values.get(&a2_id);
    assert!(
        matches!(a2_p2, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.007).abs() < 1e-10),
        "Phase 2: a2 should retain 7mm after else_members activate (solver-resolved value preserved), got {:?}",
        a2_p2
    );
    let (_, a2_det2) = snap2
        .values
        .get(&a2_id)
        .expect("a2 in snapshot after guard→false");
    assert_eq!(
        *a2_det2,
        DeterminacyState::Determined,
        "Phase 2: a2 DeterminacyState must remain Determined (Auto-skip preserved solver value)"
    );
    // p2: else_member activates → default 13mm
    assert_eq!(
        result2.values.get(&p2_id),
        Some(&Value::length(0.013)),
        "Phase 2: p2 should be 13mm after else_members activated"
    );

    // ── Phase 3: edit_param(active, true) — guard false→true ───────────────
    // Block A members activate: a1 no default_expr (stays 5mm), p1→11mm
    // Block A else_members deactivate: a2 Auto-skip (site #2), p2→Undef
    // Block B fires (guard changed false→true): same via sites #3/#4
    let result3 = engine
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param(active=true) should succeed");

    // a1: members activate; Auto with no default_expr → stays 5mm
    let a1_p3 = result3.values.get(&a1_id);
    assert!(
        matches!(a1_p3, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Phase 3: a1 should retain 5mm after guard→true (no default_expr), got {:?}",
        a1_p3
    );
    // p1: Param in members → default 11mm after re-activation
    assert_eq!(
        result3.values.get(&p1_id),
        Some(&Value::length(0.011)),
        "Phase 3: p1 should be 11mm after members re-activated"
    );
    // a2: Auto-skip in else_members deactivate (site #2) → 7mm preserved.
    // deactivate_if_not_auto skips a2 (Auto kind) so the snapshot is not touched.
    let a2_p3 = result3.values.get(&a2_id);
    assert!(
        matches!(a2_p3, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.007).abs() < 1e-10),
        "Phase 3: a2 should retain 7mm after else_members deactivate (Auto-skip preserves solver value), got {:?}",
        a2_p3
    );
    let snap3 = engine.snapshot().expect("snapshot after guard→true");
    let (_, a2_det3) = snap3
        .values
        .get(&a2_id)
        .expect("a2 in snapshot after guard→true");
    assert_eq!(
        *a2_det3,
        DeterminacyState::Determined,
        "Phase 3: a2 DeterminacyState must remain Determined after else Auto-skip"
    );
    // p2: Param in else_members → Undef after deactivation
    assert_eq!(
        result3.values.get(&p2_id),
        Some(&Value::Undef),
        "Phase 3: p2 should be Undef after else_members deactivated"
    );
}

/// Block-A-only Auto-skip regression test (task 750).
///
/// Block A in `edit_param` fires when `has_dirty_guards` — meaning the guard
/// cell is in the dirty cone via a dependency edit, even when the guard VALUE
/// does not change. Block B fires only when the guard *value* changes.
///
/// This test isolates the Block-A path by using a numeric comparison guard
/// (`count > 100mm`): editing `count` from 5mm → 10mm keeps the guard false
/// (10mm is still < 100mm), so Block A fires (guard cell is dirty) but Block B
/// does NOT (guard value unchanged).
///
/// Verifies that the Auto-skip in Block A's members-side call site
/// (engine_edit.rs line 472) preserves the solver-resolved `thickness` value
/// independently of Block B. If a future refactor drops the Auto-skip from
/// Block A's members-side call site (line 472) while keeping Block B's, this
/// test catches it — the existing
/// `edit_param_guard_false_preserves_solver_auto_param` test would not, because
/// that test fires both blocks together. The else_members-side Block A call site
/// (line 492) is covered by `edit_param_guard_group_mixed_members_via_edit_param`
/// instead (this test uses empty else_members).
#[test]
fn edit_param_block_a_only_preserves_auto_when_guard_value_unchanged() {
    let count_id = ValueCellId::new("S", "count");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    // Guard expression: count > 100mm — false when count = 5mm or 10mm
    let guard_expr = gt(
        value_ref_typed("S", "count", Type::length()),
        literal(mm(100.0)),
    );

    // Auto param 'thickness' as a guarded member (kind=Auto, no default_expr)
    let thickness_decl = ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "count",
            Type::length(),
            Some(CompiledExpr::literal(mm(150.0), Type::length())),
        )
        // Top-level auto_param so the resolution phase finds it
        .auto_param("S", "thickness", Type::length())
        // Top-level constraint so the resolution phase can match it
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![thickness_decl], // members (active only when count > 100mm, i.e. guard=true)
            vec![],               // constraints
            vec![],               // else_members
            vec![],               // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Mock solver: always resolves thickness to 5mm (0.005 SI)
    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved_values);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval with guard=true (count=150mm > 100mm); thickness is the active member.
    // Post-solver re-eval leaves active-branch Auto cells untouched → thickness=5mm.
    let initial_result = engine.eval(&module);
    let thickness_val = initial_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "thickness should be 0.005 SI (5mm) after initial eval, got {:?}",
        thickness_val
    );

    // edit_param(count, 200mm) — still > 100mm, guard remains true.
    // __guard_0 enters the dirty cone via count's reverse index → Block A fires.
    // guard value (true → true) → Block B does NOT fire.
    let edit_result = engine
        .edit_param(count_id.clone(), mm(200.0))
        .expect("edit_param should succeed");

    // Auto param must retain solver-resolved value (Block A Auto-skip at line 472).
    let thickness_after = edit_result.values.get(&thickness_id);
    assert!(
        matches!(thickness_after, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "Auto param 'thickness' should retain 0.005 SI after Block-A-only guard update, got {:?}",
        thickness_after
    );

    // DeterminacyState in snapshot must remain Determined (Block A Auto-skip must apply)
    let snapshot = engine
        .snapshot()
        .expect("snapshot should exist after edit_param");
    let (snap_val, snap_det) = snapshot
        .values
        .get(&thickness_id)
        .expect("thickness in snapshot after Block-A-only guard update");
    assert!(
        matches!(snap_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "thickness should retain 0.005 SI in snapshot, got {:?}",
        snap_val
    );
    assert_eq!(
        *snap_det,
        DeterminacyState::Determined,
        "Auto param DeterminacyState must remain Determined after Block-A-only path"
    );
}

// ── Phase 1 & 3 performance: skip unchanged guarded groups (edit_param) ──────

/// Performance lock for `edit_param`: when a single guard param flips, only the
/// affected guarded group must be re-elaborated in Phase 1 and Phase 3.
///
/// Test design:
/// - Module: structure S with 10 independent `where uN { let xN = 1mm }` groups,
///   each guarded by `uN: Bool = true`. eval(module) → all x0..x9 = 1mm.
/// - edit_param(u3, Bool(false)): flips u3 from true → false. Group 3's guard
///   expression evaluates to false → x3 deactivates to Undef. Groups 0,1,2,4..9
///   are unaffected (their guard params uN remain true).
///
/// `has_dirty_guards` fires (Phase 1) because group 3's guard cell is in the
/// dirty cone of the edit. `guard_changed` fires (Phase 3) because group 3's
/// guard value changes from true to false. Phase 1 processes group 3 and records
/// it in `phase1_reelaborated`; Phase 3 sees it in the set and skips it →
/// counter = 1.
///
/// Without per-group skip (pre-task-2088):          counter = 10 (Phase 1) + 10 (Phase 3) = 20.
/// With per-group skip, no cross-phase dedup:       counter =  1 (Phase 1) +  1 (Phase 3) =  2.
/// With cross-phase dedup via phase1_reelaborated:  counter =  1 (Phase 1) +  0 (Phase 3) =  1.
///
/// Task 2088 — edit_param Phase 1 & 3 per-group skip.
/// Task 2140 — cross-phase dedup via `phase1_reelaborated` set.
#[test]
fn edit_param_phase1_and_3_skip_unchanged_guarded_groups() {
    let module_src = ten_bool_guarded_groups("u3");
    let module = parse_and_compile(&module_src);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval: all u0..u9 = true → all guards true → all x0..x9 = 1mm.
    let initial = engine.eval(&module);

    // Snapshot the pre-edit values of the unaffected cells (x0,x1,x2,x4..x9).
    let unaffected_ids: Vec<ValueCellId> = (0..10)
        .filter(|&n| n != 3)
        .map(|n| ValueCellId::new("S", format!("x{}", n)))
        .collect();
    let pre_edit_values: Vec<Option<Value>> = unaffected_ids
        .iter()
        .map(|id| initial.values.get(id).cloned())
        .collect();

    // Flip u3 from true → false. Phase 1 fires (group 3's guard cell is in the
    // dirty cone); Phase 3 fires (group 3's guard value changes true → false).
    let u3_id = ValueCellId::new("S", "u3");
    let edited = engine
        .edit_param(u3_id, Value::Bool(false))
        .expect("edit_param must succeed");

    // (a) x3 deactivates to Undef (guard = !true = false, members branch inactive).
    let x3_id = ValueCellId::new("S", "x3");
    assert!(
        matches!(edited.values.get(&x3_id), Some(Value::Undef)),
        "x3 must deactivate to Undef when guard u3=false; got {:?}",
        edited.values.get(&x3_id)
    );

    // (b) Unaffected cells retain their pre-edit values.
    for (id, pre_val) in unaffected_ids.iter().zip(pre_edit_values.iter()) {
        let post_val = edited.values.get(id);
        assert_eq!(
            post_val,
            pre_val.as_ref(),
            "unaffected cell {id} must retain pre-edit value after edit_param(u3, false); \
             pre={:?}, post={:?}",
            pre_val,
            post_val
        );
    }

    // (c) Performance lock: only Phase 1 re-elaborates group 3. Phase 3 sees
    // group 3 in `phase1_reelaborated` (task 2140 cross-phase dedup set) and
    // skips it. The other 9 groups have unchanged guard values (uN=true) and
    // are skipped by the per-group skip (task 2088). Expected total:
    // 1 (Phase 1) + 0 (Phase 3) = exactly 1.
    // Without the skip optimisation: 10 (Phase 1) + 10 (Phase 3) = 20.
    // With per-group skip but no cross-phase dedup: 1 (Phase 1) + 1 (Phase 3) = 2.
    let counter = engine.last_guard_phase_group_evals();
    assert_eq!(
        counter, 1,
        "expected exactly 1 non-skipped guard-phase group iteration \
         (Phase 1 processes group 3; Phase 3 skips it via phase1_reelaborated set); \
         got {} — if 0, the counter increment is missing from Phase 1 \
         (instrumentation dropped); if > 1, cross-phase dedup is broken \
         (2 specifically means Phase 3 is redoing Phase 1's work)",
        counter
    );
}

// ── edit_param Phase 1-only: same-value edit on structure_controlling cell ─────

/// Phase 1 fires (u3 is structure_controlling and edit_param unconditionally
/// inserts it into changed_set) but every per-group skip applies (guard VALUE
/// unchanged for all groups), so no group is re-elaborated. Phase 3 never
/// iterates (no guard value changed). Overall `last_guard_phase_group_evals()`
/// == 0. This is the edit_param analogue of the edit_source T1 test.
///
/// Scenario: Same 10-group fixture as `edit_param_phase1_and_3_skip_unchanged_guarded_groups`.
/// `edit_param(u3, Bool(true))` — setting u3 to its CURRENT value (true → true).
/// `changed_set` unconditionally contains u3 (engine_edit.rs:424-425), the
/// dirty cone includes __guard_3 as u3's dependent, and u3 is
/// structure_controlling → `has_dirty_guards` true. Every group's guard VALUE
/// stays true (u3's value is unchanged). Phase 1 enters the body but the
/// per-group skip at engine_edit.rs:629 (`if old_guard_val == Some(&guard_val)
/// { continue; }`) suppresses all 10 groups. Phase 3: `guard_changed` false
/// → never iterates.
///
/// Note: scenario (b) from task 2138 — Phase 3 fires while Phase 1 does NOT —
/// is already covered by `edit_param_phase3_fires_for_auto_driven_guard_change`
/// at guard_eval.rs:2186 (counter == 1). T4 is the edit_param analogue of T1
/// and covers the complementary per-group skip path for same-value edits.
///
/// Regression catches:
/// == 1 → per-group skip regressed (over-fires on no-op edits to
///         structure_controlling cells, e.g. old_guard_val == Some(&guard_val)
///         arm dropped at engine_edit.rs:629).
/// > 1  → multi-group regression or Phase 3 guard_changed gate regression.
///
/// Task 2138 — edit_param Phase-1-only perf lock (T4).
#[test]
fn edit_param_phase1_fires_but_skips_when_same_value_edit_on_structure_controlling_cell() {
    let module_src = ten_bool_guarded_groups("u3");
    let module = parse_and_compile(&module_src);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Initial eval: all u0..u9 = true → all guards true → all x0..x9 = 1mm.
    let initial = engine.eval(&module);

    // Snapshot all 10 cell values before the no-op edit.
    let cell_ids: Vec<ValueCellId> = (0..10)
        .map(|n| ValueCellId::new("S", format!("x{}", n)))
        .collect();
    let pre_edit_values: Vec<Option<Value>> = cell_ids
        .iter()
        .map(|id| initial.values.get(id).cloned())
        .collect();

    // Edit u3 to its CURRENT value (true → true). Semantically a no-op, but
    // edit_param unconditionally inserts u3 into changed_set (engine_edit.rs:424-425),
    // making has_dirty_guards true (u3 is structure_controlling via guard cell
    // __guard_3 which is in structure_controlling per reify-compiler/src/guards.rs:242).
    let u3_id = ValueCellId::new("S", "u3");
    let edited = engine
        .edit_param(u3_id, Value::Bool(true))
        .expect("edit_param must succeed");

    // (a) All 10 cells must retain their pre-edit values (no guard value
    // changed, so no deactivation or re-evaluation of any member).
    for (id, pre_val) in cell_ids.iter().zip(pre_edit_values.iter()) {
        let post_val = edited.values.get(id);
        assert_eq!(
            post_val,
            pre_val.as_ref(),
            "cell {id} must retain pre-edit value after no-op edit_param(u3, true); \
             pre={:?}, post={:?}",
            pre_val,
            post_val
        );
    }

    // (b) Performance lock: counter == 0.
    // has_dirty_guards fires Phase 1 (u3 is structure_controlling and in
    // changed_set). But all 10 groups' guard VALUES are unchanged (true → true)
    // so the per-group skip (`if old_guard_val == Some(&guard_val) { continue }`)
    // suppresses all 10. Phase 3: guard_changed false → never iterates.
    // Expected: 0 group iterations in total.
    //
    // Regression catches:
    // == 1 → per-group skip regressed for same-value edits on structure_controlling
    //         cells (old_guard_val == Some(&guard_val) arm dropped at
    //         engine_edit.rs:629; group 3 would be spuriously re-elaborated).
    // > 1  → multi-group regression or Phase 3 guard_changed gate regressed.
    let counter = engine.last_guard_phase_group_evals();
    assert_eq!(
        counter, 0,
        "expected 0 non-skipped guard-phase group iterations \
         (Phase 1 body runs via has_dirty_guards but per-group skip suppresses \
          all 10 groups since guard values are unchanged; Phase 3 never fires); \
         got {} — \
         if 1, per-group skip regressed for same-value edits on structure_controlling \
           cells (old_guard_val == Some(&guard_val) arm dropped at engine_edit.rs:629); \
         if > 1, multi-group regression or Phase 3 guard_changed gate regressed",
        counter
    );
}

// ── Phase 3 carve-out: resolver-driven guard change bypasses dedup ─────────────

/// Phase 3 fires for a group whose guard is flipped by solver resolution rather
/// than by Phase 1.
///
/// Test design:
/// - Structure with `auto depth: Length`, `param ref_depth: Length = 5mm`
///   (initially), constraint `depth >= ref_depth`, and guard `depth > 3mm`.
/// - `ref_depth` is NOT structure_controlling. When it is edited, has_dirty_guards
///   is false so Phase 1 does not run.
/// - The solver resolves depth = 2mm (second sequenced result). Wave2 re-evaluates
///   the guard cell to false. Phase 3 fires (guard changed) and deactivates m.
///
/// Verifies the "Phase 3 fires via Phase 3 path" carve-out documented in the
/// cross-phase dedup comment: `phase1_reelaborated` is empty, so Phase 3 is
/// NOT skipped by the dedup and correctly handles the resolver-driven flip.
/// Expected counter: 1 (Phase 3 only; Phase 1 did not run).
#[test]
fn edit_param_phase3_fires_for_auto_driven_guard_change() {
    let ref_depth_id = ValueCellId::new("S", "ref_depth");
    let depth_id = ValueCellId::new("S", "depth");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let m_id = ValueCellId::new("S", "m");

    // Guard expression: depth > 3mm.  The guard cell reads `depth` (auto).
    let guard_expr = gt(value_ref("S", "depth"), literal(mm(3.0)));

    // Member: simple 1mm constant.  It gets deactivated when guard = false.
    let m_decl = ValueCellDecl {
        id: m_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(1.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        // ref_depth: a param that the solver uses but that does NOT feed the guard
        .param("S", "ref_depth", Type::length(), Some(literal(mm(5.0))))
        // depth: auto param resolved by the solver
        .auto_param("S", "depth", Type::length())
        // constraint reads both depth and ref_depth → dirty when ref_depth changes
        .constraint(
            "S",
            0,
            Some("depth_ge_ref_depth"),
            ge(value_ref("S", "depth"), value_ref("S", "ref_depth")),
        )
        // guarded group: guard depends on `depth` (NOT on ref_depth)
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![m_decl], // members (active when guard = true)
            vec![],       // constraints
            vec![],       // else_members
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Sequenced solver: first call → depth = 5mm (initial eval);
    //                   second call → depth = 2mm (after edit_param(ref_depth, 2mm)).
    let mut solved1 = HashMap::new();
    solved1.insert(depth_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(depth_id.clone(), mm(2.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval: depth = 5mm (solver), guard = true (5mm > 3mm), m = 1mm.
    let initial = engine.eval(&module);
    assert!(
        matches!(initial.values.get(&m_id), Some(Value::Scalar { .. })),
        "initial eval: m should be active (1mm) when depth=5mm > 3mm, got {:?}",
        initial.values.get(&m_id),
    );

    // edit_param(ref_depth, 2mm): ref_depth is NOT structure_controlling so
    // Phase 1 does not fire (has_dirty_guards = false, phase1_reelaborated = {}).
    // Solver re-resolves depth = 2mm; wave2 re-evaluates the guard cell to false.
    // Phase 3 sees guard_changed, does NOT find guard_cell in phase1_reelaborated,
    // and correctly deactivates m.
    let edited = engine
        .edit_param(ref_depth_id, Value::length(0.002)) // 2mm in SI
        .expect("edit_param must succeed");

    // (a) m deactivated to Undef: guard flipped true→false via Phase 3.
    assert!(
        matches!(edited.values.get(&m_id), Some(Value::Undef)),
        "m must be Undef after guard flips false via Phase 3; got {:?}",
        edited.values.get(&m_id),
    );

    // (b) Counter: exactly 1 (Phase 3 path only; Phase 1 did not fire).
    let counter = engine.last_guard_phase_group_evals();
    assert_eq!(
        counter, 1,
        "expected counter == 1 (Phase 3 fires for the group, Phase 1 never ran); \
         got {} — 0 means Phase 3 also skipped (dedup too aggressive); \
         > 1 means extra elaboration occurred",
        counter
    );
}

/// Perf-lock regression guard for Phase 3 iterating and re-elaborating MULTIPLE
/// guarded groups in a single `edit_param` call (task 2145).
///
/// Design: two independent auto params (`depth_a`, `depth_b`), two ref params
/// (`ref_a = 5mm`, `ref_b = 5mm`), two constraints (`depth_a >= ref_a`,
/// `depth_b >= ref_b`), and two guarded groups:
///   - group A: guard `depth_a > 3mm`, member `m_a = 1mm`
///   - group B: guard `depth_b > 3mm`, member `m_b = 1mm`
///
/// A `SequencedMockConstraintSolver` returns:
///   - initial eval: `depth_a = depth_b = 5mm` (both guards true; members active).
///   - post-edit:    `depth_a = depth_b = 2mm` (both guards flip false).
///
/// Edit: `edit_param(ref_a, 2mm)`.  `ref_a` is NOT structure_controlling (it does
/// not appear in any guard expression), so Phase 1 does not fire and
/// `phase1_reelaborated` is empty.  The solver re-resolves both autos to 2mm;
/// wave2 re-evaluates both guard cells to false; Phase 3 fires (`guard_changed`)
/// and must iterate and re-elaborate BOTH groups.
///
/// Assertions:
/// - `m_a == Undef`: member A deactivated via Phase 3.
/// - `m_b == Undef`: member B deactivated via Phase 3 (the critical multi-group
///   assertion — if Phase 3's loop is truncated after the first group, m_b retains
///   its 1mm value and this assertion fails).
/// - `last_guard_phase_group_evals() == 2`: counter pins that Phase 3 processed
///   exactly two groups.
///   - == 0 → over-aggressive dedup (Phase 3 skipped everything)
///   - == 1 → loop truncation / iteration regression (task 2145 target)
///   - > 2 → extra or double elaboration
///
/// The test passes on both the pre-refactor (`.clone()`) and post-refactor
/// (field-splitting) code; its value is locking the multi-iteration path.
#[test]
fn edit_param_phase3_reelaborates_multiple_auto_driven_guard_groups() {
    let ref_a_id = ValueCellId::new("S", "ref_a");
    let depth_a_id = ValueCellId::new("S", "depth_a");
    let depth_b_id = ValueCellId::new("S", "depth_b");
    let guard_a_id = ValueCellId::new("S", "__guard_0");
    let guard_b_id = ValueCellId::new("S", "__guard_1");
    let m_a_id = ValueCellId::new("S", "m_a");
    let m_b_id = ValueCellId::new("S", "m_b");

    // Guard expressions: each reads its own auto param.
    let guard_a_expr = gt(value_ref("S", "depth_a"), literal(mm(3.0)));
    let guard_b_expr = gt(value_ref("S", "depth_b"), literal(mm(3.0)));

    // Member declarations: simple 1mm constants; deactivated when guard = false.
    let m_a_decl = ValueCellDecl {
        id: m_a_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(1.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };
    let m_b_decl = ValueCellDecl {
        id: m_b_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(1.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        // ref_a, ref_b: plain params — NOT structure_controlling (not in any guard).
        .param("S", "ref_a", Type::length(), Some(literal(mm(5.0))))
        .param("S", "ref_b", Type::length(), Some(literal(mm(5.0))))
        // depth_a, depth_b: auto params resolved by the constraint solver.
        .auto_param("S", "depth_a", Type::length())
        .auto_param("S", "depth_b", Type::length())
        // Constraints: solver must satisfy depth_a >= ref_a and depth_b >= ref_b.
        .constraint(
            "S",
            0,
            Some("depth_a_ge_ref_a"),
            ge(value_ref("S", "depth_a"), value_ref("S", "ref_a")),
        )
        .constraint(
            "S",
            1,
            Some("depth_b_ge_ref_b"),
            ge(value_ref("S", "depth_b"), value_ref("S", "ref_b")),
        )
        // Group A: guard = depth_a > 3mm, member = m_a.
        .guarded_group(
            guard_a_expr,
            guard_a_id.clone(),
            vec![m_a_decl],
            vec![],
            vec![],
            vec![],
        )
        // Group B: guard = depth_b > 3mm, member = m_b.
        .guarded_group(
            guard_b_expr,
            guard_b_id.clone(),
            vec![m_b_decl],
            vec![],
            vec![],
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Sequenced solver: first call → depth_a = depth_b = 5mm (initial eval);
    //                   second call → depth_a = depth_b = 2mm (after edit_param).
    let mut solved1 = HashMap::new();
    solved1.insert(depth_a_id.clone(), mm(5.0));
    solved1.insert(depth_b_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(depth_a_id.clone(), mm(2.0));
    solved2.insert(depth_b_id.clone(), mm(2.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval: depth_a = depth_b = 5mm, both guards true, m_a = m_b = 1mm.
    let initial = engine.eval(&module);
    assert!(
        matches!(initial.values.get(&m_a_id), Some(Value::Scalar { .. })),
        "initial: m_a should be active (1mm) when depth_a=5mm > 3mm; got {:?}",
        initial.values.get(&m_a_id),
    );
    assert!(
        matches!(initial.values.get(&m_b_id), Some(Value::Scalar { .. })),
        "initial: m_b should be active (1mm) when depth_b=5mm > 3mm; got {:?}",
        initial.values.get(&m_b_id),
    );

    // edit_param(ref_a, 2mm): ref_a is NOT structure_controlling, so Phase 1 does
    // not fire (phase1_reelaborated = {}).  The solver re-resolves both depth params
    // to 2mm; wave2 re-evaluates both guard cells to false (2mm > 3mm = false).
    // Phase 3 must iterate and deactivate BOTH groups.
    let edited = engine
        .edit_param(ref_a_id, Value::length(0.002)) // 2mm in SI
        .expect("edit_param must succeed");

    // (a) member A deactivated: guard_a flipped true→false via Phase 3.
    assert!(
        matches!(edited.values.get(&m_a_id), Some(Value::Undef)),
        "m_a must be Undef after guard_a flips false via Phase 3; got {:?}",
        edited.values.get(&m_a_id),
    );

    // (b) member B deactivated: the critical multi-group assertion.
    // If Phase 3's loop truncates after the first group, m_b remains 1mm here.
    assert!(
        matches!(edited.values.get(&m_b_id), Some(Value::Undef)),
        "m_b must be Undef after guard_b flips false via Phase 3; got {:?} \
         (== 1mm means Phase 3 loop was truncated after the first group — \
         this is the regression task 2145 guards against)",
        edited.values.get(&m_b_id),
    );

    // (c) Counter == 2: Phase 3 processed exactly both groups.
    let counter = engine.last_guard_phase_group_evals();
    assert_eq!(
        counter, 2,
        "expected counter == 2 (Phase 3 re-elaborates both guarded groups); \
         got {} — \
         == 0: over-aggressive dedup (both skipped), \
         == 1: loop truncation regression (task 2145), \
         > 2: extra or double elaboration",
        counter
    );
}

// ── Wave2 interaction: inactive members must stay Undef after cleanup ─────────

/// Regression guard for the post-wave2 cleanup (task 2140 amendment):
/// when Phase 1 deactivates a member and wave2 subsequently rewrites it
/// (because the member's default_expr reads a resolved auto param), the
/// post-wave2 cleanup step must re-deactivate it so Phase 3's
/// `phase1_reelaborated` skip leaves the engine in a correct state.
///
/// Test design:
/// - Structure with `param x: Length = 10mm` (guards `x > 5mm`),
///   `auto depth: Length` (resolved by solver to equal x), and
///   `where x > 5mm { let m = depth }` (member reads the auto param).
/// - Initial eval: x=10mm, guard=true, solver→depth=10mm, m=10mm.
/// - edit_param(x, 3mm):
///   (1) Phase 1 fires (guard cell in dirty cone); guard goes false →
///   m is deactivated to Undef; `phase1_reelaborated` = {guard_cell}.
///   (2) Solver re-runs (constraint is dirty); depth = 3mm.
///   (3) Wave2 re-evaluates m (m reads depth) → writes m = 3mm, overwriting Undef.
///   (4) Post-wave2 cleanup (fix): inactive branch (members when guard=false)
///   re-deactivated → m = Undef again.
///   (5) Phase 3 skips the group via `phase1_reelaborated` (correct: cleanup
///   already restored the deactivated state).
/// - Assert: m = Undef after the edit.
///
/// Without the post-wave2 cleanup, m would be 3mm (wave2 value) because
/// Phase 3's dedup would skip the group and never re-deactivate m.
#[test]
fn edit_param_wave2_does_not_corrupt_inactive_members() {
    let fixture = wave2_flip_fixture();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(fixture.solver);

    // Initial eval: x=10mm, guard=true, solver→depth=10mm, m=depth=10mm.
    let initial = engine.eval(&fixture.module_initial);
    assert!(
        matches!(initial.values.get(&fixture.m_id), Some(Value::Scalar { si_value, .. }) if (*si_value - 0.010).abs() < 1e-10),
        "initial eval: m should be 10mm (= depth) when guard is true, got {:?}",
        initial.values.get(&fixture.m_id),
    );

    // edit_param(x, 3mm):
    //   Phase 1: guard goes false → m deactivated to Undef.
    //   Solver: depth = 3mm.
    //   Wave2: m re-evaluated to 3mm (overwrites Undef) ← bug trigger.
    //   Post-wave2 cleanup (fix): m re-deactivated to Undef.
    //   Phase 3: skipped via phase1_reelaborated (cleanup already correct).
    let edited = engine
        .edit_param(fixture.x_id.clone(), Value::length(0.003)) // 3mm in SI
        .expect("edit_param must succeed");

    // m must be Undef: guard is false (x=3mm ≤ 5mm), so the active branch
    // (members) is inactive.  Without the post-wave2 cleanup, m would be 3mm
    // (wave2's re-evaluation result) because Phase 3 is skipped by the dedup.
    assert!(
        matches!(edited.values.get(&fixture.m_id), Some(Value::Undef)),
        "m must be Undef after guard flips false; \
         if m is a concrete value (e.g. 3mm), wave2 corrupted the inactive member \
         and the post-wave2 cleanup is missing or broken. Got {:?}",
        edited.values.get(&fixture.m_id),
    );
}

/// Symmetric regression test for the wave2 guard-flip bug in `edit_param`.
/// Task 2146.
///
/// ## Bug
///
/// `phase1_reelaborated` (formerly `HashSet<ValueCellId>`) did not record the
/// guard value Phase 1 observed. When wave2 subsequently flipped the guard,
/// Phase 3's dedup skipped the group entirely (both the `phase1_reelaborated`
/// check and the old-vs-new check blocked re-elaboration), leaving else_members
/// that should be Determined stuck at `Undef`.
///
/// ## Fixture
///
/// Single module S:
/// - `param x: Length = -1mm` (default negative → guard starts false)
/// - `auto depth: Length` (resolved by solver to depth >= x)
/// - Composite guard: `(x > 0mm) && (depth > 5mm)`
///   Reads BOTH x (triggers Phase 1 via dirty_cone) and depth (triggers wave2
///   flip when solver updates depth after the edit).
/// - `members: [let m = 99mm]` — literal, does NOT read depth
/// - `else_members: [let n = 42mm]` — literal, does NOT read depth
///
/// ## Sequence for edit_param(x, 1mm)
///
/// 1. Initial `eval`: x=-1mm, guard=(-1>0)&&...=false. m=Undef, n=42mm.
///    Solver (call 1): depth=8mm.
/// 2. `edit_param(x, 1mm)`:
///    - **Phase 1** fires (guard_cell in dirty_cone(x)).
///      Evaluates with x=1mm, depth=8mm (stale): (1>0)&&(8>5) = true.
///      old_guard=false ≠ Phase-1 guard=true → re-elaborates.
///      Records `{guard_cell: Bool(true)}`. m=99mm, n=Undef.
///    - **Solver** (call 2): depth=3mm (constraint depth≥x=1mm).
///    - **Wave2**: guard_cell reads depth → re-eval: (1>0)&&(3>5) = false.
///      Guard flips true→false.
///    - **reapply_phase1_deactivations**: m deactivated (Undef). n skipped
///      (else_members; is_active=true under guard=false).
///    - **Phase 3** (OLD, buggy): `phase1_reelaborated.contains(guard_cell)` →
///      true → `continue` → n stays Undef. ALSO old_guard=false==current false
///      → old-vs-new skip would fire too.
///    - **Phase 3** (FIXED): guard_changed trigger detects Phase-1 flip-then-
///      revert (recorded Bool(true) ≠ current Bool(false)) → guard_changed=true.
///      Per-group match: recorded Bool(true) ≠ current Bool(false) → case (b) →
///      falls through unconditionally → n's literal default_expr evaluated →
///      n=42mm (Determined).
/// 3. Assert: m=Undef (members inactive), n=42mm (else_members active).
///    Cross-check against cold eval of a module with x default=-1mm (same as
///    the pre-edit module but with solver returning depth=3mm and guard=false):
///    n should match.
///
/// This test passes immediately after the task-2146 fix (step-2) and guards
/// against a future refactor that regresses only the edit_param path while
/// leaving the edit_source path intact.
#[test]
fn edit_param_wave2_guard_flip_activates_else_members() {
    let x_id = ValueCellId::new("S", "x");
    let depth_id = ValueCellId::new("S", "depth");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let m_id = ValueCellId::new("S", "m");
    let n_id = ValueCellId::new("S", "n");

    // Composite guard: (x > 0mm) && (depth > 5mm).
    // Reads x (triggers Phase 1 via dirty_cone) AND depth (triggers wave2 flip).
    let guard_expr = and(
        gt(value_ref("S", "x"), literal(mm(0.0))),
        gt(value_ref("S", "depth"), literal(mm(5.0))),
    );

    // Member m: literal 99mm — does NOT read depth (wave2 won't touch it).
    let m_decl = ValueCellDecl {
        id: m_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(99.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Else-member n: literal 42mm — does NOT read depth (wave2 won't touch it).
    // This is the cell that must become Determined after the guard flips.
    let n_decl = ValueCellDecl {
        id: n_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(42.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        // x: param with default -1mm so the guard starts false (x > 0 = false)
        .param("S", "x", Type::length(), Some(literal(mm(-1.0))))
        // depth: auto param resolved by solver (constraint: depth >= x)
        .auto_param("S", "depth", Type::length())
        // constraint depth >= x; reads both depth and x → dirty when x changes
        .constraint(
            "S",
            0,
            Some("depth_ge_x"),
            ge(value_ref("S", "depth"), value_ref("S", "x")),
        )
        // guarded group: composite guard reads x and depth;
        // literal member m (active when guard=true);
        // literal else_member n (active when guard=false)
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![m_decl],
            vec![],
            vec![n_decl],
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Sequenced solver:
    // call 1 (initial eval): depth=8mm (>5mm, but guard still false because x=-1<0)
    // call 2 (edit_param):   depth=3mm (≤5mm; wave2 flips guard true→false)
    let mut solved1 = HashMap::new();
    solved1.insert(depth_id.clone(), mm(8.0));
    let mut solved2 = HashMap::new();
    solved2.insert(depth_id.clone(), mm(3.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval: x=-1mm, guard=false, solver→depth=8mm.
    // m=Undef (members inactive), n=42mm (else_members active).
    let initial = engine.eval(&module);
    assert!(
        matches!(initial.values.get(&m_id), Some(Value::Undef)),
        "initial eval: m should be Undef (guard=false; x=-1mm < 0mm). Got {:?}",
        initial.values.get(&m_id),
    );
    assert!(
        matches!(initial.values.get(&n_id), Some(Value::Scalar { si_value, .. })
            if (*si_value - 0.042).abs() < 1e-10),
        "initial eval: n should be 42mm (else_members active; guard=false). Got {:?}",
        initial.values.get(&n_id),
    );

    // edit_param(x, 1mm):
    // Phase 1: guard_cell in dirty_cone(x); eval with x=1mm, depth=8mm(stale)
    //   → (1>0)&&(8>5)=true. old=false≠new=true → Phase 1 fires.
    //   phase1_reelaborated = {guard_cell: Bool(true)}. m=99mm, n=Undef.
    // Solver: constraint depth>=x reads x (dirty) → depth=3mm.
    // Wave2: guard_cell reads depth → re-eval: (1>0)&&(3>5)=false. Guard flips!
    // reapply: m deactivated (Undef). n skipped (else_members; active branch).
    // Phase 3 (OLD, buggy): guard_cell in phase1_reelaborated → skip. Also
    //   old_guard==current_guard==false → old-vs-new skip fires too. n=Undef. BUG.
    // Phase 3 (FIXED): guard_changed detects Phase-1 flip-then-revert (task 2146).
    //   Per-group case (b): recorded Bool(true) ≠ current Bool(false) → re-elaborate.
    //   n's literal default_expr evaluated → n=42mm. FIX.
    let edited = engine
        .edit_param(x_id.clone(), Value::length(0.001)) // 1mm in SI
        .expect("edit_param must succeed");

    // (a) m must be Undef: guard ended up false → members branch inactive.
    assert!(
        matches!(edited.values.get(&m_id), Some(Value::Undef)),
        "m must be Undef after guard ends up false (x=1mm; guard=(1>0)&&(3>5)=false). \
         Got {:?}",
        edited.values.get(&m_id),
    );

    // (b) n must be 42mm (Determined): guard is false → else_members branch active.
    // BUG (pre-fix): n remains Undef because Phase 3's guard_changed is false
    // (old=false==current=false) and phase1_reelaborated check blocks the fallback.
    assert!(
        matches!(edited.values.get(&n_id), Some(Value::Scalar { si_value, .. })
            if (*si_value - 0.042).abs() < 1e-10),
        "n must be 42mm (else_members active; guard ended up false after wave2 flip); \
         if n is Undef, Phase 3 incorrectly skipped the group via stale \
         phase1_reelaborated (task 2146 bug). Got {:?}",
        edited.values.get(&n_id),
    );

    // (c) Cross-check: incremental result must match cold eval with depth=3mm, guard=false.
    // Build a fresh engine with a solver that returns depth=3mm for the cold eval.
    let mut cold_solved = HashMap::new();
    cold_solved.insert(depth_id.clone(), mm(3.0));
    let cold_solver = SequencedMockConstraintSolver::new(vec![SolveResult::Solved {
        values: cold_solved,
        unique: true,
    }]);
    let cold_checker = MockConstraintChecker::new();
    let mut cold_engine =
        Engine::new(Box::new(cold_checker), None).with_solver(Box::new(cold_solver));
    // Cold eval of the same module with x=-1mm default (guard=false → n=42mm, m=Undef).
    // Note: x default is -1mm, which gives guard=false regardless of depth.
    let cold = cold_engine.eval(&module);

    assert_eq!(
        edited.values.get(&m_id),
        cold.values.get(&m_id),
        "m: incremental edit_param result must match cold eval",
    );
    assert_eq!(
        edited.values.get(&n_id),
        cold.values.get(&n_id),
        "n: incremental edit_param result must match cold eval",
    );
}

/// Post-wave2 cleanup must cover guarded groups that Phase 1 *skipped* via the
/// per-group unchanged-guard short-circuit (task 2144).
///
/// ## Bug scenario
///
/// Two guarded groups share the same `S` template:
///
/// - **Group A**: guard `x > 5mm`; member `a = 1mm` (does not read auto).
/// - **Group B**: guard `y > 5mm`; member `m = depth` (reads the auto param).
///
/// Auto param `depth: Length`, constraint `depth >= x` (dirty when x changes).
///
/// Initial state — x=10mm (guard_a=true), y=3mm (guard_b=false): `a=1mm`, `m=Undef`,
/// `depth=10mm`.
///
/// `edit_param(x, 3mm)` trace (current **buggy** impl):
///
/// 1. Phase 1 fires (`x`'s guard_a cell is in dirty_cone because guard_a reads `x`).
///    * Group A: guard flips true→false → `phase1_reelaborated = {guard_a}`; `a` deactivated.
///    * Group B: guard unchanged false → **per-group short-circuit** (NOT in
///      `phase1_reelaborated`).
/// 2. Solver re-runs (`depth >= x` is dirty); resolves `depth = 3mm`.
/// 3. Wave2 re-evaluates `m` (reverse-dep of `depth`) → writes `m = 3mm` (BUG).
/// 4. Post-wave2 cleanup (buggy): iterates only Group A (`phase1_reelaborated`-gated) →
///    Group B's `m` stays at 3mm.
/// 5. Phase 3: Group A skipped via `phase1_reelaborated`; Group B skipped via old==new
///    guard check.
///
/// Result: `m = 3mm` when it must be `Undef` (guard_b is false).
///
/// ## Fix
///
/// Broaden the post-wave2 cleanup to iterate **all** guarded groups (not just those in
/// `phase1_reelaborated`). The cleanup is idempotent for groups wave2 did not touch.
/// See task 2144 and `reapply_guard_deactivations_post_wave2` in `engine_edit.rs`.
///
/// ## Assertions
///
/// * `m == Undef` — primary assertion, **fails** on current code.
/// * `a == Undef` — Group A correctly deactivated (regression lock).
/// * `counter == 1` — Phase 1 processes Group A only; Phase 3 skips both groups.
#[test]
fn edit_param_wave2_does_not_corrupt_unchanged_guard_group() {
    let x_id = ValueCellId::new("S", "x");
    let _y_id = ValueCellId::new("S", "y");
    let depth_id = ValueCellId::new("S", "depth");
    let guard_a_id = ValueCellId::new("S", "__guard_0");
    let guard_b_id = ValueCellId::new("S", "__guard_1");
    let a_id = ValueCellId::new("S", "a");
    let m_id = ValueCellId::new("S", "m");

    // Group A: guard x > 5mm; member a = 1mm (constant — does NOT read auto param).
    let guard_a_expr = gt(value_ref("S", "x"), literal(mm(5.0)));
    let a_decl = ValueCellDecl {
        id: a_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(1.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Group B: guard y > 5mm; member m = depth (reads the auto param — wave2 target).
    let guard_b_expr = gt(value_ref("S", "y"), literal(mm(5.0)));
    let m_decl = ValueCellDecl {
        id: m_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(value_ref("S", "depth")), // reads auto param depth
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        // x: default 10mm → guard_a=true initially; becomes 3mm after edit → guard_a=false.
        .param("S", "x", Type::length(), Some(literal(mm(10.0))))
        // y: default 3mm → guard_b=false (stays false across the edit).
        .param("S", "y", Type::length(), Some(literal(mm(3.0))))
        // depth: auto param resolved by solver.
        .auto_param("S", "depth", Type::length())
        // constraint reads depth and x → dirty when x changes → solver re-runs.
        .constraint(
            "S",
            0,
            Some("depth_ge_x"),
            ge(value_ref("S", "depth"), value_ref("S", "x")),
        )
        // Group A: guard reads x → guard_a in dirty_cone(x) → Phase 1 fires.
        .guarded_group(
            guard_a_expr,
            guard_a_id.clone(),
            vec![a_decl],
            vec![],
            vec![],
            vec![],
        )
        // Group B: guard reads y → guard_b NOT in dirty_cone(x) when x is edited.
        // Phase 1 re-evaluates guard_b anyway (iterates all groups), sees unchanged
        // value (y=3mm, guard_b stays false), and takes the per-group short-circuit
        // (does NOT add guard_b to phase1_reelaborated).
        .guarded_group(
            guard_b_expr,
            guard_b_id.clone(),
            vec![m_decl],
            vec![],
            vec![],
            vec![],
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Sequenced solver: first call → depth=10mm (initial eval, x=10mm);
    //                   second call → depth=3mm (after edit_param(x, 3mm)).
    let mut solved1 = HashMap::new();
    solved1.insert(depth_id.clone(), mm(10.0));
    let mut solved2 = HashMap::new();
    solved2.insert(depth_id.clone(), mm(3.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval: x=10mm (guard_a=true), y=3mm (guard_b=false).
    // Expected: a=1mm (active branch), m=Undef (inactive branch), depth=10mm.
    let initial = engine.eval(&module);
    assert!(
        matches!(initial.values.get(&a_id), Some(Value::Scalar { si_value, .. }) if (*si_value - 0.001).abs() < 1e-12),
        "initial eval: a should be 1mm when guard_a=true, got {:?}",
        initial.values.get(&a_id),
    );
    assert!(
        matches!(initial.values.get(&m_id), Some(Value::Undef)),
        "initial eval: m should be Undef when guard_b=false (y=3mm ≤ 5mm), got {:?}",
        initial.values.get(&m_id),
    );

    // edit_param(x, 3mm): x changes 10mm→3mm.
    // Phase 1: guard_a flips true→false (x=3mm ≤ 5mm); a deactivated.
    //          guard_b unchanged false (y=3mm ≤ 5mm); per-group skip — NOT in phase1_reelaborated.
    // Solver: constraint dirty (depth>=x reads x) → depth=3mm.
    // Wave2: m reads depth → m=3mm written (BUG under current code).
    // Post-wave2 cleanup (fix): must deactivate m even though guard_b not in phase1_reelaborated.
    // Phase 3: Group A skipped (phase1_reelaborated); Group B skipped (guard unchanged).
    let edited = engine
        .edit_param(x_id, Value::length(0.003)) // 3mm in SI
        .expect("edit_param must succeed");

    // PRIMARY assertion: m must be Undef (guard_b=false, inactive branch).
    // Under the buggy code, m = 3mm (wave2 value persists because the cleanup
    // only iterates phase1_reelaborated groups, which does not include Group B).
    assert!(
        matches!(edited.values.get(&m_id), Some(Value::Undef)),
        "m must be Undef after edit (guard_b=false; y=3mm ≤ 5mm). \
         If m is concrete (e.g. 3mm), the post-wave2 cleanup missed Group B \
         (Phase-1-skipped-but-dirty scenario, task 2144). Got: {:?}",
        edited.values.get(&m_id),
    );

    // Regression lock: Group A's member a must also be Undef (guard_a flipped false).
    assert!(
        matches!(edited.values.get(&a_id), Some(Value::Undef)),
        "a must be Undef after edit (guard_a flipped false; x=3mm ≤ 5mm). Got: {:?}",
        edited.values.get(&a_id),
    );

    // Performance lock: Phase 1 re-elaborates only the group whose guard value flipped (Group A:
    // true→false). Phase 3 skips every group — Group A via the `phase1_reelaborated` cross-phase
    // dedup, Group B via the unchanged-guard short-circuit. The exact counter target is asserted below.
    let counter = engine.last_guard_phase_group_evals();
    assert_eq!(
        counter, 1,
        "expected exactly 1 non-skipped guard-phase group evaluation \
         (Phase 1 processes Group A; Phase 3 skips both groups). Got: {}",
        counter,
    );
}

/// Invariant: two paths to the same final guard configuration produce identical
/// Auto-cell snapshot state (task 2143).
///
/// Path A (direct eval, inactive Auto at start):
///   Module with `param active: Bool = true`, `auto_param thickness: length`,
///   constraint `thickness > 2mm`, guarded_group where thickness is in
///   `else_members` (active only when guard=false).  Running `eval(&module_a)`
///   with guard=true means thickness is on the INACTIVE else_members branch.
///   The solver resolves thickness=5mm before the post-solver guard re-evaluation
///   pass; the pre-fix code then overwrites inactive Auto cells to `(Undef, Auto)`,
///   destroying solver work.
///
/// Path B (eval then edit_param, flip inactive):
///   Module with `param active: Bool = false` (else_members ACTIVE).  Running
///   `eval(&module_b)` with guard=false makes thickness active; solver resolves
///   5mm.  Then `edit_param(active_id, Value::Bool(true))` flips the guard to
///   true; `deactivate_if_not_auto` in edit_param's Phase 1 / post-wave2 cleanup
///   skips the Auto cell, preserving `(Scalar(5mm), Determined)`.
///
/// The two paths should produce identical final snapshot state for `thickness`.
/// Under the pre-fix code this test FAILS because Path A produces `(Undef, Auto)`
/// while Path B produces `(Scalar(5mm), Determined)`.  After the fix (wrapping
/// the inactive-branch `Value::Undef` writes in `if !cell.kind.is_auto()`) both
/// paths produce `(Scalar(5mm), Determined)`.
///
/// The canonical rule — Auto cell lifecycle is owned by the constraint solver,
/// not by guard activation/deactivation — is documented on the module-level `//!`
/// doc of `engine_edit.rs` and on `deactivate_if_not_auto`.
///
/// Strengthened in task 2157: the assertions also pin the expected solver
/// invocation pattern in each path.  Without these counts, two paths that BOTH
/// skip the solver (or BOTH spuriously re-invoke it) could still produce
/// identical 5 mm results from the mock, masking a regression where the
/// engine's solver-invocation contract changes.
#[test]
fn eval_and_edit_param_paths_produce_same_inactive_auto_state() {
    let active_id = ValueCellId::new("S", "active");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let thickness_id = ValueCellId::new("S", "thickness");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Auto param 'thickness' as an else_member (kind=Auto, no default_expr)
    let thickness_decl = || ValueCellDecl {
        id: thickness_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Module A: active=true → guard=true → else_members (thickness) INACTIVE at eval()
    let template_a = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr.clone(),
            guard_id.clone(),
            vec![],                 // members (nothing active when guard=true)
            vec![],                 // constraints
            vec![thickness_decl()], // else_members (active only when guard=false)
            vec![],                 // else_constraints
        )
        .build();
    let module_a = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .build();

    // Module B: active=false → guard=false → else_members (thickness) ACTIVE at eval()
    let template_b = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            Some("thickness_gt_2mm"),
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr.clone(),
            guard_id.clone(),
            vec![],                 // members
            vec![],                 // constraints
            vec![thickness_decl()], // else_members (active when guard=false)
            vec![],                 // else_constraints
        )
        .build();
    let module_b = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_b)
        .build();

    // Both solvers resolve thickness=5mm
    let mut solved = HashMap::new();
    solved.insert(thickness_id.clone(), mm(5.0));
    let solver_a = MockConstraintSolver::new_solved(solved.clone());
    let solver_b = MockConstraintSolver::new_solved(solved);

    // Capture counter handles before the solvers are moved into Box<dyn ConstraintSolver>.
    let counter_a = solver_a.counter_handle();
    let counter_b = solver_b.counter_handle();

    // ── Path A: direct eval with guard=true (else_members inactive at start) ──
    let checker_a = MockConstraintChecker::new();
    let mut engine_a = Engine::new(Box::new(checker_a), None).with_solver(Box::new(solver_a));
    engine_a.eval(&module_a);
    let snap_a = engine_a
        .snapshot()
        .expect("Engine A must have a snapshot after eval");

    // ── Path B: eval with guard=false (else_members active), then flip to true ──
    let checker_b = MockConstraintChecker::new();
    let mut engine_b = Engine::new(Box::new(checker_b), None).with_solver(Box::new(solver_b));
    engine_b.eval(&module_b);
    engine_b
        .edit_param(active_id.clone(), Value::Bool(true))
        .expect("edit_param should succeed");
    let snap_b = engine_b
        .snapshot()
        .expect("Engine B must have a snapshot after edit_param");

    // Both paths must produce identical thickness snapshot state.
    // The canonical rule: Auto cell lifecycle is owned by the solver —
    // inactive-branch Auto cells retain their solver-resolved value.
    let state_a = snap_a.values.get(&thickness_id);
    let state_b = snap_b.values.get(&thickness_id);
    assert_eq!(
        state_a, state_b,
        "eval() and eval()+edit_param() paths must produce identical snapshot state \
         for inactive-branch Auto param 'thickness'.\n\
         Path A (direct eval, guard=true → else_members inactive): {:?}\n\
         Path B (eval guard=false + edit_param→true, thickness moves to inactive): {:?}",
        state_a, state_b
    );

    // Also pin the concrete expected value — guards against future regressions
    // where both paths coincidentally produce the same wrong state (e.g. None or
    // (Undef, Undetermined)).  The solver resolved thickness = 5 mm = 0.005 SI.
    assert!(
        matches!(
            state_a,
            Some((Value::Scalar { si_value, .. }, DeterminacyState::Determined))
                if (*si_value - 0.005).abs() < 1e-10
        ),
        "expected inactive-branch Auto cell to be (Scalar(5mm ≈ 0.005 SI), Determined), \
         got {:?}",
        state_a
    );

    // ── Pin solver invocation counts for both paths ──
    //
    // Path A: exactly 1 cold-eval solve.
    //   * Count = 0 → engine skipped solving entirely (pre-fix regression: inactive
    //     Auto cells were overwritten with (Undef, Auto) before the solver ran, so the
    //     engine might short-circuit).
    //   * Count > 1 → redundant re-invocations during eval() (unexpected).
    let count_a = counter_a.load(Ordering::Relaxed);
    assert_eq!(
        count_a, 1,
        "Path A: expected exactly 1 solver invocation during eval(), got {}; \
         0 means the engine skipped solving (pre-fix Auto-cell overwrite regression), \
         >1 means redundant re-invocations during eval()",
        count_a
    );

    // Path B: exactly 1 solver invocation (cold eval only).
    //   The constraint `thickness > 2mm` references only `thickness`, not `active`.
    //   Therefore edit_param(active, true) does NOT dirty that constraint, and the
    //   `constraints_dirty` guard at engine_edit.rs:776 short-circuits the second solve.
    //   * Count = 0 → cold solve was skipped → stale Undef leaks through (regression).
    //   * Count = 2 → edit_param spuriously re-invoked the solver for a non-dirty
    //     constraint (unnecessary work, and a sign the dirty-cone logic regressed).
    let count_b = counter_b.load(Ordering::Relaxed);
    assert_eq!(
        count_b, 1,
        "Path B: expected exactly 1 solver invocation across eval()+edit_param(), got {}; \
         0 means the cold solve was skipped (stale Undef → false-positive value match), \
         2 means edit_param spuriously re-invoked the solver for a non-dirty constraint",
        count_b
    );
}

/// Regression lock: `post_solver_re_eval_guard_cells` active-branch dispatch.
///
/// Exercises all three active-branch arms in a single `eval()` call:
/// 1. **Auto arm (skip):** `resolved` is solver-resolved to 5mm; the post-solver
///    re-eval must preserve that value (not overwrite with Undef).
/// 2. **Param arm:** `param_inner` has `default_expr = literal(7mm)`; the
///    post-solver re-eval must re-evaluate it to 7mm.
/// 3. **Let arm:** `let_inner` has `default_expr = value_ref("S","resolved")`; the
///    post-solver re-eval must evaluate it AFTER the solver (so it reads 5mm,
///    not the pre-solver Undef).
///
/// This test passes on the current `if`-based implementation and must continue
/// to pass after the exhaustive `match`-based refactor in task 2156. A
/// mis-refactor (e.g. collapsing Param|Let into the skip arm) would produce
/// concrete value mismatches here rather than only surfacing as diffuse
/// integration failures.
#[test]
fn post_solver_active_branch_dispatches_param_let_and_skips_auto() {
    // `S.active` (Bool param, default=true) drives the guard; its id is used
    // implicitly via the `value_ref_typed` guard_expr below.
    let guard_id = ValueCellId::new("S", "__guard_0");
    let resolved_id = ValueCellId::new("S", "resolved");
    let param_inner_id = ValueCellId::new("S", "param_inner");
    let let_inner_id = ValueCellId::new("S", "let_inner");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);

    // Member 1: Auto cell — this deliberately reuses the top-level auto_param id
    // (`resolved`) so the solver writes a concrete value (5mm) that the Auto-skip
    // arm of `post_solver_re_eval_guard_cells` must then preserve unchanged.
    // Having two decls for the same id (top-level + guarded-group member) is a
    // valid module shape: the group's Auto decl just shadows the top-level one
    // for the group scope, and the solver resolves both to the same value.
    let auto_inner_decl = ValueCellDecl {
        id: resolved_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Member 2: Param cell — the post-solver re-eval must evaluate
    // default_expr (literal 7mm) to 7mm (Param arm).
    let param_inner_decl = ValueCellDecl {
        id: param_inner_id.clone(),
        kind: ValueCellKind::Param,
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(7.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Member 3: Let cell — default_expr reads `resolved`.  The post-solver
    // re-eval must evaluate this AFTER the solver has written 5mm into
    // `resolved`, so let_inner = 5mm (Let arm).
    let let_inner_decl = ValueCellDecl {
        id: let_inner_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(value_ref("S", "resolved")),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    let template = TopologyTemplateBuilder::new("S")
        // Bool param that drives the guard (default=true → members are active).
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        // Top-level auto_param so the resolution phase finds `resolved`.
        .auto_param("S", "resolved", Type::length())
        // Constraint so the solver has something to work with.
        .constraint(
            "S",
            0,
            Some("resolved_gt_2mm"),
            gt(value_ref("S", "resolved"), literal(mm(2.0))),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![auto_inner_decl, param_inner_decl, let_inner_decl], // members (active when guard=true)
            vec![],                                                  // constraints
            vec![],                                                  // else_members
            vec![],                                                  // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Mock solver: resolves `resolved` to 5mm = 0.005 SI.
    let mut solved_values = HashMap::new();
    solved_values.insert(resolved_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved_values);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // ── Assert on result.values ──────────────────────────────────────────────

    // Auto-skip arm: solver-resolved value must be preserved (not overwritten).
    let resolved_val = result.values.get(&resolved_id);
    assert!(
        matches!(resolved_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "resolved (Auto) should be 5mm (0.005 SI) — Auto-skip arm must preserve solver work, got {:?}",
        resolved_val
    );

    // Param arm: default_expr (7mm) must be re-evaluated by the post-solver pass.
    let param_val = result.values.get(&param_inner_id);
    assert!(
        matches!(param_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.007).abs() < 1e-10),
        "param_inner (Param) should be 7mm (0.007 SI) — Param arm must re-evaluate default_expr, got {:?}",
        param_val
    );

    // Let arm: default_expr = value_ref("S","resolved") must be evaluated AFTER
    // the solver, so let_inner reads the solver-resolved 5mm, not pre-solver Undef.
    let let_val = result.values.get(&let_inner_id);
    assert!(
        matches!(let_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.005).abs() < 1e-10),
        "let_inner (Let) should be 5mm (0.005 SI) — Let arm must re-evaluate default_expr after solver resolved Auto, got {:?}",
        let_val
    );

    // ── Assert on engine.snapshot().values ──────────────────────────────────

    let snapshot = engine.snapshot().expect("snapshot must exist after eval");

    let (snap_resolved, snap_resolved_det) = snapshot
        .values
        .get(&resolved_id)
        .expect("resolved must be in snapshot");
    assert!(
        matches!(snap_resolved, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "resolved in snapshot should be 5mm, got {:?}",
        snap_resolved
    );
    assert_eq!(
        *snap_resolved_det,
        DeterminacyState::Determined,
        "resolved must be Determined in snapshot (Auto-skip arm preserved solver state)"
    );

    let (snap_param, snap_param_det) = snapshot
        .values
        .get(&param_inner_id)
        .expect("param_inner must be in snapshot");
    assert!(
        matches!(snap_param, Value::Scalar { si_value, .. } if (*si_value - 0.007).abs() < 1e-10),
        "param_inner in snapshot should be 7mm, got {:?}",
        snap_param
    );
    assert_eq!(
        *snap_param_det,
        DeterminacyState::Determined,
        "param_inner must be Determined in snapshot"
    );

    let (snap_let, snap_let_det) = snapshot
        .values
        .get(&let_inner_id)
        .expect("let_inner must be in snapshot");
    assert!(
        matches!(snap_let, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "let_inner in snapshot should be 5mm (reads solver-resolved Auto via value_ref), got {:?}",
        snap_let
    );
    assert_eq!(
        *snap_let_det,
        DeterminacyState::Determined,
        "let_inner must be Determined in snapshot"
    );
}
