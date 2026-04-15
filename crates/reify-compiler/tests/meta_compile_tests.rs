//! Tests for meta block compilation — `meta { key = "value" }` and `meta.key` access.

use reify_compiler::TopologyTemplate;
use reify_test_support::{compile_first_template, compile_source};
use reify_types::{CompiledExpr, CompiledExprKind, Severity};

/// Helper: get the default_expr for a value cell by member name.
fn get_cell_expr<'a>(
    template: &'a TopologyTemplate,
    member: &str,
) -> &'a reify_types::CompiledExpr {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("should have '{}' value cell", member));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{}' should have a default expr", member))
}

// ---------------------------------------------------------------------------
// step-1: meta block entries stored in template.meta
// ---------------------------------------------------------------------------

#[test]
fn meta_block_stored_in_template() {
    let source = r#"
        structure def Bracket {
            meta {
                description = "A bracket",
                part_number = "BR-001"
            }
            param width : Length = 10mm
        }
    "#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(template.meta.len(), 2);
    assert_eq!(template.meta.get("description").unwrap(), "A bracket");
    assert_eq!(template.meta.get("part_number").unwrap(), "BR-001");
}

// ---------------------------------------------------------------------------
// step-3: meta.key compiles to MetaAccess with Type::String
// ---------------------------------------------------------------------------

#[test]
fn meta_access_compiles_to_string() {
    let source = r#"
        structure def Bracket {
            meta {
                description = "A bracket"
            }
            let desc : String = meta.description
        }
    "#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let expr = get_cell_expr(&template, "desc");
    match &expr.kind {
        CompiledExprKind::MetaAccess { entity, key } => {
            assert_eq!(entity, "Bracket");
            assert_eq!(key, "description");
        }
        other => panic!("expected MetaAccess, got {:?}", other),
    }
    assert_eq!(expr.result_type, reify_types::Type::String);
}

// ---------------------------------------------------------------------------
// step-5: nonexistent meta key produces compile-time error
// ---------------------------------------------------------------------------

#[test]
fn meta_access_nonexistent_key_error() {
    let source = r#"
        structure def Bracket {
            meta {
                a = "1"
            }
            let x : String = meta.nonexistent
        }
    "#;
    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one error");
    assert!(
        errors.iter().any(|d| d.message.contains("no key")),
        "expected 'no key' error, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// step-7: accessing meta without a meta block produces error
// ---------------------------------------------------------------------------

#[test]
fn meta_access_no_meta_block_error() {
    let source = r#"
        structure def Bracket {
            param width : Length = 10mm
            let x : String = meta.foo
        }
    "#;
    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one error");
    assert!(
        errors.iter().any(|d| d.message.contains("no meta block")),
        "expected 'no meta block' error, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// step-9: duplicate meta blocks produce error
// ---------------------------------------------------------------------------

#[test]
fn duplicate_meta_block_error() {
    let source = r#"
        structure def Bracket {
            meta {
                a = "1"
            }
            meta {
                b = "2"
            }
            param width : Length = 10mm
        }
    "#;
    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one error");
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("duplicate meta block")),
        "expected 'duplicate meta block' error, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// step-11: meta.key works inside constraint expressions
// ---------------------------------------------------------------------------

/// Recursively checks whether any node in the expression tree is a MetaAccess.
fn contains_meta_access(expr: &CompiledExpr) -> bool {
    let mut found = false;
    expr.walk(&mut |e| {
        if matches!(&e.kind, CompiledExprKind::MetaAccess { .. }) {
            found = true;
        }
    });
    found
}

#[test]
fn meta_access_in_constraint_context() {
    let source = r#"
        structure def Bracket {
            meta {
                tag = "valid"
            }
            constraint meta.tag == "valid"
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

    let constraint_expr = &template.constraints[0].expr;
    assert!(
        contains_meta_access(constraint_expr),
        "constraint expr should contain a MetaAccess node, got: {:?}",
        constraint_expr.kind
    );

    // The constraint is `meta.tag == "valid"`, so top-level should be BinOp::Eq
    match &constraint_expr.kind {
        CompiledExprKind::BinOp { op, left, .. } => {
            assert_eq!(*op, reify_types::BinOp::Eq, "expected Eq comparison");
            // LHS should be the MetaAccess
            match &left.kind {
                CompiledExprKind::MetaAccess { entity, key } => {
                    assert_eq!(entity, "Bracket");
                    assert_eq!(key, "tag");
                    assert_eq!(left.result_type, reify_types::Type::String);
                }
                other => panic!("expected MetaAccess as LHS of comparison, got {:?}", other),
            }
        }
        other => panic!("expected BinOp at top level of constraint, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-1 (task-389): meta value change affects content_hash
// ---------------------------------------------------------------------------

/// Two entities identical except for a meta entry value must produce different
/// content_hashes. This test FAILS until meta entries are included in the hash.
#[test]
fn meta_change_affects_content_hash() {
    let source_a = r#"
        structure def Widget {
            meta {
                description = "Version A"
            }
            param width : Length = 10mm
        }
    "#;
    let source_b = r#"
        structure def Widget {
            meta {
                description = "Version B"
            }
            param width : Length = 10mm
        }
    "#;

    let (template_a, diags_a) = compile_first_template(source_a);
    let (template_b, diags_b) = compile_first_template(source_b);

    let errors_a: Vec<_> = diags_a.iter().filter(|d| d.severity == Severity::Error).collect();
    let errors_b: Vec<_> = diags_b.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors_a.is_empty(), "unexpected errors in source_a: {:?}", errors_a);
    assert!(errors_b.is_empty(), "unexpected errors in source_b: {:?}", errors_b);

    assert_ne!(
        template_a.content_hash,
        template_b.content_hash,
        "entities differing only in meta value must have different content_hashes"
    );
}

// ---------------------------------------------------------------------------
// step-2 (task-389): meta presence/absence affects content_hash
// ---------------------------------------------------------------------------

/// An entity with a meta block and the same entity without one must produce
/// different content_hashes. This test FAILS until meta entries are included
/// in the hash.
#[test]
fn meta_presence_affects_content_hash() {
    let source_with_meta = r#"
        structure def Gadget {
            meta {
                part_number = "G-001"
            }
            param height : Length = 5mm
        }
    "#;
    let source_without_meta = r#"
        structure def Gadget {
            param height : Length = 5mm
        }
    "#;

    let (template_with, diags_with) = compile_first_template(source_with_meta);
    let (template_without, diags_without) = compile_first_template(source_without_meta);

    let errors_with: Vec<_> = diags_with.iter().filter(|d| d.severity == Severity::Error).collect();
    let errors_without: Vec<_> = diags_without.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors_with.is_empty(), "unexpected errors (with meta): {:?}", errors_with);
    assert!(errors_without.is_empty(), "unexpected errors (without meta): {:?}", errors_without);

    assert_ne!(
        template_with.content_hash,
        template_without.content_hash,
        "entity with meta block must have a different content_hash than the same entity without one"
    );
}
