//! Shared AST-builder fixtures for specialization-scope tests.
//!
//! These helpers are used by:
//! - `reify-compiler` unit tests in `specialization_scope_check.rs`
//! - `reify-compiler` integration tests in `specialization_scope_validation_tests.rs`
//! - `reify-lsp` regression tests in `diagnostics.rs`
//!
//! All helpers are `pub` and consumed via
//! `use reify_test_support::specialization_fixtures::*;` inside the relevant
//! test modules — **not** re-exported from the crate root to avoid polluting the
//! top-level namespace with generic names like `dummy_span`.
//!
//! # Span layout
//!
//! The span constants below cover byte offsets 0–110.  [`source_stub()`]
//! produces a 120-char string of ASCII spaces so that every offset maps to a
//! valid `LSP Position { line: 0, character: N }` without any out-of-bounds
//! access.

use reify_ast::{ConstraintDecl, Declaration, Expr, ExprKind, LetDecl, MemberDecl, ParamDecl, PortDecl, StructureDef, SubDecl};
use reify_core::{ContentHash, ModulePath, SourceSpan};

/// Dummy span for structure-level and default-use nodes: bytes 10–20.
pub fn dummy_span() -> SourceSpan {
    SourceSpan::new(10, 20)
}

/// Span for `param` declarations: bytes 30–50.
pub fn param_span() -> SourceSpan {
    SourceSpan::new(30, 50)
}

/// Span for `port` declarations: bytes 60–80.
pub fn port_span() -> SourceSpan {
    SourceSpan::new(60, 80)
}

/// Span for `sub` declarations: bytes 90–110.
pub fn sub_span() -> SourceSpan {
    SourceSpan::new(90, 110)
}

/// Zero span: bytes 0–0.  Used at **member-level** positions in the
/// compiler integration tests in
/// `tests/specialization_scope_validation_tests.rs` — sub-member spans,
/// param-member spans, `MatchArmDeclArmDecl` spans, the discriminant
/// `Expr` span, and `MatchArmDeclGroupDecl` spans — where exact byte
/// offsets don't matter for the diagnostic assertions under test.
///
/// The **structure-level** span in those tests comes from
/// [`parsed_module_with_structure_members`], which uses [`dummy_span()`].
pub fn zero_span() -> SourceSpan {
    SourceSpan::new(0, 0)
}

/// Dummy content hash: `ContentHash(0)`.
pub fn dummy_hash() -> ContentHash {
    ContentHash(0)
}

/// Dummy expression: `BoolLiteral(true)` at [`dummy_span()`].
pub fn dummy_expr() -> Expr {
    Expr {
        kind: ExprKind::BoolLiteral(true),
        span: dummy_span(),
    }
}

/// Build a `MemberDecl::Param` with the given `name` and `span`.
///
/// All optional fields (`doc`, `type_expr`, `default`, `where_clause`) are
/// `None`; `annotations` is empty; `content_hash` is [`dummy_hash()`].
pub fn make_param(name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Param(ParamDecl {
        name: name.to_string(),
        doc: None,
        is_priv: false,
        type_expr: None,
        default: None,
        where_clause: None,
        annotations: Vec::new(),
        span,
        content_hash: dummy_hash(),
    })
}

/// Build a `MemberDecl::Port` with the given `name` and `span`.
///
/// `direction` is `None`; `type_name` is `"SomePort"`; `members` and
/// `frame_expr` are empty/`None`; `content_hash` is [`dummy_hash()`].
pub fn make_port(name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Port(PortDecl {
        name: name.to_string(),
        direction: None,
        type_name: "SomePort".to_string(),
        is_priv: false,
        members: Vec::new(),
        frame_expr: None,
        span,
        content_hash: dummy_hash(),
    })
}

/// Build a bare (no-body) `MemberDecl::Sub` with the given `name` and `span`.
///
/// `structure_name` is `"Foo"`; `body` is `None`; `content_hash` is
/// [`dummy_hash()`].
pub fn make_sub_bare(name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: "Foo".to_string(),
        type_args: Vec::new(),
        args: Vec::new(),
        is_collection: false,
        where_clause: None,
        body: None,
        spec_param_overrides: Vec::new(),
        keyed_members: Vec::new(),
        is_aux: false,
        is_priv: false,
        pose_expr: None,
        relate_relations: Vec::new(),
        span,
        content_hash: dummy_hash(),
    })
}

/// Build a `MemberDecl::Sub` that opens a specialization scope with `body`.
///
/// `structure_name` is `"Foo"`; `body` is `Some(body)`; `content_hash` is
/// [`dummy_hash()`].
pub fn make_sub_with_body(name: &str, span: SourceSpan, body: Vec<MemberDecl>) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: "Foo".to_string(),
        type_args: Vec::new(),
        args: Vec::new(),
        is_collection: false,
        where_clause: None,
        body: Some(body),
        spec_param_overrides: Vec::new(),
        keyed_members: Vec::new(),
        is_aux: false,
        is_priv: false,
        pose_expr: None,
        relate_relations: Vec::new(),
        span,
        content_hash: dummy_hash(),
    })
}

/// Build a `MemberDecl::Let` with the given `name`.
///
/// Uses [`dummy_span()`] and [`dummy_expr()`] internally; `is_pub` is `false`;
/// optional fields are `None`/empty.
pub fn make_let(name: &str) -> MemberDecl {
    MemberDecl::Let(LetDecl {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        is_priv: false,
        is_aux: false,
        type_expr: None,
        value: dummy_expr(),
        where_clause: None,
        annotations: Vec::new(),
        span: dummy_span(),
        content_hash: dummy_hash(),
    })
}

/// Build a `MemberDecl::Constraint` using [`dummy_expr()`] and [`dummy_span()`].
///
/// `label` is `None`; `where_clause` is `None`.
pub fn make_constraint() -> MemberDecl {
    MemberDecl::Constraint(ConstraintDecl {
        is_priv: false,
        label: None,
        expr: dummy_expr(),
        where_clause: None,
        span: dummy_span(),
        content_hash: dummy_hash(),
    })
}

/// Build a [`reify_syntax::ParsedModule`] with a single `Structure` named `"S"`
/// whose top-level members are the supplied `members`.
///
/// The structure's span is fixed to [`dummy_span()`].  No specialization-scope
/// test asserts on the structure-level span itself (only on the inner member
/// spans), so a single default keeps every call site terse.
pub fn parsed_module_with_structure_members(
    members: Vec<MemberDecl>,
) -> reify_ast::ParsedModule {
    reify_ast::ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: Vec::new(),
            trait_bounds: Vec::new(),
            members,
            span: dummy_span(),
            content_hash: dummy_hash(),
            pragmas: Vec::new(),
            annotations: Vec::new(),
        })],
        errors: Vec::new(),
        content_hash: dummy_hash(),
        pragmas: Vec::new(),
        declared_module_path: None,
    }
}

/// A source stub long enough for all span offsets (up to [`sub_span()`]
/// end = 110) to produce non-zero LSP positions when passed to
/// `convert_diagnostic`.
///
/// All 120 characters are ASCII spaces, so byte offset `N` maps to
/// `LSP Position { line: 0, character: N }` for every offset used by the
/// span constants in this module.
pub fn source_stub() -> String {
    " ".repeat(120)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ast::{Declaration, ExprKind, MemberDecl};

    /// Guard that every span constant's `end` fits inside [`source_stub()`]'s
    /// buffer.  This is the invariant consumers depend on: byte offset N maps to
    /// a valid LSP `Position { line: 0, character: N }` for all N in any span.
    #[test]
    fn all_span_ends_fit_within_source_stub() {
        let len = source_stub().len() as u32;
        for (name, span) in [
            ("dummy_span", dummy_span()),
            ("param_span", param_span()),
            ("port_span", port_span()),
            ("sub_span", sub_span()),
        ] {
            assert!(
                span.end <= len,
                "{}.end ({}) exceeds source_stub len ({})",
                name,
                span.end,
                len,
            );
        }
    }

    #[test]
    fn dummy_expr_is_bool_literal_true_at_dummy_span() {
        let e = dummy_expr();
        assert!(matches!(e.kind, ExprKind::BoolLiteral(true)));
        assert_eq!(e.span, dummy_span());
    }

    #[test]
    fn make_param_returns_param_with_correct_fields() {
        let m = make_param("x", param_span());
        let MemberDecl::Param(p) = m else {
            panic!("expected MemberDecl::Param, got {:?}", m);
        };
        assert_eq!(p.name, "x");
        assert_eq!(p.span, param_span());
        assert_eq!(p.content_hash, dummy_hash());
        assert!(p.doc.is_none());
        assert!(p.type_expr.is_none());
        assert!(p.default.is_none());
        assert!(p.where_clause.is_none());
        assert!(p.annotations.is_empty());
    }

    #[test]
    fn make_port_returns_port_with_correct_fields() {
        let m = make_port("p", port_span());
        let MemberDecl::Port(p) = m else {
            panic!("expected MemberDecl::Port, got {:?}", m);
        };
        assert_eq!(p.name, "p");
        assert_eq!(p.span, port_span());
        assert_eq!(p.content_hash, dummy_hash());
        assert!(p.direction.is_none());
        assert_eq!(p.type_name, "SomePort");
        assert!(p.members.is_empty());
        assert!(p.frame_expr.is_none());
    }

    #[test]
    fn make_sub_bare_returns_sub_with_no_body() {
        let m = make_sub_bare("s", sub_span());
        let MemberDecl::Sub(s) = m else {
            panic!("expected MemberDecl::Sub, got {:?}", m);
        };
        assert_eq!(s.name, "s");
        assert_eq!(s.span, sub_span());
        assert_eq!(s.content_hash, dummy_hash());
        assert!(s.body.is_none());
        assert_eq!(s.structure_name, "Foo");
        assert!(s.type_args.is_empty());
        assert!(s.args.is_empty());
        assert!(!s.is_collection);
        assert!(s.where_clause.is_none());
    }

    #[test]
    fn make_sub_with_body_returns_sub_with_body() {
        let inner = make_param("inner", param_span());
        let m = make_sub_with_body("s", sub_span(), vec![inner]);
        let MemberDecl::Sub(s) = m else {
            panic!("expected MemberDecl::Sub, got {:?}", m);
        };
        assert_eq!(s.name, "s");
        assert_eq!(s.span, sub_span());
        let body = s.body.expect("body should be Some");
        assert_eq!(body.len(), 1);
        assert!(matches!(body[0], MemberDecl::Param(_)));
    }

    #[test]
    fn make_let_uses_dummy_span_internally() {
        let m = make_let("v");
        let MemberDecl::Let(l) = m else {
            panic!("expected MemberDecl::Let, got {:?}", m);
        };
        assert_eq!(l.name, "v");
        assert_eq!(l.span, dummy_span());
        assert_eq!(l.content_hash, dummy_hash());
        assert!(!l.is_pub);
        assert!(l.doc.is_none());
        assert!(l.type_expr.is_none());
        assert!(l.where_clause.is_none());
        assert!(l.annotations.is_empty());
    }

    #[test]
    fn make_constraint_uses_dummy_expr_and_dummy_span() {
        let m = make_constraint();
        let MemberDecl::Constraint(c) = m else {
            panic!("expected MemberDecl::Constraint, got {:?}", m);
        };
        assert_eq!(c.span, dummy_span());
        assert_eq!(c.content_hash, dummy_hash());
        assert!(c.label.is_none());
        assert!(matches!(c.expr.kind, ExprKind::BoolLiteral(true)));
        assert!(c.where_clause.is_none());
    }

    #[test]
    fn parsed_module_with_structure_members_returns_correct_shape() {
        let members = vec![make_param("x", param_span())];
        let m = parsed_module_with_structure_members(members);
        assert_eq!(m.declarations.len(), 1);
        assert!(m.errors.is_empty());
        assert!(m.pragmas.is_empty());
        let Declaration::Structure(s) = &m.declarations[0] else {
            panic!(
                "expected Declaration::Structure, got {:?}",
                m.declarations[0]
            );
        };
        assert_eq!(s.name, "S");
        assert_eq!(s.span, dummy_span());
        assert_eq!(s.members.len(), 1);
        assert!(s.pragmas.is_empty());
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn source_stub_is_120_ascii_spaces() {
        let s = source_stub();
        assert_eq!(s.len(), 120);
        assert!(s.chars().all(|c| c == ' '));
    }
}
