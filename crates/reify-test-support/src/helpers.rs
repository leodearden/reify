//! Pipeline helpers for parsing, compiling, and evaluating Reify source in tests.

use reify_compiler::TopologyTemplate;
use reify_types::{Diagnostic, ModulePath, Severity};

#[cfg(feature = "eval-helpers")]
use crate::mocks::MockConstraintChecker;

/// Create a new `Engine` backed by a fresh `MockConstraintChecker` and no
/// geometry kernel. Suitable for tests that only need to evaluate logic
/// expressions and constraints without real geometry.
#[cfg(feature = "eval-helpers")]
pub fn make_engine() -> reify_eval::Engine {
    let checker = MockConstraintChecker::new();
    reify_eval::Engine::new(Box::new(checker), None)
}

/// Create a new `Engine` backed by the real `SimpleConstraintChecker` and no
/// geometry kernel. Suitable for integration tests that need the real
/// constraint semantics (Satisfied/Violated/Indeterminate) rather than the
/// mock's tracking-only stub.
#[cfg(feature = "eval-helpers")]
pub fn make_simple_engine() -> reify_eval::Engine {
    reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None)
}

/// Parse `source` with the canonical `"test"` module path, asserting no parse errors.
///
/// # Panics
/// Panics if there are any parse errors.
fn parse_or_panic(source: &str) -> reify_syntax::ParsedModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    parsed
}

/// Parse and compile `source` without asserting absence of compile errors.
/// Returns the compiled module with whatever diagnostics were produced.
///
/// Use this for tests that expect compilation errors/warnings. For tests
/// that expect clean compilation, use [`parse_and_compile`] instead.
///
/// # Panics
/// Panics if there are any parse errors (but NOT compile errors).
pub fn compile_source(source: &str) -> reify_compiler::CompiledModule {
    let parsed = parse_or_panic(source);
    reify_compiler::compile(&parsed)
}

/// Parse and compile `source` with stdlib, without asserting absence of compile errors.
///
/// Like [`compile_source`] but uses `reify_compiler::compile_with_stdlib` so that
/// stdlib types and traits are available during compilation.
///
/// # Panics
/// Panics if there are any parse errors (but NOT compile errors).
pub fn compile_source_with_stdlib(source: &str) -> reify_compiler::CompiledModule {
    let parsed = parse_or_panic(source);
    reify_compiler::compile_with_stdlib(&parsed)
}

/// Parse and compile `source`, then extract the first template.
/// Returns the template and the full list of diagnostics.
///
/// # Panics
/// Panics if there are parse errors or if the compiled module has no templates.
pub fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let compiled = compile_source(source);
    let diagnostics = compiled.diagnostics;
    let template = compiled
        .templates
        .into_iter()
        .next()
        .expect("compile_first_template: no templates in compiled module");
    (template, diagnostics)
}

/// Parse and compile `source`, then extract the template with the given `name`.
/// Returns the template and the full list of diagnostics.
///
/// # Panics
/// Panics if there are parse errors or if no template with `name` is found.
pub fn compile_template(source: &str, name: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let compiled = compile_source(source);
    let diagnostics = compiled.diagnostics;
    let template = compiled
        .templates
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("compile_template: template {:?} not found", name));
    (template, diagnostics)
}

/// Filter a diagnostic slice to only `Severity::Error` entries.
///
/// This is the primitive; [`errors_only`] is the convenience wrapper
/// that takes a `&CompiledModule`.
pub fn collect_errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Return only the `Severity::Error` diagnostics from a compiled module.
///
/// Convenience wrapper around [`collect_errors`].
pub fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    collect_errors(&module.diagnostics)
}

/// Return only the `Severity::Warning` diagnostics from a compiled module.
pub fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

/// Parse `source`, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module ready for eval.
///
/// # Panics
/// Panics if there are any parse errors or error-severity compile diagnostics.
pub fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    compiled
}

/// Parse `source`, assert no parse errors, compile with stdlib, assert no compile errors.
/// Returns the compiled module ready for eval.
///
/// Identical to [`parse_and_compile`] except uses `reify_compiler::compile_with_stdlib`
/// so that stdlib types and traits are available during compilation.
///
/// # Panics
/// Panics if there are any parse errors or error-severity compile diagnostics.
pub fn parse_and_compile_with_stdlib(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    compiled
}

/// Parse `source`, compile, assert ≥1 Error-severity diagnostic is produced.
/// If `needle` is non-empty, also assert at least one error message contains it.
/// Returns the `CompiledModule` for optional further assertions.
///
/// # Panics
/// Panics if there are parse errors, if no compile errors are produced, or
/// if `needle` is non-empty and no error message contains it.
pub fn parse_compile_expect_err(source: &str, needle: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one compile error");
    if !needle.is_empty() {
        assert!(
            errors.iter().any(|d| d.message.contains(needle)),
            "expected error containing {:?}, got: {:?}",
            needle,
            errors
        );
    }
    compiled
}

#[cfg(test)]
mod tests {
    use crate::fixtures::bracket_source;
    use reify_types::Severity;

    #[test]
    fn test_compile_source_valid() {
        let compiled = super::compile_source(bracket_source());
        assert!(
            !compiled.templates.is_empty(),
            "compile_source should produce at least one template for bracket source"
        );
    }

    #[test]
    fn test_compile_source_with_errors() {
        // Source with an undefined reference — compile_source should NOT panic,
        // instead it returns the module WITH error diagnostics.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let compiled = super::compile_source(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "compile_source with invalid source should produce error diagnostics"
        );
    }

    #[test]
    fn test_compile_source_with_stdlib() {
        // Source referencing stdlib trait Material — should compile without errors
        // only when stdlib is loaded.
        let source = r#"structure Steel : Material {
            param density: Real = 7850
            param name: String = "Steel"
        }"#;
        let compiled = super::compile_source_with_stdlib(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "compile_source_with_stdlib should compile stdlib-dependent source without errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_compile_first_template() {
        let (template, diagnostics) = super::compile_first_template(bracket_source());
        assert_eq!(template.name, "Bracket", "first template should be Bracket");
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn test_compile_template_by_name() {
        let source = r#"
            structure Alpha { param x: Scalar = 1 }
            structure Beta { param y: Scalar = 2 }
        "#;
        let (template, _diags) = super::compile_template(source, "Beta");
        assert_eq!(template.name, "Beta", "should extract template named Beta");
    }

    #[test]
    #[should_panic(expected = "not found")]
    fn test_compile_template_panics_on_missing_name() {
        super::compile_template(bracket_source(), "NonExistent");
    }

    #[test]
    fn test_collect_errors_filters_correctly() {
        // Source with an undefined reference produces Error diagnostics.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let compiled = super::compile_source(source);
        let errors = super::collect_errors(&compiled.diagnostics);
        assert!(
            !errors.is_empty(),
            "collect_errors should return error diagnostics for invalid source"
        );
        // All returned diagnostics must be Error severity.
        for d in &errors {
            assert_eq!(d.severity, Severity::Error, "collect_errors returned non-Error: {:?}", d);
        }
    }

    #[test]
    fn test_errors_only_convenience() {
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let compiled = super::compile_source(source);
        let errors = super::errors_only(&compiled);
        assert!(
            !errors.is_empty(),
            "errors_only should return error diagnostics for invalid source"
        );
    }

    #[test]
    fn test_warnings_only_filters_correctly() {
        // Use warn_source_with_unknown_port_type which produces warnings.
        let source = crate::fixtures::warn_source_with_unknown_port_type();
        let compiled = super::compile_source(source);
        let warnings = super::warnings_only(&compiled);
        assert!(
            !warnings.is_empty(),
            "warnings_only should return warning diagnostics for warn source"
        );
        for d in &warnings {
            assert_eq!(d.severity, Severity::Warning, "warnings_only returned non-Warning: {:?}", d);
        }
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_make_engine() {
        let compiled = super::parse_and_compile(bracket_source());
        let mut engine = super::make_engine();
        let result = engine.eval(&compiled);
        assert!(
            !result.values.is_empty(),
            "engine.eval should produce non-empty values for bracket source"
        );
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_make_simple_engine() {
        use reify_types::Satisfaction;
        let compiled = super::parse_and_compile(bracket_source());
        let mut engine = super::make_simple_engine();
        let result = engine.check(&compiled);
        assert!(
            !result.constraint_results.is_empty(),
            "engine.check should produce non-empty constraint_results for bracket source"
        );
        for entry in &result.constraint_results {
            assert_eq!(
                entry.satisfaction,
                Satisfaction::Satisfied,
                "constraint {} should be Satisfied under SimpleConstraintChecker, got {:?}",
                entry.id,
                entry.satisfaction
            );
        }
    }

    #[test]
    fn test_parse_compile_expect_err_detects_error() {
        // Source with an undefined reference should produce a compile error.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        // Should not panic — the function expects errors.
        let _compiled = super::parse_compile_expect_err(source, "");
    }

    #[test]
    fn test_parse_compile_expect_err_needle_match() {
        // Source with an undefined reference; needle should match the error message.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let _compiled = super::parse_compile_expect_err(source, "unknown");
    }

    #[test]
    fn test_parse_and_compile_with_stdlib() {
        // Source that references the stdlib trait `Material`.
        // This should compile only when stdlib is loaded.
        let source = r#"structure Steel : Material {
            param density: Real = 7850
            param name: String = "Steel"
        }"#;
        let compiled = super::parse_and_compile_with_stdlib(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);
    }

    #[test]
    fn test_parse_and_compile_valid() {
        let compiled = super::parse_and_compile(bracket_source());
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);
        assert!(
            !compiled.templates.is_empty(),
            "bracket source should produce at least one template"
        );
    }
}
