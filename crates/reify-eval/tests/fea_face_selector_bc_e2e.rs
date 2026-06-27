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

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary (OCCT
/// `make_box` centres the solid at the origin, so faces sit at ±5 mm). Same
/// convention as `topology_attribute_e2e.rs` and the `volume_mesh_box.ri`
/// fixture (`param width = 10mm`).
#[cfg(has_gmsh)]
const BOX_SIDE_M: f64 = 10.0e-3;

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
    }
}

/// `cfg(has_gmsh)`: a *boundary*-demanding consumer drives the realization edge
/// to produce a non-empty per-node `BoundaryAssociation` on the realized
/// VolumeMesh (steps 17-18).
///
/// Registers `bc_probe_capture_fn` for `"test::vm-demand-probe"`, marks that
/// target **boundary-demanding** (`register_volume_mesh_boundary_demand`),
/// acquires gmsh, and builds the `volume_mesh_box.ri` fixture. Boundary demand
/// implies VolumeMesh demand, so `body` still realizes to a tet VolumeMesh; the
/// edge additionally builds face anchors (`extract_faces` + `Centroid` on the
/// source OCCT kernel) and routes the surface through
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
fn boundary_demand_realization_edge_produces_nonempty_boundary() {
    use reify_ir::ExportFormat;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping boundary_demand_realization_edge_produces_nonempty_boundary: \
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
        "fixtures/volume_mesh_box.ri"
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

/// `cfg(all(has_gmsh, feature = "mesh-morph"))`: the full produce → resolve →
/// map compose, standalone on a real OCCT body.
///
/// Builds a 10 mm box on an OCCT kernel, assembles face anchors with the 4092
/// build-time helper (`build_face_anchors` = `extract_faces` + `Centroid`),
/// volume-meshes through the gmsh `mesh_surface_to_volume_attributed` trait
/// method, then resolves a `faces_by_normal([0,0,1], ~6°)` predicate selector
/// against the SAME OCCT body (`resolve_selector_faces`) and maps it to a node
/// set (`boundary_node_set`). Because both the anchors and the selector use the
/// same OCCT face handles, the composed +Z node set is non-empty and every
/// selected node lies on the top face (z ≈ +5 mm). This exercises the exact
/// chain the realization edge performs internally, end-to-end with selector
/// resolution.
///
/// `mesh-morph`-gated: the gmsh override of
/// `mesh_surface_to_volume_attributed` is `#[cfg(feature = "mesh-morph")]`
/// (the producer it wraps is `#[cfg(all(has_gmsh, feature = "mesh-morph"))]`).
#[cfg(all(has_gmsh, feature = "mesh-morph"))]
#[test]
fn faces_by_normal_resolves_to_attributed_plus_z_node_set() {
    use reify_core::identity::RealizationNodeId;
    use reify_core::ty::SelectorKind;
    use reify_core::Diagnostic;
    use reify_eval::compute_targets::bc_resolve;
    use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};
    use reify_ir::{ElementOrderTag, GeometryKernel, GeometryOp, NodeAttachment, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping faces_by_normal_resolves_to_attributed_plus_z_node_set: \
             OCCT not available"
        );
        return;
    }

    // ── (1) Build a 10 mm box on a real OCCT kernel ──────────────────────────
    let mut occt = reify_kernel_occt::OcctKernelHandle::spawn();
    let body = occt
        .execute(&GeometryOp::Box {
            width: Value::Real(BOX_SIDE_M),
            height: Value::Real(BOX_SIDE_M),
            depth: Value::Real(BOX_SIDE_M),
        })
        .expect("OCCT must build a 10mm box")
        .id;

    // Tessellate to a closed surface (1% of the side is a fine tolerance).
    let surface = occt
        .tessellate(body, BOX_SIDE_M * 1.0e-2)
        .expect("OCCT must tessellate the box into a closed surface");

    // ── (2) Build face anchors via the 4092 helper (extract_faces + Centroid)─
    let mut diags: Vec<Diagnostic> = Vec::new();
    let anchors = bc_resolve::build_face_anchors(&mut occt, body, &mut diags);
    assert_eq!(
        anchors.len(),
        6,
        "a box must yield 6 face anchors (one (face_handle, centroid) per face), \
         got {} (diags: {diags:?})",
        anchors.len()
    );

    // ── (3) Attributed volume mesh via the gmsh trait method ─────────────────
    // Match tolerance: a fraction of the side length — well above gmsh's
    // face-entity centroid drift, well below the ≈0.71·side inter-face-centroid
    // spacing (so faces never cross-match). Mirrors step-5's 0.3·side choice.
    let match_tol = 0.3 * BOX_SIDE_M;
    let gmsh = reify_kernel_gmsh::GmshKernel::new();
    let vm = gmsh
        .mesh_surface_to_volume_attributed(&surface, ElementOrderTag::P1, &anchors, match_tol)
        .expect("gmsh attributed volume meshing must succeed on the box surface");
    let boundary = vm
        .boundary
        .as_ref()
        .expect("the attributed producer must set VolumeMesh.boundary = Some");
    assert!(!boundary.is_empty(), "the BoundaryAssociation must be non-empty");

    // ── (4) Resolve faces_by_normal([0,0,1]) against the SAME OCCT body ──────
    let selector = SelectorValue::leaf(
        SelectorKind::Face,
        GeometryHandleRef {
            realization_ref: RealizationNodeId::new("body", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(body),
        },
        // ~6° tolerance (0.1 rad) about +Z.
        LeafQuery::ByNormal { dir: [0.0, 0.0, 1.0], tol_rad: 0.1 },
    )
    .expect("valid Face/ByNormal leaf");
    let plus_z_faces = bc_resolve::resolve_selector_faces(&selector, &mut occt, &mut diags)
        .expect("resolve_selector_faces must succeed against the realized OCCT body");
    assert!(
        !plus_z_faces.is_empty(),
        "faces_by_normal([0,0,1]) must select the +Z face of the box"
    );

    // ── (5) Map the resolved faces to a boundary node set ────────────────────
    let nodes = bc_resolve::boundary_node_set(boundary, &plus_z_faces);
    assert!(
        !nodes.is_empty(),
        "the +Z face node set must be non-empty — the resolved face handles must \
         match boundary OnFace attributions (both keyed by OCCT face handles)"
    );

    // Sanity: the resolved faces only attribute +Z (top) nodes, so every node in
    // the set lies on the top face (z ≈ +5 mm). Use a generous 0.1 mm band.
    let max_z = BOX_SIDE_M / 2.0;
    for &n in &nodes {
        let z = vm.vertices[n as usize * 3 + 2] as f64;
        assert!(
            z > max_z - 1.0e-4,
            "node {n} attributed to the +Z face must lie at the top (z ≈ {max_z}); got z = {z}"
        );
    }

    // Defensive: the set is exactly the OnFace(top-handle) nodes (no stray
    // OnEdge/OnVertex/other-face leakage).
    let top_faces: std::collections::HashSet<_> = plus_z_faces.iter().copied().collect();
    for (idx, attach) in boundary.iter() {
        if let NodeAttachment::OnFace(h) = attach
            && top_faces.contains(&h)
        {
            assert!(
                nodes.contains(&idx),
                "every OnFace(+Z) node must appear in the resolved node set"
            );
        }
    }
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
