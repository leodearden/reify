//! Tests for silent type defaults and missing diagnostics fixes (task 117).
//!
//! These tests verify that the compiler emits diagnostics instead of silently
//! swallowing errors or using misleading defaults.

use reify_types::Severity;

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("silent_defaults_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics.
fn error_diagnostics(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── H2: collection member typo should produce a diagnostic ──────────────

#[test]
fn collection_member_typo_produces_diagnostic() {
    // "diametr" is a typo for "diameter" — the compiler should emit
    // a diagnostic about an unknown member rather than silently defaulting
    // to Type::Real.
    let source = r#"
        structure Bolt {
            param diameter : Scalar = 10mm
        }
        structure Assembly {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[0].diametr
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_unknown_member = errors
        .iter()
        .any(|d| d.message.contains("unknown member"));
    assert!(
        has_unknown_member,
        "expected diagnostic about 'unknown member', got: {:?}",
        errors
    );
}
// ── H3: geometry call diagnostics ──────────────────────────────────────

#[test]
fn geometry_call_wrong_arg_count_produces_diagnostic() {
    // box() expects 3 arguments — passing only 2 should produce a diagnostic
    let source = r#"
        structure S {
            let shape = box(10mm, 20mm)
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_arg_count_error = errors
        .iter()
        .any(|d| d.message.contains("expects 3 arguments"));
    assert!(
        has_arg_count_error,
        "expected diagnostic about argument count, got: {:?}",
        errors
    );
}
