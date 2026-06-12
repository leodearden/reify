//! Trajectory stdlib module — `piecewise_polynomial` ctor and evaluator
//! intrinsics (evaluate_profile / _dot / _ddot, profile_duration).
//!
//! PRD: docs/prds/v0_3/trajectory-input-shaping.md §4.1, §11 Phase 1 β.

use reify_ir::Value;

mod gcode_import;
// `pub` so `reify-eval/src/trajectory_ops.rs` can reach the impulse-shaper API
// (ImpulseTrain + residual_vibration) for the engine-side band-sweep robustness
// metric — re-exported at the crate root in `lib.rs` (task ζ, prereq-1).
pub mod impulse_shaper;
// `pub(crate)` (not private) so the crate-root re-export
// `pub use trajectory::input_shape::build_train_for_shaper;` in `lib.rs` can
// name the `input_shape` path segment (task ζ, prereq-1).
pub(crate) mod input_shape;
mod sampling;
mod simulate;
mod spline;
mod tots;
// `pub(crate)` so the crate-root re-exports in `lib.rs` can name the
// `trampoline` path segment for the cache keys + `*_value` composers that
// `reify-eval/src/trajectory_ops.rs` consumes (task π, prereq-1). Mirrors the
// `input_shape` visibility above and `dynamics::trampoline`.
pub(crate) mod trampoline;

/// Evaluate a trajectory stdlib function by name.
///
/// Returns `Some(Value)` for known function names, or `None` for unknown names
/// so that `eval_builtin` can fall through to the next module.
///
/// `gcode_import` (task ο) is fully wired: it marshals its arguments through the
/// pure [`gcode_import::lower_gcode`] layer and returns a real `Value::List` of
/// profile records (or `Value::Undef` on bad args / a hard parse error). See
/// [`gcode_import::eval_gcode_import`] for the argument contract.
///
/// `gcode_import_lower` is an internal delegate intrinsic: the stdlib `.ri`
/// declaration of `gcode_import` shadows the same-named `eval_builtin` entry
/// (the compiler's `resolve_function_overload` returns `Resolved` → `UserFunctionCall`
/// for any fn with a `.ri` body, so the evaluator runs the body rather than
/// reaching `eval_builtin`). The body therefore delegates via a *distinct* name —
/// `gcode_import_lower` — which has no `.ri` declaration and thus resolves
/// `NoUserFunctions` → `FunctionCall` → `eval_builtin` → here. Both names route
/// to the single `eval_gcode_import` implementation. The original `"gcode_import"`
/// name is kept so that the Rust eval-boundary tests in `mod.rs::tests` that call
/// `eval_builtin("gcode_import", …)` directly remain green with zero churn.
///
/// `input_shape` (task ζ, extended by task λ) follows the identical delegate
/// pattern: the stdlib `.ri` `input_shape` declaration delegates to the undeclared
/// `input_shape_apply` name, so both route here to
/// [`input_shape::eval_input_shape`]. The dispatcher first checks for
/// `TOTSShaper` (λ arm) and runs the real SQP loop
/// ([`input_shape::run_tots`] → [`super::tots::solve_tots`]); only then falls
/// through to the impulse-train arms (ZV/ZVD/EI/Cascaded, ζ). Returns the
/// shaped `Profile` as a `Value::StructureInstance` (or `Value::Undef` on bad
/// args / infeasible TOTS / unrecognised shaper). See
/// [`input_shape::eval_input_shape`] for the full argument contract.
///
/// The Phase β spline intrinsics still unconditionally return `Some(Value::Undef)`:
/// the pure-Rust spline math is implemented in the `spline` submodule but is
/// not yet wired to the Value API.  Full marshalling (parsing a
/// `PiecewisePolynomialProfile` from `Value::StructureInstance`, dispatching on
/// the `BoundaryCondition` SIR type-tag, emitting `Value::List<Value::Real>`
/// per joint) is deferred to a later phase (γ/η/θ per the β PRD scope
/// boundary).  Callers that see `Value::Undef` from one of those names should
/// treat it as a "not yet implemented" stub, not a computation result.
pub(crate) fn eval_trajectory(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "gcode_import" | "gcode_import_lower" => Some(gcode_import::eval_gcode_import(args)),
        "input_shape" | "input_shape_apply" => Some(input_shape::eval_input_shape(args)),
        // η EndEffectorTrack accessor intrinsics (task π) — the `*_at` delegates
        // the trajectory.ri accessor bodies call. Wrong arity → Undef (the
        // bad-args convention); a malformed track / out-of-range location is
        // handled gracefully inside each impl (empty list / 0, no panic).
        "end_effector_track_at" => Some(match args {
            [track, location] => trampoline::end_effector_track_at(track, location),
            _ => Value::Undef,
        }),
        "deviation_from_nominal_at" => Some(match args {
            [track, location] => trampoline::deviation_from_nominal_at(track, location),
            _ => Value::Undef,
        }),
        "peak_deviation_at" => Some(match args {
            [track, location] => trampoline::peak_deviation_at(track, location),
            _ => Value::Undef,
        }),
        "evaluate_profile" | "evaluate_profile_at" => Some(match args {
            [profile, t] => trampoline::evaluate_profile_value(profile, t),
            _ => Value::Undef,
        }),
        "piecewise_polynomial"
        | "evaluate_profile_dot"
        | "evaluate_profile_ddot"
        | "profile_duration" => Some(Value::Undef),
        _ => None,
    }
}

/// Analytic reference polynomials shared by sibling submodule tests.
///
/// `spline.rs` and `sampling.rs` both test against the same closed-form cubic
/// `p(t) = 1 + 2t - 0.5t² + 0.3t³` and quintic
/// `q(t) = 1 + t + t² + t³ - 0.5t⁴ + 0.1t⁵`.  Defining them once here
/// prevents the two copies from drifting independently.
#[cfg(test)]
pub(crate) mod test_polynomials {
    /// Cubic polynomial `p(t) = 1 + 2t - 0.5t² + 0.3t³`.
    pub(crate) fn cubic_p(t: f64) -> f64 {
        1.0 + 2.0 * t - 0.5 * t * t + 0.3 * t * t * t
    }
    /// First derivative `p'(t) = 2 - t + 0.9t²`.
    pub(crate) fn cubic_dp(t: f64) -> f64 {
        2.0 - t + 0.9 * t * t
    }
    /// Second derivative `p''(t) = -1 + 1.8t`.
    pub(crate) fn cubic_ddp(t: f64) -> f64 {
        -1.0 + 1.8 * t
    }

    /// Quintic polynomial `q(t) = 1 + t + t² + t³ - 0.5t⁴ + 0.1t⁵`.
    pub(crate) fn quintic_q(t: f64) -> f64 {
        1.0 + t + t * t + t * t * t - 0.5 * t.powi(4) + 0.1 * t.powi(5)
    }
    /// First derivative `q'(t) = 1 + 2t + 3t² - 2t³ + 0.5t⁴`.
    pub(crate) fn quintic_dq(t: f64) -> f64 {
        1.0 + 2.0 * t + 3.0 * t * t - 2.0 * t * t * t + 0.5 * t.powi(4)
    }
    /// Second derivative `q''(t) = 2 + 6t - 6t² + 2t³`.
    pub(crate) fn quintic_ddq(t: f64) -> f64 {
        2.0 + 6.0 * t - 6.0 * t * t + 2.0 * t * t * t
    }
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    /// Build a 100-line Marlin g-code fixture: 100 contiguous `G1` moves with
    /// no non-motion splitters, so lowering yields a single profile.
    fn marlin_100_line_fixture() -> String {
        let mut s = String::new();
        for i in 0..100 {
            s.push_str(&format!("G1 X{i} Y{i}\n"));
        }
        s
    }

    /// Build a `MarlinDialect` dialect value as the eval path receives it: a
    /// `Value::StructureInstance` whose `type_name` is `"MarlinDialect"` (the
    /// `gcode_import` arm dispatches on this name without a StructureRegistry).
    fn marlin_dialect_value() -> Value {
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "MarlinDialect".to_string(),
            version: 0,
            fields: PersistentMap::default(),
        }))
    }

    /// `gcode_import(<100-line Marlin source>, MarlinDialect)` evaluates to a
    /// non-empty `Value::List` (one entry per lowered motion profile).
    #[test]
    fn gcode_import_marlin_fixture_returns_nonempty_list() {
        let result = eval_builtin(
            "gcode_import",
            &[Value::String(marlin_100_line_fixture()), marlin_dialect_value()],
        );
        match result {
            Value::List(items) => {
                assert!(!items.is_empty(), "expected >= 1 profile, got an empty list")
            }
            other => panic!("expected Value::List from gcode_import, got {other:?}"),
        }
    }

    /// Wrong arity, a non-String source, or a non-StructureInstance dialect
    /// each return `Value::Undef` (the stdlib bad-args convention).
    #[test]
    fn gcode_import_bad_args_return_undef() {
        let dialect = marlin_dialect_value();
        let src = Value::String("G1 X10".to_string());

        // Wrong arity: 0, 1, and 3 args.
        assert!(eval_builtin("gcode_import", &[]).is_undef());
        assert!(eval_builtin("gcode_import", std::slice::from_ref(&src)).is_undef());
        assert!(
            eval_builtin(
                "gcode_import",
                &[src.clone(), dialect.clone(), Value::Int(0)]
            )
            .is_undef()
        );

        // Non-String source.
        assert!(eval_builtin("gcode_import", &[Value::Int(5), dialect.clone()]).is_undef());

        // Non-StructureInstance dialect.
        assert!(eval_builtin("gcode_import", &[src.clone(), Value::Int(7)]).is_undef());
    }

    // ── ζ step-5: input_shape eval-boundary (registrar) ──────────────────────

    /// Build a `PiecewisePolynomialProfile` `Value::StructureInstance` as the
    /// eval path receives it (mechanism / waypoints / boundary / spline_kind
    /// fields per trajectory.ri). `boundary` is a `NaturalSpline` instance and
    /// `spline_kind` a `SplineKind::CubicSpline` enum so the echo assertions can
    /// confirm they survive the shaping.
    fn sample_profile() -> Value {
        let boundary = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "NaturalSpline".to_string(),
            version: 0,
            fields: PersistentMap::default(),
        }));
        let fields: PersistentMap<String, Value> = [
            ("mechanism".to_string(), Value::Real(0.0)),
            ("waypoints".to_string(), Value::List(Vec::new())),
            ("boundary".to_string(), boundary),
            (
                "spline_kind".to_string(),
                Value::Enum {
                    type_name: "SplineKind".to_string(),
                    variant: "CubicSpline".to_string(),
                },
            ),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(7),
            type_name: "PiecewisePolynomialProfile".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a `ZVShaper(10Hz, ζ=0)` `Value::StructureInstance` as the eval path
    /// receives it (target_frequency a Frequency scalar; damping_ratio a Real).
    fn zv_shaper_value() -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "target_frequency".to_string(),
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::FREQUENCY,
                },
            ),
            ("damping_ratio".to_string(), Value::Real(0.0)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "ZVShaper".to_string(),
            version: 0,
            fields,
        }))
    }

    /// `input_shape(profile, ZVShaper)` and the `input_shape_apply` delegate
    /// both return a `Value::StructureInstance` typed `PiecewisePolynomialProfile`
    /// that echoes the input profile's mechanism / boundary / spline_kind. (Both
    /// names route to the single `eval_input_shape` impl, so the direct
    /// `eval_builtin` boundary test pins both.)
    #[test]
    fn input_shape_zv_echoes_profile_structure() {
        let profile = sample_profile();
        let shaper = zv_shaper_value();

        for name in ["input_shape", "input_shape_apply"] {
            let result = eval_builtin(name, &[profile.clone(), shaper.clone()]);
            let Value::StructureInstance(data) = result else {
                panic!("{name} should return a Value::StructureInstance, got {result:?}");
            };
            assert_eq!(
                data.type_name, "PiecewisePolynomialProfile",
                "{name} should echo the profile's type_name"
            );

            // mechanism echoed verbatim.
            assert_eq!(
                data.fields.get(&"mechanism".to_string()),
                Some(&Value::Real(0.0)),
                "{name} should echo the profile's mechanism field"
            );
            // boundary StructureInstance echoed (NaturalSpline).
            match data.fields.get(&"boundary".to_string()) {
                Some(Value::StructureInstance(b)) => assert_eq!(
                    b.type_name, "NaturalSpline",
                    "{name} should echo the boundary NaturalSpline"
                ),
                other => panic!("{name} should echo boundary as NaturalSpline, got {other:?}"),
            }
            // spline_kind enum echoed (CubicSpline).
            match data.fields.get(&"spline_kind".to_string()) {
                Some(Value::Enum { type_name, variant }) => {
                    assert_eq!(type_name, "SplineKind", "{name} spline_kind enum type");
                    assert_eq!(variant, "CubicSpline", "{name} spline_kind variant");
                }
                other => panic!("{name} should echo spline_kind CubicSpline, got {other:?}"),
            }
        }
    }

    /// Bad args → `Value::Undef` (the stdlib convention, mirroring
    /// `gcode_import_bad_args_return_undef`): wrong arity (0/1/3), a
    /// non-`StructureInstance` profile or shaper, or a shaper whose `type_name`
    /// is not a recognised shaper (so `build_train_for_shaper` returns `None`).
    #[test]
    fn input_shape_bad_args_return_undef() {
        let profile = sample_profile();
        let shaper = zv_shaper_value();

        // Wrong arity: 0, 1, and 3 args.
        assert!(eval_builtin("input_shape", &[]).is_undef());
        assert!(eval_builtin("input_shape", std::slice::from_ref(&profile)).is_undef());
        assert!(
            eval_builtin(
                "input_shape",
                &[profile.clone(), shaper.clone(), Value::Int(0)]
            )
            .is_undef()
        );

        // Non-StructureInstance profile.
        assert!(eval_builtin("input_shape", &[Value::Int(5), shaper.clone()]).is_undef());

        // Non-StructureInstance shaper.
        assert!(eval_builtin("input_shape", &[profile.clone(), Value::Int(7)]).is_undef());

        // Unknown shaper type_name → build_train_for_shaper returns None → Undef.
        let bogus_shaper = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "FooShaper".to_string(),
            version: 0,
            fields: PersistentMap::default(),
        }));
        assert!(eval_builtin("input_shape", &[profile.clone(), bogus_shaper]).is_undef());
    }
}
