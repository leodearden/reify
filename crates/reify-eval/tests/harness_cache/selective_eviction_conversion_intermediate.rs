//! γ (#4730, PRD D4) for the one intermediate an edit can strand: a
//! cross-kernel conversion of a `Sub` operand, cached under
//! `ConversionSlot::External(<producer handle>)` and owned by the CONSUMER's
//! entity. Keyed eviction must take it down with its owner's family, and only
//! then.
//!
//! `G.moved = translate(a, …)` reads its sibling `a` as `GeomRef::Sub`, and
//! translate runs only on the Mesh kernel, so the per-op conversion executor
//! tessellates `a` on `occt`, ingests it into `manifold`, and caches the ingest
//! under `G`'s External key. That key embeds `a`'s handle id, so the pins are:
//! an unrelated edit keeps the key, an edit of the producer evicts it, and the
//! rebuild re-converts under a fresh key.
//!
//! Not part of the staleness differential: it needs a two-kernel registry, and
//! the differential's single-op-log `canonical_shape` cannot render an
//! ingested handle.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::realization_cache::{ConversionSlot, NO_OPTIONS, RealizationCache};
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{
    CapabilityDescriptor, ExportFormat, GeometryHandleId, KernelHandle, Operation, ReprKind,
};
use reify_test_support::mocks::{MockGeometryKernel, minimal_valid_mesh};
use reify_test_support::{compile_source, manufacturing_purpose, mm};

type Tessellated = Arc<Mutex<Vec<GeometryHandleId>>>;

/// The BRep kernel: builds solids and records every handle it tessellates
/// for a conversion.
struct BrepKernel {
    inner: MockGeometryKernel,
    tessellated: Tessellated,
}

impl reify_ir::GeometryKernel for BrepKernel {
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
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        self.tessellated.lock().unwrap().push(handle);
        Ok(minimal_valid_mesh(false))
    }

    /// The default kernel assembles the multi-body export.
    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.inner.make_compound(handles)
    }
}

/// The Mesh kernel: accepts ingested meshes under fresh ids and runs the
/// translate.
struct MeshKernel {
    inner: MockGeometryKernel,
    next_ingest_id: u64,
}

impl reify_ir::GeometryKernel for MeshKernel {
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
        Ok(reify_ir::GeometryHandle { id, repr: None })
    }
}

fn engine_over_brep_and_mesh_kernels(tessellated: &Tessellated) -> Engine {
    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert(
        "occt".to_string(),
        Box::new(BrepKernel {
            inner: MockGeometryKernel::new(),
            tessellated: Arc::clone(tessellated),
        }),
    );
    kernels.insert(
        "manifold".to_string(),
        Box::new(MeshKernel {
            inner: MockGeometryKernel::new(),
            next_ingest_id: 1000,
        }),
    );
    let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    registry.insert(
        "occt".to_string(),
        CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
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
            supports: vec![(Operation::TransformTranslate, ReprKind::Mesh)],
        },
    );
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(SimpleConstraintChecker),
        kernels,
        registry,
        Some("occt".to_string()),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    engine
}

fn external_key(producer: GeometryHandleId) -> String {
    RealizationCache::<KernelHandle>::intermediate_key("G", ConversionSlot::External(producer))
}

#[test]
fn an_external_conversion_intermediate_follows_its_owners_family() {
    const SRC: &str = r#"pub structure G {
    param w : Length = 10mm
    let a = box(w, 10mm, 10mm)
    let moved = translate(a, 1mm, 0mm, 0mm)
}
pub structure H {
    param h : Length = 5mm
    let k = box(h, 5mm, 5mm)
}"#;
    let mut module = compile_source(SRC);
    for (purpose, _) in [("mfg_g", "G"), ("mfg_h", "H")] {
        module
            .compiled_purposes
            .push(manufacturing_purpose(purpose, 1e-6));
    }
    let tessellated = Tessellated::default();
    let mut engine = engine_over_brep_and_mesh_kernels(&tessellated);
    engine.eval(&module);
    engine.activate_purpose("mfg_g", "G");
    engine.activate_purpose("mfg_h", "H");
    let build = |engine: &mut Engine| {
        let built = engine
            .build_snapshot(&module, ExportFormat::Stl)
            .expect("build_snapshot needs a prior eval");
        let errors: Vec<_> = built
            .diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .map(|d| d.message.clone())
            .collect();
        assert!(errors.is_empty(), "the build must succeed: {errors:#?}");
        built
    };
    let resident = |engine: &Engine, producer: GeometryHandleId| {
        engine
            .realization_cache()
            .bucket_len(&external_key(producer), ReprKind::Mesh, NO_OPTIONS)
    };

    let built = build(&mut engine);
    let converted = tessellated.lock().unwrap().clone();
    let a_cell = ValueCellId::new("G", "a");
    let a_id = match built.values.get(&a_cell) {
        Some(reify_ir::Value::GeometryHandle {
            kernel_handle: Some(id),
            ..
        }) => *id,
        other => panic!("premise: G.a holds a realized handle; got {other:?}"),
    };
    assert_eq!(
        converted,
        vec![a_id],
        "premise: the build converts exactly the Sub operand `a`, so its slot is External"
    );
    assert_eq!(
        resident(&engine, a_id),
        1,
        "premise: G owns a cached External intermediate"
    );

    engine
        .edit_param(ValueCellId::new("H", "h"), mm(6.0))
        .expect("edit_param(H.h) must succeed");
    assert!(
        engine
            .last_changed_realizations()
            .contains(&RealizationNodeId::new("H", 0)),
        "premise: the H edit is a real realization edit"
    );
    assert_eq!(
        resident(&engine, a_id),
        1,
        "an edit outside G's family must keep G's intermediate"
    );

    engine
        .edit_param(ValueCellId::new("G", "w"), mm(30.0))
        .expect("edit_param(G.w) must succeed");
    assert_eq!(
        resident(&engine, a_id),
        0,
        "an edit of the producer evicts G's family, which owns the External intermediate"
    );

    build(&mut engine);
    let converted = tessellated.lock().unwrap().clone();
    assert_eq!(
        converted.len(),
        2,
        "the rebuild re-converts `a`: {converted:?}"
    );
    let fresh_id = converted[1];
    assert_ne!(
        fresh_id, a_id,
        "the re-realized `a` carries a fresh handle id"
    );
    assert_eq!(
        resident(&engine, fresh_id),
        1,
        "the rebuild caches the new intermediate under the fresh External key"
    );
}
