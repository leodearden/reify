// Integration tests for JOINT-DRIVE δ cluster formation (Gap C, task #5334).
//
// The fixture is compiled from REAL `.ri` source (`parse_and_compile_with_stdlib`,
// the idiom in tests/cost_subtree_aggregate_eval.rs) and driven through the
// literal `reify check` entry point (`engine.check()` on a no-solver engine),
// asserting on `DiagnosticCode` rather than message substrings.
//
// The δ cluster-formation RULES (expansion-aware objective reads, transitive
// auto-reaching walk, the `@optimized` stop, instance-path normalisation) are
// unit-tested inside resolve_order.rs. THIS file covers the two things the unit
// tests cannot: that the production `eval()` call site actually threads a
// `ClusterFormationCtx` into `resolve_order`, AND that a cluster genuinely forms
// over the cell-id shape the COMPILER emits — so a derived-cost coupling becomes
// user-observable on the cold path.
//
// ## Do NOT go back to `TopologyTemplateBuilder` here
//
// This file previously hand-built its templates and declared the CHILD's own
// cells with entity `"Parent.childinst"`. That is a shape reify-compiler never
// emits (template cells are minted structure-name-scoped at
// reify-compiler/src/entity.rs:6066/:6197) and it was the ONLY reason the walk
// succeeded — the suite was green over a feature that unioned nothing on every
// real `.ri` design (review round 2, blocking issue 2). A synthetic entity
// scoping here defeats the entire purpose of this file. Rule-level isolation
// belongs in resolve_order.rs's builder fixtures; this file pins the compiler's
// actual CELL-ID SHAPE.
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
// stays below 20. No assertion here names a numeric threshold or a message
// substring.

use reify_core::DiagnosticCode;
use reify_eval::Engine;
use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};

/// Auto cells on the Costed child. Comfortably over the ~12 cap, so the δ-formed
/// {Rig, Bolt} cluster lands in `ApproximatedFallback` (the Rig itself owns no
/// autos, so the merged dim is exactly this).
const OVER_CAP_AUTOS: usize = 20;

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// Build a no-solver engine — the literal `reify check` entry point.
fn no_solver_engine() -> Engine {
    Engine::new(Box::new(MockConstraintChecker::new()), None)
}

/// Count `W_COUPLING_APPROXIMATED` diagnostics in a result set.
fn count_coupling_approximated(diagnostics: &[reify_core::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CouplingApproximated))
        .count()
}

/// `.ri` source for the over-cap Costed child + its parent.
///
/// `Costed` refines `Buy`, so `Bolt` must also declare supplier / part_number /
/// unit_cost / lead_time (the `CapScrew` def in tests/cost_subtree_aggregate_eval.rs
/// is the working template this is modelled on). The `a0..a{n-1}` autos need no
/// individual constraints — the assertion filters by `DiagnosticCode`, so
/// unconstrained-auto warnings are harmless noise.
///
/// TRAP — the aggregate must stay INLINED in the `minimize`. Hoisting it into a
/// `let subtree_cost : Money = cost(self.descendants)` forms no cluster, because
/// cluster-time expansion runs over objective TERMS only while the cell map
/// stores raw `default_expr`s (pinned by resolve_order.rs's
/// `objective_must_inline_the_aggregate_to_couple`).
fn over_cap_costed_source() -> String {
    let autos: String = (0..OVER_CAP_AUTOS)
        .map(|i| format!("    param a{i} : Length = auto\n"))
        .collect();
    format!(
        r#"
structure def Bolt : Costed {{
    param supplier          : String = "Acme"
    param part_number       : String = "B-1"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = auto
    constraint quantity_produced >= 1.0
    constraint quantity_produced <= 100.0
{autos}}}
structure Rig {{
    sub bolts = Bolt()
    minimize cost(self.descendants)
}}
"#
    )
}

// ---------------------------------------------------------------------------
// The cold production path forms δ clusters over COMPILER-EMITTED cell ids.
// ---------------------------------------------------------------------------

/// A `Rig` whose `minimize cost(self.descendants)` objective couples to its
/// Costed `Bolt` sub ONLY through the child's DERIVED `line_cost` cell forms a
/// real cluster on the cold `eval()` path, and — since the child is over-cap —
/// surfaces exactly one `W_COUPLING_APPROXIMATED`.
///
/// Every link in the chain is δ-specific, so nothing but the wired-in
/// `ClusterFormationCtx` plus instance-path normalisation can make this pass:
///
/// - the objective is `cost(self.descendants)`, so only CLUSTER-TIME expansion
///   (C1) surfaces the `line_cost` read at all;
/// - that expansion mints an INSTANCE-PATH id (`Rig.bolts.line_cost`) while the
///   maps it must hit are structure-name-keyed (`Bolt.line_cost`), so only the
///   normalisation layer resolves it;
/// - stdlib `Costed`'s `line_cost` is a `Let`, not an auto, so the pre-#5334
///   direct-`auto_owner` objective seed misses it and the read-DAG adds no edge
///   either (no SCC, no ordering crossing) — only the transitive auto-reaching
///   walk (C2) reaches the child's autos behind it;
/// - the Rig owns no autos and never reads the Bolt's autos directly, so there
///   is no other route to a cluster.
///
/// RED before the cold call site passes `Some(&ctx)` AND before instance-path
/// normalisation: either gap alone leaves `resolve_order` forming zero clusters,
/// so `check()` emits no diagnostic.
#[test]
fn derived_cost_coupling_forms_cluster_on_cold_path() {
    let compiled = parse_and_compile_with_stdlib(&over_cap_costed_source());

    let mut engine = no_solver_engine();
    let result = engine.check(&compiled);

    let count = count_coupling_approximated(&result.diagnostics);
    assert_eq!(
        count, 1,
        "a derived-cost coupling on a COMPILER-EMITTED module must form a δ cluster on \
         the cold path and surface exactly one W_COUPLING_APPROXIMATED (the child is \
         over-cap); got {}: {:?}",
        count, result.diagnostics,
    );
}
