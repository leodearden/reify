//! Static argument-type gate for the two Euler builtins' convention slot
//! (task #6082, Finding 2's user-visible half).
//!
//! Task #6082 removed the raw lowercase-string convention path from
//! `orient_euler` / `orient_to_euler`, leaving the qualified `EulerConvention`
//! enum value as the sole accepted form. Removing the eval arm alone is only
//! half the fix: with no registered argument slot, a String convention is
//! invisible to the compiler and the user's first signal is a value that
//! quietly evaluated to `Undef`. This file pins the compile-time half —
//! a String convention must raise `DiagnosticCode::ArgTypeMismatch`.
//!
//! RED before the slots are registered: `builtin_arg_slots` falls to its empty
//! `_ =>` arm for both names, so no diagnostic is emitted at all.
//!
//! House style, as used by `coupling_motionvalue_integration_gate.rs`:
//!   - assert on `DiagnosticCode`, never on message substrings;
//!   - ship an `_ok` twin with every `_reject` fixture, as the
//!     no-false-positive control;
//!   - assert an EXACT Error count, so a type cascade cannot be mistaken for
//!     the intended diagnostic;
//!   - use `compile_source_with_stdlib`, NOT `parse_and_compile_with_stdlib`
//!     (the latter asserts `errors.is_empty()` itself and would panic on the
//!     reject fixture before any assertion here could report what it saw).

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source_with_stdlib, errors_only, warnings_only};

// ── Fixtures ──────────────────────────────────────────────────────────────────

const EULER_ARG_OK: &str = include_str!("fixtures/euler_convention_arg_ok.ri");
const EULER_ARG_REJECT: &str = include_str!("fixtures/euler_convention_arg_reject.ri");

// ── REJECT: a String convention is an ArgTypeMismatch ─────────────────────────

/// Both String conventions must be diagnosed, and nothing else must be.
///
/// The exact count is the anti-cascade assertion: two calls, two Errors. A
/// third Error would mean the mismatch poisoned downstream inference instead of
/// staying the pure diagnostic side-effect `check_builtin_arg_types` is
/// documented to be.
#[test]
fn string_convention_emits_exactly_two_arg_type_mismatches() {
    let module = compile_source_with_stdlib(EULER_ARG_REJECT);
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        2,
        "expected exactly 2 Errors (one per String convention), got {}: {:?}",
        errors.len(),
        errors
    );
    for diagnostic in &errors {
        assert_eq!(
            diagnostic.code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "every Error must be an ArgTypeMismatch — a different code means the \
             convention slot is not what fired: {diagnostic:?}"
        );
    }
}

/// The constructor's convention slot is arg 0; the decomposer's is arg 1. Each
/// must account for exactly one of the two diagnostics, so a single slot
/// registered twice (or one name left unregistered) cannot pass the count
/// assertion above by accident.
#[test]
fn each_euler_builtin_contributes_one_mismatch() {
    for (source, builtin) in [
        (
            "structure def OrientEulerOnly {\n    \
             let q = orient_euler(\"xyz\", 0.1, 0.2, 0.3)\n}\n",
            "orient_euler",
        ),
        (
            "structure def OrientToEulerOnly {\n    \
             let angles = orient_to_euler(orient_identity(), \"XYZ\")\n}\n",
            "orient_to_euler",
        ),
    ] {
        let module = compile_source_with_stdlib(source);
        let errors = errors_only(&module);
        assert_eq!(
            errors.len(),
            1,
            "{builtin} with a String convention should emit exactly 1 Error, got {}: {:?}",
            errors.len(),
            errors
        );
        assert_eq!(
            errors[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "{builtin}: expected ArgTypeMismatch, got {:?}",
            errors[0]
        );
    }
}

// ── OK: the enum form is clean ────────────────────────────────────────────────

/// The no-false-positive control. The same two calls, with the qualified enum
/// value, must compile with zero Errors AND zero Warnings — a slot that
/// rejected the very form it exists to require would be worse than no slot.
#[test]
fn qualified_enum_convention_compiles_clean() {
    let module = compile_source_with_stdlib(EULER_ARG_OK);

    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "the qualified EulerConvention form must compile without Errors, got {}: {:?}",
        errors.len(),
        errors
    );

    let warnings = warnings_only(&module);
    assert!(
        warnings.is_empty(),
        "the qualified EulerConvention form must compile without Warnings, got {}: {:?}",
        warnings.len(),
        warnings
    );
}

/// All twelve conventions pass the slot, not just the one the fixture spells.
/// Guards against a slot that accidentally pins a single variant rather than
/// the enum type.
#[test]
fn every_convention_variant_passes_the_slot() {
    const ALL_TWELVE: &[&str] = &[
        "XYZ", "XZY", "YXZ", "YZX", "ZXY", "ZYX", // Tait-Bryan
        "XYX", "XZX", "YXY", "YZY", "ZXZ", "ZYZ", // proper / classic Euler
    ];
    for variant in ALL_TWELVE {
        let source = format!(
            "structure def EulerSlotOne {{\n    \
             let q      = orient_euler(EulerConvention.{variant}, 0.1, 0.2, 0.3)\n    \
             let angles = orient_to_euler(q, EulerConvention.{variant})\n}}\n"
        );
        let module = compile_source_with_stdlib(&source);
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "EulerConvention.{variant} must pass the convention slot, got {errors:?}"
        );
    }
}
