#[cfg(test)]
mod tests {
    use super::{IgnoreForm, extract_ignore_reason, ignore_attr};

    #[test]
    fn ignore_attr_forms() {
        assert_eq!(ignore_attr("#[ignore]"), Some(IgnoreForm::Bare));
        assert_eq!(
            ignore_attr("#[ignore = \"r\"]"),
            Some(IgnoreForm::WithReason)
        );
        // Indented bare form still recognised (trimmed-line start).
        assert_eq!(ignore_attr("    #[ignore]"), Some(IgnoreForm::Bare));
        // Doc-comment prose mentioning the attribute must NOT fire.
        assert_eq!(ignore_attr("/// #[ignore]"), None);
    }

    // ── extract_ignore_reason ─────────────────────────────────────────────────

    // Tests assembled using runtime concat so this source file does not contain
    // the literal `#[ignore` substring and does not self-trigger the workspace
    // guard or check_ignore_reasons.

    /// (a) Canonical `#[ignore = "reason text"]` → Some("reason text").
    #[test]
    fn eir_canonical_form_returns_reason() {
        // Assembled at runtime so this file does not self-trigger.
        let line = ["#[ignore", " = \"reason text\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("reason text"));
    }

    /// (b) Leading-whitespace/indented form → Some(...).
    #[test]
    fn eir_indented_form_returns_reason() {
        let line = ["    #[ignore", " = \"indented reason\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("indented reason"));
    }

    /// (c) Reason carrying a cite → Some("see #42").
    #[test]
    fn eir_reason_with_cite_returns_reason() {
        let line = ["#[ignore", " = \"see #42\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("see #42"));
    }

    /// (d) Bare `#[ignore]` (no reason string) → None.
    #[test]
    fn eir_bare_ignore_returns_none() {
        let line = ["#[ignore", "]"].concat();
        assert_eq!(extract_ignore_reason(&line), None);
    }

    /// (e) A non-ignore line → None.
    #[test]
    fn eir_non_ignore_line_returns_none() {
        assert_eq!(extract_ignore_reason("fn some_test() {}"), None);
    }

    /// (f) A `///` outer doc-comment line mentioning the attribute in prose → None.
    #[test]
    fn eir_outer_doc_comment_returns_none() {
        let line = ["/// example: #[ignore", " = \"r\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), None);
    }

    /// (g) A `//!` inner doc-comment line mentioning the attribute in prose → None.
    #[test]
    fn eir_inner_doc_comment_returns_none() {
        let line = ["//! example: #[ignore", " = \"r\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), None);
    }

    /// (h) Non-canonical `#[ignore="reason"]` (no spaces around `=`) → Some("reason").
    /// Pins that extract_ignore_reason agrees with the same module's
    /// ignore_attr, which classifies this hand-edited/macro-expanded non-spaced
    /// form `WithReason`, so a blocker-prose reason like `#[ignore="blocked"]`
    /// is not silently escaped.
    #[test]
    fn eir_non_canonical_no_spaces_returns_reason() {
        // Assembled at runtime so this file does not self-trigger.
        let line = ["#[ignore", "=\"pending fillet binding\"]"].concat();
        assert_eq!(extract_ignore_reason(&line), Some("pending fillet binding"));
    }
}
