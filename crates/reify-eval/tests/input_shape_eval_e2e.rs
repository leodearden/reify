//! End-to-end eval tests for `input_shape` reaching the real eval path
//! (task 3867 — input-shaping ζ).
//!
//! Compiles an inline `.ri` snippet that constructs a `PiecewisePolynomialProfile`
//! and a `ZVDShaper`, then binds
//!
//! ```text
//! let shaped = input_shape(ProfileInput(profile: p).profile,
//!                          ShaperInput(shaper: s).shaper)
//! ```
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
use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Inline source ───────────────────────────────────────────────────────────────

/// A `PiecewisePolynomialProfile` + `ZVDShaper` passed through the
/// `ProfileInput` / `ShaperInput` trait-coercion shims into `input_shape`.
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

    // Trait-coercion shims (overload resolver uses exact type equality, so the
    // concrete structs cannot match input_shape's `Profile` / `Shaper` params
    // directly — member access on the shim carries the declared trait type).
    let pi = ProfileInput(profile: profile)
    let si = ShaperInput(shaper: shaper)

    let shaped = input_shape(pi.profile, si.shaper)

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
/// passed through the `ProfileInput` / `ShaperInput` trait-coercion shims
/// into `input_shape`. `modes: []` infers `List<Mode>` from the param-type
/// context in the TOTSShaper ctor.
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

    // A single per-joint actuator limit.
    let jl = JointLimit(joint: 0.0, max_force: 100.0)

    // TOTS shaper: time-optimal trajectory shaping.
    // modes: [] infers List<Mode> from the TOTSShaper.modes param type.
    let shaper = TOTSShaper(
        modes: [],
        actuator_limits: [jl],
        velocity_limit: 300.0,
        acceleration_limit: 5000.0,
        vibration_tolerance: 0.02
    )

    // Trait-coercion shims (overload resolver uses exact type equality, so the
    // concrete structs cannot match input_shape's Profile / Shaper params
    // directly — member access on the shim carries the declared trait type).
    let pi = ProfileInput(profile: profile)
    let si = ShaperInput(shaper: shaper)

    let shaped = input_shape(pi.profile, si.shaper)

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
