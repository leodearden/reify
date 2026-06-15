//! Integration harness + gates for the unified build-DAG geometry-path
//! executors (task 4358 ε).
//!
//! δ (task 4357) landed `run_unified_pass` as a PURE planner and wired
//! `Engine::build()` to forward its diagnostics under
//! [`BuildScheduler::UnifiedDag`] (proven byte-preserving on acyclic modules by
//! `tests/unified_dag_cycle_contract.rs`). ε wires the schedule onto three
//! geometry-path executors (realization / selector-query / constraint), retires
//! the frozen pre-geometry `constraint_results` ("C7"), and lands the
//! auto-constraint guard decline — all behind the same scheduler flag.
//!
//! This file mirrors the `build_under` pattern from
//! `tests/unified_dag_cycle_contract.rs`, but the ε tests assert on geometry
//! ops, constraint verdicts, and diagnostics, so the shared helpers return the
//! FULL [`BuildResult`] (not just projected diagnostic triples). The scheduler
//! is selected through the deterministic `Engine::set_build_scheduler` test seam
//! (a `#[cfg(any(test, feature = "test-instrumentation"))]` setter reached via
//! the self-dev-dep with `test-instrumentation` enabled — see
//! `crates/reify-eval/Cargo.toml`), so these tests stay parallel-safe and
//! independent of the `unified-dag` cargo feature.
//!
//! The mock kernel's `with_query_result` / bbox / volume builders let a
//! geometry-backed constraint reach a DEFINITE verdict without OCCT; the
//! OCCT-dependent headline e2e tests (verdict-FLIP / volume-≠-all-fillet) are
//! owned by η, not ε.

// The shared `build_*` helpers below are consumed incrementally as the ε steps
// land their RED integration tests (steps 5/7/9/11). Until every helper has a
// caller, an unused helper would trip `-D warnings`; this scaffolding allow is
// intentional and is the prerequisite (`pre-1`) deliverable.
#![allow(dead_code)]

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{BuildResult, BuildScheduler, Engine};
use reify_ir::{ExportFormat, GeometryHandleId, GeometryKernel, GeometryOp, Satisfaction, Value};
use reify_test_support::{MockGeometryKernel, compile_source, compile_source_with_stdlib};

/// Compile `source`, build it on a FRESH engine under the given `scheduler`
/// with the supplied `kernel`, and return the full [`BuildResult`]
/// (`values`, `constraint_results`, `geometry_output`, `diagnostics`).
///
/// A fresh engine per call guarantees the cold-start `eval()` path runs (which
/// populates `eval_state.trace_map` that `run_unified_pass` consumes); a second
/// build on the same engine would hit the `eval_cached` path.
///
/// The `kernel` is taken by `Box<dyn GeometryKernel>` so callers can pass a
/// `MockGeometryKernel` pre-seeded with `with_query_result` / `with_bbox_result`
/// / `with_volume_result` replies (the ε constraint tests) OR the real
/// eval-test kernel.
pub fn build_with_kernel(
    source: &str,
    scheduler: BuildScheduler,
    kernel: Box<dyn GeometryKernel>,
) -> BuildResult {
    let compiled = compile_source(source);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    engine.build(&compiled, ExportFormat::Step)
}

/// Convenience over [`build_with_kernel`] using a default (unseeded)
/// [`MockGeometryKernel`] — for tests that only inspect recorded geometry ops
/// or diagnostics and need no canned query replies.
pub fn build_under(source: &str, scheduler: BuildScheduler) -> BuildResult {
    build_with_kernel(source, scheduler, Box::new(MockGeometryKernel::new()))
}

/// Like [`build_with_kernel`] but compiles `source` through the stdlib prelude
/// ([`compile_source_with_stdlib`]) so prelude names — DFM builtins
/// (`fits_build_volume`), geometry types (`Solid` / `Geometry`), and user
/// `constraint def`s — resolve. The geometry-backed constraint tests
/// (steps 7/9/11) need this because `fits_build_volume` lives in the std.process
/// prelude, whereas the curated-fillet test (step 5) uses only core geometry
/// builtins (`box` / `edges_at_height` / `fillet`) and so uses the no-stdlib
/// [`build_with_kernel`].
pub fn build_with_kernel_stdlib(
    source: &str,
    scheduler: BuildScheduler,
    kernel: Box<dyn GeometryKernel>,
) -> BuildResult {
    let compiled = compile_source_with_stdlib(source);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    engine.build(&compiled, ExportFormat::Step)
}

// ─────────────────────────────────────────────────────────────────────────────
// step-5 (RED): in-loop curated-fillet edge resolution under UnifiedDag.
// ─────────────────────────────────────────────────────────────────────────────

/// Under `UnifiedDag`, a curated fillet `fillet(b, edges_at_height(b, …), r)`
/// must dispatch with its edge selector ALREADY resolved — the recorded
/// `GeometryOp::Fillet` must carry a non-empty, curated `edges` list.
///
/// Mechanism: `let e = edges_at_height(b, …)` is a `Value::Selector` cell that
/// resolves to a `List<Geometry>` only in the topology-selector post-process
/// (legacy "P4"), which the legacy build loop runs AFTER every realization. So
/// when the fillet realization dispatches (legacy "P2"), `e` is still unresolved
/// and the 3-arg fillet eval arm (`geometry_ops.rs::compile_geometry_op`,
/// `ModifyKind::Fillet`) hits its `other => Err("curated edge selection is not
/// yet available on the current build pipeline …")` branch → the fillet
/// realization is rolled back (C9) and NO `Fillet` op reaches the kernel.
///
/// ε's schedule-driven driver (step-6) consumes `run_unified_pass`'s Kahn order,
/// which — because the fillet realization's dependency trace reads cell `e`
/// (`deps::extract_realization_dependencies` over the op args) — schedules the
/// `e` selector cell BEFORE the consuming fillet realization. The selector is
/// hydrated at its scheduled slot, so the fillet dispatches with curated edges.
///
/// RED until step-6: today (even under `UnifiedDag`, which δ wired only as an
/// additive diagnostic pass over the still-legacy build loop) the selector is
/// unresolved at dispatch, so `find_ops(Fillet)` is empty and the
/// `fillets.len() == 1` assertion fails.
///
/// Structural assertion ONLY (`edges` non-empty) — the OCCT volume-≠-all-fillet
/// e2e is η's, per PRD §8 / the ε test-strategy design decision.
#[test]
fn unified_dag_curated_fillet_resolves_edges_in_loop() {
    // `let e` is a named selector cell so the fillet realization's trace reads it
    // (an inlined selector would have no cell to schedule before the fillet).
    let source = r#"pub structure S {
    let b = box(10mm, 10mm, 10mm)
    let e = edges_at_height(b, 5mm, 1mm)
    let f = fillet(b, e, 2mm)
}"#;

    // The box is the first (and only successful) kernel `execute()` → handle 1,
    // so it is the parent solid the `edges_at_height` selector extracts against
    // (mirrors the "id=1 is the parent solid" convention in
    // tests/topology_filtered_selectors_mock.rs). Edge sub-handle ids are chosen
    // high (50/51/52) to avoid colliding with realization result handles.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(50);
    let e1 = GeometryHandleId(51);
    let e2 = GeometryHandleId(52);

    // A flat-bbox JSON whose z-extents both sit exactly on `z` (SI metres), so
    // every edge passes the `edges_at_height(b, 5mm, 1mm)` window
    // (|zmin - 0.005| ≤ 0.001 && |zmax - 0.005| ≤ 0.001). Format mirrors
    // tests/topology_filtered_selectors_mock.rs's `bbox_json`.
    let bbox_at = |z: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":{z},\
              \"xmax\":0.01,\"ymax\":0.01,\"zmax\":{z}}}"
        ))
    };

    let kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e0, e1, e2])
        .with_bbox_result(e0, bbox_at(0.005))
        .with_bbox_result(e1, bbox_at(0.005))
        .with_bbox_result(e2, bbox_at(0.005));
    // Capture the recorder BEFORE the kernel is boxed/moved into the engine.
    let ops_ref = kernel.operations_ref();

    let result = build_with_kernel(source, BuildScheduler::UnifiedDag, Box::new(kernel));

    let ops = ops_ref.lock().unwrap().clone();
    let fillets: Vec<&GeometryOp> = ops
        .iter()
        .map(|rec| &rec.op)
        .filter(|op| matches!(op, GeometryOp::Fillet { .. }))
        .collect();

    assert_eq!(
        fillets.len(),
        1,
        "UnifiedDag must dispatch exactly one curated Fillet op (the selector must \
         resolve in-loop before the fillet realization); recorded ops={:?}, \
         diagnostics={:?}",
        ops.iter().map(|r| &r.op).collect::<Vec<_>>(),
        result.diagnostics,
    );

    match fillets[0] {
        GeometryOp::Fillet { edges, .. } => assert!(
            !edges.is_empty(),
            "curated fillet must dispatch with a resolved, non-empty edge list \
             in-loop under UnifiedDag (an empty list is the all-edges back-compat \
             path / an unresolved selector)"
        ),
        _ => unreachable!("filtered to Fillet above"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// step-7 (RED): C7 retirement — post-geometry constraint re-check folds an
// INLINE geometry-query constraint to a DEFINITE verdict under UnifiedDag.
// ─────────────────────────────────────────────────────────────────────────────

/// A geometry-backed constraint written in the INLINE geometry-query form — the
/// `bounding_box(...)` leaves live directly inside the constraint predicate
/// (`fits_build_volume(bounding_box(part), bounding_box(envelope))`), NOT a
/// let-bound `let v = volume(part)` scalar — over two solids realized in the
/// SAME scope (single structure; the cross-sub `proc.build_volume` capstone is
/// step-9).
///
/// Under `UnifiedDag`, ε's Constraint executor (step-8) folds those inline
/// `bounding_box(...)` leaves against the live kernel + the realization-produced
/// `named_steps` BEFORE the kernel-less `SimpleConstraintChecker` runs, so the
/// constraint reaches a DEFINITE `Satisfaction` (`Satisfied`/`Violated`).
///
/// Contrast `LegacyMultiPass`: the Task 4229 post-realization re-check
/// (engine_build.rs) only re-evaluates constraints kernel-lessly against the
/// completed `values` map. A *let-bound* geometry cell would already be hydrated
/// by `post_process_geometry_queries` and so would fold — but an INLINE
/// `bounding_box(part)` leaf inside the constraint predicate has no value cell to
/// hydrate, so it stays `Undef` → `Indeterminate` (the intended, documented
/// `reify check` / build divergence per the ε design decision on check()).
///
/// RED until step-8: under `UnifiedDag` today the `constraint_results` still come
/// from the 4229 kernel-less re-check (ε has not yet added the Constraint
/// executor), so the inline leaf is unresolved → `Indeterminate` and the DEFINITE
/// assertion below fails.
#[test]
fn unified_dag_inline_geometry_constraint_is_definite_not_frozen() {
    // `FitsEnvelope` is a user `constraint def` whose predicate is the INLINE
    // geometry-query form (mirrors std.process `FitsBuildVolume`, but over two
    // same-scope `Solid` params instead of a cross-sub `proc.build_volume`, so
    // no cross-sub resolution is exercised here). `Widget` realizes both solids
    // and applies the constraint, so it lands in `constraint_results`.
    let source = r#"
constraint def FitsEnvelope {
    param part     : Solid
    param envelope : Solid
    fits_build_volume(bounding_box(part), bounding_box(envelope))
}

structure Widget {
    let part     = box(10mm, 10mm, 10mm)
    let envelope = box(100mm, 100mm, 100mm)
    constraint FitsEnvelope(part: part, envelope: envelope)
}
"#;

    // Declaration-order realization: `part` → handle 1, `envelope` → handle 2
    // (the MockGeometryKernel allocates ids 1,2,… across `execute()` calls —
    // same "first box → handle 1" convention as the step-5 curated-fillet test).
    // Both bboxes are valid, so `fits_build_volume` is decidable EITHER WAY: the
    // test asserts DEFINITE (Satisfied OR Violated), NOT a specific polarity, so
    // it is robust to the realization order of the two solids.
    let part = GeometryHandleId(1);
    let envelope = GeometryHandleId(2);
    // Axis-aligned bbox JSON wire reply (SI metres), mirroring the bbox replies
    // in `geometry_ops.rs`'s `rewrite_geometry_queries_folds_function_call_args`
    // unit test.
    let bbox = |hi: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\
              \"xmax\":{hi},\"ymax\":{hi},\"zmax\":{hi}}}"
        ))
    };
    // A fresh kernel per build (each `build()` takes ownership of its kernel).
    let make_kernel = || {
        MockGeometryKernel::new()
            .with_bbox_result(part, bbox(0.01))
            .with_bbox_result(envelope, bbox(0.10))
    };

    let unified =
        build_with_kernel_stdlib(source, BuildScheduler::UnifiedDag, Box::new(make_kernel()));
    let legacy =
        build_with_kernel_stdlib(source, BuildScheduler::LegacyMultiPass, Box::new(make_kernel()));

    let unified_sat = fits_envelope_satisfaction(&unified);
    let legacy_sat = fits_envelope_satisfaction(&legacy);

    // RED until step-8: UnifiedDag must fold the inline geometry-query leaves to
    // a DEFINITE verdict (un-freezing the C7 pre-geometry constraint_results).
    assert_ne!(
        unified_sat,
        Satisfaction::Indeterminate,
        "UnifiedDag must fold the inline geometry-query constraint to a DEFINITE \
         verdict (Satisfied/Violated), not the frozen kernel-less Indeterminate \
         (legacy_sat={legacy_sat:?}); constraint_results={:?}, diagnostics={:?}",
        unified.constraint_results,
        unified.diagnostics,
    );

    // Documented divergence: the LegacyMultiPass / `reify check` kernel-less
    // re-check cannot fold the inline leaf → Indeterminate.
    assert_eq!(
        legacy_sat,
        Satisfaction::Indeterminate,
        "LegacyMultiPass leaves the inline geometry-query leaf unresolved \
         (kernel-less 4229 re-check) → Indeterminate; constraint_results={:?}",
        legacy.constraint_results,
    );
}

/// Locate the single `FitsEnvelope` constraint entry's satisfaction in a
/// [`BuildResult`], matching on the constraint-def label prefix (the checker
/// labels a `constraint def` instantiation `"FitsEnvelope#0[0]"`). Panics with
/// the full constraint list if no such entry is present.
fn fits_envelope_satisfaction(result: &BuildResult) -> Satisfaction {
    result
        .constraint_results
        .iter()
        .find(|e| {
            e.label
                .as_deref()
                .is_some_and(|l| l.contains("FitsEnvelope"))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a FitsEnvelope constraint result, got: {:?}",
                result.constraint_results
            )
        })
        .satisfaction
}

// ─────────────────────────────────────────────────────────────────────────────
// step-9 (RED): cross-sub capstone — the 4275 `let proc = FdmPrinter()` form.
// bounding_box(proc.build_volume) must fold to a DEFINITE verdict under UnifiedDag.
// ─────────────────────────────────────────────────────────────────────────────

/// The 4275 cross-sub form: a `let proc = FdmPrinter()` structure instance whose
/// `Adding`-trait `build_volume` geometry member is referenced inside a
/// geometry-backed constraint via the stdlib `FitsBuildVolume` def
/// (`fits_build_volume(bounding_box(part), bounding_box(proc.build_volume))`).
///
/// IMPORTANT — the COMPILED shape (verified structurally): `proc.build_volume`
/// does NOT lower to a `self.<sub>.<member>` `CrossSubGeometryRef`. A
/// `let proc = FdmPrinter()` binding is a value cell of type
/// `StructureRef("FdmPrinter")` (it is NOT a `sub` component — a `sub` cannot be
/// passed as a constraint arg: `constraint FitsBuildVolume(proc: proc, …)` over a
/// `sub proc` fails to compile with "unresolved name: proc"). So the member
/// access compiles to:
///   `IndexAccess { object: ValueRef(SmallPart.proc):StructureRef("FdmPrinter"),
///                  index: Literal("build_volume") }`.
/// This is the original Fix-2 / design-decision-#4 member-access-on-StructureRef
/// shape — DISTINCT from the `CrossSubGeometryRef` shape step-4 handled (the
/// esc-4358-124 "no IndexAccess" correction applied only to the `self.`/`sub`
/// form, not this `let`-bound-instance form).
///
/// Under `UnifiedDag`, ε's Constraint executor (step-8) must fold BOTH inline
/// geometry-query leaves: `bounding_box(part)` (same-scope, already resolved at
/// step-7) AND `bounding_box(proc.build_volume)` (cross-`let`). The latter
/// resolves only once step-10 makes the executor (a) recognise the `IndexAccess`
/// member-access shape in `resolve_geometry_handle_arg` and (b) seed the child
/// `FdmPrinter` realization's `build_volume` handle under the composed
/// `"proc.build_volume"` key. With both leaves folded,
/// `fits_build_volume(bbox, bbox)` reaches a DEFINITE verdict.
///
/// RED until step-10: today the `IndexAccess` leaf is unresolvable
/// (`resolve_geometry_handle_arg` matches only `ValueRef`/`CrossSubGeometryRef`),
/// so it folds to `Undef` → the whole predicate is `Undef` → `Indeterminate`.
///
/// The verdict POLARITY depends on the realization order of the two boxes
/// (whichever handle the kernel hands to `build_volume` vs `part`), so the test
/// asserts DEFINITE (Satisfied OR Violated), never a fixed polarity — the
/// OCCT verdict-FLIP e2e (`dfm_fits_build_volume_4275_e2e`) is η's, per PRD §8.
#[test]
fn unified_dag_cross_sub_build_volume_constraint_is_definite() {
    // `FdmPrinter` MUST be declared before `SmallPart` (declaration order is
    // topological for the cross-`let` snapshot seeding — same forward-ref
    // limitation as `cross_sub_geometry_e2e.rs`).
    let source = r#"
import std.process

structure def FdmPrinter : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 10USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 45deg
}

structure SmallPart {
    let proc = FdmPrinter()
    let part = box(50mm, 50mm, 50mm)
    constraint FitsBuildVolume(proc: proc, part: part)
}
"#;

    // The two boxes realize as kernel handles 1,2,… across `execute()` calls
    // (FdmPrinter.build_volume then SmallPart.part, in declaration order). The
    // fold dispatches `bounding_box` against the ACTUAL realized handles read
    // back from `named_steps`, so we only need every realized handle to carry a
    // valid bbox reply. A few extra handles are seeded as a safety margin against
    // any prelude realization shifting the allocation. Both bboxes valid ⇒
    // `fits_build_volume` is decidable either way ⇒ DEFINITE.
    let bbox = |hi: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\
              \"xmax\":{hi},\"ymax\":{hi},\"zmax\":{hi}}}"
        ))
    };
    let make_kernel = || {
        let mut k = MockGeometryKernel::new();
        for i in 1..=4u64 {
            k = k.with_bbox_result(GeometryHandleId(i), bbox(if i == 1 { 0.20 } else { 0.05 }));
        }
        k
    };

    let unified =
        build_with_kernel_stdlib(source, BuildScheduler::UnifiedDag, Box::new(make_kernel()));
    let legacy =
        build_with_kernel_stdlib(source, BuildScheduler::LegacyMultiPass, Box::new(make_kernel()));

    let unified_sat = fits_build_volume_satisfaction(&unified);
    let legacy_sat = fits_build_volume_satisfaction(&legacy);

    // RED until step-10: the cross-`let` IndexAccess leaf
    // `bounding_box(proc.build_volume)` must fold to a DEFINITE verdict under
    // UnifiedDag (proving Fix-1 args recursion + the IndexAccess member-access
    // resolve + the cross-`let` snapshot seed composed end-to-end).
    assert_ne!(
        unified_sat,
        Satisfaction::Indeterminate,
        "UnifiedDag must fold the cross-`let` bounding_box(proc.build_volume) leaf to a \
         DEFINITE verdict (Satisfied/Violated), not Indeterminate (legacy_sat={legacy_sat:?}); \
         constraint_results={:?}, diagnostics={:?}",
        unified.constraint_results,
        unified.diagnostics,
    );

    // Documented divergence: the kernel-less LegacyMultiPass / `reify check`
    // re-check leaves the inline cross-`let` geometry-query leaf unresolved →
    // Indeterminate (same divergence as step-7's same-scope inline form).
    assert_eq!(
        legacy_sat,
        Satisfaction::Indeterminate,
        "LegacyMultiPass leaves the inline cross-`let` geometry-query leaf unresolved \
         → Indeterminate; constraint_results={:?}",
        legacy.constraint_results,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-11 (RED): auto-constraint guard decline. A geometry-backed constraint
// whose transitive auto-read closure reaches an `auto` parameter must be DECLINED
// (not given a bogus definite verdict) — leaving δ's E_EVAL_UNRESOLVED as the
// sole signal.
// ─────────────────────────────────────────────────────────────────────────────

/// A geometry-backed constraint whose transitive auto-read closure reaches an
/// `auto` parameter: `param w : Length = auto` feeds the realization
/// `param part : Solid = box(w, …)`, and the constraint `volume(part) <= 1000mm^3`
/// reads that realization's geometry. δ's `unresolved_diagnostics` guard
/// (`engine_fixpoint.rs`) fires `E_EVAL_UNRESOLVED` for exactly this class:
/// `Constraint(Widget#0).realization_reads ∋ Widget#realization[0]`, and that
/// realization directly reads the auto cell `Widget.w` (so it is in
/// `realizations_reaching_auto`).
///
/// SHAPE NOTE — why `param part : Solid` and NOT `let part = box(…)`: the
/// constraint→realization edge that δ's guard walks is `geometry_cell`, populated
/// by `EvaluationGraph::from_templates` ONLY for a value cell whose
/// `cell_type == Type::Geometry` and whose member name matches the realization's
/// name (graph.rs `from_templates_populates_realization_geometry_cell`). A
/// `param … : Solid = box(…)` cell satisfies that (a `Solid` cell IS
/// `Type::Geometry`); a `let part = box(…)` inferred-type binding leaves
/// `geometry_cell == None`, so `collect_constraint_realization_reads` finds no
/// backing realization and the constraint's `realization_reads` stays empty —
/// δ's guard would NOT fire. The `param : Solid` form is therefore required to
/// exercise the guard end-to-end through a real `build()`.
///
/// Asserts, under `UnifiedDag`:
///   (a) δ emits a `Severity::Error` / `DiagnosticCode::EvalUnresolved` for the
///       auto-reaching constraint (the δ contract — must keep firing), AND
///   (b) the constraint is DECLINED — it does NOT appear in `constraint_results`
///       as a DEFINITE `Satisfied`/`Violated` verdict.
///
/// RED until step-12: today (after step-8's Constraint executor) the executor
/// folds `volume(part)` against the degenerate `w = auto` realization and runs
/// the checker anyway, producing a bogus DEFINITE `Violated` verdict that
/// CONTRADICTS δ's `E_EVAL_UNRESOLVED` decline. step-12 makes the executor consult
/// the auto-read closure and SKIP such constraints (omitting them from the
/// driver-computed results → the merge leaves the pre-geometry `Indeterminate`),
/// so the definite verdict disappears and assertion (b) passes.
#[test]
fn unified_dag_auto_reaching_constraint_is_declined() {
    let source = r#"
structure Widget {
    param w    : Length = auto
    param part : Solid  = box(w, 10mm, 10mm)
    constraint volume(part) <= 1000mm^3
}
"#;

    // `w` is `auto` with NO solver, so `box(w, …)` realizes to a degenerate solid
    // (handle 1). Seeding a concrete volume reply (125000 mm³ > the 1000 mm³
    // bound) is what lets the step-8 executor reach a DEFINITE `Violated` — the
    // bogus verdict step-12 must suppress. Handles 1..=4 are seeded as a safety
    // margin against any prelude realization shifting the allocation.
    let make_kernel = || {
        let mut k = MockGeometryKernel::new();
        for i in 1..=4u64 {
            k = k.with_volume_result(GeometryHandleId(i), Value::Real(1.25e-4));
        }
        k
    };

    let unified =
        build_with_kernel_stdlib(source, BuildScheduler::UnifiedDag, Box::new(make_kernel()));

    // (a) δ's geometry-backed-constraint-on-auto guard must fire.
    let unresolved: Vec<_> = unified
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Error
                && d.code == Some(reify_core::DiagnosticCode::EvalUnresolved)
        })
        .collect();
    assert!(
        !unresolved.is_empty(),
        "δ must emit a Severity::Error E_EVAL_UNRESOLVED for the auto-reaching constraint; \
         diagnostics={:?}",
        unified
            .diagnostics
            .iter()
            .map(|d| (d.code, d.severity, d.message.clone()))
            .collect::<Vec<_>>(),
    );

    // (b) the auto-reaching constraint must be DECLINED — NO definite verdict for
    // it in constraint_results (it is either omitted or left Indeterminate). RED
    // today: the step-8 executor yields a bogus definite `Violated`.
    let bogus_definite = unified.constraint_results.iter().find(|e| {
        e.id.entity == "Widget"
            && matches!(
                e.satisfaction,
                Satisfaction::Satisfied | Satisfaction::Violated
            )
    });
    assert!(
        bogus_definite.is_none(),
        "the auto-reaching constraint must be DECLINED (left Indeterminate / omitted), not \
         given a bogus definite verdict that contradicts δ's E_EVAL_UNRESOLVED; got {:?}, \
         constraint_results={:?}",
        bogus_definite.map(|e| e.satisfaction),
        unified.constraint_results,
    );
}

/// Locate the single `FitsBuildVolume` constraint entry's satisfaction in a
/// [`BuildResult`] (the stdlib def's instantiation is labelled
/// `"FitsBuildVolume#0[0]"`). Mirrors [`fits_envelope_satisfaction`]. Panics with
/// the full constraint list if no such entry is present.
fn fits_build_volume_satisfaction(result: &BuildResult) -> Satisfaction {
    result
        .constraint_results
        .iter()
        .find(|e| {
            e.label
                .as_deref()
                .is_some_and(|l| l.contains("FitsBuildVolume"))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a FitsBuildVolume constraint result, got: {:?}",
                result.constraint_results
            )
        })
        .satisfaction
}
