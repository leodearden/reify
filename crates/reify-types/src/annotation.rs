use crate::diagnostics::SourceSpan;

/// A compiled annotation — resolved from a parsed `@name(args...)` syntax annotation.
///
/// Annotations carry compile-time metadata through to downstream consumers
/// (LSP hover, deprecation warnings, test discovery, etc.).
#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<AnnotationArg>,
    pub span: SourceSpan,
}

/// A resolved annotation argument value.
///
/// Annotation args are compile-time constants, not runtime expressions.
/// Complex expressions in annotation positions are rejected during lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationArg {
    String(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    Ident(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_annotation(name: &str) -> Annotation {
        Annotation {
            name: name.to_string(),
            args: vec![],
            span: SourceSpan::default(),
        }
    }

    #[test]
    fn test_annotation_constant_is_lowercase_test() {
        assert_eq!(TEST_ANNOTATION, "test");
    }

    #[test]
    fn has_test_annotation_returns_true_when_test_present() {
        let anns = vec![make_annotation("test")];
        assert!(has_test_annotation(&anns));
    }

    #[test]
    fn has_test_annotation_returns_false_on_empty_slice() {
        let anns: Vec<Annotation> = vec![];
        assert!(!has_test_annotation(&anns));
    }

    #[test]
    fn has_test_annotation_returns_false_when_other_annotations_present() {
        let anns = vec![make_annotation("deprecated"), make_annotation("inline")];
        assert!(!has_test_annotation(&anns));
    }

    #[test]
    fn has_test_annotation_returns_true_when_test_among_multiple() {
        let anns = vec![
            make_annotation("deprecated"),
            make_annotation("test"),
            make_annotation("inline"),
        ];
        assert!(has_test_annotation(&anns));
    }

    #[test]
    fn annotation_is_test_method_returns_true_for_test() {
        let ann = make_annotation("test");
        assert!(ann.is_test());
    }

    #[test]
    fn annotation_is_test_method_returns_false_for_other() {
        let ann = make_annotation("deprecated");
        assert!(!ann.is_test());
    }
}
