//! Compiler-side dimension resolution tests for the four flexure dimensioned types
//! added in task 3849 (Phase-1 of docs/prds/v0_3/compliant-joints-flexures.md).
//!
//! Each test compiles `structure def S { param x : T = literal }` via
//! `common::stdlib_param_si_value` and asserts:
//!   (a) no Error diagnostics (the name resolves as a type),
//!   (b) si_value == 1.0 (the compound literal folds to SI base units), and
//!   (c) the returned DimensionVector equals the corresponding reify_core constant
//!       (the dimension match).

mod common;

use reify_core::DimensionVector;

#[test]
fn rotational_stiffness_param_resolves_and_folds() {
    let (si, dim) = common::stdlib_param_si_value("RotationalStiffness", "1N*m/rad");
    assert_eq!(si, 1.0, "1 N·m/rad should fold to si_value 1.0");
    assert_eq!(
        dim,
        DimensionVector::ROTATIONAL_STIFFNESS,
        "dimension must equal ROTATIONAL_STIFFNESS (kg·m²·s⁻²·rad⁻¹)"
    );
}

#[test]
fn rotational_damping_param_resolves_and_folds() {
    let (si, dim) = common::stdlib_param_si_value("RotationalDamping", "1N*m*s/rad");
    assert_eq!(si, 1.0, "1 N·m·s/rad should fold to si_value 1.0");
    assert_eq!(
        dim,
        DimensionVector::ROTATIONAL_DAMPING,
        "dimension must equal ROTATIONAL_DAMPING (kg·m²·s⁻¹·rad⁻¹)"
    );
}

#[test]
fn translational_stiffness_param_resolves_and_folds() {
    let (si, dim) = common::stdlib_param_si_value("TranslationalStiffness", "1N/m");
    assert_eq!(si, 1.0, "1 N/m should fold to si_value 1.0");
    assert_eq!(
        dim,
        DimensionVector::TRANSLATIONAL_STIFFNESS,
        "dimension must equal TRANSLATIONAL_STIFFNESS (kg·s⁻²)"
    );
}

#[test]
fn translational_damping_param_resolves_and_folds() {
    let (si, dim) = common::stdlib_param_si_value("TranslationalDamping", "1N*s/m");
    assert_eq!(si, 1.0, "1 N·s/m should fold to si_value 1.0");
    assert_eq!(
        dim,
        DimensionVector::TRANSLATIONAL_DAMPING,
        "dimension must equal TRANSLATIONAL_DAMPING (kg·s⁻¹)"
    );
}
