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
    AutoTypeParam, MAX_AUTO_TYPE_PARAM_CANDIDATES, MultiParamResolutionOutcome, SelectionResult,
    resolve_auto_type_params,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};
use reify_types::{CompiledFunction, DiagnosticCode, Severity, SourceSpan};

/// Build a Reify source with `trait Seal {}` and `count` structures
/// `S00`..`S{count-1}` each declaring `: Seal`. Zero-padded to two digits
/// so alphabetical ordering matches numeric ordering up to S99.
///
/// Mirrors `build_n_seal_structures` from `auto_type_param_phase_a_tests.rs`.
fn build_n_seal_structures(count: usize) -> String {
    assert!(
        count <= 100,
        "build_n_seal_structures: zero-pad width is two digits; max count is 100"
    );
    let mut src = String::from("trait Seal {}\n");
    for i in 0..count {
        src.push_str(&format!(
            "structure def S{:02} : Seal {{\n    param x : Real = 1.0\n}}\n",
            i
        ));
    }
    src
}

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

// ─── step-7: Phase A overflow on first param halts orchestration ──────────

/// When the first param's bounds match more than `MAX_AUTO_TYPE_PARAM_CANDIDATES`
/// in-scope structures (overflow), the orchestrator halts after recording the
/// first param's outcome. The second param is NOT enumerated.
///
/// Pins:
/// - `per_param.len() == 1` — only the first (overflowed) param is recorded
/// - `substitution.is_empty()` — no successful substitutions
/// - exactly one `AutoTypeParamPoolOverflow` diagnostic from Phase A
/// - The first param's `SelectionResult` is `Ambiguous` (overflow maps to Ambiguous)
/// - No second diagnostic (second param was not enumerated)
#[test]
fn overflow_on_first_param_halts_and_does_not_enumerate_second_param() {
    // MAX+1 Seal structures → overflow on first param.
    let overflow_source = build_n_seal_structures(MAX_AUTO_TYPE_PARAM_CANDIDATES + 1);
    let overflow_module = parse_and_compile(&overflow_source);
    let (template_registry, trait_registry) = build_registries(&overflow_module);

    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    // Second param has a bound "Cooled" that matches nothing — but it should
    // NOT be enumerated at all (no diagnostic for it).
    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: false,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: false,
            use_site_span: SourceSpan::new(30, 40),
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

    // Only first param recorded; it overflowed → Ambiguous.
    assert_eq!(
        outcome.per_param.len(),
        1,
        "overflow on first param must halt: per_param must have exactly 1 entry, got: {:?}",
        outcome.per_param
    );
    assert_eq!(outcome.per_param[0].0, "T", "first per_param entry must be for param 'T'");
    assert!(
        matches!(outcome.per_param[0].1, SelectionResult::Ambiguous(_)),
        "overflow maps to Ambiguous; got: {:?}",
        outcome.per_param[0].1
    );
    assert!(
        outcome.substitution.is_empty(),
        "overflow on first param must yield empty substitution, got: {:?}",
        outcome.substitution
    );

    // Exactly one diagnostic: the overflow from Phase A (not a second for U).
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one overflow diagnostic expected (second param not enumerated), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamPoolOverflow),
        "diagnostic must be AutoTypeParamPoolOverflow, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "overflow diagnostic must be an error"
    );
}

// ─── step-9: Phase C NoCandidate on first param halts orchestration ────────

/// When the first param's bounds match zero in-scope structures, Phase C
/// emits a `NoCandidate` error and the orchestrator halts. The second param
/// is NOT enumerated.
///
/// Pins:
/// - `per_param == [("T", NoCandidate)]` — length 1
/// - `substitution.is_empty()` — no successful substitutions
/// - exactly one `AutoTypeParamNoCandidate` diagnostic
/// - no second diagnostic (second param not enumerated)
#[test]
fn no_candidate_on_first_param_halts_and_does_not_enumerate_second_param() {
    // Source with trait Seal (but no structures implementing it) and a
    // structure implementing Cooled for the second param.
    let source = r#"
trait Seal {}
trait Cooled {}

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
            bounds: vec!["Seal".to_string()], // zero structures implement Seal
            free: false,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()], // one structure; should NOT be enumerated
            free: false,
            use_site_span: SourceSpan::new(30, 40),
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
            per_param: vec![("T".to_string(), SelectionResult::NoCandidate)],
            substitution: vec![],
        },
        "no-candidate on first param must halt with per_param=[(T, NoCandidate)], substitution=[]"
    );

    // Exactly one diagnostic: NoCandidate for T (not a second for U).
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one no-candidate diagnostic expected (second param not enumerated), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "diagnostic must be AutoTypeParamNoCandidate, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "no-candidate diagnostic must be an error"
    );
}
