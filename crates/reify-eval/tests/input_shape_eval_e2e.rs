//! End-to-end eval tests for `input_shape` reaching the real eval path
//! (task 3867 — input-shaping ζ).
//!
//! Compiles an inline `.ri` snippet that constructs a `PiecewisePolynomialProfile`
//! and a `ZVDShaper`, then binds
//!
//! ```text
//! let shaped = input_shape(p, s)
//! ```
//! (the concrete `p` / `s` are passed DIRECTLY to the `Profile` / `Shaper`
//! trait params — the entity-scope conformance post-pass accepts a conforming
//! concrete, so no coercion shim is needed)
//!
//! runs the engine (`make_simple_engine` + `engine.eval`) and asserts that the
//! `shaped` value cell resolves to a real `Value::StructureInstance` typed
//! `PiecewisePolynomialProfile` — the eval-path `eval_input_shape` dispatch
//! echoes the input profile's `StructureInstanceData` (its already-registered
//! `type_id`, so binding back into the typed `shaped: Profile` cell validates
//! against the `StructureRegistry`; design decision: type_id echo).
//!
//! Also pins the dispatch IR contract, mirroring
//! `gcode_import_eval_e2e.rs::gcode_import_dispatch_ir_contract`:
//!   1. the `input_shape(...)` call site lowers to
//!      `CompiledExprKind::UserFunctionCall` (the `.ri` declaration shadows the
//!      builtin name and the declared `-> Profile` signature applies), and
//!   2. the delegate `input_shape_apply` inside the stdlib body lowers to
//!      `CompiledExprKind::FunctionCall` (`NoUserFunctions` → `eval_builtin`).
//!
//! The full surface+dispatch is wired by steps 1–6, so these assertions are
//! GREEN; the test is a regression guard on the end-to-end `input_shape` path.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Inline source ───────────────────────────────────────────────────────────────

/// A `PiecewisePolynomialProfile` + `ZVDShaper` passed DIRECTLY (no coercion
/// shim) to `input_shape`'s `Profile` / `Shaper` trait params.
const SNIPPET: &str = r#"
structure def InputShapeE2E {
    // Two-waypoint linear ramp over [0 s, 1 s], one joint (scalar Real).
    let wp0 = Waypoint(t: 0.0s, values: [0.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [1.0], vels: none, accels: none)

    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    // ZVD shaper: suppress 10 Hz resonance with 5 % critical damping.
    let shaper = ZVDShaper(target_frequency: 10Hz, damping_ratio: 0.05)

    // The concrete profile / shaper are passed DIRECTLY to input_shape's
    // `Profile` / `Shaper` trait params — the entity-scope conformance post-pass
    // accepts a conforming concrete at a trait-typed param, so no coercion shim
    // is needed.
    let shaped = input_shape(profile, shaper)

    // Trivially satisfiable leaf constraint.
    constraint shaper.damping_ratio >= 0.0
}
"#;

/// Parse + compile the snippet under the stdlib prelude, caching the result.
/// `parse_and_compile_with_stdlib` asserts zero compile errors internally (so a
/// regression that breaks the `input_shape` surface panics here with the
/// diagnostics), and is prelude-aware so `SplineKind.CubicSpline` resolves as
/// an `EnumAccess`.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(SNIPPET))
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRIMARY: eval-path assertion — shaped is a Profile StructureInstance
// ═══════════════════════════════════════════════════════════════════════════════

/// `InputShapeE2E.shaped` must evaluate to a `Value::StructureInstance` whose
/// `type_name` is `PiecewisePolynomialProfile`. With the compute fns registered
/// (above), the @optimized `input_shape` dispatches through the π trampoline
/// (`input_shape_value`), which does real impulse shaping (resampling the
/// convolved command into new waypoints); real shaping changes only `waypoints`
/// and preserves `type_name`, so this assertion holds. On the unregistered /
/// body-inline path the body echoes the profile — same `type_name` either way.
#[test]
fn input_shape_shaped_is_profile_structure_instance() {
    let compiled = compiled();
    let mut engine = make_simple_engine();
    // input_shape is @optimized("trajectory::input_shape") (task π): register the
    // compute fns so the call dispatches through the real trampoline rather than
    // the unregistered-target body-inline fallback (make_simple_engine registers
    // none — design-decision-10). Real shaping changes only `waypoints` and
    // preserves `type_name`, so the assertion below holds on either path.
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.eval(compiled);

    let id = ValueCellId::new("InputShapeE2E", "shaped");
    let shaped = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("InputShapeE2E.shaped cell missing from eval result"));

    match shaped {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PiecewisePolynomialProfile",
                "InputShapeE2E.shaped should echo the input profile's type_name \
                 (PiecewisePolynomialProfile), got {:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Value::StructureInstance for InputShapeE2E.shaped, got {other:?} — \
             input_shape dispatch may be returning Value::Undef (build_train_for_shaper \
             failed to recognise the ZVDShaper) or the .ri surface is unwired"
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// IR dispatch-contract regression guard (mirrors gcode_import_dispatch_ir_contract)
// ═══════════════════════════════════════════════════════════════════════════════

/// Pins the two simultaneous properties the `input_shape` → `input_shape_apply`
/// delegate scheme depends on:
///
/// 1. `InputShapeE2E.shaped` compiles to
///    `CompiledExprKind::UserFunctionCall { function_name: "input_shape" }` —
///    the `.ri` declaration shadows the builtin name (so the evaluator runs the
///    body and the call site gets the `-> Profile` result type). NOTE: task π
///    made `input_shape` `@optimized("trajectory::input_shape")`, but `@optimized`
///    does NOT change the static call-site kind — it stays `UserFunctionCall`;
///    the engine reads `optimized_target` and inserts the ComputeNode at eval
///    time (only when the trampoline is registered, `engine_eval.rs:3346/3405`).
///    The eval-time ComputeNode presence is asserted in
///    `simulate_trajectory_eval_e2e.rs` / `input_shape_tots_compute_node.rs`.
///
/// 2. the stdlib `input_shape` function body's result expression compiles to
///    `CompiledExprKind::FunctionCall { function: "input_shape_apply" }` —
///    confirming the body delegates via the *undeclared* name that resolves
///    `NoUserFunctions` → `FunctionCall` → `eval_builtin` (not recursively back
///    into `input_shape`).
#[test]
fn input_shape_dispatch_ir_contract() {
    use reify_ir::CompiledExprKind;

    let compiled = compiled();

    // ── Part 1: call site in InputShapeE2E.shaped ─────────────────────────────
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "InputShapeE2E")
        .expect("InputShapeE2E template should exist in compiled module");

    let shaped_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "shaped")
        .expect("InputShapeE2E.shaped value cell should exist");

    let init_expr = shaped_cell
        .default_expr
        .as_ref()
        .expect("InputShapeE2E.shaped should have a default_expr (let binding)");

    match &init_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "input_shape",
                "InputShapeE2E.shaped should call 'input_shape' as a UserFunctionCall \
                 — if this fails the .ri declaration may have been removed or the \
                 resolver changed to prefer builtins"
            );
        }
        other => panic!(
            "InputShapeE2E.shaped init expr should be UserFunctionCall(\"input_shape\"), \
             got: {other:?}"
        ),
    }

    // ── Part 2: body of the stdlib input_shape function ───────────────────────
    let stdlib_modules = reify_compiler::stdlib_loader::load_stdlib();
    let input_shape_fn = stdlib_modules
        .iter()
        .flat_map(|m| m.functions.iter())
        .find(|f| f.name == "input_shape")
        .expect(
            "stdlib input_shape function should appear in one of the stdlib \
             CompiledModules (trajectory stdlib module)",
        );

    match &input_shape_fn.body.result_expr.kind {
        CompiledExprKind::FunctionCall { function, .. } => {
            assert_eq!(
                function.name, "input_shape_apply",
                "input_shape body should call 'input_shape_apply' as a FunctionCall \
                 (stdlib builtin path), got function name: {:?}",
                function.name
            );
        }
        other => panic!(
            "input_shape body result_expr should be FunctionCall(\"input_shape_apply\"), \
             got: {other:?} — the body may have changed or input_shape_apply may now \
             have a .ri declaration (making it resolve as UserFunctionCall)"
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOTS arm e2e guard (task λ — 3872)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Mirrors the ZVD e2e test above, but exercises the TOTSShaper path.
// Pins that input_shape(TOTSShaper) no longer evaluates to Undef through the
// real engine (before λ, TOTSShaper fell through to build_train_for_shaper
// → None → Undef). Complements the unit tests in input_shape.rs::tests which
// call eval_input_shape directly, bypassing the `.ri` surface and engine.

/// A `PiecewisePolynomialProfile` + `TOTSShaper` (with one `JointLimit`)
/// passed DIRECTLY (no coercion shim) to `input_shape`'s `Profile` / `Shaper`
/// trait params. `modes: []` infers `List<Mode>` from the param-type context in
/// the TOTSShaper ctor.
const TOTS_SNIPPET: &str = r#"
structure def InputShapeTOTSE2E {
    // Two-waypoint linear ramp over [0 s, 1 s], one joint (scalar Real).
    let wp0 = Waypoint(t: 0.0s, values: [0.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [1.0], vels: none, accels: none)

    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    // A single per-joint actuator limit. `max_force` is a dimensioned
    // Scalar<Force> literal: `100N` is 100 N in SI, exactly what the previous
    // bare `100.0` was already read as, so this is magnitude-preserving.
    // `joint` stays BARE — `param joint : Real` is a dimensionless joint INDEX.
    let jl = JointLimit(joint: 0.0, max_force: 100N)

    // TOTS shaper: time-optimal trajectory shaping.
    // modes: [] infers List<Mode> from the TOTSShaper.modes param type.
    // The kinematic limits are dimensioned literals at the Scalar<Velocity> /
    // Scalar<Acceleration> param slots: 300 mm/s (= 0.3 m/s) and 5000 mm/s²
    // (= 5 m/s²). They carry Leo's esc-5758-2 option-B ruling — the previous
    // bare `300.0` / `5000.0` were read as SI (300 m/s, 5000 m/s²), so the
    // resulting 1000× change in SI magnitude is DELIBERATE, not a regression.
    let shaper = TOTSShaper(
        modes: [],
        actuator_limits: [jl],
        velocity_limit: 300mm/s,
        acceleration_limit: 5000mm/s^2,
        vibration_tolerance: 0.02
    )

    // The concrete profile / shaper are passed DIRECTLY to input_shape's
    // Profile / Shaper trait params — the entity-scope conformance post-pass
    // accepts a conforming concrete at a trait-typed param, so no coercion shim
    // is needed.
    let shaped = input_shape(profile, shaper)

    // Trivially satisfiable leaf constraint.
    constraint shaper.vibration_tolerance > 0
}
"#;

/// Parse + compile the TOTS snippet under the stdlib prelude, caching the
/// result. Panics with diagnostics on any compile error (regression guard).
fn compiled_tots() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(TOTS_SNIPPET))
}

/// `InputShapeTOTSE2E.shaped` must evaluate to a `Value::StructureInstance`
/// whose `type_name` is `PiecewisePolynomialProfile`. With the compute fns
/// registered (above), the @optimized `input_shape` dispatches through the π
/// trampoline whose `TOTSShaper` arm re-times the move via `solve_tots`; the
/// re-timing changes only `waypoints` and preserves `type_name`, so this
/// assertion holds (on the unregistered / body-inline path the body echoes the
/// profile — same `type_name` either way).
///
/// Before λ, a `TOTSShaper` fell through to `build_train_for_shaper` → None →
/// `Value::Undef`. This test pins that the full `.ri` → shim → delegate /
/// trampoline → TOTS-arm path produces a real `PiecewisePolynomialProfile`.
#[test]
fn input_shape_tots_shaper_echoes_profile() {
    let compiled = compiled_tots();
    let mut engine = make_simple_engine();
    // See input_shape_shaped_is_profile_structure_instance: register the compute
    // fns so input_shape (now @optimized) dispatches through the real trampoline
    // (the TOTS arm re-times the move) rather than the body-inline echo fallback.
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.eval(compiled);

    let id = ValueCellId::new("InputShapeTOTSE2E", "shaped");
    let shaped = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("InputShapeTOTSE2E.shaped cell missing from eval result"));

    match shaped {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PiecewisePolynomialProfile",
                "InputShapeTOTSE2E.shaped should echo the input profile's type_name \
                 (PiecewisePolynomialProfile), got {:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Value::StructureInstance(PiecewisePolynomialProfile) for \
             InputShapeTOTSE2E.shaped, got {other:?} — input_shape(TOTSShaper) may \
             be returning Value::Undef (TOTS arm not wired) or the .ri surface is broken"
        ),
    }
}

/// `InputShapeTOTSE2E.shaper`'s actuator/kinematic limits are constructed from
/// dimensioned unit literals, not bare `Real`s (task 5758 — PRD
/// `docs/prds/v0_6/dimensioned-construction-strictness.md` §6.3 / §11 β).
///
/// ## Two kinds of change, both asserted numerically
///
/// The `max_force` pin and the velocity/acceleration pins differ in kind:
///
///   * `max_force` 100 → `100N` is MAGNITUDE-PRESERVING. A bare literal at a
///     `Scalar<Force>` slot was already read as SI newtons, so the "SI values
///     must not change" invariant still binds here and 100.0 is asserted
///     exactly.
///   * `velocity_limit` 300 → `300mm/s` (SI 0.3) and `acceleration_limit`
///     5000 → `5000mm/s^2` (SI 5.0) are the INTENDED 1000× change ruled by Leo
///     in esc-5758-2 (option B) for this file's TOTS_SNIPPET :255/:256. Read as
///     SI, the bare literals denoted 300 m/s and 5000 m/s².
///
/// Both facts are asserted numerically at the Value layer rather than inferred
/// from compile-cleanliness (decompose-addendum D2 / INV-SF-7): "it still
/// compiles" is exactly the evidence that would NOT have caught either error.
///
/// The match is on `Value::Scalar { si_value, dimension }` explicitly, with
/// `dimension` compared against a named `DimensionVector` constant — never
/// through an f64 coercion helper, which would fold `Value::Real` and
/// `Value::Scalar` together and make this pin vacuous.
#[test]
fn input_shape_tots_shaper_limits_are_dimensioned() {
    let compiled = compiled_tots();
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.eval(compiled);

    let id = ValueCellId::new("InputShapeTOTSE2E", "shaper");
    let shaper = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("InputShapeTOTSE2E.shaper cell missing from eval result"));

    let Value::StructureInstance(data) = shaper else {
        panic!("InputShapeTOTSE2E.shaper must be a TOTSShaper StructureInstance; got {shaper:?}")
    };

    /// Assert a field is a dimensioned `Value::Scalar` with exactly this SI
    /// magnitude and dimension. Panics naming the observed variant so an
    /// un-migrated (or re-bared) ctor arg reads clearly.
    fn pin(v: &Value, expected_si: f64, expected_dim: DimensionVector, what: &str) {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(
                    *dimension, expected_dim,
                    "{what}: wrong dimension — expected {expected_dim:?}, got {dimension:?}"
                );
                assert_eq!(
                    *si_value, expected_si,
                    "{what}: wrong SI magnitude — expected {expected_si}, got {si_value}"
                );
            }
            Value::Real(r) => panic!(
                "{what}: still a BARE Value::Real({r}) — expected a dimensioned \
                 Value::Scalar {{ si_value: {expected_si}, dimension: {expected_dim:?} }}. \
                 The TOTS_SNIPPET ctor arg has not been migrated to a unit literal \
                 (task 5758 / PRD docs/prds/v0_6/dimensioned-construction-strictness.md §11 β)."
            ),
            other => panic!("{what}: expected a dimensioned Value::Scalar, got {other:?}"),
        }
    }

    let field = |name: &str| -> &Value {
        data.fields
            .get(&name.to_string())
            .unwrap_or_else(|| panic!("InputShapeTOTSE2E.shaper.{name} field missing"))
    };

    pin(
        field("velocity_limit"),
        0.3,
        DimensionVector::VELOCITY,
        "InputShapeTOTSE2E.shaper.velocity_limit",
    );
    pin(
        field("acceleration_limit"),
        5.0,
        DimensionVector::ACCELERATION,
        "InputShapeTOTSE2E.shaper.acceleration_limit",
    );

    let Value::List(limits) = field("actuator_limits") else {
        panic!(
            "InputShapeTOTSE2E.shaper.actuator_limits must be a Value::List, got {:?}",
            field("actuator_limits")
        )
    };
    assert_eq!(
        limits.len(),
        1,
        "InputShapeTOTSE2E.shaper.actuator_limits should hold exactly one JointLimit, got {}",
        limits.len()
    );
    let Value::StructureInstance(jl) = &limits[0] else {
        panic!(
            "InputShapeTOTSE2E.shaper.actuator_limits[0] must be a JointLimit \
             StructureInstance; got {:?}",
            limits[0]
        )
    };
    pin(
        jl.fields
            .get(&"max_force".to_string())
            .expect("JointLimit.max_force field missing"),
        100.0,
        DimensionVector::FORCE,
        "InputShapeTOTSE2E.shaper.actuator_limits[0].max_force",
    );
}
