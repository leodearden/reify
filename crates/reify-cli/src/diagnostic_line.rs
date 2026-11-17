//! Renders a compiler Diagnostic as one `path:line:col: severity: message` line, positioned by its first label.

use reify_core::{Diagnostic, byte_offset_to_line_col};

/// `path:line:col: severity: message`, or `path: severity: message` when the diagnostic's first
/// label is missing or does not start inside `source`.
pub(crate) fn render_diagnostic_line(path: &str, source: &str, diagnostic: &Diagnostic) -> String {
    let (severity, message) = (diagnostic.severity, &diagnostic.message);
    match primary_position(source, diagnostic) {
        Some((line, col)) => format!("{path}:{line}:{col}: {severity}: {message}"),
        None => format!("{path}: {severity}: {message}"),
    }
}

/// The 1-based (line, codepoint column) where the first label starts. `None` without a label, for
/// the prelude sentinel, or when that start does not index `source`.
fn primary_position(source: &str, diagnostic: &Diagnostic) -> Option<(usize, usize)> {
    let span = &diagnostic.labels.first()?.span;
    let start = span.start as usize;
    (!span.is_prelude() && start <= source.len()).then(|| byte_offset_to_line_col(source, start))
}

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan};

    use super::render_diagnostic_line;

    fn labelled_at(start: usize, message: &str) -> Diagnostic {
        let start = u32::try_from(start).expect("offset fits u32");
        Diagnostic::error(message)
            .with_label(DiagnosticLabel::new(SourceSpan::new(start, start), "here"))
    }

    #[test]
    fn a_labelled_diagnostic_leads_with_its_file_line_and_column() {
        let source = "structure def S {\n    param α : Nope\n}\n";
        let start = source.find("Nope").expect("fixture names Nope");
        let diagnostic = labelled_at(start, "unresolved type: Nope");

        assert_eq!(
            render_diagnostic_line("s.ri", source, &diagnostic),
            "s.ri:2:15: error: unresolved type: Nope"
        );
    }

    #[test]
    fn the_first_label_positions_the_line() {
        let source = "a\nb\nc\n";
        let on_line_3 = u32::try_from(source.find('c').expect("fixture has c")).expect("fits u32");
        let diagnostic = Diagnostic::error("m")
            .with_label(DiagnosticLabel::new(
                SourceSpan::new(on_line_3, on_line_3 + 1),
                "first",
            ))
            .with_label(DiagnosticLabel::new(SourceSpan::new(0, 1), "second"));

        assert_eq!(
            render_diagnostic_line("s.ri", source, &diagnostic),
            "s.ri:3:1: error: m"
        );
    }

    #[test]
    fn a_later_label_does_not_position_the_line_when_the_first_cannot() {
        let source = "a\nb\n";
        let past_the_end = u32::try_from(source.len() + 1).expect("fits u32");
        let diagnostic = Diagnostic::error("m")
            .with_label(DiagnosticLabel::new(
                SourceSpan::new(past_the_end, past_the_end),
                "first",
            ))
            .with_label(DiagnosticLabel::new(SourceSpan::new(0, 1), "second"));

        assert_eq!(
            render_diagnostic_line("s.ri", source, &diagnostic),
            "s.ri: error: m"
        );
    }

    #[test]
    fn a_diagnostic_without_a_label_names_only_its_file() {
        assert_eq!(
            render_diagnostic_line("s.ri", "a\n", &Diagnostic::warning("w")),
            "s.ri: warning: w"
        );
    }

    #[test]
    fn a_label_outside_the_source_names_only_its_file() {
        let source = "a\nb\n";

        let past_the_end = labelled_at(source.len() + 1, "m");
        assert_eq!(
            render_diagnostic_line("s.ri", source, &past_the_end),
            "s.ri: error: m"
        );

        let prelude = labelled_at(SourceSpan::PRELUDE_SENTINEL_OFFSET, "m");
        assert_eq!(
            render_diagnostic_line("s.ri", source, &prelude),
            "s.ri: error: m"
        );

        let at_the_end = labelled_at(source.len(), "m");
        assert_eq!(
            render_diagnostic_line("s.ri", source, &at_the_end),
            "s.ri:3:1: error: m"
        );
    }
}
