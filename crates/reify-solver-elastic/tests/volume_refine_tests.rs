//! Integration tests for `reify_solver_elastic::volume_refine`.
//!
//! Tests that don't require libgmsh run unconditionally. Tests that require
//! a real Gmsh remesh guard on `reify_kernel_gmsh::GMSH_AVAILABLE` at runtime
//! (mirroring the convention in `mesh_swept_profile_2d_tests.rs`; the
//! `reify-solver-elastic` crate has no build.rs that propagates `has_gmsh`).

use reify_kernel_gmsh::MeshingOptions;
use reify_solver_elastic::volume_refine::{RefineError, refine_with_size_field};
use reify_ir::{ElementOrderTag, Mesh, VolumeMesh};

// ---------------------------------------------------------------------------
// Test fixture helpers
// ---------------------------------------------------------------------------

/// Minimal closed-surface unit cube (8 vertices, 12 outward-winding triangles).
///
/// Inline copy of `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:19-48`.
/// Duplicated rather than dev-dep'ing on `reify-kernel-manifold` to avoid an
/// awkward layering. When B-rep test fixtures consolidate into a shared crate,
/// this helper can move there.
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

// ---------------------------------------------------------------------------
// step-3: refine_with_size_field validation tests
// ---------------------------------------------------------------------------

fn five_tet_p1_vm() -> VolumeMesh {
    // 5-tet P1 mesh with 6 vertices.
    VolumeMesh {
        vertices: vec![0.0_f32; 18], // 6 vertices × 3 coords
        tet_indices: vec![
            0, 1, 2, 3, // tet 0
            0, 1, 2, 4, // tet 1
            0, 1, 3, 4, // tet 2
            0, 2, 3, 4, // tet 3
            1, 2, 3, 4, // tet 4
        ],
        element_order: ElementOrderTag::P1,
        normals: None,
    }
}

fn three_tet_p1_vm() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![0.0_f32; 15], // 5 vertices × 3 coords
        tet_indices: vec![
            0, 1, 2, 3, // tet 0
            0, 1, 2, 4, // tet 1
            0, 1, 3, 4, // tet 2
        ],
        element_order: ElementOrderTag::P1,
        normals: None,
    }
}

fn dummy_surface() -> Mesh {
    Mesh {
        vertices: vec![0.0_f32; 9],
        indices: vec![0, 1, 2],
        normals: None,
    }
}

/// `size_hints` with wrong length must return `SizeHintsLengthMismatch`.
#[test]
fn size_hints_length_mismatch_errors() {
    let surface = dummy_surface();
    let vm = five_tet_p1_vm(); // 5 elements
    let size_hints = vec![1.0_f64; 4]; // 4 hints → mismatch
    let opts = MeshingOptions::default();

    let result = refine_with_size_field(&surface, &vm, &size_hints, &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::SizeHintsLengthMismatch { got: 4, expected: 5 })
        ),
        "expected SizeHintsLengthMismatch {{got: 4, expected: 5}}, got: {result:?}",
    );
}

/// Non-positive size hint must return `NonPositiveSize`.
#[test]
fn non_positive_size_errors() {
    let surface = dummy_surface();
    let vm = three_tet_p1_vm(); // 3 elements
    let size_hints = vec![1.0_f64, 0.0_f64, 0.5_f64];
    let opts = MeshingOptions::default();

    let result = refine_with_size_field(&surface, &vm, &size_hints, &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::NonPositiveSize { index: 1, size: s }) if s == 0.0
        ),
        "expected NonPositiveSize {{index: 1, size: 0.0}}, got: {result:?}",
    );
}

/// Non-finite (NaN) size hint must return `NonFiniteSize`.
#[test]
fn non_finite_size_errors() {
    let surface = dummy_surface();
    let vm = three_tet_p1_vm(); // 3 elements
    let size_hints = vec![1.0_f64, f64::NAN, 0.5_f64];
    let opts = MeshingOptions::default();

    let result = refine_with_size_field(&surface, &vm, &size_hints, &opts);
    assert!(
        matches!(result, Err(RefineError::NonFiniteSize { index: 1 })),
        "expected NonFiniteSize {{index: 1}}, got: {result:?}",
    );
}

// ---------------------------------------------------------------------------
// step-7: localized refinement integration test (runtime-gated on GMSH_AVAILABLE)
// ---------------------------------------------------------------------------

/// Localized size reduction refines only the marked (x < 0.5) region.
///
/// Baseline: unit cube meshed at size 0.5. Marked tets have centroid x < 0.5
/// and get size 0.125 (4× finer); unmarked tets keep size 0.5.
///
/// Skipped at runtime when libgmsh is not present (`GMSH_AVAILABLE = false`).
/// On stub builds, `mesh_to_volume` returns `GmshUnavailable` — this test
/// exits early before calling `refine_with_size_field`.
///
/// Assertions when gmsh IS available:
/// (a) `refine_with_size_field` returns `Ok`.
/// (b) Refined mesh has strictly more tets with centroid x < 0.5 than baseline.
/// (c) Average tet edge length in unmarked region (centroid x ≥ 0.5) is
///     within ±25% of baseline average (not over-refined; generous tolerance
///     for gmsh's spatial smoothing extent).
#[test]
fn localized_size_reduction_refines_marked_region_only() {
    if !reify_kernel_gmsh::GMSH_AVAILABLE {
        eprintln!("skipping: libgmsh not available in this build");
        return;
    }

    let cube = unit_cube_mesh();
    let kernel = reify_kernel_gmsh::GmshKernel::new();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    use reify_ir::ElementOrderTag;
    let vm_baseline = kernel
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .expect("baseline mesh_to_volume must succeed");

    let n_base_tets = vm_baseline.tet_indices.len() / 4;
    assert!(n_base_tets > 0, "baseline must have at least one tet");

    // Build per-element size hints: 4× finer in marked region (x < 0.5).
    let per_element_sizes: Vec<f64> = (0..n_base_tets)
        .map(|e| {
            let cx = tet_centroid_x(&vm_baseline, e);
            if cx < 0.5 { 0.125 } else { 0.5 }
        })
        .collect();

    let result = refine_with_size_field(&cube, &vm_baseline, &per_element_sizes, &opts);
    let vm_refined = result.expect("refine_with_size_field must return Ok");

    assert!(
        vm_refined.tet_indices.len() / 4 > 0,
        "refined mesh must have at least one tet"
    );

    // (b) More tets in marked region.
    let base_marked = count_tets_with_centroid_x_lt(&vm_baseline, 0.5);
    let refined_marked = count_tets_with_centroid_x_lt(&vm_refined, 0.5);
    assert!(
        refined_marked > base_marked,
        "marked region must have more tets after refinement: \
         baseline={base_marked}, refined={refined_marked}"
    );

    // (c) Unmarked region not over-refined (±25% tolerance).
    let base_avg = avg_tet_edge_in_region_x_ge(&vm_baseline, 0.5);
    let refined_avg = avg_tet_edge_in_region_x_ge(&vm_refined, 0.5);
    if base_avg > 0.0 && refined_avg > 0.0 {
        let ratio = refined_avg / base_avg;
        assert!(
            (0.75..=1.25).contains(&ratio),
            "unmarked region avg edge ratio {ratio:.3} is outside [0.75, 1.25] — \
             refine_with_size_field over-refines the unmarked region \
             (baseline avg={base_avg:.4}, refined avg={refined_avg:.4})"
        );
    }
}

// ---- geometry helpers ----

fn tet_centroid_x(vm: &VolumeMesh, elem_idx: usize) -> f64 {
    let base = elem_idx * 4;
    (0..4)
        .map(|k| vm.vertices[(vm.tet_indices[base + k] as usize) * 3] as f64)
        .sum::<f64>()
        / 4.0
}

fn count_tets_with_centroid_x_lt(vm: &VolumeMesh, threshold: f64) -> usize {
    let n = vm.tet_indices.len() / 4;
    (0..n).filter(|&e| tet_centroid_x(vm, e) < threshold).count()
}

fn avg_tet_edge_in_region_x_ge(vm: &VolumeMesh, threshold: f64) -> f64 {
    let n = vm.tet_indices.len() / 4;
    let mut total_edge = 0.0_f64;
    let mut count = 0usize;
    for e in 0..n {
        if tet_centroid_x(vm, e) < threshold {
            continue;
        }
        let base = e * 4;
        let verts: Vec<[f64; 3]> = (0..4)
            .map(|k| {
                let vi = vm.tet_indices[base + k] as usize;
                [
                    vm.vertices[vi * 3] as f64,
                    vm.vertices[vi * 3 + 1] as f64,
                    vm.vertices[vi * 3 + 2] as f64,
                ]
            })
            .collect();
        for i in 0..4 {
            for j in (i + 1)..4 {
                let dx = verts[i][0] - verts[j][0];
                let dy = verts[i][1] - verts[j][1];
                let dz = verts[i][2] - verts[j][2];
                total_edge += (dx * dx + dy * dy + dz * dz).sqrt();
                count += 1;
            }
        }
    }
    if count == 0 { 0.0 } else { total_edge / count as f64 }
}
