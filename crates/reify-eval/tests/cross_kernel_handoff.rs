//! Task 4050 (gap 3 / gap 4) — in-realization cross-kernel conversion executor,
//! end-to-end through `Engine::build`.
//!
//! This is the build()-path capstone for the in-realization conversion executor
//! TDD'd in-crate against a synthetic registry in
//! `crates/reify-eval/src/engine_build.rs::tests` (the
//! `execute_realization_ops_conversion_path_*` set). Where those unit tests call
//! `execute_realization_ops` directly, this test drives the full
//! `Engine::build` pipeline so it also exercises the υ demand-derivation wiring
//! (step-16: `compute_demanded_reprs` → `demanded_repr` argument) and the pre-1
//! `#[cfg(feature = "test-instrumentation")]` harness seam
//! (`with_test_kernels_and_registry` + `test_terminal_handle`).
//!
//! ## Why a seam is required (not optional)
//!
//! reify-eval links no Mesh-capable boolean kernel (its `Cargo.toml` has an
//! openvdb dep, an occt dev-dep, and NO `reify-kernel-manifold`), so the
//! link-time `inventory` registry (`collect_registry()`) cannot satisfy a Mesh
//! `BooleanUnion` — a real cross-kernel handoff cannot be driven through the
//! public API at all. And the terminal `KernelHandle` is not graph-observable
//! (a `RealizationNodeData` stores only `produced_repr: ReprKind`, not the
//! originating `KernelId`). The pre-1 seam closes both gaps: it injects a
//! deterministic `{occt, manifold}` capability map plus call-counting mock
//! kernels, and reads back the terminal handle's `KernelId` through the
//! realization cache. The seam is gated behind the `test-instrumentation`
//! feature, enabled for this test binary via reify-eval's self-dev-dep
//! (`Cargo.toml:54`).
//!
//! ## RED → GREEN
//!
//! RED before step-16: `build` / `build_snapshot` still pass `ReprKind::BRep`
//! as the executor's `demanded_repr` (the step-8 placeholder). With
//! demanded == BRep the terminal `BooleanUnion` dispatches at BRep, neither
//! `occt` nor `manifold` advertises `(BooleanUnion, BRep)`, the dispatcher
//! returns `None`, the no-fallback (demanded == BRep) path takes the
//! no-kernel-chain error arm, and no conversion runs — so the tessellate /
//! ingest / union counters stay at 0, `produced_repr` stays BRep, and no
//! terminal handle is cached.
//!
//! GREEN after step-16: `build` derives the per-realization demanded repr via
//! `compute_demanded_reprs(module, Stl)`, which marks the terminal realization
//! `Mesh`. The union then dispatches at Mesh → the Mesh-capable `manifold`
//! kernel, preceded by a BRep→Mesh conversion stage carried by `occt`. The
//! executor tessellates each of the union's two BRep input handles on `occt`
//! (2), ingests each into `manifold` (2), runs the union on `manifold` (1),
//! tags the terminal handle `KernelId::Manifold`, and caches it at
//! `(MyDesign, Mesh, 1e-6)` so a second identical build is served entirely
//! from the cache.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_core::{ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{CapabilityDescriptor, CompiledExpr, ExportFormat, KernelId, Operation, ReprKind};
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::MockGeometryKernel;
use reify_test_support::{MockConstraintChecker, manufacturing_purpose, mm, step_output_template};

/// occt-like counting kernel: `execute` / `query` / `export` delegate to an
/// inner [`MockGeometryKernel`] (so `PrimitiveBox` → BRep solid handles), and
/// `tessellate` bumps a shared counter before returning a trivial
/// single-triangle [`reify_ir::Mesh`] — the BRep→Mesh source projection the
/// conversion executor drives for each prior-stage input handle. Mirrors the
/// in-crate `CountingTessellateKernel` in `engine_build.rs::tests` (which is
/// private to that test module, hence re-defined here).
struct CountingTessellateKernel {
    inner: MockGeometryKernel,
    tessellate_count: Arc<Mutex<usize>>,
}

impl reify_ir::GeometryKernel for CountingTessellateKernel {
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
        *self.tessellate_count.lock().unwrap() += 1;
        Ok(reify_ir::Mesh {
            vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            normals: None,
        })
    }
}

/// manifold-like counting kernel: `ingest_mesh` bumps a shared counter and
/// returns a fresh handle (the BRep→Mesh target projection), and `execute`
/// bumps a shared counter (the final cross-kernel `BooleanUnion` op runs here
/// via the default `execute_with_history` → `execute` delegation). `query` /
/// `export` / `tessellate` delegate to an inner [`MockGeometryKernel`]; only
/// the union is ever routed here, so the `execute` counter is the
/// `BooleanUnion`-on-Manifold count. Mirrors the in-crate `CountingManifoldKernel`.
struct CountingManifoldKernel {
    inner: MockGeometryKernel,
    ingest_count: Arc<Mutex<usize>>,
    execute_count: Arc<Mutex<usize>>,
    next_ingest_id: u64,
}

impl reify_ir::GeometryKernel for CountingManifoldKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        *self.execute_count.lock().unwrap() += 1;
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
/// are two BRep `PrimitiveBox` solids consumed by a `BooleanUnion`. The
/// realization id is `(entity = "MyDesign", index = 0)` and it is named
/// `"body"` (caching requires a named realization). Entity name `"MyDesign"`
/// matches the entity the manufacturing purpose is activated against, so the
/// realization's demanded tolerance resolves to `1e-6`.
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

/// End-to-end build()-path pin for the cross-kernel conversion executor +
/// cache_repr unpin. See the module doc comment for the full RED→GREEN
/// narrative. Drives `engine.build(&module, ExportFormat::Stl)` through the
/// pre-1 seam and asserts the per-kernel call counts, the terminal
/// `KernelId::Manifold` handle, `produced_repr == Mesh`, and a fully
/// cache-served second build.
#[test]
fn build_stl_routes_cross_kernel_union_to_manifold_and_caches() {
    // STEPOutput(1e-6) supplies the output-tolerance contract and the
    // manufacturing purpose binds it to the "MyDesign" entity so the
    // realization caches at (MyDesign, _, 1e-6). Mirrors the canonical
    // cache-hit setup in tolerance_wiring_e2e.rs.
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cross_kernel_handoff_build".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_union_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    // Shared call counters, read back after each build via the Arc clones.
    let tess_count = Arc::new(Mutex::new(0usize));
    let ingest_count = Arc::new(Mutex::new(0usize));
    let union_count = Arc::new(Mutex::new(0usize));

    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert(
        "occt".to_string(),
        Box::new(CountingTessellateKernel {
            inner: MockGeometryKernel::new(),
            tessellate_count: Arc::clone(&tess_count),
        }),
    );
    kernels.insert(
        "manifold".to_string(),
        Box::new(CountingManifoldKernel {
            inner: MockGeometryKernel::new(),
            ingest_count: Arc::clone(&ingest_count),
            execute_count: Arc::clone(&union_count),
            next_ingest_id: 1000,
        }),
    );

    // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Mesh); manifold:
    // (BooleanUnion, Mesh). For demanded = Mesh / available = {BRep} the
    // dispatcher yields plan { kernel: "manifold", conversions:
    // [(Occt, BRep, Mesh)] } for the union, and PrimitiveBox at Mesh demand
    // falls back to a BRep dispatch on occt (design_decision 3).
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

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry,
        Some("occt".to_string()),
    );

    // Activate the manufacturing purpose against MyDesign so the build observes
    // demanded_tol = Some(1e-6). eval() is called first (matching the canonical
    // pattern) because build()→check()→eval() clears active_purpose_bindings,
    // and we re-activate before the second build below.
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    // ── First build (cold cache): Stl → υ demands Mesh for the terminal
    //    realization → cross-kernel handoff. ───────────────────────────────
    let build1 = engine.build(&module, ExportFormat::Stl);
    let errors: Vec<_> = build1
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "cross-kernel build must not emit error diagnostics; got: {errors:?}"
    );

    // (a) occt.tessellate fires once per BooleanUnion input handle = 2.
    assert_eq!(
        *tess_count.lock().unwrap(),
        2,
        "occt.tessellate must be called once per union input handle (2); a value \
         of 0 means build() still passes ReprKind::BRep (pre step-16) so no Mesh \
         conversion runs"
    );
    // (b) manifold.ingest_mesh fires once per converted input = 2.
    assert_eq!(
        *ingest_count.lock().unwrap(),
        2,
        "manifold.ingest_mesh must be called once per converted input (2)"
    );
    // (c) manifold runs the final BooleanUnion exactly once.
    assert_eq!(
        *union_count.lock().unwrap(),
        1,
        "manifold must run the final BooleanUnion exactly once"
    );

    // (d) the realization graph node's produced_repr is Mesh.
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let my_design_node = snap
        .graph
        .realizations
        .iter()
        .find(|(id, _)| id.entity == "MyDesign")
        .map(|(_, r)| r)
        .expect("MyDesign realization node must be present in the snapshot graph");
    assert_eq!(
        my_design_node.produced_repr,
        ReprKind::Mesh,
        "the terminal MyDesign realization must record produced_repr == Mesh \
         (the cross-kernel union resolves to the Mesh-capable manifold kernel)"
    );

    // (e) the terminal handle, read back through the realization cache, is a
    //     Manifold handle (plan.kernel for the cross-kernel union).
    let terminal = engine
        .test_terminal_handle("MyDesign", ReprKind::Mesh, 1e-6)
        .expect("terminal handle must be cached at (MyDesign, Mesh, 1e-6)");
    assert_eq!(
        terminal.kernel,
        KernelId::Manifold,
        "terminal handle must be tagged KernelId::Manifold, got {:?}",
        terminal.kernel
    );

    // ── Second build (warm cache): the realization is served entirely from
    //    the RealizationCache, so no dispatch fires and no kernel is touched. ─
    // Re-activate the purpose (build()→eval() cleared active_purpose_bindings)
    // so the second build observes the same demanded_tol = Some(1e-6) that
    // keyed the first build's cache entry.
    engine.activate_purpose("manufacturing", "MyDesign");

    let build2 = engine.build(&module, ExportFormat::Stl);
    let errors2: Vec<_> = build2
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors2.is_empty(),
        "second cross-kernel build must not emit error diagnostics; got: {errors2:?}"
    );

    // (f) cache hit at every layer: dispatch count 0 and UNCHANGED counters.
    assert_eq!(
        engine.last_dispatch_count(),
        0,
        "second build must be served entirely from the RealizationCache \
         (cache-hit short-circuit returns before the per-op loop, so dispatch \
         never fires); got last_dispatch_count()={}",
        engine.last_dispatch_count(),
    );
    assert_eq!(
        *tess_count.lock().unwrap(),
        2,
        "occt.tessellate must be UNCHANGED on the cache-served second build (still 2)"
    );
    assert_eq!(
        *ingest_count.lock().unwrap(),
        2,
        "manifold.ingest_mesh must be UNCHANGED on the cache-served second build (still 2)"
    );
    assert_eq!(
        *union_count.lock().unwrap(),
        1,
        "manifold BooleanUnion must be UNCHANGED on the cache-served second build (still 1)"
    );
}
