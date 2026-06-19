//! End-to-end eval tests for the `evaluate_profile` family / `profile_duration`
//! reaching the real spline evaluator (task 4539 — β-3816 residue).
//!
//! Compiles an inline `.ri` snippet that constructs a 3-waypoint
//! `PiecewisePolynomialProfile` (natural cubic, values [1.0]/[3.0]/[2.0] at
//! t=0s/1s/2s) and binds
//!
//! ```text
//! let q   = evaluate_profile(profile, 1.0s)
//! let qd  = evaluate_profile_dot(profile, 1.0s)
//! let qdd = evaluate_profile_ddot(profile, 1.0s)
//! let dur = profile_duration(profile)
//! ```
//! (the concrete `profile` is passed DIRECTLY to each `p : Profile` trait
//! param — the entity-scope conformance post-pass accepts a conforming
//! concrete, so no coercion shim is needed)
//!
//! then asserts:
//!
//! VALUE — `q` is `Value::List([Value::Real(3.0)])` within 1e-9 (exact at the
//! interior knot), `qd` is within 1e-9 of 0.5 (first derivative at t=1 for
//! this natural cubic), `qdd` is within 1e-9 of -4.5 (second derivative), and
//! `dur` is `Value::Scalar{TIME, si_value≈2.0}`. No `register_compute_fns`
//! needed — all four functions are plain delegates.
//!
//! IR CONTRACT — the `q` call site lowers to
//! `CompiledExprKind::UserFunctionCall("evaluate_profile")`, confirming the
//! `.ri` declaration shadows the builtin name at the call site.

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
/// queried for duration and derivatives. Natural cubic interpolates exactly at
/// every knot (spline.rs:796), so `q` at t=1s equals 3.0 exactly. For this
/// specific knot set the first derivative at t=1 is 0.5 and the second is -4.5
/// (both exact in natural-cubic arithmetic).
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

    // The concrete profile is passed DIRECTLY to the `p : Profile` trait param
    // of each accessor — the entity-scope conformance post-pass accepts a
    // conforming concrete at a trait-typed param, so no coercion shim is needed.

    // Position at interior knot (1s) — must equal 3.0 exactly.
    let q   = evaluate_profile(profile, 1.0s)
    // First derivative at t=1s — must equal 0.5 exactly.
    let qd  = evaluate_profile_dot(profile, 1.0s)
    // Second derivative at t=1s — must equal -4.5 exactly.
    let qdd = evaluate_profile_ddot(profile, 1.0s)

    // Duration = last_knot - first_knot = 2s - 0s = 2s.
    let dur = profile_duration(profile)
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
/// equals the waypoint value 3.0. A `[0.0]` or `Undef` result proves the stub
/// body is still live. No `register_compute_fns` needed — plain delegate.
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
        "evaluate_profile at knot t=1s should equal waypoint value 3.0, got {q} \
         (0.0 means the .ri stub body is still active)"
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
        "profile_duration should return 2.0s (last_knot - first_knot), got {si_value}s \
         (0.0 means the .ri stub body is still active)"
    );
}

/// `EvaluateProfileE2E.qd` must equal 0.5 (first derivative at t=1s for the
/// natural cubic on these 3 knots — exact in natural-cubic arithmetic).
/// A `[0.0]` or `Undef` result proves the _dot delegate path is broken.
#[test]
fn evaluate_profile_dot_at_knot_is_correct() {
    let compiled = compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("EvaluateProfileE2E", "qd");
    let qd_val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("EvaluateProfileE2E.qd cell missing from eval result"));

    let Value::List(items) = qd_val else {
        panic!(
            "EvaluateProfileE2E.qd should be Value::List, got {qd_val:?} — \
             if Undef: dispatch broken; if List([0.0]): _dot delegate not wired"
        );
    };
    assert_eq!(items.len(), 1, "evaluate_profile_dot on a 1-joint profile should return a 1-element list");
    let Value::Real(qd) = items[0] else {
        panic!("EvaluateProfileE2E.qd[0] should be Value::Real, got {:?}", items[0]);
    };
    assert!(
        (qd - 0.5).abs() < E2E_TOL,
        "evaluate_profile_dot at t=1s should equal 0.5 (natural-cubic first derivative), \
         got {qd} (0.0 means the .ri stub body is still active)"
    );
}

/// `EvaluateProfileE2E.qdd` must equal -4.5 (second derivative at t=1s for the
/// natural cubic on these 3 knots — equal to the inner second-derivative M_1,
/// exact in natural-cubic arithmetic).
#[test]
fn evaluate_profile_ddot_at_knot_is_correct() {
    let compiled = compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("EvaluateProfileE2E", "qdd");
    let qdd_val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("EvaluateProfileE2E.qdd cell missing from eval result"));

    let Value::List(items) = qdd_val else {
        panic!(
            "EvaluateProfileE2E.qdd should be Value::List, got {qdd_val:?} — \
             if Undef: dispatch broken; if List([0.0]): _ddot delegate not wired"
        );
    };
    assert_eq!(items.len(), 1, "evaluate_profile_ddot on a 1-joint profile should return a 1-element list");
    let Value::Real(qdd) = items[0] else {
        panic!("EvaluateProfileE2E.qdd[0] should be Value::Real, got {:?}", items[0]);
    };
    assert!(
        (qdd - (-4.5)).abs() < E2E_TOL,
        "evaluate_profile_ddot at t=1s should equal -4.5 (natural-cubic M_1), \
         got {qdd} (0.0 means the .ri stub body is still active)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// IR dispatch-contract regression guard
// ═══════════════════════════════════════════════════════════════════════════════

/// Pins the non-obvious shadowing property: `EvaluateProfileE2E.q` compiles to
/// `CompiledExprKind::UserFunctionCall { function_name: "evaluate_profile" }`,
/// confirming the `.ri` declaration shadows the builtin name at the call site.
/// The value/derivative/duration tests already verify that dispatch reaches the
/// real evaluator, so this test only pins the call-site IR shape (the part the
/// value tests cannot directly observe).
#[test]
fn evaluate_profile_call_site_is_user_function_call() {
    use reify_ir::CompiledExprKind;

    let compiled = compiled();

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
}
