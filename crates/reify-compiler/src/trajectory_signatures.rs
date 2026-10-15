//! Static result types for the scalar-valued trajectory intrinsics (#8119):
//! `profile_duration_at(p)` is a `Time` and `peak_deviation_at(track, loc)` is
//! a `Length`.
//!
//! Without this, both calls fall through to the first-arg fallback and type as
//! their first argument — the `Profile` / `EndEffectorTrack` — so the
//! `std.trajectory` wrappers `profile_duration -> Time` and
//! `peak_deviation -> Length` would have bodies contradicting their returns.
//! The runtime kernels already return scalars (`profile_duration_value` a
//! `Value::Scalar` of dimension TIME, `peak_deviation_at` an SI-metre
//! `Value::Real`), both of which kind-match a `Type::Scalar` cell.
//!
//! The list-valued trajectory intrinsics stay in `EVAL_DEFERRED_BUILTIN_NAMES`
//! under #6007. That registry migration moves these two into `reify-builtins`
//! rows and deletes this module; it must not leave both.

use reify_core::{DimensionVector, Type};

pub(crate) const TRAJECTORY_SCALAR_INTRINSIC_NAMES: &[&str] =
    &["profile_duration_at", "peak_deviation_at"];

/// The result type of a scalar-valued trajectory intrinsic, by exact name;
/// `None` for any other name.
pub(crate) fn trajectory_scalar_intrinsic_result_type(name: &str) -> Option<Type> {
    let dimension = match name {
        "profile_duration_at" => DimensionVector::TIME,
        "peak_deviation_at" => DimensionVector::LENGTH,
        _ => return None,
    };
    Some(Type::Scalar { dimension })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_name_resolves_to_its_scalar_and_the_slice_holds_exactly_them() {
        let fixture = [
            ("profile_duration_at", DimensionVector::TIME),
            ("peak_deviation_at", DimensionVector::LENGTH),
        ];

        for (name, dimension) in fixture {
            assert_eq!(
                trajectory_scalar_intrinsic_result_type(name),
                Some(Type::Scalar { dimension }),
                "{name}"
            );
            assert!(
                TRAJECTORY_SCALAR_INTRINSIC_NAMES.contains(&name),
                "{name} missing from TRAJECTORY_SCALAR_INTRINSIC_NAMES"
            );
        }
        assert_eq!(
            TRAJECTORY_SCALAR_INTRINSIC_NAMES.len(),
            fixture.len(),
            "TRAJECTORY_SCALAR_INTRINSIC_NAMES holds a name the fixture does not: {TRAJECTORY_SCALAR_INTRINSIC_NAMES:?}"
        );
    }

    #[test]
    fn non_family_names_resolve_to_none() {
        let outsiders = [
            "evaluate_profile_at",
            "evaluate_profile_dot_at",
            "evaluate_profile_ddot_at",
            "end_effector_track_at",
            "deviation_from_nominal_at",
            "gcode_import_lower",
            "profile_duration",
            "peak_deviation",
            "",
            "not_a_trajectory_intrinsic",
        ];

        for name in outsiders {
            assert_eq!(
                trajectory_scalar_intrinsic_result_type(name),
                None,
                "{name}"
            );
            assert!(
                !TRAJECTORY_SCALAR_INTRINSIC_NAMES.contains(&name),
                "{name} must not be in TRAJECTORY_SCALAR_INTRINSIC_NAMES"
            );
        }
    }
}
