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
// Amendment (reviewer_comprehensive, architecture_coherence): the non-query
// `FunctionCall`-args recursion arm ε added to `geometry_ops::rewrite_geometry_
// queries` is SHARED geometry-fold code, reached on BOTH scheduler paths — NOT a
// UnifiedDag-only addition. On `LegacyMultiPass` it runs inside
// `post_process_geometry_queries` → `try_eval_geometry_query` case (b) for any
// VALUE CELL whose `default_expr` is a non-query `FunctionCall` wrapping a
// geometry-query leaf. This is the one documented exception to ε's
// "LegacyMultiPass stays byte-identical" claim (a shared correctness fix:
// Undef → real value). This test pins that shared fix on the LEGACY path.
// ─────────────────────────────────────────────────────────────────────────────

/// Under `LegacyMultiPass`, a VALUE CELL `let fits = fits_build_volume(
/// bounding_box(part), bounding_box(envelope))` — a non-query `FunctionCall`
/// (`fits_build_volume`, a builtin → a genuine `FunctionCall` IR node, not an
/// inlined `BinOp`) wrapping two `bounding_box(..)` geometry-query leaves — must
/// fold to a concrete `Value::Bool`, NOT the pre-fix `Value::Undef`.
///
/// Path: `post_process_geometry_queries` iterates `template.value_cells` and calls
/// `try_eval_geometry_query` on `fits`'s `default_expr`. `is_geometry_query_call`
/// is false (outer call is `fits_build_volume`, not in the recognised leaf set) but
/// `expr_contains_geometry_query` is true (the inner `bounding_box` leaves), so
/// case (b) NESTED runs: `rewrite_geometry_queries` folds the leaves, then the
/// kernel-less `eval_expr` evaluates `fits_build_volume(Literal(bbox),
/// Literal(bbox))` → `Bool`.
///
/// Before ε's FunctionCall-args arm the outer `fits_build_volume(..)` call hit the
/// `_ => expr.clone()` fallthrough, so its inner `bounding_box(..)` leaves stayed
/// un-folded and the kernel-less `eval_expr` could not resolve them → `Undef`. So
/// this asserts the SHARED correctness fix on the LegacyMultiPass scheduler — it is
/// GREEN now and would RED if the recursion arm were reverted (the cell would drop
/// back to `Undef`). It exercises the value-cell geometry fold
/// (`post_process_geometry_queries`), distinct from the constraint 4229 re-check
/// the other ε constraint tests cover.
#[test]
fn legacy_multipass_folds_nonquery_functioncall_value_cell() {
    let source = r#"
structure Widget {
    let part     = box(10mm, 10mm, 10mm)
    let envelope = box(100mm, 100mm, 100mm)
    let fits     = fits_build_volume(bounding_box(part), bounding_box(envelope))
}
"#;

    // Declaration-order realization: `part` → handle 1, `envelope` → handle 2
    // (same "first box → handle 1" convention as the step-7 test). Both bboxes
    // valid ⇒ `fits_build_volume` is decidable ⇒ a concrete `Bool` either way; the
    // test asserts FOLDED (Bool, not Undef), not a specific polarity.
    let part = GeometryHandleId(1);
    let envelope = GeometryHandleId(2);
    let bbox = |hi: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\
              \"xmax\":{hi},\"ymax\":{hi},\"zmax\":{hi}}}"
        ))
    };
    let kernel = MockGeometryKernel::new()
        .with_bbox_result(part, bbox(0.01))
        .with_bbox_result(envelope, bbox(0.10));

    // LegacyMultiPass (the production default) — the path the byte-identical claim
    // covers, and the one this shared fix also affects for this cell shape.
    let legacy =
        build_with_kernel_stdlib(source, BuildScheduler::LegacyMultiPass, Box::new(kernel));

    let fits_id = reify_core::ValueCellId::new("Widget", "fits");
    let fits = legacy.values.get(&fits_id).unwrap_or_else(|| {
        panic!(
            "expected Widget.fits in values; cells={:?}, diagnostics={:?}",
            legacy.values.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            legacy.diagnostics,
        )
    });
    assert!(
        matches!(fits, Value::Bool(_)),
        "LegacyMultiPass must fold the inner bounding_box(..) leaves of the non-query \
         fits_build_volume(..) VALUE cell (shared geometry_ops FunctionCall-args recursion \
         arm), yielding a concrete Bool — not the pre-fix Undef; got {fits:?}, \
         diagnostics={:?}",
        legacy.diagnostics,
    );
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
// Amendment (reviewer_comprehensive, robustness_silent_incorrectness): the
// cross-`let` structure-instance handle seeding in `check_constraints_post_geometry`
// keys child handles by structure DEF name, NOT by `let`-binding instance, so it
// cannot disambiguate two same-def instances. ε's SAFE-DEGRADATION amendment
// DECLINES the cross-`let` fold for any def bound >1× in a template (leaving the
// leaf Undef → Indeterminate) rather than folding against the wrong (shared,
// last-snapshotted) handle — a silently-incorrect DEFINITE verdict. #4628 tracks
// per-binding snapshot keying that would let multi-instance folds resolve to a
// definite per-instance verdict.
// ─────────────────────────────────────────────────────────────────────────────

/// Two same-def `FdmPrinter` instances (`a`, `b`) in one structure, each feeding a
/// geometry-backed `FitsBuildVolume` constraint over its own `build_volume`. The
/// def-name-keyed cross-`let` snapshot cannot tell `a.build_volume` from
/// `b.build_volume`, so ε DECLINES the fold for the (count == 2) `FdmPrinter` def:
/// both `bounding_box(<inst>.build_volume)` leaves stay `Undef` → both constraints
/// degrade to `Indeterminate`, NEVER a silently-wrong DEFINITE verdict folded
/// against the shared (last-snapshotted) handle. The same-scope `bounding_box(part)`
/// leaf still folds, so the only `Undef` is the undisambiguable cross-`let` leaf.
///
/// Contrast `unified_dag_cross_sub_build_volume_constraint_is_definite`: the
/// SINGLE-instance `let proc = FdmPrinter()` form (count == 1) still folds to a
/// DEFINITE verdict. #4628 tracks the per-binding snapshot keying that would let
/// this multi-instance form fold to a definite per-instance verdict instead — at
/// which point this test flips from asserting Indeterminate to a definite verdict.
#[test]
fn unified_dag_multi_instance_cross_let_declines_fold() {
    // `FdmPrinter` MUST be declared before `MultiPrinter` (declaration order is
    // topological for the cross-`let` snapshot seeding — same forward-ref
    // limitation as the step-9 capstone).
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

structure MultiPrinter {
    let a = FdmPrinter()
    let b = FdmPrinter()
    let part = box(50mm, 50mm, 50mm)
    constraint FitsBuildVolume(proc: a, part: part)
    constraint FitsBuildVolume(proc: b, part: part)
}
"#;

    // Valid bbox replies for every realized handle (the part body + both printers'
    // build_volume bodies, plus a prelude margin). The cross-`let` leaves are
    // DECLINED before dispatch, so these replies only ever resolve the same-scope
    // `bounding_box(part)`; each `bounding_box(<inst>.build_volume)` stays `Undef`.
    let bbox = |hi: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\
              \"xmax\":{hi},\"ymax\":{hi},\"zmax\":{hi}}}"
        ))
    };
    let make_kernel = || {
        let mut k = MockGeometryKernel::new();
        for i in 1..=6u64 {
            k = k.with_bbox_result(GeometryHandleId(i), bbox(0.05));
        }
        k
    };

    let unified =
        build_with_kernel_stdlib(source, BuildScheduler::UnifiedDag, Box::new(make_kernel()));

    // The declined cross-`let` fold leaves the constraints in `constraint_results`
    // (a declined FOLD ≠ a dropped constraint — that is step-12's auto-guard), just
    // with an unresolvable leaf → Indeterminate.
    let fits: Vec<_> = unified
        .constraint_results
        .iter()
        .filter(|e| e.label.as_deref().is_some_and(|l| l.contains("FitsBuildVolume")))
        .collect();
    assert!(
        !fits.is_empty(),
        "expected FitsBuildVolume constraint entries under UnifiedDag (declined fold \
         must not drop the constraint); constraint_results={:?}, diagnostics={:?}",
        unified.constraint_results,
        unified.diagnostics,
    );
    assert!(
        fits.iter().all(|e| e.satisfaction == Satisfaction::Indeterminate),
        "a multi-instance same-def cross-`let` fold must DEGRADE SAFELY to \
         Indeterminate (the fold is DECLINED — the def-name-keyed snapshot cannot \
         disambiguate `a.build_volume` from `b.build_volume`), never a silently-wrong \
         DEFINITE verdict folded against the shared handle; got {:?}, diagnostics={:?}",
        fits.iter()
            .map(|e| (e.label.clone(), e.satisfaction))
            .collect::<Vec<_>>(),
        unified.diagnostics,
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

// ─────────────────────────────────────────────────────────────────────────────
// Amendment (reviewer_comprehensive, test_coverage): the schedule-driven build
// loop's fallback-append branch — `for r_idx in 0..len { if !realized.contains(…)
// { Realize(r_idx) } }` in `engine_build.rs` — appends every realization NOT
// covered by the Kahn schedule (residue downstream of a cycle, or a node with no
// trace entry) in declaration order. The happy-path ε tests above only exercise
// SCHEDULED realizations; this pins the residue/fallback path: under a cyclic
// module, every realization must still dispatch EXACTLY ONCE — no duplicate
// `Realize` (the `realized` dedup set) and no dropped realization (the append).
// ─────────────────────────────────────────────────────────────────────────────

/// A module mixing SCHEDULED and RESIDUE realizations under `UnifiedDag`:
///
///   * `a ↔ b` is a mutual `let`-cycle (the same shape proven cyclic by
///     `tests/unified_dag_cycle_contract.rs`), so δ's planner reports the cycle
///     and any realization transitively reading it lands in `residue`.
///   * `p` (`box(10mm,…)`) and `q` (`box(20mm,…)`) are independent, valid box
///     realizations with in-degree 0 → the Kahn schedule covers them (inserted
///     into the `realized` dedup set during the schedule walk).
///   * `r = box(a, …)` reads the cyclic cell `a` → it is downstream of the cycle
///     → `residue` → its `NodeId::Realization` is NOT in `pass.schedule`, so it
///     reaches dispatch ONLY through the `for r_idx in 0..len { if !realized … }`
///     fallback-append branch. `a` folds to `Value::Undef` (so `r`'s recorded box
///     carries `width: Undef`), but its `height`/`depth` are the concrete literal
///     `5mm` — distinct from `p`'s 10mm and `q`'s 20mm — so `r` is STILL directly
///     observable, by HEIGHT, in the recorded ops. This lets the test pin the
///     fallback-append branch dispatching the residue realization exactly once.
///
/// Every box carries `height == width`, so matching recorded boxes by their
/// concrete `Scalar` HEIGHT keys all three realizations uniformly (`p` → 0.010m,
/// `q` → 0.020m, `r` → 0.005m) even though `r`'s WIDTH folded to `Undef`.
///
/// Asserts, under `UnifiedDag`:
///   (a) δ surfaces `DiagnosticCode::EvalCycle` (confirming the planner is on the
///       cyclic/residue path, so the fallback-append branch is live), AND
///   (b) each realization dispatched EXACTLY ONCE — `p` (10mm) and `q` (20mm) via
///       the Kahn schedule + `realized` dedup, and `r` (5mm) via the fallback
///       append. A broken dedup set would double-count a scheduled box; a removed
///       fallback append would zero-count `r`; a dropped realization would
///       zero-count its box.
#[test]
fn unified_dag_residue_realizations_dispatch_exactly_once() {
    // `let a` / `let b` form a Length-typed mutual cycle (the `+ 1mm` literal
    // anchors both to `Length`), so `r = box(a, …)` type-checks while `a` itself
    // is cyclic (→ `Undef` at eval, → residue at planning).
    let source = r#"structure S {
    let a = b + 1mm
    let b = a + 1mm
    let p = box(10mm, 10mm, 10mm)
    let q = box(20mm, 20mm, 20mm)
    let r = box(a, 5mm, 5mm)
}"#;

    // Capture the op recorder BEFORE the kernel is boxed/moved into the engine
    // (mirrors `unified_dag_curated_fillet_resolves_edges_in_loop`).
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let result = build_with_kernel(source, BuildScheduler::UnifiedDag, Box::new(kernel));

    // (a) the planner reports the a↔b cycle → `r` is residue → the fallback-append
    //     branch is the only path that can dispatch it.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some(reify_core::DiagnosticCode::EvalCycle)),
        "expected DiagnosticCode::EvalCycle under UnifiedDag for the a↔b let-cycle \
         (residue must be non-empty so the fallback-append branch runs); diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.severity, d.message.clone()))
            .collect::<Vec<_>>(),
    );

    // (b) exactly-once dispatch for every realization, keyed by the box's concrete
    //     `Scalar` HEIGHT (robust to `r`'s `Undef` width).
    let ops = ops_ref.lock().unwrap().clone();
    let recorded: Vec<&GeometryOp> = ops.iter().map(|rec| &rec.op).collect();
    let box_height_count = |si_metres: f64| -> usize {
        ops.iter()
            .filter(|rec| match &rec.op {
                GeometryOp::Box {
                    height: Value::Scalar { si_value, .. },
                    ..
                } => (*si_value - si_metres).abs() < 1e-9,
                _ => false,
            })
            .count()
    };
    assert_eq!(
        box_height_count(0.010),
        1,
        "the scheduled `p` (box 10mm) must dispatch EXACTLY ONCE — no duplicate \
         Realize, no dropped realization; recorded ops={recorded:?}",
    );
    assert_eq!(
        box_height_count(0.020),
        1,
        "the scheduled `q` (box 20mm) must dispatch EXACTLY ONCE — no duplicate \
         Realize, no dropped realization; recorded ops={recorded:?}",
    );
    assert_eq!(
        box_height_count(0.005),
        1,
        "the RESIDUE `r` (box 5mm, reads cyclic `a`) must dispatch EXACTLY ONCE via \
         the `for r_idx in 0..len {{ if !realized … }}` fallback-append branch — \
         not dropped (zero) and not double-appended (two); recorded ops={recorded:?}",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Amendment (reviewer_comprehensive, test_coverage): the NEGATIVE side of the
// `hydrate_value_cell_in_loop` branch-(b) gate
// (`realization_read_cells.contains(&cell.id)`). The POSITIVE side — a
// realization-consumed selector resolving to a `List` — is pinned by
// `unified_dag_curated_fillet_resolves_edges_in_loop`. The NEGATIVE side: a
// COMPOSITION-ONLY selector cell (read by a `union`/`intersect`/`difference` value
// cell, NOT by any realization) must NOT be in `realization_read_cells`, so branch
// (b) is SKIPPED and it keeps its `Value::Selector` descriptor — which
// `reconstruct_selector_value` REQUIRES of a composition child.
//
// Observability: asserting on the COMPOSED cell's final value cannot distinguish a
// broken gate, because the whole-template `run_post_processes` re-runs AFTER the
// schedule loop (engine_build.rs) and would re-resolve the composition to a
// descriptor regardless. The gate is only observable through a realization
// dispatched IN-LOOP: a curated fillet consuming the composition. If the gate were
// dropped, the composition's child selectors would resolve to `List`s in-loop,
// `reconstruct_selector_value` would reject them (→ `None`), the composition would
// fail to resolve to a `List` before the fillet's scheduled slot, and the fillet
// would dispatch with EMPTY edges (the all-edges fallback) — an already-recorded op
// `run_post_processes` cannot un-record. So the fillet's non-empty edge list is the
// reliable pin for the negative gate.
// ─────────────────────────────────────────────────────────────────────────────

/// Under `UnifiedDag`, a curated `fillet(b, combined, r)` whose edge arg is a
/// selector COMPOSITION `combined = union(e1, e2)` over two composition-only edge
/// selectors must dispatch with a resolved, non-empty `edges` list.
///
/// Gate wiring: the fillet realization's dependency trace reads `combined` (+ `b`),
/// NOT `e1`/`e2` directly — so `realization_read_cells = {combined, b}` and
/// `e1`/`e2` are absent. `hydrate_value_cell_in_loop` therefore SKIPS branch (b)
/// for `e1`/`e2` (keeping them `Value::Selector` descriptors) and TAKES branch (b)
/// for `combined` (realization-read) → `resolve_selector_to_list(union(e1, e2))`,
/// which `reconstruct_selector_value`-wraps the two surviving descriptors and
/// resolves the union to a concrete `List<Geometry>` before the fillet's slot.
///
/// This pins BOTH sides of the gate at once: the negative side (`e1`/`e2`
/// descriptors preserved — branch (b) skipped) is the precondition for the positive
/// side (`combined` resolves to a `List` — branch (b) taken) to succeed. A dropped
/// gate would resolve `e1`/`e2` to `List`s in-loop, break the union reconstruction,
/// and leave the fillet with empty (all-edges) `edges`.
///
/// Structural assertion only (`edges` non-empty), mirroring
/// `unified_dag_curated_fillet_resolves_edges_in_loop` — the OCCT volume-≠-all-fillet
/// e2e is η's, per PRD §8.
#[test]
fn unified_dag_curated_fillet_over_selector_composition_resolves_edges() {
    // `e1`/`e2` feed the `union` COMPOSITION, never a realization; the fillet reads
    // only `combined`. Both selectors use the same height window so the union is
    // non-empty (and dedups to the same edge set — the test asserts non-empty, not a
    // specific count).
    let source = r#"pub structure S {
    let b = box(10mm, 10mm, 10mm)
    let e1 = edges_at_height(b, 5mm, 1mm)
    let e2 = edges_at_height(b, 5mm, 1mm)
    let combined = union(e1, e2)
    let f = fillet(b, combined, 2mm)
}"#;

    // Same kernel convention as `unified_dag_curated_fillet_resolves_edges_in_loop`:
    // the box is the first (and only) `execute()` → parent handle 1; edge sub-handle
    // ids are high (50/51/52) to avoid colliding with realization result handles; a
    // flat bbox on z=5mm passes the `edges_at_height(b, 5mm, 1mm)` window for each.
    let parent = GeometryHandleId(1);
    let edge_ids = [GeometryHandleId(50), GeometryHandleId(51), GeometryHandleId(52)];
    let bbox_at = |z: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":{z},\
              \"xmax\":0.01,\"ymax\":0.01,\"zmax\":{z}}}"
        ))
    };
    let mut kernel = MockGeometryKernel::new().with_extracted_edges(parent, edge_ids.to_vec());
    for id in edge_ids {
        kernel = kernel.with_bbox_result(id, bbox_at(0.005));
    }
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
        "UnifiedDag must dispatch exactly one curated Fillet op over the selector \
         composition (the composition-only child selectors must keep their descriptors \
         so the union resolves to a List before the fillet slot); recorded ops={:?}, \
         diagnostics={:?}",
        ops.iter().map(|r| &r.op).collect::<Vec<_>>(),
        result.diagnostics,
    );

    match fillets[0] {
        GeometryOp::Fillet { edges, .. } => assert!(
            !edges.is_empty(),
            "the curated fillet over `union(e1, e2)` must dispatch with a resolved, \
             non-empty edge list — proving branch (b) was SKIPPED for the composition-only \
             `e1`/`e2` (descriptors preserved) so `reconstruct_selector_value` could wrap \
             them. An empty list means a child selector was wrongly resolved to a List \
             in-loop and the union reconstruction failed (all-edges fallback)."
        ),
        _ => unreachable!("filtered to Fillet above"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// task #4668 (step-5, RED): bare-let sibling target resolves to the canonical
// named_steps[b] handle — NOT a fresh inlined box.
// ─────────────────────────────────────────────────────────────────────────────

/// Under `UnifiedDag`, `fillet(b, e, r)` where `b` is a bare let-name sibling
/// must target the CANONICAL `named_steps["b"]` handle, NOT a freshly-inlined
/// duplicate box.
///
/// Before the fix (task #4668 step-2): `compile_geometry_call` falls through to
/// the inline fallback for bare Ident("b") → re-compiles the box inline →
/// GeomRef::Step(a fresh box) → TWO Box ops at the kernel and the Fillet's
/// `target` is the inlined handle, not `named_steps["b"]`.
///
/// After the fix: the sibling pre-check intercepts bare "b" → GeomRef::Sub("b")
/// → eval resolves Sub("b") → `named_steps["b"].id` → EXACTLY ONE Box op and
/// Fillet.target == the Box's result_handle.
///
/// RED on baseline (before step-2): two boxes; Fillet.target is the fresh
/// inlined box handle (NOT the named_steps["b"] handle).
/// GREEN after step-2 + step-4 (the eval Sub resolver already handles bare
/// keys correctly once the compiler emits Sub("b")).
#[test]
fn unified_dag_curated_fillet_targets_canonical_box_handle() {
    // Same b/e/f structure as `unified_dag_curated_fillet_resolves_edges_in_loop`.
    let source = r#"pub structure S {
    let b = box(10mm, 10mm, 10mm)
    let e = edges_at_height(b, 5mm, 1mm)
    let f = fillet(b, e, 2mm)
}"#;

    // Same mock-kernel seeding convention: box is first execute() → handle 1.
    // Edge sub-handles 50/51/52 sit above the realization result-handle range.
    // A flat bbox on z=5mm passes the edges_at_height(b, 5mm, 1mm) window.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(50);
    let e1 = GeometryHandleId(51);
    let e2 = GeometryHandleId(52);
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
    let ops_ref = kernel.operations_ref();

    let result = build_with_kernel(source, BuildScheduler::UnifiedDag, Box::new(kernel));

    let ops = ops_ref.lock().unwrap().clone();
    let recorded_ops: Vec<_> = ops.iter().map(|r| &r.op).collect();

    // (1) Exactly ONE Box op — the canonical `b` realization.
    //     Before fix: TWO boxes (the named `b` + the inlined fresh box inside
    //     the fillet realization).
    let box_recs: Vec<_> = ops
        .iter()
        .filter(|r| matches!(r.op, GeometryOp::Box { .. }))
        .collect();
    assert_eq!(
        box_recs.len(),
        1,
        "UnifiedDag must record EXACTLY ONE Box op for `let b` (the canonical \
         named_steps[\"b\"] realization); the inlined-rebuild bug emits TWO. \
         Recorded ops={:?}, diagnostics={:?}",
        recorded_ops,
        result.diagnostics,
    );
    let box_handle = box_recs[0].result_handle;

    // (2) The Fillet's target must equal the Box's result_handle.
    //     Before fix: target is the inlined-fresh box handle (≠ named_steps["b"]).
    let fillet_recs: Vec<_> = ops
        .iter()
        .filter(|r| matches!(r.op, GeometryOp::Fillet { .. }))
        .collect();
    assert_eq!(
        fillet_recs.len(),
        1,
        "UnifiedDag must dispatch exactly one Fillet op; recorded ops={:?}, \
         diagnostics={:?}",
        recorded_ops,
        result.diagnostics,
    );
    match &fillet_recs[0].op {
        GeometryOp::Fillet { target, .. } => assert_eq!(
            *target,
            box_handle,
            "Fillet.target must be the canonical `b` box handle ({:?}), not a fresh \
             inlined rebuild ({:?}). Two boxes == the pre-fix inline-rebuild bug; \
             recorded ops={:?}",
            box_handle,
            *target,
            recorded_ops,
        ),
        _ => unreachable!("filtered to Fillet above"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// amend: geometry sibling-realization cycle → error diagnostics (task #4668)
// Closes the coverage gap noted in let_scope_tests.rs
// `cyclic_refs_through_transforms_resolve_to_sub`: the compile-time detector
// no longer fires; the eval step must still surface an error for the cycle.
// ─────────────────────────────────────────────────────────────────────────────

/// Under `UnifiedDag`, mutually-referencing geometry lets (`let a = translate(b,…)`
/// and `let b = rotate(a,…)`) each receive a `GeomRef::Sub(sibling)` from the
/// sibling pre-check introduced in task #4668 step-2.  At eval time the first
/// pass runs both realizations in declaration order; neither Sub ref resolves
/// (each needs the OTHER realization to have run first) → both fail with
/// "unresolvable GeomRef::Sub" error diagnostics and no geometry is produced.
///
/// This test closes the loop on the comment in
/// `cyclic_refs_through_transforms_resolve_to_sub` (let_scope_tests.rs): that test
/// proves the compile step emits `Sub` refs without a compile-time error; this test
/// proves the eval step surfaces an error (no silent success), so the user can see
/// that the mutual dependency cannot be resolved.
///
/// Note: the Kahn SCC cycle detector (`E_EVAL_CYCLE`) fires only when the trace map
/// contains cycle edges.  Realization traces are recorded only on SUCCESS; both
/// realizations fail on the first eval (Sub refs unresolvable), so their traces are
/// empty and no `EvalCycle` diagnostic is emitted.  The "unresolvable Sub" errors
/// are the observable signal for this class of cycle.
#[test]
fn geometry_sibling_realization_cycle_produces_error_diagnostics() {
    // `a` and `b` each name the other as a bare Ident geometry arg.
    // After task #4668 step-2, both compile to GeomRef::Sub("b") / GeomRef::Sub("a").
    // At eval time neither Sub ref resolves: a runs first → Sub("b") fails (b not yet
    // in named_steps); b runs second → Sub("a") may or may not resolve depending on
    // whether a's result handle was recorded; in practice both fail.
    let source = r#"structure S {
    let a = translate(b, 1, 0, 0)
    let b = rotate(a, 0, 0, 1, 90)
}"#;

    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let result = build_with_kernel(source, BuildScheduler::UnifiedDag, Box::new(kernel));

    // (1) At least one error diagnostic — the cycle is NOT silently swallowed.
    //     Before task #4668: this same source produced a COMPILE-TIME cycle error.
    //     After: compile passes, but eval surfaces "unresolvable GeomRef::Sub" errors.
    let error_count = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .count();
    assert!(
        error_count > 0,
        "a mutual geometry realization cycle must produce at least one Error diagnostic \
         at eval time — the cycle must not succeed silently (task #4668 changes the \
         compile-time visiting-set guard to eval-time unresolvable-Sub errors); \
         diagnostics={:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.severity, d.message.clone()))
            .collect::<Vec<_>>(),
    );

    // (2) No geometry ops are recorded — neither realization should produce output.
    let ops = ops_ref.lock().unwrap().clone();
    assert!(
        ops.is_empty(),
        "a mutual geometry realization cycle must produce NO kernel geometry ops \
         (both realizations fail with unresolvable Sub refs); recorded ops={:?}",
        ops.iter().map(|r| &r.op).collect::<Vec<_>>(),
    );
}
