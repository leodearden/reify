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
//! - Engine cross-kernel conversion harness (`Engine::with_test_kernels_and_registry`
//!   + `MockGeometryKernel` + a counting/tracking `ingest_mesh` wrapper):
//!     `crates/reify-eval/tests/cross_kernel_handoff.rs`.
//! - Mock-registry seeding staging (`with_extracted_faces`/`with_extracted_edges`)
//!   + `engine.topology_attribute_table()` observation:
//!     `crates/reify-eval/tests/cross_kernel_attribute_collision_e2e.rs`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_core::{ModulePath, Type};
use reify_eval::primitive_attribute_seed::record_solid_attribute;
use reify_eval::{Engine, forward_solid_attribute_on_ingest, propagate_via_kernel_attribute_hook};
use reify_ir::{
    CapabilityDescriptor, CompiledExpr, ExportFormat, FeatureId, GeometryHandleId, GeometryKernel,
    GeometryOp, KernelAttributeOutcome, KernelHandle, KernelId, Operation, ReprKind, Role,
    TopologyAttribute, TopologyAttributeTable, Value,
};
use reify_kernel_manifold::ManifoldKernel;
use reify_kernel_manifold::test_fixtures::unit_cube_mesh;
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::MockGeometryKernel;
use reify_test_support::{MockConstraintChecker, manufacturing_purpose, mm, step_output_template};

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

// ─── Completion condition (a): the INGEST PATH populates the table ─────────

/// "manifold"-like kernel whose `ingest_mesh` tracks every assigned handle
/// into a shared `Arc<Mutex<Vec<GeometryHandleId>>>` (read back by the test
/// after `build()`), and whose `execute` delegates to an inner
/// [`MockGeometryKernel`] (for the final `BooleanUnion`). Mirrors
/// `CountingManifoldKernel` in `cross_kernel_handoff.rs`, replacing its call
/// counter with a handle-id log — this test needs the actual ingested ids to
/// look them up in `engine.topology_attribute_table()`, not just a count.
struct IngestTrackingManifoldKernel {
    inner: MockGeometryKernel,
    next_ingest_id: u64,
    ingested_handles: Arc<Mutex<Vec<GeometryHandleId>>>,
}

impl reify_ir::GeometryKernel for IngestTrackingManifoldKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.inner.execute(op)
    }

    fn query(&self, q: &reify_ir::GeometryQuery) -> Result<reify_ir::Value, reify_ir::QueryError> {
        self.inner.query(q)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        handle: GeometryHandleId,
        tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn ingest_mesh(
        &mut self,
        _mesh: &reify_ir::Mesh,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        let id = GeometryHandleId(self.next_ingest_id);
        self.next_ingest_id += 1;
        self.ingested_handles.lock().unwrap().push(id);
        Ok(reify_ir::GeometryHandle { id, repr: None })
    }
}

/// Build the `MyDesign` template carrying ONE terminal realization whose ops
/// are two BRep `PrimitiveSphere` solids consumed by a `BooleanUnion`.
/// Sphere (not Box) so the primitive seeder needs no vertex/`BoundingBox`
/// staging on the mock — mirrors `cross_kernel_attribute_collision_e2e.rs`'s
/// sphere fixture, which stages only `with_extracted_faces`/`with_extracted_edges`.
fn two_sphere_union_template() -> reify_compiler::TopologyTemplate {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let sphere_op = || CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_lit(5.0))],
    };
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        .realization_named("MyDesign", 0, "body", vec![sphere_op(), sphere_op(), union_op])
        .build()
}

/// Completion condition (a): the engine's OCCT->Manifold ingest path — the
/// `'convert:` loop plus the primitive seed site in `engine_build.rs` —
/// populates `topology_attribute_table` at `{Manifold, ingested_handle}` for
/// each converted operand.
///
/// Drives a two-kernel build (`occt` produces two BRep spheres; `manifold`
/// supplies the only `(BooleanUnion, Mesh)` capability, forcing a BRep->Mesh
/// conversion + `ingest_mesh` for both sphere operands) through
/// `Engine::build`, then asserts:
/// 1. No error-severity diagnostics.
/// 2. Exactly two `ingest_mesh` calls landed on the "manifold" kernel (one
///    per sphere operand) — the non-vacuousness premise for (3).
/// 3. `engine.topology_attribute_table()` has an entry at
///    `KernelHandle{Manifold, ingested_handle}` for EVERY ingested handle.
///
/// RED today: the engine seed site does not yet call `record_solid_attribute`
/// and the `'convert:` loop does not yet call `forward_solid_attribute_on_ingest`
/// (both are step-6), so the Manifold-scoped solid lookup misses for both
/// operands.
#[test]
fn engine_convert_loop_forwards_solid_attribute_to_manifold_ingested_handle() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_manifold_ingest_attribute_forwarding".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(two_sphere_union_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    // occt: two spheres allocate handles 1 and 2 (MockGeometryKernel's own
    // sequential counter) — stage face/edge extraction for both so the
    // primitive seeder succeeds cleanly (a seeding failure would skip the
    // record_solid_attribute call once step-6 lands, permanently masking
    // this test's RED->GREEN transition).
    let occt_kernel = MockGeometryKernel::new()
        .with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(101)])
        .with_extracted_edges(GeometryHandleId(1), vec![GeometryHandleId(102)])
        .with_extracted_faces(GeometryHandleId(2), vec![GeometryHandleId(201)])
        .with_extracted_edges(GeometryHandleId(2), vec![GeometryHandleId(202)]);

    let ingested_handles = Arc::new(Mutex::new(Vec::new()));
    let manifold_kernel = IngestTrackingManifoldKernel {
        inner: MockGeometryKernel::new(),
        next_ingest_id: 5000,
        ingested_handles: Arc::clone(&ingested_handles),
    };

    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert("occt".to_string(), Box::new(occt_kernel));
    kernels.insert("manifold".to_string(), Box::new(manifold_kernel));

    // occt: (PrimitiveSphere, BRep) + (Convert{BRep}, Mesh); manifold:
    // (BooleanUnion, Mesh) — forces the union to route to manifold preceded
    // by a BRep->Mesh conversion stage on occt (mirrors cross_kernel_handoff.rs).
    let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    registry.insert(
        "occt".to_string(),
        CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveSphere, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        },
    );
    registry.insert(
        "manifold".to_string(),
        CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        },
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry,
        Some("occt".to_string()),
    );

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Stl);
    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "cross-kernel build must not emit error diagnostics; got: {errors:?}"
    );

    // Sanity: exactly one ingest_mesh call per sphere operand — the
    // non-vacuousness premise for the per-handle assertion below.
    let ingested = ingested_handles.lock().unwrap().clone();
    assert_eq!(
        ingested.len(),
        2,
        "expected exactly 2 manifold.ingest_mesh calls (one per sphere operand); got {ingested:?}"
    );

    let table = engine.topology_attribute_table();
    for &h in &ingested {
        assert!(
            table
                .lookup(KernelHandle {
                    kernel: KernelId::Manifold,
                    id: h,
                })
                .is_some(),
            "expected topology_attribute_table.lookup(KernelHandle{{Manifold, {h:?}}}) to be \
             Some after build() — forward_solid_attribute_on_ingest (wired into the 'convert: \
             loop) should have forwarded the source solid's record_solid_attribute entry across \
             the ingest seam; got None"
        );
    }
}

// ─── Regression: forwarded entries must not pollute the diagnostic scan ────

/// Regression for the reviewer's diagnostic-pollution finding (task #4636
/// step-8/9): the forwarded Manifold-scoped solid entries
/// `forward_solid_attribute_on_ingest` writes at the ingest seam share the
/// realization's `feature_id` with every other entry in the table, so the
/// post-loop centroid / local-index-reassignment scan (which filters purely
/// on `feature_id`) must explicitly exclude them too — otherwise they leak
/// into `collect_centroids_with_failure_summary`, get centroid-queried
/// against the engine's default (occt) kernel under their bare
/// `GeometryHandleId` (never staged there — they are Manifold-only ingest
/// handles), and produce a spurious "topology-attribute centroid query
/// failed" Warning (or, on an id collision with a real occt handle, a
/// false-positive reassignment diagnostic fed by the wrong shape's
/// centroid).
///
/// Reuses the completion-(a) harness (`two_sphere_union_template` +
/// `IngestTrackingManifoldKernel`, `next_ingest_id: 5000`, occt as the
/// default kernel) but additionally stages BOTH a centroid and a
/// bounding-box result for every real occt handle seeded during primitive
/// attribute seeding (faces 101/201, edges 102/202), with distinct
/// coordinates per handle, so that:
/// - every REAL scanned handle's diagnostic query succeeds — pinning
///   `query_fail_count` to exactly the forwarded-handle count (2 before the
///   fix, 0 after), for a clean non-brittle signal on assertion (1);
/// - staging both query types per handle removes any dependence on which
///   query type a given handle's `Role` actually routes through;
/// - the two `Role::Side` faces (101, 201) and the two `Role::NewEdge` edges
///   (102, 202) never share a centroid / bbox-midpoint, so they can never
///   trip the local-index-reassignment tie detector — a guard that holds in
///   both the RED and GREEN states (assertion (2)).
///
/// RED against the CURRENT already-landed code: `solid_attribute_handles`
/// (engine_build.rs) tracks only the SOURCE occt solid handles (1, 2), never
/// the forwarded TARGET handles `forward_solid_attribute_on_ingest` writes at
/// `{Manifold, 5000}` / `{Manifold, 5001}`. Those forwarded entries carry
/// this realization's `feature_id`, so they pass the `feature_id`-only scan
/// filter, get centroid-queried against the occt default kernel under bare
/// ids 5000/5001 (deliberately non-colliding with any occt handle id used
/// here, so they are unambiguously scanned), and the query fails against the
/// unstaged occt mock — emitting a "topology-attribute centroid query failed
/// for 2 handle(s)" Warning, which assertion (1) below catches.
#[test]
fn forwarded_manifold_solid_entries_excluded_from_centroid_and_reassignment_scan() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_manifold_ingest_attribute_forwarding_pollution".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(two_sphere_union_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    // Kernel-emitted JSON shapes (mirrored from crates/reify-kernel-occt/src/lib.rs):
    // Centroid -> `{"x":_,"y":_,"z":_}`; BoundingBox -> `{"xmin":_,"ymin":_,"zmin":_,
    // "xmax":_,"ymax":_,"zmax":_}`. Values must parse via this shape (not just be
    // query-able) so the reassignment tie-detector actually walks real centroid
    // data — making assertion (2) below a genuine guard, not a vacuous one.
    let centroid_json =
        |x: f64, y: f64, z: f64| Value::String(format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}"));
    let bbox_json = |xmin: f64, ymin: f64, zmin: f64, xmax: f64, ymax: f64, zmax: f64| {
        Value::String(format!(
            "{{\"xmin\":{xmin},\"ymin\":{ymin},\"zmin\":{zmin},\"xmax\":{xmax},\"ymax\":{ymax},\
             \"zmax\":{zmax}}}"
        ))
    };

    // occt: two spheres allocate handles 1 and 2, faces 101/201, edges
    // 102/202 (same topology as the completion-(a) test above). Additionally
    // stage BOTH a centroid and a bbox result per real handle — distinct
    // coordinates per handle so the Role::Side pair (101, 201) and the
    // Role::NewEdge pair (102, 202) never tie.
    let occt_kernel = MockGeometryKernel::new()
        .with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(101)])
        .with_extracted_edges(GeometryHandleId(1), vec![GeometryHandleId(102)])
        .with_extracted_faces(GeometryHandleId(2), vec![GeometryHandleId(201)])
        .with_extracted_edges(GeometryHandleId(2), vec![GeometryHandleId(202)])
        .with_centroid_result(GeometryHandleId(101), centroid_json(1.0, 0.0, 0.0))
        .with_bbox_result(GeometryHandleId(101), bbox_json(0.5, -0.5, -0.5, 1.5, 0.5, 0.5))
        .with_centroid_result(GeometryHandleId(201), centroid_json(5.0, 0.0, 0.0))
        .with_bbox_result(GeometryHandleId(201), bbox_json(4.5, -0.5, -0.5, 5.5, 0.5, 0.5))
        .with_centroid_result(GeometryHandleId(102), centroid_json(1.0, 1.0, 1.0))
        .with_bbox_result(GeometryHandleId(102), bbox_json(0.0, 0.0, 0.0, 2.0, 2.0, 2.0))
        .with_centroid_result(GeometryHandleId(202), centroid_json(11.0, 11.0, 11.0))
        .with_bbox_result(GeometryHandleId(202), bbox_json(10.0, 10.0, 10.0, 12.0, 12.0, 12.0));

    let manifold_kernel = IngestTrackingManifoldKernel {
        inner: MockGeometryKernel::new(),
        next_ingest_id: 5000,
        ingested_handles: Arc::new(Mutex::new(Vec::new())),
    };

    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert("occt".to_string(), Box::new(occt_kernel));
    kernels.insert("manifold".to_string(), Box::new(manifold_kernel));

    let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    registry.insert(
        "occt".to_string(),
        CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveSphere, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        },
    );
    registry.insert(
        "manifold".to_string(),
        CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        },
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry,
        Some("occt".to_string()),
    );

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Stl);

    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "cross-kernel build must not emit error diagnostics; got: {errors:?}"
    );

    let query_failed: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("topology-attribute centroid query failed"))
        .collect();
    assert!(
        query_failed.is_empty(),
        "forwarded Manifold-scoped solid entries must be excluded from the centroid diagnostic \
         scan, not centroid-queried against the occt default kernel under their bare (unstaged) \
         handle id; got: {query_failed:?}"
    );

    let reassignment_diags: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(reify_core::DiagnosticCode::TopologyAttributeLocalIndexReassigned)
        })
        .collect();
    assert!(
        reassignment_diags.is_empty(),
        "distinct per-handle coordinates must not trip the local-index-reassignment tie \
         detector; got: {reassignment_diags:?}"
    );
}
