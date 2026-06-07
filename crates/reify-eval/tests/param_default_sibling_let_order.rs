//! Regression tests for the eval-order bug: a structure `param` whose default
//! expression references a sibling `let` must evaluate correctly regardless of
//! declaration order (spec §8.2 "Order-Independence").
//!
//! Task 4317: collapse the kind-partitioned two-pass (Param-first, then Let) into
//! a single dependency-ordered evaluation over all non-Auto body cells so that
//! param defaults observe sibling let values.
//!
//! All regression tests — core repro, order-independence symmetry, cross-kind
//! cycle, eval_cached cross-path parity — live in this single file per
//! esc-4317-196 scope restriction.

use reify_compiler::CompiledModule;
use reify_core::{Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

/// Build an Engine with an empty prelude for self-contained tests.
fn fresh_engine() -> Engine {
    Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
}

/// Convenience: parse + compile a single-structure source string.
fn compile_source(source: &str) -> CompiledModule {
    parse_and_compile(source)
}

// ──────────────────────────────────────────────────────────────────────────────
// step-1: RED — core repro: param default references a sibling let
// ──────────────────────────────────────────────────────────────────────────────

/// The canonical repro from task 4317.
///
/// Structure:
///   param rope_dia : Length = 6mm
///   let drum_d = rope_dia * 2.0     ← let
///   param feed : Length = 1300mm
///   param p : Real = feed / drum_d  ← param whose default reads the sibling let
///   let out = p
///
/// Numerics (exact IEEE-754, single division):
///   rope_dia = 0.006 m SI
///   drum_d   = 0.006 * 2.0 = 0.012 m SI
///   feed     = 1.3 m SI
///   p        = 1.3 / 0.012 = 108.333333...
///   out      = 108.333333...
///
/// FAILS today because pass-1 evaluates param `p`'s default before the let
/// `drum_d` exists in `values`, yielding silent Undef with no diagnostic.
#[test]
fn param_default_referencing_sibling_let_evaluates_correctly() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure T { \
            param rope_dia : Length = 6mm \
            let drum_d = rope_dia * 2.0 \
            param feed : Length = 1300mm \
            param p : Real = feed / drum_d \
            let out = p \
        }",
    );

    let result = engine.eval(&module);

    let p_id = ValueCellId::new("T", "p");
    let out_id = ValueCellId::new("T", "out");

    // (a) T.p must be Determined — not (Undef, Undetermined).
    let snap = engine.snapshot().unwrap();
    let (p_val, p_det) = snap
        .values
        .get(&p_id)
        .expect("snapshot must contain T.p after eval");
    assert_eq!(
        *p_det,
        DeterminacyState::Determined,
        "T.p must be Determined; two-pass bug left it Undetermined (Undef). \
         Got: ({:?}, {:?})",
        p_val,
        p_det
    );

    // (b) T.p si_value within 1e-9 of 108.333333... (= 1.3 / 0.012).
    let expected_p = 1.3_f64 / 0.012_f64; // ~108.33333333333333
    match result.values.get(&p_id) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!(
                (si_value - expected_p).abs() < 1e-9,
                "T.p si_value must be ~108.333333...; got {si_value} (diff {})",
                (si_value - expected_p).abs()
            );
        }
        other => panic!(
            "T.p must be a Scalar in result.values; got {:?}. \
             (Bug: pass-1 evaluates param before sibling let exists in values.)",
            other
        ),
    }

    // (c) T.out must be Determined and numerically equal to T.p.
    let (_, out_det) = snap
        .values
        .get(&out_id)
        .expect("snapshot must contain T.out after eval");
    assert_eq!(
        *out_det,
        DeterminacyState::Determined,
        "T.out must be Determined; got {:?}",
        out_det
    );
    match result.values.get(&out_id) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!(
                (si_value - expected_p).abs() < 1e-9,
                "T.out si_value must equal T.p (~108.333333...); got {si_value}"
            );
        }
        other => panic!("T.out must be a Scalar in result.values; got {:?}", other),
    }

    // (d) No circular-dependency error diagnostic.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for a valid param->let dependency; \
         got: {:?}",
        errors
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// step-1 extra: order-independence symmetry — let referencing param still works
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock: the reverse direction (let default referencing a sibling
/// param) must STILL work after the fix (it already works today, but we lock
/// it so the fix cannot regress it).
///
///   param w : Length = 50mm
///   let area = w * w
///
/// area should evaluate to 0.05 * 0.05 = 0.0025 m² (LENGTH²).
#[test]
fn let_default_referencing_sibling_param_still_works() {
    let mut engine = fresh_engine();
    let module = compile_source("structure T { param w : Length = 50mm  let area = w * w }");
    let result = engine.eval(&module);

    let area_id = ValueCellId::new("T", "area");

    // area = 0.05 * 0.05 = 0.0025 m²
    match result.values.get(&area_id) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!(
                (si_value - 0.0025).abs() < 1e-12,
                "T.area should be 0.0025 m²; got {si_value}"
            );
        }
        other => panic!("T.area must be a Scalar; got {:?}", other),
    }

    let snap = engine.snapshot().unwrap();
    let (_, area_det) = snap
        .values
        .get(&area_id)
        .expect("snapshot must contain T.area");
    assert_eq!(
        *area_det,
        DeterminacyState::Determined,
        "T.area must be Determined"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// step-3: RED — cross-kind cycle: param a reads let b; let b reads param a
// ──────────────────────────────────────────────────────────────────────────────

/// A genuine cross-kind data-dependency cycle must emit a clear
/// circular-dependency Error diagnostic, NOT produce silent Undef with no
/// diagnostic.
///
///   param a : Real = b + 1.0   ← param reads let
///   let b = a * 2.0            ← let reads param → cycle!
///
/// After step-2 (unified topological pass), the topological sort detects the
/// cycle via the Kahn-drop mechanism (sorted.len() < nodes.len()) and must emit
/// a Diagnostic::error naming both cyclic members.
///
/// FAILS after step-2 if the cycle detector is still let-only (detect_let_cycle
/// kind == Let filter never sees param node `a`, so the a<->b cycle is not
/// reported).
#[test]
fn cross_kind_param_let_cycle_emits_circular_dependency_error() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure C { \
            param a : Real = b + 1.0 \
            let b = a * 2.0 \
        }",
    );

    // Must not panic and must not hang.
    let result = engine.eval(&module);

    // Must have at least one circular-dependency Error diagnostic.
    let cycle_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && (d.message.contains("circular") || d.message.contains("cycle"))
        })
        .collect();
    assert!(
        !cycle_errors.is_empty(),
        "cross-kind param<->let cycle must emit a circular-dependency Error diagnostic; \
         got diagnostics: {:?}",
        result.diagnostics
    );

    // The diagnostic must name the cyclic members.
    let diag_msg = &cycle_errors[0].message;
    assert!(
        diag_msg.contains("a") && diag_msg.contains("b"),
        "cycle diagnostic must name the cyclic members 'a' and 'b'; got: {diag_msg:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// step-5: RED — eval_cached cross-path parity lock
// ──────────────────────────────────────────────────────────────────────────────

/// eval_cached (the incremental path) must honour §8.2 order-independence the
/// same way eval() does after the fix.
///
/// Uses the same param->let repro from step-1. Evaluates via eval_cached and
/// asserts T.p / T.out agree with the eval() result (cross-path parity).
///
/// FAILS today because eval_cached carries the identical kind-partitioned
/// two-pass (its own first-pass Param/Auto loop + evaluate_let_bindings), so
/// the cached path still yields silent Undef for param p while eval() (post
/// step-2) returns the correct value.
#[test]
fn eval_cached_param_default_sibling_let_parity_with_eval() {
    use reify_core::identity::VersionId;

    let module = compile_source(
        "structure T { \
            param rope_dia : Length = 6mm \
            let drum_d = rope_dia * 2.0 \
            param feed : Length = 1300mm \
            param p : Real = feed / drum_d \
            let out = p \
        }",
    );

    let expected_p = 1.3_f64 / 0.012_f64; // ~108.33333333333333

    let p_id = ValueCellId::new("T", "p");
    let out_id = ValueCellId::new("T", "out");

    // eval() result (fresh path — must pass after step-2).
    let mut engine_eval = fresh_engine();
    let eval_result = engine_eval.eval(&module);

    let eval_p = match eval_result.values.get(&p_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("eval() T.p must be Scalar; got {:?}", other),
    };
    let eval_out = match eval_result.values.get(&out_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("eval() T.out must be Scalar; got {:?}", other),
    };

    // eval_cached() result (incremental path — must agree after step-6).
    let mut engine_cached = fresh_engine();
    let cached_result = engine_cached.eval_cached(&module, VersionId(1));

    let cached_p = match cached_result.eval_result.values.get(&p_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "eval_cached() T.p must be Scalar (cross-path parity); got {:?}. \
             Bug: eval_cached still uses kind-partitioned two-pass after step-2 fixed eval().",
            other
        ),
    };
    let cached_out = match cached_result.eval_result.values.get(&out_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "eval_cached() T.out must be Scalar (cross-path parity); got {:?}",
            other
        ),
    };

    // Cross-path parity: eval and eval_cached must agree.
    assert!(
        (eval_p - cached_p).abs() < 1e-9,
        "eval() and eval_cached() must agree on T.p: eval={eval_p}, cached={cached_p}"
    );
    assert!(
        (eval_out - cached_out).abs() < 1e-9,
        "eval() and eval_cached() must agree on T.out: eval={eval_out}, cached={cached_out}"
    );

    // Both must be within 1e-9 of the expected value.
    assert!(
        (cached_p - expected_p).abs() < 1e-9,
        "eval_cached() T.p must be ~108.333...; got {cached_p}"
    );
    assert!(
        (cached_out - expected_p).abs() < 1e-9,
        "eval_cached() T.out must be ~108.333...; got {cached_out}"
    );

    // DeterminacyState via eval_cached must also be Determined.
    // (eval_cached uses engine.snapshot() after the call)
    let snap = engine_cached.snapshot().unwrap();
    let (_, p_det) = snap
        .values
        .get(&p_id)
        .expect("eval_cached snapshot must contain T.p");
    assert_eq!(
        *p_det,
        DeterminacyState::Determined,
        "eval_cached() T.p must be Determined (cross-path parity); got {:?}",
        p_det
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Smoke: existing let-only cycle detector is NOT broken by the change
// ──────────────────────────────────────────────────────────────────────────────

/// Regression lock: a let-only circular dependency must still produce an error
/// diagnostic after the fix (the generalized cycle detector must not lose the
/// let-only case).
///
///   let a = b + 1.0
///   let b = a * 2.0   ← cycle among lets only
#[test]
fn let_only_cycle_still_emits_circular_dependency_error_after_fix() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure L { \
            let a = b + 1.0 \
            let b = a * 2.0 \
        }",
    );

    let result = engine.eval(&module);

    let cycle_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && (d.message.contains("circular") || d.message.contains("cycle"))
        })
        .collect();
    assert!(
        !cycle_errors.is_empty(),
        "let-only cycle must still emit a circular-dependency Error after the fix; \
         got: {:?}",
        result.diagnostics
    );
}
