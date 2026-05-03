#[cfg(test)]
mod tests {
    use super::*;
    use reify_syntax::{Declaration, ExprKind, MemberDecl};
    use reify_types::ContentHash;

    #[test]
    fn dummy_span_returns_10_20() {
        let s = dummy_span();
        assert_eq!(s.start, 10);
        assert_eq!(s.end, 20);
    }

    #[test]
    fn param_span_returns_30_50() {
        let s = param_span();
        assert_eq!(s.start, 30);
        assert_eq!(s.end, 50);
    }

    #[test]
    fn port_span_returns_60_80() {
        let s = port_span();
        assert_eq!(s.start, 60);
        assert_eq!(s.end, 80);
    }

    #[test]
    fn sub_span_returns_90_110() {
        let s = sub_span();
        assert_eq!(s.start, 90);
        assert_eq!(s.end, 110);
    }

    #[test]
    fn zero_span_returns_0_0() {
        let s = zero_span();
        assert_eq!(s.start, 0);
        assert_eq!(s.end, 0);
    }

    #[test]
    fn dummy_hash_is_zero() {
        assert_eq!(dummy_hash(), ContentHash(0));
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
        let m = parsed_module_with_structure_members(members, dummy_span());
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
