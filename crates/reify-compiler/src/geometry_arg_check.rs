use super::*;

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
}
