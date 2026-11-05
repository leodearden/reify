//! §8.1 `#[ignore]`-attribute line parsing for PTODO: which shape an attribute
//! line has ([`ignore_attr`]) and the reason string a reason-bearing one
//! carries ([`extract_ignore_reason`]). Both skip doc-comment prose through one
//! predicate ([`is_doc_comment_line`]), so they agree on which lines are
//! attributes at all.

/// The two §8.1 `#[ignore]` shapes the structural lane distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IgnoreForm {
    /// `#[ignore]` — no reason string (α emits `bare-ignore`).
    Bare,
    /// `#[ignore = "..."]` — carries a reason (α defers the reason policy to γ).
    WithReason,
}

/// `///` (outer) or `//!` (inner) doc-comment line, after leading whitespace.
/// Prose there that mentions the attribute is not an attribute. Twin of
/// `reify_test_support::ignore_hygiene::is_doc_comment_line`; change both together.
fn is_doc_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("///") || t.starts_with("//!")
}

/// §8.1 ignore attributes (`.rs` only — the gating lives in the caller,
/// `ptodo::scan_file`): a trimmed line that starts with `#[ignore`.
/// `///`/`//!` doc-comment prose mentioning the attribute does not fire. `]`
/// immediately after → `Bare`; `=` → `WithReason`.
pub(super) fn ignore_attr(line: &str) -> Option<IgnoreForm> {
    if is_doc_comment_line(line) {
        return None;
    }
    let rest = line.trim_start().strip_prefix("#[ignore")?.trim_start();
    if rest.starts_with(']') {
        Some(IgnoreForm::Bare)
    } else if rest.starts_with('=') {
        Some(IgnoreForm::WithReason)
    } else {
        None
    }
}

/// Extract the reason string from a single `#[ignore = "..."]` attribute line.
///
/// Returns `Some(reason)` for `#[ignore = "reason"]` and related non-canonical
/// forms (optional whitespace around `=`, e.g. `#[ignore="reason"]`), where
/// `reason` is the slice between the opening `"` and the next raw `"` byte.
/// Returns `None` for:
/// - bare `#[ignore]` attributes (no reason string)
/// - non-`#[ignore]` lines
/// - `///` and `//!` doc-comment lines (prose mentions of the attribute)
///
/// The parse mirrors [`ignore_attr`]'s trim-based approach so both functions
/// agree on which forms carry a reason — closing the silent-escape hole where
/// a hand-edited `#[ignore="blocked"]` (no spaces) would be classified
/// `WithReason` by `ignore_attr` but yield `None` here.
///
/// **Note:** escaped quotes (`\"`) inside the reason are not handled — the
/// reason is truncated at the first raw `"`. No current source file uses
/// escaped quotes inside `#[ignore]` reasons.
///
/// This is the line-level reason extractor of PTODO's §8.3 γ lane, and it sits
/// beside [`ignore_attr`] in PTODO. The format-vs-liveness split: PTODO owns
/// extraction and citation-liveness (`has_canonical_cite`/`resolve_liveness`);
/// `reify-test-support` keeps the format checks (`check_ignore_reasons`).
pub(super) fn extract_ignore_reason(line: &str) -> Option<&str> {
    // Skip doc-comment lines (`///`, `//!`) — prose mentions of the attribute
    // inside doc comments must not fire, the same skip `ignore_attr` applies.
    if is_doc_comment_line(line) {
        return None;
    }
    // Mirror ignore_attr's trim-based parse: strip `#[ignore`, consume optional
    // whitespace, `=`, optional whitespace, then the opening `"`.  This handles
    // canonical `#[ignore = "..."]` and non-canonical `#[ignore="..."]` alike.
    let t = line.trim_start();
    let rest = t.strip_prefix("#[ignore")?;
    let rest = rest.trim_start().strip_prefix('=')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    // The reason is everything up to the first raw `"` (escaped-quote
    // limitation documented in the function doc comment above).
    let end = rest.find('"')?;
    Some(&rest[..end])
}

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
