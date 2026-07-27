//! End-to-end eval tests for the free-function `generate(n, |i| expr)` combinator
//! (task 3994 / structural-query ζ, PRD §5.9 / §2.3).
//!
//! `generate(n, |i| expr)` applies the lambda to indices `0..n-1` in order and
//! collects the results into a `List`.  Model: parse → `reify_compiler::compile`
//! → `Engine::eval` → assert `result.values`, mirroring `structural_query_eval.rs`.

use reify_core::{DiagnosticCode, ModulePath, Severity, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

/// Parse + compile + eval `source`, asserting no parse/compile Error diagnostics,
/// and return the evaluated result.
fn eval_source(source: &str) -> EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ─── step-7: eval core ───

/// `generate(4, |i| i)` evaluates to `[Int(0), Int(1), Int(2), Int(3)]` — the
/// lambda is applied to indices 0..3 in order.
///
/// RED today: there is no free-fn `generate` arm in eval_expr's FunctionCall
/// dispatch, so it falls through to `reify_stdlib::eval_builtin` (which has no
/// `generate` builtin) → `Value::Undef`. The eval dispatch (step-8) makes this GREEN.
#[test]
fn generate_positive_count_yields_index_list() {
    let result = eval_source(
        r#"
        structure S {
            let a = generate(4, |i| i)
        }
    "#,
    );
    let a = result.values.get(&ValueCellId::new("S", "a"));
    assert_eq!(
        a,
        Some(&Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ])),
        "generate(4, |i| i) should be [0,1,2,3]; got: {:?}",
        a,
    );
}

/// `generate(0, |i| i)` evaluates to the empty list `[]`.
#[test]
fn generate_zero_count_yields_empty_list() {
    let result = eval_source(
        r#"
        structure S {
            let b = generate(0, |i| i)
        }
    "#,
    );
    let b = result.values.get(&ValueCellId::new("S", "b"));
    assert_eq!(
        b,
        Some(&Value::List(vec![])),
        "generate(0, |i| i) should be the empty list; got: {:?}",
        b,
    );
}

/// `generate(3, |i| i * 1mm)` evaluates to a 3-element list of `Length`s
/// `[0mm, 1mm, 2mm]` (in SI metres: 0.0, 0.001, 0.002) — proving a non-Int body
/// type flows through and length is preserved.
#[test]
fn generate_length_body_yields_list_of_lengths() {
    let result = eval_source(
        r#"
        structure S {
            let c = generate(3, |i| i * 1mm)
        }
    "#,
    );
    match result.values.get(&ValueCellId::new("S", "c")) {
        Some(Value::List(items)) => {
            assert_eq!(items.len(), 3, "expected 3 elements; got: {:?}", items);
            for (idx, item) in items.iter().enumerate() {
                match item {
                    Value::Scalar { si_value, .. } => {
                        let expected = idx as f64 * 0.001; // idx mm in SI metres
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "element {} should be {} m (= {}mm); got si_value {}",
                            idx,
                            expected,
                            idx,
                            si_value,
                        );
                    }
                    other => panic!("element {} should be a Length scalar; got {:?}", idx, other),
                }
            }
        }
        other => panic!("S.c should be a List of 3 lengths; got: {:?}", other),
    }
}

// ─── step-9: negative-count named diagnostic ───

/// `generate(-1, |i| i)` leaves the cell `Undef` AND pushes a `Severity::Error`
/// eval diagnostic carrying `DiagnosticCode::GenerateNegativeCount` — a negative
/// count is a runtime contract failure (PRD §2.3).
///
/// The negative literal `-1` types as `Int` (UnOp::Neg over Int), so it PASSES
/// the compile-time `ExpectedArg::Int` count check (step-6) and reaches eval.
///
/// RED today: `DiagnosticCode::GenerateNegativeCount` does not exist (minted in
/// step-10) so this file does not compile; and nothing is emitted for `n < 0`
/// (eval_generate_dispatch currently yields the empty list for a negative range).
/// The n<0 branch (step-10) makes this GREEN.
#[test]
fn generate_negative_count_emits_named_diagnostic() {
    let result = eval_source(
        r#"
        structure S {
            let d = generate(-1, |i| i)
        }
    "#,
    );
    let d = result.values.get(&ValueCellId::new("S", "d"));
    assert_eq!(
        d,
        Some(&Value::Undef),
        "generate(-1, |i| i) should leave the cell Undef; got: {:?}",
        d,
    );
    let has_named = result.diagnostics.iter().any(|diag| {
        diag.severity == Severity::Error
            && diag.code == Some(DiagnosticCode::GenerateNegativeCount)
    });
    assert!(
        has_named,
        "expected a Severity::Error GenerateNegativeCount diagnostic; got: {:?}",
        result.diagnostics,
    );
}

// ─── step-11: example golden (structured) ───

/// Extract the `(x, y, z)` SI-metre components of a `point3` `Value::Point`.
fn point3_xyz(v: &Value) -> (f64, f64, f64) {
    match v {
        Value::Point(comps) if comps.len() == 3 => {
            let f = |c: &Value| c.as_f64().expect("point component should be numeric");
            (f(&comps[0]), f(&comps[1]), f(&comps[2]))
        }
        other => panic!("expected a 3-component point3; got: {:?}", other),
    }
}

/// The `examples/generate_bolt_circle.ri` golden: `generate(bolt_count = 4, …)`
/// places 4 point3s at exact quarter-turns on a 50 mm circle, plus `empty =
/// generate(0, |i| i)`.
///
/// `n == Int(4)` and `empty == []` are the PRIMARY (FP-free) signals; the
/// coordinates are checked within a 1e-6 mm tolerance (basis: f64 trig error at
/// exact k·π/2 ≤ ~1e-15 absolute → ~5e-19 m at r = 50mm, ~9 orders below the
/// tolerance — NOT exact equality, which the ~6e-17 cos(π/2) residue would break).
///
/// RED today: `examples/generate_bolt_circle.ri` does not exist (step-12 creates it).
#[test]
fn generate_bolt_circle_example_golden() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/generate_bolt_circle.ri"
    );
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("example {} should exist (step-12): {}", path, e));
    let result = eval_source(&source);

    // (a) PRIMARY: count(positions) == Int(4), exact (FP-free).
    let n_id = ValueCellId::new("BoltCircle", "n");
    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::Int(4)),
        "BoltCircle.n should be Int(4); got: {:?}",
        result.values.get(&n_id),
    );

    // (b) generate(0, |i| i) → [] (exact).
    let empty_id = ValueCellId::new("BoltCircle", "empty");
    assert_eq!(
        result.values.get(&empty_id),
        Some(&Value::List(vec![])),
        "BoltCircle.empty should be the empty list; got: {:?}",
        result.values.get(&empty_id),
    );

    // (c) positions: 4 point3s at golden quarter-turns (within 1e-9 m = 1e-6 mm).
    const TOL_M: f64 = 1e-9; // 1e-6 mm in SI metres
    let r = 0.05; // 50 mm in SI metres
    let golden = [
        (r, 0.0, 0.0),
        (0.0, r, 0.0),
        (-r, 0.0, 0.0),
        (0.0, -r, 0.0),
    ];
    match result.values.get(&ValueCellId::new("BoltCircle", "positions")) {
        Some(Value::List(items)) => {
            assert_eq!(items.len(), 4, "expected 4 positions; got: {:?}", items);
            for (idx, (item, (gx, gy, gz))) in items.iter().zip(golden.iter()).enumerate() {
                let (x, y, z) = point3_xyz(item);
                assert!(
                    (x - gx).abs() < TOL_M && (y - gy).abs() < TOL_M && (z - gz).abs() < TOL_M,
                    "position {} should be ({}, {}, {}) m; got ({}, {}, {})",
                    idx,
                    gx,
                    gy,
                    gz,
                    x,
                    y,
                    z,
                );
            }
        }
        other => panic!("BoltCircle.positions should be a List of 4 point3s; got: {:?}", other),
    }
}
