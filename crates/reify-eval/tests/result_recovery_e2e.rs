//! End-to-end example test for `match`-unwrap + `fallback` recovery over
//! `Result<T, E>` — task result-fallback Layer-B ε #4039 (PRD
//! docs/prds/v0_6/result-and-fallback.md §1/§8.B task B-ε), the PRIMARY
//! Layer-B user-observable leaf and B+H integration gate.
//!
//! Mirrors `result_fallback_e2e.rs` (task δ #4038)'s scaffold (EXAMPLE_PATH
//! const, `parse_and_compile_with_stdlib` + a fresh
//! `Engine::new(MockConstraintChecker)`, `cell_value` helper), retargeted to
//! `examples/m6_result_recovery.ri`.
//!
//! `examples/m6_result_recovery.ri` declares two structures — `MountValid`
//! (raw="12mm", parses) and `MountBad` (raw="garbage", fails) — each
//! computing, via a user fn `parse_len` that wraps the erased-type
//! `parse_length_r` builtin with a concrete `Result<Length, String>` return
//! annotation (so the `match` payload binders concretize):
//!   - `bore    = match parse_len(raw) { Ok { value: v } => v, Err { error: msg } => 6mm }`
//!   - `bore_fb = fallback(parse_len(raw), 6mm)`
//!
//! This test (pair 1) pins the numeric bore/bore_fb recovery signal only;
//! the Err-payload-message `diag` cell (pair 2, the signal that
//! distinguishes this task from Option/Layer A) is added in a later
//! iteration of this same file.
//!
//! RED before step-2: `examples/m6_result_recovery.ri` does not exist yet, so
//! the fixture read fails (panic) — the missing-example RED signal.

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{collect_errors, mm, parse_and_compile_with_stdlib};

/// Absolute path to the example, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m6_result_recovery.ri"
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

/// `examples/m6_result_recovery.ri` must compile + eval with zero Error
/// diagnostics, and the bore/bore_fb value cells across `MountValid`
/// (raw="12mm") and `MountBad` (raw="garbage") must evaluate to the composed
/// recovery values:
///   - MountValid.bore    = match parse_len("12mm")    { Ok{value:v}=>v, .. }  = 12mm (Ok arm binder)
///   - MountValid.bore_fb = fallback(parse_len("12mm"), 6mm)                   = 12mm (Ok inner)
///   - MountBad.bore      = match parse_len("garbage") { .., Err{error:msg}=>6mm } = 6mm (Err arm literal)
///   - MountBad.bore_fb   = fallback(parse_len("garbage"), 6mm)                = 6mm (Err -> default)
///
/// This is the formatter-agnostic integration-gate assertion; the companion
/// `crates/reify-cli/tests/cli_eval_result_recovery.rs` pins the same signal
/// at the rendered-stdout binary surface.
#[test]
fn result_recovery_example_evals_end_to_end() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m6_result_recovery.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/m6_result_recovery.ri should compile with no Error diagnostics, got: {errors:?}"
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
        cell_value(&result, "MountValid", "bore"),
        mm(12.0),
        "MountValid.bore = match parse_len(\"12mm\") {{ Ok{{value:v}}=>v, .. }} must unwrap the parsed 12mm"
    );
    assert_eq!(
        cell_value(&result, "MountValid", "bore_fb"),
        mm(12.0),
        "MountValid.bore_fb = fallback(parse_len(\"12mm\"), 6mm) must recover the parsed 12mm"
    );

    // Unparseable input: raw="garbage" parses to Err(..).
    assert_eq!(
        cell_value(&result, "MountBad", "bore"),
        mm(6.0),
        "MountBad.bore = match parse_len(\"garbage\") {{ .., Err{{error:msg}}=>6mm }} must fall through to the Err arm's literal 6mm"
    );
    assert_eq!(
        cell_value(&result, "MountBad", "bore_fb"),
        mm(6.0),
        "MountBad.bore_fb = fallback(parse_len(\"garbage\"), 6mm) must fall back to 6mm"
    );
}
