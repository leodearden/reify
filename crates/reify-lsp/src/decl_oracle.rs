//! Syntax-only oracles over a parsed module's declarations: the module name a
//! URI implies, the declaration enclosing an offset, the name a declaration
//! declares and where its name token sits. Read by analysis, goto-def,
//! references and the document outline.

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_fixtures::{NAMED_DECL_SNIPPETS, parse_one_clean};
    use reify_core::ModulePath;

    // --- module_name_from_uri tests ---

    #[test]
    fn module_name_from_uri_extracts_test() {
        let uri = Url::parse("file:///test.ri").unwrap();
        assert_eq!(module_name_from_uri(&uri), "test");
    }

    #[test]
    fn module_name_from_uri_nested_path() {
        let uri = Url::parse("file:///path/to/bracket.ri").unwrap();
        assert_eq!(module_name_from_uri(&uri), "bracket");
    }

    #[test]
    fn module_name_from_uri_no_ri_suffix() {
        let uri = Url::parse("file:///test.txt").unwrap();
        assert_eq!(module_name_from_uri(&uri), "unnamed");
    }

    // --- enclosing_decl_at free function tests ---

    #[test]
    fn enclosing_decl_at_returns_structure_for_offset_inside() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside A: 'x' in "param x: Length = 5mm"
        let a_x_offset = source.find("param x").unwrap() + 6;
        let decl = enclosing_decl_at(&parsed.declarations, a_x_offset);
        assert!(decl.is_some(), "offset inside A should return Some");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "A"),
            other => panic!("expected Structure A, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_correct_structure_for_second_decl() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside B: 'y' in "param y: Bool = true"
        let b_y_offset = source.find("param y").unwrap() + 6;
        let decl = enclosing_decl_at(&parsed.declarations, b_y_offset);
        assert!(decl.is_some(), "offset inside B should return Some");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "B"),
            other => panic!("expected Structure B, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_occurrence() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let offset = source.find("diameter").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(
            decl.is_some(),
            "offset inside occurrence should return Some"
        );
        match decl.unwrap() {
            reify_ast::Declaration::Occurrence(o) => assert_eq!(o.name, "Joint"),
            other => panic!("expected Occurrence Joint, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_none_between_declarations() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset between A and B (the newline between them)
        let between_offset = source.find("\nstructure B").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, between_offset);
        assert!(
            decl.is_none(),
            "offset between declarations should return None"
        );
    }

    #[test]
    fn enclosing_decl_at_returns_none_for_offset_past_end() {
        let source = "structure A {\n    param x: Length = 5mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let decl = enclosing_decl_at(&parsed.declarations, source.len() + 100);
        assert!(decl.is_none(), "offset past end should return None");
    }

    #[test]
    fn enclosing_decl_at_empty_declarations() {
        let decl = enclosing_decl_at(&[], 0);
        assert!(decl.is_none(), "empty declarations should return None");
    }

    // --- decl_name_and_span free function tests (task 6388) ---

    #[test]
    fn decl_name_and_span_returns_name_and_span_for_every_named_kind() {
        for (source, expected_name) in NAMED_DECL_SNIPPETS {
            let parsed = parse_one_clean(source, "test");

            let got = decl_name_and_span(&parsed.declarations[0]);
            let (name, span) = got.unwrap_or_else(|| {
                panic!("decl_name_and_span returned None for named kind, source: {source}")
            });
            assert_eq!(name, *expected_name, "name mismatch for source: {source}");
            assert!(
                span.start < span.end,
                "span must be non-empty for source: {source}, got {span:?}"
            );
            let sliced = &source[span.start as usize..span.end as usize];
            assert!(
                sliced.contains(expected_name),
                "declaration span {span:?} sliced to {sliced:?} must contain \
                 {expected_name:?} for source: {source}"
            );
        }
    }

    #[test]
    fn decl_name_and_span_returns_none_for_unnamed_kinds() {
        // The three variants that declare no name of their own: Import binds a
        // path/entity, Module is a dotted path, Default binds an existing type.
        let unnamed = [
            "import parts.Hole",
            "module a.b.c",
            "default Material = steel",
        ];
        for source in unnamed {
            let parsed = parse_one_clean(source, "test");
            assert!(
                decl_name_and_span(&parsed.declarations[0]).is_none(),
                "unnamed declaration kind must yield None for source: {source}"
            );
        }
    }

    // --- name_token_span tests (step-1) ---
    //
    // `name_token_span(source, member_span, name)` narrows a member-statement
    // span down to the span of just the NAME identifier token. Expected offsets
    // are derived from `source.find` so the assertions stay fixture-robust.

    #[test]
    fn name_token_span_param_width_covers_just_the_name() {
        let source = "param width: Length = 80mm";
        let member_span = SourceSpan::new(0, source.len() as u32);
        let span = name_token_span(source, member_span, "width");
        let start = source.find("width").unwrap() as u32;
        let end = start + "width".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(&source[span.start as usize..span.end as usize], "width");
    }

    #[test]
    fn name_token_span_let_volume_covers_just_the_name() {
        let source = "let volume = width * height * thickness";
        let member_span = SourceSpan::new(0, source.len() as u32);
        let span = name_token_span(source, member_span, "volume");
        let start = source.find("volume").unwrap() as u32;
        let end = start + "volume".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(&source[span.start as usize..span.end as usize], "volume");
    }

    #[test]
    fn name_token_span_indented_guarded_param_covers_just_the_name() {
        // A guarded member's statement span begins at the indented `param`
        // keyword. name_token_span must still land on the identifier token,
        // searching forward from the (non-zero) member-span start.
        let source = "        param guarded_x : Length = 5mm";
        let decl_start = source.find("param").unwrap() as u32;
        let member_span = SourceSpan::new(decl_start, source.len() as u32);
        let span = name_token_span(source, member_span, "guarded_x");
        let start = source.find("guarded_x").unwrap() as u32;
        let end = start + "guarded_x".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "guarded_x"
        );
    }

    #[test]
    fn name_token_span_absent_name_falls_back_within_member_not_sibling() {
        // The member span covers ONLY the first statement (`param alpha …`), which
        // does NOT contain the name `beta`. `beta` appears only in the LATER
        // sibling statement. name_token_span must fall back to an empty span at the
        // member start rather than borrowing the sibling's `beta` token — a
        // malformed/recovered node whose span lacks its own name must not match a
        // same-named token elsewhere in the source.
        let source = "param alpha: Length = 1mm\nlet beta = alpha";
        let member_end = source.find('\n').unwrap() as u32; // end of `param alpha …`
        let member_span = SourceSpan::new(0, member_end);

        let span = name_token_span(source, member_span, "beta");
        assert_eq!(
            span,
            SourceSpan::empty(0),
            "name absent from the member span must fall back to empty, not borrow \
             the sibling `beta` token at {:?}",
            source.find("beta"),
        );
        assert!(span.is_empty(), "fallback span must be empty");
    }

    // --- name_token_span robustness: clamp / UTF-8-snap / start==len ---
    //
    // goto_def had its own name-token locator with a parallel set of these
    // tests; task 7529 collapsed the two locators into this one, so the
    // hardening is pinned here, at the primitive that performs it.

    #[test]
    fn name_token_span_start_beyond_source_len_falls_back_without_panic() {
        // A span start past the end of the source must not panic on the slice.
        let source = "structure Foo { }"; // 17 bytes
        let span = name_token_span(source, SourceSpan::new(100, 120), "Foo");
        assert_eq!(span, SourceSpan::empty(100));
        assert!(span.is_empty(), "out-of-range start must fall back to empty");
    }

    #[test]
    fn name_token_span_start_on_continuation_byte_snaps_forward() {
        // "aéb" = [0x61, 0xC3, 0xA9, 0x62]; byte 2 is the continuation byte 0xA9,
        // so the span start lands mid-character and must snap forward rather
        // than panic on a non-boundary slice.
        let source = "a\u{00E9}b Foo";
        assert!(!source.is_char_boundary(2), "fixture: byte 2 must be mid-char");
        let span = name_token_span(source, SourceSpan::new(2, source.len() as u32), "Foo");
        let start = source.find("Foo").unwrap() as u32;
        assert_eq!(span, SourceSpan::new(start, start + 3));
        assert_eq!(&source[span.start as usize..span.end as usize], "Foo");
    }

    #[test]
    fn name_token_span_start_exactly_source_len_falls_back_without_panic() {
        // An empty trailing slice must not panic.
        let source = "structure Foo { }";
        let len = source.len() as u32;
        let span = name_token_span(source, SourceSpan::new(len, len), "Foo");
        assert_eq!(span, SourceSpan::empty(len));
        assert!(span.is_empty(), "start == source.len() must fall back to empty");
    }
}
