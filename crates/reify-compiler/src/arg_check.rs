use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan};

/// Returns `true` when `got == expected`.
///
/// On mismatch, pushes a `Diagnostic::error` whose message is
/// `"{name}() expects {expected} arguments, got {got}"`, decorated with a
/// `"wrong number of arguments"` label at `span`.
pub(crate) fn check_arg_count_exact(
    name: &str,
    got: usize,
    expected: usize,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if got == expected {
        return true;
    }
    let noun = if expected == 1 {
        "argument"
    } else {
        "arguments"
    };
    diagnostics.push(
        Diagnostic::error(format!("{name}() expects {expected} {noun}, got {got}"))
            .with_label(DiagnosticLabel::new(span, "wrong number of arguments")),
    );
    false
}

/// Returns `true` when `got >= min_expected`.
///
/// On failure, pushes a `Diagnostic::error` whose message is
/// `"{name}() expects at least {min_expected} arguments, got {got}"`, decorated
/// with a `"wrong number of arguments"` label at `span`.
pub(crate) fn check_arg_count_at_least(
    name: &str,
    got: usize,
    min_expected: usize,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if got >= min_expected {
        return true;
    }
    let noun = if min_expected == 1 {
        "argument"
    } else {
        "arguments"
    };
    diagnostics.push(
        Diagnostic::error(format!(
            "{name}() expects at least {min_expected} {noun}, got {got}"
        ))
        .with_label(DiagnosticLabel::new(span, "wrong number of arguments")),
    );
    false
}

/// Push a labeled arg-count error with a fully custom message.
///
/// Used for variadic ops whose validation logic doesn't map directly onto
/// `check_arg_count_exact` or `check_arg_count_at_least` (e.g. ops that
/// require both a minimum count *and* a multiple-of-N constraint).  The
/// `"wrong number of arguments"` label text is centralised here so all
/// arg-count diagnostics use exactly the same wording regardless of which
/// helper emitted them.
pub(crate) fn push_labeled_arg_count_error(
    msg: impl Into<String>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(msg.into())
            .with_label(DiagnosticLabel::new(span, "wrong number of arguments")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // (a) got == expected returns true with empty diagnostics
    #[test]
    fn check_arg_count_exact_ok_with_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(5, 15);
        let result = check_arg_count_exact("translate", 4, 4, span, &mut diagnostics);
        assert!(result, "expected true when got == expected");
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics on ok path even with span"
        );
    }

    // (b) got != expected returns false with message AND labeled span
    #[test]
    fn check_arg_count_exact_err_with_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(10, 20);
        let result = check_arg_count_exact("translate", 2, 4, span, &mut diagnostics);
        assert!(!result, "expected false when got != expected");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert_eq!(
            diagnostics[0].message,
            "translate() expects 4 arguments, got 2"
        );
        assert_eq!(diagnostics[0].labels.len(), 1, "expected one label");
        assert_eq!(
            diagnostics[0].labels[0].message,
            "wrong number of arguments"
        );
        assert_eq!(diagnostics[0].labels[0].span, span);
    }

    // --- check_arg_count_at_least tests ---

    // (a) got == min returns true with no diagnostic
    #[test]
    fn check_arg_count_at_least_ok_equal() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(0, 10);
        let result = check_arg_count_at_least("loft", 2, 2, span, &mut diagnostics);
        assert!(result, "expected true when got == min");
        assert!(diagnostics.is_empty(), "expected no diagnostics on ok path");
    }

    // (b) got > min returns true with no diagnostic
    #[test]
    fn check_arg_count_at_least_ok_greater() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(0, 10);
        let result = check_arg_count_at_least("loft", 5, 2, span, &mut diagnostics);
        assert!(result, "expected true when got > min");
        assert!(diagnostics.is_empty(), "expected no diagnostics on ok path");
    }

    // (c) got < min pushes diagnostic with "at least" message AND labeled span
    #[test]
    fn check_arg_count_at_least_err_with_span() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let span = SourceSpan::new(10, 20);
        let result = check_arg_count_at_least("loft", 1, 2, span, &mut diagnostics);
        assert!(!result, "expected false when got < min");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert_eq!(
            diagnostics[0].message,
            "loft() expects at least 2 arguments, got 1"
        );
        assert_eq!(diagnostics[0].labels.len(), 1, "expected one label");
        assert_eq!(
            diagnostics[0].labels[0].message,
            "wrong number of arguments"
        );
        assert_eq!(diagnostics[0].labels[0].span, span);
    }
}
