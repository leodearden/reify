//! Hand-built stdlib-input fixtures shared by the trampolines' decode tests,
//! and the one assertion those tests make about a decode failure.

use reify_core::{DiagnosticCode, DimensionVector, Severity};
use reify_ir::Value;

use crate::ComputeOutcome;
use crate::compute_targets::{scalar, structure};

/// A registry-free material instance of a CONCRETE conformer type, never
/// `ElasticMaterial` itself, so a path that names the trait proves the
/// decoder named it.
pub(crate) fn hand_built_material(fields: Vec<(&str, Value)>) -> Value {
    structure("HandBuiltSteel", fields)
}

pub(crate) fn steel_youngs_modulus() -> Value {
    scalar(2.0e11, DimensionVector::PRESSURE)
}

pub(crate) fn steel_density() -> Value {
    scalar(7850.0, DimensionVector::MASS_DENSITY)
}

/// A material with `youngs_modulus` and `density` but no `poisson_ratio`.
pub(crate) fn material_missing_poisson_ratio() -> Value {
    hand_built_material(vec![
        ("youngs_modulus", steel_youngs_modulus()),
        ("density", steel_density()),
    ])
}

/// A complete material whose `youngs_modulus` is a LENGTH.
pub(crate) fn material_with_length_youngs_modulus() -> Value {
    hand_built_material(vec![
        ("youngs_modulus", scalar(2.0e11, DimensionVector::LENGTH)),
        ("poisson_ratio", Value::Real(0.29)),
        ("density", steel_density()),
    ])
}

/// `outcome` is `Failed` with exactly one diagnostic: an Error carrying
/// `code` whose message names `path`.
pub(crate) fn assert_one_coded_error(outcome: &ComputeOutcome, code: DiagnosticCode, path: &str) {
    let ComputeOutcome::Failed { diagnostics, .. } = outcome else {
        panic!("expected Failed with one {code:?} naming {path}, got {outcome:?}");
    };
    let [diagnostic] = diagnostics.as_slice() else {
        panic!("expected exactly one diagnostic naming {path}, got {diagnostics:?}");
    };
    assert_eq!(diagnostic.severity, Severity::Error, "{diagnostic:?}");
    assert_eq!(diagnostic.code, Some(code), "{diagnostic:?}");
    assert!(
        diagnostic.message.contains(path),
        "the message must name {path}: {:?}",
        diagnostic.message
    );
}
