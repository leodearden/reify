//! Compiler signatures for the fallible string→quantity parse builtins
//! (task #4535): `parse_length` / `parse_length_r`.
//!
//! Mirrors `math_signatures.rs`'s name-family + result-type-resolver pattern
//! (`is_math_typed_fn` / `math_fn_result_type`), but the parse family's result
//! type is arg-INDEPENDENT — no shape/dimension algebra is needed, since both
//! names always resolve to the same result type regardless of the (String)
//! argument's value.
//!
//! Wired into `expr.rs`'s `NoUserFunctions` result-type ladder (the
//! `is_parse_typed_fn` arm) ahead of the first-arg fallback, which would
//! otherwise type both calls as the arg's own `Type::String` — breaking the
//! consumer's `match{Some/None}`/`{Ok/Err}` type-check and the eval-time
//! `value_type_kind_matches` guard (the eval'd value is a real
//! `Value::Option` / `Value::Enum`, never a `Value::String`).

use reify_core::Type;

/// The complete set of fallible parse builtin names recognised by the
/// compiler. Single source of truth — mirrors `MATH_CONSTRUCTION_NAMES` /
/// `MATH_OPERATION_NAMES` in `math_signatures.rs`.
pub const PARSE_FN_NAMES: &[&str] = &["parse_length", "parse_length_r"];

/// Is `name` a fallible parse builtin the compiler types via
/// [`parse_fn_result_type`]? Name-only classification, mirroring
/// `math_signatures::is_math_typed_fn` (a `.contains` over the
/// single-source-of-truth slice). Case-sensitive: Reify function names are
/// snake_case.
pub(crate) fn is_parse_typed_fn(name: &str) -> bool {
    PARSE_FN_NAMES.contains(&name)
}

/// Result type for a fallible parse builtin. Arg-independent — reify-stdlib's
/// `eval_parse` (`crates/reify-stdlib/src/parse.rs`) always returns the same
/// VARIANT for a given name regardless of the (String) argument's value:
///
/// - `parse_length`   → `Option<Length>`, i.e. `Type::Option(Box::new(Type::length()))`.
/// - `parse_length_r` → the PRELUDE `Result<T,E>` (dependency task #4035),
///   registered as `Type::Enum("Result")` — matching how a landed
///   `Ok{..}`/`Err{..}` construction's cell types
///   (`result_prelude_enum_tests.rs`), so the eval-time
///   `value_type_kind_matches` guard passes against the
///   `Value::Enum{type_name: "Result", ..}` that `eval_parse` constructs.
///
/// Only reached for the two parse names (the caller gates on
/// [`is_parse_typed_fn`]); the `_` arm is therefore unreachable in practice
/// and returns a harmless `Type::String` (the pre-wiring first-arg fallback's
/// type, for a defensive default).
pub(crate) fn parse_fn_result_type(name: &str) -> Type {
    match name {
        "parse_length" => Type::Option(Box::new(Type::length())),
        "parse_length_r" => Type::Enum("Result".to_string()),
        _ => Type::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_parse_typed_fn` recognises both parse builtin names.
    #[test]
    fn is_parse_typed_fn_recognises_both_names() {
        assert!(is_parse_typed_fn("parse_length"));
        assert!(is_parse_typed_fn("parse_length_r"));
    }

    /// `is_parse_typed_fn` rejects names from other builtin families, the
    /// empty name, and an unrelated name.
    #[test]
    fn is_parse_typed_fn_rejects_other_family_and_unknown_names() {
        assert!(!is_parse_typed_fn("sqrt"), "must reject math-linalg 'sqrt'");
        assert!(
            !is_parse_typed_fn("volume"),
            "must reject geometry-query 'volume'"
        );
        assert!(!is_parse_typed_fn(""), "must reject empty name");
        assert!(
            !is_parse_typed_fn("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so
    /// PascalCase/SCREAMING_SNAKE forms must not match (mirrors
    /// `is_math_typed_fn_is_case_sensitive`).
    #[test]
    fn is_parse_typed_fn_is_case_sensitive() {
        assert!(!is_parse_typed_fn("Parse_Length"));
        assert!(!is_parse_typed_fn("PARSE_LENGTH"));
    }

    /// `PARSE_FN_NAMES` is exactly the two parse names — membership both ways
    /// plus an exact count (mirrors `math_construction_names_are_exactly_the_six`).
    #[test]
    fn parse_fn_names_are_exactly_the_two() {
        const EXPECTED: [&str; 2] = ["parse_length", "parse_length_r"];
        assert_eq!(
            PARSE_FN_NAMES.len(),
            EXPECTED.len(),
            "PARSE_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED.len(),
            PARSE_FN_NAMES
        );
        for name in EXPECTED {
            assert!(
                PARSE_FN_NAMES.contains(&name),
                "PARSE_FN_NAMES must contain {name:?}"
            );
        }
    }

    /// `parse_length` resolves to `Option<Length>`.
    #[test]
    fn parse_length_result_type_is_option_length() {
        assert_eq!(
            parse_fn_result_type("parse_length"),
            Type::Option(Box::new(Type::length()))
        );
    }

    /// `parse_length_r` resolves to the PRELUDE `Result<T,E>`, registered as
    /// `Type::Enum("Result")` (dependency task #4035's shape).
    #[test]
    fn parse_length_r_result_type_is_enum_result() {
        assert_eq!(
            parse_fn_result_type("parse_length_r"),
            Type::Enum("Result".to_string())
        );
    }

    /// Every name in `PARSE_FN_NAMES` must resolve to a non-`String` result
    /// type. Pins against the `_ => Type::String` fallback silently
    /// absorbing a future name added to `PARSE_FN_NAMES` without a matching
    /// arm in `parse_fn_result_type` — the exact failure mode (a builtin
    /// wrongly typed as `String`) this module exists to prevent (reviewer
    /// suggestion #3, task #4535 amendment round 2).
    #[test]
    fn every_parse_fn_name_maps_to_a_non_string_result_type() {
        for name in PARSE_FN_NAMES {
            let ty = parse_fn_result_type(name);
            assert_ne!(
                ty,
                Type::String,
                "parse_fn_result_type({name:?}) fell through to the \
                 unreachable `_ => Type::String` default — add a matching arm"
            );
        }
    }
}
