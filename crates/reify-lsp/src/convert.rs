use reify_syntax::ParseError;
use reify_types::{Diagnostic, Severity, SourceSpan};
use tower_lsp::lsp_types::{self, DiagnosticRelatedInformation, DiagnosticSeverity, Position, Url};

/// Convert a byte offset in `source` to an LSP Position (line, character).
///
/// The character offset uses UTF-16 code units per LSP spec.
pub fn offset_to_position(source: &str, offset: u32) -> Position {
    let offset = offset as usize;
    let bytes = source.as_bytes();
    let mut clamped = offset.min(bytes.len());

    // Snap forward to the next valid UTF-8 character boundary if we
    // landed mid-character (e.g., on a continuation byte 0x80..0xBF).
    // Tree-sitter error-recovery spans routinely produce such offsets.
    while clamped < bytes.len() && !source.is_char_boundary(clamped) {
        clamped += 1;
    }

    let mut line = 0u32;
    let mut line_start = 0usize;

    for (i, &byte) in bytes.iter().enumerate().take(clamped) {
        if byte == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }

    // Count UTF-16 code units from line_start to offset.
    // line_start is always at a char boundary (after '\n' which is a
    // single-byte ASCII character), and clamped is now snapped to a
    // char boundary, so this slice is always valid UTF-8.
    let line_slice = &source[line_start..clamped];
    let character: u32 = line_slice.chars().map(|c| c.len_utf16() as u32).sum();

    Position::new(line, character)
}

/// Convert an LSP Position (line, character) to a byte offset in `source`.
///
/// The character offset uses UTF-16 code units per LSP spec.
/// Returns `source.len()` if the position is past the end.
pub fn position_to_offset(source: &str, position: Position) -> usize {
    let target_line = position.line as usize;
    let target_char = position.character as usize;

    let mut current_line = 0usize;
    let mut byte_offset = 0usize;

    // Advance to the target line
    if target_line > 0 {
        for (i, &byte) in source.as_bytes().iter().enumerate() {
            if byte == b'\n' {
                current_line += 1;
                if current_line == target_line {
                    byte_offset = i + 1;
                    break;
                }
            }
        }
        // If target line not found, return end
        if current_line < target_line {
            return source.len();
        }
    }

    // Now advance target_char UTF-16 code units within the line
    let line_slice = &source[byte_offset..];
    let mut utf16_units = 0usize;
    for (i, ch) in line_slice.char_indices() {
        if ch == '\n' || utf16_units >= target_char {
            return byte_offset + i;
        }
        utf16_units += ch.len_utf16();
    }

    // Past end of line/source
    source.len()
}

/// Extract the identifier word at the given byte offset.
///
/// Returns `Some((start, word))` where `start` is the byte offset of the word's
/// first character, or `None` if the offset doesn't point at an identifier character.
/// Identifier characters: alphanumeric or underscore.
pub fn find_word_at_offset(source: &str, offset: usize) -> Option<(usize, &str)> {
    if offset >= source.len() {
        return None;
    }

    let bytes = source.as_bytes();

    // Check if the byte at offset is an identifier character
    if !is_ident_byte(bytes[offset]) {
        return None;
    }

    // Scan backward to find word start
    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

    // Scan forward to find word end
    let mut end = offset + 1;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }

    Some((start, &source[start..end]))
}

/// Check if a byte is an identifier character (alphanumeric or underscore).
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Convert a SourceSpan to an LSP Range.
pub fn span_to_range(source: &str, span: SourceSpan) -> tower_lsp::lsp_types::Range {
    tower_lsp::lsp_types::Range {
        start: offset_to_position(source, span.start),
        end: offset_to_position(source, span.end),
    }
}

/// Convert a Reify Severity to an LSP DiagnosticSeverity.
pub fn convert_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
    }
}

/// Convert a Reify Diagnostic to an LSP Diagnostic. The `code` field, when
/// present, is rendered as a PascalCase string identifier matching the serde
/// wire form of `DiagnosticCode`.
pub fn convert_diagnostic(diag: &Diagnostic, source: &str, uri: &Url) -> lsp_types::Diagnostic {
    let range = if let Some(first_label) = diag.labels.first() {
        span_to_range(source, first_label.span)
    } else {
        lsp_types::Range {
            start: Position::new(0, 0),
            end: Position::new(0, 0),
        }
    };

    let related_information = if diag.labels.len() > 1 {
        Some(
            diag.labels[1..]
                .iter()
                .map(|label| DiagnosticRelatedInformation {
                    location: lsp_types::Location {
                        uri: uri.clone(),
                        range: span_to_range(source, label.span),
                    },
                    message: label.message.clone(),
                })
                .collect(),
        )
    } else {
        None
    };

    // NOTE: route through serde so future field-bearing DiagnosticCode variants don't
    // leak Debug-style "Variant(...)" strings to the wire; as_str() returns None for
    // non-string JSON values, leaving the code field absent (safe degradation).
    let code = diag
        .code
        .and_then(|c| serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_owned)))
        .map(lsp_types::NumberOrString::String);

    lsp_types::Diagnostic {
        range,
        severity: Some(convert_severity(diag.severity)),
        code,
        message: diag.message.clone(),
        source: Some("reify".to_string()),
        related_information,
        ..Default::default()
    }
}

/// Convert a ParseError to an LSP Diagnostic.
pub fn convert_parse_error(err: &ParseError, source: &str, _uri: &Url) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: span_to_range(source, err.span),
        severity: Some(DiagnosticSeverity::ERROR),
        message: err.message.clone(),
        source: Some("reify".to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{DiagnosticCode, DiagnosticLabel};
    use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString, Position, Url};

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
        let span = SourceSpan { start: 6, end: 12 }; // "second"
        let range = span_to_range(source, span);
        assert_eq!(range.start, Position::new(1, 0));
        assert_eq!(range.end, Position::new(1, 6));
    }

    // --- Diagnostic conversion tests ---

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    #[test]
    fn severity_error_maps_to_lsp_error() {
        assert_eq!(convert_severity(Severity::Error), DiagnosticSeverity::ERROR);
    }

    #[test]
    fn severity_warning_maps_to_lsp_warning() {
        assert_eq!(
            convert_severity(Severity::Warning),
            DiagnosticSeverity::WARNING
        );
    }

    #[test]
    fn severity_info_maps_to_lsp_information() {
        assert_eq!(
            convert_severity(Severity::Info),
            DiagnosticSeverity::INFORMATION
        );
    }

    #[test]
    fn diagnostic_no_labels_range_at_origin() {
        let source = "hello\nworld";
        let diag = Diagnostic::error("something went wrong");
        let lsp_diag = convert_diagnostic(&diag, source, &test_uri());
        assert_eq!(lsp_diag.range.start, Position::new(0, 0));
        assert_eq!(lsp_diag.range.end, Position::new(0, 0));
        assert_eq!(lsp_diag.message, "something went wrong");
    }

    #[test]
    fn diagnostic_one_label_uses_label_span() {
        let source = "hello\nworld";
        let diag = Diagnostic::error("bad token").with_label(DiagnosticLabel::new(
            SourceSpan::new(6, 11), // "world"
            "here",
        ));
        let lsp_diag = convert_diagnostic(&diag, source, &test_uri());
        assert_eq!(lsp_diag.range.start, Position::new(1, 0));
        assert_eq!(lsp_diag.range.end, Position::new(1, 5));
    }

    #[test]
    fn diagnostic_multiple_labels_primary_and_related() {
        let source = "aaa\nbbb\nccc";
        let diag = Diagnostic::error("conflict")
            .with_label(DiagnosticLabel::new(SourceSpan::new(0, 3), "primary"))
            .with_label(DiagnosticLabel::new(SourceSpan::new(4, 7), "related"));
        let lsp_diag = convert_diagnostic(&diag, source, &test_uri());
        // Primary range from first label
        assert_eq!(lsp_diag.range.start, Position::new(0, 0));
        assert_eq!(lsp_diag.range.end, Position::new(0, 3));
        // Related information from second label
        let related = lsp_diag.related_information.unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, "related");
    }

    #[test]
    fn convert_diagnostic_populates_string_code_for_typed_code() {
        let source = "structure S {\n    param x : Real = 1\n    let f = |x| x * 2\n}\n";
        let diag = Diagnostic::warning("declaration of 'x' shadows enclosing declaration")
            .with_code(DiagnosticCode::Shadowing)
            .with_label(DiagnosticLabel::new(SourceSpan::new(0, 1), "child"))
            .with_label(DiagnosticLabel::new(SourceSpan::new(2, 3), "parent"));
        let lsp_diag = convert_diagnostic(&diag, source, &test_uri());
        assert_eq!(
            lsp_diag.code,
            Some(NumberOrString::String("Shadowing".to_string())),
            "convert_diagnostic must populate code from DiagnosticCode::Shadowing"
        );
    }

    #[test]
    fn convert_diagnostic_leaves_code_none_when_input_code_absent() {
        let source = "anything";
        let diag = Diagnostic::error("some error");
        let lsp_diag = convert_diagnostic(&diag, source, &test_uri());
        assert_eq!(
            lsp_diag.code, None,
            "convert_diagnostic must leave code as None when no DiagnosticCode is attached"
        );
    }

    /// Locks the `convert_diagnostic` code-field conversion for a representative
    /// spread of `DiagnosticCode` variants. The LSP `code` field renders
    /// `DiagnosticCode` via the serde `rename_all = "PascalCase"` wire form.
    ///
    /// Each iteration asserts two independent conditions:
    /// 1. `serde_json::to_value(&code)` produces the hard-coded PascalCase literal.
    /// 2. `convert_diagnostic` produces the same literal as the LSP `code` field.
    ///
    /// Double-binding to a fixed literal catches drift in either direction: if serde
    /// and the conversion diverge, or if either independently drifts from PascalCase.
    #[test]
    fn convert_diagnostic_code_wire_str_matches_pascal_case_for_representative_variants() {
        let cases: &[(DiagnosticCode, &str)] = &[
            (DiagnosticCode::TraitNotImplemented, "TraitNotImplemented"),
            (DiagnosticCode::DimensionMismatch, "DimensionMismatch"),
            (DiagnosticCode::DeepDotChain, "DeepDotChain"),
            (DiagnosticCode::Shadowing, "Shadowing"),
        ];
        for &(code, expected_wire) in cases {
            assert_eq!(
                serde_json::to_value(&code).unwrap().as_str().unwrap(),
                expected_wire,
                "serde wire form for DiagnosticCode::{code:?} must equal {expected_wire:?}"
            );
            let diag = Diagnostic::warning("test").with_code(code);
            let lsp = convert_diagnostic(&diag, "", &test_uri());
            assert_eq!(
                lsp.code,
                Some(NumberOrString::String(expected_wire.to_string())),
                "DiagnosticCode::{code:?} should convert to wire string {expected_wire:?}"
            );
        }
    }

    // --- UTF-8 boundary safety tests (step-21) ---

    #[test]
    fn offset_mid_multibyte_2byte() {
        // 'é' is U+00E9, encoded as 2 bytes: [0xC3, 0xA9]
        // "aéb" = [0x61, 0xC3, 0xA9, 0x62]
        let source = "a\u{00E9}b";
        assert_eq!(source.len(), 4);
        // Byte 2 is the continuation byte 0xA9 — mid-character
        let pos = offset_to_position(source, 2);
        // Must not panic; should snap forward to byte 3 (start of 'b')
        assert_eq!(pos, Position::new(0, 2)); // 'a' (1 UTF-16 unit) + 'é' (1 UTF-16 unit) = col 2
    }

    #[test]
    fn offset_mid_multibyte_3byte() {
        // '世' is U+4E16, encoded as 3 bytes: [0xE4, 0xB8, 0x96]
        // "a世b" = [0x61, 0xE4, 0xB8, 0x96, 0x62]
        let source = "a\u{4E16}b";
        assert_eq!(source.len(), 5);
        // Byte 2 is mid-character
        let pos2 = offset_to_position(source, 2);
        assert_eq!(pos2, Position::new(0, 2)); // snap to after '世'
        // Byte 3 is also mid-character
        let pos3 = offset_to_position(source, 3);
        assert_eq!(pos3, Position::new(0, 2)); // snap to after '世'
    }

    #[test]
    fn offset_mid_multibyte_4byte() {
        // '😀' is U+1F600, encoded as 4 bytes: [0xF0, 0x9F, 0x98, 0x80]
        // "a😀b" = [0x61, 0xF0, 0x9F, 0x98, 0x80, 0x62]
        let source = "a\u{1F600}b";
        assert_eq!(source.len(), 6);
        // Bytes 2, 3, 4 are mid-character continuation bytes
        let pos2 = offset_to_position(source, 2);
        assert_eq!(pos2, Position::new(0, 3)); // snap to after '😀' which is 2 UTF-16 units: col = 1 + 2 = 3
        let pos3 = offset_to_position(source, 3);
        assert_eq!(pos3, Position::new(0, 3));
        let pos4 = offset_to_position(source, 4);
        assert_eq!(pos4, Position::new(0, 3));
    }

    #[test]
    fn offset_at_exact_char_boundary() {
        // "aéb" = [0x61, 0xC3, 0xA9, 0x62]
        // Valid boundaries: 0 (a), 1 (é start), 3 (b), 4 (end)
        let source = "a\u{00E9}b";
        assert_eq!(offset_to_position(source, 0), Position::new(0, 0)); // before 'a'
        assert_eq!(offset_to_position(source, 1), Position::new(0, 1)); // before 'é'
        assert_eq!(offset_to_position(source, 3), Position::new(0, 2)); // before 'b'
        assert_eq!(offset_to_position(source, 4), Position::new(0, 3)); // end
    }

    #[test]
    fn offset_mid_char_in_second_line() {
        // "x\né y" = [0x78, 0x0A, 0xC3, 0xA9, 0x79]
        let source = "x\n\u{00E9}y";
        assert_eq!(source.len(), 5);
        // Byte 3 is continuation byte of 'é' on line 1
        let pos = offset_to_position(source, 3);
        assert_eq!(pos.line, 1);
        // Should snap forward to byte 4 ('y'), so character = 1 (é) = col 1
        assert_eq!(pos, Position::new(1, 1));
    }

    #[test]
    fn span_to_range_mid_multibyte() {
        // "aéb" = [0x61, 0xC3, 0xA9, 0x62]
        let source = "a\u{00E9}b";
        // Span where start lands mid-character (byte 2 is continuation byte)
        let span = SourceSpan { start: 2, end: 4 };
        let range = span_to_range(source, span);
        // Must not panic. Start snaps to byte 3 (after 'é'), end is at end
        assert_eq!(range.start, Position::new(0, 2));
        assert_eq!(range.end, Position::new(0, 3));
    }

    #[test]
    fn parse_error_converts_to_lsp_diagnostic() {
        let source = "hello\nworld";
        let err = ParseError {
            message: "unexpected token".to_string(),
            span: SourceSpan::new(6, 11),
        };
        let lsp_diag = convert_parse_error(&err, source, &test_uri());
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp_diag.message, "unexpected token");
        assert_eq!(lsp_diag.range.start, Position::new(1, 0));
        assert_eq!(lsp_diag.range.end, Position::new(1, 5));
    }

    // --- position_to_offset tests (pre-1) ---

    #[test]
    fn position_to_offset_first_line() {
        let source = "hello world";
        assert_eq!(position_to_offset(source, Position::new(0, 6)), 6);
    }

    #[test]
    fn position_to_offset_second_line() {
        let source = "first\nsecond";
        // Position(1, 3) → byte 6 + 3 = 9
        assert_eq!(position_to_offset(source, Position::new(1, 3)), 9);
    }

    #[test]
    fn position_to_offset_origin() {
        let source = "hello";
        assert_eq!(position_to_offset(source, Position::new(0, 0)), 0);
    }

    #[test]
    fn position_to_offset_past_end() {
        let source = "hi";
        assert_eq!(
            position_to_offset(source, Position::new(5, 0)),
            source.len()
        );
    }

    #[test]
    fn position_to_offset_utf16_multibyte() {
        // '😀' is U+1F600: 4 bytes in UTF-8, 2 code units in UTF-16
        // "a😀b" = [0x61, 0xF0, 0x9F, 0x98, 0x80, 0x62]
        let source = "a\u{1F600}b";
        // Position(0, 1) → byte 1 (start of '😀')
        assert_eq!(position_to_offset(source, Position::new(0, 1)), 1);
        // Position(0, 3) → byte 5 (start of 'b'; '😀' is 2 UTF-16 units, so col 3)
        assert_eq!(position_to_offset(source, Position::new(0, 3)), 5);
    }

    // --- find_word_at_offset tests (pre-1) ---

    #[test]
    fn find_word_middle_of_source() {
        let source = "let volume = width * height";
        // "volume" starts at 4
        let result = find_word_at_offset(source, 6); // 'l' in volume
        assert_eq!(result, Some((4, "volume")));
    }

    #[test]
    fn find_word_at_non_ident_returns_none() {
        let source = "a > b";
        // offset 2 is '>'
        assert_eq!(find_word_at_offset(source, 2), None);
        // offset 1 is ' '
        assert_eq!(find_word_at_offset(source, 1), None);
    }

    #[test]
    fn find_word_at_start_of_word() {
        let source = "hello world";
        let result = find_word_at_offset(source, 0);
        assert_eq!(result, Some((0, "hello")));
    }

    #[test]
    fn find_word_at_end_of_word() {
        let source = "hello world";
        // offset 4 is 'o' in hello (last char)
        let result = find_word_at_offset(source, 4);
        assert_eq!(result, Some((0, "hello")));
    }

    #[test]
    fn find_word_keyword_param() {
        let source = "param width: Scalar = 80mm";
        let result = find_word_at_offset(source, 2); // 'r' in param
        assert_eq!(result, Some((0, "param")));
    }

    #[test]
    fn find_word_underscore_ident() {
        let source = "let fillet_radius = 3mm";
        let result = find_word_at_offset(source, 8); // 'l' in fillet_radius
        assert_eq!(result, Some((4, "fillet_radius")));
    }

    #[test]
    fn find_word_at_quantity_digits() {
        // '80mm' — digits and letters are identifier chars, so this finds '80mm' as a word
        let source = "= 80mm";
        let result = find_word_at_offset(source, 2); // '8' in 80mm
        assert_eq!(result, Some((2, "80mm")));
    }

    #[test]
    fn position_to_offset_char_past_end_of_last_line() {
        let source = "abc\ndef";
        // Line 1 ("def") has 3 chars. Position(1, 10) is past end of line.
        // Should clamp to source.len() since it's the last line.
        assert_eq!(
            position_to_offset(source, Position::new(1, 10)),
            source.len()
        );
    }

    #[test]
    fn position_to_offset_char_past_end_of_middle_line() {
        let source = "abc\nde\nfgh";
        // Line 1 ("de") has 2 chars. Position(1, 10) is past end of that line.
        // Should clamp to the newline position (byte offset 6).
        assert_eq!(position_to_offset(source, Position::new(1, 10)), 6);
    }

    #[test]
    fn find_word_past_end() {
        let source = "abc";
        assert_eq!(find_word_at_offset(source, 3), None);
        assert_eq!(find_word_at_offset(source, 100), None);
    }
}
