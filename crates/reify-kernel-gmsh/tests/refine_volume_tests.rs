//! Integration tests for [`reify_kernel_gmsh::refine_volume_with_size_field`].
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::{MeshingOptions, refine_volume_with_size_field};
use reify_ir::{ElementOrderTag, Mesh};

/// Inline copy of `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:19-48`.
///
/// Duplicated rather than dev-dep'ing on `reify-kernel-manifold` to avoid an
/// awkward layering — gmsh would otherwise dev-depend on manifold solely for
/// this 30-line fixture. When B-rep test fixtures consolidate into a shared
/// crate, this helper can move there.
fn unit_cube_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            0.0_f32, 0.0, 0.0, // 0
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
            // -Z bottom (outward = -Z, CW from +Z view)
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

/// A uniform size field smaller than the baseline produces a mesh with
/// strictly more tetrahedra.
///
/// Baseline: unit cube meshed at target size 0.5 (via `GmshKernel::mesh_to_volume`).
/// Refinement: call `refine_volume_with_size_field` with every surface vertex
/// assigned size 0.25 (half the baseline target). The refined volume mesh must
/// have strictly more P1 tets than the baseline, and `element_order` must echo
/// the requested `ElementOrderTag::P1`.
///
/// Fails RED because the `cfg(has_gmsh)` arm of `refine_volume_with_size_field`
/// currently returns a placeholder `OperationFailed` error — step-6 replaces
/// this with the real FFI-backed remesh implementation.
#[test]
fn uniform_smaller_size_field_produces_more_tets() {
    use reify_kernel_gmsh::GmshKernel;

    let cube = unit_cube_mesh();
    let kernel = GmshKernel::new();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    // Establish the baseline mesh.
    let vm_baseline = kernel
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .expect("baseline mesh_to_volume must succeed");

    let n_base_tets = vm_baseline.tet_indices.len() / 4;
    assert!(n_base_tets > 0, "baseline must have at least one tet");

    let n_surface_verts = cube.vertices.len() / 3;
    assert!(
        n_surface_verts > 0,
        "unit cube must have at least one surface vertex"
    );

    // Uniform 0.25 per-vertex hint: half the baseline target.
    let vertex_sizes = vec![0.25_f64; n_surface_verts];

    let result = refine_volume_with_size_field(&cube, &vertex_sizes, &opts, ElementOrderTag::P1);
    let vm_refined = result.expect(
        "refine_volume_with_size_field must succeed for a unit cube with uniform hints",
    );

    assert_eq!(
        vm_refined.element_order,
        ElementOrderTag::P1,
        "element_order must echo the requested ElementOrderTag::P1",
    );
    assert_eq!(
        vm_refined.tet_indices.len() % 4,
        0,
        "P1 tet_indices.len() must be divisible by 4, got {}",
        vm_refined.tet_indices.len(),
    );

    let n_refined_tets = vm_refined.tet_indices.len() / 4;
    assert!(
        n_refined_tets > n_base_tets,
        "uniform 0.25 size field must produce strictly more tets than baseline 0.5: \
         baseline={n_base_tets}, refined={n_refined_tets}",
    );
}
