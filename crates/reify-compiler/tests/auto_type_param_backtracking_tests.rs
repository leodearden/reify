//! Auto-type-param resolution v0.2: depth-bounded DFS over the cross-product.
//!
//! Targets `crates/reify-compiler/src/auto_type_param.rs`'s public
//! `resolve_auto_type_params_with_backtracking` orchestrator (task 2659). The
//! PRD that drives this work is
//! `docs/prds/v0_2/auto-resolution-backtracking.md`.
//!
//! # Scope
//!
//! - Algorithm shape: DFS over the cross-product of per-param Phase A
//!   candidate vectors, with `filter_feasible_candidates` re-checked at each
//!   leaf (full re-check per the PRD design decision).
//! - Strict-vs-free dispatch: strict mode continues past the first feasible
//!   leaf to detect ≥2 (Ambiguous); free mode picks lex-first feasible.
//! - Depth bound: `params.len() > max_depth` ⇒ emit
//!   `AutoTypeParamDepthBoundExceeded` (Warning) + delegate to v0.1 BFS
//!   `resolve_auto_type_params`. Boundary: `params.len() == max_depth`
//!   still runs DFS.
//! - Phase A failure halt parity with BFS: Empty / Overflow on any param
//!   halts before recursion, with the same per_param/substitution shape.
//!
//! # Out of scope (sibling tasks)
//!
//! - Backjumping via "rejected because" channel (task 2660).
//! - `auto(free)` report-all cross-product enumeration with the
//!   `AutoTypeParamNonUnique` warning (task 2661).
//! - Cross-product hard cap at 100k assignments (task 2662).
//! - Rich diagnostic format with smallest infeasibility witness (task 2663).
//! - Comprehensive v0.1 BFS-failure scenario coverage (task 2664).
//! - Type-substitution mechanics
//!   (`Type::TypeParam(T)` → `Type::StructureRef(candidate)`) — separately
//!   deferred per the PRD's "Constraint-feasibility incremental binding
//!   deferred" decision.

use std::collections::HashMap;

use reify_compiler::auto_type_param::{
    AutoTypeParam, MAX_AUTO_TYPE_PARAM_CANDIDATES, MultiParamResolutionOutcome, SelectionResult,
    resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};
use reify_types::{
    CompiledExpr, CompiledFunction, DiagnosticCode, Satisfaction, Severity, SourceSpan, Type, Value,
};

/// Build the `(template_registry, trait_registry)` pair that
/// `enumerate_candidates` consumes, borrowing from a single compiled module.
///
/// Mirrors `build_registries` from `auto_type_param_multi_param_tests.rs` /
/// `auto_type_param_phase_a_tests.rs`. Lifted verbatim so tests in this
/// file are self-contained.
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

/// Build a Reify source string declaring `count` distinct structures all
/// implementing `trait Seal`. Used by the Phase-A overflow test to drive
/// the candidate pool above `MAX_AUTO_TYPE_PARAM_CANDIDATES`.
///
/// Mirrors `build_n_seal_structures` from `auto_type_param_multi_param_tests.rs`
/// — lifted verbatim so the backtracking test file remains self-contained.
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

// ─── step-15: DFS empty-params is a vacuous success (parity with BFS) ──────

/// Invoking `resolve_auto_type_params_with_backtracking` with an empty
/// `params` slice is a vacuous no-op — exactly mirroring v0.1 BFS's
/// `empty_params_returns_vacuous_success` contract. The outcome's
/// `per_param` and `substitution` are both empty, and zero diagnostics are
/// pushed (in particular: NO `AutoTypeParamDepthBoundExceeded` warning,
/// because `0 <= max_depth`).
#[test]
fn dfs_empty_params_returns_vacuous_success() {
    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let outcome = resolve_auto_type_params_with_backtracking(
        &[],
        &HashMap::new(),
        &HashMap::new(),
        &template,
        &checker,
        functions,
        6,
        &mut diagnostics,
    );

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![],
            substitution: vec![],
        },
        "DFS with empty params must return vacuous outcome with empty per_param and substitution"
    );
    assert!(
        diagnostics.is_empty(),
        "DFS with empty params must emit zero diagnostics (no depth-bound warning), got: {:?}",
        diagnostics
    );
}

// ─── step-17: DFS single-param parity with BFS happy path ─────────────────

/// One `AutoTypeParam` `[T : Seal]` whose bounds are satisfied by exactly
/// one in-scope structure (`ORingSeal : Seal`). DFS at `max_depth = 6`
/// must produce the same outcome as v0.1 BFS's
/// `single_param_happy_path_returns_selected`: `per_param` =
/// `[("T", Selected("ORingSeal"))]`, `substitution` =
/// `[("T", "ORingSeal")]`, zero diagnostics.
///
/// Sanity: DFS must not regress the trivial single-param case. With one
/// candidate the cross-product is a single leaf and the recursion is
/// degenerate, so this is the smallest non-empty exercise of the
/// Phase A → leaf-feasibility → select pipeline through DFS.
#[test]
fn dfs_single_param_one_candidate_selects_lex_first() {
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

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
        &mut diagnostics,
    );

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![("T".to_string(), SelectionResult::Selected("ORingSeal".to_string()))],
            substitution: vec![("T".to_string(), "ORingSeal".to_string())],
        },
        "DFS single-param one-candidate must produce Selected(ORingSeal) — parity with BFS happy path"
    );
    assert!(
        diagnostics.is_empty(),
        "DFS single-param happy path must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-19: DFS multi-param all-feasible picks lex-first cross-product ───

/// Two `AutoTypeParam`s `[T : Seal, U : Cooled]` where:
/// - T has two candidates (Seal lex order: `ORingSeal`, `RubberSeal`),
/// - U has two candidates (Cooled lex order: `AirCooled`, `WaterCooled`).
///
/// With a default `MockConstraintChecker` (every leaf ⇒ Satisfied) and
/// both params `free=true`, DFS must visit the cross-product in
/// lexicographic order (T outer, U inner) and stop at the first feasible
/// leaf. Expected outcome: `substitution == [(T, ORingSeal), (U, AirCooled)]`,
/// `per_param == [(T, Selected(ORingSeal)), (U, Selected(AirCooled))]`,
/// zero diagnostics (free-mode `NonUnique` warnings are task 2661's scope —
/// see file-level out-of-scope note).
///
/// Strict-Ambiguous over multiple cross-product feasibles is the inverse
/// of this test and is exercised by `dfs_strict_mode_with_two_feasible_cross_products_returns_ambiguous`
/// in step-23.
#[test]
fn dfs_multi_param_all_feasible_picks_lex_first_cross_product() {
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def RubberSeal : Seal {
    param thickness : Real = 2.0
}

structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}

structure def WaterCooled : Cooled {
    param flow_rate : Real = 12.0
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
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
    ];

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
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
        "DFS multi-param all-feasible (free=true on both) must pick the lex-first cross-product (T=ORingSeal, U=AirCooled)"
    );
    assert!(
        diagnostics.is_empty(),
        "DFS multi-param all-feasible free-mode must emit zero diagnostics in 2659 (NonUnique warning is task 2661's scope), got: {:?}",
        diagnostics
    );
}

// ─── step-21: DFS backtracks when first leaf violated, picks second ────────

/// Two `AutoTypeParam`s `[T : Seal, U : Cooled]` with two candidates each
/// (4 cross-product leaves total: `(ORingSeal, AirCooled)`, `(ORingSeal,
/// WaterCooled)`, `(RubberSeal, AirCooled)`, `(RubberSeal, WaterCooled)`).
/// The parameterized template carries one top-level constraint so leaf
/// verdicts are observable through the constraint-checker queue.
///
/// `MockConstraintChecker::with_call_queue(vec![Violated, Satisfied])` makes
/// the first leaf's check return `Violated` and the second leaf's return
/// `Satisfied`. Both params are `free=true` so the second feasible found
/// stops the search (free-mode contract).
///
/// Expected DFS visit order:
/// 1. `(ORingSeal, AirCooled)` → leaf check pops `Violated` → infeasible
///    → backtrack at the `U`-level.
/// 2. `(ORingSeal, WaterCooled)` → leaf check pops `Satisfied` → feasible
///    → record, early-terminate.
///
/// Asserts `substitution == [(T, ORingSeal), (U, WaterCooled)]` and
/// `per_param == [(T, Selected(ORingSeal)), (U, Selected(WaterCooled))]`.
/// Pins backtracking semantics on the canonical "first leaf rejected" case.
#[test]
fn dfs_backtracks_when_first_leaf_violated_then_picks_second_feasible() {
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def RubberSeal : Seal {
    param thickness : Real = 2.0
}

structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}

structure def WaterCooled : Cooled {
    param flow_rate : Real = 12.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Parameterized template carries one top-level constraint so the queue
    // mock's per-call verdict produces non-empty `ConstraintResult`s in
    // `filter_feasible_candidates`. The mock ignores expression content;
    // the literal is only needed so the builder has a value to store.
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Leaf 1 check ⇒ Violated (backtrack); Leaf 2 check ⇒ Satisfied (accept).
    let checker = MockConstraintChecker::new()
        .with_call_queue(vec![Satisfaction::Violated, Satisfaction::Satisfied]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
    ];

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
        &mut diagnostics,
    );

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![
                ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
                ("U".to_string(), SelectionResult::Selected("WaterCooled".to_string())),
            ],
            substitution: vec![
                ("T".to_string(), "ORingSeal".to_string()),
                ("U".to_string(), "WaterCooled".to_string()),
            ],
        },
        "DFS must backtrack from infeasible leaf (ORingSeal, AirCooled) and pick (ORingSeal, WaterCooled) as the next feasible leaf"
    );
    assert!(
        diagnostics.is_empty(),
        "DFS backtracking happy path (free-mode) must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-23: DFS strict-mode ≥2 feasible cross-products → Ambiguous ───────

/// Two `AutoTypeParam`s `[T : Seal, U : Cooled]` with two candidates each
/// (4 cross-product leaves total). With the default `MockConstraintChecker`
/// (no constraints on the parameterized template ⇒ every leaf trivially
/// feasible) and **both params `free=false` (strict)**, DFS must NOT stop
/// at the first feasible leaf — it must continue searching to detect ≥2
/// feasible cross-products and produce a single `Ambiguous` outcome.
///
/// The strict-mode contract here is the cross-product analog of v0.1's
/// per-param strict-Ambiguous arm: ≥2 feasibles means the user must pick
/// (no automatic disambiguation), so Phase C surfaces the witnesses for
/// the diagnostic and halts substitution.
///
/// Asserts the loose-witness contract (witnesses.len() ≥ 2, no exact
/// witness format) so this test stays decoupled from the witness-string
/// formatting decision pinned in step-24. Richer per-witness format with
/// the smallest-infeasibility witness is task 2663's scope.
///
/// Pins:
/// - `per_param.len() == 1` (single Ambiguous entry on the FIRST param's name)
/// - `per_param[0].0 == "T"` (Ambiguous attaches to params[0].name)
/// - `per_param[0].1` matches `SelectionResult::Ambiguous(_)` with ≥2 witnesses
/// - `substitution.is_empty()` (no successful substitutions on Ambiguous)
/// - exactly one `AutoTypeParamAmbiguous` diagnostic
#[test]
fn dfs_strict_mode_with_two_feasible_cross_products_returns_ambiguous() {
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def RubberSeal : Seal {
    param thickness : Real = 2.0
}

structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}

structure def WaterCooled : Cooled {
    param flow_rate : Real = 12.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // No constraints on the template ⇒ every leaf is trivially feasible
    // (filter_feasible_candidates returns Feasible for every cross-product
    // assignment with default Satisfied checker).
    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Satisfied);
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

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

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
        &mut diagnostics,
    );

    // Ambiguous attaches to the FIRST param's name (one per_param entry only),
    // mirroring the BFS contract for halt-on-first-failure: substitution stops
    // at the first failure, so per_param does too.
    assert_eq!(
        outcome.per_param.len(),
        1,
        "DFS strict-mode ≥2 feasible cross-products must produce exactly one per_param entry (the Ambiguous on params[0]), got: {:?}",
        outcome.per_param,
    );
    assert_eq!(
        outcome.per_param[0].0, "T",
        "Ambiguous outcome must attach to the first param's name (declared-order halt parity with BFS)"
    );
    match &outcome.per_param[0].1 {
        SelectionResult::Ambiguous(witnesses) => {
            assert!(
                witnesses.len() >= 2,
                "DFS strict-mode Ambiguous must carry ≥2 witnesses (lex-first two cross-product summaries), got: {:?}",
                witnesses,
            );
        }
        other => panic!(
            "DFS strict-mode ≥2 feasible cross-products must produce SelectionResult::Ambiguous, got: {:?}",
            other,
        ),
    }
    assert!(
        outcome.substitution.is_empty(),
        "DFS strict-mode Ambiguous must yield empty substitution (no successful resolutions), got: {:?}",
        outcome.substitution,
    );

    // Exactly one Ambiguous diagnostic emitted by step-24's strict dispatch.
    assert_eq!(
        diagnostics.len(),
        1,
        "DFS strict-mode Ambiguous must emit exactly one diagnostic, got: {:?}",
        diagnostics,
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamAmbiguous),
        "DFS strict-mode diagnostic must be AutoTypeParamAmbiguous, got: {:?}",
        diagnostics[0].code,
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "AutoTypeParamAmbiguous diagnostic must be an Error severity"
    );
}

// ─── step-25: DFS Phase A overflow on first param halts before recursion ───

/// When the first param's bounds match more than `MAX_AUTO_TYPE_PARAM_CANDIDATES`
/// in-scope structures (overflow), DFS halts after recording the first param's
/// outcome — the second param is NOT enumerated and the recursion never starts.
///
/// Mirrors v0.1 BFS's `overflow_on_first_param_halts_and_does_not_enumerate_second_param`,
/// exercising the Phase A overflow halt parity through DFS to pin that the
/// up-front per-param Phase A enumeration phase short-circuits identically.
///
/// Pins:
/// - `per_param.len() == 1` — only the first (overflowed) param is recorded
/// - `per_param[0].0 == "T"` — the first param's name
/// - `per_param[0].1` matches `SelectionResult::Ambiguous(_)` (overflow → Ambiguous)
/// - `substitution.is_empty()` — no successful substitutions
/// - exactly one `AutoTypeParamPoolOverflow` diagnostic from Phase A
/// - No second diagnostic (second param was not enumerated)
#[test]
fn dfs_phase_a_overflow_on_first_param_halts_before_recursion() {
    // MAX+1 Seal structures → overflow on first param.
    let overflow_source = build_n_seal_structures(MAX_AUTO_TYPE_PARAM_CANDIDATES + 1);
    let overflow_module = parse_and_compile(&overflow_source);
    let (template_registry, trait_registry) = build_registries(&overflow_module);

    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    // Second param has a bound "Cooled" that matches nothing — but it should
    // NOT be enumerated at all (no diagnostic for it). Mirrors BFS test.
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

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
        &mut diagnostics,
    );

    // Only first param recorded; it overflowed → Ambiguous.
    assert_eq!(
        outcome.per_param.len(),
        1,
        "DFS overflow on first param must halt: per_param must have exactly 1 entry, got: {:?}",
        outcome.per_param
    );
    assert_eq!(
        outcome.per_param[0].0, "T",
        "first per_param entry must be for param 'T'"
    );
    assert!(
        matches!(outcome.per_param[0].1, SelectionResult::Ambiguous(_)),
        "DFS Phase A overflow maps to Ambiguous; got: {:?}",
        outcome.per_param[0].1
    );
    assert!(
        outcome.substitution.is_empty(),
        "DFS overflow on first param must yield empty substitution, got: {:?}",
        outcome.substitution
    );

    // Exactly one diagnostic: the overflow from Phase A (not a second for U).
    assert_eq!(
        diagnostics.len(),
        1,
        "DFS exactly one overflow diagnostic expected (second param not enumerated, recursion not entered), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamPoolOverflow),
        "DFS diagnostic must be AutoTypeParamPoolOverflow, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "DFS overflow diagnostic must be an error"
    );
}
