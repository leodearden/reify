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

// ─── Scenario 3: 3-param, blame T — DFS backjumps from V directly to T ───────

/// Scenario: 3 coupled auto-params `[T: Seal, U: Cooled, V: Mounted]`, 2
/// candidates each (8-leaf cross-product).  The parameterized template carries
/// one constraint whose expression references a cell typed as
/// `TypeParam("T")`.  `build_constraint_blame_map` therefore records blame set
/// `{0}` (= T) for that constraint.
///
/// With queue `[Violated]` and default `Satisfied`, the first leaf
/// `(ORingSeal, AirCooled, BoltedMount)` is infeasible, blame = T(0).
/// DFS backjumps from level 2 (V) directly to level 0 (T), skipping U
/// entirely — the entire `(ORingSeal, *, *)` sub-tree (4 leaves) is skipped.
/// DFS advances T to RubberSeal and finds `(RubberSeal, AirCooled, BoltedMount)`
/// as the lex-first feasible leaf.
///
/// Observable distinction from normal (non-backjumping) DFS: without
/// backjumping the second leaf would be `(ORingSeal, AirCooled, WeldedMount)`
/// (Satisfied), yielding T=ORingSeal in the result.  With backjumping,
/// T=RubberSeal is the first T candidate that actually appears in a feasible
/// leaf — proving the backjump skipped the ORingSeal sub-tree.
///
/// Pins task 2660 deepest-blame backjump semantics,
/// PRD §"Sketch of approach" backjumping bullet.
#[test]
fn bfs_fails_3_param_violation_blames_first_param_dfs_backjumps_directly() {
    let source = r#"
trait Seal {}
trait Cooled {}
trait Mounted {}

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

structure def BoltedMount : Mounted {
    param torque : Real = 50.0
}

structure def WeldedMount : Mounted {
    param thickness : Real = 3.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Template: one cell typed TypeParam("T") + one constraint that ValueRefs
    // it.  build_constraint_blame_map records blame = {T(0)} for this
    // constraint.  When the constraint is violated, DFS backjumps to T level.
    let field_t = ValueCellId::new("Coupling", "field_t");
    let expr_t = CompiledExpr::value_ref(field_t.clone(), Type::TypeParam("T".into()));
    let template = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .constraint("Coupling", 0, None, expr_t)
        .build();

    // Queue: leaf 1 (ORingSeal, AirCooled, BoltedMount) → Violated, blame T(0)
    //        → backjump to T; all subsequent leaves use default Satisfied.
    let checker =
        MockConstraintChecker::new().with_call_queue(vec![Satisfaction::Violated]);

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
            name: "V".to_string(),
            bounds: vec!["Mounted".to_string()],
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

    // With backjumping to T: the entire (ORingSeal, *, *) sub-tree is skipped.
    // Lex-first feasible is (RubberSeal, AirCooled, BoltedMount).
    // Without backjumping (normal DFS): second leaf would be
    // (ORingSeal, AirCooled, WeldedMount) → T=ORingSeal.
    assert_eq!(
        outcome.substitution[0],
        ("T".to_string(), "RubberSeal".to_string()),
        "WITH backjumping to T(0): T must be RubberSeal (not ORingSeal); \
         backjump skipped the entire (ORingSeal,*,*) sub-tree; got: {:?}",
        outcome.substitution
    );
    assert_eq!(
        outcome.substitution[1],
        ("U".to_string(), "AirCooled".to_string()),
        "U must be AirCooled (lex-first Cooled under RubberSeal); got: {:?}",
        outcome.substitution
    );
    assert_eq!(
        outcome.substitution[2],
        ("V".to_string(), "BoltedMount".to_string()),
        "V must be BoltedMount (lex-first Mounted under RubberSeal, AirCooled); got: {:?}",
        outcome.substitution
    );
    // With backjumping, ORingSeal was never recorded as part of a feasible
    // leaf — must NOT appear in the NonUnique witness list.
    assert_eq!(
        diagnostics.len(),
        1,
        "4 feasibles under RubberSeal → exactly one NonUnique Warning; got: {:?}",
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
        "WITH backjumping: ORingSeal sub-tree never visited as feasible; \
         NonUnique message must NOT mention 'ORingSeal'; got: {:?}",
        diagnostics[0].message
    );
    assert!(
        diagnostics[0].message.contains("RubberSeal"),
        "all feasibles are under RubberSeal; NonUnique message must contain \
         'RubberSeal'; got: {:?}",
        diagnostics[0].message
    );
}

// ─── Scenario 4: 3-param, blame U — DFS backjumps from V to U (not T) ────────

/// Scenario: 3 coupled auto-params `[T: Seal, U: Cooled, V: Mounted]`, 2
/// candidates each.  The template carries one constraint whose expression
/// references a cell typed as `TypeParam("U")`.  `build_constraint_blame_map`
/// records blame set `{1}` (= U) for that constraint.
///
/// With queue `[Violated]` and default `Satisfied`, the first leaf
/// `(ORingSeal, AirCooled, BoltedMount)` is infeasible, blame = U(1).
/// DFS backjumps from level 2 (V) to level 1 (U) — NOT to level 0 (T).
/// Within T=ORingSeal, U advances from AirCooled to WaterCooled.  DFS then
/// finds `(ORingSeal, WaterCooled, BoltedMount)` as the lex-first feasible.
///
/// Observable distinction:
///   - Without backjumping: second leaf is `(ORingSeal, AirCooled, WeldedMount)`
///     → T=ORingSeal, U=AirCooled, V=WeldedMount.
///   - With backjump to U: T stays at ORingSeal, U advances to WaterCooled,
///     V restarts at BoltedMount → T=ORingSeal, U=WaterCooled, V=BoltedMount.
///
/// Pins task 2660 `j == K` consume / `j < K` propagate logic
/// (`auto_type_param.rs:1999-2005 DfsControl arms`).
#[test]
fn bfs_fails_3_param_violation_blames_middle_param_dfs_backjumps_to_middle() {
    let source = r#"
trait Seal {}
trait Cooled {}
trait Mounted {}

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

structure def BoltedMount : Mounted {
    param torque : Real = 50.0
}

structure def WeldedMount : Mounted {
    param thickness : Real = 3.0
}
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    // Template: one cell typed TypeParam("U") + one constraint that ValueRefs
    // it.  build_constraint_blame_map records blame = {U(1)} for this
    // constraint.  When the constraint is violated, DFS backjumps to U level
    // (NOT T).
    let field_u = ValueCellId::new("Coupling", "field_u");
    let expr_u = CompiledExpr::value_ref(field_u.clone(), Type::TypeParam("U".into()));
    let template = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_u", Type::TypeParam("U".into()), None)
        .constraint("Coupling", 0, None, expr_u)
        .build();

    // Queue: leaf 1 (ORingSeal, AirCooled, BoltedMount) → Violated, blame U(1)
    //        → backjump to U; all subsequent leaves use default Satisfied.
    let checker =
        MockConstraintChecker::new().with_call_queue(vec![Satisfaction::Violated]);

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
            name: "V".to_string(),
            bounds: vec!["Mounted".to_string()],
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

    // WITH backjump to U: T stays at ORingSeal (first T candidate),
    // U advances to WaterCooled (second U candidate), V restarts at
    // BoltedMount.  Lex-first feasible = (ORingSeal, WaterCooled, BoltedMount).
    assert_eq!(
        outcome.substitution[0],
        ("T".to_string(), "ORingSeal".to_string()),
        "WITH backjump to U(1): T must STAY at ORingSeal (backjump did not \
         reach T level); got: {:?}",
        outcome.substitution
    );
    assert_eq!(
        outcome.substitution[1],
        ("U".to_string(), "WaterCooled".to_string()),
        "U must advance to WaterCooled (second Cooled candidate after backjump \
         skipped AirCooled sub-tree); got: {:?}",
        outcome.substitution
    );
    assert_eq!(
        outcome.substitution[2],
        ("V".to_string(), "BoltedMount".to_string()),
        "V must restart at BoltedMount (lex-first Mounted after U backjump \
         reset V iteration); got: {:?}",
        outcome.substitution
    );
    // Free mode with multiple feasibles → NonUnique Warning.
    assert_eq!(
        diagnostics.len(),
        1,
        "multiple feasibles in free mode → exactly one NonUnique Warning; got: {:?}",
        diagnostics
    );
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNonUnique),
        "diagnostic must be AutoTypeParamNonUnique; got: {:?}",
        diagnostics[0].code
    );
}

// ─── Scenario 5: depth-bound fallback (PRD §"Resolved design decisions") ─────

/// Scenario: depth-bound fallback fires because `params.len() (=2) > max_depth
/// (=1)`.  DFS short-circuits to `emit_fallback_warning_and_delegate_to_bfs`,
/// emitting one `AutoTypeParamDepthBoundExceeded` Warning and returning the
/// v0.1 BFS outcome.
///
/// The setup uses exactly 1 candidate per param so BFS produces clean
/// `Selected` outcomes with zero BFS diagnostics — the only diagnostic in the
/// output is the depth-bound warning itself.
///
/// Pins PRD §"Resolved design decisions" "Default depth bound: 6 parameters"
/// and the fallback rule (`params.len() > max_depth` ⇒ BFS fallback;
/// `params.len() == max_depth` ⇒ DFS runs, pinned by
/// `dfs_at_max_depth_runs_dfs_no_fallback_diagnostic`).
#[test]
fn bfs_fallback_above_depth_bound_emits_warning_and_runs_bfs() {
    // One Seal candidate + one Cooled candidate: 2 params, 1 candidate each.
    // BFS produces clean Selected × 2, zero BFS diagnostics.
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

    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new();
    let functions: &[CompiledFunction] = &[];

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

    // Capture BFS outcome for parity check.
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
    assert!(
        bfs_diagnostics.is_empty(),
        "BFS on 1-candidate-per-param must emit zero diagnostics (baseline); \
         got: {:?}",
        bfs_diagnostics
    );

    // DFS with max_depth=1: params.len()==2 > 1 → depth-bound fallback fires.
    let mut dfs_diagnostics = Vec::new();
    let dfs_outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template,
        &checker,
        functions,
        1, // max_depth=1; 2 params > 1 → fallback
        usize::MAX,
        &mut dfs_diagnostics,
    );

    // Outcome parity: DFS-with-fallback must match BFS exactly.
    assert_eq!(
        dfs_outcome, bfs_outcome,
        "DFS above max_depth must delegate to BFS; outcome must match BFS's \
         identical-input outcome. DFS: {:?}, BFS: {:?}",
        dfs_outcome, bfs_outcome
    );

    // Exactly one extra diagnostic: the depth-bound warning.
    assert_eq!(
        dfs_diagnostics.len(),
        1,
        "DFS above max_depth (BFS baseline: 0 diagnostics) must emit exactly \
         one DepthBoundExceeded Warning; got: {:?}",
        dfs_diagnostics
    );
    let warn = &dfs_diagnostics[0];
    assert_eq!(
        warn.code,
        Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded),
        "diagnostic code must be AutoTypeParamDepthBoundExceeded; got: {:?}",
        warn.code
    );
    assert_eq!(
        warn.severity,
        Severity::Warning,
        "AutoTypeParamDepthBoundExceeded must be Severity::Warning; got: {:?}",
        warn.severity
    );
    // Message must cite the param count (2) and the max_depth (1).
    assert!(
        warn.message.contains("2"),
        "depth-bound message must mention param count '2'; got: {:?}",
        warn.message
    );
    assert!(
        warn.message.contains("1"),
        "depth-bound message must mention max_depth '1'; got: {:?}",
        warn.message
    );
}
