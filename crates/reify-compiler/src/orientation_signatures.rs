//! Compiler result-type signatures for the two Euler-angle builtins
//! (task #6082): `orient_euler` / `orient_to_euler`.
//!
//! Modelled on `parse_signatures.rs`'s name-family + result-type-resolver
//! pattern (`is_parse_typed_fn` / `parse_fn_result_type`). Like the parse
//! family — and unlike the math-linalg family — the result type here is
//! arg-INDEPENDENT: each name always resolves to the same type regardless of
//! which of the twelve conventions the call selects.
//!
//! # Why the family exists
//!
//! Neither name belonged to any signature module, so both fell through to
//! `expr.rs`'s terminal first-arg fallback, which is literally
//! `compiled_args[0].result_type.clone()` — the type of whatever happens to sit
//! at argument 0. That rule has nothing to do with what either builtin
//! evaluates to, and it was wrong for both in the same way:
//!
//! - `orient_euler(EulerConvention.XYZ, a, b, c)` — arg 0 is the CONVENTION, so
//!   the constructed rotation was typed `Enum("EulerConvention")`, the mode
//!   selector's own type.
//! - `orient_to_euler(q, EulerConvention.XYZ)` — arg 0 was the convention too,
//!   before #6082 flipped the decomposer subject-first; the flip alone would
//!   have merely re-aimed the fallback at `Orientation(3)`, trading one wrong
//!   static type for another.
//!
//! # Why not a `.ri pub fn` declaration
//!
//! A `.ri` fn declaration with a body is the one place a full
//! arguments-plus-return signature is directly expressible, but it is
//! forbidden here: `crates/reify-compiler/stdlib/geometry_traits.ri` documents
//! that such a declaration becomes a `CompiledFunction` dispatched through
//! `eval_user_function_call` (pure value eval), which would SHADOW the Rust
//! `eval_builtin` arm in `reify-stdlib/src/orientation.rs` and break evaluation
//! outright. The sanctioned route is the two-seam split #6082 uses: argument
//! types via `builtin_signatures::builtin_arg_slots`, return type via this
//! module plus an `expr.rs` ladder arm.
//!
//! # Scope
//!
//! Deliberately exactly these two names, not all `orient_*`. The remaining
//! mistyped orientation builtins (`orient_log`, `orient_to_axis_angle`,
//! `orient_compose`, …) are part of the fallback population owned by task
//! #6004's signature registry (`docs/prds/v0_6/builtin-signature-registry.md`);
//! claiming them here would pre-empt that family and enlarge the reciprocal
//! disjointness churn in `units.rs` for no gain.

use reify_core::Type;

/// The complete set of Euler-angle builtin names the compiler types via
/// [`orientation_euler_result_type`]. Single source of truth — mirrors
/// `PARSE_FN_NAMES` in `parse_signatures.rs`.
pub const ORIENTATION_EULER_FN_NAMES: &[&str] = &["orient_euler", "orient_to_euler"];

/// Is `name` a Euler-angle builtin typed by [`orientation_euler_result_type`]?
/// Name-only classification, mirroring `parse_signatures::is_parse_typed_fn` (a
/// `.contains` over the single-source-of-truth slice). Case-sensitive: Reify
/// function names are snake_case.
pub(crate) fn is_orientation_euler_fn(name: &str) -> bool {
    ORIENTATION_EULER_FN_NAMES.contains(&name)
}

/// Result type for a Euler-angle builtin. Arg-independent — reify-stdlib's
/// `eval_builtin` arms (`crates/reify-stdlib/src/orientation.rs`) return the
/// same SHAPE for a given name regardless of which convention is selected:
///
/// - `orient_euler` — a CONSTRUCTOR: composes the three angles about the
///   convention's body axes into one rotation, so → `Type::Orientation(3)`.
/// - `orient_to_euler` — a DECOMPOSER: returns the three angles that rebuild
///   the rotation under the same convention, evaluated as a 3-element
///   `Value::List` of `DimensionVector::ANGLE` scalars, so →
///   `Type::List(Box::new(Type::angle()))`.
///
/// Only reached for the two Euler names (the caller gates on
/// [`is_orientation_euler_fn`]); the `_` arm is therefore unreachable in
/// practice and returns `Type::Orientation(3)` as a harmless default — the
/// broader of the two, and the type any future `orient_*` name added to the
/// slice would most likely want. The
/// `every_orientation_euler_fn_name_maps_to_its_declared_result_type` test
/// below pins each name's arm so a name added without one cannot silently
/// absorb this default.
pub(crate) fn orientation_euler_result_type(name: &str) -> Type {
    match name {
        "orient_euler" => Type::Orientation(3),
        "orient_to_euler" => Type::List(Box::new(Type::angle())),
        _ => Type::Orientation(3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_orientation_euler_fn` recognises both Euler builtin names.
    #[test]
    fn is_orientation_euler_fn_recognises_both_names() {
        assert!(is_orientation_euler_fn("orient_euler"));
        assert!(is_orientation_euler_fn("orient_to_euler"));
    }

    /// `is_orientation_euler_fn` rejects sibling `orient_*` builtins, other
    /// families, the empty name, and an unrelated name.
    ///
    /// The `orient_*` siblings are the point: this family is deliberately
    /// narrow, and claiming `orient_log` / `orient_to_axis_angle` /
    /// `orient_compose` here would pre-empt task #6004's registry.
    #[test]
    fn is_orientation_euler_fn_rejects_siblings_and_unknown_names() {
        assert!(
            !is_orientation_euler_fn("orient_log"),
            "must reject sibling orient_log (task #6004's registry owns it)"
        );
        assert!(
            !is_orientation_euler_fn("orient_to_axis_angle"),
            "must reject sibling orient_to_axis_angle — note the shared \
             `orient_to_` prefix: this family matches whole names, not prefixes"
        );
        assert!(
            !is_orientation_euler_fn("orient_compose"),
            "must reject sibling orient_compose"
        );
        assert!(
            !is_orientation_euler_fn("orient_inverse"),
            "must reject sibling orient_inverse"
        );
        assert!(
            !is_orientation_euler_fn("parse_length"),
            "must reject parse-family 'parse_length'"
        );
        assert!(!is_orientation_euler_fn(""), "must reject empty name");
        assert!(
            !is_orientation_euler_fn("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so
    /// PascalCase/SCREAMING_SNAKE forms must not match (mirrors
    /// `is_parse_typed_fn_is_case_sensitive`).
    #[test]
    fn is_orientation_euler_fn_is_case_sensitive() {
        assert!(!is_orientation_euler_fn("Orient_Euler"));
        assert!(!is_orientation_euler_fn("ORIENT_TO_EULER"));
    }

    /// `ORIENTATION_EULER_FN_NAMES` is exactly the two Euler names — membership
    /// both ways plus an exact count (mirrors
    /// `parse_fn_names_are_exactly_the_two`).
    #[test]
    fn orientation_euler_fn_names_are_exactly_the_two() {
        const EXPECTED: [&str; 2] = ["orient_euler", "orient_to_euler"];
        assert_eq!(
            ORIENTATION_EULER_FN_NAMES.len(),
            EXPECTED.len(),
            "ORIENTATION_EULER_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED.len(),
            ORIENTATION_EULER_FN_NAMES
        );
        for name in EXPECTED {
            assert!(
                ORIENTATION_EULER_FN_NAMES.contains(&name),
                "ORIENTATION_EULER_FN_NAMES must contain {name:?}"
            );
        }
    }

    /// The constructor composes to a rotation.
    #[test]
    fn orient_euler_result_type_is_orientation3() {
        assert_eq!(
            orientation_euler_result_type("orient_euler"),
            Type::Orientation(3)
        );
    }

    /// The decomposer returns the three angles, dimensioned.
    ///
    /// `Type::angle()` — not a bare dimensionless scalar — is the load-bearing
    /// half: it is what lets the result flow into a `List<Angle>` parameter,
    /// which is the user-observable signal task #6082 exists to deliver.
    #[test]
    fn orient_to_euler_result_type_is_list_of_angle() {
        assert_eq!(
            orientation_euler_result_type("orient_to_euler"),
            Type::List(Box::new(Type::angle()))
        );
    }

    /// Every name in `ORIENTATION_EULER_FN_NAMES` must have its OWN arm in
    /// `orientation_euler_result_type`. Pins against the unreachable `_`
    /// default silently absorbing a future name added to the slice without a
    /// matching arm — the same failure mode
    /// `every_parse_fn_name_maps_to_a_non_string_result_type` guards in the
    /// parse family. Here the default is a real type rather than a suspicious
    /// one, so "differs from the default" is not a usable proxy; the check is
    /// instead an explicit per-name expectation table, which fails to compile
    /// (not merely to assert) if the family grows without being considered.
    #[test]
    fn every_orientation_euler_fn_name_maps_to_its_declared_result_type() {
        let expected: [(&str, Type); 2] = [
            ("orient_euler", Type::Orientation(3)),
            ("orient_to_euler", Type::List(Box::new(Type::angle()))),
        ];
        assert_eq!(
            expected.len(),
            ORIENTATION_EULER_FN_NAMES.len(),
            "the expectation table must cover every ORIENTATION_EULER_FN_NAMES \
             entry — a name was added to the slice without a declared result type"
        );
        for (name, want) in expected {
            assert!(
                ORIENTATION_EULER_FN_NAMES.contains(&name),
                "expectation table names {name:?}, which is not in \
                 ORIENTATION_EULER_FN_NAMES"
            );
            assert_eq!(
                orientation_euler_result_type(name),
                want,
                "orientation_euler_result_type({name:?}) must be its declared \
                 type, not the unreachable `_` default"
            );
        }
    }
}
