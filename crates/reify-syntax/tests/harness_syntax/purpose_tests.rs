//! Purpose declaration tests.
//!
//! Tests for `purpose Name(param : EntityKind) { ... }` declarations.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("purpose_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic purpose (grammar-level) ─────────────────────────

#[test]
fn parse_basic_purpose_grammar() {
    // Parse a purpose declaration and verify it produces a Purpose variant
    // (not an error). This tests that the tree-sitter grammar accepts the syntax.
    let source =
        "purpose mfg_ready(subject : Structure) { constraint subject.params == subject.params }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    match &decls[0] {
        Declaration::Purpose(p) => {
            assert_eq!(p.name, "mfg_ready");
        }
        other => panic!("expected Purpose, got {:?}", other),
    }
}

// ── Step 3: detailed purpose AST ─────────────────────────────────

#[test]
fn parse_purpose_with_forall_constraint() {
    let source = "purpose manufacturing_ready(subject : Structure) { constraint forall p in subject.params: determined(p) }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.name, "manufacturing_ready");
    assert!(!purpose.is_pub);
    assert!(purpose.type_params.is_empty());

    // Check parameters
    assert_eq!(purpose.params.len(), 1);
    assert_eq!(purpose.params[0].name, "subject");
    assert_eq!(purpose.params[0].entity_kind, "Structure");

    // Check members
    assert_eq!(purpose.members.len(), 1);
    match &purpose.members[0] {
        MemberDecl::Constraint(c) => {
            // The constraint contains a forall quantifier
            match &c.expr.kind {
                ExprKind::Quantifier { kind, variable, .. } => {
                    assert_eq!(*kind, QuantifierKind::ForAll);
                    assert_eq!(variable, "p");
                }
                other => panic!("expected Quantifier, got {:?}", other),
            }
        }
        other => panic!("expected Constraint member, got {:?}", other),
    }
}

// ── Step 5: additional purpose syntax tests ───────────────────────

#[test]
fn parse_pub_purpose() {
    let source = "pub purpose visible(part : Structure) { constraint part.mass > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert!(purpose.is_pub);
    assert_eq!(purpose.name, "visible");
}

#[test]
fn parse_purpose_with_type_parameters() {
    let source = "purpose sized<T>(subject : T) { constraint subject.volume > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.name, "sized");
    assert_eq!(purpose.type_params.len(), 1);
    assert_eq!(purpose.type_params[0].name, "T");
}

#[test]
fn parse_purpose_with_multiple_params() {
    let source = "purpose connected(a : Structure, b : Structure) { constraint a.port == b.port }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.params.len(), 2);
    assert_eq!(purpose.params[0].name, "a");
    assert_eq!(purpose.params[0].entity_kind, "Structure");
    assert_eq!(purpose.params[1].name, "b");
    assert_eq!(purpose.params[1].entity_kind, "Structure");
}

#[test]
fn parse_purpose_with_minimize() {
    let source = "purpose lightweight(subject : Structure) { minimize subject.mass }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.members.len(), 1);
    assert!(matches!(&purpose.members[0], MemberDecl::Minimize(_)));
}

#[test]
fn parse_purpose_with_maximize() {
    let source = "purpose strongest(subject : Structure) { maximize subject.strength }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.members.len(), 1);
    assert!(matches!(&purpose.members[0], MemberDecl::Maximize(_)));
}

#[test]
fn parse_purpose_with_where_guard() {
    let source = "purpose guarded(subject : Structure) { where subject.active { constraint subject.mass > 0 } }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.members.len(), 1);
    assert!(matches!(&purpose.members[0], MemberDecl::GuardedGroup(_)));
}

#[test]
fn parse_purpose_with_let() {
    let source = "purpose with_let(subject : Structure) { let total = subject.width + subject.height  constraint total > 10 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.members.len(), 2);
    match &purpose.members[0] {
        MemberDecl::Let(l) => assert_eq!(l.name, "total"),
        other => panic!("expected Let, got {:?}", other),
    }
    assert!(matches!(&purpose.members[1], MemberDecl::Constraint(_)));
}

#[test]
fn parse_purpose_with_mixed_members() {
    let source = r#"purpose manufacturing_ready(subject : Structure) {
        let weight = subject.mass
        constraint weight > 0
        constraint forall p in subject.params: determined(p)
        minimize weight
    }"#;
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.members.len(), 4);
    assert!(matches!(&purpose.members[0], MemberDecl::Let(_)));
    assert!(matches!(&purpose.members[1], MemberDecl::Constraint(_)));
    assert!(matches!(&purpose.members[2], MemberDecl::Constraint(_)));
    assert!(matches!(&purpose.members[3], MemberDecl::Minimize(_)));
}

// ── Task 4639 step-3: purpose-nested structure lowering ───────────────────────

/// Parse a purpose body containing both a `default` declaration and a
/// `structure def` and assert that:
/// - `p.structures` has exactly one entry, `InPurpose`
/// - `p.members` does NOT contain a structure entry (pure MemberDecl list)
/// - `p.defaults` still has exactly one entry (the `default Material` coexists)
///
/// RED until step-4 adds `PurposeDef.structures: Vec<StructureDef>` to reify-ast
/// and wires the lowering — a compile-error RED (field does not exist).
#[test]
fn purpose_nested_structure_lowers_to_structures_field() {
    let source = r#"purpose Exploration() {
        default Material = steel
        structure def InPurpose : Physical {
            param geometry : Solid = box(20mm, 20mm, 20mm)
        }
    }"#;
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose declaration, got {:?}", other),
    };

    // The nested structure must appear in p.structures, not in p.members.
    assert_eq!(
        purpose.structures.len(),
        1,
        "expected exactly 1 nested structure in p.structures; got {:?}",
        purpose.structures
    );
    assert_eq!(
        purpose.structures[0].name, "InPurpose",
        "nested structure name must be 'InPurpose'"
    );

    // p.members must be empty — the structure is not a MemberDecl.
    assert!(
        purpose.members.is_empty(),
        "p.members must not contain the nested structure (no stray MemberDecl); \
         got {:?}",
        purpose.members
    );

    // The default declaration still coexists in p.defaults.
    assert_eq!(
        purpose.defaults.len(),
        1,
        "default Material entry must still be in p.defaults; got {:?}",
        purpose.defaults
    );
}
