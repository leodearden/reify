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
    AutoTypeParam, MultiParamResolutionOutcome, SelectionResult,
    resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};
use reify_types::{CompiledFunction, SourceSpan};

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
