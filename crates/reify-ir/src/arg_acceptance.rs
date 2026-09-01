//! Shared value-level acceptance helper for dimensioned builtin arguments.
//!
//! **Relocated by task 5791** (PRD `docs/prds/v0_6/dimension-checked-readers.md`
//! §3 Leg A) from its former home `crates/reify-eval/src/arg_acceptance.rs`,
//! where it was `pub(crate)` and therefore reachable by `reify-eval` ONLY.
//! `reify-stdlib` cannot depend on `reify-eval` — the crate edge runs the other
//! way — so a reader-side dimension check in `reify-stdlib` had no way to call
//! this family and would have had to fork it. Hoisting the module into
//! `reify-ir`, the one crate BOTH already depend on, means `reify-stdlib` and
//! `reify-eval` now share ONE dimension-acceptance rule: a single
//! [`accept_arg`] predicate and a single rejection wording, rather than two
//! implementations drifting apart. The move needed no `Cargo.toml` edit —
//! the module depends only on `reify_core` (already a `reify-ir` dep) and on
//! `reify-ir`'s own [`crate::value::Value`] — so `reify-ir`'s
//! reify-core + reify-ast dependency floor (pinned by
//! `crates/reify-ir/tests/dag_invariant.rs`) is unchanged. `reify-eval` keeps
//! its former `crate::arg_acceptance::…` spelling alive through a crate-root
//! `pub(crate) use reify_ir::arg_acceptance;`, so every pre-existing consumer
//! compiles unchanged.
//!
//! Provides [`accept_arg`] and the associated types (`ArgSpec`, `Acceptance`,
//! `ArgRejection`) used by Contract A (`resolve_density_arg` in `geometry_ops`),
//! Contract B (`body_mass_props` density ladder in `dynamics_ops`; task δ), and
//! Contract C — the LENGTH-semantic args (task 5214, extended by 5350, 5623,
//! 5658, 5661, 5743 and 5744): `geometry_ops`' `eval_named_arg_length`, its
//! raw-`Value` wrapper `required_length_value`, and `resolve_length_scalar_arg`
//! (`edges_at_height` z/tol, `geo_equiv` tol), which share the single
//! [`length_spec`] so both emit identical rejection text.
//!
//! The positions currently routed through the Contract C chokepoint, by family.
//! THREE routes reach it, all bottoming out in the same
//! `accept_arg(&value, &length_spec())` call so their rejection wording is
//! byte-identical by construction: the NAMED-ARG route
//! (`eval_named_arg_length`); since task 5658 the VARIADIC route
//! (`accept_variadic_length_args`, for the arity-open positional coordinate
//! streams whose args the compiler names inertly `c0`…`cN`); and since task
//! 5745 the DECODED-VALUE route (`accept_length_point3`, for a position that
//! arrives already assembled into a composite `Value` by a stdlib producer —
//! `plane_yz(10mm)` → `Value::Plane`, `point3(…)` → `Value::Point` — and so
//! never passes through an argument-name read at all). Since task 5661 the
//! variadic route carries 2-D vertex PAIRS as well as 3-D triples, which is why
//! its coordinate renderer (`CoordName`) carries a per-point STRIDE: at stride 3
//! a pair stream would be named `x1,y1,z1,x2,…`, misdirecting the author on both
//! the axis letter and the vertex number. The decoded-value route has the
//! analogous renderer problem in a GRID rather than a stream, and answers it the
//! same way (`GridCoordName`, rendering `control_points[r][c].{x|y|z}`).
//!
//! | family    | builtin / position                                   | task |
//! |-----------|------------------------------------------------------|------|
//! | pattern   | linear + 2-D spacing, arbitrary-pattern offsets, mirror-plane origin | 5214 |
//! | pattern   | circular-pattern axis origin `ox`/`oy`/`oz`          | 5350 |
//! | transform | `translate` `dx`/`dy`/`dz`; `rotate_around` pivot `px`/`py`/`pz` | 5623 |
//! | sweep     | `revolve` axis origin `ox`/`oy`/`oz`                 | 5623 |
//! | curve     | `line_segment` endpoints `x1`…`z2`; `arc` centre `cx`/`cy`/`cz` + `radius`; `helix` `radius`/`pitch`/`height` | 5623 |
//! | curve     | `interp` + `bezier` variadic coordinate triples (EVERY position); `nurbs` pole coordinates (`2 .. 2 + 3·n_points`) — via the variadic route | 5658 |
//! | profile   | `polygon` variadic 2-D vertex pairs (EVERY position) — via the variadic route | 5661 |
//! | primitive | `box` width/height/depth, `cylinder` radius/height, `sphere` radius, `tube` outer_r/inner_r/height, `cone` bottom_radius/top_radius/height, `wedge` width/depth/height/top_width, `torus` major/minor_radius, `half_space` POINT `px`/`py`/`pz` (21 fields) | 5743 |
//! | profile   | `rectangle` width/height, `circle` radius, `ellipse` semi_major/semi_minor (5 fields) | 5743 |
//! | modify    | `fillet` radius, `chamfer` distance, `chamfer_asymmetric` `d1`/`d2`, `shell` thickness, `thicken` offset, `zone_slab` width, `offset_solid`/`offset_curve` distance (9 fields) | 5744 |
//! | sweep     | `extrude`/`extrude_symmetric` distance, `pipe` radius (3 fields) | 5744 |
//! | decoded value | `decode_plane` / `decode_axis` ORIGINS `ox`/`oy`/`oz`; the `nurbs_surface` control-point GRID (the SURFACE sibling of the curve poles 5658 gated) — via the decoded-value route | 5745 |
//!
//! The two **5743** rows (`primitive` + `profile`) and the two **5744** rows
//! (`modify` + `sweep`) are the R7 **raw-`Value`** positions: unlike the
//! named-arg rows above them, these 38 are stored into their `GeometryOp`
//! field as a `Value` and read by the kernel via `as_f64`, never through a
//! named-arg `f64` helper. (The **5745** `decoded value` row below them is a
//! THIRD route, not a raw-`Value` one: its coordinates are already assembled
//! into a composite `Value` by a producer and are read back out by
//! `point3_components`, so it reaches the chokepoint via `accept_length_point3`
//! rather than via `required_length_value`.)
//!
//! Those 38 reach the chokepoint through `geometry_ops`' `required_length_values`
//! (and its `N == 1` wrapper `required_length_value`), which layers over the
//! named-arg route (`required_length_args` → `required_length_arg` →
//! `eval_named_arg_length`) and re-wraps each ACCEPTED SI f64 back into a
//! dimensioned `Value` — so the stored representation is unchanged and the
//! rejection wording is shared, rather than forked for the kernel boundary.
//! Every builtin with MORE THAN ONE gated slot reads its whole set in one
//! `required_length_values` call, so a bare `box(20, 20, 10)` is diagnosed at
//! `width`, `height` AND `depth` — and a bare
//! `chamfer_asymmetric(solid, 1, 2)` at BOTH `d1` and `d2` — in a single build
//! rather than one arg name per rebuild. Task 5744 extended that same
//! all-at-once discipline to the named-arg route's last `?`-chained group,
//! `pattern_arbitrary`'s per-transform `t{i}_dx`/`t{i}_dy`/`t{i}_dz` offset
//! triple (esc-5743-4), which now reads through `required_length_args`.
//! Task 5743 also attached `reify_core::DiagnosticCode::DimensionedArgRejected`
//! to every `ArgSpec`-backed rejection emitted in `geometry_ops`, retrofitting
//! the previously code-less Contract C sites on both of the routes that existed
//! then. Task 5745's decoded-value route inherits the code for free, by calling
//! the same shared `accept_length_value`.
//!
//! Contract C is NOT yet exhaustive, and this note stays open until the closure
//! guard of task 5752 replaces it with a pointer. What remains un-gated, and
//! who owns it:
//!
//! - The SEVERITY residuals — task 6157. Task 5743 promoted the shared
//!   `accept_length_value` rejection from `Warning` to `Error` + code, but
//!   deliberately left three classes alone: the quiet-degrade readers
//!   (`resolve_spec_arg` / `resolve_density_arg`), which return `Option<f64>`
//!   and whose callers CONTINUE on `None` with no paired op-compile Error, so
//!   promoting them would flip `reify eval` to exit 1 for positions no boundary
//!   row covers; the non-finite-`Length` arm, which `accept_arg` ACCEPTED (it
//!   IS a Length, merely NaN/±inf) and which therefore carries no
//!   `ArgRejection` to hang a dimension-rejection code on; and the inline
//!   non-`ArgSpec` `ArgRejection` sites (`Int`, `Point<Length>`, `Vec3`,
//!   `Range`, `String` — including `resolve_int_value_ref`) plus Contract B.
//!
//! Deliberately NOT gated, and not a residual — the DECODED-VALUE counterparts
//! of the unit-vector row below, each with the justification task 5752's
//! closure-guard allowlist can lift verbatim (D14). All three are the remaining
//! `point3_components` callers, which is why that helper SURVIVES task 5745
//! rather than being replaced by the gate:
//!
//! - the `decode_plane` plane NORMAL — a dimensionless unit vector, normalised
//!   by `unit_vector3`; the plane equation is invariant to its scale;
//! - the `decode_axis` axis DIRECTION — likewise a dimensionless unit vector,
//!   normalised by `unit_vector3`;
//! - `offset_curve`'s 3rd argument when it is not a reference Surface — its own
//!   production diagnostic already calls it "a direction vec3".
//!
//! Gating any of the three would REJECT CORRECT `.ri` CODE, since a unit vector
//! legitimately has bare components. This is the D3 adversary finding
//! (2026-07-28, BINDING) — the same ORIGIN-vs-DIRECTION split the SCALAR forms
//! already draw, restated for the decoded-value route.
//!
//! Deliberately NOT gated, and not a residual: unit-vector DIRECTIONS
//! (`ax`/`ay`/`az`, `nx`/`ny`/`nz`, and `extrude_infinite`'s `dx`/`dy`/`dz`),
//! instance COUNTS, dimensionless scale FACTORS, and every ANGLE — angles are
//! `docs/prds/v0_6/angle-units-surface-convergence.md`'s by seam-table decree,
//! so gating one here would be a scope violation, not an improvement. That PRD
//! reuses the SAME `DimensionedArgRejected` code rather than minting a
//! per-dimension sibling, so no ANGLE row will ever appear in this table's
//! residual list — only in that PRD's. `half_space` is the one builtin whose
//! args STRADDLE the boundary: its `px`/`py`/`pz` POINT is gated (above) while
//! its `nx`/`ny`/`nz` outward NORMAL stays bare, mirroring the `ax`/`ay`/`az`
//! vs `ox`/`oy`/`oz` split already drawn for the circular pattern.
//!
//! Also deliberately NOT gated, and the reason `nurbs` gates a SPAN rather than
//! every position — its dimensionless neighbours sit on BOTH sides of the poles
//! (task 5658), each ungated for a stated reason rather than by omission:
//! `degree` is a polynomial degree, i.e. a count; `n_points` is a count; the
//! weights are rational blending factors; the knots are parameter-space values.
//! None of the four is a quantity in metres, so demanding a dimension of them
//! would reject correct `.ri` code. The gated span is consequently
//! ARITY-DEPENDENT — `2 .. 2 + 3·n_points`, computed from an argument — so a
//! mechanical allowlist over it needs per-arity keys.
//!
//! Until the residual closes, adding a length-semantic arg anywhere means
//! adding it to the owning task's triage list too.
//!
//! The helper is **value-level only**: it operates on an already-resolved
//! `Value` and has no knowledge of `CompiledExpr` or `ValueMap`.
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
        migration_hint: Some(reify_core::units::DENSITY_MIGRATION_HINT),
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
///   `line_segment`'s endpoints, `arc`'s centre, the variadic coordinate
///   streams (`interp`/`bezier`/`nurbs` control points, and `polygon`'s
///   vertices — a point in the XY PLANE, which is a plane in space);
/// - a standalone EXTENT — `arc`'s radius, `helix`'s radius/pitch/height,
///   `edges_at_height`'s z/tol, the primitive and profile DIMENSIONS
///   (`box`/`cylinder`/`sphere`/`tube`/`cone`/`wedge`/`torus`,
///   `rectangle`/`circle`/`ellipse`; task 5743), and the modify + sweep
///   MAGNITUDES — `fillet` radius, `chamfer` distance,
///   `chamfer_asymmetric`'s `d1`/`d2`, `shell` thickness, `thicken` offset,
///   `zone_slab` width, `offset_solid`/`offset_curve` distance,
///   `extrude`/`extrude_symmetric` distance and `pipe` radius (task 5744).
///
/// A bare `Value::Real`/`Int` in one of these positions is silently read as SI
/// **metres** by `Value::as_f64` (the `10` vs `10mm` = 1000× hazard); this spec
/// drives the eval-layer rejection that closes that hole (task 5214; the
/// circular-pattern axis origin was added by 5350, the transform / sweep /
/// curve families by 5623, the variadic curve coordinates by 5658,
/// `polygon`'s vertex pairs by 5661, the primitive and profile dimensions by
/// 5743 and the modify + sweep magnitudes by 5744). See the module doc for the
/// full position table and for what stays deliberately un-gated.
pub fn length_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Length",
        dimension: reify_core::DimensionVector::LENGTH,
        migration_hint: Some(reify_core::units::LENGTH_MIGRATION_HINT),
    }
}

// ── PRD 5 §3 Leg B: the DIMENSIONED reader specs ──────────────────────────────
//
// Task 5791 (PRD `docs/prds/v0_6/dimension-checked-readers.md` §3 Leg B) adds
// the eleven ADDITIVE spec constructors the reader chokepoints of leaves β…ι
// consume. Each mirrors the shape of [`density_spec`] / [`length_spec`] above
// and reads its `dimension` from the `reify_core::DimensionVector` REGISTRY
// const BY NAME — never from a hand-written `from_exps` exponent tuple — so a
// later re-dimensioning is a one-line registry edit rather than a hunt through
// every reader. (Precedent: task #5799 re-dimensioned `ROTATIONAL_STIFFNESS`
// from rad⁻¹ to rad⁻²; a reader carrying its own tuple would have silently
// disagreed with the registry from that day on.)
//
// # Why every one of them sets `migration_hint: None`
//
// `reify_core::units` today holds exactly two shared hint consts —
// [`reify_core::units::LENGTH_MIGRATION_HINT`] and
// [`reify_core::units::DENSITY_MIGRATION_HINT`] — and its "Why a HOIST and not
// a copy" module doc records precisely why: those two are IRREGULAR hard-coded
// literals that the EVAL layer (this module) and the COMPILE layer
// (`reify-compiler::builtin_signatures`) must render byte-identically for the
// same authoring mistake, so they genuinely can drift and that is what earns
// them a shared const. None of the eleven below has a compile-layer twin to
// keep byte-identical yet, so minting eleven more consts there would add an
// edit site with nothing on the other end of it. The slot is left
// INTENTIONALLY EMPTY until a consuming leaf (β…ι) grows that twin; the leaf
// that does is the one that should hoist the const, following the same
// hoist-not-a-copy argument.

/// Returns the [`ArgSpec`] for a PRESSURE-semantic reader position (Pa =
/// kg·m⁻¹·s⁻²).
///
/// PRD §3 Leg B routes these here: `ElasticMaterial`'s `youngs_modulus`,
/// `yield_stress` and `shear_modulus`; the `FDMCouponOverride` orthotropic
/// moduli `ex`/`ey`/`ez`/`gxy`; `yield_val`; `PressureLoad.magnitude`; and
/// `TractionLoad.traction`.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn pressure_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Pressure",
        dimension: reify_core::DimensionVector::PRESSURE,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a FORCE-semantic reader position (N =
/// kg·m·s⁻²).
///
/// PRD §3 Leg B routes these here: `force_limit`, `reference_load` and
/// `PointLoad.force`.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn force_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Force",
        dimension: reify_core::DimensionVector::FORCE,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a MASS-semantic reader position (kg).
///
/// PRD §3 Leg B routes the rigid-body / lumped `mass` reader here.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn mass_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Mass",
        dimension: reify_core::DimensionVector::MASS,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a FREQUENCY-semantic reader position (Hz =
/// s⁻¹).
///
/// PRD §3 Leg B routes the modal `target_frequency` reader here.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn frequency_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Frequency",
        dimension: reify_core::DimensionVector::FREQUENCY,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a TIME-semantic reader position (s).
///
/// PRD §3 Leg B routes the trajectory waypoint `t` reader here.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn time_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Time",
        dimension: reify_core::DimensionVector::TIME,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a VELOCITY-semantic reader position (m·s⁻¹).
///
/// PRD §3 Leg B routes the joint `velocity_limit` reader here.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn velocity_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Velocity",
        dimension: reify_core::DimensionVector::VELOCITY,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for an ACCELERATION-semantic reader position
/// (m·s⁻²).
///
/// PRD §3 Leg B routes `acceleration_limit` and `max_accel` here. Note the
/// already-shipped `validate_dimensioned_scalar` call at
/// `reify-stdlib/src/fea/loads.rs:161` passes an `accel_dim` of
/// `DimensionVector::ACCELERATION`, so this is the spec its consuming leaf
/// adopts.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn acceleration_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Acceleration",
        dimension: reify_core::DimensionVector::ACCELERATION,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a FORCE-DENSITY-semantic reader position
/// (N·m⁻³ = kg·m⁻²·s⁻²).
///
/// PRD §3 Leg B routes `BodyForce.force_density` here.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn force_density_spec() -> ArgSpec {
    ArgSpec {
        type_name: "ForceDensity",
        dimension: reify_core::DimensionVector::FORCE_DENSITY,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a MOMENT-OF-INERTIA-semantic reader position
/// (kg·m²).
///
/// PRD §3 Leg B routes the rigid-body inertia tensor cells here.
///
/// Not to be confused with the `moment_of_inertia` BUILTIN, whose `density`
/// argument uses [`density_spec`].
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn moment_of_inertia_spec() -> ArgSpec {
    ArgSpec {
        type_name: "MomentOfInertia",
        dimension: reify_core::DimensionVector::MOMENT_OF_INERTIA,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a PRISMATIC joint's `spring_rate` reader
/// position (N·m⁻¹ = kg·s⁻²).
///
/// `DimensionVector::TRANSLATIONAL_STIFFNESS` is a NAME ALIAS of
/// `DimensionVector::STIFFNESS` (`reify-core/src/dimension.rs:302`); the alias
/// is used here so the reader names the joint kind it serves. Its revolute
/// sibling [`rotational_stiffness_spec`] is a genuinely DISTINCT vector — PRD
/// §3 Leg B routes `spring_rate` to one or the other BY JOINT KIND, so the two
/// must never collapse. Pinned by
/// `translational_stiffness_and_stiffness_are_the_same_vector`.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn translational_stiffness_spec() -> ArgSpec {
    ArgSpec {
        type_name: "TranslationalStiffness",
        dimension: reify_core::DimensionVector::TRANSLATIONAL_STIFFNESS,
        migration_hint: None,
    }
}

/// Returns the [`ArgSpec`] for a REVOLUTE joint's `spring_rate` reader
/// position (N·m·rad⁻¹ = kg·m²·s⁻²·rad⁻²).
///
/// A DISTINCT vector from [`translational_stiffness_spec`], re-dimensioned
/// from rad⁻¹ by task #5799 — which is exactly why this constructor reads
/// `DimensionVector::ROTATIONAL_STIFFNESS` by name instead of spelling the
/// exponents out.
///
/// `migration_hint` is intentionally `None` — see the section banner above.
pub fn rotational_stiffness_spec() -> ArgSpec {
    ArgSpec {
        type_name: "RotationalStiffness",
        dimension: reify_core::DimensionVector::ROTATIONAL_STIFFNESS,
        migration_hint: None,
    }
}


/// Classify `value` against `spec`.
///
/// - `Value::Undef` → [`Acceptance::Undefined`] (quiet, no diagnostic needed).
/// - `Value::Scalar { dimension, .. }` where `dimension == spec.dimension`
///   → [`Acceptance::Accepted`] carrying the SI f64.
/// - Any other defined value → [`Acceptance::Rejected`].
pub fn accept_arg(value: &crate::value::Value, spec: &ArgSpec) -> Acceptance {
    match value {
        crate::value::Value::Undef => Acceptance::Undefined,
        crate::value::Value::Scalar {
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
fn value_short_label(value: &crate::value::Value) -> String {
    match value {
        crate::value::Value::Real(_) => "Real".to_string(),
        crate::value::Value::Scalar { dimension, .. } => {
            if dimension.is_dimensionless() {
                "dimensionless Scalar".to_string()
            } else if let Some(name) = dimension.canonical_name() {
                format!("{name} Scalar")
            } else {
                "dimensioned Scalar".to_string()
            }
        }
        crate::value::Value::Bool(_) => "Bool".to_string(),
        crate::value::Value::Int(_) => "Int".to_string(),
        crate::value::Value::GeometryHandle { .. } => "GeometryHandle".to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_mass_density_scalar_returns_accepted() {
        let value = crate::value::Value::Scalar {
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
        let value = crate::value::Value::Undef;
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
        let value = crate::value::Value::Real(7850.0);
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
        let value = crate::value::Value::Scalar {
            si_value: 2.0e11,
            dimension: reify_core::DimensionVector::PRESSURE,
        };
        let spec = density_spec();
        assert!(
            matches!(accept_arg(&value, &spec), Acceptance::Rejected(_)),
            "Pressure scalar must be Rejected (strict-dimension check closes Pressure-as-density hole)"
        );
    }

    // ── PRD 5 §3 Leg B: the DIMENSIONED spec constructors ─────────────────────

    /// Every PRD-5 spec constructor must read its `dimension` from the
    /// `reify_core::DimensionVector` REGISTRY const by name, never from a
    /// hand-written `from_exps` exponent tuple. That is what makes a later
    /// re-dimensioning (see the ROTATIONAL_STIFFNESS re-dimensioning of task
    /// #5799) a ONE-LINE registry change instead of a hunt through every
    /// reader. Comparing against the const rather than against a literal
    /// tuple is the whole point of this test — do not "strengthen" it into an
    /// exponent assertion.
    #[test]
    fn prd5_spec_constructors_read_dimensions_from_the_core_registry() {
        use reify_core::DimensionVector as DV;

        let cases: [(&str, ArgSpec, DV); 11] = [
            ("pressure_spec", pressure_spec(), DV::PRESSURE),
            ("force_spec", force_spec(), DV::FORCE),
            ("mass_spec", mass_spec(), DV::MASS),
            ("frequency_spec", frequency_spec(), DV::FREQUENCY),
            ("time_spec", time_spec(), DV::TIME),
            ("velocity_spec", velocity_spec(), DV::VELOCITY),
            ("acceleration_spec", acceleration_spec(), DV::ACCELERATION),
            ("force_density_spec", force_density_spec(), DV::FORCE_DENSITY),
            (
                "moment_of_inertia_spec",
                moment_of_inertia_spec(),
                DV::MOMENT_OF_INERTIA,
            ),
            (
                "translational_stiffness_spec",
                translational_stiffness_spec(),
                DV::TRANSLATIONAL_STIFFNESS,
            ),
            (
                "rotational_stiffness_spec",
                rotational_stiffness_spec(),
                DV::ROTATIONAL_STIFFNESS,
            ),
        ];

        for (name, spec, expected) in cases {
            assert_eq!(
                spec.dimension, expected,
                "{name}: dimension must EQUAL the named reify_core registry const"
            );
            assert!(
                !spec.type_name.is_empty(),
                "{name}: type_name must be non-empty (it is the `expects {{…}}` \
                 text in every rejection message)"
            );
        }
    }

    /// The PRD-5 constructors inherit `accept_arg`'s STRICT dimension equality:
    /// a neighbouring dimension is as wrong as a bare `Real`.
    #[test]
    fn prd5_spec_constructors_enforce_strict_dimension_equality() {
        use reify_core::DimensionVector as DV;

        let pressure = crate::value::Value::Scalar {
            si_value: 2.0e11,
            dimension: DV::PRESSURE,
        };
        assert_eq!(
            accept_arg(&pressure, &pressure_spec()),
            Acceptance::Accepted(2.0e11),
            "a PRESSURE Scalar must be Accepted by pressure_spec with its SI value"
        );

        let force = crate::value::Value::Scalar {
            si_value: 5000.0,
            dimension: DV::FORCE,
        };
        assert!(
            matches!(
                accept_arg(&force, &pressure_spec()),
                Acceptance::Rejected(_)
            ),
            "a FORCE Scalar must be Rejected by pressure_spec (strict equality)"
        );

        assert!(
            matches!(
                accept_arg(&crate::value::Value::Real(2.0e11), &pressure_spec()),
                Acceptance::Rejected(_)
            ),
            "a bare Real must be Rejected by pressure_spec"
        );
        assert!(
            matches!(
                accept_arg(
                    &crate::value::Value::Real(1.0),
                    &moment_of_inertia_spec()
                ),
                Acceptance::Rejected(_)
            ),
            "a bare Real must be Rejected by moment_of_inertia_spec"
        );

        assert_eq!(
            accept_arg(&crate::value::Value::Undef, &force_spec()),
            Acceptance::Undefined,
            "Undef must stay quiet-degrading at a force_spec position"
        );
    }

    /// `TRANSLATIONAL_STIFFNESS` is a NAME ALIAS of `STIFFNESS` (dimension.rs:302)
    /// while `ROTATIONAL_STIFFNESS` (dimension.rs:283) is a DISTINCT vector,
    /// re-dimensioned by task #5799 to carry slot 7 (angle) at -2.
    ///
    /// PRD §3 Leg B routes a joint's `spring_rate` to ONE OR THE OTHER by joint
    /// kind (prismatic → translational, revolute → rotational), so a silent
    /// collapse of the two would be a soundness hole: a revolute spring rate
    /// would be accepted at a prismatic reader and vice versa.
    #[test]
    fn translational_stiffness_and_stiffness_are_the_same_vector() {
        use reify_core::DimensionVector as DV;

        assert_eq!(
            translational_stiffness_spec().dimension,
            DV::STIFFNESS,
            "TRANSLATIONAL_STIFFNESS aliases STIFFNESS (dimension.rs:302)"
        );
        assert_ne!(
            rotational_stiffness_spec().dimension,
            translational_stiffness_spec().dimension,
            "ROTATIONAL_STIFFNESS must stay a DISTINCT vector from \
             TRANSLATIONAL_STIFFNESS — PRD §3 Leg B routes spring_rate to one \
             or the other by joint kind"
        );

        // And the distinction is observable through accept_arg, not just through
        // the consts: a rotational spring rate is rejected at a prismatic reader.
        let rotational = crate::value::Value::Scalar {
            si_value: 12.0,
            dimension: DV::ROTATIONAL_STIFFNESS,
        };
        assert!(
            matches!(
                accept_arg(&rotational, &translational_stiffness_spec()),
                Acceptance::Rejected(_)
            ),
            "a ROTATIONAL_STIFFNESS Scalar must be Rejected at a \
             translational_stiffness_spec position"
        );
    }

}
