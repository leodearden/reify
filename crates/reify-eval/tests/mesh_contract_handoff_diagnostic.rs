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
//! ## Task #5105 (kernel-seam δ) — both modes are pinned here
//!
//! β wired site 1 as warn-only. δ makes it mode-governed and flips the
//! default to [`MeshContractMode::Enforce`], with `REIFY_MESH_CONTRACT=warn`
//! as the break-glass downgrade. This file now pins BOTH postures against the
//! same harness, each mode set explicitly via the `set_mesh_contract_mode`
//! test seam so neither test depends on the ambient env var:
//!
//! - [`tessellate_handoff_warn_mode_warns_without_aborting`] — `Warn`: a
//!   `Severity::Warning` naming "occt" and "mesh contract", NO Error, and the
//!   ingest still fires once per union input (2).
//! - [`tessellate_handoff_enforce_mode_aborts_the_build`] — `Enforce`: a
//!   `Severity::Error` naming "occt" and "mesh contract", the conversion
//!   aborts BEFORE ingest (counter stays 0), and no geometry output is
//!   produced.
//!
//! Both tests carry the VALID-tetra control half, so the no-false-positive
//! guard holds under either mode.
//!
//! ## Coverage boundary: the tessellate surfaces are NOT pinned here
//!
//! `self.mesh_contract_mode` is forwarded to `tessellate_from_values` as an
//! explicit parameter, so `tessellate_realizations` / `tessellate_snapshot`
//! honour the `set_mesh_contract_mode` seam identically to `build` /
//! `build_snapshot`. That forwarding is NOT covered by a runtime test, and
//! deliberately so: those two surfaces hardcode `demanded_repr = ReprKind::BRep`
//! for every realization (engine_build.rs, "Task 5033 Gap D"), so the
//! cross-kernel BRep→Mesh handoff this file's harness relies on cannot be
//! planned there at all — the dispatcher rejects it upstream of site 1 with
//! `NoKernelChain` ("no kernel chain found for op 'BooleanUnion' to produce
//! 'BRep'"). Site 1 IS reachable from the tessellate surfaces, but only via
//! the `voxel_pipeline_demand_overrides` exception (an isosurface consumer
//! whose demand is overridden to `Mesh`), which needs a Voxel-capable mock
//! harness this file does not have. Closing that gap (plus the
//! `test_registry_override` seam the tessellate surfaces never consult) is the
//! scope of task #5530, which owns the tessellate-surface coverage.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_core::{ModulePath, Severity, Type};
use reify_eval::Engine;
use reify_ir::geometry::MeshContractMode;
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
///
/// Sourced from the shared [`reify_test_support::mocks::minimal_valid_mesh`]
/// helper rather than hand-rolled here, so a future correction to the tetra's
/// winding cannot silently desync this control half from the other mock
/// producers that use it.
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
        Ok(reify_test_support::mocks::minimal_valid_mesh(false))
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

/// PRD `kernel-seam-contracts.md` §4 site 1 (tasks #5103 β / #5105 δ,
/// INV-GEO-1) under [`MeshContractMode::Warn`]: `execute_realization_ops`'s
/// tessellate→ingest conversion executor must call `Mesh::validate()` on the
/// interchange mesh and, on a contract violation, push a `Severity::Warning`
/// diagnostic naming the producing kernel and the mesh contract — WITHOUT
/// aborting the build.
///
/// Warn is no longer the rollout default: after the δ flip it is the
/// explicitly-selected break-glass mode (`REIFY_MESH_CONTRACT=warn` in
/// production). This test therefore pins it via the `set_mesh_contract_mode`
/// seam rather than relying on the default, which also makes it independent
/// of whatever `REIFY_MESH_CONTRACT` happens to be set to in the environment
/// running the suite. The enforce counterpart is
/// [`tessellate_handoff_enforce_mode_aborts_the_build`].
///
/// Asserts both directions with two independent builds (fresh `Engine`s, so
/// there is no cross-build caching to account for): the violation half pins
/// warn's downgrade-and-continue behavior, the valid half pins the
/// no-false-positive guard.
#[test]
fn tessellate_handoff_warn_mode_warns_without_aborting() {
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
    engine.set_mesh_contract_mode(MeshContractMode::Warn);
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
        "Warn mode must never abort the build on a mesh-contract \
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
        "Warn mode must not skip the ingest: manifold.ingest_mesh must still fire \
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
    valid_engine.set_mesh_contract_mode(MeshContractMode::Warn);
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

/// Task #5105 (kernel-seam δ, INV-GEO-1): the enforce locking test — site 1
/// under [`MeshContractMode::Enforce`], which is the DEFAULT after the δ flip.
///
/// Same harness as [`tessellate_handoff_warn_mode_warns_without_aborting`],
/// only the asserted outcome differs. Under enforce, a contract violation at
/// the tessellate→ingest handoff must FAIL CLOSED: the conversion sets
/// `conversion_error` and `break 'convert`s, which routes into the existing
/// abort machinery (a `Severity::Error` diagnostic labelled with the
/// realization span, plus `kernel_error_out`). Three things follow, and all
/// three are asserted:
///
/// 1. a `Severity::Error` diagnostic naming both "occt" and "mesh contract";
/// 2. the abort happens BEFORE ingest, so `manifold.ingest_mesh` never fires
///    (counter stays 0) — this is what distinguishes enforce from warn, where
///    the same build ingests twice;
/// 3. no geometry output is emitted for the aborted realization.
///
/// The valid-tetra control half pins the no-false-positive guard under
/// enforce: the strictest mode must still build a well-formed mesh cleanly.
///
/// The mode is set explicitly rather than left to the default so the test
/// pins enforce behavior even when the suite runs with
/// `REIFY_MESH_CONTRACT=warn` in the environment.
#[test]
fn tessellate_handoff_enforce_mode_aborts_the_build() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_mesh_contract_handoff_enforce".to_string(),
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
            next_ingest_id: 3000,
        }),
    );
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry(),
        Some("occt".to_string()),
    );
    engine.set_mesh_contract_mode(MeshContractMode::Enforce);
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let violation_build = engine.build(&module, ExportFormat::Stl);

    let contract_errors: Vec<_> = violation_build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error) && d.message.contains("mesh contract"))
        .collect();
    assert!(
        !contract_errors.is_empty(),
        "Enforce mode must FAIL CLOSED on a mesh-contract violation: a \
         Severity::Error mesh-contract diagnostic is required; got diagnostics: {:?}",
        violation_build.diagnostics
    );
    assert!(
        contract_errors.iter().all(|d| d.message.contains("occt")),
        "the mesh-contract error must name the producing kernel ('occt'); got: \
         {contract_errors:?}"
    );

    assert!(
        violation_build
            .diagnostics
            .iter()
            .all(|d| !(matches!(d.severity, Severity::Warning)
                && d.message.contains("mesh contract"))),
        "Enforce must not ALSO emit the warn-mode diagnostic — the violation is \
         reported exactly once, as an Error; got diagnostics: {:?}",
        violation_build.diagnostics
    );

    assert_eq!(
        *ingest_count.lock().unwrap(),
        0,
        "Enforce must abort the conversion BEFORE the ingest: manifold.ingest_mesh \
         must never fire (contrast the warn-mode test, where the same build \
         ingests twice)"
    );

    assert!(
        violation_build.geometry_output.is_none(),
        "an aborted conversion must not produce geometry output; got {:?} bytes",
        violation_build.geometry_output.as_ref().map(|b| b.len())
    );

    // ── (b) VALID build under the SAME enforce mode: a closed, outward-wound
    //    tetrahedron must still build cleanly (no false positive). ──────────
    let valid_ingest_count = Arc::new(Mutex::new(0usize));
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
            ingest_count: Arc::clone(&valid_ingest_count),
            next_ingest_id: 4000,
        }),
    );
    let valid_checker = MockConstraintChecker::new();
    let mut valid_engine = Engine::with_test_kernels_and_registry(
        Box::new(valid_checker),
        valid_kernels,
        registry(),
        Some("occt".to_string()),
    );
    valid_engine.set_mesh_contract_mode(MeshContractMode::Enforce);
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
        "a valid closed tetra must build cleanly even under Enforce; got error \
         diagnostics: {valid_errors:?}"
    );
    assert_eq!(
        *valid_ingest_count.lock().unwrap(),
        2,
        "the valid build must reach the ingest normally under Enforce: \
         manifold.ingest_mesh fires once per union input (2)"
    );
}
