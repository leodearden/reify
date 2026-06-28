//! End-to-end test for the morph-or-remesh arm at the VolumeMesh realization
//! dispatch (task 4744 β, PRD `docs/prds/v0_6/volume-mesh-realization-and-morph-wiring.md`).
//!
//! Drives the full production chain a parameter tick takes: build the
//! `morph_box.ri` fixture through a real OCCT engine with the
//! `@optimized("test::vm-demand-probe")` VolumeMesh-demand probe, gmsh acquired,
//! AND `reify_mesh_morph::register_morph_producer` installed. The first build
//! produces a from-scratch (remesh) source VolumeMesh; a NON-structural
//! parameter tick (`width` 10mm → 10.5mm, topology-preserving) followed by a
//! warm `build_snapshot` must MORPH the prior mesh onto the new BRep —
//! preserving connectivity (identical `tet_indices`) and recording exactly one
//! `morphed` diagnostic.
//!
//! ## Gmsh dead-strip discipline (CRITICAL — mirrors volume_mesh_realization_e2e.rs)
//!
//! `reify-kernel-gmsh` is a **dev-dependency** of `reify-eval`. A dev-dep rlib
//! is only linked into a test binary when that binary references one of its
//! symbols; otherwise the linker strips it and the gmsh `inventory::submit!`
//! never fires, leaving `"gmsh"` invisible to `Engine::ensure_gmsh_kernel()`.
//! The `extern crate reify_kernel_gmsh as _;` anchor forces the link.
//!
//! **Do NOT reference any `reify_kernel_gmsh` symbol from OCCT-only reify-eval
//! test binaries** — it would pull gmsh's `inventory::submit!` into them and
//! break their `kernel_count` / registry-size assertions. This binary
//! legitimately needs gmsh (the remesh tet path produces the morph source).

// Gmsh linker anchor — see the module doc above.
#[cfg(has_gmsh)]
extern crate reify_kernel_gmsh as _;

// OCCT linker anchor. `make_occt_engine()` references `OcctKernelHandle`
// directly (dev-dep); this `extern crate` is belt-and-suspenders for the link.
#[cfg(has_gmsh)]
extern crate reify_kernel_occt as _;

/// Build a fresh `Engine` backed by a real OCCT kernel as the lex-min BRep
/// default, mirroring `volume_mesh_realization_e2e.rs::make_occt_engine`.
#[cfg(has_gmsh)]
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

// Per-thread capture slot for `morph_probe_capture_fn`. Each cargo test runs on
// its own thread; the e2e clears it at entry for defensiveness against reuse.
#[cfg(has_gmsh)]
thread_local! {
    static MORPH_PROBE_CAPTURED: std::cell::RefCell<Vec<reify_eval::RealizationReadHandle>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Probe [`reify_eval::ComputeFn`] for the morph e2e: captures
/// `realization_inputs` (the body's projected `RealizationReadHandle`) into
/// [`MORPH_PROBE_CAPTURED`], then returns `Completed`. Purity-preserving — only
/// reads the handed slice. Mirrors
/// `volume_mesh_realization_e2e.rs::vm_probe_capture_fn`.
#[cfg(has_gmsh)]
fn morph_probe_capture_fn(
    _value_inputs: &[reify_ir::Value],
    realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &reify_ir::Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    MORPH_PROBE_CAPTURED.with(|slot| {
        *slot.borrow_mut() = realization_inputs.to_vec();
    });
    reify_eval::ComputeOutcome::Completed {
        result: reify_ir::Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

/// Read back the most-recently-captured body handle's `tet_indices`, asserting
/// the probe actually captured a VolumeMesh (clear failure rather than an
/// index-out-of-bounds panic if the redispatch did not fire).
#[cfg(has_gmsh)]
fn captured_tet_indices(stage: &str) -> Vec<u32> {
    MORPH_PROBE_CAPTURED.with(|slot| {
        let captured = slot.borrow();
        assert!(
            !captured.is_empty(),
            "{stage}: the redispatch must invoke the probe with the body's \
             RealizationReadHandle; captured nothing"
        );
        captured[0]
            .volume_mesh()
            .unwrap_or_else(|| {
                panic!(
                    "{stage}: the captured body handle's volume_mesh() must be \
                     Some — the VolumeMesh-demand → execute → project → read path \
                     must deliver a tet mesh, not a None-content (BRep-only) handle"
                )
            })
            .tet_indices
            .clone()
    })
}

/// `cfg(has_gmsh)`: a NON-structural parameter tick morphs the prior mesh onto
/// the new BRep, preserving connectivity, and records exactly one `morphed`.
///
/// 1. Cold `build()` → from-scratch source VolumeMesh (remesh; the attributed
///    path is forced on when a morph producer is registered, so it carries a
///    `BoundaryAssociation`). Captured via the probe.
/// 2. `edit_param(width, 10.5mm)` — a topology-preserving scale (the box keeps
///    its face/edge/vertex counts → morph_eligible yields a full bijection).
/// 3. Warm `build_snapshot()` → the morph-or-remesh arm probes the stashed
///    source, builds a MorphRequest over the new OCCT kernel, and the installed
///    producer morphs the prior mesh IN PLACE (Laplacian quick-pass for the tiny
///    displacement) — same `tet_indices`, deformed vertices.
///
/// Asserts the terminal `volume_mesh().tet_indices` are IDENTICAL across the
/// tick (connectivity preserved — the defining property of a morph) AND
/// `reify_mesh_morph::diagnostics::snapshot().morphed == 1` (the morph_stats RPC
/// data source).
///
/// Gated `#[ignore]` on #4876: the morph arm + source-bundle stash are wired
/// (step-20), but producing the boundary-carrying source requires the 4092
/// attributed gmsh producer, which SIGSEGVs on real OCCT surfaces (#4876). The
/// morph logic is otherwise validated by the reify-mesh-morph + reify-eval unit
/// tests; this end-to-end assertion un-gates when #4876 hardens the producer.
#[cfg(has_gmsh)]
#[test]
#[ignore = "blocked on #4876 — the morph source needs a BoundaryAssociation, \
            which only the task-4092 gmsh attributed producer \
            (mesh_surface_to_volume_attributed) threads; that producer SIGSEGVs in \
            tetgen boundary recovery (recoveredgebyflips → hxt_boundary_recovery) \
            on real OCCT-tessellated surfaces — the same crash gating the sibling \
            fea_face_selector_bc_e2e::boundary_demand_realization_edge_produces_nonempty_boundary. \
            A SIGSEGV cannot be caught by the dispatch's honest degradation, so this \
            real-OCCT morph e2e is gated until #4876 hardens the producer (weld the \
            surface watertight / return Err). The morph arm itself is fully wired \
            (engine_build.rs dispatch + source-bundle stash) and validated by the \
            reify-mesh-morph compose_morph/register_morph_producer unit tests and \
            the reify-eval morph_producer decision-helper tests, none of which need \
            the crashing producer."]
fn e2e_non_structural_tick_morphs_and_preserves_connectivity() {
    use reify_core::ValueCellId;
    use reify_ir::{ExportFormat, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping e2e_non_structural_tick_morphs_and_preserves_connectivity: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    // Process-global morph counters: reset so `morphed == 1` is exact. nextest
    // runs each test in its own process, so this is isolated.
    reify_mesh_morph::diagnostics::reset_for_test();

    let compiled =
        reify_test_support::parse_and_compile_with_stdlib(include_str!("fixtures/morph_box.ri"));

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        morph_probe_capture_fn as reify_eval::ComputeFn,
    );
    // Boundary demand (⊇ VolumeMesh demand): the source mesh must carry a
    // BoundaryAssociation for the morph to project boundary nodes onto the new
    // BRep. Only the 4092 attributed path threads one — gated on #4876 (see the
    // #[ignore] above). Plain VolumeMesh demand would leave boundary == None and
    // honestly degrade to remesh (morphed would stay 0).
    engine.register_volume_mesh_boundary_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );
    reify_mesh_morph::register_morph_producer(&mut engine);

    // Defensive clear against thread reuse.
    MORPH_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    // (1) Cold build → from-scratch source VolumeMesh. `build()` establishes
    //     the eval_state snapshot internally (the redispatch + a later
    //     `edit_param` both require it), so no separate `eval()` is needed — and
    //     a separate `eval()` would finalize the probe cell, leaving the body's
    //     compute node with non-empty realization_inputs so `build()`'s
    //     redispatch skips it (capturing nothing).
    engine.build(&compiled, ExportFormat::Step);
    let source_tets = captured_tet_indices("first build (source)");
    assert!(
        !source_tets.is_empty() && source_tets.len() % 4 == 0,
        "source must be a valid P1 tet mesh (len % 4 == 0, > 0); got {} indices",
        source_tets.len()
    );
    assert_eq!(
        reify_mesh_morph::diagnostics::snapshot().morphed,
        0,
        "the first (from-scratch) build must not record a morph — no prior source"
    );

    // (2) Non-structural tick: width 10mm → 10.5mm (topology-preserving scale;
    //     a 0.25mm per-face displacement, comfortably within the Laplacian
    //     quick-pass cutover for a 10mm box).
    engine
        .edit_param(ValueCellId::new("MorphBox", "width"), Value::length(0.0105))
        .expect("edit_param must succeed against the MorphBox.width Length param");

    // (3) Warm rebuild → the morph arm fires.
    engine.build_snapshot(&compiled, ExportFormat::Step);
    let morphed_tets = captured_tet_indices("warm rebuild (morphed)");

    // Connectivity preserved: identical tet_indices is the defining property of
    // a morph (vertices deformed in place; the topology is reused, not rebuilt).
    assert_eq!(
        morphed_tets, source_tets,
        "a non-structural tick must MORPH (reuse connectivity) — the terminal \
         tet_indices must be identical to the source, not a from-scratch remesh"
    );
    // Exactly one successful morph recorded (the morph_stats RPC data source).
    assert_eq!(
        reify_mesh_morph::diagnostics::snapshot().morphed,
        1,
        "the non-structural tick must record exactly one morphed outcome"
    );
}

/// `cfg(not(has_gmsh))`: skip-stub (no gmsh adapter → no tet remesh source).
#[cfg(not(has_gmsh))]
#[test]
fn morph_arm_e2e_skipped_without_gmsh() {
    eprintln!(
        "skipping morph-arm e2e: has_gmsh cfg not set (stub-mode build); the morph \
         source requires the gmsh tet remesh path"
    );
}
