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

// ─── step-3: single-param happy path ──────────────────────────────────────

/// One `AutoTypeParam` whose bounds are satisfied by exactly one in-scope
/// structure (`ORingSeal : Seal`). Expected outcome: `per_param` has one
/// entry `("T", Selected("ORingSeal"))`, `substitution` has one entry
/// `("T", "ORingSeal")`, and zero diagnostics are emitted.
///
/// Pins that the orchestrator correctly threads a single param through
/// Phase A → B → C and produces a `Selected` outcome recorded in both
/// `per_param` and `substitution`.
#[test]
fn single_param_happy_path_returns_selected() {
    let source = r#"
trait Seal {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![AutoTypeParam {
        name: "T".to_string(),
        bounds: vec!["Seal".to_string()],
        free: false,
        use_site_span: SourceSpan::empty(0),
    }];

    let outcome = resolve_auto_type_params(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        &mut diagnostics,
    );

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![("T".to_string(), SelectionResult::Selected("ORingSeal".to_string()))],
            substitution: vec![("T".to_string(), "ORingSeal".to_string())],
        },
        "single-param happy path must produce Selected(ORingSeal)"
    );
    assert!(
        diagnostics.is_empty(),
        "single-param happy path must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-5: multi-param happy path with declared-order substitution ───────

/// Two `AutoTypeParam`s T (bound: Seal → ORingSeal) and U (bound: Cooled →
/// AirCooled), both resolving cleanly. Expected outcome: `per_param` has both
/// entries in declared order, `substitution` has both entries in declared
/// order, and zero diagnostics are emitted.
///
/// Pins that the orchestrator correctly iterates ALL params when each succeeds
/// and that both `per_param` and `substitution` accumulate in declared order.
#[test]
fn multi_param_happy_path_resolves_both_in_declared_order() {
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: false,
            use_site_span: SourceSpan::empty(0),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: false,
            use_site_span: SourceSpan::empty(0),
        },
    ];

    let outcome = resolve_auto_type_params(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        &mut diagnostics,
    );

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![
                ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
                ("U".to_string(), SelectionResult::Selected("AirCooled".to_string())),
            ],
            substitution: vec![
                ("T".to_string(), "ORingSeal".to_string()),
                ("U".to_string(), "AirCooled".to_string()),
            ],
        },
        "multi-param happy path must accumulate both params in declared order"
    );
    assert!(
        diagnostics.is_empty(),
        "multi-param happy path must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}
