//! Tests for the specialization-scope foundation (spec §8.7, task 2368).
//!
//! Covers:
//!   * AST regression: parsed bare `sub` forms have `body: None` (the new
//!     `body: Option<Vec<MemberDecl>>` field defaults to `None` for the
//!     forms the grammar currently supports).
//!   * `walk_specialization_scope_members` walker visits every member of a
//!     `SubDecl` body, recursing into nested specialization scopes and into
//!     `GuardedGroup.{members,else_members}` branches.
//!   * Walker is depth-bounded — a pathologically deep AST does not stack
//!     overflow.
//!
//! Walker tests use hand-constructed AST nodes: the grammar update for
//! `sub name : Type { body }` is intentionally NOT in this task's scope, so
//! the parser path can never produce `body: Some(_)` yet. Downstream tasks
//! (2369: diagnostic emission; 2370: comprehensive forbidden/permitted
//! coverage) build on this AST contract.

use reify_ast::{ConstraintDecl, Declaration, Expr, ExprKind, GuardedGroupDecl, LetDecl, MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParamDecl, SubDecl, walk_specialization_scope_members};
use reify_core::{ContentHash, ModulePath, SourceSpan};

// ── (a) AST regression: parsed sub forms have body == None ───────────────

/// Helper: parse source and return the first structure's members.
fn parse_first_structure_members(source: &str) -> Vec<MemberDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    match &parsed.declarations[0] {
        Declaration::Structure(s) => s.members.clone(),
        other => panic!("expected Structure, got {:?}", other),
    }
}

/// Helper: locate the first `MemberDecl::Sub` in a member slice.
fn first_sub(members: &[MemberDecl]) -> &SubDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Sub(s) => Some(s),
            _ => None,
        })
        .expect("expected at least one MemberDecl::Sub in the parsed structure")
}

#[test]
fn instantiation_form_has_no_body() {
    let source = "structure S { sub a = Foo() }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.body.is_none(),
        "bare instantiation `sub a = Foo()` should have body == None"
    );
}

#[test]
fn collection_form_has_no_body() {
    let source = "structure S { sub a : List<Foo> }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.body.is_none(),
        "collection form `sub a : List<Foo>` should have body == None"
    );
}

// ── helpers for hand-built AST tests ─────────────────────────────────────

fn dummy_span() -> SourceSpan {
    SourceSpan::new(0, 1)
}

fn dummy_hash() -> ContentHash {
    ContentHash(0)
}

fn make_param(name: &str) -> MemberDecl {
    MemberDecl::Param(ParamDecl {
        name: name.to_string(),
        doc: None,
        type_expr: None,
        default: None,
        where_clause: None,
        annotations: Vec::new(),
        is_priv: false,
        span: dummy_span(),
        content_hash: dummy_hash(),
    })
}

fn make_sub_with_body(name: &str, body: Option<Vec<MemberDecl>>) -> SubDecl {
    SubDecl {
        name: name.to_string(),
        structure_name: "Foo".to_string(),
        type_args: Vec::new(),
        args: Vec::new(),
        is_collection: false,
        where_clause: None,
        body,
        param_overrides: Vec::new(),
        keyed_members: Vec::new(),
        is_aux: false,
        is_priv: false,
        pose_expr: None,
        span: dummy_span(),
        content_hash: dummy_hash(),
    }
}

fn dummy_expr() -> Expr {
    Expr {
        kind: ExprKind::BoolLiteral(true),
        span: dummy_span(),
    }
}

fn make_let(name: &str) -> MemberDecl {
    MemberDecl::Let(LetDecl {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        is_aux: false,
        type_expr: None,
        value: dummy_expr(),
        where_clause: None,
        annotations: Vec::new(),
        span: dummy_span(),
        content_hash: dummy_hash(),
    })
}

fn make_constraint() -> MemberDecl {
    MemberDecl::Constraint(ConstraintDecl {
        label: None,
        expr: dummy_expr(),
        where_clause: None,
        span: dummy_span(),
        content_hash: dummy_hash(),
    })
}

// ── (b) walker visits direct body members ────────────────────────────────

#[test]
fn walker_visits_direct_body_members() {
    let body = vec![make_param("p"), make_constraint(), make_let("v")];
    let sub = make_sub_with_body("scope", Some(body));
    let mut count = 0usize;
    walk_specialization_scope_members(&sub, &mut |_m| count += 1);
    assert_eq!(
        count, 3,
        "walker should visit each direct body member exactly once"
    );
}

#[test]
fn walker_no_op_when_body_is_none() {
    // Bare instantiation (body == None) is NOT a specialization scope —
    // the walker must not invoke the visitor.
    let sub = make_sub_with_body("bare", None);
    let mut count = 0usize;
    walk_specialization_scope_members(&sub, &mut |_m| count += 1);
    assert_eq!(
        count, 0,
        "walker should not visit anything for body == None"
    );
}

// ── (c) walker recurses into nested specialization scopes ───────────────

/// Compact discriminant tag for asserting visitation order.
#[derive(Debug, PartialEq, Eq)]
enum Tag {
    Param,
    Let,
    Constraint,
    Sub,
    GuardedGroup,
}

fn tag_of(member: &MemberDecl) -> Tag {
    match member {
        MemberDecl::Param(_) => Tag::Param,
        MemberDecl::Let(_) => Tag::Let,
        MemberDecl::Constraint(_) => Tag::Constraint,
        MemberDecl::Sub(_) => Tag::Sub,
        MemberDecl::GuardedGroup(_) => Tag::GuardedGroup,
        // Variants below are unused by the current tests but listed
        // explicitly so that adding a new MemberDecl variant in the future
        // forces a deliberate update here rather than silently mapping to
        // an arbitrary tag.
        _ => panic!("tag_of: unexpected MemberDecl variant in test"),
    }
}

#[test]
fn walker_recurses_into_nested_specialization_scope() {
    // Outer SubDecl{body: Some([ Sub{body: Some([Param])} ])}
    // The walker must visit the outer's `Sub` member first, then descend
    // and visit the inner body's `Param`.
    let inner = MemberDecl::Sub(make_sub_with_body("inner", Some(vec![make_param("p")])));
    let outer = make_sub_with_body("outer", Some(vec![inner]));

    let mut tags = Vec::<Tag>::new();
    walk_specialization_scope_members(&outer, &mut |m| tags.push(tag_of(m)));

    assert_eq!(
        tags,
        vec![Tag::Sub, Tag::Param],
        "walker must visit the outer Sub member, then recurse into its nested body",
    );
}

#[test]
fn walker_does_not_recurse_when_nested_sub_body_is_none() {
    // Nested SubDecl with body == None is just a bare instantiation, not
    // a specialization scope — the walker must NOT recurse into it.
    let inner = MemberDecl::Sub(make_sub_with_body("inner", None));
    let outer = make_sub_with_body("outer", Some(vec![inner]));

    let mut count = 0usize;
    walk_specialization_scope_members(&outer, &mut |_m| count += 1);
    assert_eq!(
        count, 1,
        "walker should visit only the outer Sub member when its nested body is None"
    );
}

// ── (d) walker recurses into GuardedGroup branches ──────────────────────

fn make_guarded_group(members: Vec<MemberDecl>, else_members: Vec<MemberDecl>) -> MemberDecl {
    MemberDecl::GuardedGroup(GuardedGroupDecl {
        condition: dummy_expr(),
        members,
        else_members,
        span: dummy_span(),
        content_hash: dummy_hash(),
    })
}

// ── (e) walker depth-bounded ─────────────────────────────────────────────

/// Build a chain of nested specialization scopes `depth` levels deep.
/// Each level is a `SubDecl{body: Some([Sub{body: Some([…])}])}`, with a
/// single `Param` at the innermost level.
fn build_nested_sub_chain(depth: usize) -> SubDecl {
    let mut current = make_sub_with_body("leaf", Some(vec![make_param("inner")]));
    for i in 0..depth {
        let name = format!("level_{i}");
        current = make_sub_with_body(&name, Some(vec![MemberDecl::Sub(current)]));
    }
    current
}

#[test]
fn walker_terminates_at_max_depth() {
    // Pathologically deep chain — `MAX_MEMBER_NESTING_DEPTH * 2` levels
    // ensures we exceed the guard. The walker must NOT stack-overflow,
    // AND must respect `MAX_MEMBER_NESTING_DEPTH` precisely (one visit
    // at each depth in `0..=MAX_MEMBER_NESTING_DEPTH` before the next
    // recursion bails out).
    //
    // The chain produced by `build_nested_sub_chain(N)` is `N` outer Sub
    // levels wrapping a leaf Sub whose body holds a single Param. The
    // walker enters at depth 0 (visiting the outermost Sub's only child)
    // and recurses one level per inner Sub. With the chain deeper than
    // the guard, the leaf Param is unreachable: at every visited depth
    // `d ∈ 0..=MAX_MEMBER_NESTING_DEPTH` exactly one Sub child is
    // visited, then `walk_members_depth` is called at depth `d+1` and
    // returns immediately at `MAX_MEMBER_NESTING_DEPTH + 1`. Total =
    // `MAX_MEMBER_NESTING_DEPTH + 1`.
    let depth = MAX_MEMBER_NESTING_DEPTH * 2;
    let chain = build_nested_sub_chain(depth);

    let mut count = 0usize;
    walk_specialization_scope_members(&chain, &mut |_m| count += 1);

    let expected = MAX_MEMBER_NESTING_DEPTH + 1;
    assert_eq!(
        count, expected,
        "walker must visit exactly one member per allowed depth level \
         (0..=MAX_MEMBER_NESTING_DEPTH), giving {expected} visits"
    );
}

#[test]
fn walker_recurses_into_guarded_group_branches() {
    // SubDecl{body: Some([ GuardedGroup{ then=[Param("a")], else=[Constraint] } ])}
    // The walker must visit the GuardedGroup itself first, then recurse
    // into the `then` branch (Param), then the `else` branch (Constraint).
    let group = make_guarded_group(vec![make_param("a")], vec![make_constraint()]);
    let sub = make_sub_with_body("scope", Some(vec![group]));

    let mut tags = Vec::<Tag>::new();
    walk_specialization_scope_members(&sub, &mut |m| tags.push(tag_of(m)));

    assert_eq!(
        tags,
        vec![Tag::GuardedGroup, Tag::Param, Tag::Constraint],
        "walker must visit the GuardedGroup, then its `then` members, then its `else` members"
    );
}
