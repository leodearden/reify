use super::*;

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
