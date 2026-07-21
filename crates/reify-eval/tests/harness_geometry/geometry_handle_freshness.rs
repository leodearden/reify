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
use reify_ir::{ExportFormat, Freshness, GeometryHandleId, Value};
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
        matches!(
            result.values.get_or_undef(&geom_cell),
            Value::GeometryHandle { .. }
        ),
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
        matches!(
            engine.cache_store().freshness(&r0_node),
            Freshness::Pending { .. }
        ),
        "Realization Widget#0 must be Pending after its scalar input width is dirtied; got {:?}",
        engine.cache_store().freshness(&r0_node)
    );

    // Realization[0] → ValueCell(geometry): the S8 fan-out re-derives the GH
    // cell, whose cached trace now folds in R0's Pending freshness.
    assert!(
        matches!(
            engine.cache_store().freshness(&geom_node),
            Freshness::Pending { .. }
        ),
        "GeometryHandle cell must be Pending via the Realization→ValueCell edge; got {:?}",
        engine.cache_store().freshness(&geom_node)
    );
    assert!(
        updated.contains(&geom_node),
        "the GeometryHandle cell must appear in the walk's `updated` set, got: {:?}",
        updated
    );
}

/// THIRD (GHR-δ §8 Phase 4 — edit donation cross-kind cascade, S11/S12): when a
/// source edit changes a Realization's `content_hash` (landing it in
/// `changed_realizations`) WITHOUT changing the backing `Value::GeometryHandle`
/// cell's own `content_hash`, the edit-time invalidation must still invalidate
/// that cell via the Realization→ValueCell donation cascade. The changed
/// Realization invalidates its own cache entry today (engine_edit.rs:2301), but
/// the value cell holding its handle is missed — lazy revalidation (S14/S16)
/// then handles the next read.
///
/// ## Why a *reorder* fixture, not a literal `box(20mm,..)`→`box(25mm,..)` edit
///
/// The plan (S11) gives a box-arg change as its example, but editing a box()
/// argument changes BOTH the realization's ops-hash AND the geometry param
/// cell's `default_expr` content_hash — so the cell lands in the value-cell
/// `changed` set and is already invalidated by the pre-existing changed-cell
/// path (engine_edit.rs:2283, the `for id in &changed` loop). That masks the
/// cross-kind cascade: the test would pass with OR without S12 and never be RED.
///
/// Reordering two geometry params isolates the cascade. Value cells are keyed by
/// member name (`Widget.a`, `Widget.b`) and carry their `default_expr` verbatim,
/// so swapping declaration order leaves every cell's `content_hash`
/// byte-identical — the cells are NOT in `changed`. Realizations are keyed
/// positionally (`RealizationNodeId { entity, index }`), so the swap moves
/// different ops under `Widget#0` / `Widget#1` and BOTH land in
/// `changed_realizations`. The ONLY edit-time path that can now invalidate the
/// GH cells is the Realization→ValueCell edge S12 adds. This honors S11's intent
/// ("the backing realization's content_hash changes, lands in
/// `changed_realizations`") while making the donation cascade observable.
///
/// Empirically on the base commit: after the reorder edit, `Widget#0`/`Widget#1`
/// are invalidated (cache entries gone) but `Widget.a`/`Widget.b` remain cached
/// with their stale handles — exactly the gap. RED until S12 invalidates the
/// backed cells at the donation block; `assert no panic` per S11.
#[test]
fn realization_change_donates_invalidation_to_backing_geometry_cell() {
    // Two geometry-bearing params; `width` first so the box() calls have no
    // forward reference. `a` and `b` carry distinct box dimensions so their
    // realizations have distinct ops-hashes (and so the swap is observable).
    const TWO_GEOM_SRC: &str = r#"structure def Widget {
    param width : Length = 10mm
    param a : Solid = box(width, 20mm, 30mm)
    param b : Solid = box(width, 40mm, 50mm)
}"#;
    // Same two params, declaration order swapped. Cell identities + exprs are
    // byte-identical; only the realization indices move.
    const TWO_GEOM_SRC_REORDERED: &str = r#"structure def Widget {
    param width : Length = 10mm
    param b : Solid = box(width, 40mm, 50mm)
    param a : Solid = box(width, 20mm, 30mm)
}"#;

    let compiled = compile_source(TWO_GEOM_SRC);
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

    let a = ValueCellId::new("Widget", "a");
    let b = ValueCellId::new("Widget", "b");
    let a_node = NodeId::Value(a.clone());
    let b_node = NodeId::Value(b.clone());

    // Sanity: both geometry cells hydrated to handles (so the realization↔cell
    // freshness edge exists for both) and both have live cache entries.
    assert!(
        matches!(result.values.get_or_undef(&a), Value::GeometryHandle { .. }),
        "Widget.a must hydrate to a Value::GeometryHandle"
    );
    assert!(
        matches!(result.values.get_or_undef(&b), Value::GeometryHandle { .. }),
        "Widget.b must hydrate to a Value::GeometryHandle"
    );
    assert!(
        engine.cache_store().get(&a_node).is_some() && engine.cache_store().get(&b_node).is_some(),
        "both geometry cells must have cache entries after build()"
    );

    // Edit: swap the two geometry params. Realizations Widget#0/Widget#1 change
    // (ops-hash swap → changed_realizations); cells Widget.a/Widget.b do NOT
    // (member name + default_expr unchanged → absent from value-cell `changed`).
    let compiled2 = compile_source(TWO_GEOM_SRC_REORDERED);
    let edit = engine
        .edit_source(&compiled2)
        .expect("reorder edit_source must not error");

    // Precondition (already true on base): the realizations were invalidated,
    // confirming the edit really did land them in `changed_realizations`.
    let r0 = NodeId::Realization(RealizationNodeId::new("Widget", 0));
    let r1 = NodeId::Realization(RealizationNodeId::new("Widget", 1));
    assert!(
        engine.cache_store().get(&r0).is_none() && engine.cache_store().get(&r1).is_none(),
        "both realizations must be invalidated by the reorder (changed_realizations); \
         got r0 cached={}, r1 cached={}",
        engine.cache_store().get(&r0).is_some(),
        engine.cache_store().get(&r1).is_some()
    );

    // RED assertion (S12): the GeometryHandle cells backed by the changed
    // realizations must ALSO be invalidated by the donation cascade. On the base
    // commit they survive with their stale handles, so this fails until S12.
    assert!(
        engine.cache_store().get(&a_node).is_none(),
        "Widget.a (GeometryHandle cell backed by a changed Realization) must be \
         invalidated by the edit donation cascade (GHR-δ S12); it is still cached"
    );
    assert!(
        engine.cache_store().get(&b_node).is_none(),
        "Widget.b (GeometryHandle cell backed by a changed Realization) must be \
         invalidated by the edit donation cascade (GHR-δ S12); it is still cached"
    );

    // The edit must not panic and must report a coherent result (smoke per S11).
    let _ = edit.values;
}

/// FOURTH (GHR-δ §5 — lazy revalidation, S15/S16): `Engine::read_value_revalidated`
/// re-resolves a STALE `Value::GeometryHandle` against the Engine's current
/// `realization_ref → handle` map, writing the fresh handle back so the next
/// read is a fast-path hit; and returns `Value::Undef` (no panic) for a handle
/// whose backing realization is ABSENT from the map.
///
/// The slow-path counter pins the §9 Q4 fast/slow split: the stale read fires
/// the slow path exactly once, and because it writes the fresh value back, the
/// immediately-following read of the same cell takes the fast path (counter
/// stays at 1 — exact `==`, not `>=`).
///
/// RED until S16 adds the `read_value_revalidated` read entry point (the helper
/// + map landed in S13/S14).
#[test]
fn lazy_revalidation_reresolves_stale_handle_and_undefs_missing_realization() {
    // Build inline (not via `build_widget`) to capture the BuildResult: the
    // hydrated GeometryHandle lands in `result.values`, NOT in the Engine's
    // eval-state snapshot (whose geometry cell stays Undef — hydration runs on
    // build's local value map). The Engine's `realization_handles` map, though,
    // DID record `Widget#0 → fresh handle` during the same post-process, which
    // is exactly the oracle revalidation consults.
    let compiled = compile_source(WIDGET_SRC);
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let geom = ValueCellId::new("Widget", "geometry");
    let fresh = result.values.get_or_undef(&geom);
    let (realization_ref, current_handle) = match &fresh {
        Value::GeometryHandle {
            realization_ref,
            kernel_handle,
            ..
        } => (realization_ref.clone(), *kernel_handle),
        other => panic!(
            "Widget.geometry must hydrate to a GeometryHandle; got {:?}",
            other
        ),
    };

    // Clone the post-build snapshot so it can be mutated independently of the
    // Engine: the read entry point takes `&self` + a `&mut Snapshot`, so a
    // borrowed-from-engine snapshot would collide with the `&self` receiver. Its
    // geom cell is Undef; we overwrite it, preserving its DeterminacyState.
    let mut snap = engine
        .snapshot()
        .expect("engine has a snapshot after build()")
        .clone();
    let det = snap.values.get(&geom).expect("geom cell in snapshot").1;

    // (1) STALE handle for Widget#0: INVALID kernel_handle, real realization_ref.
    let stale = Value::GeometryHandle {
        realization_ref: realization_ref.clone(),
        upstream_values_hash: [0u8; 32],
        kernel_handle: Some(GeometryHandleId::INVALID),
    };
    snap.values.insert(geom.clone(), (stale, det));

    // First read: the slow path re-resolves the stale handle to the current one.
    let read1 = engine.read_value_revalidated(&mut snap, &geom);
    match read1 {
        Value::GeometryHandle { kernel_handle, .. } => assert_eq!(
            kernel_handle, current_handle,
            "stale handle must re-resolve to the Engine's current handle"
        ),
        other => panic!("expected re-resolved GeometryHandle, got {:?}", other),
    }
    assert_eq!(
        engine.geometry_revalidation_slow_path_count(),
        1,
        "the stale read must fire the slow path exactly once"
    );

    // Second read: the cell was updated in place by the first read, so this
    // takes the fast path and leaves the counter at 1.
    let _read2 = engine.read_value_revalidated(&mut snap, &geom);
    assert_eq!(
        engine.geometry_revalidation_slow_path_count(),
        1,
        "the second read must take the fast path (counter unchanged)"
    );

    // (2) MISSING realization: a handle whose `realization_ref` is absent from
    // the Engine's map must read as `Undef` without panicking.
    let orphan_cell = ValueCellId::new("Widget", "orphan");
    let orphan = Value::GeometryHandle {
        realization_ref: RealizationNodeId::new("Ghost", 0),
        upstream_values_hash: [0u8; 32],
        kernel_handle: Some(GeometryHandleId(123)),
    };
    snap.values.insert(orphan_cell.clone(), (orphan, det));
    let read3 = engine.read_value_revalidated(&mut snap, &orphan_cell);
    assert!(
        matches!(read3, Value::Undef),
        "a handle backed by an absent realization must read as Undef; got {:?}",
        read3
    );
}

/// FIFTH (GHR-δ §8 Phase 4 — removed-realization donation leg,
/// `engine_edit.rs` `for rid in &removed_realizations`): the THIRD test covers
/// the *changed*-realization donation leg (a realization whose `content_hash`
/// moves while its backing cell survives byte-identical). This covers the
/// sibling branch the reviewer flagged as untested — and the riskiest, since a
/// missed cell silently keeps a now-unbacked handle: a `Type::Geometry` value
/// cell **survives** an edit while the `RealizationNodeId` that backed it in the
/// OLD graph is **removed**. The removed-realization leg consults the OLD
/// reverse index (`Realization → Value(cell)`), gated on the cell still existing
/// in the new graph, and invalidates it.
///
/// ## Fixture: drop an *earlier* geometry param so a later one reindexes
///
/// A `Type::Geometry` param produces a realization regardless of its default
/// expression (an alias `param b : Solid = a` still emits a realization — so
/// "remove the geometry default" does NOT drop a realization). The only way a
/// surviving Geometry cell's backing `RealizationNodeId` vanishes is positional
/// reindexing: realizations are keyed by `RealizationNodeId { entity, index }`
/// in declaration order, so removing an *earlier* geometry param shifts a later
/// one to a lower index and the old high index lands in `removed_realizations`.
///
/// OLD: `a` → `Widget#0`, `b` → `Widget#1`. NEW (drop `a`): `b` → `Widget#0`.
/// → `Widget#1` removed; the OLD reverse index `Widget#1 → Value(Widget.b)` and
/// `Widget.b` is still a declared param (member name + `box(..)` default
/// unchanged, so its own `content_hash` is identical → it is NOT in the
/// value-cell `changed` set). That is exactly the removed-leg
/// membership-gate-TRUE branch.
///
/// ## Outcome guard, not leg isolation (intentional)
///
/// This pins the safety OUTCOME (the surviving cell is invalidated, so no stale
/// handle persists) and exercises the removed-leg branch end-to-end, but it is
/// not a sole-invalidator isolation test: `Widget.b`'s NEW backing realization
/// `Widget#0` (was `a`, now `b`) is in `changed_realizations`, so the
/// changed-realization leg also invalidates `Widget.b` via the NEW reverse edge.
/// A pure isolation is structurally unreachable through a source edit — any edit
/// that removes a surviving Geometry cell's backing `RealizationNodeId` either
/// changes that cell's `default_expr` (→ changed cell) or reindexes it (→ a
/// changed/added realization re-linked to the cell). Coverage of the branch plus
/// the outcome assertion is the achievable guard, matching the reviewer's ask.
#[test]
fn removed_realization_donates_invalidation_to_surviving_geometry_cell() {
    // `a` and `b` are geometry params (realizations Widget#0 / Widget#1); `width`
    // is a scalar (no realization), declared first so the box() calls have no
    // forward reference. `b` does not reference `a`, so dropping `a` compiles.
    const SRC_A: &str = r#"structure def Widget {
    param width : Length = 10mm
    param a : Solid = box(width, 20mm, 30mm)
    param b : Solid = box(width, 40mm, 50mm)
}"#;
    // Drop the EARLIER geometry param `a`. `b` reindexes Widget#1 → Widget#0, so
    // the old id Widget#1 lands in removed_realizations; the value cell Widget.b
    // survives byte-identical (same member, same default).
    const SRC_B: &str = r#"structure def Widget {
    param width : Length = 10mm
    param b : Solid = box(width, 40mm, 50mm)
}"#;

    let compiled_a = compile_source(SRC_A);
    let a_errors: Vec<_> = compiled_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        a_errors.is_empty(),
        "module A must compile cleanly; got: {:?}",
        a_errors
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled_a, ExportFormat::Step);
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| format!("[{:?}] {}", d.severity, d.message))
        .collect();
    assert!(
        build_errors.is_empty(),
        "module A must build cleanly; got: {:?}",
        build_errors
    );

    let b = ValueCellId::new("Widget", "b");
    let b_node = NodeId::Value(b.clone());
    // In SRC_A, `b` is the second geometry param → Widget#1.
    let r_b_old = RealizationNodeId::new("Widget", 1);

    // Preconditions on the built graph: `b` hydrated to a handle (it really is a
    // Realization-backed Type::Geometry cell) and Widget#1 backs it.
    assert!(
        matches!(result.values.get_or_undef(&b), Value::GeometryHandle { .. }),
        "Widget.b must hydrate to a Value::GeometryHandle before the edit"
    );
    assert!(
        engine.cache_store().get(&b_node).is_some(),
        "Widget.b must have a live cache entry after build()"
    );
    {
        let snap = engine.snapshot().expect("snapshot after build()");
        assert!(
            snap.graph.realizations.contains_key(&r_b_old),
            "Widget#1 must exist before the edit (it backs `b`)"
        );
    }

    // Edit: drop `a`. Widget#1 disappears (b reindexes to Widget#0) →
    // removed_realizations; Widget.b survives byte-identical as a param.
    let compiled_b = compile_source(SRC_B);
    let b_errors: Vec<_> = compiled_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        b_errors.is_empty(),
        "module B must compile cleanly; got: {:?}",
        b_errors
    );
    let edit = engine
        .edit_source(&compiled_b)
        .expect("drop-param edit_source must not error");

    // Precondition: the OLD backing realization Widget#1 was removed, and the
    // value cell Widget.b survived in the new graph (the removed-leg's
    // membership-gate-TRUE branch).
    let snap = engine.snapshot().expect("snapshot after edit");
    assert!(
        !snap.graph.realizations.contains_key(&r_b_old),
        "Widget#1 must be removed after dropping the earlier geometry param `a`; \
         realizations now: {:?}",
        snap.graph
            .realizations
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        snap.graph.value_cells.contains_key(&b),
        "Widget.b must SURVIVE the edit (still a declared param) — this is the \
         removed-realization donation leg's membership-gate-TRUE branch"
    );

    // Main assertion (mirrors the THIRD/reorder test): the surviving cell, whose
    // OLD backing realization was removed, must be invalidated so its stale
    // handle does not silently persist in the cache.
    assert!(
        engine.cache_store().get(&b_node).is_none(),
        "Widget.b (a surviving Type::Geometry cell whose OLD backing Realization \
         Widget#1 was removed) must be invalidated by the edit donation cascade; \
         it is still cached with a stale handle"
    );

    // Smoke: the edit produced a coherent result and did not panic.
    let _ = edit.values;
}
