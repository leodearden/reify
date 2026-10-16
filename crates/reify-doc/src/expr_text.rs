#[cfg(test)]
mod tests {
    use super::{ExprText, UnavailableReason};

    const ALL_REASONS: [UnavailableReason; 3] = [
        UnavailableReason::NoSpan,
        UnavailableReason::SpanOutsideSource,
        UnavailableReason::DeclarationWithoutValue,
    ];

    #[test]
    fn source_displays_its_text_verbatim() {
        assert_eq!(ExprText::Source("a >= b".into()).to_string(), "a >= b");
    }

    #[test]
    fn unavailable_displays_one_placeholder_per_reason() {
        let placeholders: Vec<String> = ALL_REASONS
            .iter()
            .map(|reason| ExprText::Unavailable(*reason).to_string())
            .collect();
        for placeholder in &placeholders {
            assert!(
                placeholder.starts_with("source unavailable (") && placeholder.ends_with(')'),
                "placeholder {placeholder:?} must read `source unavailable (<reason>)`"
            );
        }
        for (i, a) in placeholders.iter().enumerate() {
            for b in &placeholders[i + 1..] {
                assert_ne!(a, b, "each reason must have its own placeholder text");
            }
        }
        assert_eq!(
            ExprText::Unavailable(UnavailableReason::NoSpan).to_string(),
            "source unavailable (no source span)"
        );
    }

    #[test]
    fn source_serializes_as_a_bare_json_string() {
        let source = ExprText::Source("10mm".into());
        assert_eq!(serde_json::to_string(&source).unwrap(), "\"10mm\"");
        assert_eq!(
            serde_json::from_str::<ExprText>("\"10mm\"").unwrap(),
            source,
            "a legacy string payload must keep decoding as source text"
        );
    }

    #[test]
    fn unavailable_serializes_as_a_tagged_reason() {
        let expected_wire = [
            (UnavailableReason::NoSpan, "no_span"),
            (UnavailableReason::SpanOutsideSource, "span_outside_source"),
            (
                UnavailableReason::DeclarationWithoutValue,
                "declaration_without_value",
            ),
        ];
        for (reason, wire_name) in expected_wire {
            let value = ExprText::Unavailable(reason);
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(json, format!("{{\"unavailable\":\"{wire_name}\"}}"));
            assert_eq!(serde_json::from_str::<ExprText>(&json).unwrap(), value);
        }
    }

    #[test]
    fn default_is_unavailable_no_span() {
        assert_eq!(
            ExprText::default(),
            ExprText::Unavailable(UnavailableReason::NoSpan)
        );
    }
}
