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

/// Serializes every test in this binary that touches the **process-global**
/// `reify_mesh_morph::diagnostics` counters (`crates/reify-mesh-morph/src/diagnostics.rs`
/// `COUNTERS`).
///
/// The verify gate runs this binary under nextest, which forks a process per
/// test, so the counters are naturally isolated there. A plain
/// `cargo test -p reify-eval --test morph_arm_e2e` does NOT: it runs the tests
/// as THREADS in one process, where one test's `reset_for_test()` can zero the
/// counters between another's rebuild and its `snapshot()` — a spurious failure
/// that looks like a morph-arm regression. Task 6635 un-ignored a second
/// counter-touching test, which is what made that latent hazard reachable.
///
/// Acquired via [`lock_and_reset_morph_diagnostics`] at the top of each such
/// test and held (as a `let _diag_guard` binding) for the whole body, through
/// the final `snapshot()` assertion.
#[cfg(has_gmsh)]
static MORPH_DIAG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take [`MORPH_DIAG_LOCK`], then reset the process-global morph counters.
///
/// Returns the guard: bind it (`let _diag_guard = ...`, NOT `let _ = ...`, which
/// would drop it immediately) so the lock is held for the whole test body.
///
/// A test that panics while holding the lock poisons it; recover the inner guard
/// rather than letting a poison panic cascade into the sibling tests and mask
/// the original failure.
#[cfg(has_gmsh)]
#[must_use = "bind the guard (let _diag_guard = ...) so the lock is held for the whole test"]
fn lock_and_reset_morph_diagnostics() -> std::sync::MutexGuard<'static, ()> {
    let guard = MORPH_DIAG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reify_mesh_morph::diagnostics::reset_for_test();
    guard
}

// Per-thread capture slot for `morph_probe_capture_fn`. Each cargo test runs on
// its own thread; the e2e clears it at entry for defensiveness against reuse.
#[cfg(has_gmsh)]
thread_local! {
    static MORPH_PROBE_CAPTURED: std::cell::RefCell<Vec<reify_eval::RealizationReadHandle>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

// How many times `morph_probe_capture_fn` has been invoked on this thread.
// Same per-thread lifetime as `MORPH_PROBE_CAPTURED`, and read as a DELTA across
// a build so a stale count cannot leak between phases. This is what makes "the
// probe did not re-fire" an assertable fact rather than an inference from an
// unchanged capture — the slot is written with `=`, so a build that never
// dispatches the node leaves the PREVIOUS build's handle in place and reads back
// identically to a genuine re-capture.
#[cfg(has_gmsh)]
thread_local! {
    static MORPH_PROBE_INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    MORPH_PROBE_INVOCATIONS.with(|n| n.set(n.get() + 1));
    reify_eval::ComputeOutcome::Completed {
        result: reify_ir::Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
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
            .tet_indices()
            .expect("morph arm operates on tet-only meshes")
            .to_vec()
    })
}

/// `cfg(has_gmsh)`: a NON-structural parameter tick runs the morph arm to a
/// successful `morphed` outcome.
///
/// 1. Cold `build()` → from-scratch source VolumeMesh (remesh; the attributed
///    path is forced on when a morph producer is registered, so it carries a
///    `BoundaryAssociation`). Captured via the probe.
/// 2. `edit_param(width, 10.5mm)` — a topology-preserving scale (the box keeps
///    its face/edge/vertex counts → morph_eligible yields a full bijection).
/// 3. Warm `build_snapshot()` → the morph-or-remesh arm probes the stashed
///    source, builds a MorphRequest over the new OCCT kernel, and the installed
///    producer morphs the prior mesh IN PLACE (Laplacian quick-pass for the tiny
///    displacement).
///
/// # What this test asserts, and what it deliberately does NOT (#7332)
///
/// It asserts `reify_mesh_morph::diagnostics::snapshot().morphed == 1` — the
/// morph_stats RPC data source, and the arm's actual end-to-end signal.
///
/// It does NOT compare the pre-tick and post-tick `tet_indices`, because on the
/// `build_snapshot()` path THE CONSUMER IS NEVER RE-DISPATCHED, so there is no
/// post-tick capture to compare against. `redispatch_geometry_consuming_compute_nodes`'
/// Phase-1 candidate filter (`engine_build.rs:10022`) admits only compute nodes
/// whose `realization_inputs` is still empty, and its own doc calls that "a
/// ONE-SHOT LATCH" (`engine_build.rs:10234`). `build_snapshot()` reuses
/// `eval_state.snapshot.graph` verbatim (`engine_build.rs:3443`) and never calls
/// `eval()`, so the latch — set during the cold build's redispatch — closes the
/// only dispatch surface this path has. That is task **#7332**.
///
/// MEASURED 2026-09-08 (release, real OCCT + gmsh): the probe fires exactly
/// twice, BOTH inside the cold `build()` (the original `Undef`-bodied dispatch
/// plus its redispatch), and ZERO times during the warm `build_snapshot()`,
/// while `morphed` reaches 1. A `tet_indices` comparison across the tick would
/// therefore read the SAME thread-local slot twice and compare the cold capture
/// against itself — true exactly when the cold capture succeeded, and blind to
/// every property it names. That assertion was deleted rather than kept green.
///
/// The invocation-count pin below records the latch as MEASURED STATE. When
/// #7332 lands it goes RED, and the correct response is to TIGHTEN it back into
/// a real pre-morph-vs-post-morph connectivity comparison — not to widen it.
#[cfg(has_gmsh)]
#[test]
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

    // Process-global morph counters: take the file-local lock and reset, so
    // `morphed == 1` is exact even under a thread-per-test `cargo test` run.
    // See MORPH_DIAG_LOCK. Held for the whole body.
    let _diag_guard = lock_and_reset_morph_diagnostics();

    let compiled =
        reify_test_support::parse_and_compile_with_stdlib(include_str!("fixtures/morph_box.ri"));

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        morph_probe_capture_fn as reify_eval::ComputeFn,
    );
    // Boundary demand (⊇ VolumeMesh demand): the source mesh must carry a
    // BoundaryAssociation for the morph to project boundary nodes onto the new
    // BRep. Only the 4092 attributed path threads one. Plain VolumeMesh demand
    // would leave boundary == None and honestly degrade to remesh (morphed would
    // stay 0) — so this registration is what lets the morph solve run at all.
    // The solve DOES run today and reaches `morphed == 1` (measured 2026-09-08);
    // an earlier quality-gate hard-fail on this fixture is no longer reproducible.
    engine.register_volume_mesh_boundary_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );
    reify_mesh_morph::register_morph_producer(&mut engine);

    // Defensive clear against thread reuse.
    MORPH_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());
    MORPH_PROBE_INVOCATIONS.with(|n| n.set(0));

    // (1) Cold build → from-scratch source VolumeMesh. `build()` establishes
    //     the eval_state snapshot internally (the redispatch + a later
    //     `edit_param` both require it), so no separate `eval()` is needed — and
    //     a separate `eval()` would finalize the probe cell, leaving the body's
    //     compute node with non-empty realization_inputs so `build()`'s
    //     redispatch skips it (capturing nothing).
    engine.build(&compiled, ExportFormat::Step);
    let fires_after_cold = MORPH_PROBE_INVOCATIONS.with(|n| n.get());
    let source_tets = captured_tet_indices("first build (source)");
    assert!(
        !source_tets.is_empty() && source_tets.len().is_multiple_of(4),
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
    let fires_across_tick = MORPH_PROBE_INVOCATIONS.with(|n| n.get()) - fires_after_cold;

    // ── The #7332 latch pin ────────────────────────────────────────────────
    //
    // Premise first: the cold build MUST have dispatched the probe, or the
    // delta below is trivially 0 and pins nothing. Two fires — the original
    // `Undef`-bodied dispatch, then the post-hydration redispatch.
    assert_eq!(
        fires_after_cold, 2,
        "premise: the cold build() must dispatch the probe twice (original \
         Undef-bodied dispatch + post-hydration redispatch), else the \
         zero-refire pin below is vacuous"
    );

    // MEASURED STATE, NOT A DESIRED CONTRACT. The one-shot Phase-1 candidate
    // latch (engine_build.rs:10022/:10234) means build_snapshot() — which
    // reuses eval_state.snapshot.graph verbatim and never calls eval() — has no
    // remaining dispatch surface for this node. So the probe cannot observe the
    // morphed mesh, and the pre/post `tet_indices` comparison this test used to
    // make read one thread-local slot twice.
    //
    // WHEN #7332 LANDS THIS GOES RED. Tighten it back into a genuine
    // pre-morph-vs-post-morph connectivity comparison (`source_tets` is still
    // captured above for exactly that purpose) — do NOT widen it to accept a
    // re-fire, and do NOT delete it.
    assert_eq!(
        fires_across_tick, 0,
        "#7332: the redispatch candidate gate is a one-shot latch, so the warm \
         build_snapshot() must not re-invoke the geometry-consuming @optimized \
         consumer. A non-zero count here means #7332 landed — restore the \
         connectivity assertion (see this test's doc comment)"
    );
    // Exactly one successful morph recorded (the morph_stats RPC data source).
    // Snapshot-bound so a failure names the BUCKET, not just `left: 0, right: 1`.
    // This blind spot is why the task-6635 Stage A bug survived: the bare form
    // made a Stage-A over-reject, a Stage-B reject and a quality-gate reject
    // indistinguishable from the failure output, so diagnosis required
    // re-instrumenting the test by hand.
    let snap = reify_mesh_morph::diagnostics::snapshot();
    assert_eq!(
        snap.morphed, 1,
        "the non-structural tick must record exactly one morphed outcome; \
         snapshot: {snap:?}"
    );
}

/// `cfg(has_gmsh)`: FOUNDATION (step-21) — with NO morph producer registered, a
/// VolumeMesh-demanded build still produces a valid from-scratch tet remesh and
/// the morph arm stays fully dormant (`morphed == 0`). This is the honest
/// fallback floor the morph-or-remesh decision rests on (PRD §4.4-3): the morph
/// arm must NEVER break the remesh path.
///
/// Uses PLAIN `register_volume_mesh_demand` (not boundary demand), so it routes
/// through the non-attributed `mesh_surface_to_volume` path — it is therefore
/// #4876-INDEPENDENT (the SIGSEGV only afflicts the *attributed* producer) and
/// runs LIVE in CI, unlike the two boundary-demanding morph e2es above/below.
#[cfg(has_gmsh)]
#[test]
fn e2e_no_producer_engine_remeshes_volume_mesh() {
    use reify_ir::ExportFormat;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping e2e_no_producer_engine_remeshes_volume_mesh: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    // Process-global morph counters — see MORPH_DIAG_LOCK. Held for the whole body.
    let _diag_guard = lock_and_reset_morph_diagnostics();

    let compiled =
        reify_test_support::parse_and_compile_with_stdlib(include_str!("fixtures/morph_box.ri"));

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        morph_probe_capture_fn as reify_eval::ComputeFn,
    );
    // PLAIN VolumeMesh demand (not boundary): routes through the non-attributed
    // `mesh_surface_to_volume` path, avoiding the #4876 attributed-producer crash.
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );
    // Deliberately NO `register_morph_producer` — the morph arm must stay dormant.

    MORPH_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    engine.build(&compiled, ExportFormat::Step);
    let tets = captured_tet_indices("no-producer build (remesh)");
    assert!(
        !tets.is_empty() && tets.len().is_multiple_of(4),
        "the no-producer build must remesh a valid P1 tet mesh (len % 4 == 0, > 0); \
         got {} indices",
        tets.len()
    );
    assert_eq!(
        reify_mesh_morph::diagnostics::snapshot().morphed,
        0,
        "with no morph producer registered, the morph arm must stay dormant — \
         no morph may be recorded"
    );
}

/// `cfg(has_gmsh)`: a STRUCTURAL tick (topology change) makes the prior mesh
/// morph-INELIGIBLE, so the arm honestly falls back to a from-scratch Gmsh
/// remesh — recording an `ineligible` bucket and leaving `morphed` unchanged.
///
/// Uses an INLINE `difference` fixture (a box minus a movable Z-cylinder cutter)
/// as the structural lever: at the default `cut_z` the cutter sits far above the
/// box (removes nothing → box topology); a tick to `5mm` centres it in the box
/// (a through-hole → face/edge/vertex counts change → `morph_eligible` returns
/// Ineligible). `parse_and_compile_with_stdlib` runs at TEST RUNTIME, so this
/// inline fixture imposes no compile-time cost on the test binary and keeps the
/// shared `morph_box.ri` a clean plain box for the sibling test above.
///
/// Runs LIVE (un-`#[ignore]`d by task 6635 — MEASURED passing, 16.2s). Post-6635
/// it rejects at **Stage B**, not Stage A: measured `ineligible_naming_error: 1`
/// with `ineligible_structural_change: 0`.
///
/// That is the outcome this test's own premise always intended — its docstring
/// above claims `morph_eligible` returns Ineligible *via a topology-count
/// change*, which is a Stage-B judgement. Before 6635 the test passed only
/// because Stage A's Rule 4 misclassified the derived `Type::Geometry` cell as
/// Structural and so over-rejected EVERY tick: it was green for the wrong
/// reason and proved nothing about Stage B. The
/// `ineligible_structural_change == 0` assertion in the body is what pins the
/// difference.
#[cfg(has_gmsh)]
#[test]
fn e2e_structural_tick_remeshes_and_records_ineligible() {
    use reify_core::ValueCellId;
    use reify_ir::{ExportFormat, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping e2e_structural_tick_remeshes_and_records_ineligible: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    // Process-global morph counters — see MORPH_DIAG_LOCK. Held for the whole body.
    let _diag_guard = lock_and_reset_morph_diagnostics();

    // Inline structural fixture: a box minus a movable Z-cylinder cutter. See the
    // doc comment — parsed at runtime, so it costs no compile time.
    const STRUCTURAL_FIXTURE: &str = r#"
@optimized("test::vm-demand-probe")
fn vm_probe(g: Geometry) -> Int {
    0
}

structure StructuralMorphBox {
    param width: Length = 10mm
    param depth: Length = 10mm
    param height: Length = 10mm
    // Structural lever: cut_z positions a tall Z-cylinder cutter. At 100mm the
    // cutter sits far above the 10mm box → difference removes nothing → plain
    // box topology. A tick to 5mm centres it in the box → through-hole → counts
    // change → morph Ineligible.
    param cut_z: Length = 100mm
    let tool = translate(cylinder(2mm, 50mm), 5mm, 5mm, cut_z)
    let body = difference(box(width, depth, height), tool)
    let probe = vm_probe(body)
}
"#;
    let compiled = reify_test_support::parse_and_compile_with_stdlib(STRUCTURAL_FIXTURE);

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "test::vm-demand-probe",
        morph_probe_capture_fn as reify_eval::ComputeFn,
    );
    // Boundary demand → the source carries a BoundaryAssociation (via the 4092
    // attributed path), which is what lets morph_eligible run far enough to
    // reach Stage B and report a reject rather than short-circuiting earlier.
    engine.register_volume_mesh_boundary_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );
    reify_mesh_morph::register_morph_producer(&mut engine);

    MORPH_PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    // (1) Cold build → from-scratch source VolumeMesh (box topology, cutter far away).
    engine.build(&compiled, ExportFormat::Step);
    let source_tets = captured_tet_indices("structural source build");
    assert!(
        !source_tets.is_empty() && source_tets.len().is_multiple_of(4),
        "source must be a valid P1 tet mesh (len % 4 == 0, > 0); got {} indices",
        source_tets.len()
    );

    // (2) Structural tick: move the cutter into the box → through-hole → topology change.
    engine
        .edit_param(
            ValueCellId::new("StructuralMorphBox", "cut_z"),
            Value::length(0.005),
        )
        .expect("edit_param must succeed against StructuralMorphBox.cut_z");

    // (3) Warm rebuild → the morph arm attempts a morph, finds the topology
    //     changed (Ineligible), records an ineligible bucket, and honestly
    //     remeshes from scratch.
    engine.build_snapshot(&compiled, ExportFormat::Step);
    let remeshed_tets = captured_tet_indices("structural rebuild (remesh)");
    assert!(
        !remeshed_tets.is_empty() && remeshed_tets.len().is_multiple_of(4),
        "the structural tick must still yield a valid remeshed VolumeMesh \
         (len % 4 == 0, > 0); got {} indices",
        remeshed_tets.len()
    );

    let snap = reify_mesh_morph::diagnostics::snapshot();
    // Task 6635 regression lock. This is the e2e-level RED→GREEN for the Stage A
    // classifier fix: MEASURED `ineligible_structural_change: 1` before the fix
    // and `0` after, in this exact test.
    assert_eq!(
        snap.ineligible_structural_change, 0,
        "task 6635: Stage A must NOT veto a dimensional-leaf tick — the cut_z \
         Length tick must reach Stage B, which is what rejects it on the topology \
         change; a non-zero Stage-A bucket means the derived Type::Geometry cell \
         is vetoing again; snapshot: {snap:?}"
    );
    // …and the reject must land in the exact Stage-B bucket that was MEASURED,
    // not in an OR over the Stage-B family. An `a + b >= 1` assertion would stay
    // green in the degenerate world where Stage B errors on EVERY tick — the
    // morph arm dormant again, one stage later, which is precisely the failure
    // mode task 6635 set out to remove. Pinning each bucket exactly means drift
    // in either direction fails here and has to be looked at.
    //
    // HONEST READING of the measured state: `ineligible_naming_error: 1` says
    // Stage B could not EVALUATE the bijection at all — it is NOT a detection of
    // this fixture's through-hole topology change. `NamingLayerErrorReason` is
    // only `Imported` (no attributes on either side) or `Partial` (some handles
    // attributed, some not); see `reify-eval/src/morph_stage_b.rs`. So today's
    // reject is an ATTRIBUTION GAP: the persistent-naming layer does not
    // attribute the boolean-cut B-rep this fixture produces. What this test
    // therefore proves is the Stage A half (the assertion above) plus "the
    // reject moved downstream", no more.
    //
    // When that attribution gap closes (morph-arm family, live task #6637), this
    // pair must TIGHTEN to `ineligible_bijection_failure == 1` /
    // `ineligible_naming_error == 0` — Stage B rejecting because it measured the
    // face/edge/vertex `BijectionFailure::CountMismatch`. Until then, the
    // Stage-A-admits/Stage-B-rejects-on-a-real-count-mismatch composition is
    // demonstrated in-crate by
    // `reify-mesh-morph/src/eligibility.rs`'s
    // `morph_eligible_stage_a_admits_geometry_diff_stage_b_rejects_count_mismatch`.
    assert_eq!(
        snap.ineligible_naming_error, 1,
        "a structural (topology-changing) tick must be rejected by STAGE B, and \
         the MEASURED bucket for this fixture is the naming-layer one (Stage B \
         cannot attribute the boolean-cut B-rep, so it cannot evaluate the \
         bijection). If this now reads 0 with ineligible_bijection_failure == 1, \
         the attribution gap closed — tighten this pair rather than widening it; \
         snapshot: {snap:?}"
    );
    assert_eq!(
        snap.ineligible_bijection_failure, 0,
        "pinned to the MEASURED state: Stage B never reaches the bijection \
         comparison on this fixture today, so a non-zero count here is a change \
         in Stage B's behaviour that must be reviewed, not absorbed; \
         snapshot: {snap:?}"
    );
    assert_eq!(
        snap.morphed, 0,
        "a structural tick must NOT record a successful morph (it is ineligible → remesh)"
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
