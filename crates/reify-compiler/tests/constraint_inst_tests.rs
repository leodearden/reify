//! Compiler tests for constraint instantiation.
//!
//! Tests for `constraint ConstraintName(arg: expr, ...)` inside structure bodies.
//! Validates that the compiler resolves the constraint def, binds args to params,
//! substitutes param references in predicate expressions, and injects resulting
//! constraints into the parent entity's constraint list.

use reify_ir::*;
use reify_test_support::{compile_source, compile_template};
use reify_core::*;

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
/// After substitution, constraint expr should be `thickness > 2`.
#[test]
fn basic_constraint_inst_compiles() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
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
    // The compiled expr should be BinOp(Gt, ValueRef(S.thickness), Literal(2.0))
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
            // Number literal `2` compiles to Int(2).
            match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => {
                    assert_eq!(*v, 2, "right should be 2, got {v}");
                }
                CompiledExprKind::Literal(Value::Real(v)) => {
                    assert!((v - 2.0).abs() < 1e-9, "right should be 2.0, got {v}");
                }
                other => panic!("expected Literal(2 or 2.0) for right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp for constraint expr, got {:?}", other),
    }
}

// ── Step 9: multi-predicate constraint def ───────────────────────────────────

/// Constraint def with 3 params and 2 predicates; structure instantiates with literals.
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
    constraint Bounded(x: w, lo: 1, hi: 10)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected exactly 2 constraints");

    // First constraint: w >= 1
    match &tmpl.constraints[0].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Ge, "first constraint should be Ge (>=)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "w"),
                "left should be ValueRef(S.w)"
            );
            let right_is_one = match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => *v == 1,
                CompiledExprKind::Literal(Value::Real(v)) => (v - 1.0).abs() < 1e-9,
                _ => false,
            };
            assert!(
                right_is_one,
                "right should be Literal(1), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp for first constraint, got {:?}", other),
    }

    // Second constraint: w <= 10
    match &tmpl.constraints[1].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Le, "second constraint should be Le (<=)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "w"),
                "left should be ValueRef(S.w)"
            );
            let right_is_ten = match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => *v == 10,
                CompiledExprKind::Literal(Value::Real(v)) => (v - 10.0).abs() < 1e-9,
                _ => false,
            };
            assert!(
                right_is_ten,
                "right should be Literal(10), got {:?}",
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
    wall > 2
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
#[test]
fn constraint_inst_label_single_predicate() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
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
    constraint Bounded(x: w, lo: 1, hi: 10)
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
                vec!["Standard".to_string()],
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
                vec!["Premium".to_string()],
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
#[test]
fn multi_instantiation_labels_are_unique() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
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
    cc.arg_bindings.iter().map(|(name, _)| name.as_str()).collect()
}

#[test]
fn explicit_arg_binding_for_unused_param_survives_to_compiled_constraint() {
    let source = r#"
constraint def X {
    param a : Length
    param u : Geometry = nominal()
    a >= 0mm
}
structure WithExplicit {
    param thickness : Length
    param g : Geometry
    constraint X(a: thickness, u: g)
}
structure WithoutExplicit {
    param thickness : Length
    constraint X(a: thickness)
}
"#;
    let compiled = compile_source(source);

    let errors = error_diags(&compiled.diagnostics);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let with_explicit = compiled
        .templates
        .iter()
        .find(|t| t.name == "WithExplicit")
        .expect("WithExplicit template");
    let without_explicit = compiled
        .templates
        .iter()
        .find(|t| t.name == "WithoutExplicit")
        .expect("WithoutExplicit template");

    assert_eq!(
        with_explicit.constraints.len(),
        1,
        "WithExplicit should emit exactly 1 constraint"
    );
    assert_eq!(
        without_explicit.constraints.len(),
        1,
        "WithoutExplicit should emit exactly 1 constraint"
    );

    // Explicit instantiation: both `a` and the UNUSED `u` are recorded.
    let explicit_names = binding_names(&with_explicit.constraints[0]);
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

    // Default-only instantiation: `u` is NOT recorded — it fell to nominal().
    let default_names = binding_names(&without_explicit.constraints[0]);
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

    // B4: the compiled predicate is identical whether or not `u` was bound — the
    // unused geometry param never touches the scalar predicate expr.
    assert_eq!(
        format!("{:?}", with_explicit.constraints[0].expr.kind),
        format!("{:?}", without_explicit.constraints[0].expr.kind),
        "binding the unused param must NOT change the compiled predicate (B4)"
    );
}
