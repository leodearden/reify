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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_real_warning_emits_precision_loss_warning_with_label() {
        let span = SourceSpan::empty(42);
        let d = lossy_real_warning(span);
        assert_eq!(d.severity, Severity::Warning);
        assert!(
            d.message.contains("integer literal"),
            "expected 'integer literal' in message: {:?}",
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
}
