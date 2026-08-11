//! End-to-end surface tests for the `EulerConvention` enum and the two Euler
//! builtins it selects for (task #6082).
//!
//! Two findings drive this file:
//!
//! * **F1 — the enum under-declared eval by six variants.** `orient_euler` and
//!   `orient_to_euler` both dispatch on TWELVE rotation-sequence conventions
//!   (`crates/reify-stdlib/src/orientation.rs` — the axis table in the
//!   `orient_euler` arm and the per-convention arms of `orient_to_euler`), but
//!   `stdlib/geometry_traits.ri` declared only the six Tait-Bryan sequences.
//!   The six proper/classic Euler sequences (`XYX`, `XZX`, `YXY`, `YZY`,
//!   `ZXZ`, `ZYZ`) were therefore a HARD COMPILE ERROR at the qualified-enum
//!   surface (`expr.rs` validates the variant against
//!   `reify_ir::EnumDef::contains_variant`) and reachable only through the
//!   untyped string path.
//!
//! * **F3 — `orient_to_euler` was not subject-first.** It took
//!   `(convention, q)` while every sibling decomposer (`orient_log(q)`,
//!   `orient_to_axis_angle(q)`, `orient_inverse(q)`, `transform_log(t)`) takes
//!   the subject at argument 0.
//!
//! Assertions here follow house style: the compile-surface tests assert on the
//! ABSENCE of `Severity::Error` diagnostics rather than pinning message
//! substrings, and `compile_source_with_stdlib` is used (not
//! `parse_and_compile_with_stdlib`, which asserts `errors.is_empty()` itself
//! and would panic before the assertion could report which variants failed).

use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The twelve rotation-sequence conventions implemented by eval: six Tait-Bryan
/// (three distinct axes) followed by six proper/classic Euler (first axis
/// repeated as third).
const ALL_TWELVE: &[&str] = &[
    "XYZ", "XZY", "YXZ", "YZX", "ZXY", "ZYX", // Tait-Bryan
    "XYX", "XZX", "YXY", "YZY", "ZXZ", "ZYZ", // proper / classic Euler
];

/// Build a `.ri` module binding one `orient_euler` call per convention, each
/// selected via the qualified enum value `EulerConvention.<VARIANT>`.
fn twelve_variant_source() -> String {
    let mut src = String::from("structure def EulerSurface {\n");
    for (i, variant) in ALL_TWELVE.iter().enumerate() {
        src.push_str(&format!(
            "    let q{i} = orient_euler(EulerConvention.{variant}, 0.1, 0.2, 0.3)\n"
        ));
    }
    src.push_str("}\n");
    src
}

/// F1: every one of the twelve conventions eval dispatches on must resolve at
/// compile time as a qualified `EulerConvention` variant.
///
/// RED before the declaration is widened: the six proper-Euler variants each
/// produce an Error ("unknown variant '<V>' on enum 'EulerConvention'").
#[test]
fn all_twelve_euler_conventions_resolve_at_compile_time() {
    let module = compile_source_with_stdlib(&twelve_variant_source());
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected all {} EulerConvention variants to resolve, got {} error(s): {:?}",
        ALL_TWELVE.len(),
        errors.len(),
        errors
    );
}

/// Per-variant isolation of the same signal: compiling ONE variant at a time
/// means a failure names the exact variant that is missing from the
/// declaration, rather than reporting an aggregate count.
#[test]
fn each_euler_convention_variant_resolves_individually() {
    for variant in ALL_TWELVE {
        let source = format!(
            "structure def EulerOne {{\n    let q = orient_euler(EulerConvention.{variant}, 0.1, 0.2, 0.3)\n}}\n"
        );
        let module = compile_source_with_stdlib(&source);
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "EulerConvention.{variant} failed to resolve: {errors:?}"
        );
    }
}
