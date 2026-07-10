//! Task #5103 (kernel-seam β, INV-GEO-1) — e2e pin for the `Mesh::validate()`
//! handoff diagnostic wired into `execute_realization_ops`'s
//! tessellate→ingest conversion executor (PRD
//! `docs/prds/kernel-seam-contracts.md` §4 site 1 / DAG leaf β, §12).
//!
//! Mirrors the harness in `cross_kernel_handoff.rs` (mock occt/manifold
//! kernels injected via the `test-instrumentation` seam,
//! `Engine::with_test_kernels_and_registry`, driven through
//! `Engine::build`), swapping the tessellate mock for one of two variants:
//!
//! - [`CorruptTessellateKernel`] returns a single-triangle mesh with a NaN
//!   vertex coordinate — trips `MeshInvariant::Finite` in `Mesh::validate`
//!   (checked before any other obligation in `Mesh::check_contract`, so the
//!   single open triangle's `Closed` obligation is never reached).
//! - [`ValidTessellateKernel`] returns a closed, consistently outward-wound
//!   unit tetrahedron — satisfies every `Mesh::validate` obligation (the
//!   no-false-positive control).
//!
//! ## RED → GREEN
//!
//! RED: no `validate()` call is wired at the handoff yet, so the VIOLATION
//! build's `diagnostics` carries no `Severity::Warning` mentioning "mesh
//! contract" — assertion (a) fails.
//!
//! GREEN: once `engine_build.rs::execute_realization_ops` calls
//! `mesh.validate(per_stage_tol)` right after `mesh` is bound and, on `Err`,
//! pushes a `Diagnostic::warning` built from
//! `violation.into_geometry_error(source_name).to_string()`, assertion (a)
//! passes: a Warning naming both "occt" and "mesh contract" appears, no
//! Error diagnostic is emitted (warn-default never aborts), and
//! `manifold.ingest_mesh` still fires once per union input (2). The VALID
//! build's assertion (b) — zero mesh-contract warnings — holds on both sides
//! of the RED→GREEN line, pinning the no-false-positive guard.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_core::{ModulePath, Severity, Type};
use reify_eval::Engine;
use reify_ir::{CapabilityDescriptor, CompiledExpr, ExportFormat, Operation, ReprKind};
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::MockGeometryKernel;
use reify_test_support::{MockConstraintChecker, manufacturing_purpose, mm, step_output_template};

/// occt-like tessellate kernel whose `tessellate` always returns a
/// deliberately contract-violating single-triangle mesh: vertex 0's
/// x-coordinate is NaN. `execute` / `query` / `export` delegate to an inner
/// [`MockGeometryKernel`], mirroring `CountingTessellateKernel` in
/// `cross_kernel_handoff.rs`.
struct CorruptTessellateKernel {
    inner: MockGeometryKernel,
}

impl reify_ir::GeometryKernel for CorruptTessellateKernel {
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
        handle: reify_ir::GeometryHandleId,
        format: reify_ir::ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        Ok(reify_ir::Mesh {
            vertices: vec![f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            normals: None,
        })
    }
}

/// occt-like tessellate kernel whose `tessellate` always returns a valid,
/// closed, consistently outward-wound unit tetrahedron: V0=(0,0,0),
/// V1=(1,0,0), V2=(0,1,0), V3=(0,0,1), faces (0,2,1),(0,1,3),(0,3,2),(1,2,3)
/// — the same fixture as `reify_ir::geometry::tests::welded_tetra_mesh`,
/// satisfying every `Mesh::validate` obligation.
struct ValidTessellateKernel {
    inner: MockGeometryKernel,
}

impl reify_ir::GeometryKernel for ValidTessellateKernel {
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
        handle: reify_ir::GeometryHandleId,
        format: reify_ir::ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        Ok(reify_ir::Mesh {
            #[rustfmt::skip]
            vertices: vec![
                0.0, 0.0, 0.0, // V0
                1.0, 0.0, 0.0, // V1
                0.0, 1.0, 0.0, // V2
                0.0, 0.0, 1.0, // V3
            ],
            #[rustfmt::skip]
            indices: vec![
                0, 2, 1, // face 0
                0, 1, 3, // face 1
                0, 3, 2, // face 2
                1, 2, 3, // face 3
            ],
            normals: None,
        })
    }
}

/// manifold-like ingest kernel counting `ingest_mesh` calls. Mirrors
/// `CountingManifoldKernel` in `cross_kernel_handoff.rs`.
struct CountingManifoldKernel {
    inner: MockGeometryKernel,
    ingest_count: Arc<Mutex<usize>>,
    next_ingest_id: u64,
}

impl reify_ir::GeometryKernel for CountingManifoldKernel {
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
        handle: reify_ir::GeometryHandleId,
        format: reify_ir::ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        handle: reify_ir::GeometryHandleId,
        tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn ingest_mesh(
        &mut self,
        _mesh: &reify_ir::Mesh,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        *self.ingest_count.lock().unwrap() += 1;
        let id = reify_ir::GeometryHandleId(self.next_ingest_id);
        self.next_ingest_id += 1;
        Ok(reify_ir::GeometryHandle { id, repr: None })
    }
}

/// Build the `MyDesign` template carrying ONE terminal realization whose ops
/// are two BRep `PrimitiveBox` solids consumed by a `BooleanUnion`. Mirrors
/// `cross_kernel_handoff.rs::my_design_template_with_union_realization`
/// (private to that file's own test module, hence re-defined here).
fn my_design_template_with_union_realization() -> reify_compiler::TopologyTemplate {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let box_op = || CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(10.0)),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    };
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        .realization_named("MyDesign", 0, "body", vec![box_op(), box_op(), union_op])
        .build()
}

/// `{occt: [(PrimitiveBox,BRep),(Convert{from:BRep},Mesh)], manifold:
/// [(BooleanUnion,Mesh)]}` — the same registry as `cross_kernel_handoff.rs`,
/// routing the union's BRep inputs through an occt tessellate stage before
/// the Mesh-only manifold union.
fn registry() -> BTreeMap<String, CapabilityDescriptor> {
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
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        },
    );
    registry
}

/// PRD `kernel-seam-contracts.md` §4 site 1 / DAG leaf β (task #5103,
/// INV-GEO-1): `execute_realization_ops`'s tessellate→ingest conversion
/// executor must call `Mesh::validate()` on the interchange mesh and, on a
/// contract violation, push a `Severity::Warning` diagnostic naming the
/// producing kernel and the mesh contract — WITHOUT aborting the build
/// (warn-default during rollout; the enforce flip is task δ).
///
/// Asserts both directions with two independent builds (fresh `Engine`s, so
/// there is no cross-build caching to account for) so this test is RED
/// before the impl (the violation half finds no mesh-contract warning)
/// while also pinning the no-false-positive guard (the valid half must
/// never warn).
#[test]
fn tessellate_handoff_validates_mesh_contract_and_warns_without_aborting() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_mesh_contract_handoff".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_union_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    // ── (a) VIOLATION build: occt.tessellate returns a NaN-vertex mesh. ────
    let ingest_count = Arc::new(Mutex::new(0usize));
    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert(
        "occt".to_string(),
        Box::new(CorruptTessellateKernel {
            inner: MockGeometryKernel::new(),
        }),
    );
    kernels.insert(
        "manifold".to_string(),
        Box::new(CountingManifoldKernel {
            inner: MockGeometryKernel::new(),
            ingest_count: Arc::clone(&ingest_count),
            next_ingest_id: 1000,
        }),
    );
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry(),
        Some("occt".to_string()),
    );
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let violation_build = engine.build(&module, ExportFormat::Stl);

    let errors: Vec<_> = violation_build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "warn-default must never abort the build on a mesh-contract \
         violation; got error diagnostics: {errors:?}"
    );

    let contract_warnings: Vec<_> = violation_build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Warning) && d.message.contains("mesh contract"))
        .collect();
    assert!(
        !contract_warnings.is_empty(),
        "a NaN-vertex mesh from occt.tessellate must produce a Severity::Warning \
         mesh-contract diagnostic; got diagnostics: {:?}",
        violation_build.diagnostics
    );
    assert!(
        contract_warnings.iter().all(|d| d.message.contains("occt")),
        "the mesh-contract warning must name the producing kernel ('occt'); got: \
         {contract_warnings:?}"
    );

    assert_eq!(
        *ingest_count.lock().unwrap(),
        2,
        "warn-default must not skip the ingest: manifold.ingest_mesh must still fire \
         once per union input (2) despite the per-input mesh-contract warning"
    );

    // ── (b) VALID build: occt.tessellate returns a closed, outward-wound
    //    tetrahedron — no mesh-contract warning (no false positive). ───────
    let mut valid_kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    valid_kernels.insert(
        "occt".to_string(),
        Box::new(ValidTessellateKernel {
            inner: MockGeometryKernel::new(),
        }),
    );
    valid_kernels.insert(
        "manifold".to_string(),
        Box::new(CountingManifoldKernel {
            inner: MockGeometryKernel::new(),
            ingest_count: Arc::new(Mutex::new(0usize)),
            next_ingest_id: 2000,
        }),
    );
    let valid_checker = MockConstraintChecker::new();
    let mut valid_engine = Engine::with_test_kernels_and_registry(
        Box::new(valid_checker),
        valid_kernels,
        registry(),
        Some("occt".to_string()),
    );
    let _valid_eval = valid_engine.eval(&module);
    valid_engine.activate_purpose("manufacturing", "MyDesign");

    let valid_build = valid_engine.build(&module, ExportFormat::Stl);
    let valid_errors: Vec<_> = valid_build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        valid_errors.is_empty(),
        "a valid closed tetra must build cleanly; got error diagnostics: {valid_errors:?}"
    );
    let valid_contract_warnings: Vec<_> = valid_build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Warning) && d.message.contains("mesh contract"))
        .collect();
    assert!(
        valid_contract_warnings.is_empty(),
        "a closed, consistently-wound tetra must satisfy the mesh contract and \
         produce zero mesh-contract warnings; got: {valid_contract_warnings:?}"
    );
}
