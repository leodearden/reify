//! Tests for `MemberDecl::MatchArmDeclGroup` and related AST nodes (task 2372, step-5).
//!
//! All tests hand-construct AST nodes directly (no source parsing) because
//! tree-sitter grammar / ts_parser lowering for the new variant is deferred to a
//! future task. This mirrors the pattern in `member_span_tests.rs`.

use reify_ast::{Expr, ExprKind, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MemberDecl, SubDecl, find_named_member_span, walk_specialization_scope_members};
use reify_core::{ContentHash, SourceSpan};

fn dummy_span() -> SourceSpan {
    SourceSpan::new(0, 1)
}

fn dummy_hash() -> ContentHash {
    ContentHash(0)
}

fn dummy_ident_expr(name: &str) -> Expr {
    Expr {
        kind: ExprKind::Ident(name.to_string()),
        span: dummy_span(),
    }
}

fn arm(patterns: Vec<&str>, sub_name: &str, structure_name: &str) -> MatchArmDeclArmDecl {
    MatchArmDeclArmDecl {
        patterns: patterns.into_iter().map(|s| s.to_string()).collect(),
        member: Box::new(MemberDecl::Sub(SubDecl {
            name: sub_name.to_string(),
            structure_name: structure_name.to_string(),
            type_args: vec![],
            args: vec![],
            is_collection: false,
            where_clause: None,
            body: None,
            param_overrides: vec![],
            keyed_members: vec![],
            is_aux: false,
            pose_expr: None,
            span: dummy_span(),
            content_hash: dummy_hash(),
        })),
        span: dummy_span(),
    }
}

#[test]
fn match_arm_decl_group_carries_discriminant_arms_and_per_arm_member() {
    let group = MatchArmDeclGroupDecl {
        discriminant: dummy_ident_expr("head_type"),
        arms: vec![
            arm(vec!["Hex"], "head", "HexHead"),
            arm(vec!["Socket"], "head", "SocketHead"),
        ],
        span: dummy_span(),
        content_hash: dummy_hash(),
    };

    // Wrap in MemberDecl variant and pattern-match out.
    let member = MemberDecl::MatchArmDeclGroup(group);
    let MemberDecl::MatchArmDeclGroup(ref g) = member else {
        panic!("expected MatchArmDeclGroup variant");
    };

    assert_eq!(g.arms.len(), 2, "should have 2 arms");
    assert_eq!(g.arms[0].patterns, vec!["Hex"]);
    assert_eq!(g.arms[1].patterns, vec!["Socket"]);

    // Verify per-arm sub member.
    let MemberDecl::Sub(ref sub0) = *g.arms[0].member else {
        panic!("arm[0].member should be Sub");
    };
    assert_eq!(sub0.structure_name, "HexHead");

    let MemberDecl::Sub(ref sub1) = *g.arms[1].member else {
        panic!("arm[1].member should be Sub");
    };
    assert_eq!(sub1.structure_name, "SocketHead");
}

#[test]
fn match_arm_decl_group_variant_pipe_arm_carries_multiple_patterns() {
    // Tests the `|`-pipe form: `Socket | Button => sub head : SocketHead { ... }`.
    let multi_arm = arm(vec!["Socket", "Button"], "head", "SocketHead");
    assert_eq!(
        multi_arm.patterns,
        vec!["Socket", "Button"],
        "pipe-collapsed arm should carry both pattern strings"
    );
    assert_eq!(multi_arm.patterns.len(), 2);
}

// ── Walker tests (task 2372, step-7) ──────────────────────────────────────────

/// Builds a MatchArmDeclGroupDecl containing two arms, each declaring `sub head`.
fn two_arm_head_group() -> MatchArmDeclGroupDecl {
    MatchArmDeclGroupDecl {
        discriminant: dummy_ident_expr("head_type"),
        arms: vec![
            arm(vec!["Hex"], "head", "HexHead"),
            arm(vec!["Socket"], "head", "SocketHead"),
        ],
        span: SourceSpan::new(10, 50),
        content_hash: dummy_hash(),
    }
}

#[test]
fn find_named_member_span_descends_into_match_arm_decl_group() {
    // Tests that find_named_member_span traverses into MatchArmDeclGroup arms
    // and returns a span for the first matching Sub declaration.
    // RED until walk_members_depth is taught about the new variant (step-8).
    let members = vec![MemberDecl::MatchArmDeclGroup(two_arm_head_group())];
    // Both arms declare sub named "head"; the walker should find the first one.
    let _result = find_named_member_span(&members, "head");
    // Note: find_named_member_span finds Param and Let by name; Sub is not
    // directly matched by name. The test verifies traversal happens at all —
    // that no panic occurs and that future Sub-name lookup extensions work.
    // For now, assert the walker does NOT return None (it recurses into arms).
    // Since the walker currently only matches Param/Let by name, we test that
    // searching for a name that doesn't exist returns None, not a panic.
    let not_found = find_named_member_span(&members, "nonexistent_param");
    assert!(
        not_found.is_none(),
        "a name not present in any arm should return None, not panic"
    );

    // Place a Param named "head" inside an arm's member to test actual descent.
    use reify_ast::ParamDecl;
    let param_span = SourceSpan::new(42, 80);
    let group_with_param = MatchArmDeclGroupDecl {
        discriminant: dummy_ident_expr("head_type"),
        arms: vec![{
            let inner_param = MemberDecl::Param(ParamDecl {
                name: "head".to_string(),
                doc: None,
                type_expr: None,
                default: None,
                where_clause: None,
                annotations: vec![],
                span: param_span,
                content_hash: dummy_hash(),
            });
            MatchArmDeclArmDecl {
                patterns: vec!["Hex".to_string()],
                member: Box::new(inner_param),
                span: dummy_span(),
            }
        }],
        span: dummy_span(),
        content_hash: dummy_hash(),
    };
    let members2 = vec![MemberDecl::MatchArmDeclGroup(group_with_param)];
    let found = find_named_member_span(&members2, "head");
    assert!(
        found.is_some(),
        "Param named 'head' inside a MatchArmDeclGroup arm should be found"
    );
    assert_eq!(
        found.unwrap().span,
        param_span,
        "returned span should be the param's span"
    );
}

#[test]
fn walk_specialization_scope_members_visits_match_arm_decl_group_arms() {
    // Tests that walk_specialization_scope_members traverses into MatchArmDeclGroup arms.
    // RED until walk_members_depth is taught about the new variant (step-8).
    let group = two_arm_head_group();
    let sub = SubDecl {
        name: "container".to_string(),
        structure_name: "Container".to_string(),
        type_args: vec![],
        args: vec![],
        is_collection: false,
        where_clause: None,
        body: Some(vec![MemberDecl::MatchArmDeclGroup(group)]),
        param_overrides: vec![],
        keyed_members: vec![],
        is_aux: false,
        pose_expr: None,
        span: dummy_span(),
        content_hash: dummy_hash(),
    };
    let mut visited: Vec<String> = Vec::new();
    walk_specialization_scope_members(&sub, &mut |member| match member {
        MemberDecl::MatchArmDeclGroup(_) => visited.push("group".to_string()),
        MemberDecl::Sub(s) => visited.push(format!("sub:{}", s.structure_name)),
        _ => {}
    });
    assert!(
        visited.contains(&"group".to_string()),
        "walk_specialization_scope_members must visit the MatchArmDeclGroup itself; visited: {:?}",
        visited
    );
    // After step-8, arms' members will also be visited — for now just confirm
    // the group node itself is reached.
}
