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
//! - Cross-product hard cap at 100k assignments (task 2662).
//! - Rich diagnostic format with smallest infeasibility witness (task 2663).
//! - Comprehensive v0.1 BFS-failure scenario coverage (task 2664).
//! - Type-substitution mechanics
//!   (`Type::TypeParam(T)` → `Type::StructureRef(candidate)`) — separately
//!   deferred per the PRD's "Constraint-feasibility incremental binding
//!   deferred" decision.
//!
//! Task 2660 (backjumping via "rejected because" channel) now lands in this
//! module. The `dfs_backjumps_*` and `dfs_no_blame_*` tests below pin task
//! 2660's behavior.
//!
//! The `auto(free)` cross-product NonUnique Warning enumeration (originally
//! listed here as "task 2661's scope") now lands in this file — see
//! `dfs_free_mode_two_feasible_cross_products_emits_non_unique_warning_and_picks_lex_first`,
//! `dfs_free_mode_more_than_sixteen_feasibles_emits_non_unique_with_elision_count`,
//! `dfs_free_mode_exactly_sixteen_feasibles_emits_non_unique_without_elision_marker`,
//! and `dfs_mixed_strict_and_free_with_two_feasibles_emits_ambiguous_not_non_unique`.

use std::collections::{BTreeSet, HashMap};

use reify_compiler::auto_type_param::{
    AutoTypeParam, MAX_AUTO_TYPE_PARAM_CANDIDATES, MultiParamResolutionOutcome, SelectionResult,
    build_constraint_blame_map, resolve_auto_type_params,
    resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};
use reify_types::{
    BinOp, CompiledExpr, CompiledFunction, ConstraintNodeId, DiagnosticCode, Satisfaction,
    Severity, SourceSpan, Type, Value, ValueCellId,
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
/// one `AutoTypeParamNonUnique` Warning diagnostic (task 2661, now landed —
/// see `dfs_free_mode_two_feasible_cross_products_emits_non_unique_warning_and_picks_lex_first`).
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
    // Task 2661: all-free ≥2 feasibles now emit AutoTypeParamNonUnique (Warning).
    // With no constraints, all 4 cross-product leaves are trivially feasible.
    // The lex-first is still (ORingSeal, AirCooled) — per_param/substitution
    // unchanged; only the diagnostic count changes from 0 to 1.
    assert_eq!(
        diagnostics.len(),
        1,
        "DFS multi-param all-feasible free-mode (4 feasibles) must emit 1 AutoTypeParamNonUnique \
         diagnostic (task 2661); got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "DFS multi-param all-feasible free-mode diagnostic must be AutoTypeParamNonUnique"
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "AutoTypeParamNonUnique must be Warning severity"
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
    // Task 2661: all-free mode now collects ALL feasibles (max_feasible_to_collect=usize::MAX).
    // After the 2-item queue [Violated, Satisfied] is exhausted, leaves 3 and 4 use the
    // default Satisfied → 3 feasibles total: (ORingSeal,WaterCooled), (RubberSeal,AirCooled),
    // (RubberSeal,WaterCooled). Lex-first is still (ORingSeal, WaterCooled).
    // The backtracking semantics are unchanged; only the diagnostic count changes.
    assert_eq!(
        diagnostics.len(),
        1,
        "DFS backtracking free-mode (≥2 feasibles) must emit 1 AutoTypeParamNonUnique \
         diagnostic (task 2661); got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "DFS backtracking free-mode diagnostic must be AutoTypeParamNonUnique"
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "AutoTypeParamNonUnique must be Warning severity"
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
/// Asserts exactly 2 witnesses (strict-mode cap, max_feasible_to_collect=2),
/// no exact witness format, so this test stays decoupled from the
/// witness-string formatting decision pinned in step-24. Richer per-witness
/// format with the smallest-infeasibility witness is task 2663's scope.
///
/// Pins:
/// - `per_param.len() == 1` (single Ambiguous entry on the FIRST param's name)
/// - `per_param[0].0 == "T"` (Ambiguous attaches to params[0].name)
/// - `per_param[0].1` matches `SelectionResult::Ambiguous(_)` with exactly 2 witnesses (strict-mode cap)
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
            assert_eq!(
                witnesses.len(),
                2,
                "DFS strict-mode Ambiguous must carry exactly 2 witnesses (strict-mode early-stop cap, max_feasible_to_collect=2), got: {:?}",
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

    // ── Pins the FQN-only invariant on `Diagnostic.candidates` for the ──────
    // multi-param ≥2-feasibles emission site (task 2860).
    //
    // The structured field carries the lex-first feasible cross-product leaf's
    // FQNs (declared order); per-leaf composite witnesses live in the
    // human-readable message body only.
    //
    // In strict mode, `max_feasible_to_collect = 2`: the DFS visits feasibles
    // in declared-order × lex-within-param order and stops after collecting 2.
    // With Seal candidates {ORingSeal, RubberSeal} and Cooled candidates
    // {AirCooled, WaterCooled} (both lex-sorted), the first two feasibles
    // collected are (ORingSeal, AirCooled) and (ORingSeal, WaterCooled), so
    // `feasible_assignments[0]` is ["ORingSeal", "AirCooled"].
    //
    // (a) `candidates` carries exactly the lex-first leaf's FQN list.
    assert_eq!(
        diagnostics[0].candidates,
        vec!["ORingSeal".to_string(), "AirCooled".to_string()],
        "Diagnostic.candidates must be the lex-first feasible cross-product leaf's FQN list \
         (FQN-only invariant, task 2860); got: {:?}",
        diagnostics[0].candidates,
    );

    // (b) No entry is a composite `name=value,name=value` tuple — the FQN-only
    //     invariant shared with Phase A overflow, Phase C strict-Ambiguous, and
    //     Phase C all-rejected emission sites (see `candidates` doc-comment in
    //     `crates/reify-types/src/diagnostics.rs`).
    for entry in &diagnostics[0].candidates {
        assert!(
            !entry.contains('='),
            "Diagnostic.candidates entries must be bare FQNs (no '=' composite tuples), got: {:?}",
            entry,
        );
        assert!(
            !entry.contains(','),
            "Diagnostic.candidates entries must be bare FQNs (no ',' composite tuples), got: {:?}",
            entry,
        );
    }

    // (c) dropped — FQN content is already pinned by (a)/(b) above, and the
    //     strict-mode early-stop cap is pinned by witnesses.len() == 2 above.
    //     Message-body witness format is left uncoupled for task 2663.
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
}

// ─── step-27: DFS Phase A empty pool on first param halts before recursion ──

/// When the first param's bounds match zero in-scope structures, Phase A
/// returns `CandidateEnumeration::Empty` for that param, and the DFS
/// orchestrator emits a `NoCandidate` error and halts before enumerating
/// the second param or starting the recursion.
///
/// Mirrors v0.1 BFS's `no_candidate_on_first_param_halts_and_does_not_enumerate_second_param`
/// to pin Phase-A empty-pool halt parity through DFS.
///
/// Pins:
/// - `per_param == [("T", NoCandidate)]` — length 1
/// - `substitution.is_empty()` — no successful substitutions
/// - exactly one `AutoTypeParamNoCandidate` diagnostic with the
///   zero-rejections message form (`"...no feasible candidates for bound 'Seal'"`
///   — no `"rejected by constraint"` suffix)
/// - no second diagnostic (second param `U` was not enumerated)
#[test]
fn dfs_phase_a_empty_pool_on_first_param_halts_before_recursion() {
    // Source with `trait Seal` (no structures implementing it) and a
    // structure implementing `Cooled` for the second param. The first
    // param's empty pool must short-circuit the orchestrator.
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
            per_param: vec![("T".to_string(), SelectionResult::NoCandidate)],
            substitution: vec![],
        },
        "DFS no-candidate on first param must halt with per_param=[(T, NoCandidate)], substitution=[]"
    );

    // Exactly one diagnostic: NoCandidate for T (not a second for U).
    assert_eq!(
        diagnostics.len(),
        1,
        "DFS exactly one no-candidate diagnostic expected (second param not enumerated), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "DFS diagnostic must be AutoTypeParamNoCandidate, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "DFS no-candidate diagnostic must be an error"
    );
    // Zero-rejections message form: bound is mentioned but no "rejected by
    // constraint" suffix (Phase A's empty-pool form, not Phase C's
    // all-rejected-by-feasibility form).
    assert!(
        diagnostics[0].message.contains("'Seal'"),
        "DFS no-candidate diagnostic must mention the bound 'Seal'; got: {:?}",
        diagnostics[0].message
    );
    assert!(
        !diagnostics[0].message.contains("rejected by constraint"),
        "DFS no-candidate (zero-rejections form) must NOT mention 'rejected by constraint'; got: {:?}",
        diagnostics[0].message
    );
}

// ─── step-29: DFS above max_depth emits warning + falls back to BFS ────────

/// Seven `AutoTypeParam`s (one per distinct trait with one matching structure
/// each). Calling DFS with `max_depth = 6` triggers the depth-bound fallback:
/// the orchestrator emits `AutoTypeParamDepthBoundExceeded` (Warning) and
/// delegates back to `resolve_auto_type_params` (v0.1 BFS).
///
/// Pins:
/// - DFS outcome equals what BFS would have returned for the same inputs
///   (every param `Selected` lex-first, declared-order substitution, length 7).
/// - Exactly one extra diagnostic compared to BFS's clean run, with code
///   `AutoTypeParamDepthBoundExceeded` (Warning) and message containing both
///   "7" (params count) and "6" (max_depth).
/// - The depth-bound check uses strict `>`: 7 > 6 ⇒ fallback fires; the
///   boundary case 6 == 6 is exercised in step-31.
#[test]
fn dfs_above_max_depth_emits_warning_and_falls_back_to_bfs() {
    // Seven distinct traits, each with a single implementing structure.
    // Trait names are ordered alphabetically so the canonical name lookup
    // is stable; ditto structure names.
    let source = r#"
trait T1 {}
trait T2 {}
trait T3 {}
trait T4 {}
trait T5 {}
trait T6 {}
trait T7 {}

structure def S1 : T1 { param x : Real = 1.0 }
structure def S2 : T2 { param x : Real = 2.0 }
structure def S3 : T3 { param x : Real = 3.0 }
structure def S4 : T4 { param x : Real = 4.0 }
structure def S5 : T5 { param x : Real = 5.0 }
structure def S6 : T6 { param x : Real = 6.0 }
structure def S7 : T7 { param x : Real = 7.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];

    let params: Vec<AutoTypeParam> = (1..=7)
        .map(|i| AutoTypeParam {
            name: format!("P{}", i),
            bounds: vec![format!("T{}", i)],
            free: false,
            use_site_span: SourceSpan::new(10 * i, 10 * i + 5),
        })
        .collect();

    // Capture BFS's outcome on the same inputs for parity comparison.
    let mut bfs_diagnostics = Vec::new();
    let bfs_outcome = resolve_auto_type_params(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        &mut bfs_diagnostics,
    );

    // Now run DFS with `max_depth = 6` (7 params > 6 ⇒ fallback fires).
    let mut dfs_diagnostics = Vec::new();
    let dfs_outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
        &mut dfs_diagnostics,
    );

    // Outcome parity: DFS-with-fallback must match BFS exactly.
    assert_eq!(
        dfs_outcome, bfs_outcome,
        "DFS above max_depth must delegate to BFS — outcome must match BFS's identical-input outcome. DFS: {:?}, BFS: {:?}",
        dfs_outcome, bfs_outcome
    );

    // Diagnostic delta: DFS emits one EXTRA `AutoTypeParamDepthBoundExceeded`
    // Warning beyond BFS's diagnostics (BFS itself emits zero diagnostics on
    // a clean 7-param happy path).
    assert_eq!(
        dfs_diagnostics.len(),
        bfs_diagnostics.len() + 1,
        "DFS above max_depth must emit exactly one extra diagnostic beyond BFS. DFS diagnostics: {:?}, BFS diagnostics: {:?}",
        dfs_diagnostics, bfs_diagnostics
    );
    let extra = dfs_diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .expect("DFS must emit exactly one AutoTypeParamDepthBoundExceeded diagnostic when params.len() > max_depth");
    assert_eq!(
        extra.severity,
        Severity::Warning,
        "AutoTypeParamDepthBoundExceeded must be a Warning severity, got: {:?}",
        extra.severity
    );
    assert!(
        extra.message.contains("7"),
        "depth-bound diagnostic must mention the params count '7'; got: {:?}",
        extra.message
    );
    assert!(
        extra.message.contains("6"),
        "depth-bound diagnostic must mention the max_depth '6'; got: {:?}",
        extra.message
    );
}

// ─── step-31: DFS at max_depth runs DFS (no fallback diagnostic) ───────────

/// Six `AutoTypeParam`s — boundary case `params.len() == max_depth`. Each
/// has a single feasible candidate. Calling DFS with `max_depth = 6` must
/// run DFS proper (NOT the BFS fallback) — `params.len() > max_depth` is
/// strict-greater, so `6 > 6` is false.
///
/// Pins the off-by-one boundary: `>` triggers fallback, `==` does not.
/// This test is the lower-bound mirror of step-29's upper-bound test.
///
/// Pins:
/// - `outcome.per_param.len() == 6` and every entry is `Selected`
/// - `outcome.substitution.len() == 6` in declared order
/// - zero `AutoTypeParamDepthBoundExceeded` diagnostics in the diagnostics
///   vector (the depth-bound branch must NOT fire when n == max_depth)
#[test]
fn dfs_at_max_depth_runs_dfs_no_fallback_diagnostic() {
    // Six distinct traits, each with a single implementing structure.
    let source = r#"
trait T1 {}
trait T2 {}
trait T3 {}
trait T4 {}
trait T5 {}
trait T6 {}

structure def S1 : T1 { param x : Real = 1.0 }
structure def S2 : T2 { param x : Real = 2.0 }
structure def S3 : T3 { param x : Real = 3.0 }
structure def S4 : T4 { param x : Real = 4.0 }
structure def S5 : T5 { param x : Real = 5.0 }
structure def S6 : T6 { param x : Real = 6.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params: Vec<AutoTypeParam> = (1..=6)
        .map(|i| AutoTypeParam {
            name: format!("P{}", i),
            bounds: vec![format!("T{}", i)],
            free: false,
            use_site_span: SourceSpan::new(10 * i, 10 * i + 5),
        })
        .collect();

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

    // Six Selected entries in declared order: P1 ↦ S1, …, P6 ↦ S6.
    assert_eq!(
        outcome.per_param.len(),
        6,
        "DFS at max_depth boundary must produce 6 per_param entries (no fallback truncation), got: {:?}",
        outcome.per_param
    );
    for (i, (name, sel)) in outcome.per_param.iter().enumerate() {
        let expected_param = format!("P{}", i + 1);
        let expected_struct = format!("S{}", i + 1);
        assert_eq!(name, &expected_param, "per_param[{}].0 must be {expected_param}", i);
        assert_eq!(
            sel,
            &SelectionResult::Selected(expected_struct.clone()),
            "per_param[{}].1 must be Selected({expected_struct})",
            i
        );
    }
    assert_eq!(
        outcome.substitution.len(),
        6,
        "DFS at max_depth boundary must produce 6 substitution entries, got: {:?}",
        outcome.substitution
    );

    // Critical assertion: NO `AutoTypeParamDepthBoundExceeded` diagnostic.
    // The depth-bound branch uses strict `>`, so `6 > 6` is false — the
    // search runs DFS proper, not the BFS fallback.
    let depth_bound_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .collect();
    assert!(
        depth_bound_diagnostics.is_empty(),
        "DFS at max_depth boundary (n == max_depth) must NOT emit AutoTypeParamDepthBoundExceeded \
         (strict `>`: only n > max_depth triggers fallback), got: {:?}",
        depth_bound_diagnostics
    );
}

// ─── step-32: depth-bound boundary — per_param shape discontinuity ──────────

/// Parity test for the `per_param` shape discontinuity at the DFS / BFS-fallback
/// boundary.
///
/// # What this test pins
///
/// `MultiParamResolutionOutcome.per_param` has different shapes at the
/// depth-bound boundary:
///
/// - **DFS path** (`n ≤ max_depth`): Phase A halt on a non-first param records
///   only the failing param — `[(U, NoCandidate)]` (length 1).  This is the
///   contract documented on `MultiParamResolutionOutcome.per_param` for the DFS
///   Phase A halt arms (see its doc-comment for the authoritative statement).
///
/// - **BFS-fallback path** (`n > max_depth`): delegates to
///   `resolve_auto_type_params` (v0.1), which uses halt-on-first-failure with
///   accumulation — `[(T, Selected("ORingSeal")), (U, NoCandidate)]` (length 2).
///   The failing-param entry is still the last entry, but prior successes are
///   included.
///
/// At the boundary itself (`n = max_depth` vs `n = max_depth + 1`) the caller
/// observes a SHAPE FLIP for the same Phase A failure mode.  This discontinuity
/// is intentional; it is documented on `MultiParamResolutionOutcome.per_param`
/// (task 2861 resolution: option (a) — doc-comment update, not algorithm change).
///
/// # Fixture
///
/// Same `[T:Seal→ORingSeal Found, U:Cooled→Empty]` pair used in
/// `dfs_phase_a_empty_pool_on_second_param_halts_against_second_param` (DFS
/// shape) and `mid_list_failure_records_success_then_failure_in_per_param_but_only_success_in_substitution`
/// in `auto_type_param_multi_param_tests.rs` (BFS shape).  Only `max_depth`
/// varies between the two runs (2 vs 1).
///
/// # Pins
///
/// - **DFS run** (`max_depth = 2`, params.len() == 2; strict `>` does NOT fire):
///   `per_param.len() == 1`, entry is `("U", NoCandidate)`.
///   Exactly one `AutoTypeParamNoCandidate` diagnostic, zero
///   `AutoTypeParamDepthBoundExceeded` diagnostics.
///
/// - **BFS-fallback run** (`max_depth = 1`, 2 > 1 ⇒ fallback fires):
///   `per_param.len() == 2`, entries are
///   `[("T", Selected("ORingSeal")), ("U", NoCandidate)]`.
///   `substitution == [("T", "ORingSeal")]`.
///   Exactly one `AutoTypeParamDepthBoundExceeded` (Warning) and one
///   `AutoTypeParamNoCandidate` diagnostic (2 total).
///
/// - **Shape divergence assertion**: the two `per_param.len()` values differ
///   (1 vs 2) — this is the canonical proof that the discontinuity exists and
///   is pinned, not accidentally unified by some future refactor.
///
/// - The failing-param entry `("U", NoCandidate)` is the last entry in BOTH
///   outcomes — the only universally-stable feature across the boundary.
///
/// # Out of scope: Overflow arm boundary parity
///
/// The struct-level doc on `MultiParamResolutionOutcome.per_param` lists both
/// `Empty` (NoCandidate) and `Overflow` (Ambiguous(overflow_vec)) as Phase A
/// halt arms with the same length-1 DFS shape.  This test covers only the
/// `Empty` arm.  The `Overflow` arm's DFS shape is covered by the standalone
/// DFS overflow tests (`dfs_phase_a_overflow_on_first_param_halts_before_recursion`,
/// etc.), and its shape is structurally identical to the Empty arm (length-1,
/// anchored on the failing param).  Pinning the `Overflow` arm at the
/// depth-bound boundary (`max_depth = n-1` vs `max_depth = n`) is therefore
/// intentionally left for a follow-up if the doc-contract or algorithm changes.
#[test]
fn dfs_phase_a_failure_at_depth_bound_boundary_documents_per_param_shape_discontinuity() {
    // T's bound (Seal) has one matching structure ⇒ Phase A returns Found.
    // U's bound (Cooled) has zero matching structures ⇒ Phase A returns Empty.
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

    // Distinct spans on T vs U — same as the sibling test.
    let t_span = SourceSpan::new(10, 20);
    let u_span = SourceSpan::new(30, 40);
    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()], // one structure ⇒ Found
            free: false,
            use_site_span: t_span,
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()], // zero structures ⇒ Empty
            free: false,
            use_site_span: u_span,
        },
    ];

    // ── DFS run: max_depth = 2, params.len() = 2 (strict `>` does NOT fire) ──
    let mut dfs_diagnostics = Vec::new();
    let dfs_outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        2, // 2 > 2 is false → DFS path
        &mut dfs_diagnostics,
    );

    assert_eq!(
        dfs_outcome.per_param,
        vec![("U".to_string(), SelectionResult::NoCandidate)],
        "DFS path (max_depth=2, n=2): per_param must be length-1 [(U, NoCandidate)]; \
         the Phase A halt arm records only the failing param, not prior Phase A Found entries"
    );
    assert!(
        dfs_outcome.substitution.is_empty(),
        "DFS path: substitution must be empty on Phase A halt, got: {:?}",
        dfs_outcome.substitution
    );

    let dfs_depth_bound_count = dfs_diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .count();
    assert_eq!(
        dfs_depth_bound_count, 0,
        "DFS path must emit zero AutoTypeParamDepthBoundExceeded diagnostics (no fallback); \
         got: {:?}",
        dfs_diagnostics
    );

    let dfs_no_candidate_count = dfs_diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate))
        .count();
    assert_eq!(
        dfs_no_candidate_count, 1,
        "DFS path must emit exactly one AutoTypeParamNoCandidate diagnostic; got: {:?}",
        dfs_diagnostics
    );

    // ── BFS-fallback run: max_depth = 1, params.len() = 2 (2 > 1 ⇒ fallback) ──
    let mut bfs_diagnostics = Vec::new();
    let bfs_fallback_outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        1, // 2 > 1 is true → BFS-fallback path
        &mut bfs_diagnostics,
    );

    assert_eq!(
        bfs_fallback_outcome.per_param,
        vec![
            ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
            ("U".to_string(), SelectionResult::NoCandidate),
        ],
        "BFS-fallback path (max_depth=1, n=2): per_param must be length-2 \
         [(T, Selected(\"ORingSeal\")), (U, NoCandidate)]; \
         BFS halt-on-first-failure accumulates prior successes"
    );
    assert_eq!(
        bfs_fallback_outcome.substitution,
        vec![("T".to_string(), "ORingSeal".to_string())],
        "BFS-fallback path: substitution must contain T's resolution, got: {:?}",
        bfs_fallback_outcome.substitution
    );

    let bfs_depth_bound_count = bfs_diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .count();
    assert_eq!(
        bfs_depth_bound_count, 1,
        "BFS-fallback path must emit exactly one AutoTypeParamDepthBoundExceeded diagnostic; \
         got: {:?}",
        bfs_diagnostics
    );
    let depth_bound_diag = bfs_diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .unwrap();
    assert_eq!(
        depth_bound_diag.severity,
        Severity::Warning,
        "AutoTypeParamDepthBoundExceeded must be Warning severity, got: {:?}",
        depth_bound_diag.severity
    );

    let bfs_no_candidate_count = bfs_diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate))
        .count();
    assert_eq!(
        bfs_no_candidate_count, 1,
        "BFS-fallback path must emit exactly one AutoTypeParamNoCandidate diagnostic; \
         got: {:?}",
        bfs_diagnostics
    );

    // ── Canonical shape-discontinuity assertion ──────────────────────────────
    // This is the central pin: the two per_param shapes ARE deliberately different.
    // A future refactor that accidentally unifies them will break this assertion.
    assert_ne!(
        dfs_outcome.per_param.len(),
        bfs_fallback_outcome.per_param.len(),
        "INTENTIONAL DISCONTINUITY: DFS (max_depth=2) produces per_param.len()={} \
         while BFS-fallback (max_depth=1) produces per_param.len()={} for the same fixture. \
         This is the documented depth-bound shape discontinuity — do not \"fix\" by unifying \
         the shapes without updating MultiParamResolutionOutcome.per_param's doc-comment \
         and the task-2861 design decision.",
        dfs_outcome.per_param.len(),
        bfs_fallback_outcome.per_param.len()
    );

    // The failing-param entry is the last entry in BOTH outcomes —
    // the only universally-stable feature across the boundary.
    let dfs_last = dfs_outcome.per_param.last().unwrap();
    let bfs_last = bfs_fallback_outcome.per_param.last().unwrap();
    assert_eq!(
        dfs_last,
        &("U".to_string(), SelectionResult::NoCandidate),
        "DFS: last per_param entry must be the failing param (U, NoCandidate)"
    );
    assert_eq!(
        bfs_last,
        &("U".to_string(), SelectionResult::NoCandidate),
        "BFS-fallback: last per_param entry must be the failing param (U, NoCandidate)"
    );
}

// ─── amend (post-verification): coverage gaps surfaced in code review ──────

/// Multi-param scenario where Phase A succeeds for every param but every
/// cross-product leaf is rejected by Phase B (`Satisfaction::Violated` on the
/// per-leaf check). Exercises the `0 =>` arm of the cross-product result match
/// in `resolve_auto_type_params_with_backtracking`: Phase A enumeration phase
/// completes for both T and U, recursion enters and visits every leaf, and
/// every leaf is infeasible — `feasible_assignments.len() == 0`. The
/// orchestrator then emits a single `AutoTypeParamNoCandidate` (zero-rejections
/// form) anchored on `params[0]` and produces
/// `per_param == [(T, NoCandidate)]`, `substitution == []`.
///
/// Distinct from the Phase A empty-pool halt
/// (`dfs_phase_a_empty_pool_on_first_param_halts_before_recursion`): there
/// the up-front Phase A enumeration loop short-circuits BEFORE recursion;
/// here recursion runs to completion and the `0 =>` arm fires AFTER the
/// cross-product is exhausted with zero feasibles. The two arms produce the
/// same outward `NoCandidate` shape but reach it through different paths,
/// and the branch is small but distinct enough to drift independently.
///
/// Pins:
/// - `per_param == [(T, NoCandidate)]` — single-entry, anchored on params[0]
/// - `substitution.is_empty()`
/// - exactly one `AutoTypeParamNoCandidate` diagnostic
/// - the diagnostic uses the zero-rejections message form (no
///   "rejected by constraint" suffix), confirming the `0 =>` arm reuses
///   `emit_no_candidate_zero_rejections` rather than a Phase C all-rejected
///   path
#[test]
fn dfs_multi_param_all_leaves_violated_emits_no_candidate_on_first_param() {
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

    // Parameterized template carries one top-level constraint so the checker's
    // Violated default produces non-empty `ConstraintResult`s in Phase B's
    // leaf check; without a constraint, `filter_feasible_candidates` would
    // never see a Violated and would trivially accept every leaf.
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Default Violated ⇒ every cross-product leaf is infeasible.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: true,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true,
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

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![("T".to_string(), SelectionResult::NoCandidate)],
            substitution: vec![],
        },
        "DFS with every cross-product leaf infeasible must emit NoCandidate against params[0] (cross-product `0 =>` arm)"
    );

    assert_eq!(
        diagnostics.len(),
        1,
        "DFS `0 =>` arm must emit exactly one diagnostic, got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "DFS `0 =>` arm diagnostic must be AutoTypeParamNoCandidate, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "AutoTypeParamNoCandidate must be Error severity"
    );
    // Zero-rejections message form: bound mentioned, no "rejected by constraint"
    // suffix (distinguishes the cross-product `0 =>` arm from a hypothetical
    // Phase C all-rejected path that would carry per-candidate rejection
    // detail). Pins reuse of `emit_no_candidate_zero_rejections`.
    assert!(
        diagnostics[0].message.contains("'Seal'"),
        "DFS `0 =>` arm diagnostic must mention params[0]'s bound 'Seal'; got: {:?}",
        diagnostics[0].message
    );
    assert!(
        !diagnostics[0].message.contains("rejected by constraint"),
        "DFS `0 =>` arm (zero-rejections form) must NOT mention 'rejected by constraint'; got: {:?}",
        diagnostics[0].message
    );
}

/// Multi-param strict-mode scenario where exactly one of the four cross-product
/// leaves is feasible. Exercises the `1 =>` arm via the strict path: with
/// `max_feasible_to_collect = 2` (strict mode), the search runs all four
/// leaves to confirm only one feasible exists, then takes the `1 =>` arm to
/// produce a `Selected` substitution.
///
/// Existing tests reach the `1 =>` arm only via free-mode early-termination
/// on the first feasible (`max_feasible_to_collect = 1`). A regression that
/// drops feasible assignments after the first in strict mode (e.g. a future
/// change that early-terminates strict mode after leaf 1 even before
/// confirming uniqueness) would not be caught by those tests.
///
/// Queue verdicts (in DFS visit order T outer, U inner):
/// 1. (ORingSeal, AirCooled) → `Violated`  (infeasible — backtrack)
/// 2. (ORingSeal, WaterCooled) → `Violated` (infeasible — backtrack)
/// 3. (RubberSeal, AirCooled) → `Violated` (infeasible — backtrack)
/// 4. (RubberSeal, WaterCooled) → `Satisfied` (feasible — recorded)
///
/// Strict mode requires confirming a SECOND feasible to declare Ambiguous;
/// the search exhausts the cross-product after leaf 4 with `feasible_assignments
/// .len() == 1`, so the `1 =>` arm fires and produces the `Selected` outcome
/// for the lone feasible leaf.
///
/// Pins:
/// - `per_param == [(T, Selected("RubberSeal")), (U, Selected("WaterCooled"))]`
/// - `substitution == [(T, "RubberSeal"), (U, "WaterCooled")]`
/// - zero diagnostics — the `1 =>` arm is a clean success; no Ambiguous /
///   NoCandidate emission paths fire
#[test]
fn dfs_strict_mode_with_exactly_one_feasible_leaf_selects_it() {
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

    // One constraint on the parameterized template so the checker's queue
    // verdict produces a non-empty `ConstraintResult` per leaf.
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Three leaves Violated (backtracked), one Satisfied (the lex-last leaf
    // (RubberSeal, WaterCooled) — visited at index 3 in DFS order, T outer).
    let checker = MockConstraintChecker::new().with_call_queue(vec![
        Satisfaction::Violated,
        Satisfaction::Violated,
        Satisfaction::Violated,
        Satisfaction::Satisfied,
    ]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: false, // strict ⇒ max_feasible_to_collect = 2
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

    assert_eq!(
        outcome,
        MultiParamResolutionOutcome {
            per_param: vec![
                ("T".to_string(), SelectionResult::Selected("RubberSeal".to_string())),
                ("U".to_string(), SelectionResult::Selected("WaterCooled".to_string())),
            ],
            substitution: vec![
                ("T".to_string(), "RubberSeal".to_string()),
                ("U".to_string(), "WaterCooled".to_string()),
            ],
        },
        "DFS strict-mode with exactly one feasible leaf must Select that leaf via the `1 =>` arm"
    );
    assert!(
        diagnostics.is_empty(),
        "DFS strict-mode `1 =>` arm (clean unique success) must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}

/// Phase A halt parity for failures discovered on the SECOND param (not the
/// first). Mirrors `dfs_phase_a_empty_pool_on_first_param_halts_before_recursion`
/// but with T succeeding enumeration (one matching structure) and U failing
/// (zero matching structures). Pins that the up-front per-param Phase A loop
/// terminates on the failure regardless of position, with the
/// `AutoTypeParamNoCandidate` diagnostic anchored on U's span (not T's).
///
/// Without this test, a future change to enumeration ordering or to the
/// halt arm could silently regress halt behavior for second-or-later
/// failures while keeping the first-param tests green.
///
/// Pins:
/// - DFS halts during the up-front Phase A enumeration loop (recursion never
///   starts because U's `Empty` arm `return`s immediately).
/// - `per_param == [(U, NoCandidate)]` — DFS records only the failing param
///   (consistent with the up-front Phase A loop's halt arms, which return
///   the single failing-param entry rather than accumulating prior Selected
///   entries; selection has not happened yet at this stage of DFS).
/// - `substitution.is_empty()`.
/// - exactly one `AutoTypeParamNoCandidate` diagnostic.
/// - the diagnostic's label anchors on U's `use_site_span` (not T's),
///   confirming the failure is attributed to the second param.
#[test]
fn dfs_phase_a_empty_pool_on_second_param_halts_against_second_param() {
    // T's bound (Seal) has one matching structure ⇒ Phase A returns Found.
    // U's bound (Cooled) has zero matching structures ⇒ Phase A returns Empty.
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

    // Distinct spans on T vs U so the diagnostic-label assertion can
    // unambiguously confirm the failure is anchored on U.
    let t_span = SourceSpan::new(10, 20);
    let u_span = SourceSpan::new(30, 40);
    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()], // one structure ⇒ Found
            free: false,
            use_site_span: t_span,
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()], // zero structures ⇒ Empty
            free: false,
            use_site_span: u_span,
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
            per_param: vec![("U".to_string(), SelectionResult::NoCandidate)],
            substitution: vec![],
        },
        "DFS Phase A empty-pool on the second param must halt with per_param=[(U, NoCandidate)], substitution=[]"
    );

    assert_eq!(
        diagnostics.len(),
        1,
        "DFS Phase A second-param empty-pool halt must emit exactly one diagnostic, got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "DFS Phase A second-param empty-pool diagnostic must be AutoTypeParamNoCandidate"
    );
    assert!(
        diagnostics[0].message.contains("'Cooled'"),
        "DFS second-param empty-pool diagnostic must mention U's bound 'Cooled' (not T's 'Seal'); got: {:?}",
        diagnostics[0].message
    );
    // Anchor parity: the label must use U's span, confirming the failure is
    // attributed to the second param.
    assert_eq!(
        diagnostics[0].labels.len(),
        1,
        "DFS Phase A second-param empty-pool diagnostic must carry exactly one label"
    );
    assert_eq!(
        diagnostics[0].labels[0].span, u_span,
        "DFS second-param empty-pool diagnostic label must anchor on U's span (not T's)"
    );
}

/// Phase A halt parity for **Overflow** failures discovered on the SECOND
/// param (not the first). Mirrors
/// `dfs_phase_a_empty_pool_on_second_param_halts_against_second_param` for
/// the `Overflow` arm of the orchestrator's up-front per-param Phase A
/// enumeration loop, and mirrors
/// `dfs_phase_a_overflow_on_first_param_halts_before_recursion` for param
/// position.
///
/// **Why this test exists**: The Empty and Overflow arms are syntactically
/// symmetric in the `for param in params` loop but take diverging code paths:
/// - `Empty` arm: calls `emit_no_candidate_zero_rejections` to push
///   the diagnostic itself, then `return`s `[(name, NoCandidate)]`.
/// - `Overflow` arm: does NOT push a diagnostic — `enumerate_candidates`
///   already pushed `AutoTypeParamPoolOverflow` with the failing param's
///   `use_site_span`. It only synthesizes
///   `[(name, Ambiguous(overflow_vec))]` and `return`s.
///
/// Without this test, a future change to the Overflow arm (e.g., accidentally
/// resetting `overflow_vec` on non-first params, using `params[0].use_site_span`
/// instead of `param.use_site_span`, or skipping the `return`) could silently
/// regress overflow halt behavior for second-or-later failures while keeping
/// all first-param overflow tests green.
///
/// **Fixture asymmetry**: T's bound is `"Cooled"` (1 structure → Phase A
/// `Found`), and U's bound is `"Seal"` (MAX+1 structures → Phase A `Overflow`).
/// This reverses the trait→param mapping vs the empty-pool sibling (which has
/// T=Seal-1 and U=Cooled-0), but the test asserts on param POSITION and param
/// NAME — not trait name — so the reversal is contract-equivalent.
/// `build_n_seal_structures(MAX+1)` is reused for U's overflow pool; one
/// inline `AirCooled : Cooled` structure provides T's single-candidate pool.
///
/// Pins:
/// - `per_param == [(U, Ambiguous(_))]` — length 1, only the failing param
///   recorded (halt before recursion, before any T selection). Overflow maps
///   to `Ambiguous` (not `NoCandidate`) to distinguish it from an empty pool.
/// - `substitution.is_empty()`.
/// - exactly one `AutoTypeParamPoolOverflow` diagnostic (U's overflow;
///   T enumerated successfully so no T diagnostic is emitted).
/// - the diagnostic's label anchors on U's `use_site_span` (not T's) —
///   the critical anchor-parity assertion that distinguishes second-param
///   overflow from a regression anchored on `params[0]`.
#[test]
fn dfs_phase_a_overflow_on_second_param_halts_against_second_param() {
    // U's bound (Seal) has MAX+1 matching structures ⇒ Phase A returns Overflow.
    // T's bound (Cooled) has one matching structure ⇒ Phase A returns Found.
    // Note: T is enumerated first, but the Overflow on U must still halt
    // with only U's entry in per_param and U's span in the diagnostic label.
    let mut source = build_n_seal_structures(MAX_AUTO_TYPE_PARAM_CANDIDATES + 1);
    source.push_str(
        r#"trait Cooled {}
structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}
"#,
    );
    let module = parse_and_compile(&source);
    let (template_registry, trait_registry) = build_registries(&module);

    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    // Distinct spans on T vs U so the diagnostic-label assertion can
    // unambiguously confirm the failure is anchored on U.
    let t_span = SourceSpan::new(10, 20);
    let u_span = SourceSpan::new(30, 40);
    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Cooled".to_string()], // one structure ⇒ Found
            free: false,
            use_site_span: t_span,
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Seal".to_string()], // MAX+1 structures ⇒ Overflow
            free: false,
            use_site_span: u_span,
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

    // Only the failing (second) param recorded; it overflowed → Ambiguous.
    assert_eq!(
        outcome.per_param.len(),
        1,
        "DFS overflow on second param must halt: per_param must have exactly 1 entry, got: {:?}",
        outcome.per_param
    );
    assert_eq!(
        outcome.per_param[0].0, "U",
        "per_param entry must be for the failing param 'U', not 'T'"
    );
    assert!(
        matches!(outcome.per_param[0].1, SelectionResult::Ambiguous(_)),
        "DFS Phase A overflow maps to Ambiguous; got: {:?}",
        outcome.per_param[0].1
    );
    assert!(
        outcome.substitution.is_empty(),
        "DFS overflow on second param must yield empty substitution, got: {:?}",
        outcome.substitution
    );

    // Exactly one diagnostic: the overflow anchored on U (no T diagnostic,
    // and no second-param recursion was entered).
    assert_eq!(
        diagnostics.len(),
        1,
        "DFS exactly one overflow diagnostic expected (T enumerated OK, recursion not entered), got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamPoolOverflow),
        "DFS diagnostic must be AutoTypeParamPoolOverflow, got: {:?}",
        diagnostics[0].code
    );
    // Anchor parity: the label must use U's span, confirming the failure is
    // attributed to the second param (not T's span, which would indicate a
    // regression that used params[0].use_site_span everywhere).
    assert_eq!(
        diagnostics[0].labels.len(),
        1,
        "DFS Phase A second-param overflow diagnostic must carry exactly one label"
    );
    assert_eq!(
        diagnostics[0].labels[0].span, u_span,
        "DFS second-param overflow diagnostic label must anchor on U's span (not T's)"
    );
}

// ─── Regression: per-leaf "exactly one check() call" with multi-constraint ──

/// Pins the invariant that `dfs_leaf_feasible` invokes `constraint_checker.check()`
/// exactly **once** per leaf — even when the parameterized template carries
/// **multiple** top-level constraints.
///
/// Setup: single `auto:` param `T : Seal` with two candidates (`ORingSeal`,
/// `RubberSeal`), so there are exactly 2 DFS leaves. The parameterized
/// template carries **two** top-level constraints (indices 0 and 1). Both
/// params are `free = true` so the second feasible terminates the search.
///
/// `MockConstraintChecker::with_call_queue(vec![Violated, Satisfied])` places
/// exactly 2 items in the queue — one per expected leaf. If the implementation
/// ever changed to call `check()` once per constraint instead of once per leaf,
/// the queue (length 2) would drain after the first constraint of leaf 1 and
/// the remaining calls would fall back to the default (`Satisfied`), changing
/// the lex-first selection from `RubberSeal` back to `ORingSeal` — an
/// observable test failure.
///
/// Expected outcome:
/// - Leaf 1 (`ORingSeal`): queue pop #1 ⇒ `Violated` broadcast to both
///   constraints → infeasible → backtrack.
/// - Leaf 2 (`RubberSeal`): queue pop #2 ⇒ `Satisfied` broadcast to both
///   constraints → feasible → selected.
/// - `substitution == [("T", "RubberSeal")]`, `diagnostics.is_empty()`.
#[test]
fn dfs_leaf_invokes_constraint_checker_exactly_once_per_leaf_with_multi_constraint_template() {
    let source = r#"
trait Seal {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def RubberSeal : Seal {
    param thickness : Real = 2.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Two top-level constraints on the parameterized template.
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr.clone())
        .constraint("Coupling", 1, None, expr)
        .build();

    // Exactly 2 queue items for 2 leaves. One check() call per leaf
    // broadcasts the queued verdict to all constraints in that call.
    let checker = MockConstraintChecker::new()
        .with_call_queue(vec![Satisfaction::Violated, Satisfaction::Satisfied]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![AutoTypeParam {
        name: "T".to_string(),
        bounds: vec!["Seal".to_string()],
        free: true, // free ⇒ stop at first feasible (lex-first, which is RubberSeal here)
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
            per_param: vec![("T".to_string(), SelectionResult::Selected("RubberSeal".to_string()))],
            substitution: vec![("T".to_string(), "RubberSeal".to_string())],
        },
        "With multi-constraint template and queue [Violated, Satisfied], DFS must backtrack \
         from ORingSeal (leaf 1 = Violated) and select RubberSeal (leaf 2 = Satisfied)"
    );
    assert!(
        diagnostics.is_empty(),
        "Multi-constraint backtracking happy path must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── Regression: per-leaf "exactly one check() call" — DFS multi-param path ──

/// Pins the invariant that `dfs_leaf_feasible` invokes `constraint_checker.check()`
/// exactly **once** per leaf when the orchestrator routes through the true
/// multi-param DFS branch (`params.len() >= 2`).
///
/// **Coverage gap closed by this test:** the step-3 test
/// (`dfs_leaf_invokes_constraint_checker_exactly_once_per_leaf_with_multi_constraint_template`)
/// uses `params.len() == 1`, which triggers the early-return at
/// `resolve_auto_type_params_with_backtracking::{single-param branch}` and
/// routes through `filter_feasible_candidates` — so `dfs_search`,
/// `dfs_leaf_feasible`, and `check_constraints_violated` are **never** called
/// by that test. This test uses `params.len() == 2` to exercise the actual
/// DFS path.
///
/// Setup:
/// - Four structures: `ORingSeal : Seal`, `RubberSeal : Seal`,
///   `AirCooled : Cooled`, `WaterCooled : Cooled`.
/// - Two `AutoTypeParam`s: `T : Seal` (free=true) and `U : Cooled` (free=true).
///   With two params the orchestrator enters `dfs_search` → `dfs_leaf_feasible`
///   → `check_constraints_violated` for all 4 cross-product leaves.
/// - Parameterized template carries **two** top-level constraints (indices 0
///   and 1) — the multi-constraint shape is the regression target.
/// - `MockConstraintChecker::with_call_queue(vec![Violated, Violated, Violated, Satisfied])`
///   — exactly 4 queue items for 4 expected leaves.
///
/// Expected DFS visit order (T outer × U inner, lex within each level):
/// 1. `(T=ORingSeal, U=AirCooled)` → queue pop #1 ⇒ `Violated` broadcast
///    to both constraints → infeasible → backtrack at U-level.
/// 2. `(T=ORingSeal, U=WaterCooled)` → queue pop #2 ⇒ `Violated` broadcast
///    → infeasible → backtrack at T-level.
/// 3. `(T=RubberSeal, U=AirCooled)` → queue pop #3 ⇒ `Violated` broadcast
///    → infeasible → backtrack at U-level.
/// 4. `(T=RubberSeal, U=WaterCooled)` → queue pop #4 ⇒ `Satisfied` broadcast
///    → feasible → record, free-mode early-terminate.
///
/// **Why this catches the regression:** if `dfs_leaf_feasible` were changed
/// to call `check()` once per constraint (instead of once per leaf), then
/// with 2 constraints × 4 leaves = 8 calls the queue (length 4) would drain
/// after 2 leaves, and the remaining calls would fall back to default
/// `Satisfied`. Leaf 3 `(RubberSeal, AirCooled)` would then appear feasible,
/// changing the lex-first selection from `(RubberSeal, WaterCooled)` to
/// `(RubberSeal, AirCooled)` — an observable assertion failure.
#[test]
fn dfs_leaf_invokes_constraint_checker_exactly_once_per_leaf_with_multi_constraint_template_two_params(
) {
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

    // Two top-level constraints — the multi-constraint shape is the regression
    // target. The mock ignores expression content; literals are only needed so
    // the builder has a value to store.
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr.clone())
        .constraint("Coupling", 1, None, expr)
        .build();

    // Exactly 4 queue items for 4 cross-product leaves. One check() call per
    // leaf broadcasts the queued verdict to ALL constraints in that call — so
    // the queue drains at rate 1 per leaf regardless of constraint count.
    let checker = MockConstraintChecker::new().with_call_queue(vec![
        Satisfaction::Violated,
        Satisfaction::Violated,
        Satisfaction::Violated,
        Satisfaction::Satisfied,
    ]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    // Two params → orchestrator routes through the multi-param DFS branch,
    // exercising dfs_leaf_feasible and check_constraints_violated for real.
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
                ("T".to_string(), SelectionResult::Selected("RubberSeal".to_string())),
                ("U".to_string(), SelectionResult::Selected("WaterCooled".to_string())),
            ],
            substitution: vec![
                ("T".to_string(), "RubberSeal".to_string()),
                ("U".to_string(), "WaterCooled".to_string()),
            ],
        },
        "With multi-constraint template and queue [Violated×3, Satisfied], DFS must \
         visit all 4 cross-product leaves in lex order, backtrack from (ORingSeal,*) \
         and (RubberSeal,AirCooled), and select (RubberSeal, WaterCooled) as the only \
         feasible leaf"
    );
    assert!(
        diagnostics.is_empty(),
        "Multi-constraint two-param backtracking happy path must emit zero diagnostics, got: {:?}",
        diagnostics
    );
}

// ─── step-3 (task 2661): mixed strict+free ≥2 feasibles → Ambiguous, not NonUnique ──

/// Regression test: when ANY param is strict (`free=false`), ≥2 cross-product
/// feasibles must produce `AutoTypeParamAmbiguous` (Error), NOT
/// `AutoTypeParamNonUnique` (Warning).
///
/// Fixture: 2 params `[T:Seal (free=false STRICT), U:Cooled (free=true)]` with
/// 2 candidates each (4 cross-product leaves). Default `MockConstraintChecker`
/// (every leaf trivially feasible, no constraint on template) → 4 feasibles
/// found by the DFS (strict mode, `max_feasible_to_collect=2`; stops at 2).
///
/// Design decision: `any_strict = params.iter().any(|p| !p.free)`. If ANY param
/// is strict, the cross-product must be uniquely determined for compilation to
/// succeed; ambiguity is an error. The new all-free NonUnique path only fires
/// when `!any_strict`.
///
/// Pins:
/// (a) `diagnostics.len() == 1`, code `AutoTypeParamAmbiguous`, severity `Error`
///     (NOT `AutoTypeParamNonUnique` Warning)
/// (b) `outcome.per_param.len() == 1` — Ambiguous shape (length-1, anchored on T)
/// (c) `outcome.per_param[0].1` is `SelectionResult::Ambiguous(_)` with 2 witnesses
/// (d) `outcome.substitution.is_empty()`
///
/// This test should PASS immediately after step-2's impl (the dispatch is
/// already in place); it exists as a contract pin so a future refactor cannot
/// accidentally collapse the strict/free branches.
#[test]
fn dfs_mixed_strict_and_free_with_two_feasibles_emits_ambiguous_not_non_unique() {
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

    // No constraints on the template → every leaf trivially feasible.
    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Satisfied);
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: false, // ← STRICT: any_strict = true → Ambiguous path
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true, // free, but any_strict is already true because of T
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

    // (a) Exactly one Ambiguous Error — NOT a NonUnique Warning.
    assert_eq!(
        diagnostics.len(),
        1,
        "mixed strict/free ≥2 feasibles must emit exactly one diagnostic, got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamAmbiguous),
        "mixed strict/free diagnostic must be AutoTypeParamAmbiguous (Error), \
         NOT AutoTypeParamNonUnique (Warning); got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Error,
        "AutoTypeParamAmbiguous must be Error severity (not Warning)"
    );
    // (b) Ambiguous shape: length-1 per_param anchored on T (params[0]).
    assert_eq!(
        outcome.per_param.len(),
        1,
        "mixed strict/free Ambiguous must produce length-1 per_param (not length-2 success shape); \
         got: {:?}",
        outcome.per_param
    );
    assert_eq!(
        outcome.per_param[0].0, "T",
        "Ambiguous per_param entry must be anchored on params[0].name ('T')"
    );
    // (c) SelectionResult::Ambiguous with 2 witnesses (strict-mode cap).
    match &outcome.per_param[0].1 {
        SelectionResult::Ambiguous(witnesses) => {
            assert_eq!(
                witnesses.len(),
                2,
                "strict-mode Ambiguous must carry exactly 2 witnesses \
                 (max_feasible_to_collect=2 cap); got: {:?}",
                witnesses
            );
        }
        other => panic!(
            "mixed strict/free ≥2 feasibles must produce SelectionResult::Ambiguous, got: {:?}",
            other
        ),
    }
    // (d) Empty substitution on Ambiguous.
    assert!(
        outcome.substitution.is_empty(),
        "mixed strict/free Ambiguous must yield empty substitution; got: {:?}",
        outcome.substitution
    );
}

// ─── step-4 (task 2661): >16 feasibles → NonUnique with elision count ──────────

/// Two `AutoTypeParam`s `[T:Seal (free), U:Cooled (free)]` with 5 candidates
/// each (25 cross-product leaves). Default `MockConstraintChecker` (every leaf
/// trivially feasible) → 25 feasibles.
///
/// With `DISPLAY_CAP = 16`, expected elision count = 25 - 16 = 9.
///
/// **Current behavior (step-2 impl, pre-step-5):** the NonUnique branch was
/// added but includes ALL 25 witnesses without the elision logic.  This test
/// FAILS because `message.contains("9 more elided")` is not satisfied.
///
/// Pins:
/// (a) `diagnostics.len() == 1`, code `AutoTypeParamNonUnique`, severity Warning
/// (b) `message.contains("9 more elided")` — exact elision count substring
/// (c) `message.contains("ORingSeal")` — lex-first T candidate present
/// (d) `message.contains("AirCooled")` — lex-first U candidate present
/// (e) `outcome.per_param.len() == 2`, each entry `Selected`
/// (f) `outcome.per_param[0]` is `(T_name, Selected(lex-first-T))`
/// (g) `outcome.substitution.len() == 2`
#[test]
fn dfs_free_mode_more_than_sixteen_feasibles_emits_non_unique_with_elision_count() {
    // 5 Seal structures (alphabetical order matters for lex-first):
    //   ORingSeal < RubberSeal < SilicaSeal < TeflonSeal < UretheSeal
    // 5 Cooled structures:
    //   AirCooled < ForcedConvection < LiquidCooled < NaturalConvection < WaterCooled
    // → 5×5 = 25 cross-product leaves, all trivially feasible (default Satisfied).
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}
structure def RubberSeal : Seal {
    param thickness : Real = 2.0
}
structure def SilicaSeal : Seal {
    param hardness : Real = 7.0
}
structure def TeflonSeal : Seal {
    param friction : Real = 0.1
}
structure def UretheSeal : Seal {
    param elasticity : Real = 3.0
}

structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}
structure def ForcedConvection : Cooled {
    param fan_speed : Real = 3000.0
}
structure def LiquidCooled : Cooled {
    param coolant_flow : Real = 8.0
}
structure def NaturalConvection : Cooled {
    param fin_area : Real = 0.05
}
structure def WaterCooled : Cooled {
    param flow_rate : Real = 12.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // No constraints → all 25 leaves trivially feasible.
    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Satisfied);
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: true,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true,
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

    // (a) Exactly one NonUnique Warning.
    assert_eq!(
        diagnostics.len(),
        1,
        "25 feasibles must emit exactly one AutoTypeParamNonUnique diagnostic, got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique; got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "AutoTypeParamNonUnique must be Warning severity"
    );
    // (b) Exact elision count: 25 - 16 = 9.
    assert!(
        diagnostics[0].message.contains("9 more elided"),
        "message must contain '9 more elided' (25 - DISPLAY_CAP(16) = 9); got: {:?}",
        diagnostics[0].message
    );
    // (b2) Exactly DISPLAY_CAP=16 witnesses are rendered — not more, not fewer.
    // Count `; T=` separators: witnesses are joined by `"; "` so inter-witness
    // separators = (witness count - 1).  With 16 displayed witnesses the
    // separator count is 15.  (The `selected lexicographically-first '...'`
    // suffix also contains `T=` but is preceded by `; selected`, not `; T=`,
    // so it is not counted here.)
    {
        let sep_count = diagnostics[0]
            .message
            .matches("; T=")
            .count();
        assert_eq!(
            sep_count,
            15,
            "message must contain exactly 15 '; T=' separators (== DISPLAY_CAP-1 = 15 \
             inter-witness separators for 16 displayed witnesses), \
             got {} — a regression here means the display window was not applied; message: {:?}",
            sep_count,
            diagnostics[0].message
        );
    }
    // (c) Lex-first T candidate appears in the message.
    assert!(
        diagnostics[0].message.contains("ORingSeal"),
        "message must contain lex-first T candidate 'ORingSeal'; got: {:?}",
        diagnostics[0].message
    );
    // (d) Lex-first U candidate appears in the message.
    assert!(
        diagnostics[0].message.contains("AirCooled"),
        "message must contain lex-first U candidate 'AirCooled'; got: {:?}",
        diagnostics[0].message
    );
    // (e) Full success shape: 2 per_param entries.
    assert_eq!(
        outcome.per_param.len(),
        2,
        "25-feasible outcome must have per_param.len() == 2 (success shape); got: {:?}",
        outcome.per_param
    );
    // (f) First param maps to lex-first T candidate.
    assert_eq!(
        outcome.per_param[0],
        ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
        "per_param[0] must be (T, Selected(ORingSeal)) — lex-first T"
    );
    // (g) Full substitution Vec.
    assert_eq!(
        outcome.substitution.len(),
        2,
        "25-feasible outcome must have substitution.len() == 2; got: {:?}",
        outcome.substitution
    );
    assert_eq!(
        outcome.substitution[0],
        ("T".to_string(), "ORingSeal".to_string()),
        "substitution[0] must be (T, ORingSeal)"
    );
}

// ─── step-6 (task 2661): exactly 16 feasibles → no elision marker ──────────────

/// Two `AutoTypeParam`s `[T:Seal (free), U:Cooled (free)]` with 4 candidates
/// each (16 cross-product leaves). Default `MockConstraintChecker` (all
/// feasible) → exactly 16 feasibles.
///
/// With `DISPLAY_CAP = 16` and `total = 16`: `elided = 16.saturating_sub(16) = 0`
/// → the elision marker must NOT appear in the message.
///
/// This is the off-by-one boundary test: `total > DISPLAY_CAP` (equivalently
/// `elided > 0`) must use strict `>`, not `>=`, so that exactly 16 feasibles
/// produces no elision.
///
/// Pins:
/// (a) `diagnostics.len() == 1`, code `AutoTypeParamNonUnique`, severity Warning
/// (b) `!message.contains("elided")` — boundary must NOT produce the elision marker
/// (c) `!message.contains("more elided")` — belt-and-suspenders
/// (d) `message.contains("ORingSeal")` — lex-first T candidate present
#[test]
fn dfs_free_mode_exactly_sixteen_feasibles_emits_non_unique_without_elision_marker() {
    // 4 Seal structures (lex order: ORingSeal < RubberSeal < SilicaSeal < TeflonSeal)
    // 4 Cooled structures (lex order: AirCooled < ForcedConvection < LiquidCooled < WaterCooled)
    // → 4×4 = 16 cross-product leaves, all trivially feasible (default Satisfied).
    let source = r#"
trait Seal {}
trait Cooled {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}
structure def RubberSeal : Seal {
    param thickness : Real = 2.0
}
structure def SilicaSeal : Seal {
    param hardness : Real = 7.0
}
structure def TeflonSeal : Seal {
    param friction : Real = 0.1
}

structure def AirCooled : Cooled {
    param flow_rate : Real = 5.0
}
structure def ForcedConvection : Cooled {
    param fan_speed : Real = 3000.0
}
structure def LiquidCooled : Cooled {
    param coolant_flow : Real = 8.0
}
structure def WaterCooled : Cooled {
    param flow_rate : Real = 12.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // No constraints → all 16 leaves trivially feasible.
    let template = TopologyTemplateBuilder::new("Coupling").build();
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Satisfied);
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: true,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true,
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

    // (a) Exactly one NonUnique Warning.
    assert_eq!(
        diagnostics.len(),
        1,
        "16 feasibles must emit exactly one AutoTypeParamNonUnique diagnostic, got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique; got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "AutoTypeParamNonUnique must be Warning severity"
    );
    // (b) Boundary: exactly 16 feasibles (= DISPLAY_CAP) must NOT produce the elision marker.
    assert!(
        !diagnostics[0].message.contains("elided"),
        "boundary case (16 = DISPLAY_CAP): message must NOT contain 'elided' \
         (elision only fires when total > DISPLAY_CAP); got: {:?}",
        diagnostics[0].message
    );
    // (c) Belt-and-suspenders: no "more elided" substring either.
    assert!(
        !diagnostics[0].message.contains("more elided"),
        "boundary case (16 = DISPLAY_CAP): message must NOT contain 'more elided'; got: {:?}",
        diagnostics[0].message
    );
    // (d) Lex-first T candidate present in message.
    assert!(
        diagnostics[0].message.contains("ORingSeal"),
        "message must contain lex-first T candidate 'ORingSeal'; got: {:?}",
        diagnostics[0].message
    );
    // Success shape: 2 per_param entries, lex-first selected.
    assert_eq!(outcome.per_param.len(), 2, "success shape must have 2 per_param entries");
    assert_eq!(
        outcome.per_param[0],
        ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
        "per_param[0] must be (T, Selected(ORingSeal))"
    );
}

// ─── step-1 (task 2661): free-mode ≥2 cross-product feasibles ─────────────────
// → NonUnique Warning + lex-first success shape

/// Two `AutoTypeParam`s `[T : Seal (free=true), U : Cooled (free=true)]` with
/// 2 candidates each (4 cross-product leaves). A single constraint is added to
/// the template so the `MockConstraintChecker`'s queue fires per-leaf.
///
/// Queue `[Satisfied, Violated, Satisfied, Violated]` drives:
/// - Leaf 1 `(ORingSeal, AirCooled)`   → Satisfied → **feasible**
/// - Leaf 2 `(ORingSeal, WaterCooled)` → Violated  → infeasible
/// - Leaf 3 `(RubberSeal, AirCooled)`  → Satisfied → **feasible**
/// - Leaf 4 `(RubberSeal, WaterCooled)`→ Violated  → infeasible
///
/// → exactly 2 cross-product feasibles; lex-first = `(ORingSeal, AirCooled)`.
///
/// Task 2661 behavior: free-mode collects ALL feasible leaves
/// (`max_feasible_to_collect = usize::MAX`), emits one `AutoTypeParamNonUnique`
/// Warning, and returns the lex-first feasible in a full success shape.
///
/// Pins:
/// (a) `per_param == [(T, Selected("ORingSeal")), (U, Selected("AirCooled"))]`
///     — full length-N success shape, lex-first selected.
/// (b) `substitution == [(T, "ORingSeal"), (U, "AirCooled")]`
/// (c) `diagnostics.len() == 1`, code `AutoTypeParamNonUnique`, severity `Warning`
/// (d) `diagnostics[0].candidates == ["ORingSeal", "AirCooled"]`
///     — FQN-only invariant: bare FQNs of the lex-first leaf (task 2860)
/// (e) message contains "ORingSeal", "RubberSeal", "AirCooled"
///     (both feasible witnesses are rendered)
#[test]
fn dfs_free_mode_two_feasible_cross_products_emits_non_unique_warning_and_picks_lex_first() {
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

    // One constraint on the template so the per-call queue verdict fires at
    // each leaf (with an empty constraints slice, `check()` would never be
    // called and every leaf would be trivially feasible regardless of the queue).
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Queue [S, V, S, V]: leaves 1 and 3 feasible, leaves 2 and 4 infeasible.
    // → 2 cross-product feasibles: (ORingSeal, AirCooled) and (RubberSeal, AirCooled).
    // lex-first = (ORingSeal, AirCooled) (first discovered in DFS order).
    let checker = MockConstraintChecker::new().with_call_queue(vec![
        Satisfaction::Satisfied,
        Satisfaction::Violated,
        Satisfaction::Satisfied,
        Satisfaction::Violated,
    ]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec!["Seal".to_string()],
            free: true,
            use_site_span: SourceSpan::new(10, 20),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec!["Cooled".to_string()],
            free: true,
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

    // (a) Full N-length per_param — success shape, not length-1 Ambiguous shape.
    assert_eq!(
        outcome.per_param,
        vec![
            ("T".to_string(), SelectionResult::Selected("ORingSeal".to_string())),
            ("U".to_string(), SelectionResult::Selected("AirCooled".to_string())),
        ],
        "all-free ≥2 NonUnique path must produce length-2 per_param with Selected \
         entries (success shape, not Ambiguous); got: {:?}",
        outcome.per_param
    );
    // (b) Full substitution Vec in declared order.
    assert_eq!(
        outcome.substitution,
        vec![
            ("T".to_string(), "ORingSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
        ],
        "all-free ≥2 NonUnique path must produce full substitution Vec; got: {:?}",
        outcome.substitution
    );
    // (c) Exactly one NonUnique Warning.
    assert_eq!(
        diagnostics.len(),
        1,
        "all-free ≥2 feasibles must emit exactly one diagnostic, got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "all-free ≥2 diagnostic must be AutoTypeParamNonUnique, got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "AutoTypeParamNonUnique must be Warning severity (not Error)"
    );
    // (d) FQN-only candidates invariant: lex-first leaf's bare FQN list.
    assert_eq!(
        diagnostics[0].candidates,
        vec!["ORingSeal".to_string(), "AirCooled".to_string()],
        "Diagnostic.candidates must be the lex-first leaf's bare FQN list \
         (FQN-only invariant, task 2860); got: {:?}",
        diagnostics[0].candidates
    );
    // Candidates must be bare FQNs — no '=' or ',' composite tuples.
    for entry in &diagnostics[0].candidates {
        assert!(
            !entry.contains('='),
            "Diagnostic.candidates entries must be bare FQNs (no '='); got: {:?}",
            entry
        );
        assert!(
            !entry.contains(','),
            "Diagnostic.candidates entries must be bare FQNs (no ','); got: {:?}",
            entry
        );
    }
    // (e) Both feasible witnesses appear in the message body.
    assert!(
        diagnostics[0].message.contains("ORingSeal"),
        "message must mention ORingSeal (from lex-first witness); got: {:?}",
        diagnostics[0].message
    );
    assert!(
        diagnostics[0].message.contains("RubberSeal"),
        "message must mention RubberSeal (from second feasible witness); got: {:?}",
        diagnostics[0].message
    );
    assert!(
        diagnostics[0].message.contains("AirCooled"),
        "message must mention AirCooled (appears in both feasible witnesses); got: {:?}",
        diagnostics[0].message
    );
}

// ─── step-1 (task 2660): build_constraint_blame_map — basic TypeParam blame ──

/// `build_constraint_blame_map` must return one entry per constraint that
/// references at least one in-scope `TypeParam`-typed cell. The entry maps
/// the `ConstraintNodeId` to the `BTreeSet<usize>` of referenced param indices.
///
/// Setup: two cells (`field_t : TypeParam("T")`, `field_u : TypeParam("U")`),
/// one `BinOp(Eq)` constraint whose `ValueRef`s address both cells.
/// `params = [T(idx=0), U(idx=1)]` → blame set = `{0, 1}`.
///
/// Pins the "at least one TypeParam ref → entry present" half of the contract.
/// The "no ref → absent" half is pinned by
/// `build_constraint_blame_map_excludes_out_of_scope_type_params_and_no_typeparam_constraints`.
#[test]
fn build_constraint_blame_map_returns_param_indices_referenced_by_constraint_expression() {
    let field_t = ValueCellId::new("Coupling", "field_t");
    let field_u = ValueCellId::new("Coupling", "field_u");
    let expr = CompiledExpr::binop(
        BinOp::Eq,
        CompiledExpr::value_ref(field_t.clone(), Type::TypeParam("T".into())),
        CompiledExpr::value_ref(field_u.clone(), Type::TypeParam("U".into())),
        Type::Bool,
    );
    let template = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .param("Coupling", "field_u", Type::TypeParam("U".into()), None)
        .constraint("Coupling", 0, None, expr)
        .build();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec![],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec![],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
    ];

    let map = build_constraint_blame_map(&template, &params);

    assert_eq!(
        map.len(),
        1,
        "expect exactly one entry (one constraint with TypeParam refs); got: {:?}",
        map
    );
    let cid = ConstraintNodeId::new("Coupling", 0);
    assert_eq!(
        map.get(&cid).cloned().unwrap_or_default(),
        BTreeSet::from([0_usize, 1_usize]),
        "constraint referencing both T(idx=0) and U(idx=1) cells must map to {{0, 1}}"
    );
}

// ─── step-3 (task 2660): build_constraint_blame_map — exclusion invariants ──

/// `build_constraint_blame_map` must NOT insert an entry for constraints whose
/// blame set would be empty. Two sub-cases:
///
/// (a) A cell typed `Type::TypeParam("Z")` where `Z` is NOT in `params=[T,U]`
///     contributes nothing — the constraint that only ValueRefs that cell must
///     be absent from the result map.
///
/// (b) A constraint whose expression is `CompiledExpr::literal(Value::Bool(true),
///     Type::Bool)` (no ValueRef, no TypeParam anywhere) is also absent.
///
/// Setup: three cells (`field_t:T`, `field_u:U`, `field_z:Z`), two constraints:
/// - c0: `ValueRef(field_z)` only  → blame={} (Z ∉ params) → absent
/// - c1: `Bool(true)` literal      → blame={} (no ValueRef)  → absent
///
/// Pins the "empty blame → absent" invariant the DFS recursion relies on:
/// `compute_deepest_blame_level` returns `None` for absent constraints and falls
/// back to ordinary backtracking, so an accidental `map.insert(id, BTreeSet::new())`
/// would incorrectly block backjumping even when no TypeParam blame exists.
#[test]
fn build_constraint_blame_map_excludes_out_of_scope_type_params_and_no_typeparam_constraints() {
    let field_z = ValueCellId::new("Coupling", "field_z");
    // c0: ValueRef of field_z (typed TypeParam("Z"), out-of-scope)
    let expr_c0 = CompiledExpr::value_ref(field_z.clone(), Type::TypeParam("Z".into()));
    // c1: literal Bool(true) — no ValueRef, no TypeParam
    let expr_c1 = CompiledExpr::literal(Value::Bool(true), Type::Bool);

    let template = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .param("Coupling", "field_u", Type::TypeParam("U".into()), None)
        .param("Coupling", "field_z", Type::TypeParam("Z".into()), None)
        .constraint("Coupling", 0, None, expr_c0)
        .constraint("Coupling", 1, None, expr_c1)
        .build();

    let params = vec![
        AutoTypeParam {
            name: "T".to_string(),
            bounds: vec![],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
        AutoTypeParam {
            name: "U".to_string(),
            bounds: vec![],
            free: true,
            use_site_span: SourceSpan::empty(0),
        },
        // Z is intentionally NOT in params — it must be treated as out-of-scope
    ];

    let map = build_constraint_blame_map(&template, &params);

    assert!(
        map.is_empty(),
        "constraints with empty blame sets must not appear in the map (empty map expected); got: {:?}",
        map
    );
}

// ─── step-5 (task 2660): DFS backjumps to lex-first-param blame ──────────────

/// When the first leaf's constraint violation blames only `T` (the lex-first /
/// outermost param, index 0), the DFS must backjump directly to the T-level and
/// skip the entire remaining `(ORingSeal, *, *)` sub-tree.
///
/// # Setup
///
/// - 3 free params: `[T:Seal, U:Cooled, W:Hot]`, two candidates each.
///   Cross-product: 8 leaves, DFS order T outer × U mid × W inner.
/// - Template: one cell `field_t : TypeParam("T")`, one constraint
///   `ValueRef(field_t, TypeParam("T"))` → blame = {T(0)}.
/// - Mock: `with_call_queue(vec![Violated])`, default `Satisfied`.
///   → leaf 1 check Violated; all subsequent checks Satisfied.
///
/// # Expected visit sequence WITH backjumping
///
/// 1. `(ORingSeal, AirCooled, Hot1)` → Violated → blame={0} (T) → BackjumpTo(0)
///    - W-loop (level=2): j=0 < K=2 → propagate
///    - U-loop (level=1): j=0 < K=1 → propagate
///    - T-loop (level=0): j=0 == K=0 → pop ORingSeal, try RubberSeal
/// 2. `(RubberSeal, AirCooled, Hot1)` → Satisfied → record
/// 3. `(RubberSeal, AirCooled, Hot2)` → Satisfied → record
/// 4. `(RubberSeal, WaterCooled, Hot1)` → Satisfied → record
/// 5. `(RubberSeal, WaterCooled, Hot2)` → Satisfied → record
///
/// 4 feasibles; lex-first = `(RubberSeal, AirCooled, Hot1)`.
///
/// # Distinguishes backjump-on from backjump-off
///
/// WITHOUT backjumping, the search also visits `(ORingSeal, AirCooled, Hot2)`,
/// `(ORingSeal, WaterCooled, Hot1)`, `(ORingSeal, WaterCooled, Hot2)` — all
/// Satisfied — giving 7 feasibles with lex-first `(ORingSeal, AirCooled, Hot2)`.
/// The substitution result diverges visibly:
/// - WITH backjump:    `T=RubberSeal, U=AirCooled, W=Hot1`
/// - WITHOUT backjump: `T=ORingSeal,  U=AirCooled, W=Hot2`
///
/// The diagnostic message also differs: with backjumping, ORingSeal is never
/// recorded as feasible and must NOT appear in the NonUnique witness list.
#[test]
fn dfs_backjumps_to_blamed_param_when_leaf_violation_blames_only_lex_first_param() {
    let source = r#"
trait Seal {}
trait Cooled {}
trait Hot {}

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

structure def Hot1 : Hot {
    param temp : Real = 100.0
}

structure def Hot2 : Hot {
    param temp : Real = 200.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Template: one cell `field_t : TypeParam("T")`, one constraint that
    // ValueRefs field_t. Blame = {T(0)} — only T is referenced.
    let field_t = ValueCellId::new("Coupling", "field_t");
    let constraint_expr =
        CompiledExpr::value_ref(field_t.clone(), Type::TypeParam("T".into()));
    let template = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .constraint("Coupling", 0, None, constraint_expr)
        .build();

    // Queue: [Violated]; default: Satisfied.
    // → leaf 1 check = Violated; all subsequent checks = Satisfied.
    let checker = MockConstraintChecker::new().with_call_queue(vec![Satisfaction::Violated]);

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
        AutoTypeParam {
            name: "W".to_string(),
            bounds: vec!["Hot".to_string()],
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

    // WITH backjumping: lex-first feasible is (RubberSeal, AirCooled, Hot1)
    // because the entire (ORingSeal, *, *) sub-tree is skipped after the
    // first leaf's Violated verdict blames only T(0).
    assert_eq!(
        outcome.substitution,
        vec![
            ("T".to_string(), "RubberSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
            ("W".to_string(), "Hot1".to_string()),
        ],
        "WITH backjumping: lex-first must be (RubberSeal, AirCooled, Hot1) \
         (ORingSeal sub-tree entirely skipped); got: {:?}",
        outcome.substitution
    );

    // WITH backjumping: only 4 feasible witnesses (all under RubberSeal).
    // ORingSeal must NOT appear in the NonUnique diagnostic message.
    assert_eq!(
        diagnostics.len(),
        1,
        "4 feasibles must emit exactly one NonUnique diagnostic; got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique; got: {:?}",
        diagnostics[0].code
    );
    assert!(
        !diagnostics[0].message.contains("ORingSeal"),
        "WITH backjumping: ORingSeal sub-tree is never visited as feasible; \
         message must NOT mention 'ORingSeal'; got: {:?}",
        diagnostics[0].message
    );
    assert!(
        diagnostics[0].message.contains("RubberSeal"),
        "WITH backjumping: all 4 feasibles are under RubberSeal; \
         message must contain 'RubberSeal'; got: {:?}",
        diagnostics[0].message
    );
}

/// Backjumping uses `max` over the **union** of all violated constraints' blame
/// sets — not `min`, not "first constraint's blame". With two violated
/// constraints blaming T(0) and U(1), the conflict set = {0,1} and deepest
/// blame J = 1 = U, so the search backjumps to the U level rather than to T.
///
/// Without this max-over-union rule (e.g., min-over-union returning J=0=T)
/// the search would backjump past all of ORingSeal, yielding lex-first
/// `(RubberSeal, AirCooled, Hot1)` instead — observably different.
#[test]
fn dfs_backjumps_to_deepest_blamed_param_when_multiple_violated_constraints_blame_different_params()
{
    let source = r#"
trait Seal {}
trait Cooled {}
trait Hot {}

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

structure def Hot1 : Hot {
    param temp : Real = 100.0
}

structure def Hot2 : Hot {
    param temp : Real = 200.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Template: two cells and two constraints.
    //   field_t : TypeParam("T") — constraint c0 ValueRefs only field_t → blame={T(0)}
    //   field_u : TypeParam("U") — constraint c1 ValueRefs only field_u → blame={U(1)}
    // The mock broadcasts ONE Violated verdict across ALL constraints in the
    // first leaf's check() call, so both c0 and c1 report Violated for leaf 1.
    // Blame union = {0} ∪ {1} = {0,1}; max = 1 = U → BackjumpTo(1).
    let field_t = ValueCellId::new("Coupling", "field_t");
    let field_u = ValueCellId::new("Coupling", "field_u");
    let expr_t = CompiledExpr::value_ref(field_t.clone(), Type::TypeParam("T".into()));
    let expr_u = CompiledExpr::value_ref(field_u.clone(), Type::TypeParam("U".into()));
    let template = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .param("Coupling", "field_u", Type::TypeParam("U".into()), None)
        .constraint("Coupling", 0, None, expr_t)
        .constraint("Coupling", 1, None, expr_u)
        .build();

    // Queue: [Violated]; default: Satisfied.
    // First leaf check → both constraints Violated (one check() call, one pop).
    // All subsequent checks → Satisfied.
    let checker = MockConstraintChecker::new().with_call_queue(vec![Satisfaction::Violated]);

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
        AutoTypeParam {
            name: "W".to_string(),
            bounds: vec!["Hot".to_string()],
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

    // WITH max-over-union backjumping:
    // Leaf 1 = (ORingSeal, AirCooled, Hot1) → Violated → conflict {0,1} → J=1=U
    // → backjump to U level, skip only (ORingSeal, AirCooled, Hot2)
    // → next visited leaf = (ORingSeal, WaterCooled, Hot1) → Satisfied
    // → lex-first feasible = (ORingSeal, WaterCooled, Hot1)
    //
    // WITHOUT max-over-union (e.g., min-over-union J=0=T):
    // → would backjump past all ORingSeal, lex-first = (RubberSeal, AirCooled, Hot1)
    assert_eq!(
        outcome.substitution,
        vec![
            ("T".to_string(), "ORingSeal".to_string()),
            ("U".to_string(), "WaterCooled".to_string()),
            ("W".to_string(), "Hot1".to_string()),
        ],
        "WITH max-over-union backjumping: lex-first must be (ORingSeal, WaterCooled, Hot1); \
         J=max{{0,1}}=1=U so ORingSeal sub-tree is not fully skipped; got: {:?}",
        outcome.substitution
    );

    // The resolved substitution started with ORingSeal, confirming we did NOT
    // backjump past T (which would have skipped the entire ORingSeal sub-tree).
    assert_eq!(
        outcome.substitution[0],
        ("T".to_string(), "ORingSeal".to_string()),
        "T must resolve to ORingSeal — if T resolved to RubberSeal, the search \
         incorrectly backjumped to T-level (min-over-union) instead of U-level \
         (max-over-union); got: {:?}",
        outcome.substitution
    );

    // Free-mode: multiple feasibles → exactly one NonUnique diagnostic.
    assert_eq!(
        diagnostics.len(),
        1,
        "multiple feasibles must emit exactly one NonUnique diagnostic; got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique; got: {:?}",
        diagnostics[0].code
    );
}

/// When the parameterized template's only constraint has no `ValueRef` nodes
/// (a `Bool(true)` literal), `build_constraint_blame_map` returns an empty map.
/// At any infeasible leaf, `compute_deepest_blame_level` returns `None`, and the
/// DFS falls through to `DfsControl::Continue` — identical to ordinary backtracking.
///
/// This regression test guards the "no blame ↔ no-op ↔ ordinary backtrack"
/// contract: wiring in the backjumping infrastructure must not change the
/// observable behavior when the blame map is empty.
///
/// Expected outcome is BIT-FOR-BIT identical to
/// `dfs_backtracks_when_first_leaf_violated_then_picks_second_feasible`
/// (task 2659/2661): lex-first = `(ORingSeal, WaterCooled)`, one
/// `AutoTypeParamNonUnique` Warning diagnostic.
#[test]
fn dfs_no_blame_constraint_falls_back_to_ordinary_backtrack() {
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

    // Template with a single Bool(true) literal constraint: no ValueRef, no
    // TypeParam reference → build_constraint_blame_map returns an empty map.
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

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

    // Verify the blame map is empty for this template + params combination.
    let blame_map = build_constraint_blame_map(&template, &params);
    assert!(
        blame_map.is_empty(),
        "Bool(true) literal constraint has no ValueRef / TypeParam refs; \
         blame map must be empty; got: {:?}",
        blame_map
    );

    // Queue: [Violated, Satisfied]; default: Satisfied.
    // Leaf 1 = (ORingSeal, AirCooled) → Violated → blame empty → Continue
    //   (ordinary backtrack; NOT a backjump)
    // Leaf 2 = (ORingSeal, WaterCooled) → Satisfied → collect
    // Leaves 3-4 → default Satisfied → collect
    // 3 feasibles total; lex-first = (ORingSeal, WaterCooled)
    let checker = MockConstraintChecker::new()
        .with_call_queue(vec![Satisfaction::Violated, Satisfaction::Satisfied]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

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

    // Outcome must be BIT-FOR-BIT identical to the 2659/2661 baseline test
    // `dfs_backtracks_when_first_leaf_violated_then_picks_second_feasible`.
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
        "no-blame constraint must fall back to ordinary backtrack; \
         lex-first must be (ORingSeal, WaterCooled); got: {:?}",
        outcome
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "3 feasibles must emit exactly one NonUnique diagnostic; got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique; got: {:?}",
        diagnostics[0].code
    );
    assert_eq!(
        diagnostics[0].severity,
        Severity::Warning,
        "AutoTypeParamNonUnique must be Warning severity; got: {:?}",
        diagnostics[0].severity
    );
}
