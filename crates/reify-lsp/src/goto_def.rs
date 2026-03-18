use reify_types::ModulePath;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::analysis::module_name_from_uri;
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

/// Compute go-to-definition for the symbol at the given position.
///
/// Returns the `Location` of the symbol's declaration, or `None` if
/// the position is not on a navigable identifier (keywords, structure
/// names, and unknown words return `None`).
pub fn compute_goto_definition(
    source: &str,
    uri: &Url,
    position: Position,
) -> Option<Location> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    // Only needs ParsedModule for declaration spans (compiler discards them)
    let module_name = module_name_from_uri(uri);
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));

    // Search for a param or let declaration with matching name
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Structure(s) = decl {
            for member in &s.members {
                match member {
                    reify_syntax::MemberDecl::Param(p) if p.name == word => {
                        return Some(Location {
                            uri: uri.clone(),
                            range: span_to_range(source, p.span),
                        });
                    }
                    reify_syntax::MemberDecl::Let(l) if l.name == word => {
                        return Some(Location {
                            uri: uri.clone(),
                            range: span_to_range(source, l.span),
                        });
                    }
                    _ => {}
                }
            }
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
