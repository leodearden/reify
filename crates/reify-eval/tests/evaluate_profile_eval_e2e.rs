//! End-to-end eval tests for `evaluate_profile` / `profile_duration` reaching
//! the real spline evaluator (task 4539 — β-3816 residue).
//!
//! Compiles an inline `.ri` snippet that constructs a 3-waypoint
//! `PiecewisePolynomialProfile` (natural cubic, values [1.0]/[3.0]/[2.0] at
//! t=0s/1s/2s), wraps it through the `ProfileInput` shim, and binds
//!
//! ```text
//! let q   = evaluate_profile(pi.profile, 1.0s)
//! let dur = profile_duration(pi.profile)
//! ```
//!
//! then asserts:
//!
//! (i) VALUE — `q` is `Value::List([Value::Real(3.0)])` within 1e-9 (exact at
//!     the interior knot for a natural-cubic spline, proving the `[0.0]` stub
//!     path is gone), and `dur` is `Value::Scalar{TIME, si_value≈2.0}`.
//!     `evaluate_profile` is a plain delegate (not `@optimized`), so no
//!     `register_compute_fns` is needed — `make_simple_engine()` suffices.
//!
//! (ii) IR CONTRACT — the four stdlib function bodies (`evaluate_profile`,
//!      `evaluate_profile_dot`, `evaluate_profile_ddot`, `profile_duration`)
//!      each compile to `CompiledExprKind::FunctionCall` naming the undeclared
//!      `*_at` delegate intrinsic, and the `q` call site lowers to
//!      `CompiledExprKind::UserFunctionCall("evaluate_profile")`.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Inline source ──────────────────────────────────────────────────────────────

/// A 3-waypoint natural-cubic `PiecewisePolynomialProfile` with values
/// [1.0]/[3.0]/[2.0] at t=0s/1s/2s, sampled at the interior knot t=1s and
/// queried for duration. Natural cubic interpolates exactly at every knot
/// (spline.rs:796), so `q` at t=1s equals the waypoint value 3.0 exactly.
const SNIPPET: &str = r#"
structure def EvaluateProfileE2E {
    let wp0 = Waypoint(t: 0.0s, values: [1.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [3.0], vels: none, accels: none)
    let wp2 = Waypoint(t: 2.0s, values: [2.0], vels: none, accels: none)

    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1, wp2],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    // ProfileInput shim: passes the concrete StructureInstance as the declared
    // `Profile` trait type (overload resolver requires exact type equality).
    let pi = ProfileInput(profile: profile)

    // Sample position at the interior knot (1s) — must equal 3.0 exactly.
    let q = evaluate_profile(pi.profile, 1.0s)

    // Duration = last_knot - first_knot = 2s - 0s = 2s.
    let dur = profile_duration(pi.profile)
}
"#;

/// Parse + compile the snippet under the stdlib prelude, caching the result.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(SNIPPET))
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRIMARY: eval-path value assertions
// ═══════════════════════════════════════════════════════════════════════════════

const E2E_TOL: f64 = 1e-9;

/// `EvaluateProfileE2E.q` must evaluate to `Value::List([Value::Real(3.0)])`.
/// Natural cubic interpolates exactly at every knot, so at t=1s the result
/// equals the waypoint value 3.0. A `[0.0]` result proves the stub body is
/// still live (this is the primary indicator that task 4539 is incomplete).
/// No `register_compute_fns` needed — `evaluate_profile` is a plain delegate.
#[test]
fn evaluate_profile_at_knot_equals_waypoint_value() {
    let compiled = compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("EvaluateProfileE2E", "q");
    let q_val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("EvaluateProfileE2E.q cell missing from eval result"));

    let Value::List(items) = q_val else {
        panic!(
            "EvaluateProfileE2E.q should be Value::List, got {q_val:?} — \
             if Undef: stub body still live or dispatch broken; \
             if List([0.0]): .ri body not yet replaced with delegate"
        );
    };
    assert_eq!(
        items.len(),
        1,
        "evaluate_profile on a 1-joint profile should return a 1-element list"
    );
    let Value::Real(q) = items[0] else {
        panic!(
            "EvaluateProfileE2E.q[0] should be Value::Real, got {:?}",
            items[0]
        );
    };
    assert!(
        (q - 3.0).abs() < E2E_TOL,
        "evaluate_profile at knot t=1s should equal the waypoint value 3.0, got {q}"
    );
    assert!(
        q > E2E_TOL,
        "evaluate_profile returned [0.0] — the .ri stub body is still active"
    );
}

/// `EvaluateProfileE2E.dur` must be `Value::Scalar{{TIME, si_value≈2.0}}`.
/// Profile spans [0s, 2s], so `profile_duration` = 2.0s.
#[test]
fn profile_duration_equals_knot_span() {
    let compiled = compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("EvaluateProfileE2E", "dur");
    let dur_val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("EvaluateProfileE2E.dur cell missing from eval result"));

    let Value::Scalar { si_value, dimension } = *dur_val else {
        panic!(
            "EvaluateProfileE2E.dur should be Value::Scalar, got {dur_val:?} — \
             if Undef: stub body still live; if Scalar{{0.0}}: .ri body not yet replaced"
        );
    };
    assert_eq!(
        dimension,
        reify_core::DimensionVector::TIME,
        "profile_duration should carry TIME dimension"
    );
    assert!(
        (si_value - 2.0).abs() < E2E_TOL,
        "profile_duration should return 2.0s (last_knot - first_knot), got {si_value}s"
    );
    assert!(
        si_value > E2E_TOL,
        "profile_duration returned 0s — the .ri stub body is still active"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// IR dispatch-contract regression guard
// ═══════════════════════════════════════════════════════════════════════════════

/// Pins the two simultaneous properties the `*_at` delegate scheme depends on:
///
/// 1. `EvaluateProfileE2E.q` compiles to
///    `CompiledExprKind::UserFunctionCall { function_name: "evaluate_profile" }`
///    — the `.ri` declaration shadows the builtin name, and the call site uses
///    the declared `-> List<JointValue>` return type.
///
/// 2. Each of the four stdlib function bodies (`evaluate_profile`,
///    `evaluate_profile_dot`, `evaluate_profile_ddot`, `profile_duration`)
///    compiles its `result_expr` to `CompiledExprKind::FunctionCall` naming
///    the undeclared `*_at` intrinsic (`evaluate_profile_at` etc.) — confirming
///    the body delegates via a name that resolves `NoUserFunctions` →
///    `FunctionCall` → `eval_builtin` (not back into the public name).
#[test]
fn evaluate_profile_family_dispatch_ir_contract() {
    use reify_ir::CompiledExprKind;

    let compiled = compiled();

    // ── Part 1: call site in EvaluateProfileE2E.q ────────────────────────────
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "EvaluateProfileE2E")
        .expect("EvaluateProfileE2E template should exist in compiled module");

    let q_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "q")
        .expect("EvaluateProfileE2E.q value cell should exist");

    let init_expr = q_cell
        .default_expr
        .as_ref()
        .expect("EvaluateProfileE2E.q should have a default_expr (let binding)");

    match &init_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "evaluate_profile",
                "EvaluateProfileE2E.q should call 'evaluate_profile' as a UserFunctionCall"
            );
        }
        other => panic!(
            "EvaluateProfileE2E.q init expr should be \
             UserFunctionCall(\"evaluate_profile\"), got: {other:?}"
        ),
    }

    // ── Part 2: stdlib function bodies ───────────────────────────────────────
    let stdlib_modules = reify_compiler::stdlib_loader::load_stdlib();

    // (fn_name, expected_delegate_name) pairs for all four functions.
    let cases = [
        ("evaluate_profile", "evaluate_profile_at"),
        ("evaluate_profile_dot", "evaluate_profile_dot_at"),
        ("evaluate_profile_ddot", "evaluate_profile_ddot_at"),
        ("profile_duration", "profile_duration_at"),
    ];

    for (fn_name, delegate_name) in cases {
        let stdlib_fn = stdlib_modules
            .iter()
            .flat_map(|m| m.functions.iter())
            .find(|f| f.name == fn_name)
            .unwrap_or_else(|| {
                panic!("stdlib '{fn_name}' function should appear in a stdlib module")
            });

        match &stdlib_fn.body.result_expr.kind {
            CompiledExprKind::FunctionCall { function, .. } => {
                assert_eq!(
                    function.name, delegate_name,
                    "stdlib '{fn_name}' body should delegate to '{delegate_name}' \
                     as a FunctionCall (builtin path), got: {:?}",
                    function.name
                );
            }
            other => panic!(
                "stdlib '{fn_name}' body result_expr should be \
                 FunctionCall(\"{delegate_name}\"), got: {other:?} — \
                 the .ri stub body has not yet been replaced with the delegate call"
            ),
        }
    }
}
