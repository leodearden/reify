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
use reify_types::{CompiledExpr, CompiledFunction, ConstraintNodeId, DiagnosticCode, Satisfaction, Severity, SourceSpan, Value};

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

// ─── step-11: Phase C Ambiguous on first param halts orchestration ─────────

/// When the first param has two feasible candidates and `free=false` (strict),
/// Phase C emits an `Ambiguous` error and the orchestrator halts. The second
/// param is NOT enumerated.
///
/// Pins:
/// - `per_param == [("T", Ambiguous([lex_first, lex_second]))]` — length 1
/// - `substitution.is_empty()` — no successful substitutions
/// - exactly one `AutoTypeParamAmbiguous` diagnostic
/// - no second diagnostic (second param not enumerated)
#[test]
fn ambiguous_on_first_param_strict_halts_and_does_not_enumerate_second_param() {
    // Two Seal structures (alphabetically GraphiteSeal < ORingSeal) and one
    // Cooled structure for the second (unharvested) param.
    let source = r#"
trait Seal {}
trait Cooled {}

structure def GraphiteSeal : Seal {
    param thickness : Real = 2.0
}

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
            bounds: vec!["Seal".to_string()], // two candidates → Ambiguous under strict
            free: false,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()], // one candidate; should NOT be enumerated
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

    // per_param has length 1: the Ambiguous result for T (lex order: GraphiteSeal, ORingSeal).
    assert_eq!(
        outcome.per_param.len(),
        1,
        "ambiguous on first param must halt: per_param must have exactly 1 entry, got: {:?}",
        outcome.per_param
    );
    assert_eq!(outcome.per_param[0].0, "T", "first per_param entry must be for param 'T'");
    assert_eq!(
        outcome.per_param[0].1,
        SelectionResult::Ambiguous(vec![
            "GraphiteSeal".to_string(),
            "ORingSeal".to_string(),
        ]),
        "strict ≥2 feasible candidates must produce Ambiguous([lex_first, lex_second])"
    );
    assert!(
        outcome.substitution.is_empty(),
        "ambiguous on first param must yield empty substitution, got: {:?}",
        outcome.substitution
    );

    // Exactly one diagnostic: Ambiguous for T (not a second for U).
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one ambiguous diagnostic expected (second param not enumerated), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamAmbiguous),
        "diagnostic must be AutoTypeParamAmbiguous, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "ambiguous diagnostic must be an error"
    );
}

// ─── step-13: mid-list failure — first param resolves, second param fails ──

/// When the first param resolves successfully and the second param fails
/// (NoCandidate), both entries appear in `per_param` (Selected then
/// NoCandidate), but only the first appears in `substitution`. The
/// asymmetry is the load-bearing assertion pinning halt-on-first-failure
/// with correct accumulation.
///
/// Pins:
/// - `per_param.len() == 2` — BOTH entries recorded (success then failure)
/// - `substitution.len() == 1` — only the resolved param
/// - exactly one `AutoTypeParamNoCandidate` diagnostic (for the second param)
/// - order is declared order: T first, U second
#[test]
fn mid_list_failure_records_success_then_failure_in_per_param_but_only_success_in_substitution() {
    // T: Seal → ORingSeal (one candidate, resolves).
    // U: Cooled → no structures implementing Cooled (NoCandidate).
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
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
            bounds: vec!["Seal".to_string()], // one candidate → Selected
            free: false,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()], // zero candidates → NoCandidate
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
            per_param: vec![
                ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
                ("U".to_string(), SelectionResult::NoCandidate),
            ],
            substitution: vec![
                ("T".to_string(), "ORingSeal".to_string()),
            ],
        },
        "mid-list failure: per_param must carry both entries (T:Selected, U:NoCandidate); \
         substitution must carry only T:ORingSeal"
    );

    // Exactly one diagnostic: NoCandidate for U.
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one no-candidate diagnostic expected (for U only), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "diagnostic must be AutoTypeParamNoCandidate for U, got: {:?}",
        diagnostics[0].code
    );
}

// ─── step-15: declared-order is observable via reordering (PRD criterion 6) ──

/// Swapping the declared order of two params changes which param's failure
/// halts the orchestrator, demonstrating declared-order semantics via
/// short-circuit + per_param-length differences.
///
/// Setup: param A (Seal → ORingSeal, one feasible candidate) and param B
/// (Cooled → no structures, zero candidates). Two runs:
/// - Order [A, B]: A resolves (Selected), B fails (NoCandidate) → `per_param.len()==2`,
///   `substitution.len()==1`.
/// - Order [B, A]: B fails immediately (NoCandidate) → `per_param.len()==1`,
///   `substitution.len()==0`.
///
/// Both runs emit exactly one diagnostic (only one param is ever the "first
/// failure"); the diagnostic is `AutoTypeParamNoCandidate` in both cases.
/// This pins PRD acceptance criterion 6: declared order controls which params
/// are evaluated before the halt point.
#[test]
fn reordering_params_changes_halt_point_demonstrating_declared_order_semantics() {
    // A: Seal → ORingSeal (one candidate). B: Cooled → nothing (zero candidates).
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];

    let param_a = AutoTypeParam {
        name: "A".to_string(),
        bounds: vec!["Seal".to_string()],   // one candidate → Selected
        free: false,
        use_site_span: SourceSpan::new(10, 20),
    };
    let param_b = AutoTypeParam {
        name: "B".to_string(),
        bounds: vec!["Cooled".to_string()], // zero candidates → NoCandidate
        free: false,
        use_site_span: SourceSpan::new(30, 40),
    };

    // ── Run 1: order [A, B] ──────────────────────────────────────────────────
    let mut diag_ab = Vec::new();
    let outcome_ab = resolve_auto_type_params(
        &[param_a.clone(), param_b.clone()],
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        &mut diag_ab,
    );

    // A resolves then B fails: both entries in per_param, only A in substitution.
    assert_eq!(
        outcome_ab.per_param.len(),
        2,
        "[A,B] order: per_param must have 2 entries (A:Selected, B:NoCandidate), got: {:?}",
        outcome_ab.per_param
    );
    assert_eq!(
        outcome_ab.substitution.len(),
        1,
        "[A,B] order: substitution must have 1 entry (A→ORingSeal), got: {:?}",
        outcome_ab.substitution
    );
    assert_eq!(outcome_ab.per_param[0].0, "A");
    assert!(matches!(
        outcome_ab.per_param[0].1,
        SelectionResult::Selected(_)
    ));
    assert_eq!(outcome_ab.per_param[1].0, "B");
    assert_eq!(outcome_ab.per_param[1].1, SelectionResult::NoCandidate);
    assert_eq!(diag_ab.len(), 1, "[A,B] order: exactly one diagnostic expected");
    assert_eq!(diag_ab[0].code, Some(DiagnosticCode::AutoTypeParamNoCandidate));

    // ── Run 2: order [B, A] ──────────────────────────────────────────────────
    let mut diag_ba = Vec::new();
    let outcome_ba = resolve_auto_type_params(
        &[param_b.clone(), param_a.clone()],
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        &mut diag_ba,
    );

    // B fails immediately: only B in per_param, substitution is empty.
    assert_eq!(
        outcome_ba.per_param.len(),
        1,
        "[B,A] order: per_param must have 1 entry (B:NoCandidate), got: {:?}",
        outcome_ba.per_param
    );
    assert_eq!(
        outcome_ba.substitution.len(),
        0,
        "[B,A] order: substitution must be empty, got: {:?}",
        outcome_ba.substitution
    );
    assert_eq!(outcome_ba.per_param[0].0, "B");
    assert_eq!(outcome_ba.per_param[0].1, SelectionResult::NoCandidate);
    assert_eq!(diag_ba.len(), 1, "[B,A] order: exactly one diagnostic expected");
    assert_eq!(diag_ba[0].code, Some(DiagnosticCode::AutoTypeParamNoCandidate));
}

// ─── step-17: per-param `free` flag is honored independently ─────────────────

/// When two params have different `free` flags, the orchestrator uses each
/// param's own `free` flag independently — it does NOT apply one shared flag
/// to all params.
///
/// Setup:
/// - T: `free=false` (strict), one feasible candidate (ORingSeal) → `Selected("ORingSeal")`, no diagnostic.
/// - U: `free=true` (free), two feasible candidates (GraphiteSeal, ORingSeal2) →
///   `Selected("GraphiteSeal")` (lex-first) + one `AutoTypeParamNonUnique` Warning.
///
/// Expected outcome: `per_param == [("T", Selected("ORingSeal")), ("U", Selected("GraphiteSeal"))]`,
/// `substitution.len() == 2`, exactly one diagnostic with code `AutoTypeParamNonUnique`
/// and severity `Warning`.
///
/// Pins that each param's `free` flag is read independently. A regression that
/// applied a single `free` value to all params would cause U to emit Ambiguous
/// (error) instead of NonUnique (warning+selected).
#[test]
fn per_param_free_flag_honored_independently() {
    // T's pool: one Seal structure (ORingSeal).
    // U's pool: two Polished structures (GraphiteSeal2, ORingSeal2) — lex order: G < O.
    let source = r#"
trait Seal {}
trait Polished {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def GraphiteSeal2 : Polished {
    param thickness : Real = 2.0
}

structure def ORingSeal2 : Polished {
    param diameter : Real = 8.0
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
            bounds: vec!["Seal".to_string()],     // one candidate → Selected, no diag
            free: false,                            // strict
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Polished".to_string()],  // two candidates → NonUnique + Selected(lex-first)
            free: true,                             // free
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

    // Both params resolve: per_param has 2 Selected entries, substitution has 2 entries.
    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![
                ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
                ("U".to_string(), SelectionResult::Selected("GraphiteSeal2".to_string())),
            ],
            substitution: vec![
                ("T".to_string(), "ORingSeal".to_string()),
                ("U".to_string(), "GraphiteSeal2".to_string()),
            ],
        },
        "per-param free: T (strict) must select ORingSeal; U (free) must select GraphiteSeal2 (lex-first)"
    );

    // Exactly one NonUnique warning (for U, not for T).
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one NonUnique warning expected (for U only), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "NonUnique diagnostic must be a Warning (not Error)"
    );
}

// ─── step-19: Phase B all-rejected → Phase C NoCandidate wiring ───────────

/// Pins the orchestrator's Phase B → Phase C wiring for the "all-rejected"
/// path: Phase A finds one candidate (ORingSeal), Phase B's constraint checker
/// returns `Violated` for every candidate, and Phase C emits a
/// `AutoTypeParamNoCandidate` diagnostic with a **rejection-summary** message.
///
/// # Coverage gap this test fills
///
/// The three existing `NoCandidate`-related orchestrator tests in this file
/// (e.g. `no_candidate_on_first_param_halts_and_does_not_enumerate_second_param`)
/// all flow through Phase A's **Empty** pool (zero structures implementing the
/// trait). That path emits `AutoTypeParamNoCandidate` directly inside the
/// `CandidateEnumeration::Empty` arm of the orchestrator, bypassing Phase B
/// and Phase C entirely.
///
/// The path exercised here is:
///   Phase A `CandidateEnumeration::Found(["ORingSeal"])`
///   → Phase B `filter_feasible_candidates` (receives `parameterized_template`,
///     `constraint_checker`, and `functions`)
///   → all candidates rejected → `FeasibilityResult::Empty { rejected: [...] }`
///   → Phase C `select_candidate(FeasibilityResult::Empty { ... })`
///   → `SelectionResult::NoCandidate`
///   → orchestrator emits the rejection-summary `AutoTypeParamNoCandidate`
///     diagnostic.
///
/// A regression that drops `parameterized_template`, `constraint_checker`, or
/// `functions` from the `filter_feasible_candidates` call, or mis-routes
/// `param.use_site_span` into Phase C, would not be caught by the existing
/// empty-pool tests nor by the happy-path tests. This test catches all such
/// regressions via a set of load-bearing assertions:
///
/// - `message.contains("rejected by constraint")` — distinguishes Phase C's
///   rejection-summary form from Phase A's zero-rejections form.
/// - `candidates == ["ORingSeal"]` — Phase C's Empty arm populates `candidates`
///   with rejected names; Phase A's short-circuit leaves `candidates` empty.
/// - `labels[0].span == use_site_span` — pins span propagation into Phase C.
#[test]
fn phase_b_all_rejected_routes_through_orchestrator_to_no_candidate_with_rejection_summary() {
    // Phase A will find exactly one candidate: ORingSeal.
    let source = r#"
trait Seal {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Parameterized template carries one top-level constraint (Coupling#0) so
    // Phase B has something to check. The mock ignores expression content; the
    // literal is only needed so the builder has a value to store.
    let expr = CompiledExpr::literal(Value::Bool(true), reify_types::Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Mock returns Violated for the constraint → ORingSeal is rejected → Phase B
    // returns FeasibilityResult::Empty { rejected: [ORingSeal] }.
    let cnid = ConstraintNodeId::new("Coupling", 0);
    let checker = MockConstraintChecker::new().with_result(cnid, Satisfaction::Violated);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    // Non-zero span so we can verify span propagation through Phase C.
    let use_site_span = SourceSpan::new(100, 110);

    let params = vec![AutoTypeParam {
        name: "T".to_string(),
        bounds: vec!["Seal".to_string()],
        free: false,
        use_site_span,
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

    // 1. per_param has exactly one entry: (T, NoCandidate).
    assert_eq!(
        outcome.per_param,
        vec![("T".to_string(), SelectionResult::NoCandidate)],
        "Phase B all-rejected must produce per_param=[(T, NoCandidate)]"
    );

    // 2. No successful substitution.
    assert!(
        outcome.substitution.is_empty(),
        "Phase B all-rejected must yield empty substitution, got: {:?}",
        outcome.substitution
    );

    // 3. Exactly one diagnostic.
    assert_eq!(
        diagnostics.len(),
        1,
        "Phase B all-rejected must emit exactly one diagnostic, got: {:?}",
        diagnostics
    );

    // 4. Correct diagnostic code.
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "diagnostic code must be AutoTypeParamNoCandidate, got: {:?}",
        diagnostics[0].code
    );

    // 5. Severity must be Error.
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "no-candidate diagnostic must be an Error"
    );

    // 6. Message carries the rejection-summary form, not the zero-rejections form.
    //    Phase C's Empty arm produces "...rejected by constraint <id>"; Phase A's
    //    short-circuit produces "no feasible candidates for bound '<B>'" (no suffix).
    assert!(
        diagnostics[0].message.contains("rejected by constraint"),
        "message must contain 'rejected by constraint' (Phase C rejection-summary form); \
         got: {:?}",
        diagnostics[0].message
    );

    // 7. Structured candidates field carries the rejected FQN (ORingSeal), not empty.
    //    Phase C's Empty arm calls with_candidates(rejected_names); Phase A's
    //    short-circuit calls with_candidates(Vec::<String>::new()).
    assert_eq!(
        diagnostics[0].candidates,
        vec!["ORingSeal".to_string()],
        "candidates field must carry [\"ORingSeal\"] (Phase C rejection-summary); \
         got: {:?}",
        diagnostics[0].candidates
    );

    // 8. Label span pins span propagation into Phase C (param.use_site_span).
    assert!(
        !diagnostics[0].labels.is_empty(),
        "diagnostic must have at least one label"
    );
    assert_eq!(
        diagnostics[0].labels[0].span,
        use_site_span,
        "label span must equal use_site_span ({:?}); got: {:?}",
        use_site_span,
        diagnostics[0].labels[0].span
    );
}
