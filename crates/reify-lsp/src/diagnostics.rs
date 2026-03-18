use reify_constraints::SimpleConstraintChecker;
use reify_types::ModulePath;
use tower_lsp::lsp_types::{self, Url};

use crate::analysis::module_name_from_uri;
use crate::convert;

/// Run the full parse → compile → check pipeline and return LSP diagnostics.
pub fn compute_diagnostics(source: &str, uri: &Url) -> Vec<lsp_types::Diagnostic> {
    let mut result = Vec::new();

    // Derive a module name from the URI
    let module_name = module_name_from_uri(uri);

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));

    // Convert parse errors
    for err in &parsed.errors {
        result.push(convert::convert_parse_error(err, source, uri));
    }

    // Compile
    let compiled = reify_compiler::compile(&parsed);

    // Convert compiler diagnostics
    for diag in &compiled.diagnostics {
        result.push(convert::convert_diagnostic(diag, source, uri));
    }

    // Check (eval with constraint checker, no geometry kernel)
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    // Convert eval diagnostics
    for diag in &check_result.diagnostics {
        result.push(convert::convert_diagnostic(diag, source, uri));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    #[test]
    fn valid_bracket_source_no_errors() {
        let source = reify_test_support::bracket_source();
        let diags = compute_diagnostics(source, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(errors.is_empty(), "valid source should produce no errors, got: {errors:?}");
    }

    #[test]
    fn syntax_error_produces_diagnostic() {
        let source = "structure {";
        let diags = compute_diagnostics(source, &test_uri());
        assert!(!diags.is_empty(), "syntax error should produce diagnostics");
        assert!(
            diags.iter().any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
            "should have at least one error diagnostic"
        );
    }

    #[test]
    fn unknown_identifier_produces_diagnostic() {
        // Reference a non-existent type/name
        let source = "structure Foo {\n  param x: Length = unknown_name;\n}";
        let diags = compute_diagnostics(source, &test_uri());
        assert!(
            !diags.is_empty(),
            "unknown identifier should produce diagnostics"
        );
    }

    #[test]
    fn empty_source_no_crash() {
        let diags = compute_diagnostics("", &test_uri());
        // Should not panic; may or may not produce diagnostics
        let _ = diags;
    }
}
