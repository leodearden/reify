//! Tests for `find_named_member_span` and `MemberSpanInfo` in reify-syntax.
//!
//! These cover: basic param/let lookup, GuardedGroup recursion,
//! Port body recursion, missing-name returns None, and depth limiting.

use reify_syntax::{MAX_MEMBER_NESTING_DEPTH, find_named_member_span};
use reify_types::ModulePath;

/// Helper: parse source and return the first structure's members.
fn parse_first_structure_members(source: &str) -> Vec<reify_syntax::MemberDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s.members.clone(),
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

#[test]
fn let_without_doc_returns_none_doc() {
    let source = "structure S { let x = 5 }";
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "x");
    assert!(result.is_some(), "let 'x' should be found");
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

// ── (g) first-match-wins ordering ────────────────────────────────

#[test]
fn first_match_wins_top_level_over_nested() {
    // Two params named "dup": one at the top level, one inside a where block.
    // The top-level one comes first in source order, so it should be returned.
    let source = r#"structure S {
    param cond : Bool = true
    param dup : Scalar = 1mm
    where cond {
        param dup : Scalar = 999mm
    }
}"#;
    let members = parse_first_structure_members(source);
    let result = find_named_member_span(&members, "dup");
    assert!(result.is_some(), "param 'dup' should be found");
    let info = result.unwrap();
    let decl_text = &source[info.span.start as usize..info.span.end as usize];
    // The top-level declaration contains "1mm", the nested one "999mm".
    assert!(
        decl_text.contains("1mm") && !decl_text.contains("999mm"),
        "first match should be the top-level 'dup', got: {decl_text:?}"
    );
}

// ── (f) depth limiting ────────────────────────────────────────────

/// Build a member tree with `depth` levels of GuardedGroup nesting,
/// placing a single Param named `target` at the innermost level.
fn build_nested_guarded_members(depth: usize, target: &str) -> Vec<reify_syntax::MemberDecl> {
    use reify_syntax::{Expr, ExprKind, GuardedGroupDecl, MemberDecl, ParamDecl};
    use reify_types::{ContentHash, SourceSpan};

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

/// Build a member tree: `outer_depth` levels of GuardedGroup, then a Port
/// whose body contains `port_inner_depth` levels of GuardedGroup wrapping a
/// leaf Param named `target`.
///
/// The depth counter increments once per GuardedGroup and once when entering
/// the Port body, so the leaf is reached at depth
/// `outer_depth + 1 + port_inner_depth`.
fn build_port_in_guarded_members(
    outer_depth: usize,
    port_inner_depth: usize,
    target: &str,
) -> Vec<reify_syntax::MemberDecl> {
    use reify_syntax::{Expr, ExprKind, GuardedGroupDecl, MemberDecl, ParamDecl, PortDecl};
    use reify_types::{ContentHash, PortDirection, SourceSpan};

    let dummy_span = SourceSpan::new(0, 1);
    let dummy_hash = ContentHash(0);
    let dummy_expr = Expr {
        kind: ExprKind::BoolLiteral(true),
        span: dummy_span,
    };

    // Innermost leaf: a single Param with the target name
    let leaf = vec![MemberDecl::Param(ParamDecl {
        name: target.to_string(),
        doc: None,
        type_expr: None,
        default: None,
        where_clause: None,
        span: dummy_span,
        content_hash: dummy_hash,
    })];

    // Wrap leaf in `port_inner_depth` GuardedGroups
    let mut inner = leaf;
    for _ in 0..port_inner_depth {
        inner = vec![MemberDecl::GuardedGroup(GuardedGroupDecl {
            condition: dummy_expr.clone(),
            members: inner,
            else_members: vec![],
            span: dummy_span,
            content_hash: dummy_hash,
        })];
    }

    // Wrap in a Port
    let port = vec![MemberDecl::Port(PortDecl {
        name: "p".to_string(),
        direction: Some(PortDirection::In),
        type_name: "T".to_string(),
        members: inner,
        frame_expr: None,
        span: dummy_span,
        content_hash: dummy_hash,
    })];

    // Wrap Port in `outer_depth` GuardedGroups
    let mut current = port;
    for _ in 0..outer_depth {
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
fn port_inside_guarded_group_shares_depth_counter() {
    // The depth counter increments once per GuardedGroup and once when entering
    // a Port body.  So: 16 outer GuardedGroups + 1 Port entry + 15 inner
    // GuardedGroups puts the leaf at depth 32 (= MAX_MEMBER_NESTING_DEPTH).
    // Should succeed.
    let members = build_port_in_guarded_members(16, 15, "deep_param");
    let result = find_named_member_span(&members, "deep_param");
    assert!(
        result.is_some(),
        "param at depth 16+1+15=32 (= MAX_MEMBER_NESTING_DEPTH) should be found"
    );

    // 16 outer GuardedGroups + 1 Port entry + 16 inner GuardedGroups = depth 33
    // (> MAX_MEMBER_NESTING_DEPTH). Should return None.
    let members = build_port_in_guarded_members(16, 16, "unreachable_param");
    let result = find_named_member_span(&members, "unreachable_param");
    assert!(
        result.is_none(),
        "param at depth 16+1+16=33 (> MAX_MEMBER_NESTING_DEPTH) should NOT be found"
    );
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

/// Minimal tracing subscriber that captures WARN-level messages into a Vec.
mod warn_capture {
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Level, Metadata, Subscriber};

    pub struct MessageCapture {
        pub messages: Arc<Mutex<Vec<String>>>,
    }

    struct Visitor {
        message: Option<String>,
    }

    impl Visit for Visitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "message" {
                self.message = Some(value.to_string());
            }
        }
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.message = Some(format!("{value:?}"));
            }
        }
    }

    impl Subscriber for MessageCapture {
        fn enabled(&self, _meta: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _attrs: &Attributes<'_>) -> Id {
            // Safety: 1 is non-zero.
            unsafe { Id::from_u64(1) }
        }
        fn record(&self, _span: &Id, _values: &Record<'_>) {}
        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
        fn event(&self, event: &Event<'_>) {
            if *event.metadata().level() == Level::WARN {
                let mut v = Visitor { message: None };
                event.record(&mut v);
                if let Some(msg) = v.message {
                    self.messages.lock().unwrap().push(msg);
                }
            }
        }
        fn enter(&self, _span: &Id) {}
        fn exit(&self, _span: &Id) {}
    }
}

#[test]
fn depth_limit_emits_tracing_warning() {
    use std::sync::{Arc, Mutex};
    use warn_capture::MessageCapture;

    let messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let subscriber = MessageCapture { messages: Arc::clone(&messages) };

    tracing::subscriber::with_default(subscriber, || {
        let members =
            build_nested_guarded_members(MAX_MEMBER_NESTING_DEPTH + 1, "unreachable_param");
        let _ = find_named_member_span(&members, "unreachable_param");
    });

    let captured = messages.lock().unwrap();
    assert!(
        captured.iter().any(|m| m.contains("depth limit exceeded")),
        "expected a warning containing 'depth limit exceeded', but captured: {captured:?}"
    );
}
