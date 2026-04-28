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

    // Parse (prelude-aware so stdlib enum references like `CorrosionClass.C5`
    // disambiguate to `EnumAccess` rather than `MemberAccess`; pairs with
    // `compile_with_stdlib` below). See task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
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

    // Merge eval-time diagnostics. The `eval_diagnostics_never_use_constraint_violation_format`
    // and `eval_diag_format_*` tests enforce the invariant that eval() never emits the
    // `constraint <entity>#constraint[<index>] violated` format, covering every known
    // eval-time emitter:
    //   - circular let-binding (unfold.rs / engine_eval.rs)
    //   - param_override type-kind / dimension mismatch (engine_eval.rs)
    //   - sub-component lookup failure (engine_eval.rs)
    //   - solver Infeasible / NoProgress (engine_eval.rs)
    // No filter is applied here: if the invariant ever breaks, the cluster fails
    // loudly in CI and a maintainer must add a filter or update the merge loop.
    // Keeping a silent defensive filter would hide the very regression the cluster
    // is designed to detect.
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

    // Parse (prelude-aware so stdlib enum references disambiguate correctly;
    // pairs with `compile_with_stdlib` below). See task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));

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
    use reify_types::{DimensionVector, Severity, Value, ValueCellId};
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

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

    /// Regression guard for task 2525: `compute_diagnostics` must accept sources
    /// that reference stdlib enums (e.g. `CorrosionClass.C5`) WITHOUT inline
    /// redeclarations.
    ///
    /// Pre-task, the parser disambiguated `Type.Variant` against the current
    /// source's enum decls only, so the lowered AST carried `MemberAccess`
    /// instead of `EnumAccess` and the downstream `compile_with_stdlib` emitted
    /// an unresolved-name error diagnostic for `CorrosionClass`.
    #[test]
    fn compute_diagnostics_resolves_stdlib_enum_access_without_inline_redecl() {
        let source = "structure Sample {\n  let chosen_class = CorrosionClass.C5\n}\n";
        let diags = compute_diagnostics(source, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib enum reference without inline redecl should produce no error diagnostics, got: {errors:?}"
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

    /// End-to-end regression lock: a deep dot-chain in a `let` value flows
    /// through `compute_diagnostics` as an LSP Warning whose source is `reify`,
    /// whose message contains the rendered chain text, and whose range is
    /// non-zero (anchored to the chain's source span via the diagnostic label).
    ///
    /// This pins the LSP-side surface for spec §5.7's `DeepDotChain` lint.
    /// Conversion of the typed `DiagnosticCode::DeepDotChain` to the LSP
    /// `code` field is intentionally out-of-scope here — see plan.json
    /// design decision "Do NOT modify convert_diagnostic to populate
    /// lsp_types::Diagnostic.code" — so we assert on severity, source,
    /// message text, and a non-zero range only.
    #[test]
    fn lsp_compute_diagnostics_surfaces_deep_dot_chain_warning() {
        // 6-segment chain `a.b.c.d.e.f` (length 6 > 4) inside a `let` value
        // forces exactly one DeepDotChain warning from the compiler pre-pass.
        let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e.f
}
"#;
        let diags = compute_diagnostics(source, &test_uri());

        let deep_dot_chain_diags: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::WARNING)
                    && d.message.contains("a.b.c.d.e.f")
            })
            .collect();

        assert_eq!(
            deep_dot_chain_diags.len(),
            1,
            "expected exactly 1 LSP Warning with chain text `a.b.c.d.e.f`, got {}: {:#?}",
            deep_dot_chain_diags.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let diag = deep_dot_chain_diags[0];
        assert_eq!(
            diag.severity,
            Some(DiagnosticSeverity::WARNING),
            "expected WARNING severity, got {:?}",
            diag.severity
        );
        assert_eq!(
            diag.source.as_deref(),
            Some("reify"),
            "expected source `reify`, got {:?}",
            diag.source
        );
        // Range must be non-zero — the diagnostic carries a label whose span
        // covers the entire `a.b.c.d.e.f` chain. A zero range would mean the
        // label was dropped and convert_diagnostic fell back to (0,0)-(0,0).
        let range_is_zero = diag.range.start == diag.range.end;
        assert!(
            !range_is_zero,
            "expected non-zero diagnostic range (label span should anchor to \
             chain), got start={:?} end={:?}",
            diag.range.start, diag.range.end
        );
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

    /// Regression lock — circular let-binding emitter: `eval()` must never emit diagnostics
    /// in the `"constraint ... violated"` format checked by the inline `strip_prefix /
    /// strip_suffix / !contains(' ')` filter used throughout this cluster.
    ///
    /// This is the first of a six-test cluster (one per known eval-time emitter) that
    /// locks the invariant enabling the unfiltered merge of `eval_diagnostics` in
    /// `compute_diagnostics_with_state`.
    ///
    /// If this test fails, `eval()` has started emitting the constraint-violation format
    /// from the circular-let-binding path (`unfold.rs` / `engine_eval.rs`) — add a
    /// filter on the eval merge in `compute_diagnostics_with_state` or update the merge loop.
    #[test]
    fn eval_diagnostics_never_use_constraint_violation_format() {
        // Use circular-let-binding source: a known eval-time diagnostic emitter
        // (the unfold.rs / engine_eval.rs circular let-binding paths).
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
            let is_violation_format = diag
                .message
                .strip_prefix("constraint ")
                .and_then(|s| s.strip_suffix(" violated"))
                .is_some_and(|id| !id.is_empty() && !id.contains(' '));
            assert!(
                !is_violation_format,
                "eval() emitted a 'constraint ... violated' format message: {:?}. \
                 The compute_diagnostics_with_state merge loop relies on eval diagnostics \
                 never using this format — add a filter on the eval merge or update the loop.",
                diag.message
            );
        }
    }

    /// Negative-assertion helper: asserts that none of `diags` match the inline
    /// `strip_prefix("constraint ") / strip_suffix(" violated") / !contains(' ')` format
    /// that `compute_diagnostics_with_state` relies on never appearing in eval output.
    /// `label` is embedded in the panic message to identify which emitter path failed.
    fn assert_no_violation_format(diags: &[Diagnostic], label: &str) {
        for diag in diags {
            let is_violation_format = diag
                .message
                .strip_prefix("constraint ")
                .and_then(|s| s.strip_suffix(" violated"))
                .is_some_and(|id| !id.is_empty() && !id.contains(' '));
            assert!(
                !is_violation_format,
                "[{label}] eval() emitted a 'constraint ... violated' format message: {:?}. \
                 The compute_diagnostics_with_state merge loop relies on eval diagnostics \
                 never using this format — add a filter on the eval merge or update the loop.",
                diag.message
            );
        }
    }

    /// Shared setup for the two param-override emitter tests.
    ///
    /// Parses and compiles `"structure S { param width: Scalar = 100mm }"`, does an initial
    /// eval to warm the engine state, then overrides `width` with `override_value` and returns
    /// the diagnostics from the second eval.
    fn build_param_override_diags(override_value: Value) -> Vec<Diagnostic> {
        let source = "structure S { param width: Scalar = 100mm }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
        let _ = engine.eval(&compiled);
        engine.set_param_and_invalidate(&ValueCellId::new("S", "width"), override_value);
        engine.eval(&compiled).diagnostics
    }

    /// Shared setup for the two solver pass-through emitter tests.
    ///
    /// Parses and compiles the `"auto" + constraint-on-x` source and installs `solver`.
    /// Returns `(counter, diagnostics)` where `counter` is the live `Arc<AtomicUsize>` from
    /// `solver.counter_handle()`, allowing callers to assert the solver was dispatched.
    fn run_solver_on_constrained_auto_param(
        solver: MockConstraintSolver,
    ) -> (Arc<AtomicUsize>, Vec<Diagnostic>) {
        let source = "structure S {\n    param x: Scalar = auto\n    constraint x > 1mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);
        let counter = solver.counter_handle();
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None)
            .with_solver(Box::new(solver));
        let diagnostics = engine.eval(&compiled).diagnostics;
        (counter, diagnostics)
    }

    /// Locks the `ConstraintNodeId` Display invariant independently of any emitter.
    ///
    /// The production format `format!("constraint {} violated", ConstraintNodeId::new("S", 0))`
    /// must satisfy the inline `strip_prefix / strip_suffix / !contains(' ')` check so that
    /// drift in `ConstraintNodeId::Display` trips this test before the negative checks in the
    /// per-emitter cluster.
    #[test]
    fn eval_diag_format_anchor() {
        let real_id = ConstraintNodeId::new("S", 0u32);
        let anchor = format!("constraint {} violated", real_id);
        assert!(
            anchor
                .strip_prefix("constraint ")
                .and_then(|s| s.strip_suffix(" violated"))
                .is_some_and(|id| !id.is_empty() && !id.contains(' ')),
            "anchor: ConstraintNodeId::new(\"S\", 0) formats as {real_id:?} which does not \
             match the inline constraint-violation check; if ConstraintNodeId Display changed, \
             update the inline check in this cluster and in \
             eval_diagnostics_never_use_constraint_violation_format."
        );
    }

    /// Per-emitter regression lock — param_override type-kind mismatch path
    /// (engine_eval.rs param_override type-kind path).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the param_override type-kind mismatch emitter.
    /// Counter contract for this emitter lives in `crates/reify-eval/tests/eval_instrumentation_counters.rs`.
    #[test]
    fn eval_diag_format_param_override_type_kind() {
        let diags = build_param_override_diags(Value::Bool(true));

        assert!(
            !diags.is_empty(),
            "param_override_type_kind: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "param_override_type_kind: expected at least one Warning-severity diagnostic; \
             got: {:#?}",
            diags
        );
        // Substring discriminator: ensures the right emitter fired (the counter contract is
        // anchored at `crates/reify-eval/tests/eval_instrumentation_counters.rs`).
        assert!(
            diags.iter().any(|d| d.message.contains("type-kind mismatch")),
            "param_override_type_kind: expected a diagnostic containing 'type-kind mismatch'; \
             got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "param_override_type_kind");
    }

    /// Per-emitter regression lock — param_override dimension mismatch path
    /// (engine_eval.rs param_override dimension path).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the param_override dimension mismatch emitter.
    /// Counter contract for this emitter lives in `crates/reify-eval/tests/eval_instrumentation_counters.rs`.
    #[test]
    fn eval_diag_format_param_override_dimension() {
        let diags = build_param_override_diags(Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        });

        assert!(
            !diags.is_empty(),
            "param_override_dimension: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "param_override_dimension: expected at least one Warning-severity diagnostic; \
             got: {:#?}",
            diags
        );
        // Substring discriminator: ensures the right emitter fired (the counter contract is
        // anchored at `crates/reify-eval/tests/eval_instrumentation_counters.rs`).
        assert!(
            diags.iter().any(|d| d.message.contains("dimension mismatch")),
            "param_override_dimension: expected a diagnostic containing 'dimension mismatch'; \
             got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "param_override_dimension");
    }

    /// Per-emitter regression lock — sub-component lookup failure path
    /// (engine_eval.rs sub-component lookup).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the sub-component unknown-structure emitter.
    /// Counter contract for this emitter lives in `crates/reify-eval/tests/eval_instrumentation_counters.rs`.
    #[test]
    fn eval_diag_format_sub_component_unknown() {
        let source = "structure S { sub x = Unknown() }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
        let diags = engine.eval(&compiled).diagnostics;

        assert!(
            !diags.is_empty(),
            "sub_component_unknown: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error),
            "sub_component_unknown: expected at least one Error-severity diagnostic; \
             got: {:#?}",
            diags
        );
        // Substring discriminator: ensures the right emitter fired (the counter contract is
        // anchored at `crates/reify-eval/tests/eval_instrumentation_counters.rs`).
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("sub-component") && d.message.contains("references unknown structure")),
            "sub_component_unknown: expected a diagnostic containing both 'sub-component' and \
             'references unknown structure'; got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "sub_component_unknown");
    }

    /// Per-emitter regression lock — solver Infeasible pass-through path
    /// (engine_eval.rs solver Infeasible pass-through).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the solver Infeasible emitter. Also verifies via `MockConstraintSolver::counter_handle()`
    /// that the injected solver was actually dispatched.
    #[test]
    fn eval_diag_format_solver_infeasible() {
        let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
            "infeasible: x has no satisfying assignment",
        )]);
        let (counter, diags) = run_solver_on_constrained_auto_param(solver);

        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "solver_infeasible: MockConstraintSolver.solve() was never called; \
             the 'auto' param + constraint source may not trigger solver dispatch"
        );
        assert!(
            !diags.is_empty(),
            "solver_infeasible: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error),
            "solver_infeasible: expected at least one Error-severity diagnostic; got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "solver_infeasible");
    }

    /// Per-emitter regression lock — solver NoProgress pass-through path
    /// (engine_eval.rs solver NoProgress pass-through).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the solver NoProgress emitter. Also verifies via `MockConstraintSolver::counter_handle()`
    /// that the injected solver was actually dispatched.
    #[test]
    fn eval_diag_format_solver_no_progress() {
        let solver = MockConstraintSolver::new_no_progress("iteration limit reached");
        let (counter, diags) = run_solver_on_constrained_auto_param(solver);

        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "solver_no_progress: MockConstraintSolver.solve() was never called; \
             the 'auto' param + constraint source may not trigger solver dispatch"
        );
        assert!(
            !diags.is_empty(),
            "solver_no_progress: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "solver_no_progress: expected at least one Warning-severity diagnostic; got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "solver_no_progress");
    }

    // --- task #2337 step-17: freshness diagnostic tests ---

    /// Helper: build an EvalState whose engine has been pre-evaluated with
    /// bracket_source and a forced panic on `cell_id`.  The engine has gone
    /// through two full `eval()` passes:
    ///   1. Cold eval (all cells → Final).
    ///   2. Hot eval with forced panic on `cell_id` (cell → Failed; cells
    ///      that depend on it → Pending via §9.2 propagation).
    ///
    /// The returned EvalState has `last_content_hash` and `version_counter`
    /// pre-set so that the NEXT call to `compute_diagnostics_with_state` with
    /// the same bracket_source takes the **eval_cached** path (not cold-start).
    /// This avoids the cold-start branch recreating the engine (which would
    /// discard the freshness state we just set up).
    ///
    /// `test-instrumentation` feature is enabled in dev-deps (Cargo.toml line 29).
    #[cfg(any(test, feature = "test-instrumentation"))]
    fn build_eval_state_with_failed_cell(cell_id: ValueCellId) -> EvalState {
        let source = reify_test_support::bracket_source();
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);

        // Pass 1: cold eval — initialises the cache (all cells → Final).
        let _ = engine.eval(&compiled);

        // Inject forced panic and run a second full eval.
        engine.set_panic_on_eval(cell_id);
        let _ = engine.eval(&compiled);
        // After pass 2: `cell_id` is Failed; its dependents are Pending.

        // Package into EvalState with matching hash so next call uses eval_cached.
        let mut state = EvalState::new();
        state.engine = engine;
        state.last_content_hash = Some(compiled.content_hash);
        state.version_counter = 2;
        state
    }

    /// (a) `compute_diagnostics_with_state` must emit exactly one ERROR diagnostic
    /// with `code == "computation-failed"` for a cell whose freshness is Failed
    /// (forced-panic via test-instrumentation).
    ///
    /// This test is intentionally RED before step-18 adds the freshness-diagnostic
    /// emission block to `compute_diagnostics_with_state`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    #[test]
    fn compute_diagnostics_with_state_emits_failed_diagnostic_for_failed_cell() {
        // Force `Bracket.volume` (the only `let` in bracket_source) to fail.
        let volume_id = ValueCellId::new("Bracket", "volume");
        let mut state = build_eval_state_with_failed_cell(volume_id);

        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        // eval_cached path: content unchanged, engine initialized.
        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let failed_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-failed".to_string(),
                    ))
            })
            .collect();

        assert_eq!(
            failed_diags.len(),
            1,
            "expected exactly 1 'computation-failed' ERROR diagnostic for Bracket.volume, \
             got {}: {:#?}",
            failed_diags.len(),
            result.diagnostics
        );
        assert_eq!(
            failed_diags[0].severity,
            Some(DiagnosticSeverity::ERROR),
            "computation-failed diagnostic must have ERROR severity, got {:?}",
            failed_diags[0].severity
        );
    }

    /// (b) `compute_diagnostics_with_state` must emit at least one WARNING diagnostic
    /// with `code == "computation-pending"` for a cell that is Pending because its
    /// upstream dependency failed (Failed leaf → Pending consumer, arch §9.2).
    ///
    /// Setup: force `Bracket.width` (a param) to fail → `Bracket.volume` becomes
    /// Pending because `volume = width * height * thickness` depends on `width`.
    ///
    /// This test is intentionally RED before step-18.
    #[cfg(any(test, feature = "test-instrumentation"))]
    #[test]
    fn compute_diagnostics_with_state_emits_pending_diagnostic_for_pending_cell() {
        // Forcing the `width` param to fail makes `volume` Pending (arch §9.2).
        let width_id = ValueCellId::new("Bracket", "width");
        let mut state = build_eval_state_with_failed_cell(width_id);

        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let pending_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-pending".to_string(),
                    ))
            })
            .collect();

        assert!(
            !pending_diags.is_empty(),
            "expected at least one 'computation-pending' WARNING diagnostic \
             (Bracket.volume is Pending because Bracket.width failed), \
             got zero; all diagnostics: {:#?}",
            result.diagnostics
        );
        for d in &pending_diags {
            assert_eq!(
                d.severity,
                Some(DiagnosticSeverity::WARNING),
                "computation-pending diagnostic must have WARNING severity, got {:?}",
                d.severity
            );
        }
    }

    /// (c) A normal evaluation (all cells Final) must produce zero freshness-code
    /// diagnostics.  This covers both Final (success) and the Intermediate case
    /// (Intermediate → no emission, arch §7.2): since a completed eval leaves all
    /// cells Final, no computation-* diagnostics should appear.
    ///
    /// This test passes both before and after step-18 (it is a negative assertion
    /// that guards against spurious fresh-start emission).
    #[test]
    fn normal_eval_emits_no_freshness_diagnostics() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let freshness_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(&d.code,
                Some(lsp_types::NumberOrString::String(s))
                    if s == "computation-failed" || s == "computation-pending"
            ))
            .collect();

        assert!(
            freshness_diags.is_empty(),
            "normal (all-Final) eval must produce zero freshness-code diagnostics; \
             got: {:#?}",
            freshness_diags
        );
    }

    /// (d) **Separation regression** — constraint-violation source must NOT produce
    /// any `computation-failed` diagnostics; the existing `constraint <id> violated`
    /// diagnostics must still appear.
    ///
    /// Guards arch §9.3: constraint violations route through `Satisfaction::Violated`,
    /// NOT through `Freshness::Failed`.  This test passes both before and after
    /// step-18, ensuring the implementation never routes violations through the
    /// freshness channel.
    #[test]
    fn constraint_violation_does_not_produce_computation_failed() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source_violating = reify_test_support::bracket_source_violating();

        let result = compute_diagnostics_with_state(&mut state, &source_violating, &uri);

        // (i) The existing constraint-violated diagnostic must be present.
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
            "violating source must produce at least one constraint-violated ERROR; \
             got: {:#?}",
            result.diagnostics
        );

        // (ii) Zero `computation-failed` diagnostics — constraint violations
        //      must NEVER route through the freshness channel.
        let computation_failed_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-failed".to_string(),
                    ))
            })
            .collect();
        assert!(
            computation_failed_diags.is_empty(),
            "constraint-violation source must produce ZERO 'computation-failed' diagnostics \
             (arch §9.3 separation); got: {:#?}",
            computation_failed_diags
        );
    }

}
