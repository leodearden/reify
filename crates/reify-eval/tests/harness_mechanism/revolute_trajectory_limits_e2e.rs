//! End-to-end acceptance fixtures for the per-joint-kind actuator/kinematic
//! limit duality (task 6096) — one fixture per joint kind, both compiled and
//! evaluated through the real engine.
//!
//! ## What the duality is
//!
//! `trajectory.ri` declares two sibling halves that share every param NAME and
//! differ only in the declared dimension:
//!
//! | field                              | prismatic (linear)     | revolute (rotational)     |
//! |------------------------------------|------------------------|---------------------------|
//! | `*JointLimit.max_force`            | `Scalar<Force>` m·kg·s⁻² | `Scalar<Torque>` m²·kg·s⁻²·rad⁻¹ |
//! | `*TOTSShaper.velocity_limit`       | `Scalar<Velocity>` m·s⁻¹ | `Scalar<AngularVelocity>` rad·s⁻¹ |
//! | `*TOTSShaper.acceleration_limit`   | `Scalar<Acceleration>` m·s⁻² | `Rate<AngularVelocity>` rad·s⁻² |
//!
//! This file pins the ACCEPTANCE direction for both: a prismatic joint keeps
//! the linear spellings and a revolute joint takes the rotational ones, each
//! check-clean and eval-green end to end.
//!
//! ## Why the dimensions are pinned by matching `Value::Scalar` directly
//!
//! Every field pin below matches `Value::Scalar { si_value, dimension }`
//! explicitly and compares `dimension` against a `DimensionVector` — never
//! through an f64 coercion helper. That is the convention documented at
//! `input_shape_eval_e2e.rs::pin`, `harness_engine/dimensioned_ctor_migration_si_values.rs`
//! and `printer_print_envelope_e2e.rs`: `read_scalar_si`-style coercion folds
//! `Value::Real` and `Value::Scalar` together, which would make a dimension
//! assertion vacuous — exactly the error this file exists to catch.
//!
//! ## Why the fixtures are inline rather than `examples/trajectory/*.ri`
//!
//! A new corpus example would enter the example/corpus probe sets
//! (`tests/prd-gate/example-probe-set.json`, `corpus-probe-set.json`),
//! coupling a dimensional retype to unrelated corpus-gate registration for no
//! added signal. Inline snippets also let the same test pin exact
//! `DimensionVector`s on the evaluated values, which a corpus example cannot.
//!
//! ## Scope boundary
//!
//! The wrong-kind PAIRING diagnostic — supplying a `Scalar<Force>` max_force
//! to a `RevoluteJointLimit` ctor — is task #6240's (dep-gated on #5627) and
//! is deliberately NOT asserted here; ctor args are not dimension-checked
//! today, so such an assertion would be a doomed RED.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::EvalResult;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::Value;
use reify_test_support::{compile_source_with_stdlib, make_simple_engine};

// ── Inline fixtures, one per joint kind ─────────────────────────────────────────

/// PRISMATIC / translational fixture: `JointLimit` + `TOTSShaper`, the LINEAR
/// half, spelled exactly as the existing corpus callers spell it
/// (`examples/trajectory/tots_optimal_ptp.ri`). Retained verbatim by task
/// 6096 — the retype is purely additive, so no linear caller moves.
const PRISMATIC_SNIPPET: &str = r#"
structure def PrismaticLimitsE2E {
    let wp0 = Waypoint(t: 0.0s, values: [0.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [1.0], vels: none, accels: none)

    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    // Linear actuator limit: 1000 N of FORCE on a translational DOF.
    // `joint` stays BARE — `param joint : Real` is a dimensionless joint INDEX.
    let limit = JointLimit(joint: 0.0, max_force: 1000N)

    // Linear kinematic limits: 300 mm/s (SI 0.3 m/s) and 5000 mm/s²
    // (SI 5.0 m/s²) — the esc-5758-2 option-B mm-scale spellings.
    let shaper = TOTSShaper(
        modes: [],
        actuator_limits: [limit],
        velocity_limit: 300mm/s,
        acceleration_limit: 5000mm/s^2,
        vibration_tolerance: 0.02
    )

    let shaped = input_shape(profile, shaper)
}
"#;

/// REVOLUTE / rotational fixture: `RevoluteJointLimit` + `RevoluteTOTSShaper`,
/// the half added by task 6096. Structurally identical to the prismatic
/// fixture above — same field names, same ctor shape — differing ONLY in the
/// three dimensioned literals (N → N·m, m/s → rad/s, m/s² → rad/s²).
const REVOLUTE_SNIPPET: &str = r#"
structure def RevoluteLimitsE2E {
    let wp0 = Waypoint(t: 0.0s, values: [0.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [1.0], vels: none, accels: none)

    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    // Rotational actuator limit: 1000 N·m of TORQUE on a revolute DOF.
    // `joint` stays BARE for the same reason as the prismatic half.
    let limit = RevoluteJointLimit(joint: 0.0, max_force: 1000Nm)

    // Rotational kinematic limits: 2 rad/s and 5 rad/s².
    let shaper = RevoluteTOTSShaper(
        modes: [],
        actuator_limits: [limit],
        velocity_limit: 2.0rad/s,
        acceleration_limit: 5.0rad/s^2,
        vibration_tolerance: 0.02
    )

    let shaped = input_shape(profile, shaper)
}
"#;

// ── Harness ─────────────────────────────────────────────────────────────────────

/// The rad·s⁻² vector, composed the way the `.ri` decl composes it:
/// `Rate<AngularVelocity>` = AngularVelocity / Time. There is deliberately no
/// `DimensionVector::ANGULAR_ACCELERATION` to read — that name is chartered to
/// `docs/prds/v0_6/angle-dimension-completion.md` leaf δ and has not landed.
fn angular_acceleration_vector() -> DimensionVector {
    DimensionVector::ANGULAR_VELOCITY.div(&DimensionVector::TIME)
}

/// Compile `source` against the production stdlib asserting ZERO Error
/// diagnostics, then evaluate it through the real engine with the compute fns
/// registered (so `input_shape` dispatches through the π trampoline rather
/// than the body-inline fallback), asserting zero eval-time Error diagnostics.
///
/// Returns the `EvalResult` for field pinning. Both cleanliness gates live
/// here rather than in each test so a fixture can never be "green" merely by
/// virtue of nobody having checked its diagnostics.
fn compile_and_eval_clean(source: &str, what: &str) -> EvalResult {
    let compiled = compile_source_with_stdlib(source);

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "{what} should compile with zero Error diagnostics (task 6096); got:\n{:#?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "{what} should evaluate with zero Error diagnostics (task 6096); got:\n{:#?}",
        eval_errors
    );

    result
}

/// Fetch a `Value::StructureInstance` cell by name, panicking with the full
/// diagnostic set if it is missing or the wrong variant.
fn instance_cell<'a>(
    result: &'a EvalResult,
    structure: &str,
    cell: &str,
) -> &'a reify_ir::StructureInstanceData {
    let id = ValueCellId::new(structure, cell);
    let value = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "{structure}.{cell} cell missing from eval result (task 6096); \
             all diagnostics: {:#?}",
            result.diagnostics
        )
    });
    match value {
        Value::StructureInstance(data) => data,
        other => {
            panic!("{structure}.{cell} should be a StructureInstance (task 6096), got {other:?}")
        }
    }
}

/// Assert a field is a dimensioned `Value::Scalar` with exactly this SI
/// magnitude AND this dimension.
///
/// Matches the variant explicitly rather than routing through an f64 coercion
/// helper: `read_scalar_si`-style coercion folds `Real` and `Scalar` together
/// and discards the dimension, which would make the whole point of this file
/// — that a revolute limit carries a ROTATIONAL vector — unobservable.
fn pin(value: &Value, expected_si: f64, expected_dim: DimensionVector, what: &str) {
    match value {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension, expected_dim,
                "{what}: wrong dimension (task 6096) — expected {expected_dim:?}, \
                 got {dimension:?}"
            );
            assert_eq!(
                *si_value, expected_si,
                "{what}: wrong SI magnitude (task 6096) — expected {expected_si}, \
                 got {si_value}"
            );
        }
        Value::Real(r) => panic!(
            "{what}: a BARE Value::Real({r}) (task 6096) — expected a dimensioned \
             Value::Scalar {{ si_value: {expected_si}, dimension: {expected_dim:?} }}. \
             A bare literal at a dimensioned slot is read as SI and discards the \
             per-joint-kind distinction entirely."
        ),
        other => panic!("{what}: expected a dimensioned Value::Scalar (task 6096), got {other:?}"),
    }
}

/// Read a structure field by name.
fn field<'a>(data: &'a reify_ir::StructureInstanceData, name: &str, what: &str) -> &'a Value {
    data.fields
        .get(&name.to_string())
        .unwrap_or_else(|| panic!("{what}.{name} field missing (task 6096)"))
}

/// Read the single element of an `actuator_limits` list.
fn sole_limit<'a>(
    data: &'a reify_ir::StructureInstanceData,
    what: &str,
) -> &'a reify_ir::StructureInstanceData {
    let Value::List(limits) = field(data, "actuator_limits", what) else {
        panic!("{what}.actuator_limits should be a Value::List (task 6096)");
    };
    assert_eq!(
        limits.len(),
        1,
        "{what}.actuator_limits should hold exactly the one limit the fixture \
         constructs (task 6096); got {}",
        limits.len()
    );
    match &limits[0] {
        Value::StructureInstance(limit) => limit,
        other => panic!(
            "{what}.actuator_limits[0] should be a StructureInstance (task 6096), got {other:?}"
        ),
    }
}

// ── Acceptance: the PRISMATIC half keeps the linear spellings ──────────────────

/// The linear half is untouched by task 6096: a `JointLimit` carries a
/// `Scalar<Force>` and a `TOTSShaper` carries `Scalar<Velocity>` /
/// `Scalar<Acceleration>`, check-clean and eval-green.
///
/// This is the CONTROL for the revolute test below. It is what makes the
/// duality falsifiable: if a regression collapsed the two halves onto one set
/// of vectors, one of these two tests would fail whichever way it collapsed.
#[test]
fn prismatic_fixture_carries_linear_limit_dimensions() {
    let result = compile_and_eval_clean(PRISMATIC_SNIPPET, "the prismatic fixture");

    let shaper = instance_cell(&result, "PrismaticLimitsE2E", "shaper");
    assert_eq!(
        shaper.type_name, "TOTSShaper",
        "the prismatic fixture's shaper should be a TOTSShaper (task 6096)"
    );

    pin(
        field(shaper, "velocity_limit", "PrismaticLimitsE2E.shaper"),
        0.3,
        DimensionVector::VELOCITY,
        "PrismaticLimitsE2E.shaper.velocity_limit (300mm/s)",
    );
    pin(
        field(shaper, "acceleration_limit", "PrismaticLimitsE2E.shaper"),
        5.0,
        DimensionVector::ACCELERATION,
        "PrismaticLimitsE2E.shaper.acceleration_limit (5000mm/s^2)",
    );

    let limit = sole_limit(shaper, "PrismaticLimitsE2E.shaper");
    assert_eq!(
        limit.type_name, "JointLimit",
        "the prismatic fixture's actuator limit should be a JointLimit (task 6096)"
    );
    pin(
        field(
            limit,
            "max_force",
            "PrismaticLimitsE2E.shaper.actuator_limits[0]",
        ),
        1000.0,
        DimensionVector::FORCE,
        "PrismaticLimitsE2E.shaper.actuator_limits[0].max_force (1000N)",
    );
}

// ── Acceptance: the REVOLUTE half takes the rotational spellings ───────────────

/// The revolute half (task 6096): a `RevoluteJointLimit` carries a
/// `Scalar<Torque>` and a `RevoluteTOTSShaper` carries
/// `Scalar<AngularVelocity>` / `Rate<AngularVelocity>`, check-clean and
/// eval-green.
///
/// The three `assert_ne!`-free pins are sufficient here because the vectors
/// are compared for EQUALITY against the angular constants: TORQUE carries an
/// Angle⁻¹ slot and the two rate vectors carry an Angle⁺¹ slot, none of which
/// the linear constants have, so a collapse onto FORCE / VELOCITY /
/// ACCELERATION fails these equality assertions directly.
#[test]
fn revolute_fixture_carries_rotational_limit_dimensions() {
    let result = compile_and_eval_clean(REVOLUTE_SNIPPET, "the revolute fixture");

    let shaper = instance_cell(&result, "RevoluteLimitsE2E", "shaper");
    assert_eq!(
        shaper.type_name, "RevoluteTOTSShaper",
        "the revolute fixture's shaper should be a RevoluteTOTSShaper (task 6096)"
    );

    pin(
        field(shaper, "velocity_limit", "RevoluteLimitsE2E.shaper"),
        2.0,
        DimensionVector::ANGULAR_VELOCITY,
        "RevoluteLimitsE2E.shaper.velocity_limit (2.0rad/s)",
    );
    pin(
        field(shaper, "acceleration_limit", "RevoluteLimitsE2E.shaper"),
        5.0,
        angular_acceleration_vector(),
        "RevoluteLimitsE2E.shaper.acceleration_limit (5.0rad/s^2)",
    );

    let limit = sole_limit(shaper, "RevoluteLimitsE2E.shaper");
    assert_eq!(
        limit.type_name, "RevoluteJointLimit",
        "the revolute fixture's actuator limit should be a RevoluteJointLimit (task 6096)"
    );
    pin(
        field(
            limit,
            "max_force",
            "RevoluteLimitsE2E.shaper.actuator_limits[0]",
        ),
        1000.0,
        DimensionVector::TORQUE,
        "RevoluteLimitsE2E.shaper.actuator_limits[0].max_force (1000Nm)",
    );
}

// ── The silent-Undef dispatch gap ──────────────────────────────────────────────

/// `input_shape(profile, shaper)` must return a real shaped `Profile` for
/// BOTH joint kinds — never `Value::Undef`.
///
/// This is the half that is genuinely RED before the dispatch is widened. The
/// eval-side shaper arm is a raw string equality on `type_name`
/// (`input_shape.rs` and `trampoline.rs`), so a `RevoluteTOTSShaper` falls
/// through to `build_train_for_shaper` — which knows only ZV/ZVD/EI/Cascaded —
/// gets `None`, and yields `Value::Undef` with exit 0 and ZERO diagnostics.
///
/// That silent-Undef trap is documented in-tree at
/// `examples/trajectory/printer_print_envelope.ri`, and it is precisely why
/// the dispatch must be widened in the SAME change that introduces the type:
/// shipping the surface without it would ship something that looks green from
/// every angle except the value it produces.
#[test]
fn input_shape_returns_a_profile_for_both_joint_kinds() {
    for (snippet, structure, what) in [
        (PRISMATIC_SNIPPET, "PrismaticLimitsE2E", "TOTSShaper"),
        (REVOLUTE_SNIPPET, "RevoluteLimitsE2E", "RevoluteTOTSShaper"),
    ] {
        let result = compile_and_eval_clean(snippet, structure);
        let shaped = instance_cell(&result, structure, "shaped");
        assert_eq!(
            shaped.type_name, "PiecewisePolynomialProfile",
            "input_shape(profile, {what}) should return a shaped \
             PiecewisePolynomialProfile, not Value::Undef (task 6096) — an Undef \
             here means the eval-side shaper dispatch does not recognise \
             {what} and fell through to build_train_for_shaper"
        );
    }
}
