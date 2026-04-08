use reify_types::{Annotation, AnnotationArg, SourceSpan};

/// Create an `AnnotationArg::String` with the given string value.
pub fn ann_str(s: impl Into<String>) -> AnnotationArg {
    AnnotationArg::String(s.into())
}

/// Create an `AnnotationArg::Int` with the given integer value.
pub fn ann_int(n: i64) -> AnnotationArg {
    AnnotationArg::Int(n)
}

/// Create an `AnnotationArg::Real` with the given float value.
pub fn ann_real(f: f64) -> AnnotationArg {
    AnnotationArg::Real(f)
}

/// Create an `AnnotationArg::Bool` with the given bool value.
pub fn ann_bool(b: bool) -> AnnotationArg {
    AnnotationArg::Bool(b)
}

/// Create an `AnnotationArg::Ident` with the given identifier string.
pub fn ann_ident(s: impl Into<String>) -> AnnotationArg {
    AnnotationArg::Ident(s.into())
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

    #[test]
    fn ann_str_produces_string_arg() {
        let arg = ann_str("hello");
        assert_eq!(arg, AnnotationArg::String("hello".to_string()));
    }

    #[test]
    fn ann_int_produces_int_arg() {
        let arg = ann_int(42);
        assert_eq!(arg, AnnotationArg::Int(42));
    }

    #[test]
    fn ann_real_produces_real_arg() {
        let arg = ann_real(3.14);
        assert_eq!(arg, AnnotationArg::Real(3.14));
    }

    #[test]
    fn ann_bool_produces_bool_arg() {
        let arg = ann_bool(true);
        assert_eq!(arg, AnnotationArg::Bool(true));
    }

    #[test]
    fn ann_ident_produces_ident_arg() {
        let arg = ann_ident("deprecated");
        assert_eq!(arg, AnnotationArg::Ident("deprecated".to_string()));
    }

    #[test]
    fn annotation_produces_empty_args() {
        let ann = annotation("test");
        assert_eq!(ann.name, "test");
        assert!(ann.args.is_empty());
    }

    #[test]
    fn annotation_with_args_produces_annotation_with_args() {
        let ann = annotation_with_args("deprecated", vec![ann_str("use Foo instead")]);
        assert_eq!(ann.name, "deprecated");
        assert_eq!(ann.args.len(), 1);
        assert_eq!(ann.args[0], AnnotationArg::String("use Foo instead".to_string()));
    }
}
