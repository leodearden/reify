use std::collections::HashMap;

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_types::{ContentHash, ConstraintNodeId, ModulePath, Satisfaction, SourceSpan, VersionId};
use tower_lsp::lsp_types::{self, Url};

use crate::analysis::module_name_from_uri;
use crate::convert;

/// Persistent evaluation state maintained across edits.
///
/// Holds the Engine and last compiled module so the server can incrementally
/// update diagnostics when the source changes.
pub struct EvalState {
    pub(crate) engine: reify_eval::Engine,
    last_module: Option<CompiledModule>,
    version_counter: u64,
    last_content_hash: Option<ContentHash>,
}

impl EvalState {
    /// Create a new evaluation state with SimpleConstraintChecker and no geometry kernel.
    pub fn new() -> Self {
        let checker = SimpleConstraintChecker;
        Self {
            engine: reify_eval::Engine::new(Box::new(checker), None),
            last_module: None,
            version_counter: 0,
            last_content_hash: None,
        }
    }

    /// Returns the content hash of the last successfully compiled module, if any.
    pub fn last_content_hash(&self) -> Option<ContentHash> {
        self.last_content_hash
    }

    /// Returns true if the engine has been initialized by a prior `eval()` or `eval_cached()` call.
    pub fn is_engine_initialized(&self) -> bool {
        self.engine.is_initialized()
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
    // Early-return on parse errors: the partial AST fed to compile/eval produces
    // misleading secondary diagnostics. Match the behaviour of
    // Engine::load_from_source's early-return on parse errors.
    if !parsed.errors.is_empty() {
        return DiagnosticsResult {
            diagnostics,
            geometry_output: None,
        };
    }

    // Compile
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    for diag in &compiled.diagnostics {
        diagnostics.push(convert::convert_diagnostic(diag, source, uri));
    }

    // Eval: use incremental eval_cached when structure unchanged, else cold-start.
    state.version_counter += 1;

    let structure_unchanged = state
        .last_content_hash
        .map(|h| h == compiled.content_hash)
        .unwrap_or(false);

    if structure_unchanged && state.engine.is_initialized() {
        let _ = state
            .engine
            .eval_cached(&compiled, VersionId(state.version_counter));
    } else {
        let checker = SimpleConstraintChecker;
        state.engine = reify_eval::Engine::new(Box::new(checker), None);
        let _ = state.engine.eval(&compiled);
    }

    // Check constraints from snapshot, falling back to full check() if snapshot is absent
    let check_result = match state.engine.check_snapshot(&compiled) {
        Some(result) => result,
        None => {
            eprintln!(
                "[reify-lsp] check_snapshot returned None after eval, falling back to full check"
            );
            state.engine.check(&compiled)
        }
    };

    // Build constraint span lookup map from compiled module
    let constraint_spans: HashMap<ConstraintNodeId, SourceSpan> = compiled
        .templates
        .iter()
        .flat_map(|t| t.constraints.iter())
        .map(|c| (c.id.clone(), c.span))
        .collect();

    // Convert non-constraint eval diagnostics from check result.
    // Skip constraint checker messages (format: "constraint {id} violated")
    // since we generate span-aware versions below.
    let violated_messages: std::collections::HashSet<String> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .map(|e| format!("constraint {} violated", e.id))
        .collect();
    for diag in &check_result.diagnostics {
        if !violated_messages.contains(&diag.message) {
            diagnostics.push(convert::convert_diagnostic(diag, source, uri));
        }
    }

    // Generate explicit diagnostics for constraint violations with source spans
    for entry in &check_result.constraint_results {
        if entry.satisfaction == Satisfaction::Violated {
            let msg = match &entry.label {
                Some(label) => format!("constraint violated: {}", label),
                None => format!("constraint {} violated", entry.id),
            };
            let span_lookup = constraint_spans.get(&entry.id);
            let range = span_lookup
                .map(|span| convert::span_to_range(source, *span))
                .unwrap_or_default();
            diagnostics.push(lsp_types::Diagnostic {
                range,
                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                source: Some("reify".to_string()),
                message: msg,
                ..Default::default()
            });
        }
    }

    // Store compiled module and content hash for incremental eval on next call
    state.last_content_hash = Some(compiled.content_hash);
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
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

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

    /// Minimal source that references two stdlib symbols (Rigid trait, Material struct).
    /// Shared across all task-2176 stdlib-resolution tests to avoid tripling the literal.
    const STDLIB_PROBE_SRC: &str = r#"structure S : Rigid {
    param density: Real = 7850
    param name: String = "steel"
    param volume: Real = 1.0
    param centroid_x: Real = 0.0
    param centroid_y: Real = 0.0
    param centroid_z: Real = 0.0
    param moment_of_inertia: Real = 1.0
    param material: Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)
}"#;

    #[test]
    fn valid_bracket_source_no_errors() {
        let source = reify_test_support::bracket_source();
        let diags = compute_diagnostics(source, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "valid source should produce no errors, got: {errors:?}"
        );
    }

    #[test]
    fn syntax_error_produces_diagnostic() {
        let source = "structure {";
        let diags = compute_diagnostics(source, &test_uri());
        assert!(!diags.is_empty(), "syntax error should produce diagnostics");
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
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

    // --- check_snapshot fallback robustness tests (step-27) ---

    #[test]
    fn fresh_engine_check_snapshot_returns_none() {
        // A fresh Engine (without prior eval) should have no snapshot
        let checker = SimpleConstraintChecker;
        let engine = reify_eval::Engine::new(Box::new(checker), None);
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        let compiled = reify_compiler::compile(&parsed);
        let result = engine.check_snapshot(&compiled);
        assert!(
            result.is_none(),
            "fresh Engine without eval should return None from check_snapshot"
        );
    }

    #[test]
    fn stateful_violating_source_always_produces_constraint_violation() {
        // Regression guard: constraint violations must never be silently skipped
        let mut state = EvalState::new();
        let uri = test_uri();
        let source_violating = reify_test_support::bracket_source_violating();
        let result = compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let constraint_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();
        assert!(
            !constraint_errors.is_empty(),
            "violating source must always produce at least one constraint violation ERROR, got diagnostics: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn stateful_empty_source_does_not_panic() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let result = compute_diagnostics_with_state(&mut state, "", &uri);
        // Should not panic; result may contain parse errors but should be valid
        let _ = result;
    }

    // --- parse error early return tests (step-6 / Task 497) ---

    /// When there are parse errors, compile/eval may produce misleading secondary
    /// diagnostics on a broken AST. After the early return added in step-7, the
    /// result should contain exactly the parse error diagnostics — no more.
    #[test]
    fn parse_error_skips_compile_and_eval() {
        let source = "structure {";
        let uri = test_uri();

        // Count parse errors directly using reify_syntax
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let parse_error_count = parsed.errors.len();
        assert!(
            parse_error_count > 0,
            "test source must produce at least one parse error"
        );

        // compute_diagnostics_with_state should return only parse-error diagnostics
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, source, &uri);
        assert_eq!(
            result.diagnostics.len(),
            parse_error_count,
            "on parse error, diagnostics count ({}) should equal parse error count ({}); \
             secondary compile/eval diagnostics must not be included",
            result.diagnostics.len(),
            parse_error_count
        );
        assert!(
            result.geometry_output.is_none(),
            "geometry_output should be None when parse errors exist (eval must be skipped)"
        );
        for diag in &result.diagnostics {
            assert_eq!(
                diag.severity,
                Some(DiagnosticSeverity::ERROR),
                "all parse-error diagnostics must have severity ERROR, got: {:?}",
                diag.severity
            );
        }
    }

    // --- task-2176 step-5: stateful diagnostics resolve stdlib types ---

    #[test]
    fn stateful_diagnostics_resolve_stdlib_material_and_rigid() {
        // Drives the stateful compute_diagnostics_with_state() path.
        // A known-good stdlib source must produce zero error-severity diagnostics.
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, STDLIB_PROBE_SRC, &test_uri());
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "stateful pipeline: stdlib source should compile without errors; got: {errors:?}"
        );
    }

    // --- task-2176 step-3: stateless diagnostics resolve stdlib types ---

    #[test]
    fn compute_diagnostics_resolves_stdlib_material_and_rigid() {
        // Drives the stateless compute_diagnostics() path.
        // A known-good stdlib source must produce zero error-severity diagnostics.
        let diags = compute_diagnostics(STDLIB_PROBE_SRC, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib source should compile without errors; got: {errors:?}"
        );
    }

    // --- step-3: eval_cached path via basis_version ---

    #[test]
    fn incremental_path_uses_eval_cached_when_content_unchanged() {
        use reify_eval::cache::NodeId;
        use reify_types::ValueCellId;

        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        // (1) First call: cold-start
        let mut state = EvalState::new();
        compute_diagnostics_with_state(&mut state, source, &uri);
        assert_eq!(state.version_counter, 1, "version_counter should be 1 after first call");

        // (2) Second call with identical source: should use eval_cached path
        compute_diagnostics_with_state(&mut state, source, &uri);
        assert_eq!(state.version_counter, 2, "version_counter should be 2 after second call");

        // (3) Inspect cache: basis_version of Bracket.width should be > 0
        //     eval_cached passes VersionId(state.version_counter) which is VersionId(2) at call time
        //     (counter is incremented to 2 before eval_cached is called).
        //     A cold-start would reset the engine to a fresh state with basis_version=0.
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        let entry = state
            .engine
            .cache_store()
            .get(&node)
            .expect("Bracket.width cache entry must exist after eval");
        assert!(
            entry.basis_version.0 > 0,
            "eval_cached path should bump basis_version > 0; cold-start path would reset to 0, got {}",
            entry.basis_version.0
        );
    }

    // --- step-1: content-hash tracking across calls ---

    #[test]
    fn eval_state_tracks_content_hash_across_calls() {
        let uri = test_uri();

        // (1) Fresh state: last_content_hash() == None, is_engine_initialized() == false
        let mut state = EvalState::new();
        assert_eq!(
            state.last_content_hash(),
            None,
            "fresh EvalState must have no last_content_hash"
        );
        assert!(
            !state.is_engine_initialized(),
            "fresh EvalState engine must not be initialized"
        );

        // (2) After one call: last_content_hash() is Some(_), is_engine_initialized() is true
        let source_valid = reify_test_support::bracket_source();
        compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let hash1 = state
            .last_content_hash()
            .expect("last_content_hash must be Some after first call");
        assert!(
            state.is_engine_initialized(),
            "engine must be initialized after first call"
        );

        // (3) Calling again with the SAME source: hash unchanged
        compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let hash2 = state
            .last_content_hash()
            .expect("last_content_hash must be Some after second call");
        assert_eq!(
            hash1, hash2,
            "same source must produce the same content_hash"
        );

        // (4) Calling with violating source: hash differs
        let source_violating = reify_test_support::bracket_source_violating();
        compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let hash3 = state
            .last_content_hash()
            .expect("last_content_hash must be Some after violating call");
        assert_ne!(
            hash1, hash3,
            "violating source must produce a different content_hash"
        );
    }

    // --- constraint violation diagnostic range tests (step-31) ---

    #[test]
    fn constraint_violation_diagnostic_has_correct_range() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source_violating = reify_test_support::bracket_source_violating();
        let result = compute_diagnostics_with_state(&mut state, &source_violating, &uri);

        // Find the constraint violation ERROR diagnostic
        let violation_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();

        assert!(
            !violation_diags.is_empty(),
            "should have at least one constraint violation diagnostic"
        );

        for diag in &violation_diags {
            // Constraints are on lines 7-9 of bracket source (0-indexed), not line 0
            assert!(
                diag.range.start.line > 0,
                "constraint violation range should not be on line 0, got range: {:?}",
                diag.range
            );
            assert_ne!(
                diag.range,
                lsp_types::Range::default(),
                "constraint violation range should not be Range::default() (0,0)→(0,0)"
            );
        }
    }
}
