//! End-to-end example test for fallback recovery via the worked example
//! `examples/m6_fallback_recovery.ri` (task 3986 — result-and-fallback PRD
//! `docs/prds/v0_6/result-and-fallback.md` §8 Phase 4 task δ, THE integration
//! gate / primary user-observable signal).
//!
//! A fallible stdlib op (`parse_length`) yields `Option<Length>`; the example
//! recovers with `unwrap_or`/`or_else` and branches on presence with
//! `is_some`, so a recoverable miss becomes a language value the model
//! branches on instead of an uncatchable `Freshness::Failed` node (§5 D1).
//!
//! Model:
//!   - `option_recovery_map_or_e2e.rs` — Engine::eval + value-cell read
//!     pattern, missing-example RED driver.
//!   - `parse_length_e2e.rs` — `mm()` Length helper for parse_length results.
//!
//! RED before step-2: `examples/m6_fallback_recovery.ri` does not exist yet,
//! so the fixture read fails (panic) — the missing-example RED signal.

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{collect_errors, mm, parse_and_compile_with_stdlib};

/// Absolute path to the example, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m6_fallback_recovery.ri"
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

/// `examples/m6_fallback_recovery.ri` must compile + eval with zero Error
/// diagnostics, and the six value cells across `MountValid` (raw="12mm") and
/// `MountBad` (raw="garbage") must evaluate to the composed recovery values:
///   - MountValid.bore     = unwrap_or(parse_length("12mm"), 6mm)              = 12mm
///   - MountValid.pin      = unwrap_or(or_else(parse_length("12mm"), parse_length("4mm")), 4mm) = 12mm
///   - MountValid.has_bore = is_some(parse_length("12mm"))                     = true
///   - MountBad.bore       = unwrap_or(parse_length("garbage"), 6mm)           = 6mm
///   - MountBad.pin        = unwrap_or(or_else(parse_length("garbage"), parse_length("4mm")), 4mm) = 4mm
///   - MountBad.has_bore   = is_some(parse_length("garbage"))                  = false
///
/// This is the formatter-agnostic integration-gate assertion; the companion
/// `crates/reify-cli/tests/cli_eval_fallback_recovery.rs` pins the same
/// signal at the rendered-stdout binary surface (§1).
#[test]
fn fallback_recovery_example_evals_end_to_end() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m6_fallback_recovery.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/m6_fallback_recovery.ri should compile with no Error diagnostics, got: {errors:?}"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {eval_errors:?}");

    // Valid input: raw="12mm" parses to some(12mm).
    assert_eq!(
        cell_value(&result, "MountValid", "bore"),
        mm(12.0),
        "MountValid.bore = unwrap_or(parse_length(\"12mm\"), 6mm) must recover the parsed 12mm"
    );
    assert_eq!(
        cell_value(&result, "MountValid", "pin"),
        mm(12.0),
        "MountValid.pin = unwrap_or(or_else(parse_length(\"12mm\"), parse_length(\"4mm\")), 4mm) must keep the some(12mm) subject"
    );
    assert_eq!(
        cell_value(&result, "MountValid", "has_bore"),
        Value::Bool(true),
        "MountValid.has_bore = is_some(parse_length(\"12mm\")) must be true"
    );

    // Unparseable input: raw="garbage" parses to none.
    assert_eq!(
        cell_value(&result, "MountBad", "bore"),
        mm(6.0),
        "MountBad.bore = unwrap_or(parse_length(\"garbage\"), 6mm) must fall back to 6mm"
    );
    assert_eq!(
        cell_value(&result, "MountBad", "pin"),
        mm(4.0),
        "MountBad.pin = unwrap_or(or_else(parse_length(\"garbage\"), parse_length(\"4mm\")), 4mm) must recover through or_else to 4mm"
    );
    assert_eq!(
        cell_value(&result, "MountBad", "has_bore"),
        Value::Bool(false),
        "MountBad.has_bore = is_some(parse_length(\"garbage\")) must be false"
    );
}
