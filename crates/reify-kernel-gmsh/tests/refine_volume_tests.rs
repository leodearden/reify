//! Integration tests for [`reify_kernel_gmsh::refine_volume_with_size_field`].
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::{MeshingOptions, ffi, init, refine_volume_with_size_field};
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

/// Poison the process-global gmsh mesh-size clamp to `size`, reproducing the
/// state a sibling entry point leaves behind.
///
/// gmsh's option table is process-global and is **not** reset by `gmshClear()`,
/// so `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` written by one call survive into
/// every later call in the same process. Acquires `GMSH_LOCK` for the duration
/// of the two writes and releases it before returning, so the subsequent
/// `refine_volume_with_size_field` call can take the lock itself.
fn poison_global_mesh_size_clamp(size: f64) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::option_set_number("Mesh.MeshSizeMin", size).expect("poison MeshSizeMin");
    ffi::option_set_number("Mesh.MeshSizeMax", size).expect("poison MeshSizeMax");
}

/// A finer uniform size field produces strictly more tets **even when the
/// process-global mesh-size clamp has been poisoned by an earlier call**.
///
/// gmsh's option table is process-global and survives `gmshClear()`, and three
/// sibling entry points set `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` and never
/// restore them: `kernel_real.rs:178-179` (`mesh_to_volume`),
/// `mesh_profile_2d.rs:93-94` (`mesh_plane_2d`) and `mesh_boundary.rs:614-615`.
/// Any of those running earlier in the same process pins every element of a
/// later `refine_volume_with_size_field` remesh to *its* size, making the
/// per-vertex `vertex_sizes` hints completely inert (task #6211).
///
/// This test reproduces that leak **deterministically** — it writes the clamp
/// itself via `ffi::option_set_number` rather than depending on which sibling
/// test happened to run first in the binary. That makes it independent of the
/// `mesh_to_volume` pipeline and of sibling task #6200's classify-angle change,
/// and is why it — not the cross-pipeline
/// `uniform_smaller_size_field_produces_more_tets` below — is the primary guard
/// that a smaller size field actually produces a finer mesh.
///
/// The clamp is re-poisoned before *every* call, because the fixed
/// implementation sets these two options on entry; a single up-front poison
/// would only exercise the first hint. It is poisoned to 0.5 — the same value
/// every other test in this binary uses — so the test is immune to parallel
/// interleaving inside the test binary.
///
/// Asserts relative refinement (a strictly increasing chain of tet counts)
/// rather than pinning absolute counts, following the house convention in
/// `mesh_to_volume_tests.rs::mesh_size_override_increases_tet_count`.
#[test]
fn uniform_size_field_refines_monotonically_under_leaked_global_clamp() {
    const HINTS: [f64; 3] = [0.5, 0.25, 0.125];

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };
    let n_surface_verts = cube.vertices.len() / 3;

    let mut tet_counts: Vec<usize> = Vec::with_capacity(HINTS.len());
    for hint in HINTS {
        // Re-establish the leaked state before each remesh (see docstring).
        poison_global_mesh_size_clamp(0.5);

        let vm = refine_volume_with_size_field(
            &cube,
            &vec![hint; n_surface_verts],
            &opts,
            ElementOrderTag::P1,
        )
        .unwrap_or_else(|e| {
            panic!("refine_volume_with_size_field must succeed for uniform hint {hint}: {e:?}")
        });
        tet_counts.push(vm.tet_indices().expect("P1 tet mesh").len() / 4);
    }

    for w in 1..HINTS.len() {
        assert!(
            tet_counts[w] > tet_counts[w - 1],
            "a finer uniform size field must produce strictly more tets: \
             hint {} -> {} tets, hint {} -> {} tets (tet_counts={tet_counts:?}); \
             equal counts mean the per-vertex size field was clamped away by the \
             leaked process-global Mesh.MeshSizeMin/Max (task #6211)",
            HINTS[w - 1],
            tet_counts[w - 1],
            HINTS[w],
            tet_counts[w],
        );
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

    let n_base_tets = vm_baseline.tet_indices().expect("P1 tet mesh must have tet_indices").len() / 4;
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
