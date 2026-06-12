//! Shared value-level acceptance helper for dimensioned builtin arguments.
//!
//! Provides [`accept_arg`] and the associated types (`ArgSpec`, `Acceptance`,
//! `ArgRejection`) used by Contract A (`resolve_density_arg` in `geometry_ops`)
//! and Contract B (`body_mass_props` density ladder in `dynamics_ops`; task δ).
//!
//! The helper is **value-level only**: it operates on an already-resolved
//! `reify_ir::Value` and has no knowledge of `CompiledExpr` or `ValueMap`.
//! Callers (currently `resolve_density_arg`) are responsible for extracting the
//! value from the expression.

/// Specification for a single builtin argument — its expected type name, the
/// required `DimensionVector`, and an optional hint shown in rejection messages.
pub struct ArgSpec {
    /// Human-readable type name used in diagnostic messages (e.g. `"Density"`).
    pub type_name: &'static str,
    /// The `DimensionVector` that the `Value::Scalar` dimension must equal.
    pub dimension: reify_core::DimensionVector,
    /// Optional migration hint shown in rejection messages
    /// (e.g. `"pass a dimensioned Density literal such as \`7850kg/m^3\`"`).
    pub migration_hint: Option<&'static str>,
}

/// The outcome of [`accept_arg`].
#[derive(Debug, PartialEq)]
pub enum Acceptance {
    /// The value has the expected dimension; carries the SI f64.
    Accepted(f64),
    /// The value is `Value::Undef` (or a missing cell); silently degrade.
    Undefined,
    /// The value is defined but the wrong type/dimension.
    Rejected(ArgRejection),
}

/// Carried by [`Acceptance::Rejected`]; contains the information needed to
/// format a `Severity::Warning` diagnostic via [`ArgRejection::message`].
#[derive(Debug, PartialEq)]
pub struct ArgRejection {
    /// Short description of the actual value type/dimension that was received.
    pub got: String,
    /// The expected type name from the `ArgSpec`.
    pub expected: &'static str,
    /// The migration hint from the `ArgSpec`, if any.
    pub migration_hint: Option<&'static str>,
}

impl ArgRejection {
    /// Format a `Severity::Warning` diagnostic message for this rejection.
    ///
    /// `builtin` is the calling builtin name (e.g. `"moment_of_inertia"`);
    /// `arg_name` is the argument name (e.g. `"density"`).
    ///
    /// Example output:
    /// `"moment_of_inertia: density argument expects Density, got Real; pass a dimensioned Density literal such as \`7850kg/m^3\`"`
    pub fn message(&self, builtin: &str, arg_name: &str) -> String {
        let base = format!(
            "{builtin}: {arg_name} argument expects {expected}, got {got}",
            expected = self.expected,
            got = self.got
        );
        if let Some(hint) = self.migration_hint {
            format!("{base}; {hint}")
        } else {
            base
        }
    }
}

/// Returns the [`ArgSpec`] for the `density` argument of `center_of_mass` and
/// `moment_of_inertia`: a `Value::Scalar` with `DimensionVector::MASS_DENSITY`
/// (kg·m⁻³).
pub fn density_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Density",
        dimension: reify_core::DimensionVector::MASS_DENSITY,
        migration_hint: Some(
            "pass a dimensioned Density literal such as `7850kg/m^3`",
        ),
    }
}

/// Classify `value` against `spec`.
///
/// - `Value::Undef` → [`Acceptance::Undefined`] (quiet, no diagnostic needed).
/// - `Value::Scalar { dimension, .. }` where `dimension == spec.dimension`
///   → [`Acceptance::Accepted`] carrying the SI f64.
/// - Any other defined value → [`Acceptance::Rejected`].
pub fn accept_arg(value: &reify_ir::Value, spec: &ArgSpec) -> Acceptance {
    unimplemented!("accept_arg not yet implemented (step-2 will fill this in)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_mass_density_scalar_returns_accepted() {
        let value = reify_ir::Value::Scalar {
            si_value: 7850.0,
            dimension: reify_core::DimensionVector::MASS_DENSITY,
        };
        let spec = density_spec();
        assert_eq!(
            accept_arg(&value, &spec),
            Acceptance::Accepted(7850.0),
            "MASS_DENSITY scalar must be accepted with its SI value"
        );
    }

    #[test]
    fn accept_undef_returns_undefined() {
        let value = reify_ir::Value::Undef;
        let spec = density_spec();
        assert_eq!(
            accept_arg(&value, &spec),
            Acceptance::Undefined,
            "Undef must return Undefined (quiet degradation)"
        );
    }

    #[test]
    fn accept_bare_real_rejected_with_density_and_migration_hint() {
        let value = reify_ir::Value::Real(7850.0);
        let spec = density_spec();
        match accept_arg(&value, &spec) {
            Acceptance::Rejected(rej) => {
                let msg = rej.message("moment_of_inertia", "density").to_lowercase();
                assert!(
                    msg.contains("density"),
                    "rejection message must contain 'density', got: {msg:?}"
                );
                assert!(
                    msg.contains("7850kg/m^3"),
                    "rejection message must contain '7850kg/m^3' migration hint, got: {msg:?}"
                );
            }
            other => panic!(
                "Value::Real(7850.0) must be Rejected, got: {other:?}"
            ),
        }
    }

    #[test]
    fn accept_pressure_scalar_rejected_strict_dimension() {
        let value = reify_ir::Value::Scalar {
            si_value: 2.0e11,
            dimension: reify_core::DimensionVector::PRESSURE,
        };
        let spec = density_spec();
        assert!(
            matches!(accept_arg(&value, &spec), Acceptance::Rejected(_)),
            "Pressure scalar must be Rejected (strict-dimension check closes Pressure-as-density hole)"
        );
    }
}
