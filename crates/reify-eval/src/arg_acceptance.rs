//! Shared value-level acceptance helper for dimensioned builtin arguments.
//!
//! Provides [`accept_arg`] and the associated types (`ArgSpec`, `Acceptance`,
//! `ArgRejection`) used by Contract A (`resolve_density_arg` in `geometry_ops`),
//! Contract B (`body_mass_props` density ladder in `dynamics_ops`; task δ), and
//! Contract C — the LENGTH-semantic args (task 5214, extended by 5350 and 5623):
//! `geometry_ops`' `eval_named_arg_length` and `resolve_length_scalar_arg`
//! (`edges_at_height` z/tol, `geo_equiv` tol), which share the single
//! [`length_spec`] so both emit identical rejection text.
//!
//! The positions currently routed through `eval_named_arg_length`, by family:
//!
//! | family    | builtin / position                                   | task |
//! |-----------|------------------------------------------------------|------|
//! | pattern   | linear + 2-D spacing, arbitrary-pattern offsets, mirror-plane origin | 5214 |
//! | pattern   | circular-pattern axis origin `ox`/`oy`/`oz`          | 5350 |
//! | transform | `translate` `dx`/`dy`/`dz`; `rotate_around` pivot `px`/`py`/`pz` | 5623 |
//! | sweep     | `revolve` axis origin `ox`/`oy`/`oz`                 | 5623 |
//! | curve     | `line_segment` endpoints `x1`…`z2`; `arc` centre `cx`/`cy`/`cz` + `radius`; `helix` `radius`/`pitch`/`height` | 5623 |
//!
//! Contract C is NOT yet exhaustive, and this note stays open until the closure
//! guard of task 5752 replaces it with a pointer. What remains un-gated, and
//! who owns it:
//!
//! - Variadic curve coordinates (`interp`/`bezier`/`nurbs`), which reach f64
//!   through `eval_all_args_to_f64` rather than a named-arg helper — task 5658.
//! - `profile_polygon`'s 2-D vertex pairs — task 5661.
//! - The raw-`Value` primitive/profile chokepoint (box/cylinder/sphere/… dims,
//!   `half_space` px/py/pz, rectangle/circle/ellipse dims), plus the single
//!   shared `DiagnosticCode` that every code-less Contract C site still needs —
//!   task 5743.
//! - The modify + sweep MAGNITUDES, on that same raw-`Value` chokepoint once
//!   5743 introduces it: `fillet` radius, `chamfer` distance,
//!   `chamfer_asymmetric` `d1`/`d2`, `shell` thickness, `thicken` offset,
//!   `offset_solid`/`offset_curve` distance, `extrude`/`extrude_symmetric`
//!   distance, `pipe` radius, `zone_slab` width — task 5744. These never reach
//!   a named-arg f64 helper at all: each is stored into its `GeometryOp` field
//!   as a raw `Value` by the bare-accepting `eval_named_arg` and coerced as SI
//!   metres at the kernel boundary, so it leaves no `as_f64` fingerprint —
//!   which is exactly why repeated hand audits missed them, and why they are
//!   listed here rather than left to be re-derived.
//!
//! Deliberately NOT gated, and not a residual: unit-vector DIRECTIONS
//! (`ax`/`ay`/`az`, `nx`/`ny`/`nz`, and `extrude_infinite`'s `dx`/`dy`/`dz`),
//! instance COUNTS, dimensionless scale FACTORS, and every ANGLE — angles are
//! `docs/prds/v0_6/angle-units-surface-convergence.md`'s by seam-table decree,
//! so gating one here would be a scope violation, not an improvement.
//!
//! Until the residual closes, adding a length-semantic arg anywhere means
//! adding it to the owning task's triage list too.
//!
//! The helper is **value-level only**: it operates on an already-resolved
//! `reify_ir::Value` and has no knowledge of `CompiledExpr` or `ValueMap`.
//! Callers are responsible for extracting the value from the expression
//! (`resolve_density_arg`/`resolve_spec_arg` evaluate a `CompiledExpr`;
//! `eval_named_arg_length` goes through `eval_named_arg`).

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
        migration_hint: Some("pass a dimensioned Density literal such as `7850kg/m^3`"),
    }
}

/// Returns the [`ArgSpec`] for a LENGTH-semantic builtin argument: a
/// `Value::Scalar` with `DimensionVector::LENGTH` (metres). Mirrors
/// [`density_spec`].
///
/// Three kinds of position share this spec, because all three are lengths in
/// every component:
///
/// - a DISPLACEMENT — `translate`'s `dx`/`dy`/`dz`, pattern spacing,
///   arbitrary-pattern offsets;
/// - a POINT in space — `rotate_around`'s pivot, `revolve`'s and
///   `circular_pattern`'s axis origin, the mirror plane's origin,
///   `line_segment`'s endpoints, `arc`'s centre;
/// - a standalone EXTENT — `arc`'s radius, `helix`'s radius/pitch/height,
///   `edges_at_height`'s z/tol.
///
/// A bare `Value::Real`/`Int` in one of these positions is silently read as SI
/// **metres** by `Value::as_f64` (the `10` vs `10mm` = 1000× hazard); this spec
/// drives the eval-layer rejection that closes that hole (task 5214; the
/// circular-pattern axis origin was added by 5350, and the transform / sweep /
/// curve families by 5623). See the module doc for the full position table and
/// for what stays deliberately un-gated.
pub fn length_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Length",
        dimension: reify_core::DimensionVector::LENGTH,
        migration_hint: Some("pass a dimensioned length such as `5mm`"),
    }
}

/// Classify `value` against `spec`.
///
/// - `Value::Undef` → [`Acceptance::Undefined`] (quiet, no diagnostic needed).
/// - `Value::Scalar { dimension, .. }` where `dimension == spec.dimension`
///   → [`Acceptance::Accepted`] carrying the SI f64.
/// - Any other defined value → [`Acceptance::Rejected`].
pub fn accept_arg(value: &reify_ir::Value, spec: &ArgSpec) -> Acceptance {
    match value {
        reify_ir::Value::Undef => Acceptance::Undefined,
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == spec.dimension => Acceptance::Accepted(*si_value),
        other => Acceptance::Rejected(ArgRejection {
            got: value_short_label(other),
            expected: spec.type_name,
            migration_hint: spec.migration_hint,
        }),
    }
}

/// Produce a short human-readable label for a `Value` used in rejection
/// diagnostics (e.g. `"Real"`, `"Pressure Scalar"`, `"Bool"`).
fn value_short_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => {
            if dimension.is_dimensionless() {
                "dimensionless Scalar".to_string()
            } else if let Some(name) = dimension.canonical_name() {
                format!("{name} Scalar")
            } else {
                "dimensioned Scalar".to_string()
            }
        }
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::GeometryHandle { .. } => "GeometryHandle".to_string(),
        _ => "unknown".to_string(),
    }
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
    fn accept_bare_real_rejected_with_migration_hint() {
        // Structural assertion: Real must be Rejected, the hint must be Some,
        // and message() must include both the arg name and the hint text.
        // The exact wording of the hint is pinned in geometry_ops'
        // resolve_density_arg_diagnostics integration test, not here.
        let value = reify_ir::Value::Real(7850.0);
        let spec = density_spec();
        match accept_arg(&value, &spec) {
            Acceptance::Rejected(rej) => {
                assert!(
                    rej.migration_hint.is_some(),
                    "ArgRejection for a bare Real must carry a migration hint"
                );
                let hint = rej.migration_hint.unwrap();
                let msg = rej.message("moment_of_inertia", "density");
                assert!(
                    msg.contains(hint),
                    "message() must embed the migration_hint text; got: {msg:?}"
                );
            }
            other => panic!("Value::Real(7850.0) must be Rejected, got: {other:?}"),
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
