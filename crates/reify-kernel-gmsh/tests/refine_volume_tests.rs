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
/// Baseline: unit cube refined with every surface vertex assigned size 0.5.
/// Refinement: the same call with every vertex assigned 0.25 (half the
/// baseline). The refined volume mesh must have strictly more P1 tets than the
/// baseline, and `element_order` must echo the requested `ElementOrderTag::P1`.
///
/// # Why the baseline is `refine_volume_with_size_field`, not `mesh_to_volume`
///
/// It used to be `GmshKernel::mesh_to_volume(mesh_size = 0.5)`. That control
/// was invalid in two ways, and #6200 exposed it.
///
/// 1. It compared two *different* producers. They do not share sizing
///    semantics: `mesh_to_volume` applies a global target, while this function
///    sets per-corner sizes with `Mesh.MeshSizeFromPoints=1` and lets gmsh
///    interpolate. Measured on this cube at the same nominal 0.25, the two
///    disagree by ~2x (mesh_to_volume 382 tets, refine 176), so no inequality
///    between them pins a property of *this* function.
/// 2. The inequality it asserted was an artefact of a bug. Before #6200
///    `mesh_to_volume` passed `classify_surfaces` a 90 deg feature angle and
///    tetrahedralized only part of the solid, so its baseline was tiny: 91 tets
///    at mesh_size 0.5 (aabb fill 0.862916). With the angle fixed the same call
///    returns 194 tets (fill 1.000000) and the assertion inverts against an
///    unchanged refine result. The test was, in effect, pinned to the defect.
///
/// Refining against this function's own coarser output tests the property the
/// name claims — a smaller field yields a denser mesh — with the cross-producer
/// confound removed. Measured: 141 tets at 0.5, 176 at 0.25, both aabb fill
/// 1.000000. This path was never affected by #6200 (it has always classified at
/// PI/12, well below the 90 deg threshold that broke `mesh_to_volume`).
#[test]
fn uniform_smaller_size_field_produces_more_tets() {
    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    let n_surface_verts = cube.vertices.len() / 3;
    assert!(
        n_surface_verts > 0,
        "unit cube must have at least one surface vertex"
    );

    // Establish the baseline mesh: same producer, uniform 0.5 hint.
    let baseline_sizes = vec![0.5_f64; n_surface_verts];
    let vm_baseline =
        refine_volume_with_size_field(&cube, &baseline_sizes, &opts, ElementOrderTag::P1)
            .expect("baseline refine_volume_with_size_field must succeed");

    let n_base_tets = vm_baseline.tet_indices().expect("P1 tet mesh must have tet_indices").len() / 4;
    assert!(n_base_tets > 0, "baseline must have at least one tet");

    // Uniform 0.25 per-vertex hint: half the baseline hint.
    let vertex_sizes = vec![0.25_f64; n_surface_verts];

    let result = refine_volume_with_size_field(&cube, &vertex_sizes, &opts, ElementOrderTag::P1);
    let vm_refined = result.expect(
        "refine_volume_with_size_field must succeed for a unit cube with uniform hints",
    );

    assert_eq!(
        vm_refined.element_order(),
        Some(ElementOrderTag::P1),
        "element_order must echo the requested ElementOrderTag::P1",
    );
    let vm_refined_tet_indices = vm_refined.tet_indices().expect("P1 tet mesh must have tet_indices");
    assert_eq!(
        vm_refined_tet_indices.len() % 4,
        0,
        "P1 tet_indices.len() must be divisible by 4, got {}",
        vm_refined_tet_indices.len(),
    );

    let n_refined_tets = vm_refined_tet_indices.len() / 4;
    assert!(
        n_refined_tets > n_base_tets,
        "uniform 0.25 size field must produce strictly more tets than baseline 0.5: \
         baseline={n_base_tets}, refined={n_refined_tets}",
    );
}
