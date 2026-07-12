//! End-to-end example test for `fallback`/`or_else` propagation over
//! `Result<T, E>` — task result-fallback Layer-B δ #4038 (PRD
//! docs/prds/v0_6/result-and-fallback.md §4.3/§5 D6/§8.B task B-δ).
//!
//! Mirrors `parse_length_e2e.rs`'s manual pipeline (`parse_and_compile_with_stdlib`
//! plus a fresh `Engine::new(MockConstraintChecker)`) and `fallback_recovery_e2e.rs`'s
//! example-file-loading + value-cell-read pattern (task 3986's Option-subject
//! sibling), retargeted from `parse_length`/`unwrap_or` (Option) to
//! `parse_length_r`/`fallback` (Result).
//!
//! `examples/m6_result_fallback.ri` declares two structures — `Valid`
//! (raw="12mm", parses) and `Bad` (raw="garbage", fails) — each computing:
//!   - `bore = fallback(parse_length_r(raw), 6mm)`
//!   - `pin  = fallback(or_else(parse_length_r(raw), parse_length_r("4mm")), 6mm)`
//!
//! RED before step-5: `examples/m6_result_fallback.ri` does not exist yet, so
//! the fixture read fails (panic) — the missing-example RED signal.

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{collect_errors, mm, parse_and_compile_with_stdlib};

/// Absolute path to the example, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m6_result_fallback.ri"
);

fn cell_value(result: &reify_eval::EvalResult, structure: &str, member: &str) -> Value {
    let id = ValueCellId::new(structure, member);
    result.values.get(&id).cloned().unwrap_or_else(|| {
        panic!(
            "{structure}.{member} not found in eval result; available: {:?}",
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    })
}

/// `examples/m6_result_fallback.ri` must compile + eval with zero Error
/// diagnostics, and the four value cells across `Valid` (raw="12mm") and
/// `Bad` (raw="garbage") must evaluate to the composed recovery values:
///   - Valid.bore = fallback(parse_length_r("12mm"), 6mm)                                    = 12mm (Ok inner)
///   - Valid.pin  = fallback(or_else(parse_length_r("12mm"), parse_length_r("4mm")), 6mm)     = 12mm (or_else keeps the Ok subject)
///   - Bad.bore   = fallback(parse_length_r("garbage"), 6mm)                                  = 6mm  (Err -> default)
///   - Bad.pin    = fallback(or_else(parse_length_r("garbage"), parse_length_r("4mm")), 6mm)  = 4mm  (or_else falls to the "4mm" alt, then fallback unwraps it)
///
/// This is the formatter-agnostic integration-gate assertion; the companion
/// `crates/reify-cli/tests/cli_eval_result_fallback.rs` pins the same signal
/// at the rendered-stdout binary surface.
#[test]
fn result_fallback_example_evals_end_to_end() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m6_result_fallback.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/m6_result_fallback.ri should compile with no Error diagnostics, got: {errors:?}"
    );

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {eval_errors:?}");

    // Valid input: raw="12mm" parses to Ok(12mm).
    assert_eq!(
        cell_value(&result, "Valid", "bore"),
        mm(12.0),
        "Valid.bore = fallback(parse_length_r(\"12mm\"), 6mm) must recover the parsed 12mm"
    );
    assert_eq!(
        cell_value(&result, "Valid", "pin"),
        mm(12.0),
        "Valid.pin = fallback(or_else(parse_length_r(\"12mm\"), parse_length_r(\"4mm\")), 6mm) \
         must keep the Ok(12mm) subject via or_else, then unwrap it via fallback"
    );

    // Unparseable input: raw="garbage" parses to Err(..).
    assert_eq!(
        cell_value(&result, "Bad", "bore"),
        mm(6.0),
        "Bad.bore = fallback(parse_length_r(\"garbage\"), 6mm) must fall back to 6mm"
    );
    assert_eq!(
        cell_value(&result, "Bad", "pin"),
        mm(4.0),
        "Bad.pin = fallback(or_else(parse_length_r(\"garbage\"), parse_length_r(\"4mm\")), 6mm) \
         must recover through or_else to Ok(4mm), then unwrap it via fallback"
    );
}
