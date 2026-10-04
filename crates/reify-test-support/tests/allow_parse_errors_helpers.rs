//! Contract tests for [`reify_test_support::compile_source_allow_parse_errors`]
//! and [`reify_test_support::compile_source_with_stdlib_allow_parse_errors`]
//! (task #7063).
//!
//! The contract: each helper reports exactly what the production compile path
//! reports, i.e. each parse error ONCE, at `Severity::Error`. A test that counts
//! diagnostics through one of these helpers must see the count a real caller of
//! `reify_compiler::compile` / `compile_with_stdlib` sees.
//!
//! The survivor is identified by its label SPAN, never by its message text: the
//! wording is reify-compiler's, and the span is the structured key.

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_core::ModulePath;

/// Measured: exactly one parse error (span 42..45, the `@@@`) under both
/// `reify_syntax::parse` and `reify_compiler::parse_with_stdlib`, and zero
/// compile-layer diagnostics.
const ONE_PARSE_ERROR: &str = "structure S {\n  param x : Length = 10mm\n}\n@@@\n";

fn parse_plain(source: &str) -> ParsedModule {
    reify_syntax::parse(source, ModulePath::single("test"))
}

fn parse_stdlib(source: &str) -> ParsedModule {
    reify_compiler::parse_with_stdlib(source, ModulePath::single("test"))
}

#[track_caller]
fn assert_each_parse_error_reported_once(
    helper: &str,
    compile: fn(&str) -> CompiledModule,
    parse: fn(&str) -> ParsedModule,
) {
    let parsed = parse(ONE_PARSE_ERROR);
    assert_eq!(
        parsed.errors.len(),
        1,
        "fixture precondition: ONE_PARSE_ERROR must yield exactly one parse error, got {:?}",
        parsed.errors
    );

    let module = compile(ONE_PARSE_ERROR);
    let errors = reify_test_support::errors_only(&module);
    let rendered: Vec<_> = module
        .diagnostics
        .iter()
        .map(|d| {
            let spans: Vec<_> = d.labels.iter().map(|l| l.span).collect();
            (d.severity, d.message.as_str(), spans)
        })
        .collect();
    assert_eq!(
        errors.len(),
        parsed.errors.len(),
        "{helper} must report each parse error exactly once; the fixture is otherwise \
         clean (measured: zero compile-layer diagnostics), so an extra Error is a \
         duplicated report of the same parse error. All diagnostics: {rendered:#?}"
    );

    let expected_span = parsed.errors[0].span;
    assert!(
        errors[0].labels.iter().any(|l| l.span == expected_span),
        "{helper}: the surviving Error must be the parse error, labelled at {expected_span:?}. \
         All diagnostics: {rendered:#?}"
    );
}

#[test]
fn compile_source_allow_parse_errors_reports_each_parse_error_once() {
    assert_each_parse_error_reported_once(
        "compile_source_allow_parse_errors",
        reify_test_support::compile_source_allow_parse_errors,
        parse_plain,
    );
}

#[test]
fn compile_source_with_stdlib_allow_parse_errors_reports_each_parse_error_once() {
    assert_each_parse_error_reported_once(
        "compile_source_with_stdlib_allow_parse_errors",
        reify_test_support::compile_source_with_stdlib_allow_parse_errors,
        parse_stdlib,
    );
}
