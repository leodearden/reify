//! Statement-form `forall` per-element elaboration tests (task 2364, spec §5.4).
//!
//! Task 2363 introduced `MemberDecl::ForallConnect` /
//! `MemberDecl::ForallConstraint` AST nodes with stub error diagnostics in the
//! compiler. Task 2364 lifts those stubs and emits one `CompiledConnection` /
//! `CompiledConstraint` per collection element, with each generated decl
//! carrying a span back to the source `forall` and a label encoding the
//! bound-variable name and element index.
//!
//! These tests pin:
//!   * Per-element emission for `ListLiteral` and `Ident`-resolved-to-collection-sub
//!     collections (PRD criteria 5, 8).
//!   * Empty-collection: zero decls, no error (PRD criterion 6).
//!   * Undef-count collection: zero decls, no error (PRD criterion 7,
//!     first half — re-elaboration on count change is out of scope).
//!   * Label format `forall@<var>[<idx>]` (PRD criterion 10).
//!   * Span anchored at the source forall declaration.
//!   * Body-where-clause routing through guarded groups (PRD criterion 9).
//!   * Constraint-instantiation body shape.
//!   * Chain body shape (pairwise per element).
//!   * Non-iterable collection diagnostic.

use reify_test_support::{compile_source, errors_only};
use reify_types::{BinOp, CompiledExprKind, ModulePath, Value};

/// Recover the `MemberDecl::ForallConstraint` span by re-parsing `source`,
/// finding the structure named `structure_name`, and returning the span of
/// the first ForallConstraint member encountered. Panics if not found.
fn find_forall_constraint_span(
    source: &str,
    structure_name: &str,
) -> reify_types::SourceSpan {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Structure(s) = decl {
            if s.name == structure_name {
                for m in &s.members {
                    if let reify_syntax::MemberDecl::ForallConstraint(f) = m {
                        return f.span;
                    }
                }
            }
        }
    }
    panic!("no ForallConstraint found in structure {}", structure_name);
}

/// `forall v in [1, 2, 3]: constraint v > 0` should emit exactly 3
/// CompiledConstraints, each comparing the substituted literal element
/// against 0 (BinOp::Gt with Literal(Int) on the left), and each carrying a
/// span equal to the `MemberDecl::ForallConstraint` source span.
#[test]
fn forall_constraint_over_list_literal_emits_per_element_constraints() {
    let source = r#"
structure S {
    forall v in [1, 2, 3]: constraint v > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors after lifting ForallConstraint stub, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let forall_constraints: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .collect();

    assert_eq!(
        forall_constraints.len(),
        3,
        "expected exactly 3 forall@v[*] constraints, got {}: labels = {:?}",
        forall_constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    // Recover the source forall span to assert each emitted constraint
    // carries it.
    let forall_span = find_forall_constraint_span(source, "S");
    for c in &forall_constraints {
        assert_eq!(
            c.span, forall_span,
            "forall-emitted constraint span must equal the source forall span; \
             label = {:?}",
            c.label
        );
    }

    // Each emitted constraint is `<element> > 0` — a BinOp::Gt whose left is a
    // Literal(Int) matching the per-element value, and whose right is
    // Literal(Int(0)).
    for (i, c) in forall_constraints.iter().enumerate() {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                assert_eq!(
                    *op,
                    BinOp::Gt,
                    "expected BinOp::Gt in element {} body, got {:?}",
                    i,
                    op
                );
                let expected = (i as i64) + 1;
                match &left.kind {
                    CompiledExprKind::Literal(Value::Int(n)) => assert_eq!(
                        *n, expected,
                        "expected substituted literal {} on left of element {}, got {}",
                        expected, i, n
                    ),
                    other => panic!(
                        "expected Literal(Int({})) on left of element {}, got {:?}",
                        expected, i, other
                    ),
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(0)) => {}
                    other => panic!(
                        "expected Literal(Int(0)) on right of element {}, got {:?}",
                        i, other
                    ),
                }
            }
            other => panic!(
                "expected BinOp(Gt) for element {}, got {:?}",
                i, other
            ),
        }
    }
}
