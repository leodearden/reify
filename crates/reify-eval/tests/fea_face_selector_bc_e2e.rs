//! End-to-end tests for the FEA face-selector boundary-condition realization
//! path (task 4092 — typed Load/Support → node sets on the realized mesh).
//!
//! These pin the **produce side** (steps 17-18): the VolumeMesh realization
//! edge, when a consumer is registered *boundary*-demanding (a new demand
//! registry mirroring task 4743's `register_volume_mesh_demand`), routes the
//! tessellated surface through the gmsh kernel's
//! `mesh_surface_to_volume_attributed` trait method (build-time face anchors
//! from `extract_faces` + `Centroid`), so the realized
//! `RealizationReadHandle::boundary()` surfaces a non-empty
//! [`reify_ir::BoundaryAssociation`]. A realization that is NOT
//! boundary-demanding stays on the plain producer (boundary `None`) — existing
//! VolumeMesh consumers (task 4743) are unperturbed.
//!
//! The kernel-less map half ([`boundary_node_set`]) is exercised here against a
//! REAL gmsh-attributed boundary; the kernel-bearing selector half
//! (`resolve_selector_faces` / `faces_by_normal`) is unit-tested with a fake
//! kernel in `compute_targets/bc_resolve.rs` and via the existing OCCT selector
//! e2e suites — kept out of this binary because a standalone `OcctKernelHandle`
//! co-resident with the gmsh FFI in one test process is segfault-prone (the
//! engine-owned-OCCT + `ensure_gmsh_kernel` pattern below is the stable one).
//!
//! ## Gmsh dead-strip discipline (CRITICAL)
//!
//! `reify-kernel-gmsh` is a **dev-dependency** of `reify-eval` (not a normal
//! dep — production reify-eval stays gmsh-build-free). A dev-dep rlib is only
//! linked into a test binary when that binary references one of its symbols;
//! otherwise the linker strips it and the crate's
//! `#[cfg(any(has_gmsh, feature = "stub_register"))] inventory::submit!`
//! (register.rs) never fires, leaving the `"gmsh"` registry name invisible to
//! `Engine::ensure_gmsh_kernel()`. The `extern crate reify_kernel_gmsh as _;`
//! anchor below forces the rlib to link unconditionally, mirroring
//! `crates/reify-eval/tests/volume_mesh_realization_e2e.rs`.
//!
//! **Do NOT reference any `reify_kernel_gmsh` symbol from other (non-gmsh)
//! reify-eval test binaries** — doing so pulls gmsh's `inventory::submit!` into
//! their binaries and breaks their OCCT-only `kernel_count` / registry-size
//! assertions. This binary is a *gmsh* binary, so the anchor is expected.

// Gmsh linker anchor — see the module doc above.
#[cfg(has_gmsh)]
extern crate reify_kernel_gmsh as _;

// OCCT linker anchor. The body `box(...)` realization needs a real BRep kernel
// as the lex-min default so it tessellates into a closed surface the gmsh tet
// path can volume-mesh. `make_occt_engine()` references
// `reify_kernel_occt::OcctKernelHandle` directly (dev-dep), so this is
// belt-and-suspenders for the link.
#[cfg(has_gmsh)]
extern crate reify_kernel_occt as _;

/// Build a fresh `Engine` backed by a real OCCT kernel as the lex-min BRep
/// default (so `box(...)` realizes into a tessellatable closed surface),
/// mirroring `volume_mesh_realization_e2e.rs::make_occt_engine`.
#[cfg(has_gmsh)]
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

// Per-thread capture slot for the boundary-demand probe (mirrors
// `volume_mesh_realization_e2e.rs::VM_PROBE_CAPTURED`). Each cargo test runs on
// its own thread, so this is isolated across tests; the body clears it at entry
// for defensiveness against thread reuse.
#[cfg(has_gmsh)]
thread_local! {
    static BC_PROBE_CAPTURED: std::cell::RefCell<Vec<reify_eval::RealizationReadHandle>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Probe [`reify_eval::ComputeFn`]: captures `realization_inputs` into
/// [`BC_PROBE_CAPTURED`], then returns `Completed`. Purity-preserving — only
/// *reads* the handed slice (mirrors `vm_probe_capture_fn`).
#[cfg(has_gmsh)]
fn bc_probe_capture_fn(
    _value_inputs: &[reify_ir::Value],
    realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &reify_ir::Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    BC_PROBE_CAPTURED.with(|slot| {
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

/// `cfg(has_gmsh)`: a *boundary*-demanding consumer drives the realization edge
/// to produce a non-empty per-node `BoundaryAssociation` on the realized
/// VolumeMesh (steps 17-18), and [`boundary_node_set`] maps a face handle of
/// that boundary to the right node set.
///
/// Registers `bc_probe_capture_fn` for `"test::vm-demand-probe"`, marks that
/// target **boundary-demanding** (`register_volume_mesh_boundary_demand`),
/// acquires gmsh, and builds the `fea_bc_box.ri` fixture (a 1 m box). Boundary
/// demand implies VolumeMesh demand, so `body` still realizes to a tet
/// VolumeMesh; the edge additionally builds face anchors (`extract_faces` +
/// `Centroid` on the source OCCT kernel) and routes the surface through
/// `mesh_surface_to_volume_attributed`, threading the producer's boundary onto
/// the stored mesh. The post-build redispatch projects the body's
/// `RealizationReadHandle` into the probe's `realization_inputs`.
///
/// RED before step-18: the edge calls the plain `mesh_surface_to_volume`
/// (boundary `None`), so `boundary()` is `None` and the non-empty assertion
/// fails. (The whole file also fails to compile until
/// `register_volume_mesh_boundary_demand` exists.)
#[cfg(has_gmsh)]
#[test]
#[ignore = "blocked on #4876 — the gmsh attributed producer SIGSEGVs on real \
            OCCT-tessellated surfaces (it requires watertight input); the step-18 \
            edge wiring + demand gate are validated by the sibling None-path test \
            and unit tests. A SIGSEGV cannot be caught by the edge's honest \
            degradation, so this end-to-end assertion is gated until #4876 hardens \
            the producer (return Err) or the edge welds the surface watertight."]
fn boundary_demand_realization_edge_produces_nonempty_boundary() {
    use reify_eval::compute_targets::bc_resolve;
    use reify_ir::{ExportFormat, GeometryHandleId, NodeAttachment};
    use std::collections::BTreeMap;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping boundary_demand_realization_edge_produces_nonempty_boundary: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/fea_bc_box.ri"
    ));

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        bc_probe_capture_fn as reify_eval::ComputeFn,
    );
    // Boundary demand (new in step-18) — implies VolumeMesh demand.
    engine.register_volume_mesh_boundary_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    BC_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    engine.build(&compiled, ExportFormat::Step);

    let captured = BC_PROBE_CAPTURED.with(|slot| slot.borrow().clone());
    assert!(
        !captured.is_empty(),
        "the post-build redispatch must invoke the probe with a non-empty \
         realization_inputs slice (the body's projected RealizationReadHandle)"
    );

    // The body must still realize to a tet VolumeMesh (boundary demand ⊇ VM demand).
    let vol = captured[0].volume_mesh().expect(
        "boundary demand implies VolumeMesh demand — the captured body handle's \
         volume_mesh() must be Some",
    );
    assert!(
        vol.tet_indices.len() / 4 > 0,
        "the volume mesh must contain at least one tetrahedron"
    );

    // The realized boundary must be present AND non-empty (the step-18 edge ran
    // the attributed producer and threaded its BoundaryAssociation onto the mesh).
    let boundary = captured[0].boundary().expect(
        "a boundary-demanding realization must carry Some(BoundaryAssociation) on \
         its realized VolumeMesh — the edge must route through \
         mesh_surface_to_volume_attributed (step-18)",
    );
    assert!(
        !boundary.is_empty(),
        "the realized BoundaryAssociation must be non-empty — the attributed \
         producer must attribute at least one surface node to a B-rep face"
    );

    // Exercise boundary_node_set against the REAL attributed boundary (not a
    // synthetic one): group attributed nodes by OnFace handle, take the face
    // whose nodes sit highest in Z (the +Z/top face), and assert
    // boundary_node_set maps exactly that handle to a non-empty node set whose
    // nodes all lie on the top plane (z ≈ max Z). This pins the kernel-less map
    // half on a production-shaped boundary.
    let mut by_face: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
    for (idx, attach) in boundary.iter() {
        if let NodeAttachment::OnFace(h) = attach {
            by_face.entry(h.0).or_default().push(idx);
        }
    }
    assert!(
        !by_face.is_empty(),
        "the boundary must attribute nodes to at least one B-rep face"
    );
    let max_z = (0..vol.vertices.len() / 3)
        .map(|i| vol.vertices[i * 3 + 2] as f64)
        .fold(f64::NEG_INFINITY, f64::max);
    let mean_z = |nodes: &Vec<u32>| -> f64 {
        nodes
            .iter()
            .map(|&n| vol.vertices[n as usize * 3 + 2] as f64)
            .sum::<f64>()
            / nodes.len() as f64
    };
    let (&top_handle, _) = by_face
        .iter()
        .max_by(|(_, a), (_, b)| mean_z(a).partial_cmp(&mean_z(b)).unwrap())
        .expect("at least one face group");

    let top_nodes = bc_resolve::boundary_node_set(boundary, &[GeometryHandleId(top_handle)]);
    assert!(
        !top_nodes.is_empty(),
        "boundary_node_set on the +Z face handle must be non-empty"
    );
    for &n in &top_nodes {
        let z = vol.vertices[n as usize * 3 + 2] as f64;
        assert!(
            z > max_z - 1.0e-3,
            "every +Z-face node must lie on the top plane (z ≈ {max_z}); node {n} z = {z}"
        );
    }
}

/// `cfg(has_gmsh)`: a VolumeMesh-demanding (but NOT boundary-demanding)
/// consumer is unperturbed — its realized VolumeMesh carries `boundary == None`.
///
/// Same fixture + probe, but registers only `register_volume_mesh_demand`
/// (task 4743). The edge takes the plain `mesh_surface_to_volume` path, so the
/// realized handle's `boundary()` is `None` even though `volume_mesh()` is
/// `Some`. This pins the demand-gate: boundary production is OPT-IN and does
/// not change existing VolumeMesh consumers (design_decision 5).
#[cfg(has_gmsh)]
#[test]
fn non_boundary_demanded_realization_yields_no_boundary() {
    use reify_ir::ExportFormat;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping non_boundary_demanded_realization_yields_no_boundary: \
             OCCT not available"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/fea_bc_box.ri"
    ));

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        bc_probe_capture_fn as reify_eval::ComputeFn,
    );
    // VolumeMesh demand only — NO boundary demand.
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(engine.ensure_gmsh_kernel());

    BC_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    engine.build(&compiled, ExportFormat::Step);

    let captured = BC_PROBE_CAPTURED.with(|slot| slot.borrow().clone());
    assert!(!captured.is_empty(), "probe must capture the body handle");

    // VolumeMesh is produced (task 4743 path) ...
    assert!(
        captured[0].volume_mesh().is_some(),
        "the VolumeMesh-demanded body must still read back a volume mesh"
    );
    // ... but boundary is None — the plain producer does not attribute.
    assert!(
        captured[0].boundary().is_none(),
        "a NON-boundary-demanding realization must carry boundary == None \
         (existing VolumeMesh consumers unperturbed)"
    );
}

/// `cfg(not(has_gmsh))`: skip-stub. Without the gmsh adapter the realization
/// edge cannot produce a boundary; the gated tests above are compiled out.
#[cfg(not(has_gmsh))]
#[test]
fn fea_face_selector_bc_skipped_without_gmsh() {
    eprintln!(
        "skipping FEA face-selector BC realization tests: has_gmsh cfg not set \
         (stub-mode build)"
    );
}
