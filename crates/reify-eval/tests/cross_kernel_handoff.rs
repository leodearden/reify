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
//!
//! ## Export through the owning kernel (task 6308)
//!
//! Each counting kernel's `export` writes an owner-tagged payload
//! (`occt-export:<id>` / `manifold-export:<id>`), so a test can read off which
//! kernel serialized a product body. Handle ids are kernel-local, so exporting
//! the Manifold-owned terminal through the default (occt) kernel names an
//! unrelated occt shape; the payload makes that defect self-describing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_compiler::CompiledModule;
use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_core::{Diagnostic, ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{CapabilityDescriptor, CompiledExpr, ExportFormat, KernelId, Operation, ReprKind};
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::MockGeometryKernel;
use reify_test_support::{MockConstraintChecker, manufacturing_purpose, mm, step_output_template};

/// Write `<owner>-export:<handle id>`: the payload both counting kernels
/// export, naming the kernel that serialized the body.
fn write_owner_tagged_export(
    owner: &str,
    handle: reify_ir::GeometryHandleId,
    writer: &mut dyn std::io::Write,
) -> Result<(), reify_ir::ExportError> {
    writer
        .write_all(format!("{owner}-export:{}", handle.0).as_bytes())
        .map_err(|e| reify_ir::ExportError::IoError(e.to_string()))
}

/// occt-like counting kernel: `execute` / `query` / `make_compound` delegate to
/// an inner [`MockGeometryKernel`] (so `PrimitiveBox` → BRep solid handles),
/// `export` writes `occt-export:<id>`, and `tessellate` bumps a shared counter
/// before returning a trivial
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
        _format: reify_ir::ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        write_owner_tagged_export("occt", handle, writer)
    }

    fn make_compound(
        &mut self,
        handles: &[reify_ir::GeometryHandleId],
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.inner.make_compound(handles)
    }

    fn tessellate(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        *self.tessellate_count.lock().unwrap() += 1;
        // Task #5105 δ (INV-GEO-1): a closed tetra, NOT the open triangle this
        // fixture used to return — site 1 now enforces the mesh contract by
        // default and a lone triangle fails it (open_edges=3). Shared with the
        // other mock producers to avoid drift.
        Ok(reify_test_support::mocks::minimal_valid_mesh(false))
    }
}

/// manifold-like counting kernel: `ingest_mesh` bumps a shared counter and
/// returns a fresh handle (the BRep→Mesh target projection), and `execute`
/// bumps a shared counter (the final cross-kernel `BooleanUnion` op runs here
/// via the default `execute_with_history` → `execute` delegation). `query` /
/// `tessellate` / `make_compound` delegate to an inner [`MockGeometryKernel`]
/// and `export` writes `manifold-export:<id>`; only the union is ever routed
/// here, so the `execute` counter is the `BooleanUnion`-on-Manifold count.
/// Mirrors the in-crate `CountingManifoldKernel`.
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
        _format: reify_ir::ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        write_owner_tagged_export("manifold", handle, writer)
    }

    fn make_compound(
        &mut self,
        handles: &[reify_ir::GeometryHandleId],
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.inner.make_compound(handles)
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

/// A BRep `PrimitiveBox` solid op (10 × 20 × 5 mm), advertised only by occt.
fn box_op() -> CompiledGeometryOp {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(10.0)),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    }
}

/// Build the `entity` template carrying ONE terminal realization whose ops are
/// two BRep `PrimitiveBox` solids consumed by a `BooleanUnion`. The realization
/// id is `(entity, index = 0)` and it is named `"body"` (caching requires a
/// named realization).
fn union_realization_template(entity: &str) -> reify_compiler::TopologyTemplate {
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    TopologyTemplateBuilder::new(entity)
        .param(entity, "thickness", Type::dimensionless_scalar(), None)
        .realization_named(entity, 0, "body", vec![box_op(), box_op(), union_op])
        .build()
}

/// The `MyDesign` union module: STEPOutput(1e-6) supplies the output-tolerance
/// contract and the manufacturing purpose binds it to the "MyDesign" entity, so
/// once that purpose is activated the realization caches at
/// (MyDesign, _, 1e-6). Mirrors the canonical cache-hit setup in
/// tolerance_wiring_e2e.rs.
fn my_design_union_module(module_name: &str) -> CompiledModule {
    CompiledModuleBuilder::new(ModulePath::new(vec![module_name.to_string()]))
        .template(step_output_template(1e-6))
        .template(union_realization_template("MyDesign"))
        .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
        .build()
}

/// Shared call counters the two counting kernels bump, read back after a build.
struct KernelCallCounts {
    tessellate: Arc<Mutex<usize>>,
    ingest: Arc<Mutex<usize>>,
    union: Arc<Mutex<usize>>,
}

/// An `Engine` over the `{occt, manifold}` counting kernels, with "occt" as the
/// default kernel, plus the counters those kernels bump.
///
/// occt: (PrimitiveBox, BRep) + (Convert{BRep}, Mesh); manifold:
/// (BooleanUnion, Mesh). For demanded = Mesh / available = {BRep} the
/// dispatcher yields plan { kernel: "manifold", conversions:
/// [(Occt, BRep, Mesh)] } for the union, and PrimitiveBox at Mesh demand
/// falls back to a BRep dispatch on occt (design_decision 3).
fn occt_manifold_engine() -> (Engine, KernelCallCounts) {
    let counts = KernelCallCounts {
        tessellate: Arc::new(Mutex::new(0)),
        ingest: Arc::new(Mutex::new(0)),
        union: Arc::new(Mutex::new(0)),
    };

    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert(
        "occt".to_string(),
        Box::new(CountingTessellateKernel {
            inner: MockGeometryKernel::new(),
            tessellate_count: Arc::clone(&counts.tessellate),
        }),
    );
    kernels.insert(
        "manifold".to_string(),
        Box::new(CountingManifoldKernel {
            inner: MockGeometryKernel::new(),
            ingest_count: Arc::clone(&counts.ingest),
            execute_count: Arc::clone(&counts.union),
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
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        },
    );

    let engine = Engine::with_test_kernels_and_registry(
        Box::new(MockConstraintChecker::new()),
        kernels,
        registry,
        Some("occt".to_string()),
    );
    (engine, counts)
}

fn error_diagnostics(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect()
}

/// A build's export payload as text, for failure messages: an `occt-export:`
/// payload where `manifold-export:` was expected explains itself.
fn payload_text(geometry_output: &Option<Vec<u8>>) -> String {
    match geometry_output {
        Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        None => "<no geometry output>".to_string(),
    }
}

/// End-to-end build()-path pin for the cross-kernel conversion executor +
/// cache_repr unpin. See the module doc comment for the full RED→GREEN
/// narrative. Drives `engine.build(&module, ExportFormat::Stl)` through the
/// pre-1 seam and asserts the per-kernel call counts, the terminal
/// `KernelId::Manifold` handle, `produced_repr == Mesh`, and a fully
/// cache-served second build.
#[test]
fn build_stl_routes_cross_kernel_union_to_manifold_and_caches() {
    let module = my_design_union_module("test_cross_kernel_handoff_build");
    let (mut engine, counts) = occt_manifold_engine();
    let KernelCallCounts {
        tessellate: tess_count,
        ingest: ingest_count,
        union: union_count,
    } = counts;

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

/// Both Phase-B export sites (`build`, `build_snapshot`) must serialize the
/// product body through the kernel that OWNS its handle. The terminal union
/// lives on manifold; exporting its id through the default occt kernel would
/// name an unrelated occt shape (task 6308).
#[test]
fn build_and_build_snapshot_export_the_cross_kernel_terminal_through_its_owner() {
    let module = my_design_union_module("test_cross_kernel_export_through_owner");
    let (mut engine, _counts) = occt_manifold_engine();
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Stl);
    let errors = error_diagnostics(&build.diagnostics);
    assert!(
        errors.is_empty(),
        "build must not emit error diagnostics; got: {errors:?}"
    );

    let terminal = engine
        .test_terminal_handle("MyDesign", ReprKind::Mesh, 1e-6)
        .expect("terminal handle must be cached at (MyDesign, Mesh, 1e-6)");
    assert_eq!(terminal.kernel, KernelId::Manifold);
    let owner_export = format!("manifold-export:{}", terminal.id.0);
    assert_eq!(
        payload_text(&build.geometry_output),
        owner_export,
        "build must export the manifold-owned terminal through manifold"
    );

    // build()→eval() cleared the purpose bindings; re-activate so the snapshot
    // build observes the same demanded tolerance and is served the cached
    // terminal.
    engine.activate_purpose("manufacturing", "MyDesign");
    let snapshot_build = engine
        .build_snapshot(&module, ExportFormat::Stl)
        .expect("build_snapshot must return Some after eval()");
    let snapshot_errors = error_diagnostics(&snapshot_build.diagnostics);
    assert!(
        snapshot_errors.is_empty(),
        "build_snapshot must not emit error diagnostics; got: {snapshot_errors:?}"
    );
    assert_eq!(
        payload_text(&snapshot_build.geometry_output),
        owner_export,
        "build_snapshot must export the manifold-owned terminal through manifold"
    );
}

/// Two product bodies on two different kernels cannot be assembled into one
/// compound (a compound takes ids in a single kernel's id space). The build
/// must say so with one Error naming both kernels and write no geometry —
/// not hand the manifold id to occt's `make_compound` and export whatever
/// occt shape that id happens to name.
#[test]
fn multi_body_export_with_mixed_owners_is_an_error_not_a_default_kernel_compound() {
    let box_part = TopologyTemplateBuilder::new("BoxPart")
        .realization_named("BoxPart", 0, "body", vec![box_op()])
        .build();
    let assembly = TopologyTemplateBuilder::new("Assembly")
        .sub_component("u", "UnionPart", vec![])
        .sub_component("b", "BoxPart", vec![])
        .build();
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cross_kernel_mixed_owner_export".to_string(),
    ]))
    .template(assembly)
    .template(union_realization_template("UnionPart"))
    .template(box_part)
    .build();
    let (mut engine, _counts) = occt_manifold_engine();

    let build = engine.build(&module, ExportFormat::Stl);

    let owners: Vec<KernelId> = engine
        .realization_kernel_provenance()
        .into_iter()
        .map(|p| p.kernel)
        .collect();
    assert!(
        owners.contains(&KernelId::Manifold) && owners.contains(&KernelId::Occt),
        "precondition: the two product bodies must live on manifold and occt; \
         realization owners were {owners:?}"
    );
    assert!(
        build.geometry_output.is_none(),
        "a mixed-owner multi-body export must write no geometry; got payload {:?}",
        payload_text(&build.geometry_output)
    );
    let errors = error_diagnostics(&build.diagnostics);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one Error for the mixed-owner export; got: {errors:?}"
    );
    assert!(
        errors[0].message.contains("manifold") && errors[0].message.contains("occt"),
        "the mixed-owner error must name both kernels; got: {}",
        errors[0].message
    );
}
