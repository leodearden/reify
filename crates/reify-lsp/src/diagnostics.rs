use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction};
use tower_lsp::lsp_types::{self, Url};

use crate::analysis::module_name_from_uri;
use crate::convert;

/// Persistent evaluation state maintained across edits.
///
/// Holds the Engine and last compiled module so the server can incrementally
/// update diagnostics when the source changes.
pub struct EvalState {
    engine: reify_eval::Engine,
    last_module: Option<CompiledModule>,
    version_counter: u64,
}

impl EvalState {
    /// Create a new evaluation state with SimpleConstraintChecker and no geometry kernel.
    pub fn new() -> Self {
        let checker = SimpleConstraintChecker;
        Self {
            engine: reify_eval::Engine::new(Box::new(checker), None),
            last_module: None,
            version_counter: 0,
        }
    }
}

impl Default for EvalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Result from the stateful diagnostics pipeline.
pub struct DiagnosticsResult {
    /// LSP diagnostics to publish.
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    /// Exported geometry data (if geometry kernel is configured).
    pub geometry_output: Option<Vec<u8>>,
}

/// Run the stateful parse → compile → eval → check pipeline.
///
/// Maintains a persistent Engine in EvalState across calls. On each call:
/// re-parse, re-compile, cold-start eval (source text change invalidates all state),
/// then check_snapshot for constraint results, and convert to LSP diagnostics.
pub fn compute_diagnostics_with_state(
    state: &mut EvalState,
    source: &str,
    uri: &Url,
) -> DiagnosticsResult {
    let mut diagnostics = Vec::new();

    // Derive module name from URI
    let module_name = uri
        .path_segments()
        .and_then(|mut segs| segs.next_back())
        .and_then(|name| name.strip_suffix(".ri"))
        .unwrap_or("unnamed");

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));
    for err in &parsed.errors {
        diagnostics.push(convert::convert_parse_error(err, source, uri));
    }

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    for diag in &compiled.diagnostics {
        diagnostics.push(convert::convert_diagnostic(diag, source, uri));
    }

    // Cold-start eval (source text change invalidates all cached state)
    state.version_counter += 1;

    // Create a fresh Engine for each source change to ensure clean state.
    // True source-level incrementality (diffing compiled modules) is future work.
    let checker = SimpleConstraintChecker;
    state.engine = reify_eval::Engine::new(Box::new(checker), None);

    let _eval_result = state.engine.eval(&compiled);

    // Check constraints from snapshot
    if let Some(check_result) = state.engine.check_snapshot(&compiled) {
        // Convert eval/constraint diagnostics
        for diag in &check_result.diagnostics {
            diagnostics.push(convert::convert_diagnostic(diag, source, uri));
        }

        // Generate explicit diagnostics for constraint violations
        for entry in &check_result.constraint_results {
            if entry.satisfaction == Satisfaction::Violated {
                let msg = match &entry.label {
                    Some(label) => format!("constraint violated: {}", label),
                    None => format!("constraint {} violated", entry.id),
                };
                diagnostics.push(lsp_types::Diagnostic {
                    range: lsp_types::Range::default(),
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    source: Some("reify".to_string()),
                    message: msg,
                    ..Default::default()
                });
            }
        }
    }

    // Store compiled module for potential future use
    state.last_module = Some(compiled);

    DiagnosticsResult {
        diagnostics,
        geometry_output: None,
    }
}

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

    // --- compute_diagnostics_with_state unit tests (step-25) ---

    #[test]
    fn eval_state_new_starts_with_version_counter_zero() {
        let state = EvalState::new();
        assert_eq!(state.version_counter, 0);
    }

    #[test]
    fn stateful_diagnostics_three_phase_lifecycle() {
        let mut state = EvalState::new();
        let uri = test_uri();

        // Phase 1: valid source — no ERROR diagnostics
        let source_valid = reify_test_support::bracket_source();
        let result1 = compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let errors1: Vec<_> = result1
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors1.is_empty(),
            "Phase 1: valid source should produce no errors, got: {errors1:?}"
        );

        // Phase 2: violating source — at least one constraint violation ERROR
        let source_violating = reify_test_support::bracket_source_violating();
        let result2 = compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let errors2: Vec<_> = result2
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            !errors2.is_empty(),
            "Phase 2: violating source should produce at least one ERROR"
        );
        let has_constraint_violation = errors2.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("constraint") && msg.contains("violated")
        });
        assert!(
            has_constraint_violation,
            "Phase 2: should have a 'constraint violated' diagnostic, got: {errors2:?}"
        );

        // Phase 3: back to valid source — violations should clear
        let result3 = compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let errors3: Vec<_> = result3
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors3.is_empty(),
            "Phase 3: valid source should clear violations, got: {errors3:?}"
        );

        // Verify version_counter persistence: 3 calls = version_counter 3
        assert_eq!(
            state.version_counter, 3,
            "version_counter should be 3 after three calls"
        );
    }
}
