//! End-to-end integration test for the task ε Auto + too-thick fallback path.
//!
//! `examples/fea_shell_too_thick_auto.ri` (a 50 mm × 20 mm × 20 mm steel block
//! with bare `ElasticOptions()` — `shell_force` defaults to `ShellForce::Auto`)
//! is evaluated through the full engine. The body's thickness/extent ratio is
//! `height / min(L,W) = 20/20 = 1.0 ≥ shell_threshold 0.2`, so it is "too thick"
//! for the shell route. With `ShellForce::Auto` (TetFallbackWithWarning policy)
//! the trampoline must:
//!   1. Emit NO Error-severity diagnostics (the solve succeeds).
//!   2. Emit a Warning-severity diagnostic with `code == DiagnosticCode::ShellTooThick`.
//!   3. Return `result.shell_channels == Value::Undef` (tet path, no ShellStress).
//!   4. Return a populated (non-Undef) `result.stress` Field.
//!
//! PRD: docs/prds/v0_4/shell-extract-engine-bridge.md task ε (§9 Phase 4).

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::Value;

// ── helpers ────────────────────────────────────────────────────────────────────

/// The `Auto` too-thick fixture, compile-time baked via `include_str!`
/// (single source of truth — stays in sync with the user-facing example file).
fn auto_source() -> &'static str {
    include_str!("../../../examples/fea_shell_too_thick_auto.ri")
}

/// Build an engine with BOTH the elastic-static trampoline and the shell-extract
/// trampoline registered — mirrors `shell_solve_e2e.rs::shell_engine()`.
fn shell_engine() -> reify_eval::Engine {
    let mut engine = reify_test_support::make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}

/// Extract a named field from a `Value::StructureInstance`.
fn struct_field(val: &Value, key: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
        _ => None,
    }
}

// ── test ───────────────────────────────────────────────────────────────────────

/// Auto on a too-thick body: the solve must fall back to tet (success), emit a
/// `Severity::Warning` with `code == DiagnosticCode::ShellTooThick`, produce
/// `shell_channels == Undef` (tet path), and produce a non-Undef `stress` Field.
///
/// RED: after step-6, Auto+too-thick runs the tet path (success, shell_channels
/// Undef) but the gate does NOT yet push a ShellTooThick warning into
/// `route_diagnostics` — assertion (2) fails.
/// GREEN after step-8 adds the warning push.
#[test]
fn auto_on_too_thick_body_warns_and_falls_back_to_tet() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(auto_source());
    let mut engine = shell_engine();
    let eval_result = engine.eval(&compiled);

    // (1) No Error-severity diagnostics — the solve must succeed.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics (tet fallback should succeed); \
         got errors: {:?}",
        errors
    );

    // (2) Must surface a Severity::Warning with ShellTooThick code.
    let shell_too_thick_warning = eval_result.diagnostics.iter().find(|d| {
        d.severity == Severity::Warning && d.code == Some(DiagnosticCode::ShellTooThick)
    });
    assert!(
        shell_too_thick_warning.is_some(),
        "expected a Severity::Warning diagnostic with code=DiagnosticCode::ShellTooThick; \
         got diagnostics: {:?}",
        eval_result.diagnostics
    );

    // (3) result.shell_channels must be Undef — the tet path, not the shell path.
    let result_cell = ValueCellId::new("FeaShellTooThickAuto", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .expect("result cell must be present — tet solve should have succeeded");

    let shell_channels = struct_field(result_val, "shell_channels")
        .unwrap_or(Value::Undef);
    assert!(
        matches!(shell_channels, Value::Undef),
        "expected result.shell_channels == Undef (tet path, no ShellStress), \
         got: {:?}",
        shell_channels
    );

    // (4) result.stress must be a populated (non-Undef) Field.
    let stress = struct_field(result_val, "stress")
        .unwrap_or(Value::Undef);
    assert!(
        !matches!(stress, Value::Undef),
        "expected result.stress to be a populated Field (tet solve produces stress), \
         got Undef"
    );
}
