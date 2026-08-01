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

use reify_core::{DimensionVector, Severity};

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

// ─── Task #5799 (ruled by Leo 2026-07-29): the SEPARATION guarantee ──────────
//
// A dimension-checked reader keys on the DimensionVector alone: `accept_arg`
// compares `*dimension == spec.dimension` (reify-eval/src/arg_acceptance.rs),
// and `ArgSpec.type_name` is display-only. So two quantities that share a
// vector are indistinguishable to every reader in the system, no matter what
// names `NAMED_DIMENSIONS` hangs off that vector — a name-level alias row could
// never have separated torque from rotational stiffness. The two tests below
// are the executable form of that argument: they demand that the torque
// spelling be REJECTED where a rotational stiffness/damping is declared.
//
// RED before the re-dimensioning, because ROTATIONAL_STIFFNESS and TORQUE are
// byte-identical vectors today, so `1N*m/rad` satisfies the declared type and
// no diagnostic is emitted at all.

/// Compile `structure def S { param x : <ty> = <literal> }` and return its
/// Error-severity diagnostics.
///
/// `common::stdlib_param_si_value` cannot be used for a negative test — it
/// asserts `errs.is_empty()` internally and would panic on the very diagnostic
/// these tests are looking for.
fn param_errors(param_type: &str, literal: &str) -> Vec<String> {
    let source = format!(
        "structure def S {{ param x : {} = {} }}",
        param_type, literal
    );
    common::compile_with_stdlib_helper(&source)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// `1N*m/rad` is a TORQUE, not a rotational stiffness — declaring it as one
/// must be an error.
///
/// Task #5799, ruled by Leo 2026-07-29. Asserts on `Severity::Error` presence
/// rather than on message wording, so a future rephrasing of the
/// declared-vs-initializer diagnostic does not break this contract.
#[test]
fn rotational_stiffness_param_rejects_the_torque_spelling() {
    let errs = param_errors("RotationalStiffness", "1N*m/rad");
    assert!(
        !errs.is_empty(),
        "`param x : RotationalStiffness = 1N*m/rad` must be rejected: N·m/rad is a \
         Torque (rad⁻¹), while a rotational stiffness is N·m/rad² (rad⁻²). If this \
         is accepted, the two quantities share a DimensionVector and no \
         dimension-checked reader can tell them apart. Task #5799, ruled 2026-07-29."
    );
}

/// `1N*m*s/rad` is a torque-per-unit-nothing, not a rotational damping —
/// declaring it as one must be an error.
///
/// Task #5799, ruled by Leo 2026-07-29. The damping counterpart of
/// [`rotational_stiffness_param_rejects_the_torque_spelling`]; ROTATIONAL_DAMPING
/// moves to rad⁻² under the same ruling, so it needs its own separation pin.
#[test]
fn rotational_damping_param_rejects_the_torque_second_spelling() {
    let errs = param_errors("RotationalDamping", "1N*m*s/rad");
    assert!(
        !errs.is_empty(),
        "`param x : RotationalDamping = 1N*m*s/rad` must be rejected: a rotational \
         damping is N·m·s/rad² (rad⁻²), so that c·θ̇ closes on Torque. \
         Task #5799, ruled 2026-07-29."
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
