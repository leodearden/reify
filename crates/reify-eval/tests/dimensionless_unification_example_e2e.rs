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
use reify_core::{DimensionVector, Severity, Type, ValueCellId};
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

// ── Slice 2: Vector3 type-identity + Invariant-V leak-guard (steps 3→4) ─────

/// Recursive helper: returns `true` if `v` is (or contains) a
/// `Value::Scalar { dimension }` where `dimension.is_dimensionless()`.
///
/// Recurses into: Vector, Point, Tensor, List, Set, Map (keys+values),
/// Option, and Matrix.  All other variants return `false`.
fn has_dimensionless_scalar(v: &Value) -> bool {
    match v {
        Value::Scalar { dimension, .. } => dimension.is_dimensionless(),
        Value::Vector(comps)
        | Value::Point(comps)
        | Value::Tensor(comps)
        | Value::List(comps) => comps.iter().any(has_dimensionless_scalar),
        Value::Set(set) => set.iter().any(has_dimensionless_scalar),
        Value::Map(map) => map
            .iter()
            .any(|(k, v)| has_dimensionless_scalar(k) || has_dimensionless_scalar(v)),
        Value::Option(opt) => opt
            .as_ref()
            .map(|inner| has_dimensionless_scalar(inner))
            .unwrap_or(false),
        Value::Matrix(rows) => rows
            .iter()
            .any(|row| row.iter().any(has_dimensionless_scalar)),
        _ => false,
    }
}

/// Signal 2 (type layer):
/// `dir_sum = dir_real + dir_dimensionless` where `dir_real: Vector3<Real>` and
/// `dir_dimensionless: Vector3<Dimensionless>`.  This vector-add type-checks ONLY
/// because Vector3<Real> and Vector3<Dimensionless> resolve to the identical
/// Type::Vector{3, Scalar{DIMENSIONLESS}} (γ/4375).  The compile-clean gate in
/// eval_example() is the proof that they are ONE type; this test confirms the
/// evaluated result is `Value::Vector([Real(2), Real(0), Real(0)])` and that all
/// components are Value::Real (Invariant V: normalize(dir_real) produces
/// Value::Real components, so dir_sum should too).
///
/// RED: `dir_sum` binding is absent from the step-2 example → `get(&r, "dir_sum")` panics.
#[test]
fn vector3_real_dimensionless_interop_is_one_type() {
    let r = eval_example();
    match get(&r, "dir_sum") {
        Value::Vector(comps) => {
            assert_eq!(
                comps.len(),
                3,
                "dir_sum must be a 3-component vector, got {} components",
                comps.len()
            );
            let expected = [2.0_f64, 0.0_f64, 0.0_f64];
            for (i, (c, e)) in comps.iter().zip(expected.iter()).enumerate() {
                match c {
                    Value::Real(v) => assert!(
                        (v - e).abs() < TOL,
                        "dir_sum[{i}] expected {e}, got {v} (delta = {})",
                        (v - e).abs()
                    ),
                    other => panic!(
                        "dir_sum[{i}] must be Value::Real (Invariant V), got {:?}",
                        other
                    ),
                }
            }
        }
        other => panic!(
            "dir_sum must be Value::Vector([Real(2.0), Real(0.0), Real(0.0)]), got {:?}",
            other
        ),
    }
}

/// Invariant V (full pipeline, end-to-end leak-guard):
/// No value in the eval result — including all container elements — is
/// `Value::Scalar { dimension }` where `dimension.is_dimensionless()`.
///
/// Complements β/4374's unit-level leak-guard
/// (point_vector_eval_tests::scalar_div_scalar_dimensionless_returns_real) with an
/// end-to-end scan over all cells produced by a committed example.
#[test]
fn no_value_is_dimensionless_scalar_leak_guard() {
    let r = eval_example();
    for (id, v) in r.values.iter() {
        assert!(
            !has_dimensionless_scalar(v),
            "Invariant V leak: cell {:?} holds a Value::Scalar{{dimensionless}}: {:?}",
            id,
            v
        );
    }
}

/// Invariant T (canonical-form anchor):
/// `Type::dimensionless_scalar()` must equal `Type::Scalar { dimension: DIMENSIONLESS }`.
///
/// Module-doc note: Invariant T (Type::Real not constructible) is enforced
/// STRUCTURALLY by rustc — α/4373 deleted the `Type::Real` variant, so this test
/// file cannot even name it.  The faithful non-meta expression of Invariant T is:
///   (1) only `Type::dimensionless_scalar()` / `Type::Scalar{DIMENSIONLESS}` appear
///       as the canonical dimensionless type in this file;
///   (2) the Real/Dimensionless interop (turns_per_unit:Real mixed with ratio_b;
///       Vector3<Real> ≡ Vector3<Dimensionless>) compiles + evaluates clean;
///   (3) this explicit value-equality anchor below.
/// NO source-grep / introspection meta-test.
#[test]
fn invariant_t_canonical_dimensionless_type_is_scalar_not_real() {
    assert_eq!(
        Type::dimensionless_scalar(),
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS
        },
        "Invariant T: Type::dimensionless_scalar() must equal \
         Type::Scalar{{dimension: DIMENSIONLESS}}"
    );
}
