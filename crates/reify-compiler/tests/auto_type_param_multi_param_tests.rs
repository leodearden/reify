//! Multi-param orchestration tests for `auto` type-parameter resolution.
//!
//! Targets `crates/reify-compiler/src/auto_type_param.rs`'s public
//! `resolve_auto_type_params` orchestrator function and its supporting types
//! [`AutoTypeParam`] (input record) and [`MultiParamResolutionOutcome`] (result
//! record). The PRD that drives this work is
//! `docs/prds/auto-type-param-resolution.md` §"Phase D" (PRD task 4) and
//! acceptance criterion 6: "`Coupling<auto: A, auto: B>` — `A` resolves first;
//! `B`'s candidate pool is computed against the resolved `A`."
//!
//! # Scope
//!
//! This file covers the orchestrator-level behaviors (multi-param iteration,
//! halt-on-first-failure, declared-order semantics, per-param `free` flag)
//! using the same `MockConstraintChecker` + `TopologyTemplateBuilder` helpers
//! used by Phase B/C tests, plus `parse_and_compile` for registry fixtures
//! where real trait/structure lookups are needed (same pattern as Phase A tests).
//!
//! SchemaNode topology-trigger wiring (task 2388), LSP diagnostic surface
//! (task 2389), determinism smoke test (task 2390), and type-substitution
//! mechanics are out of scope here.

use std::collections::HashMap;

use reify_compiler::auto_type_param::{
    AutoTypeParam, MultiParamResolutionOutcome, SelectionResult, resolve_auto_type_params,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};
use reify_types::{CompiledFunction, DiagnosticCode, Severity, SourceSpan};

/// Build the `(template_registry, trait_registry)` pair that
/// `enumerate_candidates` consumes, borrowing from a single compiled module.
///
/// Mirrors `build_registries` from `auto_type_param_phase_a_tests.rs`.
fn build_registries(
    module: &CompiledModule,
) -> (
    HashMap<String, &TopologyTemplate>,
    HashMap<String, &CompiledTrait>,
) {
    let template_registry: HashMap<String, &TopologyTemplate> = module
        .templates
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    let trait_registry: HashMap<String, &CompiledTrait> = module
        .trait_defs
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    (template_registry, trait_registry)
}

// ─── step-1: empty params slice is a vacuous success ──────────────────────

/// Invoking `resolve_auto_type_params` with an empty `params` slice is a
/// no-op: the outcome's `per_param` is empty, `substitution` is empty, and
/// zero diagnostics are pushed. This pins the vacuous-success contract from
/// the design decision: an empty params slice is semantically valid (a
/// definition with zero `auto:` type-params has no orchestration work to do)
/// and MUST NOT panic or emit a diagnostic.
#[test]
fn empty_params_returns_vacuous_success() {
    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let outcome = resolve_auto_type_params(
        &[],
        &HashMap::new(),
        &HashMap::new(),
        &template,
        &checker,
        functions,
        &mut diagnostics,
    );

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![],
            substitution: vec![],
        },
        "empty params must return vacuous outcome with empty per_param and substitution"
    );
    assert!(
        diagnostics.is_empty(),
        "empty params must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}
