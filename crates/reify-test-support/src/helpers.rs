//! Pipeline helpers for parsing, compiling, and evaluating Reify source in tests.

use reify_types::{ModulePath, Severity};

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
