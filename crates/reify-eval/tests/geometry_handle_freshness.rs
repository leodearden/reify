//! Integration tests for GHR-δ geometry-handle freshness propagation.
//!
//! PRD `docs/prds/v0_3/geometry-handle-runtime.md` §5 + §7.1 (producer-side
//! "Freshness walk: Realization → ValueCell edge"): a value cell holding a
//! `Value::GeometryHandle` has an implicit dependency on the upstream
//! Realization named in its `realization_ref`. Per §5 the cell's freshness is
//! "the meet of (existing VC-input freshness, all referenced Realization
//! freshness)". These tests exercise that meet end-to-end over a real
//! `Engine::build`, asserting both:
//!
//!   (1) the GH cell tracks its backing Realization's freshness directly
//!       (PRD §7.1 boundary row — mark the upstream Realization, observe the
//!       cell follow), and
//!   (2) a `width → Realization → GeometryHandle` cascade drives the GH cell
//!       Pending through the freshness walk's Realization→ValueCell fan-out
//!       (S8) once the cached trace carries `realization_reads` (S10) and the
//!       Realization is recorded as a freshness-bearing cache node on the
//!       build success path (esc-3606-37 ruling step 1).
//!
//! **RED** until S10 wires `realization_reads` into the GH cell's *cached*
//! trace end-to-end and records the geometry-backed Realization in the cache
//! during `build()`. Before that, the param-cell record path stores an empty
//! `DependencyTrace` and no `NodeId::Realization` cache entry exists on the
//! success path, so neither leg of the meet is observable.
//!
//! ## StructureInstance cascade intentionally NOT asserted (esc-3606-37 Finding 2)
//!
//! An earlier draft of S9 asserted that a parent `structure def Asm { sub w :
//! Widget }` StructureInstance cell also goes Pending when `Widget.geometry`
//! does. That is **structurally impossible for this fixture**: SIR flattens
//! `sub w = Widget()` into independent param cells (`Asm.w.geometry`,
//! `Asm.w.width`) that are *copies* — none of them *reads* `Widget.geometry`,
//! so there is no VC→VC edge to carry the cascade. A genuine SI-cascade
//! fixture needs a cell that actually consumes the GH cell, e.g.
//! `let v = volume(self.w.geometry)`, which depends on later GHR phases
//! (volume/query over a `Value::GeometryHandle`). Deferred as a follow-up;
//! the §7.1 acceptance contract never demanded an SI cascade for this row.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::{RealizationNodeId, ValueCellId};
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_ir::{ExportFormat, Freshness, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

/// A single geometry-bearing structure: `geometry` is a `Solid` realization
/// that reads the scalar param `width`, so the eval graph carries
/// `width → Realization[0] → ValueCell(geometry)`.
///
/// `width` is declared first so the box() call has no forward reference; the
/// geometry param is still the only realization, hence `RealizationNodeId::new
/// ("Widget", 0)`.
const WIDGET_SRC: &str = r#"structure def Widget {
    param width : Length = 10mm
    param geometry : Solid = box(width, 20mm, 30mm)
}"#;

/// Build the Widget fixture with a mock kernel, asserting a clean build.
fn build_widget() -> Engine {
    let compiled = compile_source(WIDGET_SRC);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time errors; got: {:?}",
        compile_errors
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| format!("[{:?}] {}", d.severity, d.message))
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time errors; got: {:?}",
        build_errors
    );

    // Sanity: the geometry cell really did hydrate to a GeometryHandle so the
    // realization↔cell name-match (and hence the freshness edge) is in play.
    let geom_cell = ValueCellId::new("Widget", "geometry");
    assert!(
        matches!(result.values.get_or_undef(&geom_cell), Value::GeometryHandle { .. }),
        "Widget.geometry must hydrate to a Value::GeometryHandle"
    );

    engine
}

/// PRIMARY (PRD §7.1): a `Value::GeometryHandle` cell folds its backing
/// Realization's freshness into its own (PRD §5 "meet of VC-input freshness +
/// referenced Realization freshness").
///
/// The PRD §7.1 row phrases this as "mark the upstream Realization as
/// Intermediate; observe the cell becomes Pending." The exact per-state
/// mapping is the §7.2/§9.2 truth table (pinned at the cache layer by
/// `cache::tests` S5/S6): an **Intermediate** input yields an Intermediate
/// output, and a **Pending** input (a Realization downgraded / awaiting
/// rebuild) yields the Pending output the §7.1 headline calls for. We assert
/// both legs so the realization_reads fold is exercised across states.
///
/// Two preconditions make this observable, both delivered by S10 +
/// esc-3606-37 ruling step 1:
///   - the Realization `Widget#0` is recorded as a freshness-bearing cache
///     node on the build success path (so it is markable), and
///   - the GH cell's *cached* `dependency_trace.realization_reads` contains
///     `Widget#0` (so the derivation folds it in).
///
/// RED until then: `mark_pending_with_cause(R0, R0)` returns false (no entry)
/// and the cell's cached trace is empty, so the cell derives Final regardless
/// of R0.
#[test]
fn geometry_handle_cell_freshness_folds_backing_realization() {
    let mut engine = build_widget();

    let r0 = RealizationNodeId::new("Widget", 0);
    let r0_node = NodeId::Realization(r0.clone());
    let geom_node = NodeId::Value(ValueCellId::new("Widget", "geometry"));
    let generation = 1u64;

    // The upstream Realization must be a markable, freshness-bearing cache node
    // (PRD §7.1 presupposes this; esc-3606-37 ruling step 1 records it). Mark it
    // Pending with *itself* as the diagnostic-chain root: a directly-downgraded
    // Realization is the chain root, mirroring the cache-layer contract pinned by
    // `cache::tests::derive_output_freshness_folds_realization_reads` (S5), which
    // this test defers to for the §7.2/§9.2 truth-table semantics. The
    // forward-only meet then surfaces R0 as the cell's cause; a bare
    // `mark_pending` would clear the cause (cache.rs:686) and the meet would
    // forward `None`, exactly as a Pending VC input with no recorded upstream.
    let marked = engine
        .cache_store_mut()
        .mark_pending_with_cause(&r0_node, r0_node.clone());
    assert!(
        marked,
        "Realization Widget#0 must be recorded as a freshness-bearing cache node \
         on the build success path (esc-3606-37 ruling step 1)"
    );

    // Leg A — Pending Realization → Pending cell (the §7.1 headline outcome),
    // with the chain root tracing to the Realization.
    let (f_pending, cause) = engine
        .cache_store()
        .derive_output_freshness_for_node_with_cause(&geom_node, false, generation);
    assert!(
        matches!(f_pending, Freshness::Pending { .. }),
        "GeometryHandle cell must become Pending when its backing Realization is \
         downgraded (PRD §5/§7.1 realization_reads meet); got {:?}",
        f_pending
    );
    assert_eq!(
        cause,
        Some(r0_node.clone()),
        "the Pending cause must trace to the backing Realization Widget#0"
    );

    // Leg B — Intermediate Realization → Intermediate cell (§7.2 main rule),
    // confirming the fold tracks the Intermediate state too, not just Pending.
    // `set_freshness` is `#[must_use]` (returns false when absent); R0 is present
    // (just marked above), so explicitly discard the bool.
    let _ = engine
        .cache_store_mut()
        .set_freshness(&r0_node, Freshness::Intermediate { generation: 7 });
    let f_inter = engine
        .cache_store()
        .derive_output_freshness_for_node(&geom_node, false, generation);
    assert!(
        matches!(f_inter, Freshness::Intermediate { .. }),
        "GeometryHandle cell must track its backing Realization's Intermediate \
         freshness; got {:?}",
        f_inter
    );
}

/// SECOND (cascade through the freshness walk): editing the scalar `width`
/// that the geometry realization reads drives the realization Pending, and the
/// S8 Realization→ValueCell fan-out carries Pending onto the GeometryHandle
/// cell — `width → Realization[0] → ValueCell(geometry)`.
///
/// Enabled by esc-3606-37 ruling step 1 (R0 recorded with
/// `dependency_trace.reads = [width]`) + S10 (geometry's cached trace carries
/// `realization_reads = [R0]`). RED until then: R0 has no cache entry, so the
/// walk re-derives it as Final (absent → Final) and never fans out.
#[test]
fn width_edit_cascades_through_realization_to_geometry_handle_cell() {
    let mut engine = build_widget();

    let r0_node = NodeId::Realization(RealizationNodeId::new("Widget", 0));
    let width = ValueCellId::new("Widget", "width");
    let geom_node = NodeId::Value(ValueCellId::new("Widget", "geometry"));
    let generation = 1u64;

    // Dirty the upstream scalar param the realization reads.
    let marked = engine
        .cache_store_mut()
        .mark_pending(&NodeId::Value(width.clone()));
    assert!(marked, "width must be a cache node after build()");

    // Drive the freshness-only walk seeded from the changed param.
    let updated = engine.propagate_freshness_only(std::iter::once(&width), generation);

    // width → Realization[0]: the realization re-derives Pending from its dirty
    // scalar input (its cached trace reads [width]).
    assert!(
        matches!(engine.cache_store().freshness(&r0_node), Freshness::Pending { .. }),
        "Realization Widget#0 must be Pending after its scalar input width is dirtied; got {:?}",
        engine.cache_store().freshness(&r0_node)
    );

    // Realization[0] → ValueCell(geometry): the S8 fan-out re-derives the GH
    // cell, whose cached trace now folds in R0's Pending freshness.
    assert!(
        matches!(engine.cache_store().freshness(&geom_node), Freshness::Pending { .. }),
        "GeometryHandle cell must be Pending via the Realization→ValueCell edge; got {:?}",
        engine.cache_store().freshness(&geom_node)
    );
    assert!(
        updated.contains(&geom_node),
        "the GeometryHandle cell must appear in the walk's `updated` set, got: {:?}",
        updated
    );
}
