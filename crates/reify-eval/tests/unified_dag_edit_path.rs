//! θ2 (task 4531) edit-path unified-driver test binary.
//!
//! Pins the design-doc "warm output == cold output becomes structural" claim on
//! the EDIT surface: `edit_param` / `edit_source` / `edit_check` must order their
//! value re-evaluation through the SAME unified driver
//! (`engine_fixpoint::run_unified_pass`) as cold/build/concurrent, retiring edit's
//! hand-maintained second scheduler (solver wave-2 + Phase-3 flip dedup) before the
//! ι (#4362) cutover.
//!
//! The shared differential harness (`common/differential.rs`) is `#[path]`-included
//! so this binary reuses the θ projection + parity helpers
//! (`assert_edit_matches_cold`, `assert_edit_source_matches_cold`,
//! `project_eval_values`) with zero edits to existing shared files.
//!
//! Steps land RED tests here incrementally (guard flip via edit, solver autos via
//! edit, collection grow → upstream edit, edit_source/edit_check mirror, P0 latency
//! gate). This file starts with the harness smoke tests that prove the pre-1
//! infrastructure is wired and GREEN on the existing edit behavior.
#![allow(dead_code, unused_imports)]

#[path = "common/differential.rs"]
mod differential;

use differential::{
    BRACKET_EDIT_SRC, WARM_PREDICATE_K5_SRC, WARM_PREDICATE_SRC, assert_edit_matches_cold,
    assert_edit_matches_cold_with_solver, bracket_source,
};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, Type, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::journal::EventKind;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{CompiledExpr, GeometryKernel, Value};
use reify_test_support::{
    CompiledModuleBuilder, MockGeometryKernel, TopologyTemplateBuilder, compile_source, mm,
    value_ref_typed,
};

// ─────────────────────────────────────────────────────────────────────────────
// pre-1 harness smoke tests.
//
// These exercise `assert_edit_matches_cold` on a known-good pure-scalar fixture
// (`WARM_PREDICATE_SRC` k=2.0 → edit k=5.0 → cold `WARM_PREDICATE_K5_SRC` k=5.0),
// which the LEGACY edit_param already satisfies — so the prerequisite is GREEN
// before any production change. The structural RED tests arrive in later steps.
// ─────────────────────────────────────────────────────────────────────────────

/// pre-1: the edit-vs-cold parity harness wires up and is GREEN on the existing
/// `edit_param` behavior under `LegacyMultiPass` — editing `WarmPredicate.k` from
/// 2.0 to 5.0 yields the same values as a cold eval of the k=5.0 source.
#[test]
fn harness_edit_param_matches_cold_legacy() {
    assert_edit_matches_cold(
        WARM_PREDICATE_SRC,
        &[(ValueCellId::new("WarmPredicate", "k"), Value::Real(5.0))],
        WARM_PREDICATE_K5_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
    );
}

/// pre-1: the same parity holds under `UnifiedDag` — `edit_param` is
/// scheduler-agnostic by construction (never reads `build_scheduler`), so the
/// harness must be GREEN under both schedulers.
#[test]
fn harness_edit_param_matches_cold_unified() {
    assert_edit_matches_cold(
        WARM_PREDICATE_SRC,
        &[(ValueCellId::new("WarmPredicate", "k"), Value::Real(5.0))],
        WARM_PREDICATE_K5_SRC,
        BuildScheduler::UnifiedDag,
        false,
    );
}

/// pre-1: the bracket latency fixture is loadable both inline
/// ([`BRACKET_EDIT_SRC`]) and from disk ([`bracket_source`]). The on-disk
/// `examples/bracket.ri` shares the `Bracket` structure shape, so both compile and
/// the inline fixture is non-empty — the deterministic input for the step-15 P0
/// latency gate.
#[test]
fn harness_bracket_fixture_loads() {
    assert!(BRACKET_EDIT_SRC.contains("structure Bracket"));
    let on_disk = bracket_source();
    assert!(
        on_disk.contains("structure Bracket"),
        "examples/bracket.ri should define `structure Bracket`"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3 (RED): edit_param re-evaluates in the unified DRIVER's schedule order.
//
// The fixture is chosen so the LEGACY `compute_eval_set` order (level-by-level
// Kahn, `dirty::compute_levels`) differs from `run_unified_pass`'s GLOBAL
// DebugOrd-priority Kahn order. Editing `p` dirties {a, b, c, z}:
//   a = p          (reads param p — external to the eval_set ⇒ in-degree 0)
//   b = a          (reads a)
//   c = b          (reads b)              a→b→c is a depth-2 chain
//   z = p          (reads param p — external ⇒ in-degree 0, DebugOrd-large)
//
// Within the eval_set {a,b,c,z}:
//   • LEGACY level order:  [a, z, b, c]  — level 0 = {a, z}, so the shallow
//     sibling `z` is emitted BEFORE the chain's interior `b`/`c`.
//   • DRIVER global Kahn:  [a, b, c, z]  — once `a` is popped, `b` (DebugOrd <
//     `z`) is immediately ready and drains the whole chain before `z`.
//
// The Started-event sequence therefore distinguishes the two orderings. Legacy
// edit_param iterates `compute_eval_set` order → [a, z, b, c]; after step-4 the
// executor walks the driver schedule → [a, b, c, z]. RED until step-4.
// ─────────────────────────────────────────────────────────────────────────────

const DRIVER_ORDER_P1_SRC: &str = r#"structure DriverOrder {
    param p: Real = 1.0
    let a = p * 1.0
    let b = a * 1.0
    let c = b * 1.0
    let z = p * 2.0
}"#;

/// The post-edit-equivalent cold reference: same module with `p = 2.0`.
const DRIVER_ORDER_P2_SRC: &str = r#"structure DriverOrder {
    param p: Real = 2.0
    let a = p * 1.0
    let b = a * 1.0
    let c = b * 1.0
    let z = p * 2.0
}"#;

/// Construct a fresh kernel-backed engine pinned to `scheduler`. Mirrors the
/// inline constructor used across the unified-dag test binaries.
fn fresh_engine(scheduler: BuildScheduler) -> Engine {
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new()) as Box<dyn GeometryKernel>),
    );
    engine.set_build_scheduler(scheduler);
    engine
}

/// step-3 (RED until step-4): `edit_param` must re-evaluate the dirty∩demand value
/// cells in the unified driver's Kahn schedule order — observed via the journal
/// `EvalEvent{kind: Started}` sequence — AND produce values equal to a cold `eval()`
/// of the post-edit-equivalent module. This pins "edit rides the same ordering core
/// as cold" (structural warm==cold), which legacy `compute_eval_set` order does not
/// guarantee.
#[test]
fn edit_param_revaluates_in_driver_schedule_order() {
    let p = ValueCellId::new("DriverOrder", "p");

    // (1) Value parity — the edited value map equals cold eval() of the p=2.0
    // module. Already GREEN under legacy (documents the full warm==cold claim).
    assert_edit_matches_cold(
        DRIVER_ORDER_P1_SRC,
        &[(p.clone(), Value::Real(2.0))],
        DRIVER_ORDER_P2_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
    );

    // (2) Ordering — the Started-event sequence over the eval_set must equal the
    // driver's Kahn order [a, b, c, z], NOT legacy level order [a, z, b, c].
    let compiled = compile_source(DRIVER_ORDER_P1_SRC);
    let mut engine = fresh_engine(BuildScheduler::LegacyMultiPass);
    engine.eval(&compiled);

    let len_before = engine.journal().all_events().len();
    engine
        .edit_param(p.clone(), Value::Real(2.0))
        .expect("edit_param must succeed");

    // Only Value nodes emit Started events inside the value loop, so this is the
    // re-evaluation order restricted to the eval_set.
    let started: Vec<NodeId> = engine.journal().all_events()[len_before..]
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Started))
        .map(|e| e.node_id.clone())
        .collect();

    let v = |field: &str| NodeId::Value(ValueCellId::new("DriverOrder", field));
    assert_eq!(
        started,
        vec![v("a"), v("b"), v("c"), v("z")],
        "edit_param must re-evaluate in the unified driver's Kahn order [a, b, c, z]; \
         legacy compute_eval_set level-order is [a, z, b, c] (RED until step-4 routes \
         the value loop through run_unified_pass_seeded). Observed: {started:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-5: GUARD-FLIP-VIA-EDIT parity (GREEN safety net).
//
// Editing a structure-controlling Bool param (`use_thick`) flips the active
// branch of a `where … else` guarded group; a downstream cone (`derived`,
// `derived2`) reads the flipped member. The edited ValueMap — active/inactive
// `effective` + the downstream cone + the `__guard_*` cell — must equal a cold
// `eval()` of the post-flip source (`use_thick = false`).
//
// FRAMING (not RED): unlike step-3's ordering observable, guard-flip-via-edit_param
// ALREADY achieves cold parity under the legacy Phase-1/Phase-3 re-elaboration
// (also exercised by guard_eval.rs's 30 tests), so this differential is GREEN from
// the start. It is the behavior-preservation SPEC the guard re-elaboration refactor
// must keep green — step-6 wires the elaborate→re-elaborate→reseed OUTER LOOP and
// step-12 retires the Phase-3 flip-then-revert dedup; this test is the net that
// proves the outer-loop reseed SUBSUMES Phase-3 (no value/topology regression).
// Mirrors the plan's design decision #1 (existing tests are the preservation net).
// ─────────────────────────────────────────────────────────────────────────────

const GUARD_FLIP_TRUE_SRC: &str = r#"structure GuardFlip {
    param thickness: Length = 5mm
    param use_thick: Bool = true

    where use_thick {
        let effective = thickness * 2.0
    } else {
        let effective = thickness
    }

    let derived = effective * 3.0
    let derived2 = derived + thickness
}"#;

/// Post-flip cold reference: same module with `use_thick = false`, so the
/// else-branch activates and `effective = thickness` (5mm).
///
/// IMPORTANT (esc-4531-36): the downstream cone does NOT re-propagate off the
/// flipped member. Cold's deferred-third-pass guard model computes `derived`
/// (and `derived2`) in the MAIN pass while `effective` is still `Undef` →
/// `Undef`, and the guard pass re-elaborates `effective`=5mm WITHOUT re-running
/// dependents. So a COLD eval of this source yields effective=5mm, derived=Undef,
/// derived2=Undef — NOT 15mm/20mm (empirically verified). The edit-vs-cold parity
/// this fixture pins is therefore `undef==undef` on the downstream cone, and the
/// step-6 guard reseed is bounded to members-only specifically to preserve it.
/// (Re-homing cold's guarded-member eval onto the driver so warm==cold==logically-
/// correct 15mm is a follow-up that depends on #4531; engine_eval.rs is out of
/// scope here.)
const GUARD_FLIP_FALSE_SRC: &str = r#"structure GuardFlip {
    param thickness: Length = 5mm
    param use_thick: Bool = false

    where use_thick {
        let effective = thickness * 2.0
    } else {
        let effective = thickness
    }

    let derived = effective * 3.0
    let derived2 = derived + thickness
}"#;

/// step-5 (GREEN safety net): editing the guard's controlling Bool param to flip
/// the active branch yields values equal to a cold eval of the post-flip source —
/// the flipped member `effective`=5mm AND the downstream cone (`derived`/`derived2`),
/// which is `undef==undef` under cold's deferred-third-pass semantics (see
/// GUARD_FLIP_FALSE_SRC, esc-4531-36). Pins the warm==cold guard claim that the
/// step-6 bounded outer-loop reseed and step-12 Phase-3 retirement preserve.
#[test]
fn edit_param_guard_flip_matches_cold() {
    let use_thick = ValueCellId::new("GuardFlip", "use_thick");
    assert_edit_matches_cold(
        GUARD_FLIP_TRUE_SRC,
        &[(use_thick, Value::Bool(false))],
        GUARD_FLIP_FALSE_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-7: SOLVER-AUTOS-VIA-EDIT parity (safety net for the step-8 wave-2 deletion).
//
// `x` is an `auto` Length uniquely determined by `constraint x == base + 2mm`,
// which reads the upstream param `base`. A downstream chain `y = x * 2.0`,
// `z = y + 1mm` reads the RESOLVED auto but NOT `base` directly — so `y`/`z` are
// NOT in `base`'s original dirty cone. They become dirty only after the edit
// Resolution phase re-runs the solver and `x` changes value.
//
// Editing `base` (3mm → 7mm):
//   cold(base=3mm):  x = 5mm,  y = 10mm,  z = 11mm
//   cold(base=7mm):  x = 9mm,  y = 18mm,  z = 19mm
// The edit path must re-resolve `x` (Resolution phase) AND re-propagate `y`/`z`
// (today via the hand-rolled wave-2 at engine_edit.rs:1532-1588) to match the
// cold base=7mm reference.
//
// FRAMING (GREEN safety net, mirrors step-5): the current wave-2 already achieves
// this parity (incremental.rs::edit_param_let_binding_re_evaluates_after_re_resolution
// pins the single-hop case), so this differential is GREEN at HEAD. It is the
// behavior-preservation SPEC the step-8 refactor must keep green: step-8 DELETES
// wave-2 and re-dirties `all_resolved_ids ∩ demand` → reseeds the unified driver
// for one additional value pass. This test proves that reseed SUBSUMES wave-2 (no
// value regression on the downstream-let-not-in-original-cone re-propagation), and
// that the edit path's solver-problem construction does not diverge from cold's
// `build_solver_problem`.
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-edit solver fixture: `base = 3mm` ⇒ `x == 5mm`, `y = 10mm`, `z = 11mm`.
const SOLVER_AUTO_BASE3_SRC: &str = r#"structure SolverAuto {
    param base : Length = 3mm
    param x : Length = auto
    constraint x == base + 2mm
    let y = x * 2.0
    let z = y + 1mm
}"#;

/// Post-edit cold reference: `base = 7mm` ⇒ `x == 9mm`, `y = 18mm`, `z = 19mm`.
/// Same structure/cell IDs as [`SOLVER_AUTO_BASE3_SRC`]; only `base`'s default
/// differs, so the cold solver resolves `x` from the template constraint and
/// propagates the downstream chain.
const SOLVER_AUTO_BASE7_SRC: &str = r#"structure SolverAuto {
    param base : Length = 7mm
    param x : Length = auto
    constraint x == base + 2mm
    let y = x * 2.0
    let z = y + 1mm
}"#;

/// step-7 (GREEN safety net): editing the upstream `base` re-runs the constraint
/// solver so the `auto` `x` re-resolves, and the downstream chain `y`/`z` (which
/// read the resolved `x`, NOT `base` — so they are outside `base`'s original dirty
/// cone) re-propagates to the SAME values a cold `eval()` of the post-edit source
/// produces. Pins the wave-2-subsumption contract step-8 must preserve: the
/// downstream let must re-propagate via the driver reseed, not a hand-rolled second
/// wave. Asserted under BOTH schedulers (`edit_param` is scheduler-agnostic).
#[test]
fn edit_param_solver_auto_re_resolution_matches_cold() {
    let base = ValueCellId::new("SolverAuto", "base");
    for scheduler in [BuildScheduler::LegacyMultiPass, BuildScheduler::UnifiedDag] {
        assert_edit_matches_cold_with_solver(
            SOLVER_AUTO_BASE3_SRC,
            &[(base.clone(), mm(7.0))],
            SOLVER_AUTO_BASE7_SRC,
            scheduler,
            false,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// step-9: COLLECTION-GROW → UPSTREAM-EDIT re-propagation (safety net for step-10).
//
// A `List<Bolt>` collection sub whose instance COUNT is driven by the
// structure-controlling `__count_bolts = n` cell, and whose instances' `diameter`
// default is a cross-structure value_ref to the parent param `Parent.bolt_d`
// (pure value-propagation through `default_expr`, no solver). The sequence
//   1. eval (n=2, bolt_d=0.01m)       → bolts[0],bolts[1] created
//   2. edit_param(n, 4)               → GROW: bolts[2],bolts[3] created; task-4530
//                                       rebuilds reverse_index/trace_map/demand
//                                       against the grown graph
//   3. edit_param(bolt_d, 0.02m)      → ALL 4 instances — incl. the grown
//                                       bolts[2],bolts[3] absent from bolt_d's
//                                       ORIGINAL dirty cone — must re-propagate to
//                                       0.02m over the REBUILT edges
// must yield 0.02m on every instance (the grown ones inclusive).
//
// WHY EDIT-PATH CORRECTNESS, NOT edit-vs-COLD parity (discovery, task 4531):
// the plan framed this as an edit-vs-cold differential mirroring
// collection_sub_eval.rs `grown_collection_instances_track_upstream_param_edits`
// (task-4530 step-1). That is NOT achievable: a fresh COLD `eval()` of this fixture
// resolves every `Parent.bolts[i].diameter` to **Undef** — cold's SCOPED instance
// evaluation does not resolve a collection instance's cross-structure value_ref up
// to a parent param, whereas the EDIT path (flat values-map eval) does. (The auto+
// forall alternative — `forall b in bolts: constraint b.diameter == bolt_d` — fares
// no better: it PANICS cold eval via the `collect_member_list` eval-order invariant.)
// So "warm == cold" is structurally inapplicable to parent-param-dependent
// collection instances — NOT because the edit path is wrong (it produces the
// correct 0.02m) but because cold eval is deficient here. That cold-eval gap lives
// in engine_eval.rs instance scoping, OUT OF SCOPE for this edit-path re-homing
// task; the named mirror never cold-evals the grown source, so it never surfaced.
// This test therefore pins the LOAD-BEARING, achievable contract: the edit path
// re-propagates upstream edits to grown instances over the rebuilt edges.
//
// GREEN safety net (mirrors step-5/step-7): task-4530 already rebuilds the dep
// structures after the grow and the edit already re-propagates to grown instances
// (the named mirror passes the warm side), so this is GREEN at HEAD. It is the
// behavior-preservation SPEC the step-10 reseed-over-rebuilt-edges must keep green:
// grown instances must evaluate over the CURRENT dependency structure.
// ─────────────────────────────────────────────────────────────────────────────

/// Build the grown-collection fixture module with the given `n` and `bolt_d`
/// defaults. `Bolt.diameter` defaults to a cross-structure value_ref to
/// `Parent.bolt_d`; `Parent` drives the collection count via the
/// structure-controlling `__count_bolts = n` cell. Mirrors
/// collection_sub_eval.rs::grown_collection_instances_track_upstream_param_edits.
fn grown_collection_module(n_default: i64, bolt_d_m: f64) -> reify_compiler::CompiledModule {
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(value_ref_typed("Parent", "bolt_d", Type::length())),
        )
        .build();
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "bolt_d",
            Type::length(),
            Some(CompiledExpr::literal(
                Value::length(bolt_d_m),
                Type::length(),
            )),
        )
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(n_default), Type::Int)),
        )
        .let_binding(
            "Parent",
            "__count_bolts",
            Type::Int,
            value_ref_typed("Parent", "n", Type::Int),
        )
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build()
}

/// step-9 (GREEN safety net): growing a collection via `edit_param(n, 4)` then
/// editing the upstream `bolt_d` re-propagates to ALL instances — including the
/// grown `bolts[2]`/`bolts[3]` not present at the original edit. Pins the
/// task-4530-rebuild → driver-reseed contract step-10 must preserve: grown
/// instances evaluate over the CURRENT (rebuilt) dependency structure. The upstream
/// scalars ARE compared against cold (cold resolves them); the instance diameters
/// are asserted on the edit-path value (cold returns Undef for them — see the
/// section comment for why edit-vs-cold parity is inapplicable to parent-dependent
/// collection instances). Asserted under BOTH schedulers (`edit_param` is
/// scheduler-agnostic).
#[test]
fn edit_param_collection_grow_then_upstream_edit_repropagates_to_grown_instances() {
    let n = ValueCellId::new("Parent", "n");
    let bolt_d = ValueCellId::new("Parent", "bolt_d");
    let dia = |i: usize| ValueCellId::new(format!("Parent.bolts[{i}]"), "diameter");

    for scheduler in [BuildScheduler::LegacyMultiPass, BuildScheduler::UnifiedDag] {
        // eval n=2/bolt_d=0.01m, GROW to n=4, then edit upstream bolt_d→0.02m.
        let mut engine = fresh_engine(scheduler);
        engine.eval(&grown_collection_module(2, 0.01));
        let grown = engine
            .edit_param(n.clone(), Value::Int(4))
            .expect("edit_param(n, 4) must grow the collection");

        // Sanity: the grow produced exactly 4 live instances over the rebuilt graph
        // (so the subsequent upstream edit has a non-trivial cone to re-propagate).
        let live_instances = (0..6).filter(|&i| grown.values.contains(&dia(i))).count();
        assert_eq!(
            live_instances, 4,
            "[{scheduler:?}] expected exactly 4 bolt instances after grow to n=4, got {live_instances}"
        );

        let warm = engine
            .edit_param(bolt_d.clone(), Value::length(0.02))
            .expect("edit_param(bolt_d, 0.02) must re-propagate to grown instances");

        // CONTRACT: every instance — incl. the grown bolts[2],bolts[3], which were
        // absent from bolt_d's ORIGINAL (pre-grow) dirty cone — re-propagates to the
        // edited upstream value over the rebuilt edges. A stale/absent grown instance
        // is the exact failure the task-4530 rebuild + step-10 reseed prevent.
        for i in 0..4 {
            assert_eq!(
                warm.values.get(&dia(i)),
                Some(&Value::length(0.02)),
                "[{scheduler:?}] Parent.bolts[{i}].diameter must re-propagate to 0.02m after \
                 grow+upstream edit (grown instances over rebuilt edges), got {:?}",
                warm.values.get(&dia(i))
            );
        }

        // Upstream scalars DO cold-resolve, so pin edit-vs-cold parity on them.
        let mut cold_engine = fresh_engine(scheduler);
        let cold = cold_engine.eval(&grown_collection_module(4, 0.02));
        for cell in [&bolt_d, &n] {
            assert_eq!(
                warm.values.get(cell),
                cold.values.get(cell),
                "[{scheduler:?}] {cell} edit-vs-cold parity: warm={:?} cold={:?}",
                warm.values.get(cell),
                cold.values.get(cell)
            );
        }
    }
}
