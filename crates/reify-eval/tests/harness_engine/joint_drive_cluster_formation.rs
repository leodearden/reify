// Integration tests for JOINT-DRIVE δ cluster formation (Gap C, task #5334).
//
// Skeleton mirrors coupling_approximated.rs: engine-level builder fixtures
// driven through the literal `reify check` entry point (`engine.check()` on a
// no-solver engine), asserting on `DiagnosticCode` rather than message
// substrings.
//
// The δ cluster-formation RULES (expansion-aware objective reads, transitive
// auto-reaching walk, the `@optimized` stop) are unit-tested inside
// resolve_order.rs. THIS file covers exactly one thing the unit tests cannot:
// that the production `eval()` call site actually threads a
// `ClusterFormationCtx` into `resolve_order`, so a derived-cost coupling forms
// a real cluster on the cold path and becomes user-observable.
//
// W_COUPLING_APPROXIMATED is used as the observation channel because it is the
// only δ-cluster consequence that surfaces without a solver: the emitter sits
// OUTSIDE `eval()`'s `has_active_solver` gate (engine_eval.rs), so a
// `MockConstraintChecker` engine with no solver still reports it.
//
// ## Cap independence
//
// This external test cannot read the `pub(crate)` `WHOLE_MODEL_CLUSTER_DIM_CAP`,
// so the fixture gives the child a comfortably-over-cap auto count (20, the
// convention coupling_approximated.rs already uses): robust as long as the cap
// stays below 20. No assertion here names a numeric threshold.

use reify_core::{DiagnosticCode, ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{BinOp, ObjectiveSense, ObjectiveSet};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder, binop, fn_call,
    method_call_expr, value_ref, value_ref_typed,
};

/// Auto cells on the Costed child. Comfortably over the ~12 cap, so the
/// δ-formed {Parent, Child} cluster lands in `ApproximatedFallback` (the parent
/// itself owns no autos, so the merged dim is exactly this).
const OVER_CAP_AUTOS: usize = 20;

/// Entity path of the child instance under the parent, matching what
/// `enumerate_descendants` produces for `sub_component("childinst", "Child")`.
const CHILD_PATH: &str = "Parent.childinst";

// ---------------------------------------------------------------------------
// Helpers (mirror coupling_approximated.rs).
// ---------------------------------------------------------------------------

/// Build a no-solver engine — the literal `reify check` entry point.
fn no_solver_engine() -> Engine {
    Engine::new(Box::new(MockConstraintChecker::new()), None)
}

/// Add `n` auto cells (`<entity>.a0..a{n-1}`) to a template builder.
fn with_n_autos(mut b: TopologyTemplateBuilder, entity: &str, n: usize) -> TopologyTemplateBuilder {
    for i in 0..n {
        b = b.auto_param(entity, &format!("a{i}"), Type::length());
    }
    b
}

/// Money-dimensioned scalar type (`line_cost : Money`), mirroring the stdlib
/// `Costed` shape.
fn money_ty() -> Type {
    Type::Scalar {
        dimension: reify_core::DimensionVector::MONEY,
    }
}

/// Count `W_COUPLING_APPROXIMATED` diagnostics in a result set.
fn count_coupling_approximated(diagnostics: &[reify_core::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CouplingApproximated))
        .count()
}

// ---------------------------------------------------------------------------
// step-8: the cold production path forms δ clusters.
// ---------------------------------------------------------------------------

/// A Parent whose `minimize cost(self.descendants)` objective couples to its
/// Costed child ONLY through the child's DERIVED `line_cost` cell forms a real
/// cluster on the cold `eval()` path, and — since the child is over-cap —
/// surfaces exactly one `W_COUPLING_APPROXIMATED`.
///
/// Every link in the chain is δ-specific, so nothing but the wired-in
/// `ClusterFormationCtx` can make this pass:
///
/// - the objective is the compiler's raw `cost(self.descendants)` shape, so
///   only CLUSTER-TIME expansion (C1) surfaces the `line_cost` read at all;
/// - `line_cost` is a `Let`, not an auto, so the pre-#5334 direct-`auto_owner`
///   objective seed misses it and the read-DAG adds no edge either (no SCC, no
///   ordering crossing) — only the transitive auto-reaching walk (C2) reaches
///   `Parent.childinst.a0` behind it;
/// - the parent owns no autos and never reads the child's autos directly, so
///   there is no other route to a cluster.
///
/// Before the cold call site passes `Some(&ctx)` this is RED: `resolve_order`
/// forms zero clusters and `check()` emits no diagnostic.
#[test]
fn derived_cost_coupling_forms_cluster_on_cold_path() {
    // Parent: no autos, one Costed child sub-component, objective over the
    // structural query `cost(self.descendants)`.
    let parent = TopologyTemplateBuilder::new("Parent")
        .sub_component("childinst", "Child", vec![])
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            fn_call(
                "cost",
                "cost",
                vec![method_call_expr(
                    value_ref("Parent", "__self"),
                    "descendants",
                    vec![],
                    Type::List(Box::new(Type::StructureRef("Structure".to_string()))),
                )],
                money_ty(),
            ),
        ))
        .build();

    // Child: Costed, over-cap autos, and a derived `line_cost` that reads one of
    // them — the only bridge between the parent objective and a child auto.
    let child = with_n_autos(
        TopologyTemplateBuilder::new("Child").trait_bound("Costed"),
        CHILD_PATH,
        OVER_CAP_AUTOS,
    )
    .let_binding(
        CHILD_PATH,
        "line_cost",
        money_ty(),
        binop(
            BinOp::Mul,
            value_ref_typed(CHILD_PATH, "unit_cost", money_ty()),
            value_ref(CHILD_PATH, "a0"),
        ),
    )
    .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(child)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.check(&module);

    let count = count_coupling_approximated(&result.diagnostics);
    assert_eq!(
        count, 1,
        "a derived-cost coupling must form a δ cluster on the cold path and surface \
         exactly one W_COUPLING_APPROXIMATED (the child is over-cap); got {}: {:?}",
        count, result.diagnostics,
    );
}
