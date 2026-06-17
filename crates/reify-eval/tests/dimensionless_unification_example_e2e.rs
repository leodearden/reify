//! Integration gate for the real-dimensionless unification PRD
//! (docs/prds/v0_6/real-dimensionless-unification.md) — task η/#4377.
//!
//! Exercises the unified surface through the committed example
//! `examples/dimensionless_unification.ri` and asserts the four §Contract
//! signals across two RED→GREEN slices:
//!
//!   Slice 1 (steps 1→2): value-layer arithmetic
//!     - groove_len (L×(Real+Real) → Scalar{LENGTH}) within 1e-12 of 0.009 m
//!     - ratio_b (6mm/4mm) is Value::Real(1.5), NOT Value::Scalar{dimensionless}
//!
//!   Slice 2 (steps 3→4): Vector3 type-identity + Invariant-V leak-guard
//!     - Vector3<Real> + Vector3<Dimensionless> type-checks and evaluates
//!       (behavioral proof that the two are ONE type)
//!     - No value in the eval result is Value::Scalar{dimensionless}
//!     - Explicit Invariant-T canonical-form anchor for Type::dimensionless_scalar()
//!
//! Model: crates/reify-eval/tests/compose_example_smoke.rs (path const +
//! parse_and_compile_with_stdlib + Engine::eval pattern) and
//! crates/reify-eval/tests/dimensionless_real_arithmetic_e2e.rs (Value match
//! + ValueCellId extraction pattern).

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Compile-time path to the example fixture, resolved from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dimensionless_unification.ri"
);

/// Absolute tolerance for SI-value comparisons.
/// 2mm × 4.5 = 0.009000000000000001 (1 ULP above 0.009); actual error ~1.7e-18.
/// 1e-12 is ~6 orders of magnitude above the actual error — safely achievable
/// by pure IEEE-754 arithmetic (no solver). Design decision: NEVER use
/// bit-exact assert_eq!(si_value, 0.009) — that is permanently RED.
const TOL: f64 = 1e-12;

/// Structure name in the example fixture.
const STRUCT: &str = "DimensionlessUnification";

/// Read + compile + eval `examples/dimensionless_unification.ri` with the
/// stdlib, asserting zero Error diagnostics at both compile and eval layers.
fn eval_example() -> reify_eval::EvalResult {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/dimensionless_unification.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors in dimensionless_unification.ri, got: {:?}",
        errors_only(&compiled)
    );

    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected no eval-level errors in dimensionless_unification.ri, got: {:?}",
        eval_errors
    );

    result
}

/// Extract a binding value from the eval result, panicking if absent.
fn get(result: &reify_eval::EvalResult, binding: &str) -> Value {
    result
        .values
        .get(&ValueCellId::new(STRUCT, binding))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "binding '{}' not found in eval result for structure '{}'",
                binding, STRUCT
            )
        })
}

// ── Slice 1: value-layer arithmetic (steps 1→2) ─────────────────────────────

/// Signal 1 (value layer, root-bug shape):
/// `groove_len = lead * (turns_per_unit + ratio_b)` = 2mm × (3.0 + 1.5) = 9mm.
///
/// MUST be `Value::Scalar { dimension: LENGTH }` with si_value within TOL of
/// 0.009 m.  A bit-exact `assert_eq!(si_value, 0.009)` is permanently RED
/// because 2mm (SI = 0.002, non-dyadic) × 4.5 = 0.009000000000000001 (1 ULP
/// above the f64 nearest to literal 0.009).  See design decision in plan.json.
#[test]
fn groove_len_evaluates_to_9mm_within_tolerance() {
    let r = eval_example();
    match get(&r, "groove_len") {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH,
                "groove_len must have dimension LENGTH"
            );
            assert!(
                (si_value - 0.009).abs() < TOL,
                "groove_len si_value expected ~0.009 m (9mm), got {si_value} \
                 (delta = {}; TOL = {TOL})",
                (si_value - 0.009).abs()
            );
        }
        other => panic!(
            "groove_len must be Value::Scalar{{LENGTH}}, got {:?}",
            other
        ),
    }
}

/// Invariant V (producer side):
/// `ratio_b = hole_dia / pitch` = 6mm / 4mm = 1.5 must be `Value::Real(1.5)`,
/// NEVER `Value::Scalar{{ dimension: DIMENSIONLESS }}`.
///
/// β/4374 routes eval_div through from_real_scalar so all L/L divisions collapse
/// to Value::Real; this test gates that invariant end-to-end through a committed
/// example (complements point_vector_eval_tests::scalar_div_scalar_dimensionless_returns_real).
#[test]
fn ratio_b_is_real_not_dimensionless_scalar() {
    let r = eval_example();
    match get(&r, "ratio_b") {
        Value::Real(v) => {
            assert!(
                (v - 1.5).abs() < TOL,
                "ratio_b (6mm/4mm) expected Value::Real(1.5), got Value::Real({v})"
            );
        }
        other => panic!(
            "ratio_b (6mm/4mm) must be Value::Real(1.5), got {:?} — \
             Invariant V: L/L division must collapse to Value::Real, never Value::Scalar{{dimensionless}}",
            other
        ),
    }
}
