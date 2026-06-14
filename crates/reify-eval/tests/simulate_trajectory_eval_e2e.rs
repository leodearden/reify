//! Compiler-surface + IR dispatch-contract tests for trajectory task π's two
//! `@optimized` ComputeNode trampolines (`simulate_trajectory`, `input_shape`)
//! and the three EndEffectorTrack accessor delegates (`end_effector_track` /
//! `deviation_from_nominal` / `peak_deviation`). Mirrors the IR-inspection
//! pattern in `input_shape_eval_e2e.rs`.
//!
//! WHY the static `optimized_target` field rather than a post-eval ComputeNode
//! graph probe: `@optimized` does NOT change a call site's static
//! `CompiledExprKind` — it stays `UserFunctionCall`. The engine
//! (`engine_eval.rs:3346`) matches the `UserFunctionCall`, reads the resolved
//! `CompiledFunction::optimized_target`, and inserts a ComputeNode into the
//! post-eval graph ONLY when a trampoline is registered for that target
//! (`engine_eval.rs:3405` — `compute_dispatch(&target).is_some()`); an
//! unregistered target emits an Error diagnostic and body-inlines (no node).
//! Registration for the `trajectory::*` targets lands in later steps (24 / 28),
//! so a graph probe here would be registration-gated. `optimized_target` is the
//! registration-independent compile-time surface that decides the lowering, so
//! these tests probe it directly (and the accessor delegate scheme via the fn
//! body `result_expr`, exactly as `input_shape_eval_e2e.rs` Part 2 does for
//! `input_shape` → `input_shape_apply`).
//!
//! RED (step-21): `input_shape` is not yet `@optimized` and the three accessor
//! bodies are still type-correct stubs (`[]` / `[0mm]` / `0mm`), so the
//! optimized-target and delegate-body assertions fail. GREEN once step-22 adds
//! `@optimized("trajectory::input_shape")` and the three delegate bodies.
//! `simulate_trajectory` is already `@optimized` (prereq-2), so its
//! optimized-target assertion is GREEN throughout — it pins the contract.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_ir::CompiledExprKind;
use reify_test_support::parse_and_compile_with_stdlib;

/// Find a stdlib `CompiledFunction` by name across every loaded stdlib module.
/// `load_stdlib` returns a `&'static` cached slice, so the borrow is `'static`.
fn stdlib_fn(name: &str) -> &'static reify_ir::CompiledFunction {
    reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .flat_map(|m| m.functions.iter())
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("stdlib function {name:?} not found in any stdlib module"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// @optimized ComputeNode targets (the two trampolines)
// ═══════════════════════════════════════════════════════════════════════════════

/// Both `simulate_trajectory` (trajectory_fns.ri) and `input_shape`
/// (trajectory.ri) must carry the `@optimized("trajectory::…")` target the
/// engine reads to lower their call sites to a ComputeNode.
///
/// - `simulate_trajectory` → `"trajectory::simulate"` (already `@optimized` at
///   prereq-2; GREEN throughout — the regression pin).
/// - `input_shape` → `"trajectory::input_shape"` (whole fn is `@optimized` so
///   the heavy TOTS arm can be cached; the cheap impulse arms route through the
///   same ComputeNode). RED until step-22 adds the annotation.
#[test]
fn trajectory_optimized_targets_present() {
    assert_eq!(
        stdlib_fn("simulate_trajectory").optimized_target.as_deref(),
        Some("trajectory::simulate"),
        "simulate_trajectory must be @optimized(\"trajectory::simulate\") so its \
         call site dispatches through the simulate trampoline"
    );

    assert_eq!(
        stdlib_fn("input_shape").optimized_target.as_deref(),
        Some("trajectory::input_shape"),
        "input_shape must be @optimized(\"trajectory::input_shape\") — the whole \
         fn is @optimized so the TOTS arm is cache-eligible; the impulse arms \
         route through the same ComputeNode harmlessly (RED until step-22 adds \
         the annotation)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Accessor delegate bodies (delegate-to-undeclared-intrinsic scheme)
// ═══════════════════════════════════════════════════════════════════════════════

/// Each EndEffectorTrack accessor's body must delegate to its undeclared `*_at`
/// intrinsic, so the fn body's `result_expr` is a `CompiledExprKind::FunctionCall`
/// naming the `*_at` delegate (which resolves `NoUserFunctions` → `eval_builtin`
/// → `eval_trajectory`, exactly like `input_shape` → `input_shape_apply`). The
/// `*_at` name is undeclared in the stdlib, so it never resolves back into the
/// declared accessor (no infinite recursion).
///
/// RED until step-22 replaces the type-correct stub bodies (`[]` / `[0mm]` /
/// `0mm`) with the delegate calls.
#[test]
fn trajectory_accessor_bodies_delegate_to_intrinsics() {
    for (accessor, delegate) in [
        ("end_effector_track", "end_effector_track_at"),
        ("deviation_from_nominal", "deviation_from_nominal_at"),
        ("peak_deviation", "peak_deviation_at"),
    ] {
        let f = stdlib_fn(accessor);
        match &f.body.result_expr.kind {
            CompiledExprKind::FunctionCall { function, .. } => {
                assert_eq!(
                    function.name, delegate,
                    "{accessor} body should delegate to the {delegate}() intrinsic"
                );
            }
            other => panic!(
                "{accessor} body result_expr should be FunctionCall({delegate:?}), got: \
                 {other:?} — the type-correct stub body has not yet been replaced with \
                 the delegate-to-intrinsic call (step-22)"
            ),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Surface regression guard: the call sites compile and stay UserFunctionCall
// ═══════════════════════════════════════════════════════════════════════════════
//
// `@optimized` does not change a call site's static CompiledExprKind, so every
// call site below is a UserFunctionCall both before AND after step-22 (the
// `.ri` declarations shadow the same-named builtins; the resolver runs the body
// / the engine reads optimized_target). This guards that step-22's `.ri` edits
// keep the whole surface compiling with zero Error diagnostics.

/// A profile + ZVShaper through the `ProfileInput` / `ShaperInput` shims into
/// `input_shape`, plus the three accessor call sites over a default
/// `EndEffectorTrack()` (its ctor defaults were added in prereq-2). The
/// `location` arg is now a kernel-free topology selector (LocationId = Selector,
/// task 4577) rather than a `0.0` Real; runtime selector→mesh-node resolution
/// is task 4122, until which the accessors return an empty series / 0 Length.
const SURFACE_SNIPPET: &str = r#"
structure def TrajPiSurface {
    let wp0 = Waypoint(t: 0.0s, values: [0.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [1.0], vels: none, accels: none)

    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    let shaper = ZVShaper(target_frequency: 10Hz, damping_ratio: 0.0)

    let pi = ProfileInput(profile: profile)
    let si = ShaperInput(shaper: shaper)
    let shaped = input_shape(pi.profile, si.shaper)

    let b = box(10mm, 10mm, 10mm)
    let dir = vec3(1.0, 0.0, 0.0)
    let tol = 1deg
    let loc = faces_by_normal(b, dir, tol)

    let track = EndEffectorTrack()
    let series = end_effector_track(track, loc)
    let dev = deviation_from_nominal(track, loc)
    let peak = peak_deviation(track, loc)

    constraint shaper.damping_ratio >= 0.0
}
"#;

/// Parse + compile the surface snippet under the stdlib prelude, caching the
/// result. `parse_and_compile_with_stdlib` asserts zero compile errors
/// internally, so a regression that breaks the `input_shape` / accessor surface
/// panics here with the diagnostics.
fn compiled_surface() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(SURFACE_SNIPPET))
}

/// Assert that `TrajPiSurface.<member>`'s initializer is a
/// `UserFunctionCall(function_name)`.
fn assert_user_call(compiled: &CompiledModule, member: &str, function_name: &str) {
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "TrajPiSurface")
        .expect("TrajPiSurface template should exist in compiled module");
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("TrajPiSurface.{member} value cell should exist"));
    let init_expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("TrajPiSurface.{member} should have a default_expr"));
    match &init_expr.kind {
        CompiledExprKind::UserFunctionCall {
            function_name: fname,
            ..
        } => assert_eq!(
            fname, function_name,
            "TrajPiSurface.{member} should call {function_name:?} as a UserFunctionCall"
        ),
        other => panic!(
            "TrajPiSurface.{member} init expr should be UserFunctionCall({function_name:?}), \
             got: {other:?}"
        ),
    }
}

/// The `input_shape` + three accessor call sites all compile and lower to
/// `UserFunctionCall` (the static call-site kind is unchanged by `@optimized`).
#[test]
fn trajectory_surface_call_sites_are_user_function_calls() {
    let compiled = compiled_surface();
    assert_user_call(compiled, "shaped", "input_shape");
    assert_user_call(compiled, "series", "end_effector_track");
    assert_user_call(compiled, "dev", "deviation_from_nominal");
    assert_user_call(compiled, "peak", "peak_deviation");
}
