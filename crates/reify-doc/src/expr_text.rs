//! The text of an expression-valued doc field, or the typed reason it has none
//! (PRD `docs/prds/v0_6/tooling-front-end.md` §4.6, INV-SF-5).

use std::fmt;

use serde::{Deserialize, Serialize};

/// What a doc field shows for an expression: its source text, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ExprTextWire", into = "ExprTextWire")]
pub enum ExprText {
    /// Text sliced verbatim from the declaring module's source.
    Source(String),
    /// No such text could be produced.
    Unavailable(UnavailableReason),
}

/// Why an expression-valued doc field carries no source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The compiled node records no source span (e.g. a `CompiledExpr`).
    NoSpan,
    /// A span exists but does not address text in the source the lowering was
    /// given: the prelude sentinel, an empty or synthesized span, out of range,
    /// not on a char boundary, or no source passed at all.
    SpanOutsideSource,
    /// The declaration span's text has no `=` value.
    DeclarationWithoutValue,
}

/// Decodes a missing serialized key: no span was recorded for it.
impl Default for ExprText {
    fn default() -> Self {
        ExprText::Unavailable(UnavailableReason::NoSpan)
    }
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnavailableReason::NoSpan => "no source span",
            UnavailableReason::SpanOutsideSource => "span outside module source",
            UnavailableReason::DeclarationWithoutValue => "declaration has no value",
        })
    }
}

impl fmt::Display for ExprText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprText::Source(text) => f.write_str(text),
            ExprText::Unavailable(reason) => write!(f, "source unavailable ({reason})"),
        }
    }
}

/// Wire form: source text is a bare JSON string, so string-valued payloads
/// written before `ExprText` existed keep decoding.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ExprTextWire {
    Source(String),
    Unavailable { unavailable: UnavailableReason },
}

impl From<ExprTextWire> for ExprText {
    fn from(wire: ExprTextWire) -> Self {
        match wire {
            ExprTextWire::Source(text) => ExprText::Source(text),
            ExprTextWire::Unavailable { unavailable } => ExprText::Unavailable(unavailable),
        }
    }
}

impl From<ExprText> for ExprTextWire {
    fn from(text: ExprText) -> Self {
        match text {
            ExprText::Source(text) => ExprTextWire::Source(text),
            ExprText::Unavailable(unavailable) => ExprTextWire::Unavailable { unavailable },
        }
    }
}

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
