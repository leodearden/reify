use reify_types::SourceSpan;
use tower_lsp::lsp_types::Position;

/// Convert a byte offset in `source` to an LSP Position (line, character).
///
/// The character offset uses UTF-16 code units per LSP spec.
pub fn offset_to_position(source: &str, offset: u32) -> Position {
    let offset = offset as usize;
    let bytes = source.as_bytes();
    let clamped = offset.min(bytes.len());

    let mut line = 0u32;
    let mut line_start = 0usize;

    for i in 0..clamped {
        if bytes[i] == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }

    // Count UTF-16 code units from line_start to offset
    let line_slice = &source[line_start..clamped];
    let character: u32 = line_slice.chars().map(|c| c.len_utf16() as u32).sum();

    Position::new(line, character)
}

/// Convert a SourceSpan to an LSP Range.
pub fn span_to_range(source: &str, span: SourceSpan) -> tower_lsp::lsp_types::Range {
    tower_lsp::lsp_types::Range {
        start: offset_to_position(source, span.start),
        end: offset_to_position(source, span.end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn offset_zero_in_empty_string() {
        let pos = offset_to_position("", 0);
        assert_eq!(pos, Position::new(0, 0));
    }

    #[test]
    fn offset_within_first_line() {
        let source = "hello world";
        let pos = offset_to_position(source, 6);
        assert_eq!(pos, Position::new(0, 6));
    }

    #[test]
    fn offset_on_second_line() {
        let source = "first\nsecond";
        // "second" starts at byte 6, so offset 9 is character 3 on line 1
        let pos = offset_to_position(source, 9);
        assert_eq!(pos, Position::new(1, 3));
    }

    #[test]
    fn offset_at_newline_character() {
        let source = "abc\ndef";
        // offset 3 is the newline itself — end of line 0
        let pos = offset_to_position(source, 3);
        assert_eq!(pos, Position::new(0, 3));
    }

    #[test]
    fn multiline_bracket_source() {
        let source = reify_test_support::bracket_source();
        // The source starts with "structure Bracket {"
        // Line 0 offset 0 should be Position(0, 0)
        assert_eq!(offset_to_position(source, 0), Position::new(0, 0));

        // Find position of second line start
        let second_line_start = source.find('\n').unwrap() + 1;
        let pos = offset_to_position(source, second_line_start as u32);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn span_to_range_basic() {
        let source = "first\nsecond\nthird";
        let span = SourceSpan {
            start: 6,
            end: 12,
        }; // "second"
        let range = span_to_range(source, span);
        assert_eq!(range.start, Position::new(1, 0));
        assert_eq!(range.end, Position::new(1, 6));
    }
}
