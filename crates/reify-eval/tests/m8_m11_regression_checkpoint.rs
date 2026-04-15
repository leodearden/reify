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
/// Step-2 replaces this stub with a comprehensive multi-structure source.
const SOURCE: &str = "";

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
                (f - 7.0).abs() < 1e-6,
                "expected 7.0 for FieldUser.f3 (sample(linear_f, 3.0) = 2*3+1 = 7.0), got {f}"
            );
        }
        other => panic!("expected Value::Real for FieldUser.f3, got {:?}", other),
    }
}

// ── Type / Value variant coverage (step-3 stub) ───────────────────────────────
//
// Step-3 adds tests that exercise every Type and Value variant programmatically.
// They are not yet present — step-3 will add them as failing stubs and step-4
// will implement the exhaustive coverage.

// ── Test-count floor checkpoint (step-5 stub) ────────────────────────────────
//
// Step-5 adds the #[ignore]-annotated test-count floor. Not yet present.
