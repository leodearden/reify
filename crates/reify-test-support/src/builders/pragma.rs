use reify_ast::{Pragma, PragmaArg, PragmaValue};
use reify_core::SourceSpan;

/// Create a `PragmaValue::Ident` with the given identifier string.
pub fn pragma_ident(s: impl Into<String>) -> PragmaValue {
    PragmaValue::Ident(s.into())
}

/// Create a `PragmaValue::Number` with the given float value.
pub fn pragma_number(n: f64) -> PragmaValue {
    PragmaValue::Number(n)
}

/// Create a `PragmaValue::String` with the given string value.
pub fn pragma_string(s: impl Into<String>) -> PragmaValue {
    PragmaValue::String(s.into())
}

/// Create a `PragmaValue::Bool` with the given bool value.
pub fn pragma_bool(b: bool) -> PragmaValue {
    PragmaValue::Bool(b)
}

/// Create a `PragmaArg::KeyValue` with the given key and value.
pub fn pragma_kv(key: impl Into<String>, value: PragmaValue) -> PragmaArg {
    PragmaArg::KeyValue {
        key: key.into(),
        value,
    }
}

/// Create a `PragmaArg::Bare` with the given value.
pub fn pragma_bare(value: PragmaValue) -> PragmaArg {
    PragmaArg::Bare(value)
}

/// Create a `Pragma` with the given name and no arguments.
pub fn pragma(name: impl Into<String>) -> Pragma {
    Pragma {
        name: name.into(),
        args: Vec::new(),
        span: SourceSpan::new(0, 0),
    }
}

/// Create a `Pragma` with the given name and arguments.
pub fn pragma_with_args(name: impl Into<String>, args: Vec<PragmaArg>) -> Pragma {
    Pragma {
        name: name.into(),
        args,
        span: SourceSpan::new(0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pragma_ident_produces_ident_value() {
        let v = pragma_ident("opt_level");
        assert_eq!(v, PragmaValue::Ident("opt_level".to_string()));
    }

    #[test]
    fn pragma_number_produces_number_value() {
        let v = pragma_number(42.0);
        assert_eq!(v, PragmaValue::Number(42.0));
    }

    #[test]
    fn pragma_string_produces_string_value() {
        let v = pragma_string("hello");
        assert_eq!(v, PragmaValue::String("hello".to_string()));
    }

    #[test]
    fn pragma_bool_produces_bool_value() {
        let v = pragma_bool(true);
        assert_eq!(v, PragmaValue::Bool(true));
    }

    #[test]
    fn pragma_kv_produces_key_value_arg() {
        let arg = pragma_kv("level", pragma_number(3.0));
        match arg {
            PragmaArg::KeyValue { key, value } => {
                assert_eq!(key, "level");
                assert_eq!(value, PragmaValue::Number(3.0));
            }
            _ => panic!("expected PragmaArg::KeyValue"),
        }
    }

    #[test]
    fn pragma_bare_produces_bare_arg() {
        let arg = pragma_bare(pragma_bool(false));
        match arg {
            PragmaArg::Bare(v) => assert_eq!(v, PragmaValue::Bool(false)),
            _ => panic!("expected PragmaArg::Bare"),
        }
    }

    #[test]
    fn pragma_produces_empty_args() {
        let p = pragma("inline");
        assert_eq!(p.name, "inline");
        assert!(p.args.is_empty());
    }

    #[test]
    fn pragma_with_args_produces_pragma_with_args() {
        let p = pragma_with_args("optimize", vec![pragma_kv("level", pragma_number(2.0))]);
        assert_eq!(p.name, "optimize");
        assert_eq!(p.args.len(), 1);
    }
}
