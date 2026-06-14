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

    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors, got: {:?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 4,
        "expected at least 4 constraint results, got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?} — B6 constant_field signal",
            entry.id,
            entry.satisfaction
        );
    }
}
