//! Source-driven rewrite of the leaf-signal test from
//! phase-3-grammar-fiction-triage-log.md §B3 (task 3571).
//!
//! User-observable leaf signal: existing `compile_pipeline_invokes_specialization_scope_validator`
//! test (hand-built AST) rewritten to start from `.ri` source and continues to pass.
//!
//! These tests parse `.ri` source through the full
//! `reify_syntax::parse → reify_compiler::compile` pipeline and verify that
//! `SubDecl.body` is correctly populated (`Some(...)`) when the
//! `specialization_body` CST node is present (task 3571).
//!
//! Tests filter `compiled.diagnostics` by `DiagnosticCode::SpecializationForbiddenDecl`
//! to isolate the relevant diagnostics from unrelated noise (e.g. unresolved-name
//! diagnostics from stub types like `"Foo"`).

use reify_types::{DiagnosticCode, ModulePath, Severity};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Filter a slice of diagnostics to only those with
/// `code == DiagnosticCode::SpecializationForbiddenDecl`.
fn forbidden_diagnostics(diagnostics: &[reify_types::Diagnostic]) -> Vec<&reify_types::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl))
        .collect()
}

// ── Leaf-signal test (RED before step-2) ─────────────────────────────────────

/// Source-driven rewrite of `compile_pipeline_invokes_specialization_scope_validator`.
///
/// Parses `.ri` source `structure S { sub scope : Foo { param x } }`, runs the
/// full compile pipeline, and asserts that the specialization-scope validator
/// fires `SpecializationForbiddenDecl` — which requires `lower_sub` to
/// populate `body: Some([MemberDecl::Param("x")])`.
///
/// This test is RED before step-2 because `lower_sub` currently hardcodes
/// `body: None` — the validator's walker is a no-op when `body.is_none()`, so
/// `param x` is never visited and no `SpecializationForbiddenDecl` is emitted.
///
/// Leaf signal from phase-3-grammar-fiction-triage-log.md §B3.
#[test]
fn compile_pipeline_invokes_specialization_scope_validator_from_source() {
    let source = "structure S { sub scope : Foo { param x } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_spec_scope_validator"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors (grammar from task 3569 must accept this form), got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert!(
        !diags.is_empty(),
        "expected at least one SpecializationForbiddenDecl diagnostic confirming the validator \
         fires when body is populated — lower_sub must set body: Some([Param(x)]) for the \
         validator to visit `param x` and emit the diagnostic; got none.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );

    assert_eq!(
        diags[0].severity,
        Severity::Error,
        "SpecializationForbiddenDecl must be Error severity"
    );
}
