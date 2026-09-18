//! Compiler tests for constraint instantiation.
//!
//! Tests for `constraint ConstraintName(arg: expr, ...)` inside structure bodies.
//! Validates that the compiler resolves the constraint def, binds args to params,
//! substitutes param references in predicate expressions, and injects resulting
//! constraints into the parent entity's constraint list.

use reify_core::*;
use reify_ir::*;
use reify_test_support::{compile_source, compile_template};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect only error diagnostics from a list.
fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Assert that `expr` is a `ValueRef` with the given member name.
///
/// Panics with a descriptive message if the expression is not a `ValueRef` or
/// if the member name does not match `expected_member`.
fn assert_value_ref(expr: &CompiledExpr, expected_member: &str) {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => assert_eq!(
            id.member, expected_member,
            "expected ValueRef with member '{}', got '{}'",
            expected_member, id.member
        ),
        other => panic!("expected ValueRef({}) but got {:?}", expected_member, other),
    }
}

/// Extract the `ValueCellId` from a `ValueRef` expression.
///
/// Returns a reference to the `ValueCellId` inside the expression, panicking
/// with a descriptive message (including `label`) if the expression is not a
/// `ValueRef`.  Used for cross-branch consistency assertions where all
/// references to the same parameter must resolve to the same `ValueCellId`.
fn extract_value_ref_id<'a>(expr: &'a CompiledExpr, label: &str) -> &'a ValueCellId {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => id,
        other => panic!(
            "extract_value_ref_id({}): expected ValueRef but got {:?}",
            label, other
        ),
    }
}

// ── Step 7: basic single-arg instantiation ───────────────────────────────────

/// Constraint def with single param, structure with one instantiation.
/// After substitution, constraint expr should be `thickness > 2mm`.
///
/// NOTE (task 4490): `wall > 2` was updated to `wall > 2mm` because comparing
/// a dimensional Length parameter to a bare integer (> 2) is now correctly
/// rejected by the compile-time operand guard.  The literal `2mm` compiles to
/// Scalar<Length>(0.002 SI m).
#[test]
fn basic_constraint_inst_compiles() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2mm
}
structure S {
    param thickness: Length
    constraint MinWall(wall: thickness)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    let cc = &tmpl.constraints[0];
    // The compiled expr should be BinOp(Gt, ValueRef(S.thickness), Literal(Scalar<Length>(0.002)))
    match &cc.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Gt, "expected Gt operator");
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(id.entity, "S", "left.entity should be S");
                    assert_eq!(id.member, "thickness", "left.member should be thickness");
                }
                other => panic!("expected ValueRef for left, got {:?}", other),
            }
            // Literal `2mm` compiles to Scalar<Length>(0.002 SI m).
            assert!(
                matches!(
                    &right.kind,
                    CompiledExprKind::Literal(Value::Scalar { si_value, .. })
                    if (si_value - 0.002_f64).abs() < 1e-9
                ),
                "right should be Literal(Scalar 2mm = 0.002 SI m), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp for constraint expr, got {:?}", other),
    }
}

// ── Step 9: multi-predicate constraint def ───────────────────────────────────

/// Constraint def with 3 params and 2 predicates; structure instantiates with literals.
///
/// NOTE (task 4490): `lo: 1, hi: 10` were updated to `lo: 1mm, hi: 10mm` because
/// passing a bare integer as a Length argument results in `w >= 1` (Scalar<Length>
/// >= Int) which is now correctly rejected by the compile-time dimension guard.
#[test]
fn multi_predicate_constraint_inst() {
    let source = r#"
constraint def Bounded {
    param x: Length
    param lo: Length
    param hi: Length
    x >= lo
    x <= hi
}
structure S {
    param w: Length
    constraint Bounded(x: w, lo: 1mm, hi: 10mm)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected exactly 2 constraints");

    // First constraint: w >= 1mm (Scalar<Length>(0.001 SI m))
    match &tmpl.constraints[0].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Ge, "first constraint should be Ge (>=)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "w"),
                "left should be ValueRef(S.w)"
            );
            assert!(
                matches!(
                    &right.kind,
                    CompiledExprKind::Literal(Value::Scalar { si_value, .. })
                    if (si_value - 0.001_f64).abs() < 1e-9
                ),
                "right should be Literal(Scalar 1mm = 0.001 SI m), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp for first constraint, got {:?}", other),
    }

    // Second constraint: w <= 10mm (Scalar<Length>(0.010 SI m))
    match &tmpl.constraints[1].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Le, "second constraint should be Le (<=)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "w"),
                "left should be ValueRef(S.w)"
            );
            assert!(
                matches!(
                    &right.kind,
                    CompiledExprKind::Literal(Value::Scalar { si_value, .. })
                    if (si_value - 0.010_f64).abs() < 1e-9
                ),
                "right should be Literal(Scalar 10mm = 0.010 SI m), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp for second constraint, got {:?}", other),
    }
}

// ── Step 11: complex expression substitution ─────────────────────────────────

/// Constraint def with complex predicates; validates substitution through BinOp nesting.
#[test]
fn complex_expr_substitution() {
    let source = r#"
constraint def Ratio {
    param a: Length
    param b: Length
    a / b > 0.5
    a / b < 2
}
structure S {
    param width: Length
    param height: Length
    constraint Ratio(a: width, b: height)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected 2 constraints");

    // Both constraints should have BinOp with nested BinOp(Div) left-hand side
    for cc in &tmpl.constraints {
        match &cc.expr.kind {
            CompiledExprKind::BinOp { left, right: _, .. } => match &left.kind {
                CompiledExprKind::BinOp {
                    op,
                    left: inner_left,
                    right: inner_right,
                } => {
                    assert_eq!(*op, BinOp::Div, "inner op should be Div");
                    assert!(
                        matches!(&inner_left.kind, CompiledExprKind::ValueRef(id) if id.member == "width"),
                        "inner left should be ValueRef(width)"
                    );
                    assert!(
                        matches!(&inner_right.kind, CompiledExprKind::ValueRef(id) if id.member == "height"),
                        "inner right should be ValueRef(height)"
                    );
                }
                other => panic!(
                    "expected BinOp(Div) as left side of constraint, got {:?}",
                    other
                ),
            },
            other => panic!("expected outer BinOp, got {:?}", other),
        }
    }
}

// ── Step 13: unknown constraint def name ─────────────────────────────────────

#[test]
fn unknown_constraint_def_name() {
    let source = r#"
structure S {
    param t: Length
    constraint UnknownDef(x: t)
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown constraint def"
    );
    let found = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        (msg.contains("unknown") || msg.contains("not found")) && d.message.contains("UnknownDef")
    });
    assert!(
        found,
        "expected error mentioning 'unknown'/'not found' and 'UnknownDef', got: {:?}",
        errors
    );
}

// ── Step 15: missing required argument ───────────────────────────────────────

#[test]
fn missing_required_argument() {
    let source = r#"
constraint def TwoParam {
    param a: Length
    param b: Length
    a > b
}
structure S {
    param x: Length
    constraint TwoParam(a: x)
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for missing argument 'b'"
    );
    let found = errors
        .iter()
        .any(|d| d.message.contains('b') || d.message.to_lowercase().contains("missing"));
    assert!(
        found,
        "expected error mentioning missing argument 'b', got: {:?}",
        errors
    );
}

// ── Step 17: extra/unknown argument ──────────────────────────────────────────

#[test]
fn unknown_argument_name() {
    let source = r#"
constraint def OneParam {
    param a: Length
    a > 0
}
structure S {
    param x: Length
    constraint OneParam(a: x, b: 5)
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown argument 'b'"
    );
    let found = errors
        .iter()
        .any(|d| d.message.contains('b') || d.message.to_lowercase().contains("unknown"));
    assert!(
        found,
        "expected error mentioning unknown argument 'b', got: {:?}",
        errors
    );
}

// ── Step 19: where-clause on constraint instantiation ────────────────────────

#[test]
fn constraint_inst_with_where_clause() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2mm
}
structure S {
    param mode: Bool
    param t: Length
    constraint MinWall(wall: t) where mode
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // The constraint should NOT appear in top-level constraints
    assert!(
        tmpl.constraints.is_empty(),
        "constraint should not be in top-level constraints when guarded, got: {:?}",
        tmpl.constraints
    );

    // The constraint SHOULD appear in a guarded_group
    assert!(
        !tmpl.guarded_groups.is_empty(),
        "expected at least one guarded_group"
    );
    let total_guarded_constraints: usize = tmpl
        .guarded_groups
        .iter()
        .map(|g| g.constraints.len())
        .sum();
    assert_eq!(
        total_guarded_constraints, 1,
        "expected 1 guarded constraint, found {total_guarded_constraints}"
    );
}

// ── Step 1 (task-198): constraint instantiation labels ───────────────────────

/// Single-predicate constraint def instantiation should produce a CompiledConstraint
/// with label == Some("MinWall#0[0]").
///
/// NOTE (task 4490): `wall > 2` was updated to `wall > 2mm` because comparing
/// a dimensional Length parameter to a bare integer (> 2) is now correctly
/// rejected by the compile-time operand guard.
#[test]
fn constraint_inst_label_single_predicate() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2mm
}
structure S {
    param thickness: Length
    constraint MinWall(wall: thickness)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    let cc = &tmpl.constraints[0];
    assert_eq!(
        cc.label,
        Some("MinWall#0[0]".to_string()),
        "expected label Some(\"MinWall#0[0]\"), got: {:?}",
        cc.label
    );
}

/// Multi-predicate constraint def instantiation should produce labeled constraints
/// Some("Bounded#0[0]") and Some("Bounded#0[1]") respectively.
///
/// NOTE (task 4490): `lo: 1, hi: 10` were updated to `lo: 1mm, hi: 10mm` because
/// passing a bare integer as a Length argument results in `w >= 1` (Scalar<Length>
/// >= Int) which is now correctly rejected by the compile-time dimension guard.
#[test]
fn constraint_inst_label_multi_predicate() {
    let source = r#"
constraint def Bounded {
    param x: Length
    param lo: Length
    param hi: Length
    x >= lo
    x <= hi
}
structure S {
    param w: Length
    constraint Bounded(x: w, lo: 1mm, hi: 10mm)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected exactly 2 constraints");

    // Use label-based lookup rather than positional access (task 848.2) —
    // robust against future changes to constraint ordering in the template.
    // Presence of each label is the only assertion; the `.find()` calls
    // panic with a clear message if either is missing.
    tmpl.constraints
        .iter()
        .find(|c| c.label.as_deref() == Some("Bounded#0[0]"))
        .unwrap_or_else(|| {
            panic!(
                "expected constraint with label Some(\"Bounded#0[0]\"), got labels: {:?}",
                tmpl.constraints
                    .iter()
                    .map(|c| c.label.as_deref())
                    .collect::<Vec<_>>()
            )
        });
    tmpl.constraints
        .iter()
        .find(|c| c.label.as_deref() == Some("Bounded#0[1]"))
        .unwrap_or_else(|| {
            panic!(
                "expected constraint with label Some(\"Bounded#0[1]\"), got labels: {:?}",
                tmpl.constraints
                    .iter()
                    .map(|c| c.label.as_deref())
                    .collect::<Vec<_>>()
            )
        });

    // Locks in intra-instantiation predicate source order (task 2083).
    // Prefix filter pins inst_idx=0 so additional instantiations, if any, are ignored.
    let inst_0_pred_indices: Vec<usize> = tmpl
        .constraints
        .iter()
        .filter_map(|c| c.label.as_deref())
        .filter_map(|lbl| {
            lbl.strip_prefix("Bounded#0[")
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|n| n.parse::<usize>().ok())
        })
        .collect();
    assert_eq!(
        inst_0_pred_indices,
        vec![0, 1],
        "pred_idx must match source order within a single instantiation \
         (inst_idx=0); got {:?}",
        inst_0_pred_indices
    );
}

// ── Step 3 (task-1717): substitute_expr recurses into Conditional branches ───

/// substitute_expr must recurse into all three branches of a Conditional
/// (condition, then-branch, else-branch).  This test verifies the behavior
/// by instantiating a constraint def whose predicate is an `if/then/else`
/// expression and asserting that ValueRefs (not bare idents) appear in every
/// branch after substitution + compilation.
#[test]
fn constraint_inst_conditional_substitution() {
    let source = r#"
constraint def Gated {
    param x: Length
    param threshold: Length
    if x > threshold then x < 100mm else x > 0mm
}
structure S {
    param width: Length
    param limit: Length
    constraint Gated(x: width, threshold: limit)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    let cc = &tmpl.constraints[0];
    // The compiled constraint should be a Conditional expression.
    match &cc.expr.kind {
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            // condition: x > threshold  →  BinOp(Gt, ValueRef(S.width), ValueRef(S.limit))
            match &condition.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    assert_eq!(*op, BinOp::Gt, "condition op should be Gt");
                    assert!(
                        matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "width"),
                        "condition left should be ValueRef(S.width), got {:?}",
                        left.kind
                    );
                    assert!(
                        matches!(&right.kind, CompiledExprKind::ValueRef(id) if id.member == "limit"),
                        "condition right should be ValueRef(S.limit), got {:?}",
                        right.kind
                    );
                }
                other => panic!("expected BinOp for condition, got {:?}", other),
            }
            // then_branch: x < 100mm  →  BinOp(Lt, ValueRef(S.width), Literal)
            match &then_branch.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    assert_eq!(*op, BinOp::Lt, "then_branch op should be Lt");
                    assert_value_ref(left, "width");
                    assert!(
                        matches!(&right.kind, CompiledExprKind::Literal(_)),
                        "then_branch right should be Literal (100mm), got {:?}",
                        right.kind
                    );
                }
                other => panic!("expected BinOp for then_branch, got {:?}", other),
            }
            // else_branch: x > 0mm  →  BinOp(Gt, ValueRef(S.width), Literal)
            match &else_branch.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    assert_eq!(*op, BinOp::Gt, "else_branch op should be Gt");
                    assert_value_ref(left, "width");
                    assert!(
                        matches!(&right.kind, CompiledExprKind::Literal(_)),
                        "else_branch right should be Literal (0mm), got {:?}",
                        right.kind
                    );
                }
                other => panic!("expected BinOp for else_branch, got {:?}", other),
            }

            // Cross-branch ValueCellId consistency: all three references to
            // parameter `x` (substituted to `width`) must resolve to the
            // *same* ValueCellId (entity + member).  A bug that produces
            // different entity prefixes across branches (e.g. 'S' vs 'S2')
            // would be invisible to the per-branch member-name checks above.
            let condition_width_id = match &condition.kind {
                CompiledExprKind::BinOp { left, .. } => {
                    extract_value_ref_id(left, "condition.left")
                }
                other => panic!("expected BinOp for condition, got {:?}", other),
            };
            let then_width_id = match &then_branch.kind {
                CompiledExprKind::BinOp { left, .. } => {
                    extract_value_ref_id(left, "then_branch.left")
                }
                other => panic!("expected BinOp for then_branch, got {:?}", other),
            };
            let else_width_id = match &else_branch.kind {
                CompiledExprKind::BinOp { left, .. } => {
                    extract_value_ref_id(left, "else_branch.left")
                }
                other => panic!("expected BinOp for else_branch, got {:?}", other),
            };
            assert_eq!(
                condition_width_id, then_width_id,
                "condition and then_branch must reference the same ValueCellId for param x"
            );
            assert_eq!(
                condition_width_id, else_width_id,
                "condition and else_branch must reference the same ValueCellId for param x"
            );
        }
        other => panic!("expected Conditional constraint expr, got {:?}", other),
    }
}

// ── Step 4 (task-1717): substitute_expr handles Match arms (no shadowing) ────

/// substitute_expr must substitute param references in both the match
/// discriminant and each arm body.  Match arm patterns are structural
/// (enum variants) — they do NOT introduce binders, so no shadowing
/// suppression applies.  This test verifies that behaviour.
#[test]
fn constraint_inst_match_substitution() {
    let source = r#"
enum Quality { Standard, Premium }

constraint def QualityBound {
    param grade: Quality
    param x: Length
    match grade { Standard => x < 100mm, Premium => x < 10mm }
}
structure S {
    param quality: Quality
    param size: Length
    constraint QualityBound(grade: quality, x: size)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    let cc = &tmpl.constraints[0];
    // The compiled constraint should be a Match expression.
    match &cc.expr.kind {
        CompiledExprKind::Match { discriminant, arms } => {
            // Discriminant: grade → ValueRef(S.quality)
            assert!(
                matches!(&discriminant.kind, CompiledExprKind::ValueRef(id) if id.member == "quality"),
                "discriminant should be ValueRef(S.quality), got {:?}",
                discriminant.kind
            );
            // Two arms: Standard (x < 100mm) and Premium (x < 10mm).
            // Arms are emitted in source order by the compiler; tests index
            // by position to detect body swaps or duplication.
            assert_eq!(arms.len(), 2, "expected 2 match arms");

            // arms[0]: Standard => x < 100mm
            assert_eq!(
                arms[0].patterns,
                vec![reify_ir::CompiledPattern::variant("Standard")],
                "first arm should be Standard"
            );
            match &arms[0].body.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    assert_eq!(*op, BinOp::Lt, "Standard arm op should be Lt");
                    assert_value_ref(left, "size");
                    // Right should be Literal(100mm); 100mm = 0.1 m in SI.
                    assert!(
                        matches!(
                            &right.kind,
                            CompiledExprKind::Literal(Value::Scalar { si_value, .. })
                            if (si_value - 0.1_f64).abs() < 1e-9
                        ),
                        "Standard arm right should be Literal(100mm = 0.1 SI), got {:?}",
                        right.kind
                    );
                }
                other => panic!("expected BinOp in Standard arm body, got {:?}", other),
            }

            // arms[1]: Premium => x < 10mm
            assert_eq!(
                arms[1].patterns,
                vec![reify_ir::CompiledPattern::variant("Premium")],
                "second arm should be Premium"
            );
            match &arms[1].body.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    assert_eq!(*op, BinOp::Lt, "Premium arm op should be Lt");
                    assert_value_ref(left, "size");
                    // Right should be Literal(10mm); 10mm = 0.01 m in SI.
                    assert!(
                        matches!(
                            &right.kind,
                            CompiledExprKind::Literal(Value::Scalar { si_value, .. })
                            if (si_value - 0.01_f64).abs() < 1e-9
                        ),
                        "Premium arm right should be Literal(10mm = 0.01 SI), got {:?}",
                        right.kind
                    );
                }
                other => panic!("expected BinOp in Premium arm body, got {:?}", other),
            }
        }
        other => panic!("expected Match constraint expr, got {:?}", other),
    }
}

// ── Task 845: unique labels across multi-instantiation ───────────────────────

/// Two distinct instantiations of the same constraint def inside one entity
/// must produce distinct labels (else they collide in diagnostic output).
/// The label format is `{def_name}#{inst_idx}[{pred_idx}]`; each instantiation
/// gets its own inst_idx so two single-predicate instantiations become
/// `MinWall#0[0]` and `MinWall#1[0]`.
///
/// NOTE (task 4490): `wall > 2` was updated to `wall > 2mm` because comparing
/// a dimensional Length parameter to a bare integer (> 2) is now correctly
/// rejected by the compile-time operand guard.
#[test]
fn multi_instantiation_labels_are_unique() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2mm
}
structure S {
    param wall_a: Length
    param wall_b: Length
    constraint MinWall(wall: wall_a)
    constraint MinWall(wall: wall_b)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected exactly 2 constraints");

    let labels: Vec<_> = tmpl.constraints.iter().map(|c| c.label.clone()).collect();

    assert_ne!(
        labels[0], labels[1],
        "labels from two instantiations must differ, got {:?}",
        labels
    );
    assert!(
        labels.contains(&Some("MinWall#0[0]".to_string())),
        "expected MinWall#0[0] among labels, got {:?}",
        labels
    );
    assert!(
        labels.contains(&Some("MinWall#1[0]".to_string())),
        "expected MinWall#1[0] among labels, got {:?}",
        labels
    );
}

// ── η/4480 step-05: explicit arg-binding capture on CompiledConstraint ─────────
//
// The η conformance pass (PRD docs/prds/v0_6/gdt-geometric-zones-and-containment.md,
// contract C3/C5) detects a *geometric* `Conforms` instance by the presence of an
// EXPLICIT `actual` argument binding on the compiled constraint instance.
// `Conforms`'s predicate body never references `actual`, so — unlike
// `RepresentationWithin`, whose args ARE its predicate — the binding cannot be
// recovered by walking the compiled predicate. It must be captured at
// instantiation time onto `CompiledConstraint.arg_bindings`.
//
// This test pins the general capability with a minimal fixture: a constraint def
// whose `u : Geometry` param is UNUSED in the predicate. An explicit `u: g`
// binding must survive to `arg_bindings`, while an instantiation that omits `u`
// (letting it fall to its `nominal()` default) must NOT record `u`. The compiled
// predicate must be identical in both cases (B4: scalar path byte-identical).

/// Collect the parameter names captured in a constraint's `arg_bindings` — the
/// explicit call-site argument bindings recorded on the compiled instance.
fn binding_names(cc: &reify_compiler::CompiledConstraint) -> Vec<&str> {
    cc.arg_bindings
        .iter()
        .map(|(name, _)| name.as_str())
        .collect()
}

#[test]
fn explicit_arg_binding_for_unused_param_survives_to_compiled_constraint() {
    // Two instantiations of the SAME constraint def in the SAME structure, so
    // the compiled predicate (`thickness >= 0mm`, qualified by entity "S") is
    // byte-identical between them — the only difference is whether the UNUSED
    // geometry param `u` was explicitly bound.
    let source = r#"
constraint def X {
    param a : Length
    param u : Geometry = nominal()
    a >= 0mm
}
structure S {
    param thickness : Length
    param g : Geometry
    constraint X(a: thickness, u: g)
    constraint X(a: thickness)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(
        tmpl.constraints.len(),
        2,
        "expected exactly 2 constraints (two X instantiations)"
    );

    // Partition by whether the UNUSED geometry param `u` was explicitly bound.
    let explicit = tmpl
        .constraints
        .iter()
        .find(|c| binding_names(c).contains(&"u"))
        .expect("one instantiation explicitly binds the unused param `u`");
    let default_only = tmpl
        .constraints
        .iter()
        .find(|c| !binding_names(c).contains(&"u"))
        .expect("one instantiation omits `u` (falls to its nominal() default)");

    // Explicit instantiation records BOTH the used `a` and the UNUSED `u`.
    let explicit_names = binding_names(explicit);
    assert!(
        explicit_names.contains(&"a"),
        "explicit binding must record 'a', got {:?}",
        explicit_names
    );
    assert!(
        explicit_names.contains(&"u"),
        "explicit binding of the UNUSED geometry param 'u' must survive to \
         arg_bindings (the η detection signal), got {:?}",
        explicit_names
    );

    // Default-only instantiation records `a` only — `u` fell to nominal().
    let default_names = binding_names(default_only);
    assert!(
        default_names.contains(&"a"),
        "default-only binding must record 'a', got {:?}",
        default_names
    );
    assert!(
        !default_names.contains(&"u"),
        "an omitted param (defaulted to nominal()) must NOT appear in arg_bindings, \
         got {:?}",
        default_names
    );

    // B4: binding the unused param does NOT change the compiled predicate — both
    // instantiations live in entity "S", so the scalar predicate is byte-identical.
    assert_eq!(
        format!("{:?}", explicit.expr.kind),
        format!("{:?}", default_only.expr.kind),
        "binding the unused param must NOT change the compiled predicate (B4)"
    );
}

// ── Task 6416: the #4546 arg type check now fires for enum-typed params ──────

/// Count `ConstraintArgTypeMismatch` diagnostics in a compiled module.
///
/// Shared by the task-6416 cases below so the reject and accept sides are
/// counted identically — the established idiom from
/// `constraint_def_compile_tests.rs`'s `constraint_arg_type_mismatch_carries_code`.
fn arg_type_mismatch_count(module: &reify_compiler::CompiledModule) -> usize {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintArgTypeMismatch))
        .count()
}

/// Assert the `ConstraintArgTypeMismatch` count for `source`, with a message
/// naming `what` and dumping the diagnostics on failure.
///
/// When `expected == 0` the module is ALSO asserted to compile with zero
/// Error-severity diagnostics. Without that second half every accept-side guard
/// here is VACUOUS: a constraint def that failed to compile, a `Zq.Close` that
/// failed to resolve, a parse error, or a spurious def-site `unknown type` each
/// yield zero mismatch diagnostics too, so a bare count of 0 cannot tell
/// "accepted" from "never checked". With it, 0 means compiled, checked, passed.
fn assert_arg_type_mismatches(source: &str, expected: usize, what: &str) {
    let module = compile_source(source);
    let got = arg_type_mismatch_count(&module);
    assert_eq!(
        got, expected,
        "expected {} ConstraintArgTypeMismatch diagnostic(s) for {}, got {}; diagnostics: {:?}",
        expected, what, got, module.diagnostics
    );
    if expected == 0 {
        let errors = error_diags(&module.diagnostics);
        assert!(
            errors.is_empty(),
            "expected {} to compile CLEAN, but got error diagnostic(s): {:?}. \
             A zero mismatch count is only meaningful if the arg was actually \
             reached and checked",
            what,
            errors
        );
    }
}

/// Before task 6416 an enum-typed param carried `ty: None`, so
/// `expand_constraint_inst` skipped it and an Int arg passed where an enum was
/// declared produced ZERO diagnostics. Now `ty` is `Some(Enum("Zq"))` and Rule 5
/// of `constraint_arg_type_conforms` rejects it.
///
/// This is the user-visible defect in the task title, pinned independently of
/// the def-side structural assertion in `constraint_def_compile_tests.rs`.
#[test]
fn int_arg_for_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: 5)
}
"#,
        1,
        "an Int literal passed to an Enum(Zq) param",
    );
}

/// Bool counterpart of `int_arg_for_enum_param_is_rejected` — also measured at
/// zero diagnostics before task 6416.
#[test]
fn bool_arg_for_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: true)
}
"#,
        1,
        "a Bool literal passed to an Enum(Zq) param",
    );
}

/// A variant of the WRONG enum must be rejected too — `Enum(Other)` vs
/// `Enum(Zq)` is a Rule-5 rejection, not merely a non-enum-vs-enum one. This is
/// the case most likely to bite in real code, since it still *looks* enum-typed.
#[test]
fn wrong_enum_variant_arg_for_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }
enum Other { A, B }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: Other.A)
}
"#,
        1,
        "an Other.A variant passed to an Enum(Zq) param",
    );
}

/// Guard: the correct variant literal must NOT be rejected.
///
/// Task 6416 activates a type check against a population it has never run on,
/// so the dominant risk is a FALSE POSITIVE on valid code, not a missed
/// rejection. The three guards below pin that boundary from the accept side —
/// the reject tests above would stay green under an over-broad future change
/// that also began rejecting valid enum args.
#[test]
fn correct_enum_variant_arg_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: Zq.Close)
}
"#,
        0,
        "the correct Zq.Close variant passed to an Enum(Zq) param",
    );
}

/// Guard: an enum param actually REFERENCED by the predicate must still be
/// accepted, exercising the predicate-substitution path rather than the trivial
/// `true` body used by the other cases.
#[test]
fn enum_param_referenced_by_predicate_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    g == Zq.Close
}
structure S {
    constraint K(g: Zq.Medium)
}
"#,
        0,
        "an enum arg bound to a param referenced by the predicate",
    );
}

/// Guard: an enum-typed STRUCTURE param forwarded as the arg must be accepted.
///
/// This is the shape of the one pre-existing enum-typed constraint-def param in
/// tracked source (`constraint_inst_match_substitution` above), converting an
/// incidental pass into an explicit contract. Structure params already resolved
/// bare enum names before task 6416, so the arg's type is `Enum(Zq)` and Rule 3
/// (`type_compatible` identity) accepts.
#[test]
fn enum_typed_structure_param_arg_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    param q: Zq
    constraint K(g: q)
}
"#,
        0,
        "an enum-typed structure param forwarded to an Enum(Zq) param",
    );
}

// ── Task 6416: the arg type check on the OPTION-WRAPPED enum param ──────────
//
// `param g : Option<Zq>` is the third spelling the `EnumNameScope` install newly
// resolves (`constraint_def_compile_tests.rs` pins the def-side
// `Some(Option(Enum("Zq")))`), and the only one that was user-visibly BROKEN
// before rather than merely under-typed. Resolving it activates #4546's arg
// check on a shape that had none, so the three cases below pin the consequence
// users actually see.

/// The `some(..)` spelling must be accepted for an `Option<Zq>` param.
#[test]
fn some_wrapped_enum_arg_for_option_typed_param_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Option<Zq>
    true
}
structure S {
    constraint K(g: some(Zq.Close))
}
"#,
        0,
        "a some(Zq.Close) arg passed to an Option<Enum(Zq)> param",
    );
}

/// A BARE variant passed to an `Option<Zq>` param must be rejected: there is no
/// `T -> Option<T>` widening in `type_compatible` (`type_compat.rs`), so the
/// `some(..)` wrapper is required — the same rule struct params already follow.
///
/// This direction is deliberate, not incidental, and is pinned so a future
/// widening (or a regression back to `ty: None`, which would skip the check and
/// silently accept) cannot land unnoticed. Before task 6416 this source emitted
/// a spurious def-site `unknown type 'Option'` and zero mismatches.
#[test]
fn bare_enum_arg_for_option_typed_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Option<Zq>
    true
}
structure S {
    constraint K(g: Zq.Close)
}
"#,
        1,
        "a bare Zq.Close variant passed to an Option<Enum(Zq)> param",
    );
}

/// A bare `none` is REJECTED for an `Option<Zq>` param. This pins TODAY's
/// behaviour, not a desired contract: `expr.rs` types a bare `none` as
/// `Option<Real>` ("contextual override happens at param/let sites") and the
/// constraint-arg binding site applies no such override, so #4546's check
/// compares `Option<Real>` against `Option<Enum(Zq)>`.
///
/// MEASURED: the cause is enum-independent and pre-existing — `param g :
/// Option<Length>` + `K(g: none)` fails identically ("expected
/// Option<Scalar[m]>, got Option<Real>"). Task 6416 only makes `Option<Zq>`
/// reach the check at all; before it, the def site emitted a spurious `unknown
/// type 'Option'`. Contextual typing of `none` here is filed as follow-up
/// ticket tkt_0RTT1BWK4518B7W6XX74XSE79D — flip the expected count to 0 when it
/// lands.
#[test]
fn bare_none_arg_for_option_typed_enum_param_is_rejected_today() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Option<Zq>
    true
}
structure S {
    constraint K(g: none)
}
"#,
        1,
        "a bare `none` passed to an Option<Enum(Zq)> param (today's behaviour: \
         `none` defaults to Option<Real>)",
    );
}

// ── Task 6416 / step-5: the arg type check on the ENUM-BODIED ALIAS path ─────
//
// Task 6259's parity harness in `tests/harness_langcore/type_alias_compile_tests.rs`
// compares `alias_ty` against `direct_ty` only, so reverting task 6416's
// `EnumNameScope` install collapses both sides to `None` and leaves it green.
// The two cases below pin ABSOLUTE diagnostic counts through the alias spelling,
// which is what actually detects such a revert.

/// An Int literal passed to an ALIAS-typed enum param must be rejected exactly
/// as it is for the direct spelling — `type AL = Zq` resolves to `Enum(Zq)`, so
/// `expand_constraint_inst` type-checks the arg instead of skipping it.
#[test]
fn int_arg_for_alias_typed_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }
type AL = Zq

constraint def K {
    param g : AL
    true
}
structure S {
    constraint K(g: 5)
}
"#,
        1,
        "an Int literal passed to an alias-typed (`type AL = Zq`) Enum param",
    );
}

/// Accept-side guard for the alias spelling: the correct variant must still pass.
///
/// Without this, the reject case above would stay green under an over-broad
/// future change that began rejecting every alias-typed enum arg — the same
/// false-positive risk the direct-spelling guards above cover.
#[test]
fn correct_enum_variant_arg_for_alias_typed_param_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }
type AL = Zq

constraint def K {
    param g : AL
    true
}
structure S {
    constraint K(g: Zq.Close)
}
"#,
        0,
        "the correct Zq.Close variant passed to an alias-typed (`type AL = Zq`) Enum param",
    );
}
