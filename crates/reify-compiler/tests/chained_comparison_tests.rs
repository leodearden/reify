//! Chained comparison desugaring tests.
//!
//! Tests that chained comparisons like `a < b < c` are desugared
//! into And-chains of pairwise comparisons: `And(Lt(a,b), Lt(b,c))`.

use reify_test_support::compile_first_template;
use reify_core::Severity;
use reify_ir::{BinOp, CompiledExprKind};

/// step-1: `constraint a < b < c` desugars to `And(Lt(a,b), Lt(b,c))`.
#[test]
fn simple_chain_desugars_to_and() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    param c : Int = 3
    constraint a < b < c
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::And, "top-level op should be And, got {:?}", op);
            // left should be Lt(a, b)
            match &left.kind {
                CompiledExprKind::BinOp {
                    op: lop,
                    left: ll,
                    right: lr,
                } => {
                    assert_eq!(*lop, BinOp::Lt, "left pairwise op should be Lt");
                    assert!(
                        matches!(&ll.kind, CompiledExprKind::ValueRef(_)),
                        "left.left should be value ref a"
                    );
                    assert!(
                        matches!(&lr.kind, CompiledExprKind::ValueRef(_)),
                        "left.right should be value ref b"
                    );
                }
                other => panic!("expected BinOp(Lt) for left, got {:?}", other),
            }
            // right should be Lt(b, c)
            match &right.kind {
                CompiledExprKind::BinOp {
                    op: rop,
                    left: rl,
                    right: rr,
                } => {
                    assert_eq!(*rop, BinOp::Lt, "right pairwise op should be Lt");
                    assert!(
                        matches!(&rl.kind, CompiledExprKind::ValueRef(_)),
                        "right.left should be value ref b"
                    );
                    assert!(
                        matches!(&rr.kind, CompiledExprKind::ValueRef(_)),
                        "right.right should be value ref c"
                    );
                }
                other => panic!("expected BinOp(Lt) for right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp(And) at top level, got {:?}", other),
    }
}

/// step-3: `constraint a < b <= c` desugars to `And(Lt(a,b), Le(b,c))`.
/// Both operator variants are preserved in the desugared output.
#[test]
fn mixed_operators_chain_preserves_ops() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    param c : Int = 3
    constraint a < b <= c
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::And, "top-level op should be And");
            // left should be Lt(a, b)
            match &left.kind {
                CompiledExprKind::BinOp { op: lop, .. } => {
                    assert_eq!(*lop, BinOp::Lt, "left pairwise op should be Lt");
                }
                other => panic!("expected BinOp for left, got {:?}", other),
            }
            // right should be Le(b, c)
            match &right.kind {
                CompiledExprKind::BinOp { op: rop, .. } => {
                    assert_eq!(*rop, BinOp::Le, "right pairwise op should be Le");
                }
                other => panic!("expected BinOp for right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp(And) at top level, got {:?}", other),
    }
}

/// step-5: `constraint a < b < c < d` desugars to `And(And(Lt(a,b), Lt(b,c)), Lt(c,d))`.
/// Verifies the recursive left-folded And structure with 3 pairwise comparisons.
#[test]
fn three_operator_chain_left_folds() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    param c : Int = 3
    param d : Int = 4
    constraint a < b < c < d
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    // Outer: And(And(...), Lt(c,d))
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::And, "outermost op should be And");
            // right should be Lt(c, d)
            match &right.kind {
                CompiledExprKind::BinOp { op: rop, .. } => {
                    assert_eq!(*rop, BinOp::Lt, "outermost right should be Lt(c,d)");
                }
                other => panic!("expected BinOp(Lt) for outermost right, got {:?}", other),
            }
            // left should be And(Lt(a,b), Lt(b,c))
            match &left.kind {
                CompiledExprKind::BinOp {
                    op: lop,
                    left: ll,
                    right: lr,
                } => {
                    assert_eq!(*lop, BinOp::And, "inner left op should be And");
                    match &ll.kind {
                        CompiledExprKind::BinOp { op: llop, .. } => {
                            assert_eq!(*llop, BinOp::Lt, "inner And left should be Lt(a,b)");
                        }
                        other => panic!("expected BinOp(Lt) for inner And left, got {:?}", other),
                    }
                    match &lr.kind {
                        CompiledExprKind::BinOp { op: lrop, .. } => {
                            assert_eq!(*lrop, BinOp::Lt, "inner And right should be Lt(b,c)");
                        }
                        other => panic!("expected BinOp(Lt) for inner And right, got {:?}", other),
                    }
                }
                other => panic!("expected BinOp(And) for inner left, got {:?}", other),
            }
        }
        other => panic!("expected BinOp(And) at outer level, got {:?}", other),
    }
}

/// step-7: `constraint a == b == c` desugars to `And(Eq(a,b), Eq(b,c))`.
/// Equality operators participate in chaining.
#[test]
fn equality_chain_desugars_to_and() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 1
    param c : Int = 1
    constraint a == b == c
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::And, "top-level op should be And");
            match &left.kind {
                CompiledExprKind::BinOp { op: lop, .. } => {
                    assert_eq!(*lop, BinOp::Eq, "left pairwise op should be Eq");
                }
                other => panic!("expected BinOp(Eq) for left, got {:?}", other),
            }
            match &right.kind {
                CompiledExprKind::BinOp { op: rop, .. } => {
                    assert_eq!(*rop, BinOp::Eq, "right pairwise op should be Eq");
                }
                other => panic!("expected BinOp(Eq) for right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp(And) at top level, got {:?}", other),
    }
}

/// step-9: middle expression compiled once.
/// `constraint a < b + c < d` — the middle expression `b + c` should appear as BinOp(Add)
/// in both the right side of the first comparison and the left side of the second,
/// and both occurrences should have the same content_hash (cloned, not recompiled).
#[test]
fn middle_expression_compiled_once() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    param c : Int = 3
    param d : Int = 10
    constraint a < b + c < d
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::And, "top-level op should be And");
            // left: Lt(a, b+c) — extract right side (b+c)
            let left_rhs_hash = match &left.kind {
                CompiledExprKind::BinOp {
                    op: lop, right: lr, ..
                } => {
                    assert_eq!(*lop, BinOp::Lt);
                    assert!(
                        matches!(&lr.kind, CompiledExprKind::BinOp { op: addop, .. } if *addop == BinOp::Add),
                        "left rhs should be Add(b,c)"
                    );
                    lr.content_hash
                }
                other => panic!("expected BinOp(Lt) for left, got {:?}", other),
            };
            // right: Lt(b+c, d) — extract left side (b+c)
            let right_lhs_hash = match &right.kind {
                CompiledExprKind::BinOp {
                    op: rop, left: rl, ..
                } => {
                    assert_eq!(*rop, BinOp::Lt);
                    assert!(
                        matches!(&rl.kind, CompiledExprKind::BinOp { op: addop, .. } if *addop == BinOp::Add),
                        "right lhs should be Add(b,c)"
                    );
                    rl.content_hash
                }
                other => panic!("expected BinOp(Lt) for right, got {:?}", other),
            };
            assert_eq!(
                left_rhs_hash, right_lhs_hash,
                "middle expression b+c should have identical content_hash in both comparisons"
            );
        }
        other => panic!("expected BinOp(And) at step-9 top level, got {:?}", other),
    }
}

/// step-11: non-comparison binary op on left does NOT trigger chaining.
/// `(a + b) < c` should compile as plain `Lt(Add(a,b), c)` — no And-wrapping.
#[test]
fn arithmetic_on_left_does_not_chain() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    param c : Int = 10
    constraint (a + b) < c
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Lt, "should be plain Lt, not And");
            assert!(
                matches!(&left.kind, CompiledExprKind::BinOp { op: addop, .. } if *addop == BinOp::Add),
                "left should be Add(a,b), not an And-chain"
            );
            assert!(
                matches!(&right.kind, CompiledExprKind::ValueRef(_)),
                "right should be value ref c"
            );
        }
        other => panic!("expected plain BinOp(Lt), got {:?}", other),
    }
}

/// step-13: single comparison `a < b` stays as plain `Lt(a,b)` — no desugaring.
/// Regression test ensuring desugaring only activates for actual chains (>2 operands).
#[test]
fn single_comparison_stays_plain() {
    let source = r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    constraint a < b
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(
                *op,
                BinOp::Lt,
                "single comparison should stay as Lt, not wrapped in And"
            );
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(_)),
                "left should be value ref a"
            );
            assert!(
                matches!(&right.kind, CompiledExprKind::ValueRef(_)),
                "right should be value ref b"
            );
        }
        other => panic!("expected plain BinOp(Lt), got {:?}", other),
    }
}

/// step-15: chained comparison with Scalar quantities — real-world range constraint.
/// `constraint 2mm < thickness < 10mm` should compile without errors,
/// produce an And of two comparisons, and have result_type Bool.
#[test]
fn scalar_range_constraint() {
    let source = r#"
structure S {
    param thickness : Scalar = 5mm
    constraint 2mm < thickness < 10mm
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let expr = &template.constraints[0].expr;
    // Result type should be Bool
    assert_eq!(
        expr.result_type,
        reify_core::Type::Bool,
        "constraint expression should have type Bool"
    );

    // Should be And(Lt(2mm, thickness), Lt(thickness, 10mm))
    match &expr.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(*op, BinOp::And, "top-level op should be And");
        }
        other => panic!("expected BinOp(And), got {:?}", other),
    }
}

/// step-17: chained comparison in a let binding.
/// `let in_range = 0 < x < 100` — the let cell's initializer is desugared to
/// an And-chain with type Bool.
#[test]
fn let_binding_chain_desugars_to_and() {
    let source = r#"
structure S {
    param x : Int = 50
    let in_range = 0 < x < 100
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Find the `in_range` value cell
    let in_range_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "in_range")
        .expect("should have 'in_range' value cell");

    // Its default_expr should be desugared to And-chain
    let init = in_range_cell
        .default_expr
        .as_ref()
        .expect("in_range should have default_expr");
    assert_eq!(
        init.result_type,
        reify_core::Type::Bool,
        "in_range should have type Bool"
    );
    match &init.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(op, &BinOp::And, "in_range default_expr should be And-chain");
        }
        other => panic!(
            "expected BinOp(And) for in_range default_expr, got {:?}",
            other
        ),
    }
}
