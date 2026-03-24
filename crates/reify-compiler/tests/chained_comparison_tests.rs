//! Chained comparison desugaring tests.
//!
//! Tests that chained comparisons like `a < b < c` are desugared
//! into And-chains of pairwise comparisons: `And(Lt(a,b), Lt(b,c))`.

use reify_compiler::*;
use reify_types::{CompiledExprKind, BinOp, Diagnostic, ModulePath, Severity};

/// Helper: parse source and compile, returning first template.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let parsed = reify_syntax::parse(source, ModulePath::single("test_chain"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let template = compiled.templates.into_iter().next().expect("expected 1 template");
    (template, compiled.diagnostics)
}

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
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(!template.constraints.is_empty(), "should have at least one constraint");

    let expr = &template.constraints[0].expr;
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::And, "top-level op should be And, got {:?}", op);
            // left should be Lt(a, b)
            match &left.kind {
                CompiledExprKind::BinOp { op: lop, left: ll, right: lr } => {
                    assert_eq!(*lop, BinOp::Lt, "left pairwise op should be Lt");
                    assert!(matches!(&ll.kind, CompiledExprKind::ValueRef(_)), "left.left should be value ref a");
                    assert!(matches!(&lr.kind, CompiledExprKind::ValueRef(_)), "left.right should be value ref b");
                }
                other => panic!("expected BinOp(Lt) for left, got {:?}", other),
            }
            // right should be Lt(b, c)
            match &right.kind {
                CompiledExprKind::BinOp { op: rop, left: rl, right: rr } => {
                    assert_eq!(*rop, BinOp::Lt, "right pairwise op should be Lt");
                    assert!(matches!(&rl.kind, CompiledExprKind::ValueRef(_)), "right.left should be value ref b");
                    assert!(matches!(&rr.kind, CompiledExprKind::ValueRef(_)), "right.right should be value ref c");
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

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert!(!template.constraints.is_empty(), "should have at least one constraint");

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
