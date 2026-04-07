//! Guard evaluation tests.
//!
//! Tests for evaluating guarded groups: conditional member activation,
//! else branches, undef guards, and schema re-elaboration.

use std::collections::HashMap;

use reify_eval::Engine;
use reify_test_support::builders::{gt, literal, value_ref, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintSolver, SequencedMockConstraintSolver,
    TopologyTemplateBuilder, mm,
};
use reify_types::*;

use reify_compiler::{CompiledConstraint, ValueCellDecl, ValueCellKind, Visibility};

/// Helper to create a ValueCellDecl for tests.
fn make_param_decl(entity: &str, member: &str, cell_type: Type, default: Value) -> ValueCellDecl {
    ValueCellDecl {
        id: ValueCellId::new(entity, member),
        kind: ValueCellKind::Param,
        visibility: Visibility::Public,
        cell_type: cell_type.clone(),
        default_expr: Some(CompiledExpr::literal(default, cell_type)),
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

    // BUG: Block B in edit_param overwrites solver-resolved Auto param with Undef.
    // After fix, thickness should retain its solver-resolved value.
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
        span: SourceSpan::new(0, 0),
    };

    // Auto param 'depth' in else_members
    let depth_decl = ValueCellDecl {
        id: depth_id.clone(),
        kind: ValueCellKind::Auto { free: false },
        visibility: Visibility::Public,
        cell_type: Type::length(),
        default_expr: None,
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
