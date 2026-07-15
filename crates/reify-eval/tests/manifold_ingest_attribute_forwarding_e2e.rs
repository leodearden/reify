//! Task 4636 — Manifold cross-kernel attribute substrate: OCCT->Manifold
//! ingest attribute forwarding (completion condition (b)).
//!
//! ## Root cause (LINK2)
//!
//! `ManifoldKernel::propagate_attributes`'s `parent_map` is built by looking
//! up each parent handle under `KernelHandle{kernel: KernelId::Manifold, id:
//! handle}` (`crates/reify-kernel-manifold/src/kernel.rs`). Before this task,
//! nothing ever recorded a SOLID-level entry at that key — primitive seeding
//! (`record_all_faces_as_side`) only records per-FACE entries — so the lookup
//! always missed, `parent_map` came back empty, and `propagate_attributes`
//! took the degenerate path (`Ok(KernelAttributeOutcome::Discarded)`) even
//! when the source solid legitimately carried an attribute.
//!
//! ## Fix under test
//!
//! `record_solid_attribute` (primitive_attribute_seed.rs) authors a per-solid
//! representative entry, and `forward_solid_attribute_on_ingest`
//! (engine_build.rs, re-exported at the crate root) forwards that entry
//! across the OCCT->Manifold ingest seam: `table.lookup(source).cloned()`
//! then `table.record(target, attr)`. This test drives both helpers directly
//! against a REAL `ManifoldKernel` (no engine involved — that wiring is
//! covered separately by the engine e2e in this same task) and asserts the
//! forwarded entry is exactly what `propagate_attributes` needs to return
//! `Ok(Propagated)`.
//!
//! Fails to compile until `forward_solid_attribute_on_ingest` exists.
//!
//! ## Reuse
//! - `record_solid_attribute`: `crates/reify-eval/src/primitive_attribute_seed.rs`.
//! - `unit_cube_mesh` fixture + real `ManifoldKernel::ingest_mesh` /
//!   `execute(Union)` / `propagate_attributes` pattern:
//!   `crates/reify-kernel-manifold/src/kernel.rs`
//!   (`propagate_attributes_returns_propagated_when_parent_provenance_present`).
//! - `propagate_via_kernel_attribute_hook` dispatcher: `crates/reify-eval/src/kernel_attribute_hook.rs`.

use reify_eval::primitive_attribute_seed::record_solid_attribute;
use reify_eval::{forward_solid_attribute_on_ingest, propagate_via_kernel_attribute_hook};
use reify_ir::{
    FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, KernelAttributeOutcome, KernelHandle,
    KernelId, Role, TopologyAttribute, TopologyAttributeTable,
};
use reify_kernel_manifold::ManifoldKernel;
use reify_kernel_manifold::test_fixtures::unit_cube_mesh;

/// Completion condition (b): forwarding a solid-level attribute across the
/// OCCT->Manifold ingest seam is exactly what `propagate_attributes` needs
/// to return `Ok(Propagated)` with a non-empty `parent_map`.
///
/// 1. Ingest two `unit_cube_mesh` solids into a real `ManifoldKernel` ->
///    `handle_a`, `handle_b` (each `as_original()`-tagged by `ingest_mesh`,
///    so `original_id() >= 0`).
/// 2. Author two SOURCE solid entries under an arbitrary pre-ingest scope
///    (`KernelHandle{Occt, src_a/src_b}`) via `record_solid_attribute` — this
///    stands in for the primitive-seeding step that runs before conversion in
///    the real engine pipeline.
/// 3. Forward each source entry onto its ingested Manifold handle via
///    `forward_solid_attribute_on_ingest` — this is the OCCT->Manifold ingest
///    seam under test.
/// 4. Execute `Union{handle_a, handle_b}` and call
///    `propagate_via_kernel_attribute_hook` with `[handle_a, handle_b]` as
///    parents — must return `Ok(Propagated)`, proving the forwarded entries
///    populated `parent_map`.
#[test]
fn forward_solid_attribute_on_ingest_enables_propagated_outcome() {
    let mut kernel = ManifoldKernel::new();
    let mesh_a = unit_cube_mesh([0.0, 0.0, 0.0]);
    let mesh_b = unit_cube_mesh([0.5, 0.0, 0.0]);

    let handle_a = kernel
        .ingest_mesh(&mesh_a)
        .expect("unit_cube_mesh fixture must ingest into a real ManifoldKernel");
    let handle_b = kernel
        .ingest_mesh(&mesh_b)
        .expect("unit_cube_mesh fixture must ingest into a real ManifoldKernel");

    let feature_id = FeatureId::realization("t", 0);
    let src_a = KernelHandle {
        kernel: KernelId::Occt,
        id: GeometryHandleId(9001),
    };
    let src_b = KernelHandle {
        kernel: KernelId::Occt,
        id: GeometryHandleId(9002),
    };

    let mut table = TopologyAttributeTable::default();
    record_solid_attribute(&mut table, src_a.kernel, src_a.id, &feature_id);
    record_solid_attribute(&mut table, src_b.kernel, src_b.id, &feature_id);

    // The OCCT->Manifold ingest seam under test: forward each source solid's
    // entry onto the handle its mesh was just ingested under.
    forward_solid_attribute_on_ingest(
        &mut table,
        src_a,
        KernelHandle {
            kernel: KernelId::Manifold,
            id: handle_a.id,
        },
    );
    forward_solid_attribute_on_ingest(
        &mut table,
        src_b,
        KernelHandle {
            kernel: KernelId::Manifold,
            id: handle_b.id,
        },
    );

    let op = GeometryOp::Union {
        left: handle_a.id,
        right: handle_b.id,
    };
    let result = kernel
        .execute(&op)
        .expect("union of two ingested cubes must succeed");

    let outcome = propagate_via_kernel_attribute_hook(
        &kernel,
        &mut table,
        &op,
        &[handle_a.id, handle_b.id],
        result.id,
        &feature_id,
    );

    match outcome {
        Ok(KernelAttributeOutcome::Propagated) => {}
        other => panic!(
            "expected Ok(Propagated) after forward_solid_attribute_on_ingest populated the \
             Manifold-scoped solid entries for both parents; got {other:?}"
        ),
    }
}

/// Companion assertion documenting the LINK2 root cause: a table populated
/// with ONLY a per-face-scoped entry (never at the parent SOLID handle) must
/// still yield `Ok(Discarded)` — proving the defect this task fixes is
/// specifically the absence of a solid-level entry, not a general failure of
/// `propagate_attributes`' correlation walk.
///
/// The synthetic face entry is deliberately keyed at a `GeometryHandleId`
/// distinct from `handle_a.id` / `handle_b.id` (mirroring how
/// `record_all_faces_as_side` seeds per-face handles that are never the
/// solid handle itself), so the parent lookup in `propagate_attributes`
/// (`KernelHandle{Manifold, handle_a.id}` / `{Manifold, handle_b.id}`) misses
/// for both parents exactly as it did before this task's fix.
#[test]
fn table_with_only_face_scoped_entry_yields_discarded_link2_root_cause() {
    let mut kernel = ManifoldKernel::new();
    let handle_a = kernel
        .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
        .expect("unit_cube_mesh fixture must ingest into a real ManifoldKernel");
    let handle_b = kernel
        .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
        .expect("unit_cube_mesh fixture must ingest into a real ManifoldKernel");

    let feature_id = FeatureId::realization("t", 0);
    let mut table = TopologyAttributeTable::default();
    // A face-scoped entry only — NOT at handle_a.id / handle_b.id — mirrors
    // the pre-fix state where record_all_faces_as_side seeded faces but no
    // helper ever recorded the solid handle itself.
    table.record(
        KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(90001),
        },
        TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        },
    );

    let op = GeometryOp::Union {
        left: handle_a.id,
        right: handle_b.id,
    };
    let result = kernel
        .execute(&op)
        .expect("union of two ingested cubes must succeed");

    let outcome = propagate_via_kernel_attribute_hook(
        &kernel,
        &mut table,
        &op,
        &[handle_a.id, handle_b.id],
        result.id,
        &feature_id,
    );

    match outcome {
        Ok(KernelAttributeOutcome::Discarded) => {}
        other => panic!(
            "expected Ok(Discarded) — no entry exists at either parent's SOLID handle, only at \
             an unrelated face-scoped handle, so parent_map must stay empty; got {other:?}"
        ),
    }
}
