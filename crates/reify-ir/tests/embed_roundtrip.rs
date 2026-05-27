//! B7 observable signal: the ir→ast embed compiles and round-trips.
//!
//! PRD §8 B7: "the ir→ast embed compiles" — constructs an
//! `AnnotationArgValue::Expr(reify_ast::Expr)` and reads it back, asserting
//! that the `AnnotationArgValue::Expr` variant correctly embeds an AST-level
//! expression inside an IR-level annotation arg, that the value is accessible
//! across the crate boundary, and that `PartialEq` works through the embed.
//!
//! This test proves:
//!   (a) the ir→ast embed compiles after the atomic module move (step-2);
//!   (b) constructing an IR-level annotation arg with an AST-level expression
//!       payload type-checks across the reify-ir / reify-ast crate boundary;
//!   (c) destructuring the `AnnotationArgValue::Expr` variant round-trips the
//!       embedded value losslessly (value == 42.0 is preserved);
//!   (d) `PartialEq` on `AnnotationArgValue::Expr` works through the embed.

use reify_ast::{Expr, ExprKind};
use reify_core::SourceSpan;
use reify_ir::annotation::{AnnotationArg, AnnotationArgValue};

/// (a) + (b) + (c): construct, embed, and destructure round-trip.
#[test]
fn annotation_arg_value_expr_embed_round_trips() {
    // Build a parsed expression carrying a number literal: `42`
    let expr = Expr {
        kind: ExprKind::NumberLiteral {
            value: 42.0,
            is_real: false,
        },
        span: SourceSpan::new(0, 3),
    };

    // Wrap in AnnotationArgValue::Expr and place inside a positional AnnotationArg.
    let arg = AnnotationArg::positional(AnnotationArgValue::Expr(expr));

    // Destructure and assert the embedded value is preserved.
    match arg.value {
        AnnotationArgValue::Expr(inner_expr) => {
            assert!(
                matches!(
                    inner_expr.kind,
                    ExprKind::NumberLiteral { value, .. } if value == 42.0
                ),
                "embedded Expr round-tripped with wrong kind: {:?}",
                inner_expr.kind
            );
        }
        other => panic!(
            "wrong AnnotationArgValue variant after embed round-trip: {:?}",
            other
        ),
    }
}

/// (d): `PartialEq` works through the ir→ast embed.
#[test]
fn annotation_arg_value_expr_partial_eq_through_embed() {
    let make_arg = || {
        AnnotationArgValue::Expr(Expr {
            kind: ExprKind::NumberLiteral {
                value: 1.0,
                is_real: true,
            },
            span: SourceSpan::new(0, 1),
        })
    };

    assert_eq!(
        make_arg(),
        make_arg(),
        "AnnotationArgValue::Expr PartialEq must hold across the ir→ast embed"
    );
}
