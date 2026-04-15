//! M8–M11 regression checkpoint tests.
//!
//! **Purpose**: A durable regression guard ensuring all pre-existing tests still pass
//! after M8–M11 changes and providing compile-time exhaustiveness coverage for the
//! `Type` (27 variants) and `Value` (25 variants) enums.
//!
//! **Coverage**:
//!   - Cross-milestone integration: parse → compile → eval → check pipeline using a
//!     Reify source that exercises one feature from each milestone.
//!   - M8: stdlib SI units (`mm`, `N`, `kg`) in structure parameters.
//!   - M9: trait conformance (`structure def Foo : Trait`), constraint definitions.
//!   - M10: geometric builtins (`point3`, `vec3`, `orient_identity`, `transform3`, `frame3`).
//!   - M11: field calculus (`field def`, `sample`, `gradient`), `@test` annotation.
//!   - Compile-time exhaustive `match` guards for all 27 `Type` variants.
//!   - Compile-time exhaustive `match` guards for all 25 `Value` variants, with
//!     runtime calls to `Display`, `content_hash`, `try_infer_type`, `format_hover`.
//!
//! **Design notes**:
//!   - All integration tests share a single compiled module via `OnceLock` to avoid
//!     re-parsing the source on every test invocation.
//!   - The `test_count_floor` test is `#[ignore]` (slow subprocess); use
//!     `cargo test -- --include-ignored` to run it explicitly.
//!
//! Follows the pattern established by `m9_combined.rs`, `m10_combined.rs`, and
//! `m11_full_integration.rs`.

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{ModulePath, Satisfaction, Value, ValueCellId};

// ── Cross-milestone inline source ─────────────────────────────────────────────
//
// Step-2 fills this in with a comprehensive Reify source that exercises one
// feature from each of the four milestones M8–M11. The empty string is the
// step-1 stub; all integration tests below will fail until SOURCE is replaced.

/// Inline Reify source exercising M8–M11 cross-milestone features.
///
/// Milestone coverage:
///   M8 — stdlib SI units (`mm`, `N`) used in structure params and lets.
///   M9 — `trait def Measured`, `constraint def InRange`, structure conformance.
///   M10 — `point3`, `vec3`, `orient_identity`, `transform3`, `frame3` builtins.
///   M11 — `field def`, `sample`, `gradient` operators; `@test` annotation.
const SOURCE: &str = r#"
// M9: trait with inline length constraint
trait Measured {
    param length : Length
    constraint length > 0mm
}

// M9: reusable constraint definition
constraint def InRange {
    param val : Length
    param lo  : Length
    param hi  : Length
    val >= lo
    val <= hi
}

// M8 + M9: stdlib Length units, trait conformance, constraint def invocation
structure def SimpleBox : Measured {
    param length : Length = 100mm
    param width  : Length = 50mm
    param depth  : Length = 25mm
    let half_length = length / 2.0
    constraint InRange(val: depth, lo: 10mm, hi: 30mm)
    constraint width < length
}

// M10: geometric builtins from the stdlib prelude
structure def GeomPart {
    let origin = point3(0mm, 0mm, 0mm)
    let tip    = point3(100mm, 0mm, 0mm)
    let disp   = tip - origin
    let fvec   = vec3(0N, 0N, 1N)
    let rot    = orient_identity()
    let tf     = transform3(rot, vec3(10mm, 0mm, 0mm))
    let fr     = frame3(origin, rot)
    let moved  = tf * origin
}

// M11: field calculus — field def, sample, gradient operators
// Use non-integer values to ensure Real (not Int) evaluation results.
field def linear_f : Real -> Real { source = analytical { |x| 2.5 * x + 1.0 } }

structure def FieldUser {
    let f3  = sample(linear_f, 3.0)    // 2.5 * 3.0 + 1.0 = 8.5
    let gf  = gradient(linear_f)
    let df3 = sample(gf, 3.0)          // ≈ 2.5 (constant derivative)
    constraint f3  > 8.4
    constraint f3  < 8.6
    constraint df3 > 2.4
    constraint df3 < 2.6
}

// M11: @test annotation on a structure (valid on structures, no error diagnostic)
@test structure def TestBox {
    param width : Length = 50mm
    constraint width > 0mm
}
"#;

// ── Cached helpers ────────────────────────────────────────────────────────────

/// Parse + compile SOURCE with the stdlib prelude. Cached for the test process.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(SOURCE))
}

/// Evaluate the compiled module with a fresh engine. Not cached (each test
/// gets an independent engine state).
fn eval_checkpoint() -> reify_eval::EvalResult {
    let mut engine = make_simple_engine();
    engine.eval(compiled())
}

/// Check constraints in the compiled module with a fresh engine.
fn check_checkpoint() -> reify_eval::CheckResult {
    let mut engine = make_simple_engine();
    engine.check(compiled())
}

// ── Integration tests ─────────────────────────────────────────────────────────

/// Verify the cross-milestone source parses with zero errors and produces at
/// least 6 top-level declarations (trait def, constraint def, 3+ structures,
/// field def, @test structure).
///
/// **Fails in step-1** because SOURCE = "" → 0 declarations < 6.
#[test]
fn checkpoint_parses() {
    let parsed = reify_syntax::parse(SOURCE, ModulePath::single("m8_m11_checkpoint"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    assert!(
        parsed.declarations.len() >= 6,
        "expected >= 6 top-level declarations from cross-milestone source, got {}; \
         step-2 must replace SOURCE with the comprehensive cross-milestone Reify source",
        parsed.declarations.len()
    );
}

/// Verify zero error-severity compile diagnostics and at least 4 compiled templates.
///
/// **Fails in step-1** because SOURCE = "" → templates.len() = 0 < 4.
#[test]
fn checkpoint_compiles_no_errors() {
    let m = compiled();
    let errors = collect_errors(&m.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    assert!(
        m.templates.len() >= 4,
        "expected >= 4 compiled structure templates, got {}; \
         step-2 must replace SOURCE with the comprehensive cross-milestone Reify source",
        m.templates.len()
    );
}

/// Verify zero error-severity eval diagnostics and a non-empty evaluated-values map.
///
/// **Fails in step-1** because SOURCE = "" → values map is empty.
#[test]
fn checkpoint_evals_no_errors() {
    let result = eval_checkpoint();
    let errors = collect_errors(&result.diagnostics);
    assert!(errors.is_empty(), "eval errors: {:?}", errors);
    assert!(
        !result.values.is_empty(),
        "expected non-empty evaluated-values map; \
         step-2 must replace SOURCE with the comprehensive cross-milestone Reify source"
    );
}

/// Spot-check M8+M9: `SimpleBox.half_length` should be 100mm / 2.0 = 0.05 SI.
///
/// **Fails in step-1** because SOURCE = "" → SimpleBox.half_length not found.
#[test]
fn checkpoint_m8_unit_half_length() {
    let result = eval_checkpoint();
    let id = ValueCellId::new("SimpleBox", "half_length");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "SimpleBox.half_length not found in eval values — \
             step-2 must replace SOURCE with the comprehensive cross-milestone source"
        )
    });
    match val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-9,
                "expected 0.05 SI for SimpleBox.half_length (100mm / 2.0 = 50mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for SimpleBox.half_length, got {:?}", other),
    }
}

/// Spot-check M9: all constraint results should be Satisfied with at least 6
/// constraint entries (trait constraint, InRange predicates, inline constraints,
/// FieldUser interval constraints).
///
/// **Fails in step-1** because SOURCE = "" → 0 constraint results < 6.
#[test]
fn checkpoint_m9_constraints_satisfied() {
    let result = check_checkpoint();
    assert!(
        result.constraint_results.len() >= 6,
        "expected >= 6 constraint results, got {}; \
         step-2 must replace SOURCE with the comprehensive cross-milestone source",
        result.constraint_results.len()
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

/// Spot-check M10: `GeomPart.origin` should evaluate to a `Value::Point`.
///
/// **Fails in step-1** because SOURCE = "" → GeomPart.origin not found.
#[test]
fn checkpoint_m10_geometric_types_eval() {
    let result = eval_checkpoint();
    let id = ValueCellId::new("GeomPart", "origin");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "GeomPart.origin not found in eval values — \
             step-2 must replace SOURCE with the comprehensive cross-milestone source"
        )
    });
    assert!(
        matches!(val, Value::Point(_)),
        "expected Value::Point for GeomPart.origin, got {:?}",
        val
    );
}

/// Spot-check M11 field calculus: `FieldUser.f3 = sample(linear_f, 3.0)` where
/// `linear_f(x) = 2x + 1` → expected value is 7.0.
///
/// **Fails in step-1** because SOURCE = "" → FieldUser.f3 not found.
#[test]
fn checkpoint_m11_field_sample_at_three() {
    let result = eval_checkpoint();
    let id = ValueCellId::new("FieldUser", "f3");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "FieldUser.f3 not found in eval values — \
             step-2 must replace SOURCE with the comprehensive cross-milestone source"
        )
    });
    match val {
        Value::Real(f) => {
            assert!(
                (f - 8.5).abs() < 1e-6,
                "expected 8.5 for FieldUser.f3 (sample(linear_f, 3.0) = 2.5*3.0+1.0), got {f}"
            );
        }
        other => panic!("expected Value::Real for FieldUser.f3, got {:?}", other),
    }
}

// ── Type / Value variant coverage ────────────────────────────────────────────

/// Compile-time exhaustiveness guard for all 27 `Type` variants.
///
/// This function is NEVER called at runtime. Its only purpose is to force a
/// compile error if a new `Type` variant is added without updating this list.
/// If any variant is removed from the `match`, the compiler will warn about
/// unreachable patterns. If a variant is ADDED to the enum without being listed
/// here, the match becomes non-exhaustive and the test file won't compile.
///
/// **Fails (won't compile) in step-3** because it is annotated with
/// `#[allow(dead_code)]` — the live runtime call that drives exhaustiveness is
/// added in step-4.
#[allow(dead_code)]
fn assert_all_type_variants_listed(t: &reify_types::Type) {
    use reify_types::Type;
    let _ = match t {
        // Primitive scalars
        Type::Bool | Type::Int | Type::Real | Type::String => true,
        // Dimensioned scalar
        Type::Scalar { .. } => true,
        // Named enum
        Type::Enum(_) => true,
        // Collection types
        Type::List(_) | Type::Set(_) | Type::Map(..) | Type::Option(_) => true,
        // Callable / generic
        Type::Function { .. } | Type::TypeParam(_) | Type::StructureRef(_) => true,
        // Field mapping
        Type::Field { .. } => true,
        // Geometry handle
        Type::Geometry => true,
        // Geometric vector spaces
        Type::Point { .. } | Type::Vector { .. } | Type::Tensor { .. } => true,
        // Complex and range
        Type::Complex(_) | Type::Range(_) => true,
        // Rigid-body / orientation
        Type::Orientation(_) | Type::Frame(_) | Type::Transform(_) => true,
        // 3D geometric primitives
        Type::Plane | Type::Axis | Type::BoundingBox => true,
        // Matrix
        Type::Matrix { .. } => true,
    };
}

/// Compile-time exhaustiveness guard for all 25 `Value` variants.
///
/// Same design as `assert_all_type_variants_listed`: never called at runtime
/// in step-3, causes compile error if new variants aren't listed here.
///
/// **Fails (won't compile) in step-3** because it references an import
/// (`reify_types::FieldSourceKind`) that hasn't been added to the `use`
/// declaration yet — step-4 adds the necessary imports and makes this work.
#[allow(dead_code)]
fn assert_all_value_variants_listed(v: &reify_types::Value) {
    use reify_types::Value;
    let _ = match v {
        // Primitive scalars
        Value::Bool(_) | Value::Int(_) | Value::Real(_) | Value::String(_) => true,
        // Dimensioned scalar
        Value::Scalar { .. } => true,
        // Named enum
        Value::Enum { .. } => true,
        // Collection types
        Value::List(_) | Value::Set(_) | Value::Map(_) | Value::Option(_) => true,
        // Field and lambda (callable values)
        Value::Field { .. } | Value::Lambda { .. } => true,
        // Tensor / numeric arrays
        Value::Tensor(_) => true,
        // Geometric vectors / points
        Value::Point(_) | Value::Vector(_) => true,
        // Complex number
        Value::Complex { .. } => true,
        // Rigid-body / orientation
        Value::Orientation { .. } | Value::Frame { .. } | Value::Transform { .. } => true,
        // 3D geometric primitives
        Value::Plane { .. } | Value::Axis { .. } | Value::BoundingBox { .. } => true,
        // Range
        Value::Range { .. } => true,
        // Matrix
        Value::Matrix(_) => true,
        // Undefined
        Value::Undef => true,
    };
}

/// Verify that `assert_all_type_variants_listed` covers the expected variant
/// count and that all 27 variants are exercised at the call site (step-4).
///
/// **Fails in step-3** because no Type instances are constructed yet; the
/// `todo!()` will panic when run.
#[test]
fn checkpoint_type_variant_coverage() {
    todo!("step-4: construct all 27 Type variants and exercise the exhaustiveness guard")
}

/// Verify that all 25 `Value` variants can be constructed without panics in
/// `Display`, `content_hash`, `try_infer_type`, and `format_hover`.
///
/// **Fails in step-3** because no Value instances are constructed yet; the
/// `todo!()` will panic when run.
#[test]
fn checkpoint_value_variant_coverage() {
    todo!("step-4: construct all 25 Value variants and call Display/content_hash/try_infer_type/format_hover")
}

// ── Test-count floor checkpoint (step-5 stub) ────────────────────────────────
//
// Step-5 adds the #[ignore]-annotated test-count floor. Not yet present.
