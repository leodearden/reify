//! End-to-end mesh-to-volume tests for the real `GmshKernel::mesh_to_volume`.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::{GmshKernel, MeshingOptions};
use reify_ir::{ElementOrderTag, GeometryHandleId, GeometryKernel, QueryError};
use reify_test_support::fixtures::unit_cube_mesh;
use reify_kernel_gmsh::mesh_size_scope::GMSH_SIZE_OPTION_DEFAULTS;
use reify_kernel_gmsh::{ffi, init};

/// Round-trip a unit cube (8 vertices, 12 outward-winding triangles)
/// through `mesh_to_volume` with the default options + P1 element order.
///
/// Asserts the basic structural invariants of the returned `VolumeMesh`:
/// - tet_indices length is divisible by 4 (P1 = 4 nodes/element).
/// - tet count > 0 (the meshing actually produced something).
/// - vertex count is divisible by 3 (flat XYZ stride).
/// - every vertex sits inside `[-1e-3, 1+1e-3]³` (small slack for
///   boundary-extracted nodes).
/// - element_order matches the requested `ElementOrderTag::P1`.
#[test]
fn cube_surface_produces_nonempty_p1_tet_mesh() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    let result = kernel.mesh_to_volume(&cube, &MeshingOptions::default(), ElementOrderTag::P1);
    let vm = result.expect("mesh_to_volume must succeed for a closed unit-cube surface");

    assert_eq!(
        vm.element_order(),
        Some(ElementOrderTag::P1),
        "element_order must echo the requested ElementOrderTag::P1",
    );
    let vm_tet_indices = vm.tet_indices().expect("P1 tet mesh must have tet_indices");
    assert_eq!(
        vm_tet_indices.len() % 4,
        0,
        "P1 tets carry 4 nodes/element; tet_indices.len() = {} is not divisible by 4",
        vm_tet_indices.len(),
    );
    assert!(
        vm_tet_indices.len() / 4 > 0,
        "expected at least one tet from a closed unit cube; tet_indices.len() = {}",
        vm_tet_indices.len(),
    );
    assert_eq!(
        vm.vertices.len() % 3,
        0,
        "VolumeMesh.vertices is flat XYZ; len() = {} is not divisible by 3",
        vm.vertices.len(),
    );

    let eps = 1e-3_f32;
    for (i, xyz) in vm.vertices.chunks_exact(3).enumerate() {
        for (k, &component) in xyz.iter().enumerate() {
            assert!(
                component >= -eps && component <= 1.0 + eps,
                "vertex {i} component {k} = {component} is outside [-{eps}, 1+{eps}]",
            );
        }
    }

    // Connectivity bounds: every tet index must address a real vertex.
    // A regression in the gmsh-tag → 0-based-idx remap could push indices
    // past the end of `vertices` and the tests above would still pass
    // (counts/divisibility/bbox don't witness it) — assert it explicitly.
    let n_local_verts = vm.vertices.len() / 3;
    assert!(
        vm_tet_indices.iter().all(|&i| (i as usize) < n_local_verts),
        "tet_indices contains an out-of-range index for a {n_local_verts}-vertex mesh; \
         max idx = {:?}",
        vm_tet_indices.iter().max(),
    );
}

/// γ store/retrieve round-trip: a `VolumeMesh` produced by `mesh_to_volume`
/// can be stored in the kernel via `store_volume_mesh` and read back by handle
/// through the `GeometryKernel::volume_mesh` accessor, preserving its
/// structural invariants (multiple-of-4 P1 connectivity, >0 tets,
/// `element_order == P1`).
///
/// This is the retrieval half of the realization-read VolumeMesh projection
/// arm: γ provides `store_volume_mesh` (the population seam) + `volume_mesh`
/// (handle-addressable retrieval); the production dispatch that actually calls
/// `store_volume_mesh` at realize-time is downstream task 3429.
#[test]
fn volume_mesh_store_round_trips_produced_tet_mesh() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    let produced = kernel
        .mesh_to_volume(&cube, &MeshingOptions::default(), ElementOrderTag::P1)
        .expect("mesh_to_volume must succeed for a closed unit-cube surface");

    let handle = kernel.store_volume_mesh(produced);
    let vm = kernel
        .volume_mesh(handle)
        .expect("volume_mesh(handle) must return the stored VolumeMesh");

    assert_eq!(
        vm.element_order(),
        Some(ElementOrderTag::P1),
        "stored element_order must round-trip as P1",
    );
    let vm_tet_indices = vm.tet_indices().expect("P1 tet mesh must have tet_indices");
    assert_eq!(
        vm_tet_indices.len() % 4,
        0,
        "P1 tets carry 4 nodes/element; tet_indices.len() = {} is not divisible by 4",
        vm_tet_indices.len(),
    );
    assert!(
        vm_tet_indices.len() / 4 > 0,
        "expected at least one tet in the stored mesh; tet_indices.len() = {}",
        vm_tet_indices.len(),
    );
}

/// Task 4743 (VolumeMesh realization α): the same produce→store→read-back
/// round-trip, but driven through the **`GeometryKernel` trait methods** the
/// engine's execute call edge uses — `mesh_surface_to_volume` (produce a tet
/// VolumeMesh value) and the trait `store_volume_mesh` (returns
/// `Result<GeometryHandleId, _>`), as opposed to the inherent
/// `mesh_to_volume` / `store_volume_mesh` exercised above.
///
/// The call MUST go through a `&dyn GeometryKernel` binding: on a concrete
/// `GmshKernel` the inherent `store_volume_mesh` (returning a bare
/// `GeometryHandleId`) shadows the trait method, so only the trait object
/// resolves to the new Result-returning trait method the engine consumes.
///
/// Structural assertions only (multiple-of-4 P1 connectivity, >0 tets,
/// `element_order == P1`, equal round-trip) — no numeric-accuracy bound.
#[test]
fn trait_mesh_surface_to_volume_then_store_round_trips_through_dyn_kernel() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    let kernel_ref: &dyn GeometryKernel = &kernel;

    // (1) Produce a tet VolumeMesh VALUE via the trait method.
    let produced = kernel_ref
        .mesh_surface_to_volume(&cube, ElementOrderTag::P1)
        .expect("trait mesh_surface_to_volume must succeed for a closed unit-cube surface");
    assert_eq!(
        produced.element_order(),
        Some(ElementOrderTag::P1),
        "produced element_order must echo the requested ElementOrderTag::P1",
    );
    let produced_tet_indices = produced.tet_indices().expect("P1 tet mesh must have tet_indices");
    assert_eq!(
        produced_tet_indices.len() % 4,
        0,
        "P1 tets carry 4 nodes/element; tet_indices.len() = {} is not divisible by 4",
        produced_tet_indices.len(),
    );
    assert!(
        produced_tet_indices.len() / 4 > 0,
        "expected at least one tet from a closed unit cube; tet_indices.len() = {}",
        produced_tet_indices.len(),
    );

    // (2) Store it via the trait method → handle.
    let produced_clone = produced.clone();
    let handle = kernel_ref
        .store_volume_mesh(produced)
        .expect("trait store_volume_mesh must return Ok(handle)");

    // (3) Read it back by handle → equal VolumeMesh (round-trip).
    let read_back = kernel_ref
        .volume_mesh(handle)
        .expect("volume_mesh(handle) must return the stored VolumeMesh");
    assert_eq!(
        read_back.element_order(),
        Some(ElementOrderTag::P1),
        "stored element_order must round-trip as P1",
    );
    assert_eq!(
        read_back.tet_indices(), produced_clone.tet_indices(),
        "tet_indices must round-trip equal through trait store→volume_mesh",
    );
    assert_eq!(
        read_back.vertices, produced_clone.vertices,
        "vertices must round-trip equal through trait store→volume_mesh",
    );
}

/// A handle that was never stored → `Err(QueryError::InvalidHandle(_))`.
///
/// The store-backed `volume_mesh` accessor must reject unknown handles with
/// the structured `InvalidHandle` variant (not a stringly-typed `QueryFailed`)
/// so the projection site can distinguish "kernel can't project THIS handle"
/// from "kernel doesn't support volume meshes at all" (the default-Err path).
#[test]
fn volume_mesh_unknown_handle_is_invalid_handle_err() {
    let kernel = GmshKernel::new();
    let bogus = GeometryHandleId(987_654_321);
    let result = kernel.volume_mesh(bogus);
    assert!(
        matches!(result, Err(QueryError::InvalidHandle(_))),
        "a never-stored handle must yield Err(QueryError::InvalidHandle(_)), got: {result:?}",
    );
}

/// Pin that an explicit `MeshingOptions.threads` override propagates through
/// `mesh_to_volume` without erroring.
///
/// Doesn't assert a specific thread count is honoured by HXT — that's not
/// observable from the API surface. Only proves the option round-trips
/// (i.e. the `Some(t) => t as f64` arm of the match in
/// `kernel_real::mesh_to_volume` still wires `General.NumThreads`). A
/// regression that drops that arm would be silently masked on most CI
/// machines by the `available_parallelism` fallback.
#[test]
fn threads_override_succeeds() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    let opts = MeshingOptions {
        threads: Some(2),
        ..Default::default()
    };
    let vm = kernel
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .expect("threads=Some(2) mesh_to_volume must succeed");
    assert!(
        vm.tet_indices().expect("P1 mesh must have tet_indices").len() / 4 > 0,
        "threads=Some(2) must still produce tets; tet count = {}",
        vm.tet_indices().expect("P1 mesh must have tet_indices").len() / 4,
    );
}

/// Pin that an explicit `mesh_size` override produces a strictly finer mesh
/// than the default options.
///
/// With a unit cube (1.0 m edges), the auto-derived default `mesh_size` is
/// `1.0` (the smallest triangle edge), giving a coarse mesh. Forcing
/// `mesh_size = 0.25` quarters the target edge length, which under HXT
/// produces strictly more tets.
///
/// This test fails if `kernel_real::mesh_to_volume` ignores
/// `MeshingOptions.mesh_size` (i.e. does not propagate it to
/// `gmshOptionSetNumber("Mesh.MeshSizeMin/Max", ...)`).
#[test]
fn mesh_size_override_increases_tet_count() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();

    let vm_default = kernel
        .mesh_to_volume(&cube, &MeshingOptions::default(), ElementOrderTag::P1)
        .expect("default-options mesh_to_volume must succeed");
    let n_default = vm_default
        .tet_indices()
        .expect("P1 mesh must have tet_indices")
        .len()
        / 4;

    let override_options = MeshingOptions {
        mesh_size: Some(0.25),
        ..Default::default()
    };
    let vm_fine = kernel
        .mesh_to_volume(&cube, &override_options, ElementOrderTag::P1)
        .expect("mesh_size=0.25 override mesh_to_volume must succeed");
    let n_fine = vm_fine
        .tet_indices()
        .expect("P1 mesh must have tet_indices")
        .len()
        / 4;

    assert!(
        n_fine > n_default,
        "expected mesh_size=0.25 to produce strictly more tets than the default; \
         got n_default={n_default}, n_fine={n_fine}",
    );
}

/// Pin that `ElementOrderTag::P2` produces 10-node tetrahedra (stride 10
/// in the flat `tet_indices` array).
///
/// Gmsh's element type 11 is a 10-node second-order tet (4 corner + 6
/// edge-midpoint nodes). Requesting `P2` must:
///  - set `Mesh.ElementOrder = 2` BEFORE `mesh_generate(3)` so HXT emits
///    P2 tets in the first place;
///  - read elements via `get_elements_by_type(11)` instead of `4`;
///  - tag the returned `VolumeMesh.element_order` as `P2`.
///
/// This test fails if any of those three steps is missing.
#[test]
fn p2_element_order_produces_stride_10_tet_indices() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();

    let vm = kernel
        .mesh_to_volume(&cube, &MeshingOptions::default(), ElementOrderTag::P2)
        .expect("P2 mesh_to_volume must succeed for a closed unit cube");

    assert_eq!(
        vm.element_order(),
        Some(ElementOrderTag::P2),
        "element_order must echo the requested ElementOrderTag::P2",
    );
    let vm_tet_indices = vm.tet_indices().expect("P2 tet mesh must have tet_indices");
    assert_eq!(
        vm_tet_indices.len() % 10,
        0,
        "P2 tets carry 10 nodes/element; tet_indices.len() = {} is not divisible by 10",
        vm_tet_indices.len(),
    );
    assert!(
        vm_tet_indices.len() / 10 > 0,
        "expected at least one P2 tet from a closed unit cube; tet_indices.len() = {}",
        vm_tet_indices.len(),
    );
}

/// Pin that `deterministic = true` (which sets `General.NumThreads = 1`)
/// does not fail the meshing call.
///
/// Doesn't assert bit-exact reproducibility — that's the job of the
/// downstream cache-key + replay layer (sibling task #2926). This test
/// only proves the option propagates without erroring out HXT's threading
/// configuration.
///
/// This test fails if `kernel_real::mesh_to_volume` ignores
/// `MeshingOptions.deterministic` and the resulting `General.NumThreads`
/// value happens to be invalid (it currently isn't, but the assertion pins
/// the contract for future drift).
#[test]
fn deterministic_threads_one_succeeds() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();

    let det_options = MeshingOptions {
        deterministic: true,
        ..Default::default()
    };
    let vm = kernel
        .mesh_to_volume(&cube, &det_options, ElementOrderTag::P1)
        .expect("deterministic=true mesh_to_volume must succeed");
    assert!(
        vm.tet_indices().expect("P1 mesh must have tet_indices").len() / 4 > 0,
        "deterministic=true must still produce tets; tet count = {}",
        vm.tet_indices().expect("P1 mesh must have tet_indices").len() / 4,
    );
}

/// Done-criterion #2: two back-to-back calls on the same surface mesh
/// produce tet counts within a bounded macro-regression budget (or within
/// ±1 tet for very coarse meshes).
///
/// Runs in `deterministic = true` mode, which sets `General.NumThreads = 1`
/// and removes the dominant source of HXT run-to-run drift (thread-
/// scheduling-dependent insertion order). Under single-thread HXT the
/// counts should be exactly reproducible — a tight ±1% budget is the
/// strongest assertion we can make without claiming bit-exactness, and
/// it has real regression-detection power (anything bigger than rounding-
/// scale noise surfaces a real change). An earlier multi-threaded
/// formulation of this test had to relax to ±10% to chase intrinsic
/// drift; that's too loose to catch the regressions this assertion is
/// meant to catch.
///
/// The `|n1 - n2| <= 1` short-circuit handles the low-count noise floor —
/// at ~12 tets a 1-tet drift is already 8%, which is intrinsic mesher
/// discretisation noise, not the kind of macro regression the budget
/// guards against.
///
/// Uses `mesh_size = 0.25` (rather than the default ~1.0 for the unit
/// cube) so the resulting count is in the 100s and the percentage budget
/// becomes statistically meaningful. If this test fails, that surfaces
/// a >1% macro-regression that warrants investigation.
#[test]
fn cuboid_round_trip_within_count_variation_budget() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();

    // Finer mesh_size moves the absolute count above the noise floor so
    // the percentage budget is meaningful. With size=0.25 on a unit cube
    // we get ~100s of tets per run. `deterministic = true` forces
    // single-threaded HXT, eliminating the thread-scheduling drift that
    // forced the previous multi-threaded variant of this test up to a
    // ±10% budget.
    let opts = MeshingOptions {
        mesh_size: Some(0.25),
        deterministic: true,
        ..Default::default()
    };
    let vm1 = kernel
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .expect("first cube mesh_to_volume must succeed");
    let vm2 = kernel
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .expect("second cube mesh_to_volume must succeed");

    let n1 = vm1.tet_indices().expect("P1 mesh must have tet_indices").len() / 4;
    let n2 = vm2.tet_indices().expect("P1 mesh must have tet_indices").len() / 4;
    assert!(n1 > 0, "first call produced no tets (n1 = {n1})");
    assert!(n2 > 0, "second call produced no tets (n2 = {n2})");

    // 1% budget under single-thread HXT: counts should be exactly
    // reproducible run-to-run, but we leave a hair of slack so a future
    // gmsh point-release that tweaks insertion-order tie-breaking
    // doesn't immediately flake the suite.
    const MAX_DRIFT: f64 = 0.01;
    let abs_diff = n1.abs_diff(n2);
    let max_n = n1.max(n2) as f64;
    let drift = abs_diff as f64 / max_n;
    let within_budget = drift <= MAX_DRIFT || abs_diff <= 1;
    assert!(
        within_budget,
        "cuboid mesh count drift exceeds the ±{:.0}% budget: \
         n1={n1}, n2={n2}, abs_diff={abs_diff}, drift={drift:.3}",
        MAX_DRIFT * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Input-validation guards (preflight at the FFI boundary)
// ---------------------------------------------------------------------------
//
// `mesh_to_volume` validates its input mesh before acquiring the gmsh lock so
// silent floor-divides (`vertices.len() / 3`, `indices.len() / 3`) don't
// discard trailing data and feed gmsh a partially-malformed buffer, and so
// out-of-bounds indices fail with a precise diagnostic rather than an opaque
// gmsh internal error. The tests below pin those guards: a regression that
// removes any of them, or swaps a modulus, would surface here.

/// Vertices.len() not divisible by 3 → caller-side error before any FFI work.
#[test]
fn vertices_length_not_multiple_of_three_errors() {
    let mut bad = unit_cube_mesh();
    bad.vertices.truncate(7); // 7 floats — not a flat XYZ stride.
    let kernel = GmshKernel::new();
    let result = kernel.mesh_to_volume(&bad, &MeshingOptions::default(), ElementOrderTag::P1);
    let err = result.expect_err("vertices.len()=7 must error before any FFI work");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("vertices") && msg.contains("3"),
        "error message should mention vertices stride; got: {msg}"
    );
}

/// Indices.len() not divisible by 3 → caller-side error before any FFI work.
#[test]
fn indices_length_not_multiple_of_three_errors() {
    let bad = reify_ir::Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1], // 2 indices — not a triangle stride.
        normals: None,
    };
    let kernel = GmshKernel::new();
    let result = kernel.mesh_to_volume(&bad, &MeshingOptions::default(), ElementOrderTag::P1);
    let err = result.expect_err("indices.len()=2 must error before any FFI work");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("indices") && msg.contains("3"),
        "error message should mention indices triangle stride; got: {msg}"
    );
}

/// Empty surface mesh → caller-side error before any FFI work. Gmsh accepts
/// empty input but produces a useless zero-tet result; failing fast keeps
/// the diagnostic close to the real cause.
#[test]
fn empty_surface_mesh_errors() {
    let bad = reify_ir::Mesh {
        vertices: vec![],
        indices: vec![],
        normals: None,
    };
    let kernel = GmshKernel::new();
    let result = kernel.mesh_to_volume(&bad, &MeshingOptions::default(), ElementOrderTag::P1);
    let err = result.expect_err("empty surface mesh must error before any FFI work");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("empty surface mesh"),
        "error message should mention empty surface mesh; got: {msg}"
    );
}

/// Index out-of-bounds for the supplied vertex buffer → caller-side error
/// before any FFI work.
#[test]
fn out_of_bounds_index_errors() {
    let bad = reify_ir::Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // 3 vertices
        indices: vec![0, 1, 99],                                     // 99 ≥ 3
        normals: None,
    };
    let kernel = GmshKernel::new();
    let result = kernel.mesh_to_volume(&bad, &MeshingOptions::default(), ElementOrderTag::P1);
    let err = result.expect_err("out-of-bounds index 99 must error before any FFI work");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("99") && msg.contains("out of bounds"),
        "error message should mention the out-of-bounds tag and phrasing; got: {msg}"
    );
}

/// The success-path half of "stop the capture on EVERY exit path".
///
/// MEASURED: one unit-cube `mesh_to_volume` emits 82 captured lines, so a
/// success path that left the capture armed would leave all 82 buffered for
/// the next caller in this process to report as its own — and this read
/// would find them. `logger_stop` drains, so empty is the witness that the
/// guard fired. The error path is covered by
/// `log_capture_tests::log_capture_guard_folds_captured_lines_into_the_error_and_stops_on_drop`
/// and, end to end, by
/// `mesher_poison_recovery::a_failed_mesh_to_volume_reports_gmshs_captured_log_not_just_the_last_error`
/// — which lives there because it needs a deliberate mesher failure, kept out
/// of this binary.
#[test]
fn mesh_to_volume_leaves_the_gmsh_logger_stopped() {
    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    kernel
        .mesh_to_volume(&cube, &MeshingOptions::default(), ElementOrderTag::P1)
        .expect("mesh_to_volume must succeed for a closed unit-cube surface");

    // `mesh_to_volume` released GMSH_LOCK on return, so this read is
    // serialised against any concurrent mesher in this binary rather than
    // racing one mid-flight.
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let leftover = ffi::logger_get().expect("ffi::logger_get failed");
    assert!(
        leftover.is_empty(),
        "mesh_to_volume must leave gmsh's capture stopped and drained; {} lines left: {leftover:?}",
        leftover.len(),
    );
}
// Coverage gap: the `surface_tags.is_empty()` branch in
// `kernel_real::mesh_to_volume` is not reachable from real input geometry.
// gmsh's classify_surfaces+create_geometry produces a surface entity even for
// an open mesh, so the obvious candidate — a single open triangle — sails
// past that branch and fails later, at `gmshModelMeshGenerate(3)`, when HXT
// cannot 3D-mesh an unclosed region. The branch stays as defensive guarding
// against future gmsh-version changes, verified by code review rather than
// runtime coverage. The three sibling validation tests above
// (`vertices_length_not_multiple_of_three_errors`,
// `indices_length_not_multiple_of_three_errors`, `out_of_bounds_index_errors`)
// cover the preflight validation that does have testable error paths.
//
// Deliberate mesher failures live in `tests/mesher_poison_recovery.rs`, whose
// header carries the mechanism and why they are kept out of this binary.

/// `mesh_to_volume` neither inherits nor leaks a mesh-size process-global,
/// whatever the option table held when it was called.
///
/// The outbound half was closed for the `Mesh.MeshSizeMin`/`MeshSizeMax` pair
/// by task #6298. What #6968 adds here is the other three options and the
/// inbound direction: before it, `mesh_to_volume` never wrote the size-SOURCE
/// trio at all, so a poisoned `Mesh.MeshSizeExtendFromBoundary` was restored by
/// nothing and passed straight through to the next caller in the process.
///
/// Both halves in one test on purpose. A poisoned table that comes back clean
/// proves the outbound direction; the SAME poisoned table producing the same
/// tet count as an unpoisoned run proves the inbound one. Splitting them would
/// let the inbound assertion run from a table the outbound assertion had
/// already cleaned.
///
/// # Measured RED, and what each leg is worth
///
/// With `MeshSizeScope::entered` commented out of `kernel_real::mesh_to_volume`
/// — unit cube, `deterministic: true`, P1, poison as below:
///
/// ```text
/// leg                                 armed    disarmed
/// tet count, from a defaults table      186         186
/// tet count, from a poisoned table      186         141   <- RED
/// table read, MeshSizeMin                 0           1   <- RED
/// table read, MeshSizeMax              1e22           1   <- RED
/// table read, FromPoints                  1           0   <- RED
/// table read, FromCurvature               0          20   <- RED
/// table read, ExtendFromBoundary          1           0   <- RED
/// ```
///
/// Both legs bite, and they bite for different reasons. The three trio rows
/// read back EXACTLY the poison they were handed: `mesh_to_volume` never wrote
/// those options, so it carried a sibling's leak through untouched — the
/// specific hole #6298 left open and #6968 closes. The clamp rows read `1`
/// rather than the poison because the function writes `Min == Max ==
/// resolved_size` itself, and `resolved_size` is this cube's extent; pre-#6968
/// those two rows were already clean, restored by #6298's guard, so the three
/// trio rows are what this task actually adds.
///
/// The tet-count leg is a genuine detector, not a lock-in: 141 against 186 is
/// a 24% drop, driven by the poisoned `MeshSizeFromPoints = 0` — which a shut
/// `Min == Max` clamp does NOT mask, unlike `ExtendFromBoundary`.
///
/// Note the two 186s in the first row. This producer's output from a clean
/// table is byte-identical armed and disarmed, which is the measurement behind
/// the claim that closing the inbound hole moves nothing downstream and lets
/// `reify-solver-elastic`'s calibrated constants stay untouched.
///
/// # Why `deterministic: true`
///
/// Not decoration. `MeshingOptions::default()` leaves `deterministic` false,
/// which lets gmsh run HXT on `available_parallelism()` threads, and the tet
/// count is then not reproducible: measured 185 / 184 / 184 across three
/// consecutive calls on identical input, against a flat 186 / 186 / 186 with
/// `deterministic: true`. An exact-equality tet assertion under the default
/// options is a coin flip, and this test held one until the disarm measurement
/// above exposed it.
///
/// Needs no whole-body serialising mutex, for the reason
/// [`mesh_to_volume_leaves_the_gmsh_logger_stopped`] gives: the asserted
/// property is one every sibling in this binary also leaves behind, so an
/// interleaving sibling cannot flip the result. Adding `CLAMP_TEST_ORDER` here
/// would serialise thirteen unrelated `mesh_to_volume` calls as a side effect.
#[test]
fn mesh_to_volume_enters_and_leaves_gmshs_size_defaults_whatever_the_table_held() {
    /// Distinctive, and far finer than the cube's extent, so a leak into the
    /// mesher would be loud rather than marginal.
    const POISON_SIZE: f64 = 0.05;

    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    // `deterministic: true` is load-bearing here, not decoration — see the doc
    // comment's "Why `deterministic: true`".
    let options = MeshingOptions { deterministic: true, ..Default::default() };
    let tets = || {
        kernel
            .mesh_to_volume(&cube, &options, ElementOrderTag::P1)
            .expect("mesh_to_volume must succeed for a closed unit-cube surface")
            .tet_indices()
            .expect("P1 tet mesh")
            .len()
            / 4
    };
    let write_size_options = |value_of: &dyn Fn(&str, f64) -> f64| {
        let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init::ensure_initialized();
        for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
            ffi::option_set_number(option, value_of(option, default))
                .unwrap_or_else(|e| panic!("ffi::option_set_number({option}) failed: {e:?}"));
        }
    };

    write_size_options(&|_, default| default);
    let from_defaults = tets();
    assert!(from_defaults > 0, "mesh_to_volume must produce tets");

    // Every size option away from its default: a fine shut clamp, plus the
    // three size-SOURCE options flipped.
    write_size_options(&|option, default| match option {
        "Mesh.MeshSizeMin" | "Mesh.MeshSizeMax" => POISON_SIZE,
        "Mesh.MeshSizeFromCurvature" => 20.0,
        _ => 1.0 - default,
    });
    let from_poisoned = tets();

    {
        // `mesh_to_volume` released GMSH_LOCK on return, so this read is
        // serialised against any concurrent mesher rather than racing one.
        let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
            let observed = ffi::option_get_number(option)
                .unwrap_or_else(|e| panic!("ffi::option_get_number({option}) failed: {e:?}"));
            assert_eq!(
                observed, default,
                "mesh_to_volume must leave every mesh-size process-global at gmsh's default \
                 on exit, whatever it was handed: {option} reads {observed}, expected \
                 {default}. gmsh's option table survives gmshClear(), so mesh_to_volume \
                 passing a poisoned option straight through makes it a silent CARRIER of \
                 another entry point's leak — task #6968, enforced by `MeshSizeScope` in \
                 kernel_real.rs",
            );
        }
    }

    assert_eq!(
        from_poisoned, from_defaults,
        "mesh_to_volume must mesh against gmsh's size defaults, not against whatever a \
         sibling entry point left in the process-global table: the same call gave \
         {from_defaults} tets from a defaults table and {from_poisoned} from a fully \
         poisoned one. This is the inbound direction of task #6968, closed by \
         `MeshSizeScope::entered` in kernel_real.rs — which matters most on the \
         resolved_size == 0.0 path, where mesh_to_volume writes no clamp of its own and \
         used to inherit the table wholesale",
    );
}
