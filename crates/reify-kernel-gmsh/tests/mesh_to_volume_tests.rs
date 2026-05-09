//! End-to-end mesh-to-volume tests for the real `GmshKernel::mesh_to_volume`.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::{GmshKernel, MeshingOptions};
use reify_types::{ElementOrderTag, Mesh};

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
