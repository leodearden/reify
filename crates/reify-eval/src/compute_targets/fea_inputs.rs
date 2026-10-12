//! The FEA leaf input readers shared by the `solve_elastic_static` and
//! `solve_buckling` trampolines: an SI scalar, a structure's scalar / real
//! field, and the isotropic material. Each reads into a `Result` whose `Err` is
//! a [`FeaValueShapeError`], so a malformed input never panics.

use reify_ir::{StructureInstanceData, Value};
use reify_solver_elastic::IsotropicElastic;

use super::fea_value_shape::FeaValueShapeError;

/// Read a `Value::Scalar { si_value, .. }` field from a StructureInstance.
pub(super) fn scalar_si_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    match data.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => Ok(*si_value),
        // `context` stays the fixed function-name label per the PRD §C3
        // contract (`ExpectedScalar { context: &'static str, got: String }`,
        // shared with D3/D5/D7/D8/D9); `got`'s wording is unpinned (open
        // question 2), so the offending field key rides along in `got`
        // instead so a `panic!("{e}")`/eventual D9 diagnostic can name it.
        Some(other) => Err(FeaValueShapeError::ExpectedScalar {
            context: "scalar_si_field",
            got: format!("{other:?} (field {key:?})"),
        }),
        None => Err(FeaValueShapeError::MissingField {
            context: "scalar_si_field",
            field: key,
        }),
    }
}

/// Read a `Value::Real` field from a StructureInstance.
pub(super) fn real_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    match data.fields.get(key) {
        Some(Value::Real(r)) => Ok(*r),
        // See `scalar_si_field`'s comment above: `context` stays the fixed
        // function-name label; the field key rides in `got` instead so it
        // isn't lost for the eventual D9 diagnostic.
        Some(other) => Err(FeaValueShapeError::ExpectedReal {
            context: "real_field",
            got: format!("{other:?} (field {key:?})"),
        }),
        None => Err(FeaValueShapeError::MissingField {
            context: "real_field",
            field: key,
        }),
    }
}

/// Read `youngs_modulus`/`poisson_ratio` off an already-destructured
/// `StructureInstanceData` into `IsotropicElastic`. The data-taking core
/// behind `extract_material` (below), and behind `law_from_value`'s
/// isotropic fallback arm, which already holds the law's destructured
/// `&StructureInstanceData` and calls straight in here — taking a second,
/// independently-matched `&Value` there instead (as the pre-amendment
/// `extract_material(law_val)` call did) would let a future caller pass a
/// `law_data`/`val` pair describing two DIFFERENT `StructureInstance`s with
/// no error, silently reading the wrong one's fields (task #7210 review
/// round 2 suggestion 1).
///
/// Note (task #5081 review round 3, suggestion 2): `scalar_si_field` accepts
/// any `Value::Scalar` for `youngs_modulus` regardless of its `dimension`
/// (e.g. a length or dimensionless scalar would be silently accepted as a
/// pressure). This is pre-existing behavior, not a D3 regression — dimension
/// checking is deferred to a later D-step, if/when the `FeaValueShapeError`
/// taxonomy grows a variant for it.
pub(super) fn isotropic_from_data(
    data: &StructureInstanceData,
) -> Result<IsotropicElastic, FeaValueShapeError> {
    let youngs_modulus = scalar_si_field(data, "youngs_modulus")?;
    let poisson_ratio = real_field(data, "poisson_ratio")?;
    Ok(IsotropicElastic {
        youngs_modulus,
        poisson_ratio,
    })
}

/// Extract `IsotropicElastic` from a `Value::StructureInstance` carrying
/// `youngs_modulus: Scalar<Pressure>` and `poisson_ratio: Real`. Thin
/// `&Value`-destructuring wrapper around `isotropic_from_data`.
///
/// PRD compute-fea-hardening D3: Result-ified leaf extractor. Returns
/// `Err(FeaValueShapeError)` instead of panicking on a malformed `Value`;
/// the sole call site (`classify_material`'s isotropic fallback, D7) now
/// propagates this `Result` directly via `?`; `classify_material`'s own sole
/// production call site is the validate-all-inputs gate (D9/task 5087) in
/// `solve_elastic_static_trampoline`.
pub(super) fn extract_material(val: &Value) -> Result<IsotropicElastic, FeaValueShapeError> {
    let data = match val {
        Value::StructureInstance(d) => d,
        other => {
            return Err(FeaValueShapeError::ExpectedStructureInstance {
                context: "extract_material",
                got: format!("{other:?}"),
            });
        }
    };
    isotropic_from_data(data)
}

/// Extract SI scalar value from `Value::Scalar { si_value, .. }`.
pub(super) fn extract_scalar_si(val: &Value) -> Result<f64, FeaValueShapeError> {
    match val {
        Value::Scalar { si_value, .. } => Ok(*si_value),
        other => Err(FeaValueShapeError::ExpectedScalar {
            context: "extract_scalar_si",
            got: format!("{other:?}"),
        }),
    }
}
