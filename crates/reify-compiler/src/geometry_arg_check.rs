use super::*;

/// Returns `true` when `got == expected`.
///
/// On mismatch, pushes a `Diagnostic::error` whose message is
/// `"{name}() expects {expected} arguments, got {got}"`.
/// When `span` is `Some(s)`, the diagnostic is decorated with a
/// `"wrong number of arguments"` label at that span; when `None`, no label
/// is attached (preserving the unlabeled behavior of transform/curve callers).
pub(crate) fn check_arg_count_exact(
    name: &str,
    got: usize,
    expected: usize,
    span: Option<SourceSpan>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if got == expected {
        return true;
    }
    let diag = Diagnostic::error(format!("{name}() expects {expected} arguments, got {got}"));
    let diag = match span {
        Some(s) => diag.with_label(DiagnosticLabel::new(s, "wrong number of arguments")),
        None => diag,
    };
    diagnostics.push(diag);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // (a) got == expected returns true with empty diagnostics and no label
    #[test]
    fn check_arg_count_exact_ok_no_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = check_arg_count_exact("translate", 4, 4, None, &mut diagnostics);
        assert!(result, "expected true when got == expected");
        assert!(diagnostics.is_empty(), "expected no diagnostics on ok path");
    }

    // (b) got != expected with span = None returns false, pushes diagnostic with correct message and empty labels
    #[test]
    fn check_arg_count_exact_err_no_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = check_arg_count_exact("translate", 2, 4, None, &mut diagnostics);
        assert!(!result, "expected false when got != expected");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("translate() expects 4 arguments, got 2"),
            "unexpected message: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].labels.is_empty(),
            "expected no labels when span is None"
        );
    }

    // (c) got != expected with span = Some(...) returns false with message AND labeled span
    #[test]
    fn check_arg_count_exact_err_with_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(10, 20);
        let result = check_arg_count_exact("translate", 2, 4, Some(span), &mut diagnostics);
        assert!(!result, "expected false when got != expected");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("translate() expects 4 arguments, got 2"),
            "unexpected message: {:?}",
            diagnostics[0].message
        );
        assert_eq!(diagnostics[0].labels.len(), 1, "expected one label");
        assert_eq!(
            diagnostics[0].labels[0].message,
            "wrong number of arguments"
        );
        assert_eq!(diagnostics[0].labels[0].span, span);
    }

    // (d) OK path with span = Some(...) returns true, no diagnostic pushed
    #[test]
    fn check_arg_count_exact_ok_with_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(5, 15);
        let result = check_arg_count_exact("translate", 4, 4, Some(span), &mut diagnostics);
        assert!(result, "expected true when got == expected");
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics on ok path even with span"
        );
    }

    // --- check_arg_count_at_least tests ---

    // (a) got == min returns true with no diagnostic
    #[test]
    fn check_arg_count_at_least_ok_equal() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = check_arg_count_at_least("loft", 2, 2, None, &mut diagnostics);
        assert!(result, "expected true when got == min");
        assert!(diagnostics.is_empty(), "expected no diagnostics on ok path");
    }

    // (b) got > min returns true with no diagnostic
    #[test]
    fn check_arg_count_at_least_ok_greater() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = check_arg_count_at_least("loft", 5, 2, None, &mut diagnostics);
        assert!(result, "expected true when got > min");
        assert!(diagnostics.is_empty(), "expected no diagnostics on ok path");
    }

    // (c) got < min with span = None returns false, pushes diagnostic with "at least" message and empty labels
    #[test]
    fn check_arg_count_at_least_err_no_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = check_arg_count_at_least("loft", 1, 2, None, &mut diagnostics);
        assert!(!result, "expected false when got < min");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("loft() expects at least 2 arguments, got 1"),
            "unexpected message: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].labels.is_empty(),
            "expected no labels when span is None"
        );
    }

    // (d) got < min with span = Some(...) pushes diagnostic with "at least" message AND labeled span
    #[test]
    fn check_arg_count_at_least_err_with_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(10, 20);
        let result = check_arg_count_at_least("loft", 1, 2, Some(span), &mut diagnostics);
        assert!(!result, "expected false when got < min");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("loft() expects at least 2 arguments, got 1"),
            "unexpected message: {:?}",
            diagnostics[0].message
        );
        assert_eq!(diagnostics[0].labels.len(), 1, "expected one label");
        assert_eq!(
            diagnostics[0].labels[0].message,
            "wrong number of arguments"
        );
        assert_eq!(diagnostics[0].labels[0].span, span);
    }
}
