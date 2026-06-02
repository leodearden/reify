use super::*;

/// Build the precision-loss warning for an integer-form literal that overflowed i64 bounds
/// and was classified as [`reify_syntax::NumberClass::LossyReal`].
///
/// Shared between `compile_expr_guarded` (`crate::expr`) and `lower_annotations`
/// (`crate::annotations`) so both sites emit an identical diagnostic — keeping
/// the message text in one place mirrors why `classify_number_literal` was
/// centralised in `reify-syntax` (task 3251).
pub(crate) fn lossy_real_warning(span: SourceSpan) -> Diagnostic {
    Diagnostic::warning(
        "integer literal too large to represent as Int; \
         using Real (precision may be lost)",
    )
    .with_label(DiagnosticLabel::new(span, "precision lost"))
}

/// Build the `E_DUP_MEMBER_KEY` error for two members of the same `Keyed<T>`
/// sub-collection that declare the same author-assigned String key.
///
/// Keys are author-assigned and must be unique within one keyed collection, so
/// a duplicate is a compile-time identity collision (task 3930 β / PRD
/// `docs/prds/keyed-collection-identity.md`). Mirrors the duplicate port-name
/// and duplicate meta-key pre-pass diagnostics: two labels anchor the duplicate
/// occurrence and the first occurrence. The `E_DUP_MEMBER_KEY` mnemonic is
/// embedded in the message text; downstream tooling matches on
/// [`DiagnosticCode::DuplicateMemberKey`].
pub(crate) fn dup_member_key_error(
    sub_name: &str,
    key: &str,
    first_span: SourceSpan,
    dup_span: SourceSpan,
) -> Diagnostic {
    Diagnostic::error(format!(
        "E_DUP_MEMBER_KEY: duplicate keyed member key '{key}' in keyed sub '{sub_name}'"
    ))
    .with_code(DiagnosticCode::DuplicateMemberKey)
    .with_label(DiagnosticLabel::new(dup_span, "duplicate key defined here"))
    .with_label(DiagnosticLabel::new(first_span, "first defined here"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_real_warning_emits_precision_loss_warning_with_label() {
        let span = SourceSpan::empty(42);
        let d = lossy_real_warning(span);
        assert_eq!(d.severity, Severity::Warning);
        assert!(
            d.message.contains("too large"),
            "expected 'too large' in message: {:?}",
            d.message
        );
        assert!(
            d.message.contains("precision"),
            "expected 'precision' in message: {:?}",
            d.message
        );
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.labels[0].span, span);
        assert!(
            d.labels[0].message.contains("precision lost"),
            "expected 'precision lost' in label message: {:?}",
            d.labels[0].message
        );
    }

    #[test]
    fn dup_member_key_error_is_error_with_code_and_two_labels() {
        let first_span = SourceSpan::new(0, 5);
        let dup_span = SourceSpan::new(10, 15);
        let d = dup_member_key_error("vents", "intake", first_span, dup_span);

        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DuplicateMemberKey));

        // Message embeds the E_DUP_MEMBER_KEY mnemonic, the offending key, and
        // the sub name (downstream tooling matches on the code, not the text).
        assert!(
            d.message.contains("E_DUP_MEMBER_KEY"),
            "expected 'E_DUP_MEMBER_KEY' in message: {:?}",
            d.message
        );
        assert!(
            d.message.contains("intake"),
            "expected key 'intake' in message: {:?}",
            d.message
        );
        assert!(
            d.message.contains("vents"),
            "expected sub name 'vents' in message: {:?}",
            d.message
        );

        // Exactly two labels: the duplicate site first, then the first-seen site.
        assert_eq!(d.labels.len(), 2);
        assert_eq!(d.labels[0].span, dup_span);
        assert!(
            d.labels[0].message.contains("duplicate key defined here"),
            "expected 'duplicate key defined here' in label 0: {:?}",
            d.labels[0].message
        );
        assert_eq!(d.labels[1].span, first_span);
        assert!(
            d.labels[1].message.contains("first defined here"),
            "expected 'first defined here' in label 1: {:?}",
            d.labels[1].message
        );
    }
}
