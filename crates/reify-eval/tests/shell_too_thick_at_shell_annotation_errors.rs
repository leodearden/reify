//! End-to-end integration test for the task ε `@shell` + too-thick hard-error path.
//!
//! `examples/fea_shell_too_thick_annotated.ri` (a 50 mm × 20 mm × 20 mm steel block
//! with `ElasticOptions(shell_force: ShellForce.On)` — the `@shell` proxy) is
//! evaluated through the full engine. The body's thickness/extent ratio is
//! `height / min(L,W) = 20/20 = 1.0 ≥ shell_threshold 0.2`, so it is "too thick"
//! for the shell route. With `ShellForce::On` (hard-error policy) the trampoline
//! must reject the solve with `DiagnosticCode::ShellTooThick` and return
//! `ComputeOutcome::Failed` (no tet fallback).
//!
//! PRD: docs/prds/v0_4/shell-extract-engine-bridge.md task ε (§9 Phase 4).

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::Value;

// ── helpers ────────────────────────────────────────────────────────────────────

/// The `@shell` too-thick fixture, compile-time baked via `include_str!`
/// (single source of truth — stays in sync with the user-facing example file).
fn annotated_source() -> &'static str {
    include_str!("../../../examples/fea_shell_too_thick_annotated.ri")
}

/// Build an engine with BOTH the elastic-static trampoline and the shell-extract
/// trampoline registered — mirrors `shell_solve_e2e.rs::shell_engine()`.
fn shell_engine() -> reify_eval::Engine {
    let mut engine = reify_test_support::make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}

/// Extract a named field from a `Value::StructureInstance` (returns `None` for
/// any other Value shape or a missing field).
fn struct_field(val: &Value, key: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
        _ => None,
    }
}

// ── test ───────────────────────────────────────────────────────────────────────

/// `@shell` on a too-thick body hard-errors: the solve must surface a
/// `Severity::Error` diagnostic with `code == DiagnosticCode::ShellTooThick`
/// and must NOT produce a `ShellStress` shell_channels result.
///
/// RED: before the too-thick gate, `@shell`(On) → classify Shell → trampoline
/// runs the shell solve on the thick body and returns Completed with no error,
/// so this assertion fails because no ShellTooThick diagnostic is emitted.
/// GREEN after step-6 adds the gate to `solve_elastic_static_trampoline`.
#[test]
fn shell_annotation_on_too_thick_body_errors_with_shell_too_thick_code() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(annotated_source());
    let mut engine = shell_engine();
    let eval_result = engine.eval(&compiled);

    // (1) Must surface a Severity::Error diagnostic with ShellTooThick code.
    let shell_too_thick_error = eval_result
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::ShellTooThick));
    assert!(
        shell_too_thick_error.is_some(),
        "expected a Severity::Error diagnostic with code=DiagnosticCode::ShellTooThick; \
         got diagnostics: {:?}",
        eval_result.diagnostics
    );

    // (2) No successful shell solve: result.shell_channels must NOT be a
    //     "ShellStress" StructureInstance. On the Failed path the result cell
    //     is either absent or Undef — the shell path must not have executed.
    let result_cell = ValueCellId::new("FeaShellTooThickAnnotated", "result");
    let no_shell_channels_populated = match eval_result.values.get(&result_cell) {
        None => true, // result cell absent — solve failed, no value produced
        Some(val) => {
            // result present but shell_channels must not be a ShellStress
            match struct_field(val, "shell_channels") {
                None => true,
                Some(Value::Undef) => true,
                Some(Value::StructureInstance(ref d)) if d.type_name == "ShellStress" => false,
                Some(_) => true,
            }
        }
    };
    assert!(
        no_shell_channels_populated,
        "expected result.shell_channels to be absent/Undef (failed solve), \
         but found a ShellStress StructureInstance — the @shell too-thick hard-error \
         must abort before running the shell solve"
    );
}
