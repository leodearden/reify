//! End-to-end mesh-to-volume tests for the real `GmshKernel::mesh_to_volume`.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::{GmshKernel, MeshingOptions};
use reify_ir::{ElementOrderTag, Mesh};

/// Inline copy of `crates/reify-kernel-manifold/src/test_fixtures.rs:37-67`.
///
/// Duplicated rather than dev-dep'ing on `reify-kernel-manifold` to avoid an
/// awkward layering — gmsh would otherwise dev-depend on manifold solely for
/// this 30-line fixture. When B-rep test fixtures consolidate into a shared
/// crate, this helper can move there.
fn unit_cube_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            1.0, 1.0, 0.0, // 2
            0.0, 1.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            1.0, 0.0, 1.0, // 5
            1.0, 1.0, 1.0, // 6
            0.0, 1.0, 1.0, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            // -Z bottom (outward = -Z, so CW from +Z view)
            0, 2, 1,  0, 3, 2,
            // +Z top
            4, 5, 6,  4, 6, 7,
            // -Y front
            0, 1, 5,  0, 5, 4,
            // +Y back
            3, 7, 6,  3, 6, 2,
            // -X left
            0, 4, 7,  0, 7, 3,
            // +X right
            1, 2, 6,  1, 6, 5,
        ],
        normals: None,
    }
}

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
        vm.element_order,
        ElementOrderTag::P1,
        "element_order must echo the requested ElementOrderTag::P1",
    );
    assert_eq!(
        vm.tet_indices.len() % 4,
        0,
        "P1 tets carry 4 nodes/element; tet_indices.len() = {} is not divisible by 4",
        vm.tet_indices.len(),
    );
    assert!(
        vm.tet_indices.len() / 4 > 0,
        "expected at least one tet from a closed unit cube; tet_indices.len() = {}",
        vm.tet_indices.len(),
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
        vm.tet_indices.iter().all(|&i| (i as usize) < n_local_verts),
        "tet_indices contains an out-of-range index for a {n_local_verts}-vertex mesh; \
         max idx = {:?}",
        vm.tet_indices.iter().max(),
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
        vm.tet_indices.len() / 4 > 0,
        "threads=Some(2) must still produce tets; tet count = {}",
        vm.tet_indices.len() / 4,
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
    let n_default = vm_default.tet_indices.len() / 4;

    let override_options = MeshingOptions {
        mesh_size: Some(0.25),
        ..Default::default()
    };
    let vm_fine = kernel
        .mesh_to_volume(&cube, &override_options, ElementOrderTag::P1)
        .expect("mesh_size=0.25 override mesh_to_volume must succeed");
    let n_fine = vm_fine.tet_indices.len() / 4;

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
        vm.element_order,
        ElementOrderTag::P2,
        "element_order must echo the requested ElementOrderTag::P2",
    );
    assert_eq!(
        vm.tet_indices.len() % 10,
        0,
        "P2 tets carry 10 nodes/element; tet_indices.len() = {} is not divisible by 10",
        vm.tet_indices.len(),
    );
    assert!(
        vm.tet_indices.len() / 10 > 0,
        "expected at least one P2 tet from a closed unit cube; tet_indices.len() = {}",
        vm.tet_indices.len(),
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
        vm.tet_indices.len() / 4 > 0,
        "deterministic=true must still produce tets; tet count = {}",
        vm.tet_indices.len() / 4,
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

    let n1 = vm1.tet_indices.len() / 4;
    let n2 = vm2.tet_indices.len() / 4;
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

// Coverage gap: the `surface_tags.is_empty()` branch in
// `kernel_real::mesh_to_volume` (post-classify_surfaces +
// post-create_geometry) is intentionally not exercised by an integration
// test. Empirical investigation showed that the obvious candidate input —
// a single open triangle — does NOT hit that branch: gmsh's
// classify_surfaces+create_geometry produces a surface entity even for an
// open mesh, and the failure surfaces later in `gmshModelMeshGenerate(3)`
// when HXT cannot 3D-mesh an unclosed region. Worse, an HXT mesh_generate
// failure leaves thread-local HXT state that survives `gmshClear()` and
// corrupts the *next* meshing call's output (it returns 0 tets instead
// of erroring). So an integration test that reliably hits the
// empty-entities branch isn't reachable from real input geometry, and a
// test that triggers HXT failure pollutes other tests in the same binary.
// The branch remains as defensive guarding against future gmsh-version
// changes; verification relies on code review rather than runtime
// coverage. The other three reviewer-requested validation tests
// (`vertices_length_not_multiple_of_three_errors`,
// `indices_length_not_multiple_of_three_errors`,
// `out_of_bounds_index_errors`) cover the preflight validation that does
// have testable error paths.
