//! End-to-end tests for the VolumeMesh realization demand → execute → read
//! path (task 4743, PRD v0_6/volume-mesh-realization-and-morph-wiring.md α).
//!
//! ## Gmsh dead-strip discipline (CRITICAL)
//!
//! `reify-kernel-gmsh` is a **dev-dependency** of `reify-eval` (not a normal
//! dep — production reify-eval stays gmsh-build-free). A dev-dep rlib is only
//! linked into a test binary when that binary references one of its symbols;
//! otherwise the linker strips it and the crate's
//! `#[cfg(any(has_gmsh, feature = "stub_register"))] inventory::submit!`
//! (register.rs) never fires, leaving the `"gmsh"` registry name invisible to
//! `Engine::ensure_gmsh_kernel()`.
//!
//! The `extern crate reify_kernel_gmsh as _;` anchor below forces the rlib to
//! link unconditionally (more durable than a const read — rustc may inline a
//! const without emitting a symbol reference), mirroring the OpenVDB anchor in
//! `crates/reify-eval/tests/ensure_openvdb_kernel.rs:23`.
//!
//! **Do NOT reference any `reify_kernel_gmsh` symbol from other reify-eval test
//! binaries** — doing so would pull gmsh's `inventory::submit!` into their
//! binaries and break their OCCT-only `kernel_count` / registry-size assertions
//! (the manifold dead-strip discipline noted in `crates/reify-eval/Cargo.toml`).

// Gmsh linker anchor — see the module doc above.
#[cfg(has_gmsh)]
extern crate reify_kernel_gmsh as _;

// OCCT linker anchor. The body `box(...)` realization needs a real BRep kernel
// as the lex-min default so it tessellates into a closed surface the gmsh tet
// path can volume-mesh. `make_occt_engine()` below references
// `reify_kernel_occt::OcctKernelHandle` directly (dev-dep, mirrors the gmsh
// dev-dep), so this `extern crate` is belt-and-suspenders for the link.
#[cfg(has_gmsh)]
extern crate reify_kernel_occt as _;

/// `cfg(has_gmsh)`: `Engine::ensure_gmsh_kernel()` idempotently inserts the
/// Gmsh adapter from the inventory registry.
///
/// A fresh `Engine::new(checker, None)` holds zero kernels. The first
/// `ensure_gmsh_kernel()` looks up `KernelId::Gmsh.as_registry_name()` in
/// `kernel_registry::registry()` (populated by the anchored
/// `inventory::submit!`), inserts the adapter, returns `true`, and increments
/// `kernel_count()` by exactly 1. A second call is idempotent (returns `true`,
/// count unchanged).
///
/// RED before step-10: `Engine::ensure_gmsh_kernel` does not exist.
#[cfg(has_gmsh)]
#[test]
fn ensure_gmsh_kernel_adds_gmsh_and_is_idempotent() {
    use reify_kernel_gmsh::register::GMSH_KERNEL_NAME;
    use reify_test_support::mocks::MockConstraintChecker;

    let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(MockConstraintChecker::new()), None);

    // Baseline: `Engine::new(checker, None)` holds zero kernels; gmsh absent.
    let baseline = engine.kernel_count();
    assert!(
        !engine
            .registered_kernel_names()
            .any(|n| n == GMSH_KERNEL_NAME),
        "baseline Engine::new must not pre-load gmsh.\nregistered kernels: {:?}",
        engine.registered_kernel_names().collect::<Vec<_>>(),
    );

    // First call: inserts gmsh from the inventory registry → true, +1.
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must return true when the gmsh adapter is in the \
         registry and not yet in the engine"
    );
    assert!(
        engine
            .registered_kernel_names()
            .any(|n| n == GMSH_KERNEL_NAME),
        "after ensure_gmsh_kernel(): gmsh must be in registered_kernel_names().\n\
         registered kernels: {:?}",
        engine.registered_kernel_names().collect::<Vec<_>>(),
    );
    assert_eq!(
        engine.kernel_count(),
        baseline + 1,
        "kernel_count must increase by exactly 1 after ensure_gmsh_kernel()"
    );

    // Second call: idempotent — returns true, count unchanged.
    assert!(
        engine.ensure_gmsh_kernel(),
        "second ensure_gmsh_kernel() call must also return true (kernel already present)"
    );
    assert_eq!(
        engine.kernel_count(),
        baseline + 1,
        "kernel_count must be unchanged after an idempotent second ensure_gmsh_kernel() call"
    );
}

/// Build a fresh `Engine` backed by a real OCCT kernel as the lex-min BRep
/// default (so `box(...)` realizes into a tessellatable closed surface),
/// mirroring `as_printed_body_realization_e2e.rs::make_occt_engine`.
#[cfg(has_gmsh)]
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), Some(Box::new(kernel)))
}

/// `cfg(has_gmsh)`: the demand → execute call edge writes VolumeMesh + Gmsh.
///
/// Build the `volume_mesh_box.ri` fixture (a `box` body consumed by the
/// `@optimized("test::vm-demand-probe")` `vm_probe`) through a real OCCT engine
/// with the probe target registered VolumeMesh-demanding and gmsh acquired.
/// The module-static demand pass overrides `body`'s demanded `ReprKind` to
/// `VolumeMesh`; `execute_realization_ops` then tessellates the terminal OCCT
/// BRep handle and routes it through `dispatch_volume_mesh` (tet path) → gmsh
/// `store_volume_mesh`, re-terminating the `body` realization on the gmsh
/// kernel.
///
/// Asserts via the public `realization_kernel_provenance()` that exactly the
/// call edge's writes landed: a realization with `repr == VolumeMesh` AND
/// `kernel == Gmsh`. (The `volume_mesh()` *content* round-trip — tet structure,
/// `% 4 == 0`, P1 tag — is the probe-read path, covered by the step-13 e2e;
/// `realization_handles` itself is a private field, so the public provenance
/// surface is the dead-strip-safe assertion here.)
///
/// RED before step-12: no call edge exists, so `body` falls back to a BRep box
/// on the OCCT kernel (`repr == BRep`, `kernel == Occt`) and no VolumeMesh+Gmsh
/// realization is present.
#[cfg(has_gmsh)]
#[test]
fn call_edge_writes_volume_mesh_repr_and_gmsh_kernel_for_demanded_body() {
    use reify_core::KernelId;
    use reify_ir::{ExportFormat, ReprKind};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping call_edge_writes_volume_mesh_repr_and_gmsh_kernel_for_demanded_body: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/volume_mesh_box.ri"
    ));

    let mut engine = make_occt_engine();
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    engine.build(&compiled, ExportFormat::Step);

    // Public provenance read: the body realization must have been re-terminated
    // on the gmsh kernel at VolumeMesh by the call edge.
    let provenance = engine.realization_kernel_provenance();
    assert!(
        provenance
            .iter()
            .any(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh),
        "after build, the VolumeMesh-demanded `body` realization must produce \
         repr == VolumeMesh on kernel == Gmsh (the execute_realization_ops → \
         dispatch_volume_mesh call edge). provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );
}

/// `cfg(has_gmsh)`: the call edge degrades HONESTLY when the VolumeMesh demand
/// is set but no gmsh kernel is registered (the gmsh-absent branch).
///
/// Drives the same demand → execute call edge as
/// `call_edge_writes_volume_mesh_repr_and_gmsh_kernel_for_demanded_body`, but
/// deliberately does NOT call `ensure_gmsh_kernel()`. The module-static demand
/// pass still overrides `body`'s demanded `ReprKind` to `VolumeMesh`, so the
/// call edge fires: it tessellates the terminal OCCT BRep handle, then finds no
/// `"gmsh"` kernel in the map (`kernels.get(KernelId::Gmsh…)` → `None`) and
/// degrades honestly — emitting a "no gmsh kernel registered" warning and
/// leaving `body` at its BRep/Occt fallback. No panic, no VolumeMesh provenance,
/// no silent BRep→VolumeMesh mislabel.
///
/// This pins the call edge's primary honest-degradation branch. A regression
/// that turned that warning into a panic, or that stored a BRep/Occt handle
/// under a VolumeMesh/Gmsh provenance, would be caught here (the happy path and
/// the registry-absent `ensure_gmsh_kernel == false` path do not exercise it).
#[cfg(has_gmsh)]
#[test]
fn call_edge_degrades_honestly_without_gmsh_kernel() {
    use reify_core::KernelId;
    use reify_ir::{ExportFormat, ReprKind};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping call_edge_degrades_honestly_without_gmsh_kernel: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/volume_mesh_box.ri"
    ));

    let mut engine = make_occt_engine();
    // Demand IS registered, but `ensure_gmsh_kernel()` is deliberately NOT
    // called — the engine holds only the OCCT default kernel.
    engine.register_volume_mesh_demand("test::vm-demand-probe");

    let result = engine.build(&compiled, ExportFormat::Step);

    // (1) Honest fallback: NO realization was re-terminated at VolumeMesh or on
    //     the gmsh kernel — the call edge left `body` at its BRep/Occt terminal.
    let provenance = engine.realization_kernel_provenance();
    assert!(
        provenance
            .iter()
            .all(|p| p.repr != ReprKind::VolumeMesh && p.kernel != KernelId::Gmsh),
        "without ensure_gmsh_kernel(), the call edge must NOT produce a VolumeMesh \
         repr or a Gmsh-owned realization (honest degradation — no silent \
         mis-store). provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );
    // The single `body` realization (the only geometry realization in the
    // fixture) must remain a BRep box on OCCT. The realization id string is
    // "VolumeMeshBox#realization[0]" — the source name `body` is NOT in the id —
    // so identify the fallback structurally by its (BRep, Occt) signature.
    assert!(
        provenance
            .iter()
            .any(|p| p.repr == ReprKind::BRep && p.kernel == KernelId::Occt),
        "the VolumeMesh-demanded `body` realization must fall back to a BRep box \
         on the OCCT kernel when gmsh is absent. provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );

    // (2) Honest signal: the call edge surfaced the "no gmsh kernel registered"
    //     warning into BuildResult.diagnostics rather than failing silently.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no gmsh kernel registered")),
        "the call edge must emit a 'no gmsh kernel registered' diagnostic when the \
         demand is set but ensure_gmsh_kernel() was not called. diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
    );
}

// ── step-13 e2e: full demand → execute → read probe-capture path ─────────────

// Per-thread capture slot for `vm_probe_capture_fn`.
//
// Each cargo test runs on its own thread, so this is isolated across tests;
// the e2e clears it at entry for defensiveness against thread reuse. Mirrors
// `realization_read_api.rs::PROBE_CAPTURED`.
#[cfg(has_gmsh)]
thread_local! {
    static VM_PROBE_CAPTURED: std::cell::RefCell<Vec<reify_eval::RealizationReadHandle>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Probe [`reify_eval::ComputeFn`] for the VolumeMesh e2e: captures
/// `realization_inputs` into [`VM_PROBE_CAPTURED`], then returns `Completed`.
///
/// Purity-preserving (realization-read-api §3.2-1): only *reads* the handed
/// `&[RealizationReadHandle]` slice — the capture is test-only observation of
/// what the engine pre-projected, and the `ComputeFn` signature is unchanged
/// (no `&Engine` / `GeometryKernel` reachable). Mirrors
/// `realization_read_api.rs::probe_capture_fn`.
#[cfg(has_gmsh)]
fn vm_probe_capture_fn(
    _value_inputs: &[reify_ir::Value],
    realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &reify_ir::Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    VM_PROBE_CAPTURED.with(|slot| {
        *slot.borrow_mut() = realization_inputs.to_vec();
    });
    reify_eval::ComputeOutcome::Completed {
        result: reify_ir::Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// `cfg(has_gmsh)`: the user-observable end-to-end VolumeMesh realization path
/// (PRD v0_6/volume-mesh-realization-and-morph-wiring.md α, §7 done-gate).
///
/// Compiles the `volume_mesh_box.ri` fixture (a `box` body consumed by the
/// `@optimized("test::vm-demand-probe")` `vm_probe`), registers
/// `vm_probe_capture_fn` for that target, marks the target VolumeMesh-demanding,
/// acquires gmsh, and drives `engine.build(&compiled, ExportFormat::Step)`.
///
/// ## Why `build()`, not `eval()`
///
/// `engine.eval()` is the pure-eval entry: it evaluates value cells and mints
/// **symbolic** geometry handles (`mint_symbolic_geometry_handles_into_values`,
/// `kernel_handle: None`) — it never runs `execute_realization_ops` and never
/// runs `redispatch_geometry_consuming_compute_nodes`. The VolumeMesh read-back
/// requires a REAL kernel-realized handle (the gmsh tet mesh), so the test must
/// drive the realizing path. `build()` → `build_with_geometry_output()` realizes
/// geometry through the kernel AND runs the post-hydration redispatch
/// (engine_build.rs), which is the only production path that delivers a
/// VolumeMesh to a geometry-consuming `@optimized` consumer.
///
/// The full production chain under test:
///   1. The module-static demand pass (`compute_demanded_reprs`) sees the
///      `vm_probe(body)` value-cell `UserFunctionCall` resolving to the
///      registered VolumeMesh-demand target and overrides `body`'s demanded
///      `ReprKind` to `VolumeMesh`.
///   2. `execute_realization_ops` tessellates the terminal OCCT BRep handle and
///      routes it through `dispatch_volume_mesh` (tet path) → gmsh
///      `store_volume_mesh`, re-terminating the `body` realization on gmsh.
///   3. The post-hydration redispatch (`redispatch_geometry_consuming_compute_nodes`)
///      projects the body's `RealizationReadHandle` (VolumeMesh arm →
///      `resolve_realization_kernel` → gmsh `volume_mesh()`) into the probe's
///      `realization_inputs` and re-dispatches `vm_probe`, which captures it.
///
/// Asserts the captured body handle's `volume_mesh()` is `Some` with
/// `tet_indices.len() % 4 == 0`, `> 0` tets, and `element_order ==
/// ElementOrderTag::P1` (the P1 tag round-trips production → storage →
/// read-back). STRUCTURAL only — no numeric-accuracy bound (PRD §7).
#[cfg(has_gmsh)]
#[test]
fn e2e_vm_probe_reads_back_tet_volume_mesh_from_demanded_body() {
    use reify_ir::{ElementOrderTag, ExportFormat};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping e2e_vm_probe_reads_back_tet_volume_mesh_from_demanded_body: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/volume_mesh_box.ri"
    ));

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        vm_probe_capture_fn as reify_eval::ComputeFn,
    );
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    // Defensive clear against thread reuse.
    VM_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    // `build()` (not `eval()`) realizes geometry through the kernel and runs the
    // post-hydration redispatch — see the "Why build(), not eval()" doc above.
    engine.build(&compiled, ExportFormat::Step);

    let captured = VM_PROBE_CAPTURED.with(|slot| slot.borrow().clone());
    assert!(
        !captured.is_empty(),
        "the post-build redispatch must invoke vm_probe with a non-empty \
         realization_inputs slice (the body's projected RealizationReadHandle); \
         captured nothing — the geometry-consuming @optimized node was not \
         re-dispatched or its inputs were empty"
    );

    let vol = captured[0].volume_mesh().expect(
        "the captured body handle's volume_mesh() must be Some — the demand → \
         execute (gmsh tet) → project → read path must deliver a VolumeMesh, not \
         a None-content (BRep-only) handle",
    );
    let vol_tet_indices = vol
        .tet_indices()
        .expect("gmsh-produced volume mesh must be tet connectivity");
    assert_eq!(
        vol_tet_indices.len() % 4,
        0,
        "tet_indices.len() must be divisible by 4 (P1 tet connectivity); got {}",
        vol_tet_indices.len()
    );
    assert!(
        vol_tet_indices.len() / 4 > 0,
        "the volume mesh must contain at least one tetrahedron; got {} indices",
        vol_tet_indices.len()
    );
    assert_eq!(
        vol.element_order(),
        Some(ElementOrderTag::P1),
        "the P1 element-order tag must round-trip production → storage → read-back"
    );
}

/// The `volume_mesh_box.ri` shape plus a tolerance contract, so the
/// VolumeMesh-demanded `body` terminal is written to the realization cache.
///
/// The terminal cache insert is gated on `is_terminal_realization &&
/// demanded_tol.is_some() && realization_name.is_some()`; the
/// `RepresentationWithin` bound declared inside the SAME structure is what
/// makes `demanded_tol` Some (the shape proven in
/// `solve_elastic_static_body_e2e.rs::NON_PRISMATIC_ONE_CASE_BODY_SOURCE`).
#[cfg(has_gmsh)]
const CACHED_VOLUME_MESH_BOX_SOURCE: &str = r#"
@optimized("test::vm-demand-probe")
fn vm_probe(g: Geometry) -> Int {
    0
}

structure FeaMeshTolerance {
    param nominal : Real = 1.0
}

structure CachedVolumeMeshBox {
    param mesh_tol : FeaMeshTolerance = FeaMeshTolerance()
    constraint RepresentationWithin(mesh_tol, 100um)

    param width: Length = 10mm
    param depth: Length = 10mm
    param height: Length = 10mm

    let body = box(width, depth, height)
    let probe = vm_probe(body)
}
"#;

/// OCCT names every exported product `Open CASCADE STEP translator <version>
/// <n>`, where `<n>` counts the STEP writes made by this process.
#[cfg(has_gmsh)]
const OCCT_PRODUCT_NAME: &str = "Open CASCADE STEP translator ";

/// The STEP text without its two per-write markers, so two exports of the
/// same body compare equal: the header line carrying the write timestamp, and
/// the version and write count after [`OCCT_PRODUCT_NAME`].
#[cfg(has_gmsh)]
fn step_without_write_markers(step: &[u8]) -> String {
    String::from_utf8_lossy(step)
        .lines()
        .filter(|line| !line.contains("FILE_NAME("))
        .map(|line| {
            let mut pieces = line.split(OCCT_PRODUCT_NAME);
            let mut kept = pieces.next().unwrap_or_default().to_string();
            for piece in pieces {
                kept.push_str(OCCT_PRODUCT_NAME);
                kept.push_str(&piece[piece.find('\'').unwrap_or(piece.len())..]);
            }
            kept
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `cfg(has_gmsh)`: a cache-served VolumeMesh terminal exports the same body
/// the cold build exported.
///
/// The cold build meshes the OCCT `box` into a gmsh VolumeMesh and caches that
/// terminal; Phase-B egress reads the body the VolumeMesh was meshed from. The
/// warm build is served from the realization cache without the VolumeMesh edge
/// running, so it must find the same meshed-from body rather than handing the
/// gmsh VolumeMesh to the exporter.
#[cfg(has_gmsh)]
#[test]
fn cache_served_volume_mesh_terminal_exports_the_body_the_cold_build_exported() {
    use reify_core::{KernelId, Severity};
    use reify_ir::{ExportFormat, ReprKind};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping cache_served_volume_mesh_terminal_exports_the_body_the_cold_build_exported: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(CACHED_VOLUME_MESH_BOX_SOURCE);

    let mut engine = make_occt_engine();
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    let before = engine.cache_stats().realization_entries;
    let cold = engine.build(&compiled, ExportFormat::Step);
    let mid = engine.cache_stats().realization_entries;
    let warm = engine.build(&compiled, ExportFormat::Step);
    let after = engine.cache_stats().realization_entries;

    assert!(
        mid - before >= 1,
        "precondition: the cold build must cache the toleranced VolumeMesh terminal; \
         realization_entries went {before} -> {mid}"
    );
    assert_eq!(
        after, mid,
        "precondition: the warm build must be cache-served and realize nothing new"
    );
    let provenance = engine.realization_kernel_provenance();
    assert!(
        provenance
            .iter()
            .any(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh),
        "precondition: after the warm build a (VolumeMesh, Gmsh) realization must \
         still be reported. provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );

    let mut exports = Vec::new();
    for (label, result) in [("cold", &cold), ("warm", &warm)] {
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.clone())
            .collect();
        assert!(
            errors.is_empty(),
            "the {label} build must report no Error diagnostic. got: {errors:?}"
        );
        let bytes = result
            .geometry_output
            .as_deref()
            .filter(|bytes| !bytes.is_empty())
            .unwrap_or_else(|| panic!("the {label} build must export non-empty STEP geometry"));
        exports.push(step_without_write_markers(bytes));
    }
    assert_eq!(
        exports[0], exports[1],
        "the cache-served build must export the same body the cold build exported"
    );
}

/// The `[x, y, z]` extents of `mesh`'s vertices, in metres.
#[cfg(has_gmsh)]
fn axis_extents_m(mesh: &reify_ir::Mesh) -> [f64; 3] {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in mesh.vertices.chunks_exact(3) {
        for axis in 0..3 {
            min[axis] = min[axis].min(f64::from(vertex[axis]));
            max[axis] = max[axis].max(f64::from(vertex[axis]));
        }
    }
    [0, 1, 2].map(|axis| max[axis] - min[axis])
}

/// `cfg(has_gmsh)`: after a build has cached a body's gmsh VolumeMesh terminal,
/// the tessellate walk (the GUI viewport surface) still draws that body as the
/// 10 mm box it was meshed from.
///
/// The VolumeMesh is an FEA representation the viewport cannot draw. The
/// tessellate walk demands BRep, so it is not served the cached VolumeMesh;
/// this pins the drawn outcome, not that mechanism.
#[cfg(has_gmsh)]
#[test]
fn tessellate_walk_draws_a_volume_mesh_demanded_body_as_its_box() {
    use reify_core::{KernelId, Severity};
    use reify_ir::{ExportFormat, ReprKind};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping tessellate_walk_draws_a_volume_mesh_demanded_body_as_its_box: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }
    const BOX_EDGE_M: f64 = 0.010;
    const EXTENT_TOLERANCE_M: f64 = 1e-6;

    let compiled = reify_test_support::parse_and_compile_with_stdlib(CACHED_VOLUME_MESH_BOX_SOURCE);
    let mut engine = make_occt_engine();
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    engine.build(&compiled, ExportFormat::Step);
    assert!(
        engine
            .realization_kernel_provenance()
            .iter()
            .any(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh),
        "precondition: the build must realize `body` as a gmsh VolumeMesh"
    );
    let result = engine.tessellate_realizations(&compiled);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.is_empty(),
        "the tessellate walk must report no Error diagnostic. got: {errors:?}"
    );
    let surfaces: Vec<_> = result.meshes.iter().map(|s| &s.entity_path).collect();
    let [body] = result.meshes.as_slice() else {
        panic!("the tessellate walk must draw exactly the `body` surface. got: {surfaces:?}");
    };
    assert!(
        !body.mesh.indices.is_empty(),
        "the `body` surface must have triangles"
    );
    let extents = axis_extents_m(&body.mesh);
    assert!(
        extents
            .iter()
            .all(|extent| (extent - BOX_EDGE_M).abs() <= EXTENT_TOLERANCE_M),
        "the `body` surface must span the 10 mm box on every axis. extents (m): {extents:?}"
    );
}

/// `cfg(not(has_gmsh))`: skip-stub.
///
/// When the gmsh adapter is absent (no `has_gmsh` cfg), the registry does not
/// contain `"gmsh"` → `ensure_gmsh_kernel()` returns `false` (honest absence).
#[cfg(not(has_gmsh))]
#[test]
fn ensure_gmsh_kernel_returns_false_without_gmsh() {
    use reify_test_support::mocks::MockConstraintChecker;

    eprintln!(
        "skipping VolumeMesh-realization gmsh assertions: has_gmsh cfg not set \
         (stub-mode build); ensure_gmsh_kernel() must return false"
    );

    let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(MockConstraintChecker::new()), None);
    assert!(
        !engine.ensure_gmsh_kernel(),
        "stub mode: ensure_gmsh_kernel() must return false when the gmsh adapter \
         is absent from the registry (honest degradation)"
    );
}
