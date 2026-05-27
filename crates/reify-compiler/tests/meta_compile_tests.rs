//! Tests for meta block compilation — `meta { key = "value" }` and `meta.key` access.

use reify_compiler::TopologyTemplate;
use reify_test_support::compile_first_template;
use reify_core::Severity;
use reify_ir::{CompiledExpr, CompiledExprKind};

/// Helper: get the default_expr for a value cell by member name.
fn get_cell_expr<'a>(
    template: &'a TopologyTemplate,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
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
    assert_eq!(expr.result_type, reify_core::Type::String);
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
// step-1: duplicate meta key within a single meta block produces error
// ---------------------------------------------------------------------------

#[test]
fn duplicate_meta_key_error() {
    let source = r#"
        structure def Bracket {
            meta {
                a = "1",
                a = "2"
            }
            param width : Length = 10mm
        }
    "#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error for duplicate meta key, got: {:?}",
        errors
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("duplicate meta key")),
        "expected 'duplicate meta key' error, got: {:?}",
        errors
    );
    assert!(
        errors[0].labels.iter().any(|l| l.message.contains("'a'")),
        "label should name the duplicate key 'a', got labels: {:?}",
        errors[0].labels
    );

    // The label should name the specific key for better IDE hover UX.
    assert!(
        errors[0].labels.iter().any(|l| l.message.contains("'a'")),
        "expected label to contain the key name \"'a'\", got labels: {:?}",
        errors[0].labels
    );

    // First occurrence should be kept; second (duplicate) should be discarded.
    assert_eq!(
        template.meta.get("a").map(|s| s.as_str()),
        Some("1"),
        "first value should be kept, not the duplicate"
    );
}

// ---------------------------------------------------------------------------
// step-3: multiple distinct duplicate meta keys each produce an error
// ---------------------------------------------------------------------------

#[test]
fn duplicate_meta_key_multiple_duplicates() {
    let source = r#"
        structure def Bracket {
            meta {
                x = "1",
                y = "2",
                x = "3",
                y = "4"
            }
            param width : Length = 10mm
        }
    "#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    // One error for the duplicate "x" and one for the duplicate "y".
    assert_eq!(
        errors.len(),
        2,
        "expected exactly two errors (one per duplicated key), got: {:?}",
        errors
    );
    assert!(
        errors
            .iter()
            .all(|d| d.message.contains("duplicate meta key")),
        "all errors should be 'duplicate meta key' errors, got: {:?}",
        errors
    );
    // Each duplicated key should have its own error message naming the key.
    assert!(
        errors.iter().any(|d| d.message.contains("'x'")),
        "expected an error mentioning key 'x', got: {:?}",
        errors
    );
    assert!(
        errors.iter().any(|d| d.message.contains("'y'")),
        "expected an error mentioning key 'y', got: {:?}",
        errors
    );
    // All label texts should mention "duplicate key" (not just the static fallback).
    assert!(
        errors
            .iter()
            .all(|d| d.labels.iter().any(|l| l.message.contains("duplicate key"))),
        "all error labels should contain 'duplicate key', got: {:?}",
        errors
    );

    // Regression guard: each error message must name the specific duplicated key.
    assert!(
        errors.iter().any(|d| d.message.contains("'x'")),
        "expected an error mentioning key \"'x'\", got: {:?}",
        errors
    );
    assert!(
        errors.iter().any(|d| d.message.contains("'y'")),
        "expected an error mentioning key \"'y'\", got: {:?}",
        errors
    );

    // Each error's label should also name the specific key (IDE hover UX).
    for key in ["'x'", "'y'"] {
        assert!(
            errors
                .iter()
                .any(|d| d.labels.iter().any(|l| l.message.contains(key))),
            "expected a label mentioning key {key}, got: {errors:?}",
        );
    }

    // First values should be kept.
    assert_eq!(template.meta.get("x").map(|s| s.as_str()), Some("1"));
    assert_eq!(template.meta.get("y").map(|s| s.as_str()), Some("2"));
}

// ---------------------------------------------------------------------------
// step-5: unique meta keys produce no errors (regression guard)
// ---------------------------------------------------------------------------

#[test]
fn meta_block_no_duplicate_keys_no_error() {
    let source = r#"
        structure def Bracket {
            meta {
                a = "1",
                b = "2",
                c = "3"
            }
            param width : Length = 10mm
        }
    "#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected errors for unique meta keys: {:?}",
        errors
    );

    assert_eq!(template.meta.get("a").map(|s| s.as_str()), Some("1"));
    assert_eq!(template.meta.get("b").map(|s| s.as_str()), Some("2"));
    assert_eq!(template.meta.get("c").map(|s| s.as_str()), Some("3"));
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
            assert_eq!(*op, reify_ir::BinOp::Eq, "expected Eq comparison");
            // LHS should be the MetaAccess
            match &left.kind {
                CompiledExprKind::MetaAccess { entity, key } => {
                    assert_eq!(entity, "Bracket");
                    assert_eq!(key, "tag");
                    assert_eq!(left.result_type, reify_core::Type::String);
                }
                other => panic!("expected MetaAccess as LHS of comparison, got {:?}", other),
            }
        }
        other => panic!("expected BinOp at top level of constraint, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-1 (task-388): empty meta block + access gives 'no key' (not 'no meta block')
// ---------------------------------------------------------------------------

#[test]
fn empty_meta_block_access_gives_no_key_error() {
    let source = r#"
        structure def Bracket {
            meta {}
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
        errors
            .iter()
            .any(|d| d.message.contains("meta block has no key")),
        "expected 'meta block has no key' error for empty meta block access, got: {:?}",
        errors
    );
    assert!(
        !errors.iter().any(|d| d.message.contains("no meta block")),
        "should NOT produce 'no meta block' error when meta {{}} is present, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// step-2 (task-388): empty meta block without access compiles cleanly
// ---------------------------------------------------------------------------
// Regression guard (task-388): entity with NO meta block at all — accessing
// `meta.foo` must still produce "no meta block" (not "no key").  This guards
// against the `has_meta_block` refactor accidentally merging the two error
// paths.
#[test]
fn no_meta_block_access_still_gives_no_meta_block_error() {
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
        "expected 'no meta block' error when no meta block is declared, got: {:?}",
        errors
    );
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("meta block has no key")),
        "should NOT produce 'meta block has no key' when no meta block is declared, got: {:?}",
        errors
    );
}

#[test]
fn empty_meta_block_stored_in_template() {
    let source = r#"
        structure def Bracket {
            meta {}
            param width : Length = 10mm
        }
    "#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "empty meta block should compile without errors, got: {:?}",
        errors
    );
    assert!(
        template.meta.is_empty(),
        "template.meta should be empty for an empty meta block"
    );
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

    let errors_a: Vec<_> = diags_a
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors_b: Vec<_> = diags_b
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_a.is_empty(),
        "unexpected errors in source_a: {:?}",
        errors_a
    );
    assert!(
        errors_b.is_empty(),
        "unexpected errors in source_b: {:?}",
        errors_b
    );

    assert_ne!(
        template_a.content_hash, template_b.content_hash,
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

    let errors_with: Vec<_> = diags_with
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors_without: Vec<_> = diags_without
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_with.is_empty(),
        "unexpected errors (with meta): {:?}",
        errors_with
    );
    assert!(
        errors_without.is_empty(),
        "unexpected errors (without meta): {:?}",
        errors_without
    );

    assert_ne!(
        template_with.content_hash, template_without.content_hash,
        "entity with meta block must have a different content_hash than the same entity without one"
    );
}

// ---------------------------------------------------------------------------
// step-4 (task-389): meta content_hash is deterministic across compilations
// ---------------------------------------------------------------------------

/// Compiling the same source twice must yield identical content_hashes.
///
/// Note: Rust's HashMap uses a random hasher seeded once per process, so
/// within a single process the iteration order for the same map tends to be
/// consistent.  This test is a useful regression guard but cannot reliably
/// catch a *missing* sort on its own.  The stronger determinism property is
/// covered by `meta_content_hash_key_order_independent` below, which compiles
/// two sources whose keys are listed in reverse lexicographic order and asserts
/// they produce the same hash.
#[test]
fn meta_content_hash_is_deterministic() {
    let source = r#"
        structure def Plate {
            meta {
                material = "steel",
                finish = "anodized",
                part_number = "PL-007"
            }
            param thickness : Length = 3mm
        }
    "#;

    let (template1, diags1) = compile_first_template(source);
    let (template2, diags2) = compile_first_template(source);

    let errors1: Vec<_> = diags1
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors2: Vec<_> = diags2
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors1.is_empty(),
        "unexpected errors (run 1): {:?}",
        errors1
    );
    assert!(
        errors2.is_empty(),
        "unexpected errors (run 2): {:?}",
        errors2
    );

    assert_eq!(
        template1.content_hash, template2.content_hash,
        "two compilations of identical source must produce the same content_hash"
    );
}

// ---------------------------------------------------------------------------
// amend (task-389): meta key rename affects content_hash
// ---------------------------------------------------------------------------

/// Two entities with the same meta *value* but different *keys* must produce
/// different content_hashes.  This closes the gap between the stated intent
/// ("hash both key and value so that key renames and value changes are both
/// detected") and the test coverage — the value-change case is covered by
/// `meta_change_affects_content_hash`, this covers the key-rename case.
#[test]
fn meta_key_rename_affects_content_hash() {
    let source_author = r#"
        structure def Widget {
            meta {
                author = "Alice"
            }
            param width : Length = 10mm
        }
    "#;
    let source_creator = r#"
        structure def Widget {
            meta {
                creator = "Alice"
            }
            param width : Length = 10mm
        }
    "#;

    let (template_author, diags_author) = compile_first_template(source_author);
    let (template_creator, diags_creator) = compile_first_template(source_creator);

    let errors_author: Vec<_> = diags_author
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors_creator: Vec<_> = diags_creator
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_author.is_empty(),
        "unexpected errors (author): {:?}",
        errors_author
    );
    assert!(
        errors_creator.is_empty(),
        "unexpected errors (creator): {:?}",
        errors_creator
    );

    assert_ne!(
        template_author.content_hash, template_creator.content_hash,
        "entities differing only in meta key name must have different content_hashes"
    );
}

// ---------------------------------------------------------------------------
// amend (task-389): meta content_hash is independent of source key order
// ---------------------------------------------------------------------------

/// Two entities with the same meta entries listed in different lexicographic
/// orders (e.g., z-key before a-key vs a-key before z-key) must produce
/// *identical* content_hashes.  This is the strongest determinism test: it
/// can only pass if the implementation sorts keys before hashing, regardless
/// of HashMap iteration order within a single process.
#[test]
fn meta_content_hash_key_order_independent() {
    // Same two entries, listed in opposite source order.
    let source_z_first = r#"
        structure def Bolt {
            meta {
                zinc_plated = "yes",
                alloy = "steel"
            }
            param length : Length = 20mm
        }
    "#;
    let source_a_first = r#"
        structure def Bolt {
            meta {
                alloy = "steel",
                zinc_plated = "yes"
            }
            param length : Length = 20mm
        }
    "#;

    let (template_z, diags_z) = compile_first_template(source_z_first);
    let (template_a, diags_a) = compile_first_template(source_a_first);

    let errors_z: Vec<_> = diags_z
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors_a: Vec<_> = diags_a
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_z.is_empty(),
        "unexpected errors (z-first): {:?}",
        errors_z
    );
    assert!(
        errors_a.is_empty(),
        "unexpected errors (a-first): {:?}",
        errors_a
    );

    assert_eq!(
        template_z.content_hash, template_a.content_hash,
        "meta entries listed in different source order must produce the same content_hash"
    );
}
