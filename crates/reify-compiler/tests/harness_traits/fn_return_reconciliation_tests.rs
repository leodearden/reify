//! Fn body type vs declared or defaulted return type reconciliation (task #5991).

use reify_compiler::CompiledModule;
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source;

fn with_code(module: &CompiledModule, code: DiagnosticCode) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(code))
        .collect()
}

fn assert_no_errors(module: &CompiledModule) {
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:?}"
    );
}

fn has_label_spanning(diagnostic: &Diagnostic, source: &str, needle: &str) -> bool {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("fixture must contain `{needle}`")) as u32;
    let end = start + needle.len() as u32;
    diagnostic
        .labels
        .iter()
        .any(|l| l.span.start == start && l.span.end == end)
}

// ── Un-annotated free fns ───────────────────────────────────────────────────

/// An un-annotated fn whose body is dimensioned is typed `Real` at every call
/// site; the contradiction is reported as a warning anchored on the body.
#[test]
fn unannotated_fn_whose_body_is_dimensioned_warns() {
    let source = r#"
fn area(w: Length, h: Length) { w * h }
structure Plate {
    param w : Length = 2mm
    let a = area(w, w)
    constraint a > 5.0
}
"#;
    let module = compile_source(source);

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
    assert_eq!(unannotated[0].severity, Severity::Warning);
    assert_no_errors(&module);
    assert!(
        has_label_spanning(unannotated[0], source, "w * h"),
        "expected a label spanning the body `w * h`, got: {:?}",
        unannotated[0].labels
    );
}

#[test]
fn unannotated_fn_with_bool_body_warns() {
    let module = compile_source("fn is_long(x: Length) { x > 5mm }\n");

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Trait-static fns are compiled through `compile_function` under the
/// namespaced name `Defaults::default_len`.
#[test]
fn unannotated_trait_static_fn_with_dimensioned_body_warns() {
    let module = compile_source("trait Defaults { fn default_len() { 10mm } }\n");

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Bodies that agree with the `Real` default stay silent: a dimensionless
/// body, an `Int` body (widens to the default), and a bare type-param body
/// (generic, so agreement depends on the instantiation; see #7008).
#[test]
fn unannotated_fn_whose_body_agrees_with_real_is_silent() {
    for source in [
        "fn half(x: Real) { x / 2.0 }\n",
        "fn three() { 3 }\n",
        "fn id<T>(x: T) { x }\n",
    ] {
        let module = compile_source(source);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
            "expected no FnReturnTypeUnannotated for {source:?}, got: {:?}",
            module.diagnostics
        );
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

#[test]
fn unannotated_fn_with_erroring_body_reports_only_the_root_cause() {
    let module = compile_source("fn broken() { no_such_name }\n");

    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "expected the unresolved name to be reported as an Error, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "expected no cascading FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}
