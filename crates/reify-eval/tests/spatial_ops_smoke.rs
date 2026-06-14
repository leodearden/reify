//! End-to-end smoke tests for the std.fields spatial-op constructors (task 4223 ε).
//!
//! Tests are RED before their corresponding impl steps land:
//!   step-1 / step-2: constant_field_samples_constant_at_any_point (B6)
//!   step-3 / step-4: clamp_field_clamps_over_range_to_bound (B7)
//!   step-3 / step-4: remap_field_linearly_remaps
//!   step-5 / step-6: threshold_returns_bool_field_sampling_true_and_false (B8)
//!
//! Pattern: inline source → parse_and_compile_with_stdlib → Engine::eval +
//! Engine::check → all constraints Satisfaction::Satisfied.

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compile an inline source string with stdlib, asserting no error diagnostics.
fn compile_inline(source: &str) -> reify_compiler::CompiledModule {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors, got: {:?}",
        errors_only(&compiled)
    );
    compiled
}

/// Eval + check the compiled module, asserting all constraint results are Satisfied.
fn assert_all_satisfied(compiled: &reify_compiler::CompiledModule, min_constraints: usize) {
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let check = engine.check(compiled);
    assert!(
        check.constraint_results.len() >= min_constraints,
        "expected at least {} constraint results, got {}",
        min_constraints,
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── B6: constant_field ────────────────────────────────────────────────────────

/// B6: `constant_field(v)` sampled at any domain point returns `v`.
///
/// Inline source samples `constant_field(42.0)` at two distinct points
/// (0.0 and 99.0); both must equal 42.0.
///
/// RED until step-2 adds `pub fn constant_field<D, C>` to stdlib/fields.ri:
/// without the fn body the call falls through to reify_stdlib::eval_builtin →
/// Value::Undef → sample(Undef, ..) → Undef → constraints Indeterminate.
#[test]
fn constant_field_samples_constant_at_any_point() {
    let source = r#"
structure def ConstantFieldDemo {
    let c0 = sample(constant_field(42.0), 0.0)
    let c1 = sample(constant_field(42.0), 99.0)
    constraint c0 > 41.999
    constraint c0 < 42.001
    constraint c1 > 41.999
    constraint c1 < 42.001
}
"#;
    assert_all_satisfied(&compile_inline(source), 4);
}

// ── B7: clamp_field and remap_field ──────────────────────────────────────────

/// B7: `clamp_field(f, lo, hi)` clamps over-range scalar inputs to the bound.
///
/// Cases:
///   Real domain: `clamp_field(constant_field(250.0), 10.0, 200.0)` → 200.0
///   Pressure (MPa): `clamp_field(constant_field(250MPa), 10MPa, 200MPa)` → 200MPa
///     — exercises Q=Pressure binding, the generic-over-Q value (task G dependency).
///
/// RED until step-4 adds `pub fn clamp_field<D, Q: Dimension>` to fields.ri:
/// without the fn body calls fall through → Value::Undef → constraints Indeterminate.
#[test]
fn clamp_field_clamps_over_range_to_bound() {
    // Real domain: 250.0 clamped to [10.0, 200.0] → 200.0
    let source_real = r#"
structure def ClampFieldRealDemo {
    let clamped = sample(clamp_field(constant_field(250.0), 10.0, 200.0), 0.0)
    constraint clamped > 199.999
    constraint clamped < 200.001
}
"#;
    assert_all_satisfied(&compile_inline(source_real), 2);

    // Pressure domain: 250MPa clamped to [10MPa, 200MPa] → 200MPa
    // Q binds to PRESSURE; constraint windows dwarf floating-point epsilon at MPa scale.
    let source_pressure = r#"
structure def ClampFieldPressureDemo {
    let clamped = sample(clamp_field(constant_field(250MPa), 10MPa, 200MPa), 0.0)
    constraint clamped > 199.99MPa
    constraint clamped < 200.01MPa
}
"#;
    assert_all_satisfied(&compile_inline(source_pressure), 2);
}

/// `remap_field(f, from_lo, from_hi, to_lo, to_hi)` linearly remaps the codomain.
///
/// `remap_field(constant_field(50.0), 0.0, 100.0, 0.0, 200.0)` at any point:
///   remap(50, [0,100], [0,200]) = 50/100 * 200 = 100.0
///
/// RED until step-4 adds `pub fn remap_field<D, Q: Dimension>` to fields.ri.
#[test]
fn remap_field_linearly_remaps() {
    let source = r#"
structure def RemapFieldDemo {
    let remapped = sample(remap_field(constant_field(50.0), 0.0, 100.0, 0.0, 200.0), 0.0)
    constraint remapped > 99.999
    constraint remapped < 100.001
}
"#;
    assert_all_satisfied(&compile_inline(source), 2);
}

// ── B8: threshold ─────────────────────────────────────────────────────────────

/// B8/D7: `threshold(f, value)` produces a `Field<D, Bool>`.
///
/// Above-threshold sample (250MPa > 200MPa) → true → `constraint above` Satisfied.
/// Below-threshold sample (150MPa < 200MPa) → false → `constraint !below` Satisfied.
/// Exercises Q=Pressure binding (generic-over-Q value; reason task blocks on G).
///
/// RED until step-6 adds `pub fn threshold<D, Q: Dimension>` to fields.ri:
/// without the fn body the call falls through → Value::Undef → constraints Indeterminate.
#[test]
fn threshold_returns_bool_field_sampling_true_and_false() {
    let source = r#"
structure def ThresholdDemo {
    let above = sample(threshold(constant_field(250MPa), 200MPa), 0.0)
    let below = sample(threshold(constant_field(150MPa), 200MPa), 0.0)
    constraint above
    constraint !below
}
"#;
    // Note: in the RED state (no threshold fn body), the compile may emit a
    // type-mismatch Warning ("constraint expression has type Scalar, expected Bool")
    // because the compiler sees the threshold call returning a Scalar codomain
    // inferred from the arguments.  This is Severity::Warning, not Error, so
    // compile_inline's errors_only check passes.  The constraints are
    // Indeterminate (undef inputs) → assert_all_satisfied fails → RED.
    assert_all_satisfied(&compile_inline(source), 2);
}
