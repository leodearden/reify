use reify_types::{Annotation, AnnotationArg, AnnotationArgValue, SourceSpan};

/// Create a positional `String`-valued annotation argument.
pub fn ann_str(s: impl Into<String>) -> AnnotationArg {
    AnnotationArg::positional(AnnotationArgValue::String(s.into()))
}

/// Create a positional `Int`-valued annotation argument.
pub fn ann_int(n: i64) -> AnnotationArg {
    AnnotationArg::positional(AnnotationArgValue::Int(n))
}

/// Create a positional `Real`-valued annotation argument.
pub fn ann_real(f: f64) -> AnnotationArg {
    AnnotationArg::positional(AnnotationArgValue::Real(f))
}

/// Create a positional `Bool`-valued annotation argument.
pub fn ann_bool(b: bool) -> AnnotationArg {
    AnnotationArg::positional(AnnotationArgValue::Bool(b))
}

/// Create a positional `Ident`-valued annotation argument.
pub fn ann_ident(s: impl Into<String>) -> AnnotationArg {
    AnnotationArg::positional(AnnotationArgValue::Ident(s.into()))
}

/// Create an `Annotation` with the given name and no arguments.
pub fn annotation(name: impl Into<String>) -> Annotation {
    Annotation {
        name: name.into(),
        args: Vec::new(),
        span: SourceSpan::new(0, 0),
    }
}

/// Create an `Annotation` with the given name and arguments.
pub fn annotation_with_args(name: impl Into<String>, args: Vec<AnnotationArg>) -> Annotation {
    Annotation {
        name: name.into(),
        args,
        span: SourceSpan::new(0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{DEPRECATED_ANNOTATION, TEST_ANNOTATION};

    #[test]
    fn ann_str_produces_string_arg() {
        let arg = ann_str("hello");
        assert_eq!(
            arg,
            AnnotationArg::positional(AnnotationArgValue::String("hello".to_string()))
        );
    }

    #[test]
    fn ann_int_produces_int_arg() {
        let arg = ann_int(42);
        assert_eq!(arg, AnnotationArg::positional(AnnotationArgValue::Int(42)));
    }

    #[test]
    fn ann_real_produces_real_arg() {
        let arg = ann_real(3.125);
        assert_eq!(
            arg,
            AnnotationArg::positional(AnnotationArgValue::Real(3.125))
        );
    }

    #[test]
    fn ann_bool_produces_bool_arg() {
        let arg = ann_bool(true);
        assert_eq!(
            arg,
            AnnotationArg::positional(AnnotationArgValue::Bool(true))
        );
    }

    #[test]
    fn ann_ident_produces_ident_arg() {
        let arg = ann_ident("deprecated");
        assert_eq!(
            arg,
            AnnotationArg::positional(AnnotationArgValue::Ident("deprecated".to_string()))
        );
    }

    #[test]
    fn annotation_produces_empty_args() {
        let ann = annotation(TEST_ANNOTATION);
        assert_eq!(ann.name, TEST_ANNOTATION);
        assert!(ann.args.is_empty());
    }

    #[test]
    fn annotation_with_args_produces_annotation_with_args() {
        let ann = annotation_with_args(DEPRECATED_ANNOTATION, vec![ann_str("use Foo instead")]);
        assert_eq!(ann.name, DEPRECATED_ANNOTATION);
        assert_eq!(ann.args.len(), 1);
        assert_eq!(
            ann.args[0],
            AnnotationArg::positional(AnnotationArgValue::String("use Foo instead".to_string()))
        );
    }
}
