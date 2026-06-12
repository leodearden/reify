//! M-007 first real-source exercise: backjumping from real `.ri` candidate data,
//! with NO `MockConstraintChecker`.
//!
//! # What this file tests
//!
//! Task 4436 ε — Deliverable (2): activate / characterize M-007 backjumping
//! using actual candidate structures parsed from inline `.ri` source (via
//! `parse_and_compile`) instead of a scripted `MockConstraintChecker` call queue.
//!
//! The production behavior was delivered by prerequisite γ (task 4434), which
//! landed per-candidate value seeding inside `dfs_search` via
//! `seed_candidate_value_map`. This file adds the first integration-test
//! exercise that exercises that path with a real, data-driven checker — no
//! mock call queue anywhere in the file.
//!
//! # Design
//!
//! `CountingRealChecker` implements `reify_ir::ConstraintChecker` by reading
//! the per-candidate seeded `ConstraintInput.values` (populated by γ's
//! `seed_candidate_value_map` keyed as `ValueCellId::new(param_member, field)`)
//! and computing `Violated`/`Satisfied` from REAL candidate data.
//!
//! Specifically: the template carries a `field_t : TypeParam("T")` cell.
//! `param_type_member` returns `"field_t"` for param `T`, so
//! `seed_candidate_value_map` keys candidate cells as
//! `ValueCellId::new("field_t", candidate_field)`. The checker reads
//! `input.values.get(&ValueCellId::new("field_t", "diameter"))` and applies
//! the threshold `> 5.0`:
//!
//! - ORingSeal (`diameter = 10.0`) → `Some(Real(10.0)) > 5.0` → `Violated`
//! - RubberSeal (`diameter = 2.0`) → `Some(Real(2.0)) ≤ 5.0` → `Satisfied`
//!
//! # Test A — real-source backjump
//!
//! Template A carries a constraint whose expression references
//! `ValueRef(Coupling.field_t, TypeParam("T"))`. `build_constraint_blame_map`
//! maps that constraint to blame `{T(0)}`. When (ORingSeal, AirCooled, Hot1)
//! is the first leaf and the checker returns `Violated`, `DfsControl::BackjumpTo(0)`
//! fires and the entire `(ORingSeal, *, *)` sub-tree is skipped.
//!
//! Expected call counts (1 constraint per leaf, no within-leaf short-circuit):
//! - WITH backjumping: 5 leaves visited = 1 ORingSeal leaf + 4 RubberSeal leaves.
//! - WITHOUT backjumping: 8 leaves visited (full cross-product 2×2×2).
//!
//! Proves BackjumpTo fired and the blame map was non-empty.
//!
//! # Test B — no-blame control (violations present)
//!
//! Template B carries BOTH a `TypeParam("T")` cell (so T-candidate values ARE
//! seeded and `CountingRealChecker` returns `Violated` for ORingSeal leaves,
//! exactly as in test A) AND a `Type::Real` cell whose `ValueRef` is used in
//! the constraint expression. `build_constraint_blame_map` returns an empty map
//! (constraint references only the Real cell); no `BackjumpTo` fires; the DFS
//! visits all 8 leaves including the 4 Violated ORingSeal ones. Count == 8.
//!
//! This isolates "TypeParam blame drives BackjumpTo" from both "no violations"
//! and "early-termination": violations alone do not reduce the count — only the
//! blame-driven `BackjumpTo` in test A does. The WITH/WITHOUT contrast is
//! therefore airtight.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use reify_compiler::auto_type_param::{
    AutoTypeParam, resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_core::{SourceSpan, Type, ValueCellId};
use reify_ir::{
    CompiledExpr, CompiledFunction, ConstraintChecker, ConstraintDiagnostics, ConstraintInput,
    ConstraintResult, Satisfaction, Value,
};
use reify_test_support::{TopologyTemplateBuilder, parse_and_compile};

// ─── Source ─────────────────────────────────────────────────────────────────

/// Inline `.ri` source declaring three trait families:
///
/// - **Seal** (T): `ORingSeal` (diameter=10.0) and `RubberSeal` (diameter=2.0).
/// - **Cooled** (U): `AirCooled` and `WaterCooled`.
/// - **Hot** (W): `Hot1` and `Hot2`.
///
/// All structures carry literal-default `Real` params so
/// `seed_candidate_value_map` seeds their values (one-level literal seeding).
/// The diameter threshold in `CountingRealChecker` (> 5.0) causes `ORingSeal`
/// (10.0) to be `Violated` and `RubberSeal` (2.0) to be `Satisfied`.
const REAL_SOURCE: &str = r#"
trait Seal {}
trait Cooled {}
trait Hot {}

structure def ORingSeal : Seal {
    param diameter : Real = 10.0
}

structure def RubberSeal : Seal {
    param diameter : Real = 2.0
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

// ─── Helper: build registries ─────────────────────────────────────────────

/// Build the `(template_registry, trait_registry)` pair that
/// `resolve_auto_type_params_with_backtracking` consumes.
///
/// Mirrors `build_registries` from `auto_type_param_backtracking_tests.rs`.
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

/// Canonical three-param vec (T:Seal, U:Cooled, W:Hot) shared by both tests.
fn seal_cooled_hot_params() -> Vec<AutoTypeParam> {
    vec![
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
    ]
}

// ─── CountingRealChecker ──────────────────────────────────────────────────

/// A `ConstraintChecker` that reads the per-candidate seeded `ConstraintInput.values`
/// (γ's `seed_candidate_value_map` output) to produce a real, data-driven verdict.
///
/// This is the opposite of `MockConstraintChecker`: verdicts are NOT scripted
/// by a call queue. Instead, `check()` looks at the REAL seeded candidate
/// values in `input.values` to decide `Violated`/`Satisfied`.
///
/// Verdict logic:
/// - Read `input.values.get(&ValueCellId::new("field_t", "diameter"))`.
///   - `Some(Value::Real(v))` where `*v > 5.0` → `Violated` (ORingSeal: 10.0).
///   - Anything else (RubberSeal: 2.0; absent for U/W or Template-B leaves) → `Satisfied`.
///
/// The call counter is an `AtomicUsize` so the checker works through `&dyn`
/// (the established `CountingIndeterminate` pattern from
/// `auto_type_param_checker_inject_tests.rs:101`).
struct CountingRealChecker {
    count: AtomicUsize,
}

impl CountingRealChecker {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
        }
    }

    /// Number of times `check()` has been called.
    fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl ConstraintChecker for CountingRealChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        self.count.fetch_add(1, Ordering::Relaxed);

        // Read the seeded T-candidate diameter from the per-leaf ValueMap.
        // `param_type_member` returns "field_t" for param T (the member of the
        // template cell typed as TypeParam("T")), so `seed_candidate_value_map`
        // seeds: `ValueCellId::new("field_t", "diameter") → Value::Real(<v>)`.
        let diameter_key = ValueCellId::new("field_t", "diameter");
        let violated = matches!(input.values.get(&diameter_key), Some(Value::Real(v)) if *v > 5.0);

        // Return one ConstraintResult per constraint, using the shared verdict.
        // For Template A (1 constraint): Violated for ORingSeal, Satisfied for
        // RubberSeal → blame non-empty → BackjumpTo(0) fires.
        // For Template B (TypeParam cell for seeding + Real cell in constraint):
        // same Violated/Satisfied pattern as A, but blame={} → no BackjumpTo.
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: if violated {
                    Satisfaction::Violated
                } else {
                    Satisfaction::Satisfied
                },
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }
}

// ─── Test A: real-source backjump ─────────────────────────────────────────

/// First real-source exercise of M-007 backjumping.
///
/// The parameterized template carries a constraint referencing a cell of type
/// `TypeParam("T")`. `build_constraint_blame_map` produces a non-empty blame
/// map `{ constraint_0 → {T(0)} }`, so a `Violated` verdict on the first leaf
/// `(ORingSeal, AirCooled, Hot1)` fires `DfsControl::BackjumpTo(0)` and skips
/// the entire `(ORingSeal, *, *)` sub-tree.
///
/// # Assertions
///
/// **(a) Result matches the exhaustive-search baseline.**
/// Exhaustive enumeration (all 8 leaves in DFS order, same verdict logic):
/// - ORingSeal (diameter=10.0 > 5.0) → Violated for all 4 ORingSeal-containing leaves.
/// - RubberSeal (diameter=2.0 ≤ 5.0) → Satisfied for 4 RubberSeal-containing leaves.
///   Lex-first feasible = (RubberSeal, AirCooled, Hot1).
///
/// **(b) Checker.count() < full cross-product.**
/// WITH backjumping:  5 leaves visited × 1 constraint = 5 calls.
/// WITHOUT backjumping: 8 leaves (all) × 1 constraint = 8 calls.
/// `5 < 8` ⟹ a sub-tree was pruned ⟹ blame was non-empty ⟹ `BackjumpTo` fired.
#[test]
fn real_source_backjump_blame_t_prunes_oringen_subtree() {
    let module = parse_and_compile(REAL_SOURCE);
    let (template_registry, trait_registry) = build_registries(&module);

    // Template A: cell `Coupling.field_t : TypeParam("T")` + one constraint
    // that ValueRefs `field_t`. build_constraint_blame_map maps constraint_0
    // → blame {T(0)}.
    //
    // `param_type_member(template_A, "T")` returns "field_t" (the member of
    // the cell typed TypeParam("T")), so `seed_candidate_value_map` seeds:
    //   ORingSeal:   { ValueCellId::new("field_t", "diameter") → Real(10.0) }
    //   RubberSeal:  { ValueCellId::new("field_t", "diameter") → Real(2.0)  }
    let field_t = ValueCellId::new("Coupling", "field_t");
    let constraint_expr_a =
        CompiledExpr::value_ref(field_t, Type::TypeParam("T".into()));
    let template_a = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .constraint("Coupling", 0, None, constraint_expr_a)
        .build();

    let checker = CountingRealChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = seal_cooled_hot_params();

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template_a,
        &checker,
        functions,
        6,         // max_depth: 3 params ≤ 6, runs DFS (not BFS fallback)
        usize::MAX, // max_cross_product_size: no cap
        &mut diagnostics,
    );

    // (a) Result matches the exhaustive-search baseline.
    //
    // Exhaustive baseline (apply same verdict: ORingSeal Violated, RubberSeal Satisfied):
    // Full cross-product in DFS order (T outer, U mid, W inner):
    //   (ORingSeal, AirCooled, Hot1)    → Violated
    //   (ORingSeal, AirCooled, Hot2)    → Violated
    //   (ORingSeal, WaterCooled, Hot1)  → Violated
    //   (ORingSeal, WaterCooled, Hot2)  → Violated
    //   (RubberSeal, AirCooled, Hot1)   → Satisfied ← lex-first feasible
    //   (RubberSeal, AirCooled, Hot2)   → Satisfied
    //   (RubberSeal, WaterCooled, Hot1) → Satisfied
    //   (RubberSeal, WaterCooled, Hot2) → Satisfied
    // Lex-first feasible = (RubberSeal, AirCooled, Hot1).
    assert_eq!(
        outcome.substitution,
        vec![
            ("T".to_string(), "RubberSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
            ("W".to_string(), "Hot1".to_string()),
        ],
        "WITH backjumping: lex-first feasible must be (RubberSeal, AirCooled, Hot1); \
         got: {:?}",
        outcome.substitution
    );

    // (b) Checker call count < full cross-product (8).
    //
    // WITH backjumping:
    //   Leaf 1 (ORingSeal, AirCooled, Hot1) → Violated → blame T(0) → BackjumpTo(0)
    //   → skips leaves 2–4: (ORingSeal, AirCooled, Hot2), (ORingSeal, WaterCooled, Hot1),
    //                        (ORingSeal, WaterCooled, Hot2)
    //   Leaves 2–5 under RubberSeal → Satisfied
    // Total calls with backjumping: 5
    // Total calls without backjumping: 8 (full cross-product 2×2×2)
    let full_cross_product = 2 * 2 * 2;
    assert!(
        checker.count() < full_cross_product,
        "WITH backjumping: checker must be called fewer than {} times (got {}); \
         count < cross-product proves BackjumpTo fired and blame was non-empty",
        full_cross_product,
        checker.count()
    );
    assert_eq!(
        checker.count(),
        5,
        "WITH backjumping: exactly 5 leaves visited × 1 constraint = 5 calls \
         (1 ORingSeal leaf + 4 RubberSeal leaves; vs 8 without backjumping); \
         got: {}",
        checker.count()
    );
}

// ─── Test B: no-blame control (violations present) ───────────────────────

/// No-blame strict control: same violation pattern as test A (ORingSeal Violated,
/// RubberSeal Satisfied), but template B's constraint references only a
/// `Type::Real` cell — so `build_constraint_blame_map` returns an empty map.
///
/// This isolates "TypeParam blame" from "no violations": violations alone do not
/// reduce the leaf count; only the blame-driven `BackjumpTo` in test A does.
/// No `BackjumpTo` fires → the DFS visits all 8 leaves including the 4 Violated
/// ORingSeal ones. The WITH/WITHOUT contrast is therefore airtight.
///
/// # Assertions
///
/// **(a) Result matches the exhaustive-search baseline.**
/// No backjump despite violations: lex-first Satisfied (DFS order) is
/// (RubberSeal, AirCooled, Hot1) — the same result as test A.
///
/// **(b) Checker.count() == full cross-product.**
/// No blame → no pruning → exactly 8 leaves visited × 1 constraint = 8 calls,
/// including 4 Violated ORingSeal leaves. count == 8 proves that test A's
/// reduction to 5 is attributable solely to the blame-driven `BackjumpTo`.
#[test]
fn no_blame_with_violations_visits_full_cross_product() {
    let module = parse_and_compile(REAL_SOURCE);
    let (template_registry, trait_registry) = build_registries(&module);

    // Template B: two cells —
    //   `Coupling.field_t : TypeParam("T")` enables T-candidate seeding so
    //   `CountingRealChecker` receives real ORingSeal/RubberSeal diameter values.
    //   `Coupling.field_control : Type::Real` is used in the constraint expression.
    // `build_constraint_blame_map` returns {} because the constraint refs only the
    // Real cell → no BackjumpTo fires even though ORingSeal leaves are Violated.
    let field_control = ValueCellId::new("Coupling", "field_control");
    let constraint_expr_b =
        CompiledExpr::value_ref(field_control, Type::dimensionless_scalar());
    let template_b = TopologyTemplateBuilder::new("Coupling")
        .param("Coupling", "field_t", Type::TypeParam("T".into()), None)
        .param("Coupling", "field_control", Type::dimensionless_scalar(), None)
        .constraint("Coupling", 0, None, constraint_expr_b)
        .build();

    let checker = CountingRealChecker::new();
    let functions: &[CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let params = seal_cooled_hot_params();

    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &template_b,
        &checker,
        functions,
        6,
        usize::MAX,
        &mut diagnostics,
    );

    // (a) Result matches the exhaustive-search baseline.
    //
    // Seeding: field_t : TypeParam("T") → ORingSeal: Violated (10.0 > 5.0),
    //          RubberSeal: Satisfied (2.0 ≤ 5.0).
    // Constraint refs field_control (Real) → blame={} → no BackjumpTo.
    // All 8 leaves visited; lex-first Satisfied (DFS order): (RubberSeal, AirCooled, Hot1).
    assert_eq!(
        outcome.substitution,
        vec![
            ("T".to_string(), "RubberSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
            ("W".to_string(), "Hot1".to_string()),
        ],
        "VIOLATIONS+NO-BLAME: lex-first Satisfied must be (RubberSeal, AirCooled, Hot1); \
         got: {:?}",
        outcome.substitution
    );

    // (b) Checker call count == full cross-product (8).
    //
    // Violations are present (ORingSeal subtree) but blame is empty → no BackjumpTo →
    // all 8 leaves visited. count == 8 proves that test A's reduction (5 vs 8) is
    // attributable solely to the blame-driven BackjumpTo, not to violations per se.
    let full_cross_product = 2 * 2 * 2;
    assert_eq!(
        checker.count(),
        full_cross_product,
        "VIOLATIONS+NO-BLAME: all {} leaves visited × 1 constraint = {} calls \
         (incl. 4 Violated ORingSeal leaves — blame-empty proves no BackjumpTo fired); \
         got: {}",
        full_cross_product,
        full_cross_product,
        checker.count()
    );
}
