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
    MultiParamResolutionOutcome, resolve_auto_type_params_with_backtracking,
};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder};
use reify_types::CompiledFunction;

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
