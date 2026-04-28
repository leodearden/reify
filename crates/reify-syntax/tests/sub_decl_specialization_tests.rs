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

use reify_syntax::{
    Declaration, MemberDecl, ParamDecl, SubDecl, walk_specialization_scope_members,
    MAX_MEMBER_NESTING_DEPTH,
};
use reify_types::{ContentHash, ModulePath, SourceSpan};

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
        span: dummy_span(),
        content_hash: dummy_hash(),
    }
}
