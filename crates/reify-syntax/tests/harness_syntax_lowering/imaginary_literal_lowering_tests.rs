//! Integration tests for imaginary literal lowering.
//!
//! Verifies that `4.1j`, `2j`, and `1.5e-3j` desugar to
//! `ExprKind::FunctionCall { name: "complex", args: [NumberLiteral{0.0,..}, NumberLiteral{x,..}] }`.
//!
//! Design decision: `imaginary_literal(x)` is desugared in ts_parser.rs to a
//! `complex(re, im)` FunctionCall (not a new ExprKind variant), so the existing
//! `complex` builtin produces `Complex{0,x,DIMENSIONLESS}` without touching the
//! exhaustive ExprKind match sites across reify-compiler/reify-eval/reify-lsp.
//!
//! See also: `tree-sitter-reify/test/corpus/imaginary_literal.txt` for CST-level tests.

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("imaginary_test"));
    let structure = match &module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// Assert that a source string lowers its first let-binding value to
/// `ExprKind::FunctionCall { name: "complex", args }` and return the args.
fn extract_complex_call_args(source: &str) -> Vec<Expr> {
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    assert!(!members.is_empty(), "expected at least one member, got empty");
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match &let_decl.value.kind {
        ExprKind::FunctionCall { name, args, .. } => {
            assert_eq!(name, "complex", "expected function name 'complex', got '{name}'");
            args.clone()
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

// ── 4.1j: decimal imaginary literal ──────────────────────────────────────────

/// `4.1j` lowers to `complex(0.0, 4.1)`.
/// args[0] is NumberLiteral{value: 0.0, is_real: true} (synthetic re).
/// args[1] is NumberLiteral{value: 4.1, is_real: true} (mantissa has decimal point).
#[test]
fn imaginary_literal_4_1j_lowers_to_complex_call() {
    let args = extract_complex_call_args("structure S {\n  let x = 4.1j\n}");
    assert_eq!(args.len(), 2, "complex() call must have exactly 2 args");

    match args[0].kind {
        ExprKind::NumberLiteral { value, .. } => {
            assert_eq!(value, 0.0_f64, "re arg must be 0.0, got {value}");
        }
        ref other => panic!("expected NumberLiteral for re arg, got {:?}", other),
    }
    match args[1].kind {
        ExprKind::NumberLiteral { value, .. } => {
            assert_eq!(value, 4.1_f64, "im arg must be 4.1, got {value}");
        }
        ref other => panic!("expected NumberLiteral for im arg, got {:?}", other),
    }
}

// ── 2j: integer imaginary literal ────────────────────────────────────────────

/// `2j` lowers to `complex(0.0, 2.0)`.
/// args[1] has value 2.0 (the integer 2 cast to f64).
#[test]
fn imaginary_literal_2j_lowers_to_complex_call() {
    let args = extract_complex_call_args("structure S {\n  let x = 2j\n}");
    assert_eq!(args.len(), 2, "complex() call must have exactly 2 args");

    match args[0].kind {
        ExprKind::NumberLiteral { value, .. } => {
            assert_eq!(value, 0.0_f64, "re arg must be 0.0, got {value}");
        }
        ref other => panic!("expected NumberLiteral for re arg, got {:?}", other),
    }
    match args[1].kind {
        ExprKind::NumberLiteral { value, .. } => {
            assert_eq!(value, 2.0_f64, "im arg must be 2.0, got {value}");
        }
        ref other => panic!("expected NumberLiteral for im arg, got {:?}", other),
    }
}

// ── 1.5e-3j: scientific-notation imaginary literal ────────────────────────────

/// `1.5e-3j` lowers to `complex(0.0, 1.5e-3)`.
/// args[1] has value 1.5e-3_f64 (scientific notation mantissa).
#[test]
fn imaginary_literal_1_5e_minus_3j_lowers_to_complex_call() {
    let args = extract_complex_call_args("structure S {\n  let x = 1.5e-3j\n}");
    assert_eq!(args.len(), 2, "complex() call must have exactly 2 args");

    match args[0].kind {
        ExprKind::NumberLiteral { value, .. } => {
            assert_eq!(value, 0.0_f64, "re arg must be 0.0, got {value}");
        }
        ref other => panic!("expected NumberLiteral for re arg, got {:?}", other),
    }
    match args[1].kind {
        ExprKind::NumberLiteral { value, .. } => {
            assert_eq!(
                value, 1.5e-3_f64,
                "im arg must be 1.5e-3, got {value}"
            );
        }
        ref other => panic!("expected NumberLiteral for im arg, got {:?}", other),
    }
}
