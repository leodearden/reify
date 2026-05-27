//! Tests for `find_named_member_span` and `MemberSpanInfo` in reify-syntax.
//!
//! These cover: basic param/let lookup, GuardedGroup recursion,
//! Port body recursion, missing-name returns None, and depth limiting.

use reify_ast::{MAX_MEMBER_NESTING_DEPTH, find_named_member_span};
use reify_core::ModulePath;

/// Helper: parse source and return the first structure's members.
fn parse_first_structure_members(source: &str) -> Vec<reify_ast::MemberDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    match &parsed.declarations[0] {
        reify_ast::Declaration::Structure(s) => s.members.clone(),
        other => panic!("expected Structure, got {:?}", other),
    }
}

// ── (a) basic param lookup ────────────────────────────────────────

#[test]
fn basic_param_returns_member_span_info_with_span_and_doc() {
    let source = r#"structure S {
    /// Width doc
    param width : Scalar = 80mm
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "width");
    assert!(result.is_some(), "param 'width' should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    assert!(
        decl_text.contains("width"),
        "span should cover the param declaration, got: {decl_text:?}"
    );
    assert_eq!(info.doc, Some("Width doc"));
}

#[test]
fn param_without_doc_returns_none_doc() {
    let source = "structure S { param x : Scalar = 5mm }";
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "x");
    assert!(result.is_some(), "param 'x' should be found");
    let info = result.unwrap();
    assert_eq!(info.doc, None);
}

// ── (b) basic let lookup ──────────────────────────────────────────

#[test]
fn basic_let_returns_member_span_info() {
    let source = r#"structure S {
    /// Ratio doc
    let ratio = 2
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "ratio");
    assert!(result.is_some(), "let 'ratio' should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    assert!(
        decl_text.contains("ratio"),
        "span should cover the let declaration, got: {decl_text:?}"
    );
    assert_eq!(info.doc, Some("Ratio doc"));
}

// ── (c) GuardedGroup recursion ────────────────────────────────────

#[test]
fn guarded_group_members_found() {
    let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param guarded_p : Scalar = 5mm
    }
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "guarded_p");
    assert!(result.is_some(), "param inside where block should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    assert!(
        decl_text.contains("guarded_p"),
        "span should cover the guarded param, got: {decl_text:?}"
    );
}

#[test]
fn guarded_group_else_members_found() {
    let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param guarded_p : Scalar = 5mm
    } else {
        param else_p : Scalar = 10mm
    }
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "else_p");
    assert!(result.is_some(), "param inside else block should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    assert!(
        decl_text.contains("else_p"),
        "span should cover the else param, got: {decl_text:?}"
    );
}

// ── (d) Port body recursion ───────────────────────────────────────

#[test]
fn port_body_param_found() {
    let source = r#"structure S {
    port x : MechPort { param d : Length = 10mm }
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "d");
    assert!(result.is_some(), "param inside port body should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    assert!(
        decl_text.contains("d") && decl_text.contains("10mm"),
        "span should cover full param declaration, got: {decl_text:?}"
    );
}

#[test]
fn port_body_let_found() {
    let source = r#"structure S {
    port x : MechPort { let ratio = 2 }
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "ratio");
    assert!(result.is_some(), "let inside port body should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    assert!(
        decl_text.contains("ratio"),
        "span should cover the let declaration, got: {decl_text:?}"
    );
}

// ── (e) missing name returns None ─────────────────────────────────

#[test]
fn missing_name_returns_none() {
    let source = "structure S { param x : Scalar = 5mm }";
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "nonexistent");
    assert!(result.is_none(), "nonexistent name should return None");
}

#[test]
fn empty_members_returns_none() {
    let result = find_named_member_span(&[], "anything");
    assert!(result.is_none(), "empty member slice should return None");
}

// ── (f) depth limiting ────────────────────────────────────────────

/// Build a member tree with `depth` levels of GuardedGroup nesting,
/// placing a single Param named `target` at the innermost level.
fn build_nested_guarded_members(depth: usize, target: &str) -> Vec<reify_ast::MemberDecl> {
    use reify_ast::{Expr, ExprKind, GuardedGroupDecl, MemberDecl, ParamDecl};
    use reify_core::{ContentHash, SourceSpan};

    let dummy_span = SourceSpan::new(0, 1);
    let dummy_hash = ContentHash(0);
    let dummy_expr = Expr {
        kind: ExprKind::BoolLiteral(true),
        span: dummy_span,
    };

    // Innermost level: a single param with the target name
    let leaf = vec![MemberDecl::Param(ParamDecl {
        name: target.to_string(),
        doc: None,
        type_expr: None,
        default: None,
        where_clause: None,
        annotations: Vec::new(),
        span: dummy_span,
        content_hash: dummy_hash,
    })];

    // Wrap in `depth` levels of GuardedGroup
    let mut current = leaf;
    for _ in 0..depth {
        current = vec![MemberDecl::GuardedGroup(GuardedGroupDecl {
            condition: dummy_expr.clone(),
            members: current,
            else_members: vec![],
            span: dummy_span,
            content_hash: dummy_hash,
        })];
    }
    current
}

#[test]
fn depth_limit_succeeds_within_limit() {
    let members = build_nested_guarded_members(5, "deep_param");
    let result = find_named_member_span(&members, "deep_param");
    assert!(
        result.is_some(),
        "param at 5 levels of nesting should be found"
    );
}

#[test]
fn depth_limit_succeeds_at_boundary() {
    // Exactly MAX_MEMBER_NESTING_DEPTH levels — should still succeed
    let members = build_nested_guarded_members(MAX_MEMBER_NESTING_DEPTH, "boundary_param");
    let result = find_named_member_span(&members, "boundary_param");
    assert!(
        result.is_some(),
        "param at exactly MAX_MEMBER_NESTING_DEPTH levels should be found"
    );
}

#[test]
fn depth_limit_returns_none_beyond_limit() {
    // MAX_MEMBER_NESTING_DEPTH + 1 levels — should fail
    let members = build_nested_guarded_members(MAX_MEMBER_NESTING_DEPTH + 1, "unreachable_param");
    let result = find_named_member_span(&members, "unreachable_param");
    assert!(
        result.is_none(),
        "param beyond MAX_MEMBER_NESTING_DEPTH should NOT be found"
    );
}

// ── (g) hand-constructed slice edge cases ─────────────────────────

#[test]
fn find_named_member_span_hand_constructed_depth_2_match() {
    // Exercises GuardedGroup.members recursion at exactly depth-2 on a
    // hand-constructed slice (as opposed to parsed source). Complements
    // the parsed-source tests above by isolating the recursion to a
    // deterministic 2-level slice.
    use reify_core::SourceSpan;
    let members = build_nested_guarded_members(2, "target");
    let result = find_named_member_span(&members, "target");
    assert!(result.is_some(), "param at depth-2 should be found");
    let info = result.unwrap();
    assert_eq!(
        info.span,
        SourceSpan::new(0, 1),
        "returned MemberSpanInfo should carry the dummy span (0,1) used by the helper"
    );
    assert_eq!(info.doc, None, "helper builds param with no doc");
}

#[test]
fn find_named_member_span_hand_constructed_else_only_found() {
    // Exercises the `find_named_member_span_depth` else-branch recursion:
    // GuardedGroup with empty `members` and a single ParamDecl named
    // "target" in `else_members`. Complements the existing parsed-source
    // `guarded_group_else_members_found` test by isolating the else
    // recursion on a hand-constructed slice.
    use reify_ast::{Expr, ExprKind, GuardedGroupDecl, MemberDecl, ParamDecl};
    use reify_core::{ContentHash, SourceSpan};

    let param_span = SourceSpan::new(42, 77);
    let dummy_hash = ContentHash(0);
    let dummy_expr = Expr {
        kind: ExprKind::BoolLiteral(true),
        span: SourceSpan::new(0, 1),
    };

    let members = [MemberDecl::GuardedGroup(GuardedGroupDecl {
        condition: dummy_expr,
        members: vec![],
        else_members: vec![MemberDecl::Param(ParamDecl {
            name: "target".to_string(),
            doc: None,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: Vec::new(),
            span: param_span,
            content_hash: dummy_hash,
        })],
        span: SourceSpan::new(0, 100),
        content_hash: dummy_hash,
    })];

    let result = find_named_member_span(&members, "target");
    assert!(
        result.is_some(),
        "param found only in else_members should be returned"
    );
    let info = result.unwrap();
    assert_eq!(
        info.span, param_span,
        "span should match the else-branch param's span"
    );
    assert_eq!(info.doc, None);
}

#[test]
fn find_named_member_span_hand_constructed_both_branches_empty_returns_none() {
    // Covers the degenerate empty-guard edge case: GuardedGroup with
    // both `members` and `else_members` empty. The top-level
    // `empty_members_returns_none` test covers an empty top-level slice;
    // this adds coverage for an empty *GuardedGroup* with no declarations
    // in either branch.
    use reify_ast::{Expr, ExprKind, GuardedGroupDecl, MemberDecl};
    use reify_core::{ContentHash, SourceSpan};

    let dummy_hash = ContentHash(0);
    let dummy_expr = Expr {
        kind: ExprKind::BoolLiteral(true),
        span: SourceSpan::new(0, 1),
    };

    let members = [MemberDecl::GuardedGroup(GuardedGroupDecl {
        condition: dummy_expr,
        members: vec![],
        else_members: vec![],
        span: SourceSpan::new(0, 10),
        content_hash: dummy_hash,
    })];

    assert!(
        find_named_member_span(&members, "anything").is_none(),
        "empty GuardedGroup in both branches should return None"
    );
}
