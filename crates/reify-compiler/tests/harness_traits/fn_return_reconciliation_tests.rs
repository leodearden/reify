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

// ── Annotated free fns ──────────────────────────────────────────────────────

/// A body that contradicts an explicit scalar annotation is reported against
/// both the annotation and the body.
#[test]
fn annotated_scalar_return_contradicted_by_body_warns() {
    let source = "fn sq(x: Length) -> Length { x * x }\n";
    let module = compile_source(source);

    let mismatches = with_code(&module, DiagnosticCode::FnReturnTypeMismatch);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
    assert_eq!(mismatches[0].severity, Severity::Warning);
    assert_no_errors(&module);

    let annotation_start = (source.find("-> Length").unwrap() + 3) as u32;
    assert!(
        mismatches[0]
            .labels
            .iter()
            .any(|l| l.span.start == annotation_start),
        "expected a label starting at the return annotation, got: {:?}",
        mismatches[0].labels
    );
    assert!(
        has_label_spanning(mismatches[0], source, "x * x"),
        "expected a label spanning the body `x * x`, got: {:?}",
        mismatches[0].labels
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "an annotated fn must not report FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Evaluation returns the body value unconverted, so a bare literal body hands
/// a `Real` to a `Length`-typed call site: no numeric-literal exemption.
#[test]
fn annotated_dimensioned_return_with_bare_literal_body_warns() {
    let module = compile_source("fn unit_len() -> Length { 1.0 }\n");

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}

#[test]
fn annotated_int_return_with_real_body_warns() {
    let module = compile_source("fn trunc(x: Real) -> Int { x }\n");

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}

/// Agreeing bodies stay silent, including `Int` widening to `Real` and generic
/// declared types outside the `Int | Scalar` reconciliation scope.
#[test]
fn annotated_return_agreeing_with_body_is_silent() {
    for source in [
        "fn sq(x: Length) -> Area { x * x }\n",
        "fn one() -> Real { 1 }\n",
        "fn n() -> Int { 3 }\n",
        "fn id<T>(x: T) -> T { x }\n",
        "fn scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }\n",
    ] {
        let module = compile_source(source);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

#[test]
fn unresolved_return_annotation_reports_only_the_root_cause() {
    let module = compile_source("fn f(x: Length) -> Bogus { x }\n");

    assert!(
        !with_code(&module, DiagnosticCode::UnresolvedType).is_empty(),
        "expected the unresolved annotation to be reported, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
        "expected no cascading FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}
