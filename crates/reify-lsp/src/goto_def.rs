use reify_types::ModulePath;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::analysis::{find_named_member_span, module_name_from_uri};
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

/// Compute go-to-definition for the symbol at the given position.
///
/// Returns the `Location` of the symbol's declaration, or `None` if
/// the position is not on a navigable identifier (keywords, structure
/// names, and unknown words return `None`).
pub fn compute_goto_definition(source: &str, uri: &Url, position: Position) -> Option<Location> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    // Only needs ParsedModule for declaration spans (compiler discards them)
    let module_name = module_name_from_uri(uri);
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));

    // Search for a param or let declaration with matching name
    // (recursing into guarded groups via find_named_member_span)
    for decl in &parsed.declarations {
        let members = match decl {
            reify_syntax::Declaration::Structure(s) => &s.members,
            reify_syntax::Declaration::Occurrence(o) => &o.members,
            _ => continue,
        };
        if let Some((span, _doc)) = find_named_member_span(members, word) {
            return Some(Location {
                uri: uri.clone(),
                range: span_to_range(source, span),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    // --- step-7: go-to-definition tests ---

    #[test]
    fn goto_def_thickness_in_constraint_returns_param_location() {
        let source = reify_test_support::bracket_source();
        // 'thickness' in 'constraint thickness > 2mm' is on line 9
        let position = Position::new(9, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for thickness ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to the param declaration: "param thickness: Scalar = 5mm"
        // which starts at byte 79 (line 3, char 4)
        assert_eq!(loc.range.start.line, 3);
    }

    #[test]
    fn goto_def_width_in_constraint_expr_returns_param_location() {
        let source = reify_test_support::bracket_source();
        // 'width' in 'constraint thickness < width / 4' is on line 10
        // "    constraint thickness < width / 4"
        //                            ^-- char 30
        let position = Position::new(10, 30);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for width ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to param width on line 1
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn goto_def_volume_returns_let_location() {
        let source = reify_test_support::bracket_source();
        // 'volume' in "let volume = ..." on line 7
        let position = Position::new(7, 8);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for volume should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to itself (the let declaration) on line 7
        assert_eq!(loc.range.start.line, 7);
    }

    #[test]
    fn goto_def_occurrence_param_returns_location() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n    constraint diameter > 5mm\n}";
        // 'diameter' in the constraint is on line 2, col 15
        let position = Position::new(2, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for diameter ref in occurrence should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to param declaration on line 1
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn goto_def_keyword_returns_none() {
        let source = reify_test_support::bracket_source();
        // 'param' keyword on line 1
        let position = Position::new(1, 6);
        assert!(
            compute_goto_definition(source, &test_uri(), position).is_none(),
            "goto-def on keyword should return None"
        );
    }

    #[test]
    fn goto_def_structure_name_returns_none() {
        let source = reify_test_support::bracket_source();
        // 'Bracket' on line 0
        let position = Position::new(0, 12);
        assert!(
            compute_goto_definition(source, &test_uri(), position).is_none(),
            "goto-def on structure name should return None"
        );
    }

    // --- guarded group go-to-definition tests ---

    #[test]
    fn goto_def_param_inside_where_block() {
        // Source with guarded_x declared inside a where block,
        // referenced by let ref_x = guarded_x on line 5.
        let source = "structure S {\n    param cond : Bool = true\n    where cond {\n        param guarded_x : Scalar = 5mm\n    }\n    let ref_x = guarded_x\n}";
        // Line 5: "    let ref_x = guarded_x"
        //                          ^-- char 16 = start of 'guarded_x' reference
        let position = Position::new(5, 16);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for guarded_x ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to the param declaration on line 3:
        // "        param guarded_x : Scalar = 5mm"
        assert_eq!(loc.range.start.line, 3);
    }

    #[test]
    fn goto_def_let_inside_else_block() {
        // Source with fallback declared inside an else block,
        // referenced by let use_fb = fallback on line 7.
        let source = "structure S {\n    param cond : Bool = true\n    where cond {\n        param a : Scalar = 1mm\n    } else {\n        let fallback = 10\n    }\n    let use_fb = fallback\n}";
        // Line 7: "    let use_fb = fallback"
        //                           ^-- char 17 = start of 'fallback' reference
        let position = Position::new(7, 17);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for fallback ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to the let declaration on line 5:
        // "        let fallback = 10"
        assert_eq!(loc.range.start.line, 5);
    }

    // --- enclosing-declaration scoping tests ---

    #[test]
    fn goto_def_cursor_in_second_decl_scopes_to_enclosing() {
        // Two structures with identically-named param x.
        // Cursor on 'x' in B's `let y = x` should jump to B's param x, not A's.
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n    let y = x\n}";
        // Line 5: "    let y = x"
        //                      ^ col 12 = 'x' reference
        let position = Position::new(5, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x in B should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to B's param x on line 4, NOT A's on line 1
        assert_eq!(
            loc.range.start.line, 4,
            "expected B's param x (line 4), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_cursor_in_occurrence_scopes_to_enclosing() {
        // Structure A and occurrence B both have param diameter.
        // Cursor on 'diameter' in B's constraint should jump to B's param, not A's.
        let source = "structure A {\n    param diameter: Scalar = 10mm\n}\noccurrence def B {\n    param diameter: Scalar = 20mm\n    constraint diameter > 5mm\n}";
        // Line 5: "    constraint diameter > 5mm"
        //                        ^ col 15 = 'diameter' reference
        let position = Position::new(5, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for diameter in B should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to B's param diameter on line 4, NOT A's on line 1
        assert_eq!(
            loc.range.start.line, 4,
            "expected B's param diameter (line 4), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_unknown_word_returns_none() {
        let source = "structure Foo {\n  param x: Scalar = 5mm\n}";
        // Position past end of meaningful content
        let position = Position::new(0, 12); // on 'Foo'
        // 'Foo' is a structure name, should return None
        assert!(
            compute_goto_definition(source, &test_uri(), position).is_none(),
            "goto-def on structure name should return None"
        );
    }
}
