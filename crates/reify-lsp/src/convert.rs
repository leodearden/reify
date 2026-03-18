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

/// Convert a Reify Diagnostic to an LSP Diagnostic.
pub fn convert_diagnostic(
    diag: &Diagnostic,
    source: &str,
    uri: &Url,
) -> lsp_types::Diagnostic {
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

    lsp_types::Diagnostic {
        range,
        severity: Some(convert_severity(diag.severity)),
        message: diag.message.clone(),
        source: Some("reify".to_string()),
        related_information,
        ..Default::default()
    }
}

/// Convert a ParseError to an LSP Diagnostic.
pub fn convert_parse_error(
    err: &ParseError,
    source: &str,
    _uri: &Url,
) -> lsp_types::Diagnostic {
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
    use reify_types::DiagnosticLabel;
    use tower_lsp::lsp_types::{DiagnosticSeverity, Position, Url};

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
}
