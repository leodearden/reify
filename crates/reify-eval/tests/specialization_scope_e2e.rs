//! End-to-end eval test for `sub name : StructName { body }` specialization-scope body (task 3573).
//!
//! PRD AC-7: a module containing forbidden specialization-scope body members must
//! be evaluable without panic — the engine must gracefully handle a compiled module
//! that carries `SpecializationForbiddenDecl` Error diagnostics.
//!
//! Tests drive real `.ri` source through the full
//! `compile_source_with_stdlib → make_simple_engine → engine.eval` pipeline.
//! Diagnostics are filtered by `DiagnosticCode::SpecializationForbiddenDecl` to
//! isolate the relevant signal from unrelated noise (mirrors the dep-test convention).
//!
//! `parse_and_compile_with_stdlib` is intentionally NOT used here because it asserts
//! `errors.is_empty()` — the forbidden fixture produces `Severity::Error` diagnostics
//! by design. `compile_source_with_stdlib` is used instead to compile without that assertion.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{compile_source_with_stdlib, make_simple_engine};

/// PRD AC-7: The forbidden specialization-scope fixture must:
/// 1. Surface at least one `SpecializationForbiddenDecl` Error diagnostic through
///    the full `compile_with_stdlib` path (validator is wired).
/// 2. Evaluate without panicking — the engine handles compiled modules with
///    compile-time Error diagnostics (eval result may be empty / diagnostic-only).
///
/// Reaching the post-eval assertion is the AC-7 signal: "no parse-error/panic surfaces".
#[test]
fn forbidden_spec_scope_evaluates_without_panic_and_surfaces_diagnostic() {
    let source = include_str!("fixtures/specialization_scope_forbidden.ri");

    // Compile (without the `errors.is_empty()` assertion — the forbidden fixture
    // produces Error-severity diagnostics intentionally).
    let compiled = compile_source_with_stdlib(source);

    // --- AC-7 pre-condition: validator surfaced SpecializationForbiddenDecl ---
    let forbidden: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl))
        .collect();

    assert!(
        !forbidden.is_empty(),
        "expected at least one SpecializationForbiddenDecl diagnostic in compiled.diagnostics \
         (confirming the validator fires through compile_with_stdlib), got none.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );

    assert_eq!(
        forbidden[0].severity,
        Severity::Error,
        "SpecializationForbiddenDecl must be Error severity"
    );

    // --- AC-7 signal: eval runs without panic, no new Error diagnostics injected ---
    // `eval_result.diagnostics` are engine-emitted at eval time; they are distinct
    // from `compiled.diagnostics` (the compile-time pass). For a module whose only
    // errors are compile-time `SpecializationForbiddenDecl` diagnostics, the eval
    // pass must handle the compiled module gracefully and must NOT inject new
    // Error-severity diagnostics of its own. An empty `eval_result.values` map is
    // an acceptable outcome — the specialization body members were not lowered.
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    let eval_errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval must not inject new Error diagnostics for the forbidden spec-scope fixture \
         (compile-time SpecializationForbiddenDecl errors live in compiled.diagnostics, \
         not eval_result.diagnostics); got: {:#?}",
        eval_errors
    );
}
