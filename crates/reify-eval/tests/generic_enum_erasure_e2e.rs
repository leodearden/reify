//! Generic-enum end-to-end integration gate (task ε #4033, step-1).
//!
//! Confirms the full generic data-carrying-enum eval pipeline — construction
//! type-arg inference (γ #4031), type-preserving pattern binders (δ #4032),
//! and DCE payload-binding eval (ζ #3946) — is wired end-to-end, and that eval
//! is UNCHANGED under F-Mono type erasure (D1/INV-2, PRD
//! docs/prds/v0_6/generic-data-carrying-enums.md §8).
//!
//! Mirrors crates/reify-eval/tests/m6_data_carrying_enum.rs: parse → compile →
//! eval; extract Demo.bore / Demo.r.
//!
//! Tests:
//!   1. bore_ok_default_is_5mm (PRIMARY §1 signal) — `Ok { value: 5mm }` default
//!      → Demo.bore = 0.005 m; Demo.r = Result::Ok.
//!   2. tree_sum_total_is_3mm — recursive `Tree<Length>` (1mm + 2mm leaves)
//!      summed via nested two-arm match → Demo.total = 0.003 m (INV-5 e2e).
//!   3. bore_err_switch_is_6mm — `Err { error: "bad" }` default switch
//!      → Demo.bore = 0.006 m; Demo.r = Result::Err.
//!   4. recursive_tree_decl_emits_no_error_diagnostics — INV-5/D5: an
//!      isolated recursive generic-enum decl (Tree<T> alone, no Demo
//!      structure) emits no static-termination error (there is no
//!      termination checker).

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

// ── helper ───────────────────────────────────────────────────────────────────

fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

/// Read + eval `examples/m6_generic_enum.ri`. Centralizes the path and
/// expect message shared by the example-integration tests below (amend:
/// review — dedup of repeated read_to_string + eval_source call sites).
fn eval_example() -> reify_eval::EvalResult {
    let source = std::fs::read_to_string("../../examples/m6_generic_enum.ri")
        .expect("examples/m6_generic_enum.ri should exist");
    eval_source(&source)
}

// ── test 1: PRIMARY §1 signal ─────────────────────────────────────────────────

/// `reify eval examples/m6_generic_enum.ri` → Demo.bore = 0.005 m
/// (`Ok { value: 5mm }` default, extracted through the Ok arm's `v` binder).
///
/// The PRD §1/§8 user-observable signal. RED until step-2 authors
/// examples/m6_generic_enum.ri: `read_to_string(...)` panics.
#[test]
fn bore_ok_default_is_5mm() {
    let result = eval_example();

    let bore_id = ValueCellId::new("Demo", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Demo.bore not found in eval result"));

    match bore_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected Demo.bore ≈ 0.005 m (5mm), got {si_value} m"
            );
        }
        other => panic!("expected Value::Scalar for Demo.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Demo", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Demo.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(
                variant, "Ok",
                "Demo.r should be Result::Ok (default variant)"
            );
        }
        other => panic!("expected Value::Enum for Demo.r, got {:?}", other),
    }
}

// ── test 2: recursive Tree<Length> summed via nested match (INV-5 e2e) ──────

/// `reify eval examples/m6_generic_enum.ri` → Demo.total = 0.003 m: the
/// recursive `Tree<Length>` value (`Node { left: Leaf{1mm}, right: Leaf{2mm} }`)
/// is built bottom-up and summed via a nested two-arm match — INV-5 end-to-end.
#[test]
fn tree_sum_total_is_3mm() {
    let result = eval_example();

    let total_id = ValueCellId::new("Demo", "total");
    let total_val = result
        .values
        .get(&total_id)
        .unwrap_or_else(|| panic!("Demo.total not found in eval result"));

    match total_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.003).abs() < 1e-12,
                "expected Demo.total ≈ 0.003 m (1mm+2mm Tree<Length> leaves), got {si_value} m"
            );
            // amend: review — pin the dimension too, not just the SI magnitude,
            // so a regression that sums Tree<Length> leaves as dimensionless (or
            // with the wrong unit exponent) can't land 0.003 silently.
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "expected Demo.total to carry LENGTH dimension (Tree<Length> leaves), got {dimension:?}"
            );
        }
        other => panic!("expected Value::Scalar for Demo.total, got {:?}", other),
    }
}

// ── test 3: Err-switch — the §1 default-switch signal ────────────────────────

/// Inline source with `Err { error: "bad" }` default → Demo.bore = 0.006 m
/// (the Err arm's literal fallback), and Demo.r reports the Err tag.
#[test]
fn bore_err_switch_is_6mm() {
    let source = r#"
module m6_test_generic_enum_err

enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}

structure def Demo {
    param r : Result<Length, String> = Err { error: "bad" }

    let bore = match r {
        Ok { value: v } => v,
        Err { error: m } => 6mm
    }
}
"#;

    let result = eval_source(source);

    let bore_id = ValueCellId::new("Demo", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Demo.bore not found in eval result"));
    match bore_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.006).abs() < 1e-12,
                "expected Demo.bore ≈ 0.006 m (Err-arm fallback 6mm), got {si_value} m"
            );
        }
        other => panic!("expected Value::Scalar for Demo.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Demo", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Demo.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(variant, "Err", "Demo.r should switch to Result::Err");
        }
        other => panic!("expected Value::Enum for Demo.r, got {:?}", other),
    }
}

// ── test 4: INV-5 — recursive generic-enum decl compiles clean ──────────────

/// INV-5/D5: a well-formed recursive generic-enum declaration (`Tree<T>`'s
/// `Node` variant recurring on itself) emits NO static-termination error —
/// there is no termination checker; a `Value::Enum` is a finite, bottom-up-built
/// value (pinned at runtime by `tree_sum_total_is_3mm` above).
///
/// Uses an ISOLATED inline source containing ONLY the `Tree<T>` decl (no
/// `Demo` structure / bore / total lets), so this assertion pins the
/// recursive-decl behavior itself rather than "the whole example has zero
/// errors" (amend: review — the previous version compiled the full example,
/// which would also pass/fail on unrelated diagnostics elsewhere in the
/// file). Full-example compile-cleanliness is separately covered by
/// `examples_smoke.rs`'s glob-based gate and is implied by the eval tests
/// above (a broken compile would make them panic).
#[test]
fn recursive_tree_decl_emits_no_error_diagnostics() {
    let source = r#"
module m6_test_tree_decl_only

enum Tree<T> {
    Leaf { value: T },
    Node { left: Tree<T>, right: Tree<T> },
}
"#;

    let compiled = parse_and_compile(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "recursive Tree<T> declaration must compile with zero Error diagnostics \
         (INV-5: no static-termination checker); got {:?}",
        errors
    );
}
