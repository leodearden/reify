//! A user `fn` that wraps a stdlib `.ri` fn evaluates exactly as the direct
//! call does, in value and in constraint position (task #8393, σ S-6).
//!
//! The defect these pin was silent: the wrapper's body compiled against a fn
//! table holding no stdlib `.ri` fn, so its call lowered to a builtin call
//! eval cannot dispatch, and the wrapper evaluated to Undef with no compile
//! diagnostic. Each test therefore compares a wrapped value with its direct
//! twin and guards the twin against being Undef itself.
//!
//! Lives under `harness_engine/` for the anti-re-accretion reason recorded in
//! `dimensioned_ctor_migration_si_values.rs`.

use reify_core::ValueCellId;
use reify_eval::EvalResult;
use reify_ir::{Satisfaction, UndefCause, Value};
use reify_test_support::{cell_value, make_simple_engine, parse_and_compile_with_stdlib};

/// Three wrappers of stdlib `.ri` fns: a generic field combinator, a zero-arg
/// list constant, and an expression over one that `sqrt` can fail.
const WRAPPERS_SRC: &str = r#"
field def f_double : Real -> Real { source = analytical { |x| 2.0 * x } }
field def g_plus1 : Real -> Real { source = analytical { |x| x + 1.0 } }

fn wrap_min(a: Field<Real, Real>, b: Field<Real, Real>) -> Field<Real, Real> { pointwise_min(a, b) }
fn wrap_bolts() -> List<Length> { standard_bolt_lengths() }
fn scaled_first_bolt(k: Real) -> Length { standard_bolt_lengths()[0] * sqrt(k) }

structure def ValuePosition {
    let direct_min = sample(pointwise_min(f_double, g_plus1), 3.0)
    let wrapped_min = sample(wrap_min(f_double, g_plus1), 3.0)
    let direct_bolts = standard_bolt_lengths()
    let wrapped_bolts = wrap_bolts()
    let direct_scaled = standard_bolt_lengths()[0] * sqrt(4.0)
    let wrapped_scaled = scaled_first_bolt(4.0)
}

structure def ConstraintPosition {
    constraint sample(wrap_min(f_double, g_plus1), 0.5) == sample(pointwise_min(f_double, g_plus1), 0.5)
    constraint wrap_bolts()[6] == standard_bolt_lengths()[6]
}

structure def Provenance {
    let direct_scaled = standard_bolt_lengths()[0] * sqrt(4.0)
    let wrapped_scaled = scaled_first_bolt(4.0)
    let wrapped_failing = scaled_first_bolt(-1.0)
}
"#;

/// A same-module structure whose param default calls a stdlib `.ri` fn,
/// constructed both directly and from inside a fn body.
const HOLDER_SRC: &str = r#"
structure def Holder {
    param first : Length = standard_bolt_lengths()[0]
}

fn make_holder() -> Holder { Holder() }

structure def SkeletonRoute {
    let direct = Holder()
    let direct_first = direct.first
    let via_fn = make_holder()
    let via_fn_first = via_fn.first
}
"#;

/// A wrapper declared BEFORE the user fn that shadows the stdlib fn it calls.
const LATER_SHADOW_SRC: &str = r#"
fn wrap_sheets() -> List<Length> { standard_sheet_thicknesses() }
fn standard_sheet_thicknesses() -> List<Length> { [7mm] }

structure def LaterShadow {
    let via_wrapper = wrap_sheets()
    let top = standard_sheet_thicknesses()
}
"#;

/// LATER_SHADOW_SRC, but the shadow's return type differs from the stdlib fn's.
const LATER_RETYPED_SHADOW_SRC: &str = r#"
fn wrap_sheets() -> List<Length> { standard_sheet_thicknesses() }
fn standard_sheet_thicknesses() -> Length { 7mm }

structure def LaterRetypedShadow {
    let via_wrapper = wrap_sheets()
    let top = standard_sheet_thicknesses()
    let indexed = wrap_sheets()[0]
}
"#;

/// A trait-STATIC fn wrapping a stdlib `.ri` fn, called by its `Trait::method`
/// symbol.
const TRAIT_STATIC_SRC: &str = r#"
trait Stock {
    fn bolts() -> List<Length> { standard_bolt_lengths() }
}

structure def TraitRoute {
    let via_trait = Stock::bolts()
    let direct = standard_bolt_lengths()
}
"#;

/// Asserts `structure.wrapped` evaluates to the same Value as
/// `structure.direct`, after guarding that the direct call is not Undef.
#[track_caller]
fn assert_wrapped_equals_direct(result: &EvalResult, structure: &str, direct: &str, wrapped: &str) {
    let direct_value = cell_value(result, structure, direct);
    assert!(
        !direct_value.is_undef(),
        "premise: {structure}.{direct} (the direct stdlib call) must not be Undef"
    );
    assert_eq!(
        cell_value(result, structure, wrapped),
        direct_value,
        "{structure}.{wrapped} must equal {structure}.{direct}"
    );
}

#[test]
fn wrapped_stdlib_fns_equal_direct_calls_in_value_position() {
    let compiled = parse_and_compile_with_stdlib(WRAPPERS_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    for (direct, wrapped) in [
        ("direct_min", "wrapped_min"),
        ("direct_bolts", "wrapped_bolts"),
        ("direct_scaled", "wrapped_scaled"),
    ] {
        assert_wrapped_equals_direct(&result, "ValuePosition", direct, wrapped);
    }
    // min(f_double(3.0), g_plus1(3.0)) = min(6.0, 4.0), exact in f64.
    assert_eq!(
        cell_value(&result, "ValuePosition", "wrapped_min"),
        Value::Real(4.0)
    );
}

#[test]
fn wrapped_stdlib_fns_are_satisfied_in_constraint_position() {
    let compiled = parse_and_compile_with_stdlib(WRAPPERS_SRC);
    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    let check = engine.check(&compiled);

    let entries: Vec<_> = check
        .constraint_results
        .iter()
        .filter(|entry| entry.id.entity == "ConstraintPosition")
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "expected both ConstraintPosition constraints to be checked; got {:?}",
        check.constraint_results
    );
    for entry in entries {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} must be Satisfied; indeterminate reason: {:?}",
            entry.id,
            entry.indeterminate_reason
        );
    }
}

/// INV-SF-1: a wrapper that legitimately fails is Undef WITH a recorded cause,
/// and no Undef cell of the structure is left without one.
#[test]
fn remaining_undef_in_a_stdlib_wrapper_carries_provenance() {
    let compiled = parse_and_compile_with_stdlib(WRAPPERS_SRC);
    let mut engine = make_simple_engine();
    engine.set_capture_undef_causes(true);
    let result = engine.eval(&compiled);

    assert_wrapped_equals_direct(&result, "Provenance", "direct_scaled", "wrapped_scaled");

    let failing_id = ValueCellId::new("Provenance", "wrapped_failing");
    assert!(
        cell_value(&result, "Provenance", "wrapped_failing").is_undef(),
        "scaled_first_bolt(-1.0) takes sqrt of a negative and must be Undef"
    );
    let causes = engine.undef_causes();
    assert!(
        matches!(
            causes.get(&failing_id),
            Some(UndefCause::OpContractFailed { .. })
        ),
        "Provenance.wrapped_failing must carry an OpContractFailed cause; got {:?}",
        causes.get(&failing_id)
    );

    let causeless: Vec<&ValueCellId> = result
        .values
        .iter()
        .filter(|(id, value)| id.entity == "Provenance" && value.is_undef())
        .map(|(id, _)| id)
        .filter(|id| !causes.contains_key(*id))
        .collect();
    assert!(
        causeless.is_empty(),
        "every Undef cell of Provenance must carry an undef cause; causeless: {causeless:?}"
    );
}

#[test]
fn fn_body_ctor_of_same_module_structure_resolves_its_stdlib_default() {
    let compiled = parse_and_compile_with_stdlib(HOLDER_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    assert_wrapped_equals_direct(&result, "SkeletonRoute", "direct_first", "via_fn_first");
}

/// KNOWN LIMITATION, owned by #9363 (eval runs the overload the compiler
/// chose), which flips this test and the next: the earlier wrapper's call
/// binds the stdlib fn at compile time, but eval re-resolves it user-first, so
/// a stdlib name shadowed by a LATER-declared user fn dispatches to the user fn
/// at runtime, from inside the earlier wrapper too.
#[test]
fn later_declared_user_shadow_wins_at_runtime() {
    let compiled = parse_and_compile_with_stdlib(LATER_SHADOW_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    assert_wrapped_equals_direct(&result, "LaterShadow", "top", "via_wrapper");
}

/// The same #9363 limitation when the shadow's return type differs: the
/// `List<Length>`-typed wrapper evaluates to the shadow's bare Length, and
/// indexing it is Undef.
#[test]
fn later_declared_retyped_shadow_wins_at_runtime_despite_the_wrapper_type() {
    let compiled = parse_and_compile_with_stdlib(LATER_RETYPED_SHADOW_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let top = cell_value(&result, "LaterRetypedShadow", "top");
    assert!(
        !matches!(top, Value::List(_)),
        "premise: the user shadow returns a bare Length; got {top:?}"
    );
    assert_wrapped_equals_direct(&result, "LaterRetypedShadow", "top", "via_wrapper");
    assert!(
        cell_value(&result, "LaterRetypedShadow", "indexed").is_undef(),
        "indexing the wrapper's bare-Length value must be Undef"
    );
}

#[test]
fn trait_static_fn_wrapping_a_stdlib_fn_equals_the_direct_call() {
    let compiled = parse_and_compile_with_stdlib(TRAIT_STATIC_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    assert_wrapped_equals_direct(&result, "TraitRoute", "direct", "via_trait");
}
