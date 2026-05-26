use crate::diagnostics::SourceSpan;
pub use crate::primitives::{
    DEPRECATED_ANNOTATION, OPTIMIZED_ANNOTATION, SHELL_ANNOTATION, SOLID_ANNOTATION,
    SOLVER_HINT_ANNOTATION, TEST_ANNOTATION,
};

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

impl Annotation {
    /// Returns `true` if this annotation is the `@test` marker.
    pub fn is_test(&self) -> bool {
        self.name == TEST_ANNOTATION
    }
}

/// Returns `true` if the given annotation slice contains a `@test` annotation.
///
/// This is the canonical predicate for checking test-tagged entities.
/// Prefer this over manually scanning annotations.
pub fn has_test_annotation(annotations: &[Annotation]) -> bool {
    annotations.iter().any(Annotation::is_test)
}

/// A lowered annotation argument: an optional name plus a value.
///
/// `name: None` is a positional argument (`@shell(0.5)`, `@allow(shadowing)`);
/// `name: Some(_)` is a named argument (`@optimized(target = "…")`). Named-arg
/// lowering lands in task η — task δ (the `Expr`-variant widening) only ever
/// produces positional args.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationArg {
    /// `None` = positional, `Some` = named.
    pub name: Option<String>,
    pub value: AnnotationArgValue,
}

impl AnnotationArg {
    /// Construct a positional argument (`name: None`) wrapping `value`.
    pub fn positional(value: AnnotationArgValue) -> Self {
        Self { name: None, value }
    }
}

/// The value of an annotation argument.
///
/// The literal variants (`String`/`Int`/`Real`/`Bool`/`Ident`) are compile-time
/// constants produced for the common case. `Expr` carries an *unevaluated*
/// parsed expression for annotations whose schema declares
/// `eval_time = AtMaterialization` (see `AnnotationSchema` in
/// `reify-compiler/src/annotations/schema.rs`); it is evaluated in instance
/// scope at structure-instance materialization (annotation-args PRD §4). Stored
/// by `lower_annotations` whenever an annotation arg is a non-literal expression
/// (task 3555 / annotation-args δ).
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationArgValue {
    String(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    Ident(String),
    Expr(crate::Expr),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_annotation(name: &str) -> Annotation {
        Annotation {
            name: name.to_string(),
            args: vec![],
            span: SourceSpan::empty(0),
        }
    }

    #[test]
    fn test_annotation_constant_is_lowercase_test() {
        assert_eq!(TEST_ANNOTATION, "test");
    }

    #[test]
    fn has_test_annotation_returns_true_when_test_present() {
        let anns = vec![make_annotation(TEST_ANNOTATION)];
        assert!(has_test_annotation(&anns));
    }

    #[test]
    fn has_test_annotation_returns_false_on_empty_slice() {
        let anns: Vec<Annotation> = vec![];
        assert!(!has_test_annotation(&anns));
    }

    #[test]
    fn has_test_annotation_returns_false_when_other_annotations_present() {
        let anns = vec![
            make_annotation(DEPRECATED_ANNOTATION),
            make_annotation("inline"),
        ];
        assert!(!has_test_annotation(&anns));
    }

    #[test]
    fn has_test_annotation_returns_true_when_test_among_multiple() {
        let anns = vec![
            make_annotation(DEPRECATED_ANNOTATION),
            make_annotation(TEST_ANNOTATION),
            make_annotation("inline"),
        ];
        assert!(has_test_annotation(&anns));
    }

    #[test]
    fn annotation_is_test_method_returns_true_for_test() {
        let ann = make_annotation(TEST_ANNOTATION);
        assert!(ann.is_test());
    }

    #[test]
    fn annotation_is_test_method_returns_false_for_other() {
        let ann = make_annotation(DEPRECATED_ANNOTATION);
        assert!(!ann.is_test());
    }

    #[test]
    fn constants_are_usable_in_annotation_construction() {
        // Constants can be used directly in make_annotation() and is_test() works correctly.
        let dep = make_annotation(DEPRECATED_ANNOTATION);
        let opt = make_annotation(OPTIMIZED_ANNOTATION);
        let hint = make_annotation(SOLVER_HINT_ANNOTATION);
        assert!(!dep.is_test());
        assert!(!opt.is_test());
        assert!(!hint.is_test());
        // Names round-trip through the Annotation struct.
        assert_eq!(dep.name, "deprecated");
        assert_eq!(opt.name, "optimized");
        assert_eq!(hint.name, "solver_hint");
    }
}
