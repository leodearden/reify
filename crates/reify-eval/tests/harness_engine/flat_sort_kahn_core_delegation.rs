//! Task 5045 (eval cell-commit ν): `dirty.rs`'s flat sort IS the one Kahn core.
//!
//! `dirty::topological_sort` used to run its own level-batched Kahn pass — a
//! second scheduling implementation (retired by this task) that diverged from
//! `engine_fixpoint::run_unified_pass_seeded`, the single scheduling core shared
//! by the cold/build/edit paths, along two independent axes:
//!
//! 1. **Drain shape.** The level sort emitted *every* in-degree-0 node as one
//!    batch per round; the core pops ONE node at a time off a
//!    `BTreeSet<DebugOrd>` ready set, so a deep chain drains completely before a
//!    `DebugOrd`-larger shallow sibling.
//! 2. **Edge kinds.** The level sort counted in-degree from `trace.reads` only,
//!    and its decrement loop fired only for `NodeId::Value` level members — so a
//!    `realization_reads` consumer could be emitted BEFORE its `Realization`
//!    producer. The core counts and decrements both edge kinds uniformly.
//!
//! These tests pin the delegation STRUCTURALLY: wherever practical they assert
//! against `run_unified_pass_seeded`'s own output rather than a hardcoded
//! literal, so the pin can never drift if `DebugOrd` or the core's tie-breaking
//! changes. A literal-order pin would have to be edited in lockstep with the
//! core; an equality-to-the-core pin cannot rot.

use std::collections::{HashMap, HashSet};

use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::demand::DemandRegistry;
use reify_eval::deps::DependencyTrace;
use reify_eval::dirty::{compute_eval_set, topological_sort};
use reify_eval::engine_fixpoint::run_unified_pass_seeded;

/// `NodeId::Value(S.<member>)`.
fn v(member: &str) -> NodeId {
    NodeId::Value(ValueCellId::new("S", member))
}

/// `NodeId::Realization(S#realization[<index>])`.
fn r(index: u32) -> NodeId {
    NodeId::Realization(RealizationNodeId::new("S", index))
}

/// A trace reading the given `S.<member>` value cells and nothing else.
fn reads(members: &[&str]) -> DependencyTrace {
    DependencyTrace {
        reads: members.iter().map(|m| ValueCellId::new("S", *m)).collect(),
        realization_reads: Vec::new(),
    }
}

/// A trace reading the given `S#realization[i]` producers and no value cells.
fn realization_reads(indices: &[u32]) -> DependencyTrace {
    DependencyTrace {
        reads: Vec::new(),
        realization_reads: indices
            .iter()
            .map(|i| RealizationNodeId::new("S", *i))
            .collect(),
    }
}

/// The chain-plus-shallow-sibling shape on which the two sorts provably diverge:
/// `a = ∅`, `b = reads[a]`, `c = reads[b]`, `z = ∅`.
///
/// Level order is `[a, z, b, c]` (`a` and `z` are both in-degree 0, so they
/// share level 0); the core's order is `[a, b, c, z]` (`a` pops, releasing `b`,
/// which sorts before `z` under `DebugOrd`, and so on down the chain). The same
/// divergence is documented in-tree at `tests/unified_dag_edit_path.rs`.
fn chain_and_sibling_fixture() -> (HashSet<NodeId>, HashMap<NodeId, DependencyTrace>) {
    let nodes: HashSet<NodeId> = [v("a"), v("b"), v("c"), v("z")].into_iter().collect();
    let traces: HashMap<NodeId, DependencyTrace> = [
        (v("a"), reads(&[])),
        (v("b"), reads(&["a"])),
        (v("c"), reads(&["b"])),
        (v("z"), reads(&[])),
    ]
    .into_iter()
    .collect();
    (nodes, traces)
}

/// Index of `node` in `order`, or a panic naming the whole order.
fn position(order: &[NodeId], node: &NodeId) -> usize {
    order
        .iter()
        .position(|n| n == node)
        .unwrap_or_else(|| panic!("{node:?} missing from schedule {order:?}"))
}

#[test]
fn flat_topological_sort_is_the_kahn_core() {
    let (nodes, traces) = chain_and_sibling_fixture();

    let flat = topological_sort(&nodes, &traces);
    let core = run_unified_pass_seeded(&traces, &nodes);

    assert_eq!(
        flat, core,
        "dirty::topological_sort must BE engine_fixpoint::run_unified_pass_seeded, \
         not a second Kahn implementation that happens to agree on easy shapes. \
         flat sort returned {flat:?}, the core returned {core:?} — this fixture \
         (chain a→b→c plus shallow sibling z) is exactly where a level-batched \
         drain diverges from the core's single-pop priority drain."
    );

    // Guard against a vacuous pass: the fixture must actually exercise all four
    // nodes, and the chain must be ordered (a valid linear extension) either way.
    assert_eq!(flat.len(), 4, "all four fixture nodes must be scheduled");
    assert!(position(&flat, &v("a")) < position(&flat, &v("b")));
    assert!(position(&flat, &v("b")) < position(&flat, &v("c")));
}

#[test]
fn flat_sort_orders_realization_producer_before_consumer() {
    // a = ∅ ; b = reads[a] ; Realization(S,0) = reads[b] ; w = realization_reads[R0]
    //
    // The chain depth on the producer side is LOAD-BEARING. A flat one-level
    // fixture would pass vacuously: `Debug` renders the variants as
    // "Realization(..." and "Value(...", and "R" < "V", so `DebugOrd` already
    // happens to place a Realization ahead of a Value inside a single batch.
    // Burying the producer two `reads` edges deep is what makes the missing
    // realization edge observable in the emitted order.
    let nodes: HashSet<NodeId> = [v("a"), v("b"), r(0), v("w")].into_iter().collect();
    let traces: HashMap<NodeId, DependencyTrace> = [
        (v("a"), reads(&[])),
        (v("b"), reads(&["a"])),
        (r(0), reads(&["b"])),
        (v("w"), realization_reads(&[0])),
    ]
    .into_iter()
    .collect();

    let flat = topological_sort(&nodes, &traces);

    assert_eq!(flat.len(), 4, "all four fixture nodes must be scheduled");
    let producer = position(&flat, &r(0));
    let consumer = position(&flat, &v("w"));
    assert!(
        producer < consumer,
        "the realization PRODUCER Realization(S,0) must be scheduled before its \
         realization_reads CONSUMER Value(S.w), but got producer at index \
         {producer} and consumer at index {consumer} in {flat:?}. A reads-only \
         in-degree count ignores `realization_reads` entirely, emitting the \
         consumer against a not-yet-produced handle."
    );

    let core = run_unified_pass_seeded(&traces, &nodes);
    assert_eq!(
        flat, core,
        "the flat sort must be realization-aware BECAUSE it delegates to the \
         core, not because it grew a parallel realization-edge pass: \
         flat {flat:?} vs core {core:?}"
    );
}

#[test]
fn compute_eval_set_is_the_kahn_core() {
    let (nodes, traces) = chain_and_sibling_fixture();

    // Every fixture node is both dirty and demanded, so the dirty ∩ demand
    // intersection `compute_eval_set` sorts is exactly `nodes`. `full_scope` is
    // the cold-path override that makes `is_demanded` total without needing a
    // whole `EvaluationGraph` to `rebuild_cone` against; `add_demand` records
    // the same four nodes as roots so the registry is not merely degenerate.
    let dirty: HashSet<NodeId> = nodes.clone();
    let mut demand = DemandRegistry::new();
    for node in &nodes {
        demand.add_demand(node.clone());
    }
    demand.set_full_scope(true);

    let eval_set = compute_eval_set(&dirty, &demand, &traces);
    let core = run_unified_pass_seeded(&traces, &nodes);

    assert_eq!(
        eval_set, core,
        "compute_eval_set must inherit the core's order transitively through \
         topological_sort — the demand-intersected entry point is what the edit \
         path actually calls. eval_set {eval_set:?} vs core {core:?}"
    );
    assert_eq!(
        eval_set.len(),
        4,
        "all four dirty ∩ demanded nodes must be scheduled"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3: RESULT-SET equivalence across the reorder.
//
// Steps 1-2 pin that the flat sort now emits the core's order. That is a pure
// ORDERING change, and the done-criteria signal is that ordering is all it is:
// both the retired level order and the core's order are valid linear extensions
// of the same dependency DAG, and a pure dataflow fold over a DAG is invariant
// across its linear extensions. These tests hold that claim to the fire on the
// surfaces that consume the changed order.
//
// They are characterization / safety-net assertions and are expected GREEN
// immediately — that IS the point. A RED here would mean the reorder surfaced a
// real trace-extraction gap (a read a node performs but does not declare in its
// `DependencyTrace`), which is a latent correctness bug the delegation EXPOSED
// rather than caused. The response is to escalate `design_concern` naming the
// divergent cell, never to weaken the assertion.
//
// The shared differential harness is `#[path]`-included once at the
// `harness_engine.rs` unit root (`common/differential.rs` deliberately is NOT
// re-exported through `common/mod.rs`), so this submodule reaches it through the
// crate root — the same way the `selective_demand_*` submodules do.
// ─────────────────────────────────────────────────────────────────────────────

use crate::differential::{
    GOLDEN_CORPUS, SEED_CORPUS, assert_cell_definite, assert_edit_matches_cold,
    assert_equivalent_or_allowed, build_case, build_case_keep_engine,
};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, Severity, Type};
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{GeometryKernel, Value};
use reify_test_support::{
    CompiledModuleBuilder, MockGeometryKernel, TopologyTemplateBuilder, compile_source,
    value_ref_typed,
};

/// The shape on which the two sorts provably diverge, as a real `.ri` module:
/// editing `p` dirties `{a, b, c, z}` where `a→b→c` is a depth-2 chain and `z`
/// is a `DebugOrd`-larger shallow sibling. The retired level sort re-evaluated
/// `[a, z, b, c]`; the core re-evaluates `[a, b, c, z]`. Same module as
/// `unified_dag_edit_path.rs`'s DRIVER_ORDER fixture, deliberately — this is the
/// sharpest available signal, since a corpus case that happens to be a chain or
/// happens to be flat would not exercise the divergence at all.
const DIVERGENT_SHAPE_P1_SRC: &str = r#"structure DriverOrder {
    param p: Real = 1.0
    let a = p * 1.0
    let b = a * 1.0
    let c = b * 1.0
    let z = p * 2.0
}"#;

/// Post-edit-equivalent cold reference: the same module with `p = 2.0`.
const DIVERGENT_SHAPE_P2_SRC: &str = r#"structure DriverOrder {
    param p: Real = 2.0
    let a = p * 1.0
    let b = a * 1.0
    let c = b * 1.0
    let z = p * 2.0
}"#;

fn fresh_engine(scheduler: BuildScheduler) -> Engine {
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new()) as Box<dyn GeometryKernel>),
    );
    engine.set_build_scheduler(scheduler);
    engine
}

#[test]
fn flat_sort_reorder_preserves_results_on_the_divergent_shape() {
    let p = reify_core::ValueCellId::new("DriverOrder", "p");

    // The warm/edited ValueMap must equal a cold eval of the post-edit-equivalent
    // module, bit-exact on canonical content-hash — across the exact re-evaluation
    // order the reorder changed.
    assert_edit_matches_cold(
        DIVERGENT_SHAPE_P1_SRC,
        &[(p, Value::Real(2.0))],
        DIVERGENT_SHAPE_P2_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
    );

    // Anti-vacuity: `assert_edit_matches_cold` compares warm against cold, so a
    // regression that left EVERY cell Undef on BOTH sides would satisfy it. Pin
    // the concrete cold values so an all-Undef collapse cannot pass silently.
    let mut engine = fresh_engine(BuildScheduler::LegacyMultiPass);
    let cold = engine.eval(&compile_source(DIVERGENT_SHAPE_P1_SRC));
    for (member, expected) in [
        ("a", Value::Real(1.0)),
        ("b", Value::Real(1.0)),
        ("c", Value::Real(1.0)),
        ("z", Value::Real(2.0)),
    ] {
        let cell = reify_core::ValueCellId::new("DriverOrder", member);
        assert_eq!(
            cold.values.get(&cell),
            Some(&expected),
            "cold eval of the divergent-shape fixture must produce a definite \
             {member} = {expected:?}; an Undef here means the fixture stopped \
             exercising the dataflow and the parity assertion above is vacuous"
        );
    }
}

#[test]
fn flat_sort_reorder_preserves_corpus_results() {
    // The `unified_dag_differential_corpus`-class gate, re-anchored on the
    // flat-sort surface: identical projected values, constraint verdicts,
    // diagnostics and geometry output under both schedulers, for every case.
    for case in SEED_CORPUS.iter().chain(GOLDEN_CORPUS.iter()) {
        let legacy = build_case(case, BuildScheduler::LegacyMultiPass);
        let unified = build_case(case, BuildScheduler::UnifiedDag);
        assert_equivalent_or_allowed(case, &legacy, &unified);
    }
}

/// The stale-Undef violations that are PRE-EXISTING on `build()` over this
/// corpus. Keyed `"<case>/<cell>"`, sorted. **Now EMPTY** — and deliberately kept
/// as a named, exact-equality baseline rather than an inline `is_empty()`, because
/// an empty baseline is the *precise* regression detector for the defect it used
/// to record (see below): the moment it comes back, this gate names both cells.
///
/// ## History — what used to be here, and why it is gone (task 5578)
///
/// This list carried `golden:tensegrity_t_prism/TPrism.{forces,solved}` and
/// attributed them to "the `@optimized` compute-dispatch/exemption path in
/// `invariants.rs`/`engine_build.rs`". **That attribution was wrong.** Neither
/// production file was at fault; the defect was in THIS harness's own wiring:
///
/// `common/differential.rs`'s `fresh_engine` — the single engine-construction
/// site every build helper routes through — never called
/// `reify_eval::compute_targets::register_compute_fns`. With no trampoline
/// registered for `solver::form_find_free`, `engine_eval.rs`'s `@optimized`
/// dispatch takes its documented else-branch: push a codeless Error diagnostic
/// ("no registered compute trampoline (falling back to body-inlining)") and
/// body-inline the fn. `form_find_free`'s body is the never-run sentinel
/// `{ FormFindResult() }` (`crates/reify-compiler/stdlib/tensegrity.ri`), whose
/// five params are ALL required with no defaults — so `TPrism.form` became a
/// non-Undef struct whose every field is Undef, and its two FieldAccess
/// consumers (`form.nodes` / `form.member_forces`) folded to Undef with all of
/// their declared reads RESOLVED. That is exactly the causeless staleness §6.1
/// exists to catch, so the checker was right to report it and `invariants.rs`
/// clause 5 was right not to exempt it: a consumer of a properly-dispatched
/// compute result left Undef IS a genuine violation, and widening the exemption
/// would have blinded the checker to real mis-scheduling forever.
///
/// Corroboration that the checker/build path were never implicated:
/// `no_stale_undef_invariant_gate.rs`'s `examples/` sweep — which DOES register
/// the trampolines — has always been green on this very file, and
/// `tests/golden/tensegrity_t_prism.txt` shows both cells fully resolved. Same
/// defect class as task 4458 one layer up (`cmd_build` skipping
/// `configured_eval_engine` ⇒ `Error + exit 0`).
///
/// This list is asserted by EXACT equality, matching the differential harness's
/// own `CorpusCase::allowed` convention: a NEW violation fails the gate (the
/// regression signal this test exists for), and a STALE entry also fails it, so
/// whoever fixes the underlying defect is told to delete the entry rather than
/// leaving a blanket exemption behind. Never widen this to a prefix/blanket
/// match — and never re-add these two entries to make a red run pass: their
/// return means `register_compute_fns` was dropped from `fresh_engine` again.
const PREEXISTING_STALE_UNDEF: &[&str] = &[];

#[test]
fn flat_sort_reorder_leaves_no_stale_undef() {
    // Task 4952's checker applied on the reordered path: a cell left Undef while
    // every one of its declared reads resolved is exactly the failure mode an
    // out-of-order schedule would produce, so this is the sharpest available
    // correctness gate on a scheduling change.
    //
    // The assertion is "exactly the known pre-existing set, and nothing else",
    // not "empty" — see PREEXISTING_STALE_UNDEF for why those two entries are
    // there and why the check is still tight. Detail strings are collected
    // alongside so a failure names the cells rather than just a count.
    let mut observed: Vec<String> = Vec::new();
    let mut details: Vec<String> = Vec::new();
    for case in SEED_CORPUS.iter().chain(GOLDEN_CORPUS.iter()) {
        for scheduler in [BuildScheduler::LegacyMultiPass, BuildScheduler::UnifiedDag] {
            let (engine, _result) = build_case_keep_engine(case, scheduler);
            for violation in engine.check_no_stale_undef() {
                // Both schedulers are swept, but the key is scheduler-free: a
                // violation under ONE scheduler only would appear as an unexpected
                // duplicate-free entry either way, and the per-scheduler detail is
                // preserved in `details` for the failure message.
                let key = format!("{}/{}", case.name, violation.cell);
                if !observed.contains(&key) {
                    observed.push(key.clone());
                }
                details.push(format!(
                    "[{}/{scheduler:?}] {} — {}",
                    case.name, violation.cell, violation.detail
                ));
            }
        }
    }
    observed.sort();

    let expected: Vec<String> = PREEXISTING_STALE_UNDEF
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        observed,
        expected,
        "the stale-Undef violation set over the differential corpus changed.\n\
         Every cell listed is Undef while all of its declared reads resolved.\n\
         NEW entries mean this reorder (or a later change to the ONE scheduling \
         core) left a demanded cell unevaluated — a real correctness regression, \
         do NOT add it to PREEXISTING_STALE_UNDEF to make this pass.\n\
         MISSING entries mean a pre-existing defect got fixed — delete its entry \
         from PREEXISTING_STALE_UNDEF.\n\
         full detail:\n  {}",
        details.join("\n  ")
    );
}

/// The POSITIVE companion to `flat_sort_reorder_leaves_no_stale_undef` (task 5578).
///
/// "Zero stale-Undef violations" is, on its own, satisfiable by degrading in the
/// OTHER direction: leave the PRODUCER (`TPrism.form`) Undef too, and clause 4
/// exempts both consumers — which is precisely how this defect could have hidden
/// itself. Pinning the two cells DEFINITE cannot be satisfied that way, so this
/// is strictly stronger than the negative assertion above and fails LOUDLY if the
/// `@optimized` `solver::form_find_free` trampoline ever silently degrades back
/// to body-inlining its all-required-params `FormFindResult()` sentinel.
///
/// `assert_cell_definite` is reused for exactly its documented purpose: it exists
/// because an equivalence gate "would otherwise silently compare two identical
/// failures" — the same two-identical-FAILURE-modes false pass this test blocks.
#[test]
fn optimized_compute_outputs_are_definite_on_build_surface() {
    let case = GOLDEN_CORPUS
        .iter()
        .find(|c| c.name == "golden:tensegrity_t_prism")
        .expect(
            "GOLDEN_CORPUS must carry the tensegrity_t_prism idiom — it is the only \
             corpus case that exercises an @optimized compute dispatch",
        );
    for scheduler in [BuildScheduler::LegacyMultiPass, BuildScheduler::UnifiedDag] {
        let result = build_case(case, scheduler);
        for member in ["solved", "forces"] {
            assert_cell_definite(
                &result,
                &ValueCellId::new("TPrism", member),
                &format!("optimized form_find_free output under {scheduler:?}"),
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// step-5: the two contracts the delegation must not silently break.
//
// (a) CYCLE-DROP is load-bearing, not incidental. Three production sites detect
//     a dependency cycle by exactly `sorted.len() < nodes.len()`:
//     `engine_eval::detect_let_cycle`, `engine_eval::build_combined_param_let_graph`
//     (twice — once on the combined param+let graph, once on the let-only
//     re-check) and `unfold::elaborate_child_lets_only`. If a future change made
//     the delegate append cyclic residue to return a total order, all three would
//     silently stop firing while their call sites still read as correct. That is
//     the failure this test exists to make loud.
//
// (b) The ONE genuinely new behavior of the delegation: a realization-edge cycle
//     inside a seed is now detected. It was invisible to the retired reads-only
//     level sort, which never counted realization edges at all and so scheduled
//     both members of such a cycle as though they were independent roots.
// ─────────────────────────────────────────────────────────────────────────────

/// A two-cycle over value cells (`x` reads `y`, `y` reads `x`) plus one acyclic
/// node must schedule ONLY the acyclic node — the cycle members are residue.
#[test]
fn delegated_sort_still_drops_value_cycle_members() {
    let nodes: HashSet<NodeId> = [v("x"), v("y"), v("ok")].into_iter().collect();
    let traces: HashMap<NodeId, DependencyTrace> = [
        (v("x"), reads(&["y"])),
        (v("y"), reads(&["x"])),
        (v("ok"), reads(&[])),
    ]
    .into_iter()
    .collect();

    let sorted = topological_sort(&nodes, &traces);

    assert_eq!(
        sorted.len(),
        1,
        "the 2-cycle members must be DROPPED, leaving only the acyclic node — \
         `sorted.len() < nodes.len()` is the cycle signal three production sites \
         key on, so a delegate that appended residue to reach len()==3 would \
         silently disable engine_eval::detect_let_cycle, \
         engine_eval::build_combined_param_let_graph (both of its checks) and \
         unfold::elaborate_child_lets_only. Got {sorted:?}"
    );
    assert_eq!(
        sorted,
        vec![v("ok")],
        "the single scheduled node must be the acyclic one, not an arbitrary \
         cycle member: {sorted:?}"
    );
}

/// A realization-edge two-cycle must be dropped ENTIRELY.
///
/// This is the one behavior the delegation genuinely adds. The retired
/// reads-only level sort counted in-degree from `trace.reads` alone and
/// decremented only for `NodeId::Value` level members, so neither member of a
/// `realization_reads` cycle ever had a non-zero in-degree: both were emitted in
/// level 0 as though independent. The realization-aware core gives each member
/// in-degree 1, so neither ever becomes ready and the schedule is empty.
///
/// Dropping them is correct. A realization cycle is a genuine bug, already gated
/// by `dirty::check_dag_complete` and `tests/assert_dag_complete_gate.rs`. And on
/// the edit path `engine_edit.rs`'s driver-schedule block retains a defensive
/// residue-append (`for node_id in &eval_set { if !scheduled … }`), so no
/// demanded cell is silently skipped there even when the schedule is short.
#[test]
fn delegated_sort_drops_realization_cycle_members() {
    let nodes: HashSet<NodeId> = [r(0), r(1)].into_iter().collect();
    let traces: HashMap<NodeId, DependencyTrace> = [
        (r(0), realization_reads(&[1])),
        (r(1), realization_reads(&[0])),
    ]
    .into_iter()
    .collect();

    let sorted = topological_sort(&nodes, &traces);

    assert!(
        sorted.is_empty(),
        "both members of a realization_reads 2-cycle must be residue, so the \
         schedule is EMPTY. The retired reads-only level sort emitted both in \
         level 0 (it never counted realization edges), which is exactly the \
         blind spot the delegation closes. Got {sorted:?}"
    );
}

/// The unit-level cycle-drop contract, tied to the user-visible behavior it
/// protects: a template whose two let-bindings read each other must still emit
/// the circular-let-binding error diagnostic. `dirty::topological_sort` dropping
/// the cycle members is the mechanism that detection rests on, so this and
/// `delegated_sort_still_drops_value_cycle_members` are two ends of one contract.
#[test]
fn cyclic_let_bindings_still_emit_the_circular_dependency_diagnostic() {
    // `x = y` / `y = x` — a direct 2-cycle between two let-bindings.
    let template = TopologyTemplateBuilder::new("Cyc")
        .let_binding(
            "Cyc",
            "x",
            Type::Int,
            value_ref_typed("Cyc", "y", Type::Int),
        )
        .let_binding(
            "Cyc",
            "y",
            Type::Int,
            value_ref_typed("Cyc", "x", Type::Int),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let mut engine = fresh_engine(BuildScheduler::LegacyMultiPass);
    let result = engine.eval(&module);

    let has_cycle_error = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && (d.message.contains("circular")
                || d.message.contains("cycle")
                || d.message.contains("cyclic"))
            && d.message.contains('x')
            && d.message.contains('y')
    });
    assert!(
        has_cycle_error,
        "a let-binding 2-cycle must still produce an error diagnostic naming both \
         cells — this is the user-visible consequence of the cycle-drop contract, \
         and it fires ONLY because topological_sort returns fewer nodes than it \
         was given. Got: {:?}",
        result.diagnostics
    );

    // Neither cycle member may hold a definite value.
    for member in ["x", "y"] {
        let got = result
            .values
            .get(&reify_core::ValueCellId::new("Cyc", member));
        assert!(
            got.is_none() || got == Some(&Value::Undef),
            "Cyc.{member} must be absent or Undef inside a dependency cycle, got {got:?}"
        );
    }
}
