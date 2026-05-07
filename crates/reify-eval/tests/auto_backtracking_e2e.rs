//! Auto-backtracking v0.2: PRD-documented BFS-failure scenario suite.
//!
//! PRD: `docs/prds/v0_2/auto-resolution-backtracking.md`
//!
//! # Scope
//!
//! This file pins the v0.2 DFS backtracking algorithm against the concrete BFS
//! failure scenarios documented in the PRD's "Background" section: "when
//! parameter A's locally-feasible choice rules out every parameter B, the
//! algorithm fails."  Each test is a PRD-traceable regression guard so that
//! any future regression to BFS-only behavior is caught by name-bearing tests.
//!
//! # Source-level syntax limitation
//!
//! The parser does not yet accept `auto: TraitName` in `type_arg_list`
//! (`tree-sitter-reify/grammar.js:601-605`).  Tests therefore invoke
//! `resolve_auto_type_params_with_backtracking` directly on registries built
//! by `parse_and_compile`, following the same convention as sibling files
//! `auto_type_param_topology_trigger_tests.rs` and
//! `auto_type_param_determinism_tests.rs`.  "e2e" in the file name is
//! approximate: tests exercise the full compile pipeline up to the point where
//! the orchestrator runs, but stop short of `Engine::eval`.  When source-level
//! `auto:` syntax lands (separate task), follow-up tests in this file can
//! graduate to true Engine-driven e2e form without renaming.
//!
//! # Per-leaf-differing verdicts via MockConstraintChecker
//!
//! Production constraint feasibility runs against the unspecialized template
//! with empty values (PRD "Constraint-feasibility incremental binding
//! deferred" decision; see auto_type_param.rs:1327-1341 TODO).  Real
//! BFS-fail-DFS-success scenarios therefore require `MockConstraintChecker`
//! to script per-leaf outcomes that mimic what cross-product-aware constraints
//! would produce post-substitution.  `with_call_queue([Verdict, ...])` is the
//! load-bearing primitive: one queue pop per leaf check.

use std::collections::HashMap;

use reify_compiler::auto_type_param::{
    AutoTypeParam, MultiParamResolutionOutcome, SelectionResult, resolve_auto_type_params,
    resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};
use reify_types::{
    CompiledExpr, CompiledFunction, DiagnosticCode, Satisfaction, Severity, SourceSpan, Type,
    Value, ValueCellId,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build the `(template_registry, trait_registry)` pair from a compiled module.
///
/// Mirrors `build_registries` from
/// `crates/reify-compiler/tests/auto_type_param_backtracking_tests.rs` and
/// `crates/reify-eval/tests/auto_type_param_determinism_tests.rs`.
/// Lifted verbatim so each test in this file is self-contained.
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

// ─── Scenario 1: 2-param lex-first conflict (PRD §"Background" para 3) ──────

/// Scenario: 2 coupled auto-params, lex-first conflict.
///
/// BFS would pick `(T=ORingSeal, U=AirCooled)` because ORingSeal is the
/// lex-first Seal candidate and AirCooled is the lex-first Cooled candidate —
/// both pass Phase B's per-param feasibility check independently.  However,
/// the cross-product leaves `(ORingSeal, AirCooled)` and `(ORingSeal, WaterCooled)`
/// are globally infeasible (queue pops Violated), so ORingSeal is globally
/// ruled out.  DFS backtracks T from ORingSeal to RubberSeal and finds
/// `(RubberSeal, AirCooled)` as the first globally feasible leaf.
///
/// Queue: `[Violated, Violated, Satisfied]`
///   - pop 1: `(ORingSeal, AirCooled)` → infeasible
///   - pop 2: `(ORingSeal, WaterCooled)` → infeasible
///   - pop 3: `(RubberSeal, AirCooled)` → feasible
///   - default Satisfied: `(RubberSeal, WaterCooled)` → also feasible
///     (2 feasibles total → NonUnique Warning in free mode)
///
/// Pins PRD §"Background" para 3: "when A's locally-feasible choice rules
/// out every B, the algorithm fails."
#[test]
fn bfs_fails_2_param_lex_first_conflict_dfs_backtracks_to_second() {
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

    // Template carries one literal constraint so the mock's per-call queue
    // produces non-empty ConstraintResult slices (the mock ignores expression
    // content; the literal exists only to give the constraint list a member).
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Queue: leaf 1 and 2 (both ORingSeal leaves) are infeasible;
    //        leaf 3 (RubberSeal, AirCooled) is feasible.
    //        default Satisfied for remaining leaves.
    let checker = MockConstraintChecker::new()
        .with_call_queue(vec![
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
        usize::MAX,
        &mut diagnostics,
    );

    // DFS must backtrack T from ORingSeal to RubberSeal and pick
    // (RubberSeal, AirCooled) as the lex-first globally-feasible leaf.
    assert_eq!(
        outcome.substitution,
        vec![
            ("T".to_string(), "RubberSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
        ],
        "DFS must backtrack from infeasible ORingSeal cross-product and find \
         (RubberSeal, AirCooled) as lex-first globally feasible; got: {:?}",
        outcome.substitution
    );
    assert_eq!(
        outcome.per_param.len(),
        2,
        "per_param must have exactly 2 entries (both params resolved); got: {:?}",
        outcome.per_param
    );
    assert!(
        matches!(&outcome.per_param[0].1, SelectionResult::Selected(n) if n == "RubberSeal"),
        "T must be Selected(RubberSeal) after backtrack; got: {:?}",
        outcome.per_param[0]
    );
    assert!(
        matches!(&outcome.per_param[1].1, SelectionResult::Selected(n) if n == "AirCooled"),
        "U must be Selected(AirCooled) (lex-first feasible U under RubberSeal); got: {:?}",
        outcome.per_param[1]
    );
}

// ─── Scenario 2: only last leaf feasible (maximum-distance backtrack) ─────────

/// Scenario: 2 coupled auto-params, only the last cross-product leaf is
/// globally feasible.  BFS picks `(ORingSeal, AirCooled)` — the lex-first
/// per-param combination — but every leaf except `(RubberSeal, WaterCooled)`
/// is infeasible.  DFS must exhaust all 4 leaves in declared order and find
/// the unique global solution at the deepest lex position.
///
/// Queue: `[Violated, Violated, Violated, Satisfied]`
///   - pop 1: `(ORingSeal, AirCooled)` → infeasible
///   - pop 2: `(ORingSeal, WaterCooled)` → infeasible
///   - pop 3: `(RubberSeal, AirCooled)` → infeasible
///   - pop 4: `(RubberSeal, WaterCooled)` → feasible (unique leaf)
///
/// With strict mode (free=false) and exactly 1 feasible leaf, DFS produces
/// `Selected` for both params and emits zero Warning diagnostics (no
/// Ambiguous, no NonUnique).  Pins the maximum-distance backtrack path.
///
/// Pins PRD §"Background" para 3 (maximum-distance case).
#[test]
fn bfs_fails_2_param_only_last_leaf_feasible_dfs_finds_it() {
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

    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let template = TopologyTemplateBuilder::new("Coupling")
        .constraint("Coupling", 0, None, expr)
        .build();

    // Queue makes only the 4th leaf (RubberSeal, WaterCooled) feasible.
    let checker = MockConstraintChecker::new().with_call_queue(vec![
        Satisfaction::Violated,
        Satisfaction::Violated,
        Satisfaction::Violated,
        Satisfaction::Satisfied,
    ]);

    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    // Strict mode (free=false) on both params: a single feasible leaf must
    // produce Selected (not Ambiguous) with zero Warning diagnostics.
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

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        6,
        usize::MAX,
        &mut diagnostics,
    );

    // DFS must exhaust all 4 leaves and find (RubberSeal, WaterCooled) as the
    // unique globally feasible leaf.
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
        "DFS must find (RubberSeal, WaterCooled) as the unique globally feasible leaf \
         after exhausting all 3 infeasible leaves; got: {:?}",
        outcome
    );
    // Single feasible leaf in strict mode → no Ambiguous error, no NonUnique
    // warning — zero diagnostics.
    assert!(
        diagnostics.is_empty(),
        "strict mode with exactly one feasible leaf must emit zero diagnostics; \
         got: {:?}",
        diagnostics
    );
}
