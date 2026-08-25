//! Shared validation-message constants for geometry kernel operations.
//!
//! Both `reify-kernel-fidget` and `reify-kernel-occt` must emit byte-identical
//! error messages for Sphere radius and Box dimension validation so that
//! callers (tests, log parsers, UI) can match on a single string regardless of
//! which kernel is active.  The constants here are the single source of truth:
//!
//! - Fidget production site: `crates/reify-kernel-fidget/src/kernel.rs` —
//!   `execute(Sphere)` and `execute(Box)` arms.
//! - OCCT production site: `crates/reify-kernel-occt/src/lib.rs` —
//!   `OcctKernel::execute` `Sphere` and `Box` arms.
//!
//! Every test that asserts the error message should `assert_eq!` against these
//! constants rather than using substring containment, so that message drift
//! between the two kernels is caught at compile time rather than by accident.

/// Error message emitted when a Sphere `radius` value fails the
/// finite-and-strictly-positive check.
///
/// Byte-identical across fidget and OCCT kernels; both must reference this
/// constant rather than inlining a literal.
pub const SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE: &str =
    "sphere radius must be a finite positive value";

/// Error message emitted when any Box dimension (`width`, `height`, or `depth`)
/// fails the finite-and-strictly-positive check.
///
/// Note the plural "values": all three dimensions are validated in a single
/// combined check, so a single message covers any dimension failure.
/// Byte-identical across fidget and OCCT kernels; both must reference this
/// constant rather than inlining a literal.
pub const BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE: &str =
    "box dimensions must be finite positive values";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use reify_core::DimensionVector;

    /// The five non-LENGTH values a kernel length field can receive.
    ///
    /// `Real`/`Int` are the bare-literal case the C4 tripwire exists to
    /// detect; `Scalar{MASS}` is a *dimensioned but wrong* value;
    /// `String`/`Undef` are the non-numeric cases that also fail
    /// `Value::as_f64`.
    fn non_length_values() -> Vec<Value> {
        vec![
            Value::Real(1.0),
            Value::Int(1),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::String("x".into()),
            Value::Undef,
        ]
    }

    /// The legacy context-free kernel error string that C4 forbids on a
    /// length field ("never a bare `expected numeric value`").
    const LEGACY_BARE: &str = "expected numeric value";

    /// A properly dimensioned LENGTH value is not a violation.
    #[test]
    fn dimensioned_length_value_is_not_a_violation() {
        assert_eq!(
            check_length_field("Fillet", "radius", &Value::length(0.001)),
            None
        );
    }

    /// Every non-LENGTH value is a violation whose message names BOTH the op
    /// kind and the field — boundary rows 13/14's message contract, stated
    /// once and profile-independently.
    #[test]
    fn every_non_length_value_is_a_violation_naming_op_kind_and_field() {
        for v in non_length_values() {
            let msg = check_length_field("Fillet", "radius", &v)
                .unwrap_or_else(|| panic!("expected a violation for {v:?}"));
            assert!(
                msg.contains("Fillet"),
                "message for {v:?} does not name the op kind: {msg}"
            );
            assert!(
                msg.contains("radius"),
                "message for {v:?} does not name the field: {msg}"
            );
        }
    }

    /// C4: the length-field diagnostic is never the bare legacy string.
    #[test]
    fn tripwire_message_is_never_the_bare_legacy_string() {
        for v in non_length_values() {
            let msg = check_length_field("Fillet", "radius", &v)
                .unwrap_or_else(|| panic!("expected a violation for {v:?}"));
            assert_ne!(msg, LEGACY_BARE);
            assert!(
                !msg.contains(LEGACY_BARE),
                "message for {v:?} still carries the bare legacy string: {msg}"
            );
        }
        assert!(!non_numeric_kernel_field_message("Fillet", "radius").contains(LEGACY_BARE));
    }

    /// Both formatters name op kind and field, and `check_length_field`'s
    /// message is EXACTLY `non_length_kernel_field_message`'s output — the
    /// `assert_eq!`-against-a-single-source doctrine this module's docs
    /// mandate, so kernel-side drift is a compile/test failure, not an
    /// accident of substring containment.
    #[test]
    fn formatters_name_op_kind_and_field_and_anchor_the_classifier() {
        let non_length = non_length_kernel_field_message("Extrude", "distance", "Real");
        assert!(non_length.contains("Extrude"), "{non_length}");
        assert!(non_length.contains("distance"), "{non_length}");

        let non_numeric = non_numeric_kernel_field_message("Extrude", "distance");
        assert!(non_numeric.contains("Extrude"), "{non_numeric}");
        assert!(non_numeric.contains("distance"), "{non_numeric}");

        // Single-source anchor: the classifier returns the formatter's own
        // output verbatim.
        assert_eq!(
            check_length_field("Extrude", "distance", &Value::Real(1.0)),
            Some(non_length_kernel_field_message("Extrude", "distance", "Real"))
        );
    }

    /// Table-driven over four distinct (op kind, field) pairs, so a formatter
    /// that hardcodes a single op or field cannot pass.
    #[test]
    fn formatters_are_parameterised_over_op_kind_and_field() {
        let pairs = [
            ("Box", "width"),
            ("Fillet", "radius"),
            ("Extrude", "distance"),
            ("Shell", "thickness"),
        ];
        for (op_kind, field) in pairs {
            let msg = check_length_field(op_kind, field, &Value::Real(1.0))
                .unwrap_or_else(|| panic!("expected a violation for {op_kind}.{field}"));
            assert!(msg.contains(op_kind), "{op_kind}.{field}: {msg}");
            assert!(msg.contains(field), "{op_kind}.{field}: {msg}");
            assert_eq!(
                msg,
                non_length_kernel_field_message(op_kind, field, "Real"),
                "classifier message drifted from the shared formatter"
            );
            let non_numeric = non_numeric_kernel_field_message(op_kind, field);
            assert!(non_numeric.contains(op_kind), "{op_kind}.{field}: {non_numeric}");
            assert!(non_numeric.contains(field), "{op_kind}.{field}: {non_numeric}");
        }

        // Distinct pairs must produce distinct messages — a formatter that
        // ignores its arguments would collapse them.
        let a = non_length_kernel_field_message("Box", "width", "Real");
        let b = non_length_kernel_field_message("Shell", "thickness", "Real");
        assert_ne!(a, b);
    }

    /// The `got` label distinguishes the observed variant, and names the
    /// dimension for a dimensioned-but-wrong `Scalar`.
    #[test]
    fn violation_message_labels_the_observed_value() {
        let real = check_length_field("Box", "width", &Value::Real(1.0)).unwrap();
        let int = check_length_field("Box", "width", &Value::Int(1)).unwrap();
        assert_ne!(
            real, int,
            "Real and Int must not share a `got` label: {real}"
        );

        let mass = check_length_field(
            "Box",
            "width",
            &Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
        )
        .unwrap();
        assert!(
            mass.contains("Mass"),
            "a dimensioned-but-wrong Scalar must name its dimension: {mass}"
        );
    }
}
