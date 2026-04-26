use std::collections::HashMap;

use reify_constraints::SimpleConstraintChecker;
use reify_types::{
    ContentHash, ConstraintNodeId, Diagnostic, ModulePath, Satisfaction, SourceSpan, VersionId,
};
use tower_lsp::lsp_types::{self, Url};

use crate::analysis::module_name_from_uri;
use crate::convert;

/// Persistent evaluation state maintained across edits.
///
/// Holds the Engine and last compiled module so the server can incrementally
/// update diagnostics when the source changes.
pub struct EvalState {
    engine: reify_eval::Engine,
    version_counter: u64,
    last_content_hash: Option<ContentHash>,
}

impl EvalState {
    /// Create a new evaluation state with SimpleConstraintChecker and no geometry kernel.
    pub fn new() -> Self {
        let checker = SimpleConstraintChecker;
        Self {
            engine: reify_eval::Engine::new(Box::new(checker), None),
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
/// re-parse, re-compile, then either incremental `eval_cached` (when the
/// content hash is unchanged) or a fresh cold-start `eval` (when the content
/// changed), then `check_snapshot` for constraint results, and convert to
/// LSP diagnostics.
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

    // Use the incremental eval_cached path only when content is unchanged AND
    // the engine has already been initialized by a prior eval(). An uninitialized
    // engine must always take the cold-start branch regardless of last_content_hash:
    // eval_cached returns empty diagnostics by construction, so routing an
    // uninitialized engine through it would silently drop eval-time errors.
    let content_unchanged = state
        .last_content_hash
        .map(|h| h == compiled.content_hash)
        .unwrap_or(false)
        && state.is_engine_initialized();

    // Capture eval-time diagnostics from eval() / eval_cached().
    //
    // Both eval() and eval_cached() now emit the same diagnostic kinds:
    // circular let-binding dependencies, sub-component lookup failures,
    // param_override type/dimension mismatches, solver Infeasible / NoProgress.
    // These are NOT reflected in check_snapshot()'s CheckResult and would be
    // silently dropped without this capture.
    //
    // On the rare check_snapshot → None fallback we drop the captured copy
    // because check() internally re-runs eval() and prepends those diagnostics
    // to CheckResult.diagnostics — keeping them would double-emit.
    let mut eval_diagnostics: Vec<Diagnostic> = if content_unchanged {
        state
            .engine
            .eval_cached(&compiled, VersionId(state.version_counter))
            .eval_result
            .diagnostics
    } else {
        // Observability: if the hash *did* match but the engine was uninitialized,
        // that is the specific invariant violation the engine-init guard (above) was
        // added to catch — last_content_hash was set without a preceding eval().
        // Log a warning in debug builds so the decoupling is not silent. We cannot
        // use debug_assert! here because the graceful-handling test intentionally
        // constructs this state to verify the cold-start branch is taken; the right
        // response is to handle it correctly (which we do below) and warn.
        #[cfg(debug_assertions)]
        if state.last_content_hash == Some(compiled.content_hash)
            && !state.is_engine_initialized()
        {
            eprintln!(
                "[reify-lsp] WARNING: content_hash matched but engine was uninitialized \
                 — last_content_hash was set without a preceding eval(); \
                 cold-start forced to prevent silent diagnostic loss (engine-init guard)"
            );
        }
        let checker = SimpleConstraintChecker;
        state.engine = reify_eval::Engine::new(Box::new(checker), None);
        state.engine.eval(&compiled).diagnostics
    };

    // Check constraints from snapshot, falling back to full check() if snapshot is absent
    let check_result = match state.engine.check_snapshot(&compiled) {
        Some(result) => result,
        None => {
            eprintln!(
                "[reify-lsp] check_snapshot returned None after eval, falling back to full check"
            );
            // check() re-runs eval() internally and includes its diagnostics in
            // CheckResult.diagnostics; drop our independently captured copy to
            // avoid double-emission.
            eval_diagnostics = Vec::new();
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

    // Merge eval-time diagnostics. The invariant "eval() never emits the
    // `constraint <entity>#constraint[<index>] violated` format" lets us merge
    // without a filter. The regression-lock cluster covers every known eval-time
    // emitter:
    //   - circular let-binding (unfold.rs / engine_eval.rs)
    //   - param_override type-kind / dimension mismatch (engine_eval.rs)
    //   - sub-component lookup failure (engine_eval.rs)
    //   - solver Infeasible / NoProgress (engine_eval.rs)
    // Each test asserts the diagnostic shape via `matches_constraint_violation_format`
    // (mirroring `format!("constraint {} violated", entry.id)` over `ConstraintNodeId`'s
    // Display). If any test fails, re-introduce the `violated_messages` filter on the
    // eval merge below or update the merge loop.
    for diag in &eval_diagnostics {
        diagnostics.push(convert::convert_diagnostic(diag, source, uri));
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

    // Record the content hash so the next call can choose incremental vs cold-start.
    state.last_content_hash = Some(compiled.content_hash);

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

    // Additional imports for the eval-diagnostics regression-lock cluster.
    use reify_test_support::MockConstraintSolver;
    use reify_types::{DimensionVector, Value, ValueCellId};

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Returns `true` iff `msg` matches `format!("constraint {} violated", entry.id)` where
    /// `entry.id: ConstraintNodeId` has Display `"<entity>#constraint[<index>]"`.
    ///
    /// Structural requirements:
    /// - Prefix: `"constraint "` (with trailing space)
    /// - Suffix: `" violated"` (with leading space)
    /// - Middle token: `<entity>#constraint[<index>]` where
    ///   - `entity` is non-empty and whitespace-free
    ///   - `index` is a non-empty `u32`-parseable decimal string
    ///
    /// Uses `rsplit_once` on `"#constraint["` so entities that hypothetically
    /// contain that literal (none do in practice) still bind the suffix correctly.
    ///
    /// Shared by the six-test regression-lock cluster in this module.
    fn matches_constraint_violation_format(msg: &str) -> bool {
        let Some(rest) = msg.strip_prefix("constraint ") else {
            return false;
        };
        let Some(id_str) = rest.strip_suffix(" violated") else {
            return false;
        };
        let Some((entity, after)) = id_str.rsplit_once("#constraint[") else {
            return false;
        };
        if entity.is_empty() || entity.chars().any(char::is_whitespace) {
            return false;
        }
        let Some(index_str) = after.strip_suffix(']') else {
            return false;
        };
        !index_str.is_empty() && index_str.parse::<u32>().is_ok()
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

    // --- step-5: cold-start fallback regression lock ---

    #[test]
    fn structural_change_detects_violations_and_updates_content_hash() {
        let uri = test_uri();

        // (1) First call with valid source — no ERROR diagnostics
        let mut state = EvalState::new();
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
        let hash_after_valid = state.last_content_hash();

        // (2) Second call with violating source (different content_hash) — at least one ERROR
        let source_violating = reify_test_support::bracket_source_violating();
        let result2 = compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let errors2: Vec<_> = result2
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();
        assert!(
            !errors2.is_empty(),
            "Phase 2: violating source should produce at least one constraint violation ERROR"
        );

        // (3) The content hash in state must have changed — an LSP-layer invariant.
        //     Whether cold-start or eval_cached was used internally is an engine-level
        //     detail; diagnostic correctness (assertions 1 and 2) is the behavioral
        //     contract. This assertion locks the state-management invariant that
        //     last_content_hash() always reflects the most recently evaluated source.
        assert_ne!(
            hash_after_valid,
            state.last_content_hash(),
            "last_content_hash must update when source changes"
        );
    }

    // --- step-1 (task-2236): eval_diagnostics_surfaced_in_stateful_pipeline ---

    /// Eval-time diagnostics (e.g. circular let-binding) must appear in the LSP result.
    ///
    /// `structure S { let a = b + 1; let b = a + 1 }` has a cyclic let-binding
    /// dependency that is NOT detected at compile time (only geometry-let cycles
    /// are caught in the compiler). The engine catches it inside
    /// `evaluate_let_bindings` (engine_eval.rs:1529) and records it in
    /// `EvalResult::diagnostics` as "circular let-binding dependency in template S: [a, b]".
    #[test]
    fn eval_diagnostics_surfaced_in_stateful_pipeline() {
        let mut state = EvalState::new();
        let uri = test_uri();
        // Cyclic let-bindings: `a` depends on `b` and `b` depends on `a`.
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";
        let result = compute_diagnostics_with_state(&mut state, source, &uri);
        let circular_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    // Matches the exact engine message: "circular let-binding dependency in template S: [a, b]"
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
            .collect();
        assert!(
            !circular_errors.is_empty(),
            "eval-time circular let-binding diagnostic must be surfaced as an LSP ERROR; \
             got diagnostics: {:?}",
            result.diagnostics
        );
    }

    /// Invariant: an uninitialized engine must take the cold-start eval() branch,
    /// even when `last_content_hash` already matches the compiled module's hash.
    ///
    /// This guards against a future decoupling of EvalState's `last_content_hash`
    /// and engine-initialization state: if `last_content_hash` is set without
    /// initializing the engine (e.g. by a new code path), `eval_cached()` would
    /// silently return empty diagnostics — dropping eval-time errors. The
    /// `content_unchanged` predicate must AND in `is_engine_initialized()` to
    /// guarantee the cold-start branch runs whenever the engine is not ready.
    #[test]
    fn cold_start_branch_taken_when_engine_uninitialized_with_matching_hash() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";

        // Pre-compile to obtain the content_hash for this exact source.
        // Must use compile_with_stdlib + ModulePath::single("test") to match
        // what compute_diagnostics_with_state derives from "file:///test.ri".
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        // Inject a matching hash while leaving the engine uninitialized.
        // (private-field write from child mod — same pattern as the
        //  state.version_counter assertion at line 330)
        state.last_content_hash = Some(compiled.content_hash);

        // Sanity: engine must not be initialized — this is the precondition
        // for the bug we are guarding against.
        assert!(
            !state.is_engine_initialized(),
            "engine must be uninitialized after EvalState::new() + hash injection"
        );

        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        // The cold-start eval() branch must run and surface the circular error.
        // On buggy code (before the fix): content_unchanged=true → eval_cached()
        // → empty diagnostics → this assertion fails.
        let circular_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
            .collect();
        assert!(
            !circular_errors.is_empty(),
            "cold-start branch must be taken when engine is uninitialized, \
             surfacing the circular let-binding diagnostic; \
             got diagnostics: {:?}",
            result.diagnostics
        );
    }

    /// Canary: `eval_cached()` currently returns empty diagnostics by construction
    /// (engine_eval.rs:1183 — `let diagnostics = Vec::new()` is never appended to).
    ///
    /// This test asserts *current* behavior so it fails loudly the moment
    /// `eval_cached` starts emitting diagnostics — that failure is the expected
    /// signal to update the assertion (flip `is_empty()` → `!is_empty()`).
    /// An `#[ignore]`'d future-state test would bitrot silently; a canary that
    /// asserts today's behavior forces maintainer attention at the right time.
    #[test]
    fn eval_cached_path_surfaces_circular_let_binding_when_fixed() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";

        // First call: cold-start eval() — must surface the circular let-binding diagnostic.
        let result1 = compute_diagnostics_with_state(&mut state, source, &uri);
        let has_circular_on_cold_start = result1.diagnostics.iter().any(|d| {
            d.severity == Some(DiagnosticSeverity::ERROR)
                && d.message.contains("circular let-binding dependency")
                && d.message.contains("in template S")
        });
        assert!(
            has_circular_on_cold_start,
            "cold-start call must surface circular let-binding diagnostic; got: {:?}",
            result1.diagnostics
        );

        // Second call: same source → content_unchanged=true → eval_cached path.
        // eval_cached() now emits cycle diagnostics (task 2259 fixed the immutable
        // `let diagnostics = Vec::new()` and inserted per-template cycle detection).
        let result2 = compute_diagnostics_with_state(&mut state, source, &uri);
        let circular_on_cached_path: Vec<_> = result2
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
            .collect();
        assert!(
            !circular_on_cached_path.is_empty(),
            "eval_cached() must surface the circular let-binding diagnostic on cached path; \
             got: {:?}",
            result2.diagnostics,
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

    // --- step-5 regression lock: eval diagnostics never use constraint-violation format ---

    /// Pins the contract of the `matches_constraint_violation_format` helper.
    ///
    /// The helper mirrors `format!("constraint {} violated", entry.id)` where
    /// `entry.id: ConstraintNodeId` has Display `<entity>#constraint[<index>]`.
    ///
    /// POSITIVES — formats the production builder can produce — must return `true`.
    /// NEGATIVES — must return `false`, including the previously-loose "no-spaces"
    /// heuristic which would accept "constraint foo violated" (no `#constraint[N]`
    /// shape). Shared by the six-test regression-lock cluster that follows.
    #[test]
    fn matches_constraint_violation_format_helper_is_precise() {
        // POSITIVES: must return true
        assert!(
            matches_constraint_violation_format("constraint S#constraint[0] violated"),
            "canonical single-token entity should match"
        );
        assert!(
            matches_constraint_violation_format("constraint Bracket#constraint[7] violated"),
            "multi-character entity, non-zero index should match"
        );
        assert!(
            matches_constraint_violation_format("constraint S.sub#constraint[2] violated"),
            "dotted entity (sub-component) should match"
        );
        assert!(
            matches_constraint_violation_format("constraint S.sub[0]#constraint[2] violated"),
            "collection-sub entity with bracket suffix should match"
        );

        // NEGATIVES: must return false
        assert!(
            !matches_constraint_violation_format("constraint foo violated"),
            "single-token middle without #constraint[N] shape must not match"
        );
        assert!(
            !matches_constraint_violation_format("constraint #constraint[0] violated"),
            "empty entity must not match"
        );
        assert!(
            !matches_constraint_violation_format("constraint S #constraint[0] violated"),
            "whitespace before marker (entity contains space) must not match"
        );
        assert!(
            !matches_constraint_violation_format("constraint S#constraint[abc] violated"),
            "non-numeric index must not match"
        );
        assert!(
            !matches_constraint_violation_format("constraint S#constraint[] violated"),
            "empty index must not match"
        );
        assert!(
            !matches_constraint_violation_format(
                "constraint inference failed because X was violated"
            ),
            "extra prose between prefix and trailing ' violated' must not match"
        );
        assert!(
            !matches_constraint_violation_format("some random message"),
            "no prefix must not match"
        );
        assert!(
            !matches_constraint_violation_format(""),
            "empty string must not match"
        );
    }

    /// Regression lock — circular let-binding emitter: `eval()` must never emit diagnostics
    /// in the `"constraint <entity>#constraint[<index>] violated"` format.
    ///
    /// This is the first of a six-test cluster (one per known eval-time emitter) that
    /// locks the invariant enabling the no-filter merge of `eval_diagnostics` in
    /// `compute_diagnostics_with_state`. The invariant check uses
    /// `matches_constraint_violation_format` (defined above), which precisely mirrors
    /// `format!("constraint {} violated", entry.id)` over `ConstraintNodeId`'s Display.
    ///
    /// If this test fails, `eval()` has started emitting the constraint-violation format
    /// from the circular-let-binding path (`unfold.rs` / `engine_eval.rs`) — re-introduce
    /// the `violated_messages` filter or update the merge loop.
    #[test]
    fn eval_diagnostics_never_use_constraint_violation_format() {
        // Use circular-let-binding source: a known eval-time diagnostic emitter
        // (engine_eval.rs:1545, 1819 and unfold.rs:518).
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let result = engine.eval(&compiled);

        // Sanity: eval must emit at least one diagnostic for the circular let-binding
        // so the negative assertion below cannot pass vacuously.
        assert!(
            !result.diagnostics.is_empty(),
            "eval() must emit at least one diagnostic for circular-let-binding source; \
             got none — check that the source is still erroneous"
        );

        for diag in &result.diagnostics {
            assert!(
                !matches_constraint_violation_format(&diag.message),
                "eval() emitted a 'constraint <entity>#constraint[<index>] violated' \
                 format message: {:?}. The compute_diagnostics_with_state merge loop \
                 relies on eval diagnostics never using this format — re-introduce the \
                 violated_messages filter or update the merge loop.",
                diag.message
            );
        }
    }

    /// Regression lock — param_override type-kind mismatch emitter: `eval()` must never emit
    /// diagnostics in the `"constraint <entity>#constraint[<index>] violated"` format.
    ///
    /// Locks `engine_eval.rs` lines 282-287 and 619-625 (type-kind mismatch warning path).
    /// If the override validation path is ever changed to emit a constraint-violation format
    /// message, this test will fail and the `violated_messages` filter must be re-introduced.
    #[test]
    fn eval_diagnostics_param_override_type_kind_mismatch_avoids_constraint_violation_format() {
        let source = "structure S { param width: Scalar = 100mm }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        // First eval to register the module.
        let _ = engine.eval(&compiled);

        // Set a Bool override into a Scalar cell — type-kind mismatch.
        let width_id = ValueCellId::new("S", "width");
        engine.set_param_and_invalidate(&width_id, Value::Bool(true));

        // Second eval should emit a type-kind mismatch warning.
        let result = engine.eval(&compiled);

        // Sanity: at least one diagnostic must mention the cell and the mismatch kind.
        assert!(
            result.diagnostics.iter().any(|d| d.message.contains("type-kind mismatch")),
            "expected a 'type-kind mismatch' warning from the param_override path; \
             got: {:?}",
            result.diagnostics
        );

        for diag in &result.diagnostics {
            assert!(
                !matches_constraint_violation_format(&diag.message),
                "eval() emitted a 'constraint <entity>#constraint[<index>] violated' \
                 format message from the param_override type-kind-mismatch path: {:?}. \
                 Re-introduce the violated_messages filter or update the merge loop.",
                diag.message
            );
        }
    }

    /// Regression lock — param_override dimension mismatch emitter: `eval()` must never emit
    /// diagnostics in the `"constraint <entity>#constraint[<index>] violated"` format.
    ///
    /// Locks `engine_eval.rs` lines 289-293 and 627-633 (dimension mismatch warning path).
    /// The override here is `Scalar[MASS]` against a `Scalar[LENGTH]` cell — same type-kind,
    /// mismatched dimension — so the type-kind guard passes and the dimension guard fires.
    #[test]
    fn eval_diagnostics_param_override_dimension_mismatch_avoids_constraint_violation_format() {
        let source = "structure S { param width: Scalar = 100mm }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        // First eval to register the module.
        let _ = engine.eval(&compiled);

        // Set a Scalar[MASS] override against a Scalar[LENGTH] cell — dimension mismatch.
        let width_id = ValueCellId::new("S", "width");
        engine.set_param_and_invalidate(
            &width_id,
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
        );

        // Second eval should emit a dimension mismatch warning.
        let result = engine.eval(&compiled);

        // Sanity: at least one diagnostic must mention the dimension mismatch.
        assert!(
            result.diagnostics.iter().any(|d| d.message.contains("dimension mismatch")),
            "expected a 'dimension mismatch' warning from the param_override path; \
             got: {:?}",
            result.diagnostics
        );

        for diag in &result.diagnostics {
            assert!(
                !matches_constraint_violation_format(&diag.message),
                "eval() emitted a 'constraint <entity>#constraint[<index>] violated' \
                 format message from the param_override dimension-mismatch path: {:?}. \
                 Re-introduce the violated_messages filter or update the merge loop.",
                diag.message
            );
        }
    }

    /// Regression lock — sub-component lookup failure emitter: `eval()` must never emit
    /// diagnostics in the `"constraint <entity>#constraint[<index>] violated"` format.
    ///
    /// Locks `engine_eval.rs` lines 877-880 and 1675-1678.
    /// Source `structure S { sub x = Unknown() }` compiles cleanly (confirmed per
    /// `crates/reify-compiler/tests/recursive_detection_tests.rs:506-521`) but the eval
    /// path emits a runtime "sub-component references unknown structure" diagnostic.
    #[test]
    fn eval_diagnostics_sub_component_unknown_structure_avoids_constraint_violation_format() {
        let source = "structure S { sub x = Unknown() }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let result = engine.eval(&compiled);

        // Sanity: the sub-component lookup failure must emit at least one diagnostic.
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("sub-component")
                    && d.message.contains("references unknown structure")),
            "expected a 'sub-component ... references unknown structure' diagnostic; \
             got: {:?}",
            result.diagnostics
        );

        for diag in &result.diagnostics {
            assert!(
                !matches_constraint_violation_format(&diag.message),
                "eval() emitted a 'constraint <entity>#constraint[<index>] violated' \
                 format message from the sub-component lookup path: {:?}. \
                 Re-introduce the violated_messages filter or update the merge loop.",
                diag.message
            );
        }
    }

    /// Regression lock — solver Infeasible emitter: `eval()` must never emit diagnostics in
    /// the `"constraint <entity>#constraint[<index>] violated"` format.
    ///
    /// Locks `engine_eval.rs` lines 1165-1169 and 1743-1747 (SolveResult::Infeasible arm).
    /// The `MockConstraintSolver::new_infeasible` returns solver diagnostics that are passed
    /// through directly; this test verifies the pass-through never accidentally introduces the
    /// constraint-violation format.
    #[test]
    fn eval_diagnostics_solver_infeasible_avoids_constraint_violation_format() {
        let source = "structure S {\n    param x: Scalar = auto\n    constraint x > 1mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
            "infeasible: x has no satisfying assignment",
        )]);
        let checker = SimpleConstraintChecker;
        let mut engine =
            reify_eval::Engine::new(Box::new(checker), None).with_solver(Box::new(solver));
        let result = engine.eval(&compiled);

        // Sanity: the infeasibility diagnostic must be forwarded.
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("infeasible: x has no satisfying assignment")),
            "expected the Infeasible solver diagnostic to be forwarded; got: {:?}",
            result.diagnostics
        );

        for diag in &result.diagnostics {
            assert!(
                !matches_constraint_violation_format(&diag.message),
                "eval() emitted a 'constraint <entity>#constraint[<index>] violated' \
                 format message from the solver Infeasible path: {:?}. \
                 Re-introduce the violated_messages filter or update the merge loop.",
                diag.message
            );
        }
    }

    /// Regression lock — solver NoProgress emitter: `eval()` must never emit diagnostics in
    /// the `"constraint <entity>#constraint[<index>] violated"` format.
    ///
    /// Locks `engine_eval.rs` lines 1170-1175 and 1748-1751 (SolveResult::NoProgress arm).
    /// The emitted message is `"Constraint solver made no progress: {reason}"`.
    #[test]
    fn eval_diagnostics_solver_no_progress_avoids_constraint_violation_format() {
        let source = "structure S {\n    param x: Scalar = auto\n    constraint x > 1mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let solver = MockConstraintSolver::new_no_progress("iteration limit reached");
        let checker = SimpleConstraintChecker;
        let mut engine =
            reify_eval::Engine::new(Box::new(checker), None).with_solver(Box::new(solver));
        let result = engine.eval(&compiled);

        // Sanity: the NoProgress warning must be forwarded.
        assert!(
            result.diagnostics.iter().any(|d| d.message.contains("made no progress")),
            "expected a 'Constraint solver made no progress' diagnostic; got: {:?}",
            result.diagnostics
        );

        for diag in &result.diagnostics {
            assert!(
                !matches_constraint_violation_format(&diag.message),
                "eval() emitted a 'constraint <entity>#constraint[<index>] violated' \
                 format message from the solver NoProgress path: {:?}. \
                 Re-introduce the violated_messages filter or update the merge loop.",
                diag.message
            );
        }
    }
}
