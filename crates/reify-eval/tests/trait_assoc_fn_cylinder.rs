//! End-to-end eval gate for trait-INSTANCE associated-fn dispatch (task ζ 3941).
//!
//! This is the user-observable counterpart to the sibling static-dispatch gate
//! `trait_assoc_fn_static_e2e.rs`. It pins the PRD §1 motivating example through
//! the full compile + eval pipeline:
//!
//! `examples/trait_assoc_fn_cylinder.ri` declares a `Cylindrical` trait with a
//! default-providing INSTANCE fn `lateral_area(self) -> Scalar<Area>` whose body
//! is `pi * diameter * length` (bare member refs sugar for `self.diameter` /
//! `self.length`, PRD §4.4), a `Pin` conformer (diameter = 8mm, length = 40mm),
//! and an `Assembly` that consumes it via instance dispatch
//! `pin.(Cylindrical::lateral_area)()`.
//!
//! Two cases are exercised:
//!   1. DEFAULT DISPATCH (from the shipped example): `Assembly.wetted` evaluates
//!      to a `Value::Scalar` ≈ `pi * 0.008 * 0.040` m² (the wetted lateral area),
//!      proving the dispatch lowers to a `UserFunctionCall` of the registered
//!      per-conformer symbol and reuses the shipped `eval_user_function_call`
//!      path with no new evaluator entry point.
//!   2. UNDEF PROPAGATION (§9.2, inline source): a conformer whose `diameter` is
//!      `undef` makes `self.diameter` undef, so the body's arithmetic propagates
//!      to `Value::Undef` — a well-typed partial design with NO Error diagnostic.
//!
//! The structure-OVERRIDE conformer case (a conformer supplying its own
//! `fn lateral_area(self) { … }` body) is intentionally NOT exercised here: the
//! grammar does not yet admit `function_definition` inside a `structure def`
//! `_member`, so the override body is unparseable. That gap (grammar arm +
//! parser regen + override-beats-default e2e) is tracked as a dedicated
//! follow-up task; ζ's acceptance is default-dispatch + undef propagation.
//!
//! RED until step-8 authors `examples/trait_assoc_fn_cylinder.ri` (the
//! `include_str!` below fails to compile while the file is absent). Steps 2/4/6
//! (dispatch lowering, self.member desugar, per-conformer registration) have
//! already landed, so once the example exists this gate is GREEN.

#![allow(clippy::mutable_key_type)]

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// The shipped PRD §1 example (default-dispatch only — pedagogical, clean-compile).
fn example_source() -> &'static str {
    include_str!("../../../examples/trait_assoc_fn_cylinder.ri")
}

/// Inline §9.2 undef-propagation variant. Kept out of the shipped example so the
/// example stays a clean, single-purpose PRD §1 illustration; the undef case is a
/// behavioural corner pinned here rather than pedagogical example content.
///
/// `diameter` is bound to the `undef` literal (a fully-specified UserUndef
/// default — no missing-required-param error on `sub`), so the instance's
/// `diameter` field is undef and `pi * self.diameter * self.length` propagates to
/// `Value::Undef`.
const UNDEF_SRC: &str = r#"
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * length }
}

structure def HollowPin : Cylindrical {
    param diameter : Length = undef
    param length : Length = 40mm
}

structure def UndefAssembly {
    sub hp : HollowPin
    let wetted = hp.(Cylindrical::lateral_area)()
}
"#;

/// (step-7 default-dispatch) The shipped example must compile + eval clean and
/// the consuming `Assembly.wetted` cell must be a `Value::Scalar` ≈
/// `pi * 0.008 * 0.040` m² (8mm diameter × 40mm length lateral area).
///
/// The expected value is recomputed in-test from the same constants in the same
/// left-to-right order the body uses (`(pi * diameter) * length`), so the only
/// thing under test is that the dispatch + desugar + registration machinery
/// produced the right arithmetic — not the floating-point constant itself.
#[test]
fn trait_instance_fn_dispatch_end_to_end() {
    let compiled = parse_and_compile_with_stdlib(example_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // (a) The example must compile and eval with no Error diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the default-dispatch example, got: {errors:?}"
    );

    // (b) Assembly.wetted = pin.(Cylindrical::lateral_area)() → pi * 8mm * 40mm.
    let wetted_id = ValueCellId::new("Assembly", "wetted");
    match eval_result.values.get(&wetted_id) {
        Some(Value::Scalar { si_value, .. }) => {
            // Same operation order as the body: (pi * diameter) * length, in SI metres.
            let expected = std::f64::consts::PI * 0.008_f64 * 0.040_f64;
            let tol = expected.abs() * 1e-9;
            assert!(
                (si_value - expected).abs() < tol,
                "Assembly.wetted: expected {expected} m² (pi*8mm*40mm), got {si_value} m²"
            );
        }
        other => panic!(
            "Assembly.wetted should evaluate to a Value::Scalar (the lateral area); got {other:?}"
        ),
    }
}

/// (step-7 undef propagation, §9.2) A conformer whose `diameter` is `undef`
/// makes the body's `pi * self.diameter * self.length` evaluate to
/// `Value::Undef` — a well-typed partial design that must NOT raise any Error
/// diagnostic (the dispatch still type-checks; only the value is indeterminate).
#[test]
fn trait_instance_fn_undef_diameter_propagates_to_undef() {
    let compiled = parse_and_compile_with_stdlib(UNDEF_SRC);
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // No Error diagnostics: an undef field is a legitimate partial design (§9.2),
    // not a type error.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "an undef diameter is a well-typed partial design (§9.2) — expected no Error \
         diagnostics, got: {errors:?}"
    );

    // wetted propagates the undef diameter through the body arithmetic to Undef.
    // An undef-valued cell is surfaced as Value::Undef (or absent from `values`);
    // it must NOT be a concrete Scalar.
    let wetted_id = ValueCellId::new("UndefAssembly", "wetted");
    match eval_result.values.get(&wetted_id) {
        Some(Value::Undef) | None => { /* undef propagated through the dispatch (§9.2) */ }
        other => panic!(
            "UndefAssembly.wetted should propagate the undef diameter to Value::Undef \
             (not a concrete value); got {other:?}"
        ),
    }
}
