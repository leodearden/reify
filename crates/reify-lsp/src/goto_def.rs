use reify_ast::ImportKind;
use reify_core::ModulePath;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::analysis::{enclosing_decl_at, find_named_member_span, module_name_from_uri};
use crate::convert::{find_word_at_offset, offset_to_position, position_to_offset, span_to_range};

/// Compute go-to-definition for the symbol at the given position.
///
/// Returns the `Location` of the symbol's declaration, or `None` if
/// the position is not on a navigable identifier (keywords, structure
/// names, and unknown words return `None`).
pub fn compute_goto_definition(source: &str, uri: &Url, position: Position) -> Option<Location> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    // Only needs ParsedModule for declaration spans (compiler discards them).
    // Use prelude-aware parse for AST-shape consistency with the rest of
    // reify-lsp (diagnostics + analysis); see task 2525.
    let module_name = module_name_from_uri(uri);
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));

    // Try to find the enclosing declaration by checking if the cursor offset
    // falls within a declaration's span. If found, search only that declaration
    // first for scoped resolution.
    if let Some(enclosing) = enclosing_decl_at(&parsed.declarations, offset) {
        let members: &[_] = match enclosing {
            reify_ast::Declaration::Structure(s) => &s.members,
            reify_ast::Declaration::Occurrence(o) => &o.members,
            reify_ast::Declaration::Trait(t) => &t.members,
            reify_ast::Declaration::Purpose(p) => &p.members,
            _ => &[], // Variants without members (Import, Enum, Function, etc.)
        };
        if let Some(info) = find_named_member_span(members, word) {
            return Some(Location {
                uri: uri.clone(),
                range: span_to_range(source, info.span),
            });
        }
        // Member not found in enclosing declaration; fall through to
        // the all-declarations search below.
    }

    // Fallback: search all declarations (cursor outside any declaration,
    // or enclosing declaration didn't contain the member).
    for decl in &parsed.declarations {
        let members = match decl {
            reify_ast::Declaration::Structure(s) => &s.members,
            reify_ast::Declaration::Occurrence(o) => &o.members,
            reify_ast::Declaration::Trait(t) => &t.members,
            reify_ast::Declaration::Purpose(p) => &p.members,
            _ => continue,
        };
        if let Some(info) = find_named_member_span(members, word) {
            return Some(Location {
                uri: uri.clone(),
                range: span_to_range(source, info.span),
            });
        }
    }

    None
}

/// Compute go-to-definition with cross-file import resolution.
///
/// First tries single-file resolution (same logic as [`compute_goto_definition`]).
/// On failure, checks if the word matches an imported name and resolves it to
/// the target file using the provided resolver closure.
///
/// `resolve_import` maps an import dot-path (e.g., "parts") to
/// `(target_uri, target_source_text)`, or returns `None` if the module can't be found.
pub fn compute_goto_definition_cross_file(
    source: &str,
    uri: &Url,
    position: Position,
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<Location> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    let module_name = module_name_from_uri(uri);
    // Prelude-aware parse for AST-shape consistency across reify-lsp;
    // see task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));

    let offset_u32 = offset as u32;

    // Phase 0: Check if cursor is within an import statement's span.
    // This takes priority — when the cursor is on an import, navigate to the target.
    for decl in &parsed.declarations {
        if let reify_ast::Declaration::Import(import) = decl
            && offset_u32 >= import.span.start
            && offset_u32 < import.span.end
            && let Some((target_uri, target_source)) = resolve_import(&import.path)
        {
            // Determine what entity to look for in the target
            let entity_name = match &import.kind {
                ImportKind::Entity(name) => Some(name.as_str()),
                ImportKind::EntityAliased { entity, .. } => Some(entity.as_str()),
                ImportKind::Destructured(names) => {
                    // Find which name the cursor is on
                    names
                        .iter()
                        .find(|n| n.as_str() == word)
                        .map(|n| n.as_str())
                }
                ImportKind::Module | ImportKind::Aliased { .. } => None,
            };

            if let Some(name) = entity_name
                && let Some(loc) = find_declaration_in_source(&target_source, name, &target_uri)
            {
                return Some(loc);
            }
            // For module imports or unresolved entity, navigate to file start
            return Some(Location {
                uri: target_uri,
                range: Range::default(),
            });
        }
    }

    // Phase 1 + 1b: Single-file resolution (delegate to existing function).
    if let Some(loc) = compute_goto_definition(source, uri, position) {
        return Some(loc);
    }

    // Phase 2: Cross-file import resolution.
    // Check if the word matches an imported name.
    for decl in &parsed.declarations {
        if let reify_ast::Declaration::Import(import) = decl {
            let target_name = match &import.kind {
                ImportKind::Entity(name) if name == word => Some(name.as_str()),
                ImportKind::EntityAliased { entity, alias } if alias == word => {
                    Some(entity.as_str())
                }
                ImportKind::Destructured(names) => names
                    .iter()
                    .find(|n| n.as_str() == word)
                    .map(|n| n.as_str()),
                _ => None,
            };

            if let Some(target_entity) = target_name
                && let Some((target_uri, target_source)) = resolve_import(&import.path)
                && let Some(loc) =
                    find_declaration_in_source(&target_source, target_entity, &target_uri)
            {
                return Some(loc);
            }
        }
    }

    None
}

/// Find a top-level declaration by name in a source string and return its Location.
///
/// Searches Structure, Occurrence, Function, Enum, Trait, and Field declarations
/// for a matching name, returning the declaration's span as an LSP Location.
fn find_declaration_in_source(source: &str, name: &str, uri: &Url) -> Option<Location> {
    // Prelude-aware parse for AST-shape consistency across reify-lsp;
    // see task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("_target"));

    for decl in &parsed.declarations {
        let (decl_name, span) = match decl {
            reify_ast::Declaration::Structure(s) => (s.name.as_str(), s.span),
            reify_ast::Declaration::Occurrence(o) => (o.name.as_str(), o.span),
            reify_ast::Declaration::Function(f) => (f.name.as_str(), f.span),
            reify_ast::Declaration::Enum(e) => (e.name.as_str(), e.span),
            reify_ast::Declaration::Trait(t) => (t.name.as_str(), t.span),
            reify_ast::Declaration::Field(f) => (f.name.as_str(), f.span),
            _ => continue,
        };
        if decl_name == name {
            // Point to the name within the declaration, not the entire span.
            // Find the name's byte position within the declaration text.
            let name_offset = find_name_offset_in_decl(source, span.start, name);
            let start = offset_to_position(source, name_offset);
            let end = offset_to_position(source, name_offset + name.len() as u32);
            return Some(Location {
                uri: uri.clone(),
                range: Range { start, end },
            });
        }
    }
    None
}

/// Find the byte offset of a declaration's name within the source.
///
/// Searches from `decl_start` forward for the name string, returning its offset.
/// Falls back to `decl_start` if not found (shouldn't happen for valid declarations).
fn find_name_offset_in_decl(source: &str, decl_start: u32, name: &str) -> u32 {
    // Clamp to source length to prevent out-of-bounds panic.
    let mut start = (decl_start as usize).min(source.len());

    // Snap forward to the next valid UTF-8 character boundary if we
    // landed mid-character (e.g., on a continuation byte 0x80..0xBF).
    // This mirrors the pattern in offset_to_position (convert.rs:11-18).
    while start < source.len() && !source.is_char_boundary(start) {
        start += 1;
    }

    if let Some(rel_offset) = source[start..].find(name) {
        (start + rel_offset) as u32
    } else {
        decl_start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Test helper: return the UTF-16 code unit count of the Nth line of `source`.
    ///
    /// LSP `Position.character` is defined in UTF-16 code units
    /// (`PositionEncodingKind::UTF16`), matching the convention already used in
    /// `convert.rs` (`offset_to_position` / `position_to_offset`).
    ///
    /// Every declaration in these tests is single-line with `range.end`
    /// pinned to the end of that line. Computing the expected end from
    /// the source keeps assertions self-consistent if the declaration
    /// text ever changes (e.g. renaming a param or widening a literal),
    /// avoiding manual recompute of hard-coded character offsets.
    fn line_end_char(source: &str, line: u32) -> u32 {
        source
            .lines()
            .nth(line as usize)
            .expect("line index out of range in test source")
            .encode_utf16()
            .count() as u32
    }

    #[test]
    fn line_end_char_returns_utf16_units() {
        // Supplementary-plane emoji U+1F600 is 1 `char` but 2 UTF-16 code units.
        // "abc\u{1F600}" → 3 ASCII chars + 1 emoji = 4 chars but 5 UTF-16 code units.
        // LSP Position.character is defined in UTF-16 code units (PositionEncodingKind::UTF16),
        // so line_end_char must return 5, not 4.
        let source = "abc\u{1F600}";
        assert_eq!(
            line_end_char(source, 0),
            5,
            "line_end_char must return UTF-16 code unit count, not char count"
        );
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
        // Should point to the param declaration:
        // "    param thickness: Scalar = 5mm" (line 3)
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 3, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 3),
            "end should cover full 'param thickness: Scalar = 5mm'"
        );
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
        // Should point to param width on line 1:
        // "    param width: Scalar = 80mm"
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 1, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 1),
            "end should cover full 'param width: Scalar = 80mm'"
        );
    }

    #[test]
    fn goto_def_volume_returns_let_location() {
        let source = reify_test_support::bracket_source();
        // 'volume' in "let volume = ..." on line 7
        let position = Position::new(7, 8);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for volume should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to itself (the let declaration) on line 7:
        // "    let volume = width * height * thickness"
        assert_eq!(loc.range.start.line, 7);
        assert_eq!(
            loc.range.start.character, 4,
            "let keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 7, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 7),
            "end should cover full 'let volume = width * height * thickness'"
        );
    }

    #[test]
    fn goto_def_occurrence_param_returns_location() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n    constraint diameter > 5mm\n}";
        // 'diameter' in the constraint is on line 2, col 15
        let position = Position::new(2, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for diameter ref in occurrence should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to param declaration on line 1:
        // "    param diameter: Scalar = 10mm"
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 1, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 1),
            "end should cover full 'param diameter: Scalar = 10mm'"
        );
    }

    #[test]
    fn goto_def_occurrence_let_returns_location() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n    let radius = diameter / 2\n}";
        // 'radius' on line 2, col 8
        let position = Position::new(2, 8);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for let member in occurrence should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to let declaration on line 2:
        // "    let radius = diameter / 2"
        assert_eq!(loc.range.start.line, 2);
        assert_eq!(
            loc.range.start.character, 4,
            "let keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 2, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 2),
            "end should cover full 'let radius = diameter / 2'"
        );
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
        assert_eq!(
            loc.range.start.character, 8,
            "param keyword starts after 8-space indent"
        );
        // Assert range.end covers the full declaration line.
        assert_eq!(loc.range.end.line, 3, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 3),
            "end should cover full 'param guarded_x : Scalar = 5mm'"
        );
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
        assert_eq!(
            loc.range.start.character, 8,
            "let keyword starts after 8-space indent"
        );
        // Assert range.end covers the full declaration line.
        assert_eq!(loc.range.end.line, 5, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 5),
            "end should cover full 'let fallback = 10'"
        );
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
        // Should point to B's param x on line 4, NOT A's on line 1:
        // "    param x: Bool = true"
        assert_eq!(
            loc.range.start.line, 4,
            "expected B's param x (line 4), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 4, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 4),
            "end should cover full 'param x: Bool = true'"
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
        // Should point to B's param diameter on line 4, NOT A's on line 1:
        // "    param diameter: Scalar = 20mm"
        assert_eq!(
            loc.range.start.line, 4,
            "expected B's param diameter (line 4), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 4, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 4),
            "end should cover full 'param diameter: Scalar = 20mm'"
        );
    }

    #[test]
    fn goto_def_existing_single_decl_unchanged() {
        // Verify that all existing single-declaration goto_def behavior still
        // works after the enclosing-declaration scoping refactoring.
        let source = reify_test_support::bracket_source();
        // Test 1: thickness ref in constraint → param declaration
        let loc = compute_goto_definition(source, &test_uri(), Position::new(9, 15))
            .expect("thickness ref should resolve");
        assert_eq!(loc.range.start.line, 3);
        // Test 2: width ref in constraint expr → param declaration
        let loc = compute_goto_definition(source, &test_uri(), Position::new(10, 30))
            .expect("width ref should resolve");
        assert_eq!(loc.range.start.line, 1);
        // Test 3: volume let → itself
        let loc = compute_goto_definition(source, &test_uri(), Position::new(7, 8))
            .expect("volume should resolve");
        assert_eq!(loc.range.start.line, 7);
    }

    #[test]
    fn goto_def_cursor_in_first_decl_still_finds_own_member() {
        // When cursor is inside the first declaration, scoped search should
        // still find members (not accidentally skip them).
        let source = "structure A {\n    param x: Scalar = 5mm\n    let y = x\n}\nstructure B {\n    param x: Bool = true\n}";
        // Line 2: "    let y = x"
        //                      ^ col 12 = 'x' reference inside A
        let position = Position::new(2, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x in A should return location");
        // Should point to A's param x on line 1
        assert_eq!(
            loc.range.start.line, 1,
            "expected A's param x (line 1), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_enclosing_decl_member_not_found_falls_back() {
        // Cursor on 'y' in A's `let z = y`. Phase 1 finds enclosing A, but 'y'
        // is not a member of A → break. Phase 2 fallback searches all declarations
        // and finds 'y' as a param in B.
        let source = "structure A {\n    param x: Scalar = 5mm\n    let z = y\n}\nstructure B {\n    param y: Scalar = 20mm\n}";
        // Line 2: "    let z = y"
        //                      ^ col 12 = 'y' reference inside A's span
        let position = Position::new(2, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for y inside A should fall back and find y in B");
        assert_eq!(loc.uri, test_uri());
        // Should point to B's param y on line 5, proving Phase 2 fallback fired:
        // "    param y: Scalar = 20mm"
        assert_eq!(
            loc.range.start.line, 5,
            "expected B's param y (line 5), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 5, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 5),
            "end should cover full 'param y: Scalar = 20mm'"
        );
    }

    #[test]
    fn goto_def_cursor_outside_declarations_falls_back_to_first() {
        // Standalone 'x' between two declarations, outside both spans.
        // Phase 1 loop finds no enclosing declaration.
        // Phase 2 fallback searches all declarations and finds 'x' in A.
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nx\nstructure B {\n    param y: Scalar = 20mm\n}";
        // Line 3: "x" — standalone word between declarations
        //          ^ col 0
        let position = Position::new(3, 0);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x outside declarations should fall back and find x in A");
        assert_eq!(loc.uri, test_uri());
        // Should point to A's param x on line 1
        assert_eq!(
            loc.range.start.line, 1,
            "expected A's param x (line 1), got line {}",
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

    #[test]
    fn goto_def_cursor_inside_enum_decl_falls_through_to_global() {
        // Enum variant 'x' shares name with param x in structure S.
        // Cursor on 'x' inside enum span → enclosing_decl_at returns Enum,
        // _ => &[] gives empty members, falls through to phase-2 global search,
        // which finds param x in S.
        let source = "enum Foo { x }\nstructure S {\n    param x: Scalar = 5mm\n}";
        // Line 0: "enum Foo { x }"
        //                     ^ col 11 = 'x' variant
        let position = Position::new(0, 11);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x inside enum should fall through to S's param x");
        assert_eq!(loc.uri, test_uri());
        // Should point to S's param x on line 2
        assert_eq!(
            loc.range.start.line, 2,
            "expected S's param x (line 2), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_fallback_finds_trait_member() {
        // Cursor on 'mass' inside structure S, which has no member named 'mass'.
        // Phase 1 scoped lookup returns None (S has member 'y', not 'mass').
        // Phase 2 fallback should find the trait param mass.
        let source =
            "trait Rigid {\n    param mass: Scalar = 5mm\n}\nstructure S {\n    let y = mass\n}";
        // Line 4: "    let y = mass"
        //                      ^ col 12 = 'mass' reference
        let position = Position::new(4, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for mass in S should fall through to trait param");
        assert_eq!(loc.uri, test_uri());
        // Should point to Rigid's param mass on line 1
        assert_eq!(
            loc.range.start.line, 1,
            "expected trait's param mass (line 1), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_cursor_in_trait_scopes_to_enclosing() {
        // Structure A and trait T both have param x.
        // Cursor on 'x' in T's `let y = x` should jump to T's param x, not A's.
        let source = "structure A {\n    param x: Scalar = 5mm\n}\ntrait T {\n    param x: Scalar = 10mm\n    let y = x\n}";
        // Line 5: "    let y = x"
        //                      ^ col 12 = 'x' reference
        let position = Position::new(5, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x in trait T should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to T's param x on line 4, NOT A's on line 1:
        // "    param x: Scalar = 10mm"
        assert_eq!(
            loc.range.start.line, 4,
            "expected T's param x (line 4), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 4, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 4),
            "end should cover full 'param x: Scalar = 10mm'"
        );
    }

    // --- cross-file goto-definition tests ---

    fn parts_uri() -> Url {
        Url::parse("file:///project/parts.ri").unwrap()
    }

    /// Helper: build a mock resolver from a HashMap of (import_path -> (uri, source))
    fn mock_resolver(
        map: std::collections::HashMap<String, (Url, String)>,
    ) -> impl Fn(&str) -> Option<(Url, String)> {
        move |path: &str| map.get(path).cloned()
    }

    #[test]
    fn cross_file_entity_import_resolves_to_target_structure() {
        // Main source imports 'parts.Hole' and uses it as a sub-component type.
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        // Target file declares 'structure Hole { ... }'
        let target_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'sub hole = Hole' (line 2, col 16)
        let position = Position::new(2, 16);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve imported Hole");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Hole declaration on line 0"
        );
    }

    #[test]
    fn cross_file_destructured_import_resolves_to_target_structure() {
        // Main source imports 'parts.{Bolt, Nut}', cursor on 'Bolt' in a constraint.
        let source =
            "import parts.{Bolt, Nut}\nstructure Assembly {\n    sub b = Bolt\n    sub n = Nut\n}";
        // Target file has both structures
        let target_source = "structure Bolt {\n    param length: Scalar = 20mm\n}\nstructure Nut {\n    param size: Scalar = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Bolt' in 'sub b = Bolt' (line 2, col 12)
        let position = Position::new(2, 12);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve destructured import Bolt");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Bolt declaration on line 0"
        );
    }

    #[test]
    fn cross_file_aliased_entity_import_resolves_to_original_name() {
        // Main source imports 'parts.Bolt as StdBolt', cursor on 'StdBolt' in code.
        let source = "import parts.Bolt as StdBolt\nstructure Assembly {\n    sub b = StdBolt\n}";
        // Target file has 'structure Bolt { ... }' (original name)
        let target_source = "structure Bolt {\n    param length: Scalar = 20mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'StdBolt' in 'sub b = StdBolt' (line 2, col 12)
        let position = Position::new(2, 12);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve aliased import StdBolt -> Bolt");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Bolt (original name) on line 0"
        );
    }

    #[test]
    fn cross_file_function_import_resolves_to_fn_declaration() {
        // Main source imports 'math.Sqrt' (entity import, uppercase) and uses it.
        // In Reify, entity imports use uppercase first letter per convention.
        let source = "import math.Sqrt\nstructure Circle {\n    param r: Scalar = 5mm\n    let d = Sqrt(r)\n}";
        // Target file declares 'fn Sqrt(...)'
        let target_source = "fn Sqrt(x: Scalar) -> Scalar {\n    x\n}";
        let math_uri = Url::parse("file:///project/math.ri").unwrap();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "math".to_string(),
            (math_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Sqrt' in 'let d = Sqrt(r)' (line 3, col 12)
        let position = Position::new(3, 12);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve imported fn Sqrt");
        assert_eq!(loc.uri, math_uri, "should point to the math target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to fn Sqrt declaration on line 0"
        );
    }

    #[test]
    fn cross_file_cursor_on_import_entity_navigates_to_target() {
        // Cursor on 'Hole' within 'import parts.Hole' (on the import statement itself)
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'import parts.Hole' (line 0, col 13)
        let position = Position::new(0, 13);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cursor on import entity name should navigate to target declaration");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Hole in the target file"
        );
    }

    #[test]
    fn cross_file_cursor_on_import_path_navigates_to_target() {
        // Cursor on 'parts' within 'import parts.Hole'
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'parts' in 'import parts.Hole' (line 0, col 8)
        let position = Position::new(0, 8);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cursor on import path should navigate to target");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        // For entity import with cursor on path, navigate to the entity declaration
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Hole declaration"
        );
    }

    #[test]
    fn cross_file_cursor_on_module_import_navigates_to_file_start() {
        // Module import: 'import utils' (no entity)
        let source = "import utils\nstructure S {\n    param x: Scalar = 1mm\n}";
        let target_source = "structure Helper {\n    param y: Scalar = 2mm\n}";
        let utils_uri = Url::parse("file:///project/utils.ri").unwrap();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "utils".to_string(),
            (utils_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'utils' in 'import utils' (line 0, col 8)
        let position = Position::new(0, 8);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cursor on module import should navigate to file start");
        assert_eq!(loc.uri, utils_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "module import should navigate to file start"
        );
        assert_eq!(loc.range.start.character, 0);
    }

    // --- verification / regression tests ---

    #[test]
    fn cross_file_sub_component_type_resolves_through_entity_import() {
        // Verify: sub-component type 'sub hole = Hole' resolves through entity import.
        // (This overlaps step-1 but explicitly verifies the sub-component pattern.)
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'sub hole = Hole' (line 2, col 16)
        let position = Position::new(2, 16);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("sub-component type should resolve through entity import");
        assert_eq!(loc.uri, target_uri);
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn cross_file_unresolvable_import_returns_none_without_panic() {
        // Verify: when the resolver returns None, we get None back with no panic.
        let source = "import nonexistent.Foo\nstructure S {\n    sub f = Foo\n}";
        let resolver = |_: &str| -> Option<(Url, String)> { None };

        // Cursor on 'Foo' in 'sub f = Foo'
        let position = Position::new(2, 12);
        let result = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver);
        assert!(
            result.is_none(),
            "unresolvable import should return None, not panic"
        );
    }

    // --- find_name_offset_in_decl robustness tests (step-16: panic_on_invalid_span) ---

    #[test]
    fn find_name_offset_decl_start_exceeds_source_len() {
        // (a) decl_start=100 but source is only 20 bytes — must not panic.
        let source = "structure Foo { }"; // 17 bytes
        let result = find_name_offset_in_decl(source, 100, "Foo");
        // Should fall back to decl_start since the slice is invalid
        assert_eq!(result, 100);
    }

    #[test]
    fn find_name_offset_decl_start_on_continuation_byte() {
        // (b) decl_start lands on a UTF-8 continuation byte — must not panic.
        // "aéb" = [0x61, 0xC3, 0xA9, 0x62], byte 2 is continuation byte 0xA9
        let source = "a\u{00E9}b Foo";
        // source = [0x61, 0xC3, 0xA9, 0x62, 0x20, 0x46, 0x6F, 0x6F]
        //           a     é(1)  é(2)  b     ' '   F     o     o
        // decl_start=2 is the continuation byte
        let result = find_name_offset_in_decl(source, 2, "Foo");
        // Should snap forward and find "Foo" at byte 5
        assert_eq!(result, 5);
    }

    #[test]
    fn find_name_offset_decl_start_exactly_source_len() {
        // (c) decl_start is exactly source.len() (empty trailing slice) — must not panic.
        let source = "structure Foo { }";
        let len = source.len() as u32; // 17
        let result = find_name_offset_in_decl(source, len, "Foo");
        // Should fall back to decl_start since there's nothing after
        assert_eq!(result, len);
    }

    #[test]
    fn find_declaration_in_source_with_multibyte_before_decl() {
        // End-to-end: target source has multi-byte chars before a structure declaration.
        // Parser should still find the declaration, and find_name_offset_in_decl
        // should handle any tricky offsets gracefully.
        let target_source =
            "// comment with é accent\nstructure Widget {\n    param size: Scalar = 5mm\n}";
        let target_uri = Url::parse("file:///target.ri").unwrap();
        let result = find_declaration_in_source(target_source, "Widget", &target_uri);
        assert!(
            result.is_some(),
            "should find Widget declaration despite multi-byte chars"
        );
        let loc = result.unwrap();
        assert_eq!(loc.uri, target_uri);
        // Widget declaration is on line 1
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn cross_file_single_file_behavior_unchanged_with_resolver() {
        // Verify: existing single-file goto-def behavior is unchanged when
        // a cross-file resolver is present.
        let source = reify_test_support::bracket_source();
        let resolver = |_: &str| -> Option<(Url, String)> { None };

        // Test 1: thickness ref in constraint → param declaration
        let loc = compute_goto_definition_cross_file(
            source,
            &test_uri(),
            Position::new(9, 15),
            &resolver,
        )
        .expect("thickness ref should still resolve in single-file mode");
        assert_eq!(loc.range.start.line, 3);

        // Test 2: width ref in constraint expr → param declaration
        let loc = compute_goto_definition_cross_file(
            source,
            &test_uri(),
            Position::new(10, 30),
            &resolver,
        )
        .expect("width ref should still resolve in single-file mode");
        assert_eq!(loc.range.start.line, 1);

        // Test 3: volume let → itself
        let loc =
            compute_goto_definition_cross_file(source, &test_uri(), Position::new(7, 8), &resolver)
                .expect("volume should still resolve in single-file mode");
        assert_eq!(loc.range.start.line, 7);
    }

    // --- enclosing_decl_at integration regression tests ---

    #[test]
    fn enclosing_decl_at_integration_scoped_member_in_second_decl() {
        // Verify that using enclosing_decl_at from goto_def's context
        // correctly identifies the enclosing declaration for scoped member resolution.
        use crate::analysis::enclosing_decl_at;
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n    let y = x\n}";
        let uri = test_uri();
        let module_name = crate::analysis::module_name_from_uri(&uri);
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module_name));

        // Offset inside B's 'let y = x'
        let offset = source.find("let y").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(decl.is_some(), "offset inside B should find enclosing decl");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => {
                assert_eq!(s.name, "B", "enclosing decl should be B");
                // Verify we can extract members from the returned declaration
                assert!(!s.members.is_empty(), "B should have members");
            }
            other => panic!("expected Structure B, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_integration_cursor_outside_returns_none() {
        // Cursor outside all declarations — enclosing_decl_at should return None,
        // and goto_def should fall back to searching all declarations.
        use crate::analysis::enclosing_decl_at;
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nx\nstructure B {\n    param y: Scalar = 20mm\n}";
        let uri = test_uri();
        let module_name = crate::analysis::module_name_from_uri(&uri);
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module_name));

        // Offset on 'x' standalone between declarations
        let offset = source.find("\nx\n").unwrap() + 1;
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(
            decl.is_none(),
            "offset outside declarations should return None"
        );

        // But goto_def should still find 'x' via fallback (line 3 is the standalone 'x')
        let loc = compute_goto_definition(source, &test_uri(), Position::new(3, 0))
            .expect("goto-def should fall back to find x in A");
        assert_eq!(loc.range.start.line, 1, "should point to A's param x");
    }

    #[test]
    fn enclosing_decl_at_integration_missing_member_falls_back() {
        // Cursor in A on 'y', which doesn't exist in A but exists in B.
        // enclosing_decl_at finds A, but member not found → falls back.
        use crate::analysis::enclosing_decl_at;
        let source = "structure A {\n    param x: Scalar = 5mm\n    let z = y\n}\nstructure B {\n    param y: Scalar = 20mm\n}";
        let uri = test_uri();
        let module_name = crate::analysis::module_name_from_uri(&uri);
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module_name));

        // Offset inside A
        let offset = source.find("let z").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(decl.is_some());
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "A"),
            _ => panic!("expected A"),
        }

        // goto_def should fall through and find y in B
        let loc = compute_goto_definition(source, &test_uri(), Position::new(2, 12))
            .expect("goto-def for y should fall back to B");
        assert_eq!(loc.range.start.line, 5, "should find y in B via fallback");
    }

    // --- step-09: injectable parsed cores over a shared ParsedModule ---

    /// `compute_goto_definition_with_parsed`, fed a `ParsedModule` built once by
    /// the caller, must return the same `Location` as the
    /// `compute_goto_definition` wrapper (which parses internally) for an
    /// in-document member reference — proving the cache-fed core is
    /// output-equivalent to the per-request path.
    #[test]
    fn compute_goto_definition_with_parsed_matches_wrapper() {
        let source = reify_test_support::bracket_source();
        let uri = test_uri();
        // 'thickness' in 'constraint thickness > 2mm' (line 9) → param on line 3.
        let position = Position::new(9, 15);

        let parsed = reify_compiler::parse_with_stdlib(
            source,
            reify_core::ModulePath::single(crate::analysis::module_name_from_uri(&uri)),
        );

        let via_parsed = compute_goto_definition_with_parsed(&parsed, source, &uri, position);
        let via_wrapper = compute_goto_definition(source, &uri, position);

        assert!(
            via_parsed.is_some(),
            "with-parsed goto-def should resolve the thickness reference"
        );
        assert_eq!(
            via_parsed, via_wrapper,
            "with-parsed goto-def must match the wrapper output"
        );
    }

    /// `compute_goto_definition_cross_file_with_parsed`, fed a `ParsedModule`
    /// built once by the caller plus an import resolver, must return the same
    /// `Location` as the `compute_goto_definition_cross_file` wrapper (which
    /// parses internally) for an imported-entity reference.
    #[test]
    fn compute_goto_definition_cross_file_with_parsed_matches_wrapper() {
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        let uri = test_uri();
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'sub hole = Hole' (line 2, col 16).
        let position = Position::new(2, 16);

        let parsed = reify_compiler::parse_with_stdlib(
            source,
            reify_core::ModulePath::single(crate::analysis::module_name_from_uri(&uri)),
        );

        let via_parsed = compute_goto_definition_cross_file_with_parsed(
            &parsed, source, &uri, position, &resolver,
        );
        let via_wrapper = compute_goto_definition_cross_file(source, &uri, position, &resolver);

        assert!(
            via_parsed.is_some(),
            "cross-file with-parsed goto-def should resolve the imported Hole"
        );
        assert_eq!(
            via_parsed, via_wrapper,
            "cross-file with-parsed goto-def must match the wrapper output"
        );
    }
}
