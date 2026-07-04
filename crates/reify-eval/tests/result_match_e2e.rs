//! Eval end-to-end pins for `match` over the PRELUDE `Result<T, E>` — task β
//! #4036 (PRD docs/prds/v0_6/result-and-fallback.md).
//!
//! Exercises the full generic data-carrying-enum eval pipeline — type-arg
//! inference (γ #4031), typed pattern binders (δ #4032), generic eval
//! (ε #4033), and DCE payload-binding eval (ζ #3946) — through the PRELUDE
//! `Result` (`stdlib/result.ri`) specifically, with NO inline `enum Result`
//! declared anywhere. Mirrors `generic_enum_erasure_e2e.rs` (#4033)'s
//! bore_ok/bore_err shape, but compiles via
//! `reify_test_support::parse_and_compile_with_stdlib` so the bare `Ok`/`Err`
//! constructors resolve against the prelude.
//!
//! Tests:
//!   1. bore_is_12mm_when_r_defaults_to_ok (PRIMARY §1 signal) — `Ok { value:
//!      12mm }` default → Widget.bore = 0.012 m; Widget.r = Result::Ok.
//!   2. bore_is_6mm_when_r_defaults_to_err — `Err { error: "bad" }` default
//!      switch → Widget.bore = 0.006 m; Widget.r = Result::Err.
//!
//! Both are formatter-agnostic numeric pins (`|si_value - x| < 1e-12` +
//! `DimensionVector::LENGTH`) of the same 12mm/6mm switch the CLI leaf
//! (`cli_eval_result_match.rs`) pins at the rendered-string level.

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile_with_stdlib;

// ── helper ───────────────────────────────────────────────────────────────────

fn eval(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ── test 1: PRIMARY §1 signal — Ok default ──────────────────────────────────

/// `param r : Result<Length, String> = Ok { value: 12mm }` and
/// `let bore : Length = match r { Ok { value: v } => v, Err { error: msg } => 6mm }`
/// → Widget.bore ≈ 0.012 m (the Ok arm's `v` binder, unwrapped); Widget.r
/// reports the Ok tag.
#[test]
fn bore_is_12mm_when_r_defaults_to_ok() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String> = Ok { value: 12mm }
    let bore : Length = match r {
        Ok { value: v } => v,
        Err { error: msg } => 6mm,
    }
}
"#;
    let result = eval(source);

    let bore_id = ValueCellId::new("Widget", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Widget.bore not found in eval result"));
    match bore_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.012).abs() < 1e-12,
                "expected Widget.bore ≈ 0.012 m (12mm, Ok-arm default), got {si_value} m"
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "expected Widget.bore to carry LENGTH dimension, got {dimension:?}"
            );
        }
        other => panic!("expected Value::Scalar for Widget.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Widget", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Widget.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(variant, "Ok", "Widget.r should be Result::Ok (default variant)");
        }
        other => panic!("expected Value::Enum for Widget.r, got {:?}", other),
    }
}

// ── test 2: Err-switch — the §1 default-switch signal ───────────────────────

/// Same shape as test 1 but `param r` defaults to `Err { error: "bad" }` →
/// Widget.bore ≈ 0.006 m (the Err arm's literal fallback); Widget.r reports
/// the Err tag.
#[test]
fn bore_is_6mm_when_r_defaults_to_err() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String> = Err { error: "bad" }
    let bore : Length = match r {
        Ok { value: v } => v,
        Err { error: msg } => 6mm,
    }
}
"#;
    let result = eval(source);

    let bore_id = ValueCellId::new("Widget", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Widget.bore not found in eval result"));
    match bore_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.006).abs() < 1e-12,
                "expected Widget.bore ≈ 0.006 m (Err-arm fallback 6mm), got {si_value} m"
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "expected Widget.bore to carry LENGTH dimension, got {dimension:?}"
            );
        }
        other => panic!("expected Value::Scalar for Widget.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Widget", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Widget.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(variant, "Err", "Widget.r should switch to Result::Err");
        }
        other => panic!("expected Value::Enum for Widget.r, got {:?}", other),
    }
}
