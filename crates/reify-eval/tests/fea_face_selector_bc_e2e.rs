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
//! [`reify_ir::BoundaryAssociation`] — exercised directly against a
//! hand-built watertight surface in
//! `crates/reify-kernel-gmsh/tests/mesh_surface_to_volume_attributed.rs`. On
//! a REAL OCCT-tessellated surface the raw index buffer is unwelded
//! (per-face vertex blocks, `occt_wrapper.cpp:5847`) and the #4876
//! watertightness preflight (`preflight_watertight_surface`,
//! `reify-kernel-gmsh/src/mesh_boundary.rs`) refuses it, so the edge
//! gracefully degrades to the plain producer instead — see
//! [`boundary_demand_realization_edge_degrades_gracefully_on_occt_surface`].
//! Producing a non-empty boundary from a REAL OCCT surface requires the
//! attribution-preserving repair tracked by #5116. A realization that is NOT
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

/// `cfg(has_gmsh)`: on a REAL OCCT-tessellated surface, a *boundary*-demanding
/// consumer drives the realization edge through the gmsh attributed producer
/// — which the #4876 watertightness preflight (`preflight_watertight_surface`,
/// mesh_boundary.rs) refuses, because the raw OCCT surface is unwelded (see
/// [`characterizes_4876_occt_tessellation_unwelded_witness`] below) — so the
/// edge's EXISTING honest-degradation falls back to the plain producer.
///
/// Registers `bc_probe_capture_fn` for `"test::vm-demand-probe"`, marks that
/// target **boundary-demanding** (`register_volume_mesh_boundary_demand`),
/// acquires gmsh, and builds the `fea_bc_box.ri` fixture (a 1 m box). Boundary
/// demand implies VolumeMesh demand, so `body` still realizes to a tet
/// VolumeMesh via the plain producer; the attributed producer is attempted
/// first, refused by the preflight (`Err(GeometryError::MeshContractViolation)`),
/// and the refusal is surfaced as a visible `Severity::Warning` diagnostic —
/// NOT a SIGSEGV.
///
/// Before #4876, this real-OCCT path SIGSEGVs inside gmsh — a crash the
/// edge's `Result`-based honest degradation cannot catch. The non-empty
/// `BoundaryAssociation` observable this test asserted pre-#4876 requires the
/// attribution-preserving repair tracked by #5116 and is deliberately NOT
/// re-asserted here (it is exercised on a hand-built watertight surface in
/// `crates/reify-kernel-gmsh/tests/mesh_surface_to_volume_attributed.rs`
/// instead).
#[cfg(has_gmsh)]
#[test]
fn boundary_demand_realization_edge_degrades_gracefully_on_occt_surface() {
    use reify_core::Severity;
    use reify_ir::ExportFormat;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping boundary_demand_realization_edge_degrades_gracefully_on_occt_surface: \
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
    // Boundary demand — implies VolumeMesh demand.
    engine.register_volume_mesh_boundary_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    BC_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    let result = engine.build(&compiled, ExportFormat::Step);

    let captured = BC_PROBE_CAPTURED.with(|slot| slot.borrow().clone());
    assert!(
        !captured.is_empty(),
        "the post-build redispatch must invoke the probe with a non-empty \
         realization_inputs slice (the body's projected RealizationReadHandle)"
    );

    // The body must still realize to a tet VolumeMesh: boundary demand ⊇ VM
    // demand, and the plain producer (the degradation target) still yields
    // tets even though the attributed producer was refused.
    let vol = captured[0].volume_mesh().expect(
        "boundary demand implies VolumeMesh demand — the captured body handle's \
         volume_mesh() must be Some even when the attributed producer is refused",
    );
    assert!(
        vol.tet_indices().expect("fixture is tet-only").len() / 4 > 0,
        "the volume mesh must contain at least one tetrahedron"
    );

    // Graceful degradation: the #4876 preflight refused the attributed
    // producer's raw (unwelded) OCCT surface, so NO boundary is threaded onto
    // the realized mesh — an honest degradation, not a crash.
    assert!(
        captured[0].boundary().is_none(),
        "on a real OCCT surface the #4876 watertightness preflight must refuse the \
         attributed producer, so the realized VolumeMesh's boundary must be None \
         (graceful degradation to the plain producer — a non-empty boundary \
         requires the attribution-preserving repair tracked by #5116)"
    );

    // The degradation must be VISIBLE: a Severity::Warning diagnostic naming
    // both the attributed-producer failure and the plain-producer fallback
    // (the existing engine_build.rs honest-degradation arm). Matched on the
    // stable keyword tokens "attributed" + "gmsh" + "plain producer" rather
    // than the full sentence as one contiguous phrase: scanning
    // engine_build.rs's other realization Warning messages around this arm
    // shows "attributed" + "gmsh" together uniquely identify this branch
    // (the sibling "attributed store_volume_mesh failed" message shares
    // "attributed" and "plain producer" but never says "gmsh"; the
    // plain-producer "gmsh ... failed" messages never say "attributed"), so
    // this survives a benign reword of the message's phrasing/word-order.
    // engine_build.rs is outside this task's locked scope, so factoring the
    // text into a shared const both sites reference isn't available here.
    let degradation_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(d.severity, Severity::Warning)
                && d.message.contains("attributed")
                && d.message.contains("gmsh")
                && d.message.contains("plain producer")
        })
        .collect();
    assert!(
        !degradation_warnings.is_empty(),
        "the attributed-producer refusal must surface a visible honest-degradation \
         warning diagnostic naming both the failure and the plain-producer fallback \
         (matched loosely on 'attributed' + 'gmsh' + 'plain producer' tokens); \
         got diagnostics: {:?}",
        result.diagnostics
    );

    // No SIGSEGV: reaching this assertion (rather than the test process
    // aborting) is itself part of what this test pins.
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

/// `cfg(has_gmsh)`: characterizes the REAL OCCT tessellation of the #4876
/// fixture (`fea_bc_box.ri`, a 1 m box) against the α `MeshContract`
/// validator (`Mesh::validate`, INV-GEO-1) and the `weldedness()` axis,
/// pinning the concrete witness the gmsh attributed producer's SIGSEGV
/// (#4876) trips on.
///
/// `tessellate_realizations` drives the SAME `kernel.tessellate(handle,
/// tol)` OCCT path the realization edge (engine_build.rs:7655) feeds to
/// `mesh_surface_to_volume_attributed`, so the characterized mesh is
/// faithful to the crash input (G6 discipline). OCCT tessellates per-face
/// vertex blocks (occt_wrapper.cpp:5847) — unwelded by design — so the
/// surface is a valid closed 2-manifold ONLY on the position-welded
/// quotient; on the RAW indices it is non-watertight (the gmsh attributed
/// producer forbids vertex-merge repair, mesh_boundary.rs:219-227, and
/// consumes exactly that raw non-watertight surface).
///
/// This is a CHARACTERIZATION (pinning) test documenting EXISTING behavior
/// — green on arrival by design. It would go RED under a regression: OCCT
/// welding its own output, or α gating weldedness / rejecting unwelded
/// input outright.
///
/// Greenness is also contingent on OCCT emitting bit-exact (identical f32)
/// coordinates for corner/edge vertices shared across faces, since welding
/// is bit-exact (`weld_positions` compares `to_bits`). This holds for a
/// unit axis-aligned box (corner coords 0.0/1.0 are exact f32, planar faces
/// are not subdivided); a future OCCT/tessellation-tolerance change that
/// emits a shared face-perimeter vertex with even a 1-ULP difference
/// between its two incident faces would surface here first, as a spurious
/// `validate(0.0)` failure unrelated to the α contract this test pins.
///
/// RED before the next step: `real_occt_box_surface` does not yet exist →
/// the gmsh test binary fails to compile.
#[cfg(has_gmsh)]
#[test]
fn characterizes_4876_occt_tessellation_unwelded_witness() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping characterizes_4876_occt_tessellation_unwelded_witness: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let mesh = real_occt_box_surface();

    // Witness 1+2 (coupled): the raw OCCT surface is UNWELDED (the
    // consumer-capability axis the gmsh `requires_welded` producer trips
    // on) yet fully satisfies every producer obligation on the
    // position-welded quotient — unwelded is a capability gap, not a
    // producer-obligation violation (PRD §2).
    let w = mesh.weldedness(0.0);
    // `raw_welded` is defined as `weld_merged_verts == 0` (geometry.rs), so
    // asserting `weld_merged_verts > 0` below also pins `!raw_welded` — one
    // witness, not two independent ones.
    assert!(
        w.weld_merged_verts > 0,
        "real OCCT box tessellation must be unwelded (per-face vertex \
         blocks, occt_wrapper.cpp:5847) — a box shares corners across 3 \
         faces and edge vertices across 2 faces, so position-welding must \
         collapse duplicates: {w:?}"
    );
    let validated = mesh.validate(0.0);
    assert!(
        validated.is_ok(),
        "real OCCT box tessellation must satisfy every producer obligation \
         on the welded quotient (unwelded is a capability, not a \
         violation): {:?}",
        validated.err()
    );

    // Witness 3: the RAW (pre-weld) open-edge count — the "open/
    // non-watertight edges" witness that only exists on the raw topology.
    // α's `validate` welds internally, so its `open_edges` count is on the
    // welded quotient, which is 0 for a box; this is a deliberately
    // distinct, unwelded directed-edge tally recording exactly what the
    // gmsh attributed producer (which forbids vertex-merge repair,
    // mesh_boundary.rs:219-227) sees and SIGSEGVs on.
    let raw_open_edges = raw_open_edge_count(&mesh);
    assert!(
        raw_open_edges > 0,
        "the RAW per-face-block OCCT surface must be non-watertight (open \
         edges at every shared face-perimeter edge) — this is the raw \
         topology the gmsh attributed producer consumes: {raw_open_edges}"
    );

    eprintln!(
        "characterizes_4876_occt_tessellation_unwelded_witness: \
         weld_merged_verts={}, raw_open_edges={raw_open_edges}",
        w.weld_merged_verts
    );
}

/// `cfg(has_gmsh)`: obtain the REAL OCCT tessellation of the #4876 fixture's
/// `body` (`fea_bc_box.ri`, a 1 m box), via engine-owned OCCT.
///
/// Drives `Engine::tessellate_realizations`, which calls the SAME
/// `kernel.tessellate(handle, tol)` OCCT path the realization edge
/// (engine_build.rs:7655) feeds to `mesh_surface_to_volume_attributed`, so
/// the returned mesh is faithful to the crash input (G6 discipline).
/// Deliberately never calls `ensure_gmsh_kernel()` — gmsh FFI is therefore
/// never co-resident with the standalone-OCCT-kernel path, per the module
/// doc's segfault warning above.
///
/// Panics if tessellation reports an `Error`-severity diagnostic, produces
/// no meshes at all, or surfaces no mesh whose entity path is prefixed
/// `FeaBcBox` (the box `body` realization).
#[cfg(has_gmsh)]
fn real_occt_box_surface() -> reify_ir::Mesh {
    use reify_core::Severity;

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/fea_bc_box.ri"
    ));

    let mut engine = make_occt_engine();
    // The fixture's `@optimized("test::vm-demand-probe")` target needs a
    // registered compute trampoline, or evaluation falls back to
    // body-inlining and records an Error-severity diagnostic (mirrors the
    // registration the sibling tests above perform). This helper never
    // reads the probe's captured result — it only needs the diagnostics to
    // stay clean so the `errors.is_empty()` assert below is meaningful.
    engine.register_compute_fn(
        "test::vm-demand-probe",
        bc_probe_capture_fn as reify_eval::ComputeFn,
    );
    let result = engine.tessellate_realizations(&compiled);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected tessellation errors on fea_bc_box.ri: {:?}",
        errors
    );
    assert!(
        !result.meshes.is_empty(),
        "tessellate_realizations must surface at least one mesh for fea_bc_box.ri"
    );

    // Select the box `body` realization: the sole geometry realization in
    // the fixture, surfaced under an entity path prefixed by the structure
    // name `FeaBcBox`. Deliberately panics (rather than silently falling
    // back to an unrelated mesh) if that convention ever drifts, so an
    // entity-path rename surfaces as a test failure instead of this helper
    // silently characterizing the wrong surface.
    let surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path.starts_with("FeaBcBox"))
        .unwrap_or_else(|| {
            panic!(
                "no FeaBcBox realization mesh in fea_bc_box.ri output \
                 (entity-path convention may have drifted); entity paths \
                 present: {:?}",
                result
                    .meshes
                    .iter()
                    .map(|s| s.entity_path.as_str())
                    .collect::<Vec<_>>()
            )
        });

    surface.mesh.clone()
}

/// `cfg(has_gmsh)`: count directed edges in the mesh's RAW (pre-weld) index
/// buffer whose reverse does not occur — i.e. open/non-watertight boundary
/// edges on the topology the gmsh attributed producer actually consumes (it
/// forbids vertex-merge repair, mesh_boundary.rs:219-227).
///
/// Deliberately distinct from [`reify_ir::Mesh::validate`]'s
/// `Closed`/`ConsistentWinding` obligations, which run on the
/// POSITION-WELDED quotient topology (where a box is closed — 0 open
/// edges). This is a raw, unwelded directed-edge tally answering a
/// different topological question.
#[cfg(has_gmsh)]
fn raw_open_edge_count(mesh: &reify_ir::Mesh) -> usize {
    use std::collections::HashSet;

    let mut directed: HashSet<(u32, u32)> = HashSet::new();
    for tri in mesh.indices.chunks_exact(3) {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        for &(u, v) in &[(a, b), (b, c), (c, a)] {
            directed.insert((u, v));
        }
    }

    directed
        .iter()
        .filter(|&&(u, v)| !directed.contains(&(v, u)))
        .count()
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
