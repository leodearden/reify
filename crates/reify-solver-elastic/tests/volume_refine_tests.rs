//! Integration tests for `reify_solver_elastic::volume_refine`.
//!
//! Tests that don't require libgmsh run unconditionally. Tests that require
//! a real Gmsh remesh guard on `reify_kernel_gmsh::GMSH_AVAILABLE` at runtime
//! (mirroring the convention in `mesh_swept_profile_2d_tests.rs`; the
//! `reify-solver-elastic` crate has no build.rs that propagates `has_gmsh`).

use reify_kernel_gmsh::MeshingOptions;
use reify_solver_elastic::refine_marked_elements;
use reify_solver_elastic::volume_refine::{RefineError, refine_with_size_field};
use reify_ir::{ElementOrderTag, Mesh, VolumeConnectivity, VolumeMesh};

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
        connectivity: VolumeConnectivity::Tet {
            indices: vec![
                0, 1, 2, 3, // tet 0
                0, 1, 2, 4, // tet 1
                0, 1, 3, 4, // tet 2
                0, 2, 3, 4, // tet 3
                1, 2, 3, 4, // tet 4
            ],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

fn three_tet_p1_vm() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![0.0_f32; 15], // 5 vertices × 3 coords
        connectivity: VolumeConnectivity::Tet {
            indices: vec![
                0, 1, 2, 3, // tet 0
                0, 1, 2, 4, // tet 1
                0, 1, 3, 4, // tet 2
            ],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// Coarse 6-tet (Kuhn) decomposition of the unit cube over its 8 corners.
///
/// Every tet shares the main diagonal 0→6, one per monotone lattice path from
/// (0,0,0) to (1,1,1); together they partition the cube exactly.
///
/// The minimal sizing mesh spanning the box: it carries a UNIFORM size field
/// into a remesh without invoking gmsh, and a uniform field needs no more
/// resolution than the 8 corners. A field that varies in space needs
/// `kuhn_lattice_unit_cube_vm`'s interior vertices instead — see
/// `localized_size_reduction_refines_marked_region_only`.
fn kuhn_6tet_unit_cube_vm() -> VolumeMesh {
    VolumeMesh {
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
        connectivity: VolumeConnectivity::Tet {
            indices: vec![
                0, 1, 2, 6, // x,y,z
                0, 1, 5, 6, // x,z,y
                0, 3, 2, 6, // y,x,z
                0, 3, 7, 6, // y,z,x
                0, 4, 5, 6, // z,x,y
                0, 4, 7, 6, // z,y,x
            ],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// Kuhn decomposition of the unit cube over an `n^3` lattice of cells —
/// `(n+1)^3` vertices, `6 * n^3` tets.
///
/// Each cell is cut into the 6 tets that share the cell's main diagonal, one
/// per permutation of the three axes; together they partition the cell exactly,
/// and adjacent cells match face-to-face because every cell is cut the same way.
///
/// Hand-built and gmsh-free for the two reasons `kuhn_6tet_unit_cube_vm` gives:
/// producer symmetry, so both sides of a comparison come from the same source,
/// and determinism under a gmsh version bump.
///
/// This generalises `kuhn_6tet_unit_cube_vm` but does NOT subsume it: that
/// fixture numbers each z-level as a CCW ring (`(0,0,0), (1,0,0), (1,1,0),
/// (0,1,0)`), while a lattice must number rasterwise, so `n = 1` here emits the
/// same six tets under a different index order. Both are kept rather than
/// silently changing the index order every existing assertion was measured
/// against.
///
/// `n >= 2` is what a mid-span size field needs: with no INTERIOR vertices the
/// min-projection in `project_per_element_sizes_to_vertices` has nowhere to put
/// an interior minimum, so an 8-vertex seed cannot represent one at all.
fn kuhn_lattice_unit_cube_vm(n: usize) -> VolumeMesh {
    assert!(n >= 1, "lattice needs at least one cell per axis");
    let side = n + 1;
    let vid = |i: usize, j: usize, k: usize| ((k * side + j) * side + i) as u32;

    let mut vertices = Vec::with_capacity(3 * side * side * side);
    for k in 0..side {
        for j in 0..side {
            for i in 0..side {
                vertices.push(i as f32 / n as f32);
                vertices.push(j as f32 / n as f32);
                vertices.push(k as f32 / n as f32);
            }
        }
    }

    // One tet per permutation (a, b, c) of the axes: walk from the cell's low
    // corner along a, then b, then c, landing on the opposite corner. The
    // shared main diagonal is the (low corner -> opposite corner) edge.
    const AXIS_ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];

    let mut indices = Vec::with_capacity(4 * 6 * n * n * n);
    for ck in 0..n {
        for cj in 0..n {
            for ci in 0..n {
                for order in AXIS_ORDERS {
                    let mut step = [0_usize; 3];
                    indices.push(vid(ci, cj, ck));
                    for axis in order {
                        step[axis] = 1;
                        indices.push(vid(ci + step[0], cj + step[1], ck + step[2]));
                    }
                }
            }
        }
    }

    VolumeMesh {
        vertices,
        connectivity: VolumeConnectivity::Tet {
            indices,
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

fn dummy_surface() -> Mesh {
    Mesh {
        vertices: vec![0.0_f32; 9],
        indices: vec![0, 1, 2],
        normals: None,
    }
}

/// Hex8 `VolumeMesh` (8 vertices, P1-only) — non-tetrahedral connectivity
/// used to exercise the tet-only connectivity guard (task 4996).
fn hex_vm() -> VolumeMesh {
    VolumeMesh {
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
        connectivity: VolumeConnectivity::Hex {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7],
        },
        normals: None,
        boundary: None,
    }
}

/// Wedge/PRI6 `VolumeMesh` (6 vertices, P1-only) — non-tetrahedral
/// connectivity used to exercise the tet-only connectivity guard (task 4996).
fn wedge_vm() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            0.0, 1.0, 0.0, // 2
            0.0, 0.0, 1.0, // 3
            1.0, 0.0, 1.0, // 4
            0.0, 1.0, 1.0, // 5
        ],
        connectivity: VolumeConnectivity::Wedge {
            indices: vec![0, 1, 2, 3, 4, 5],
        },
        normals: None,
        boundary: None,
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

/// A Hex `VolumeMesh` passed to `refine_with_size_field` (tet-only) must be
/// rejected via `RefineError::UnsupportedConnectivity` from the
/// `tet_shape` guard, before any size-hint validation or gmsh call
/// (task 4996).
#[test]
fn refine_with_size_field_errors_on_hex_connectivity() {
    let surface = dummy_surface();
    let vm = hex_vm();
    let opts = MeshingOptions::default();

    // size_hints length is irrelevant here: the connectivity guard fires
    // before the length check.
    let result = refine_with_size_field(&surface, &vm, &[], &opts);
    assert!(
        matches!(result, Err(RefineError::UnsupportedConnectivity)),
        "expected UnsupportedConnectivity, got: {result:?}",
    );
}

/// A Wedge `VolumeMesh` passed to `refine_marked_elements` (tet-only) must be
/// rejected via `RefineError::UnsupportedConnectivity` from the shared
/// `tet_shape` chokepoint, before any size-hint/marked-index validation
/// or gmsh call (task 4996).
#[test]
fn refine_marked_elements_errors_on_wedge_connectivity() {
    let surface = dummy_surface();
    let vm = wedge_vm();
    let opts = MeshingOptions::default();

    let result = refine_marked_elements(&surface, &vm, &[], &[], &opts);
    assert!(
        matches!(result, Err(RefineError::UnsupportedConnectivity)),
        "expected UnsupportedConnectivity, got: {result:?}",
    );
}

/// One-element P2 tet `VolumeMesh` (10 nodes, stride 10) — the only fixture in
/// this crate's suites that exercises the non-P1 branch of
/// `VolumeMesh::nodes_per_element()`.
///
/// Node positions are irrelevant to `tet_shape`, which reads only the
/// index-buffer length and the order tag; they are laid out as the 4 corners
/// followed by the 6 edge midpoints so the fixture reads as a real P2 tet.
fn one_p2_tet_vm() -> VolumeMesh {
    VolumeMesh {
        #[rustfmt::skip]
        vertices: vec![
            // 4 corners
            0.0_f32, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
            // 6 edge midpoints
            0.5, 0.0, 0.0,
            0.5, 0.5, 0.0,
            0.0, 0.5, 0.0,
            0.0, 0.0, 0.5,
            0.5, 0.0, 0.5,
            0.0, 0.5, 0.5,
        ],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            order: ElementOrderTag::P2,
        },
        normals: None,
        boundary: None,
    }
}

/// Stride regression pin: `tet_shape` must divide a P2 tet index buffer by
/// 10, not 4.
///
/// Every other fixture in this crate's suites is P1 (stride 4), so without
/// this test the P2 branch of `VolumeMesh::nodes_per_element()` — adopted when
/// the crate-local `nodes_per_element(order)` helper was deleted — is
/// unexercised here, and a stride regression would surface only as a silently
/// wrong element count. Asserting `expected: 1` (not `expected: 2`) on the
/// length-mismatch report pins the divisor: 10 indices / 10 nodes = 1 element.
#[test]
fn tet_shape_divides_p2_tet_indices_by_ten() {
    let surface = dummy_surface();
    let vm = one_p2_tet_vm(); // 10 indices, P2 → exactly 1 element
    let size_hints = vec![1.0_f64; 3]; // deliberately wrong length
    let opts = MeshingOptions::default();

    let result = refine_with_size_field(&surface, &vm, &size_hints, &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::SizeHintsLengthMismatch { got: 3, expected: 1 })
        ),
        "expected SizeHintsLengthMismatch {{got: 3, expected: 1}} (10 indices / \
         10 nodes per P2 tet = 1 element; a stride-4 divisor would report 2), \
         got: {result:?}",
    );
}

/// Tet index buffer whose length is not a whole multiple of the per-element
/// node count — 5 indices at P1 stride 4 — must be rejected at the
/// `tet_shape` chokepoint.
///
/// Before the divisibility guard, the truncating division reported 1 element,
/// so `size_hints` of length 1 cleared the length check and
/// `project_per_element_sizes_to_vertices` then panicked with an
/// index-out-of-bounds on the trailing remainder chunk emitted by
/// `chunks(4)`. This pins the structured error in place of that panic.
#[test]
fn refine_with_size_field_errors_on_non_multiple_tet_indices() {
    let surface = dummy_surface();
    let vm = VolumeMesh {
        vertices: vec![0.0_f32; 15], // 5 vertices × 3 coords
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 3, 4], // 5 indices, P1 stride 4 → not a multiple
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };
    let opts = MeshingOptions::default();

    // Length 1 is exactly what the old truncating count would have accepted.
    let result = refine_with_size_field(&surface, &vm, &[0.5_f64], &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::MalformedTetIndices { len: 5, stride: 4 })
        ),
        "expected MalformedTetIndices {{len: 5, stride: 4}} rather than a \
         downstream index-out-of-bounds panic, got: {result:?}",
    );
}

/// The same malformed buffer must be rejected through the `adaptive` entry
/// point too — both public entry points share the `tet_shape` chokepoint.
#[test]
fn refine_marked_elements_errors_on_non_multiple_tet_indices() {
    let surface = dummy_surface();
    let vm = VolumeMesh {
        vertices: vec![0.0_f32; 15],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 3, 4],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };
    let opts = MeshingOptions::default();

    let result = refine_marked_elements(&surface, &vm, &[0], &[0.5_f64], &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::MalformedTetIndices { len: 5, stride: 4 })
        ),
        "expected MalformedTetIndices {{len: 5, stride: 4}}, got: {result:?}",
    );
}

/// A correctly-SHAPED tet buffer carrying an out-of-range index VALUE must be
/// rejected at the same chokepoint.
///
/// The structural guards (connectivity family, length divisibility) pass here:
/// 4 indices at P1 stride 4 is exactly one element. Only the index *value* is
/// wrong — 99 with 4 vertices — which used to reach
/// `project_per_element_sizes_to_vertices` and abort the process on its
/// unguarded `vertex_sizes[99]`. This pins the structured error in place of
/// that panic (mirrors `reify-mesh-morph`'s `InvalidTetIndex`).
#[test]
fn refine_with_size_field_errors_on_out_of_range_tet_index() {
    let surface = dummy_surface();
    let vm = VolumeMesh {
        vertices: vec![0.0_f32; 12], // 4 vertices × 3 coords ⇒ valid ids are 0..=3
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 99],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };
    let opts = MeshingOptions::default();

    // One hint for one element: the size-hint length check would pass, so the
    // only thing standing between this mesh and the panic is the index gate.
    let result = refine_with_size_field(&surface, &vm, &[0.5_f64], &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::InvalidTetIndex {
                vertex_index: 99,
                vertex_count: 4
            })
        ),
        "expected InvalidTetIndex {{vertex_index: 99, vertex_count: 4}} rather \
         than an index-out-of-bounds panic in the projector, got: {result:?}",
    );
}

/// The out-of-range index must be rejected through the `adaptive` entry point
/// too, and the STRUCTURAL check must win when a buffer is both mis-sized and
/// out-of-range.
#[test]
fn refine_marked_elements_errors_on_out_of_range_tet_index() {
    let surface = dummy_surface();
    let vm = VolumeMesh {
        vertices: vec![0.0_f32; 12], // 4 vertices
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 99],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };
    let opts = MeshingOptions::default();

    let result = refine_marked_elements(&surface, &vm, &[0], &[0.5_f64], &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::InvalidTetIndex {
                vertex_index: 99,
                vertex_count: 4
            })
        ),
        "expected InvalidTetIndex {{vertex_index: 99, vertex_count: 4}}, got: {result:?}",
    );

    // Both defects at once (5 indices AND index 99): the structural check runs
    // first, so the report names the shape, not the value.
    let both = VolumeMesh {
        vertices: vec![0.0_f32; 12],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 99, 3],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };
    let result = refine_marked_elements(&surface, &both, &[0], &[0.5_f64], &opts);
    assert!(
        matches!(
            result,
            Err(RefineError::MalformedTetIndices { len: 5, stride: 4 })
        ),
        "a mesh that is both mis-sized and out-of-range must report the \
         structural defect first, got: {result:?}",
    );
}

// ---------------------------------------------------------------------------
// step-7: localized refinement integration test (runtime-gated on GMSH_AVAILABLE)
// ---------------------------------------------------------------------------

/// Localized size reduction refines the marked (x < 0.5) half and leaves the
/// far end of the unmarked half alone.
///
/// Both calls remesh the unit cube through `refine_with_size_field` over the
/// SAME hint carrier, `kuhn_lattice_unit_cube_vm(8)` (3072 tets): the baseline
/// under a uniform 0.5 field, the refinement under 0.125 (4x finer) on every
/// carrier tet whose centroid has x < 0.5 and 0.5 elsewhere.
///
/// Skipped at runtime when libgmsh is not present (`GMSH_AVAILABLE = false`):
/// on stub builds `refine_with_size_field` returns `GmshUnavailable`.
///
/// Assertions when gmsh IS available:
/// (a) Both calls return `Ok`.
/// (b) The refined mesh has strictly more tets with centroid x < 0.5.
/// (c) Over the FAR unmarked band, centroid x >= `FAR_BAND_MIN_X` (0.75), the
///     mean tet edge is within ±25% of the baseline's — after asserting that
///     both meshes have tets there, so (c) cannot pass on an empty band.
///
/// # Why (c) reads the far band, not the whole unmarked half
///
/// The mesh is SUPPOSED to be graded next to the marked boundary. The carrier's
/// hints reach gmsh as a per-vertex field by MIN projection, and a cell width
/// of 1/8 puts a vertex plane at exactly x = 0.5, so the requested field is
/// 0.125 up to x = 0.5 and ramps to 0.5 across the next cell; gmsh's gradient
/// limiter then grades the mesh further out than that. Measured on this
/// fixture (task #7447, libgmsh 4.15.2, bit-stable across repeated runs) —
/// mean tet edge, tet count in parentheses:
///
/// | band                                 | baseline    | refined      | ratio |
/// |--------------------------------------|-------------|--------------|-------|
/// | whole unmarked half, x >= 0.5        | 0.4140 (86) | 0.3028 (200) | 0.731 |
/// | far band, x >= 0.75                  | 0.3782 (57) | 0.3414 (71)  | 0.903 |
/// | far band, refined UNIFORMLY at 0.125 | 0.3782 (57) | 0.1562 (688) | 0.413 |
///
/// Whole meshes: 181 tets baseline, 1335 refined; marked half 95 -> 1135.
///
/// The whole half reads 0.731, outside ±25% — correctly: that is the graded
/// transition, not over-refinement. The far band reads 0.903, ~1.2x inside the
/// lower bound. The last row is the discrimination check, run by temporarily
/// making the refined field uniform: a refiner that does NOT localize reads
/// 0.413 and fails (c) with ~1.8x to spare, so the bound separates the two.
/// The band and the bound were fixed by ruling (esc-7447-7); a drifted 0.903 is
/// a finding to escalate, not a bound to widen.
///
/// The carrier used to be the baseline mesh itself (181 gmsh tets), whose
/// coarse tets straddling x = 0.5 dragged the MIN-projected fine hint a whole
/// tet's width into the unmarked half. Once the background field made the
/// refiner honour every vertex's hint (#7447), the whole-half ratio on that
/// carrier read 0.576 — the fixture, not the refiner, was what failed.
///
/// # Why the baseline is `refine_with_size_field`, not `mesh_to_volume`
///
/// It used to be `GmshKernel::mesh_to_volume(mesh_size = 0.5)`, and #6200
/// exposed that control as invalid: it compared two *different* producers —
/// `mesh_to_volume` applies a global target, while this path installs a
/// background size field over the carrier's tets — so no inequality between
/// them pins a property of the function under test. It passed only because
/// the pre-#6200 `mesh_to_volume` baseline was an *undersized* mesh (a 90°
/// `classify_surfaces` feature angle left the box only ~74–86%
/// tetrahedralized). Completing the box roughly doubled that baseline at the
/// same nominal size and the assertion inverted against an unchanged refine
/// result: (b) failed with `baseline=99, refined=95`.
///
/// Based on this function's own output, both sides come from one producer and
/// the test checks what its name claims. This is the same remedy applied one
/// crate over to
/// `reify-kernel-gmsh/tests/refine_volume_tests.rs::uniform_smaller_size_field_produces_more_tets`
/// (commit 187e3751f27d, plan step-7) for the identical cause.
///
/// # Why the carrier is hand-built (`kuhn_lattice_unit_cube_vm`, not `mesh_to_volume`)
///
/// The reason a hand-built carrier was first needed has since been closed at
/// BOTH ends. `mesh_to_volume` sets the **global** gmsh options
/// `Mesh.MeshSizeMin` and `Mesh.MeshSizeMax` to its resolved size, and
/// `ffi::clear()` clears *models*, not *options*; before task #6211
/// `refine_volume_with_size_field` wrote neither, so a `mesh_to_volume` seed
/// squeezed every later size in the process into `[size, size]` and the size
/// field silently became a no-op. Today the refine writes the pair itself on
/// entry (#6211), and every entry point enters `mesh_size_scope::MeshSizeScope`,
/// which establishes gmsh's defaults for every size option on entry and
/// restores them on every exit path (#6298, #6968). `reify-kernel-gmsh` pins
/// both halves and the end-to-end seed-then-refine sequence; its
/// `mesh_size_scope` module doc maps each writer to its guard, so that map is
/// not restated here.
///
/// **Why it stays hand-built anyway.** (i) *Producer symmetry*: a
/// `mesh_to_volume` carrier would put a second producer's sizing semantics
/// back on one side of the comparison the section above made single-producer.
/// (ii) *Determinism*: the lattice is fixed in source, so it cannot drift under
/// a gmsh version bump and needs no gmsh at all to build. (iii) *Where the fine
/// front lands*: the carrier's vertices decide where the MIN projection puts
/// the marked boundary, which the section above shows (c) is sensitive to; a
/// meshed carrier would hand that decision to the mesher.
///
/// One candidate mechanism for the #6200 confound is settled and is NOT the
/// cause: the two producers' differing clamps. Measured for #7447, one process
/// per reading, unit cube, P1 — `mesh_to_volume(S)`'s hard `[S, S]` against
/// this path's `[0, S]` at the same `S` — 186 vs 188 tets at S=0.5, 403 vs 397
/// at S=0.25, 2513 vs 2549 at S=0.125. Same density to within seed noise at
/// every size, so the clamp asymmetry is not a confound; reason (i) rests on
/// the producers' sizing SEMANTICS, which is a separate thing and still stands.
///
/// Measured **before #6211 and #6298**, one process per reading (unit cube,
/// P1, the pre-#7447 corner-anchor sizing) — the two `mesh_to_volume →` rows
/// record the inbound leak as it behaved then. Historical evidence, NOT a
/// description of today's behaviour:
///
/// | call sequence                              | tets |
/// |--------------------------------------------|------|
/// | refine(uniform 0.5) alone                  |  141 |
/// | refine(uniform 0.25) alone                 |  176 |
/// | refine(uniform 0.125) alone                |  367 |
/// | refine(0.125 on x=0 corners) alone         |  238 |
/// | mesh_to_volume(0.5) → refine(any field)    |  181 |
/// | mesh_to_volume(0.125) → refine(0.5)        | 2420 |
///
/// The last row was the direction-flipping confirmation: a *coarser* requested
/// field yielded a 17× denser mesh because the seed's 0.125 clamp, not the
/// field, decided the size. With a `mesh_to_volume` seed the baseline and the
/// refined call returned bit-identical meshes (181 vs 181), so (b) could not
/// pass no matter how the field was built. Kept as measured: it is the
/// evidence for the defect #6211 and #6298 fixed between them.
#[test]
fn localized_size_reduction_refines_marked_region_only() {
    /// Centroid-x lower edge of the far unmarked band (c) is measured over.
    const FAR_BAND_MIN_X: f64 = 0.75;

    if !reify_kernel_gmsh::GMSH_AVAILABLE {
        eprintln!("skipping: libgmsh not available in this build");
        return;
    }

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    // One hint carrier for BOTH calls, so the baseline and the refinement
    // differ in nothing but the hints. Cell width 1/8 puts a vertex plane at
    // x = 0.5, so the MIN-projected fine front lands exactly on the marked
    // boundary rather than a cell's width past it.
    let carrier = kuhn_lattice_unit_cube_vm(8);
    let n_carrier_tets = carrier.tet_indices().expect("carrier is tet-only").len() / 4;
    assert_eq!(
        n_carrier_tets, 3072,
        "fixture sanity: 6 tets per cell, 8^3 cells"
    );

    let vm_baseline =
        refine_with_size_field(&cube, &carrier, &vec![0.5_f64; n_carrier_tets], &opts)
            .expect("baseline refine_with_size_field must succeed");

    // 4x finer where the CARRIER tet's centroid is in the marked half.
    let localized_sizes: Vec<f64> = (0..n_carrier_tets)
        .map(|e| {
            if tet_centroid_x(&carrier, e) < 0.5 {
                0.125
            } else {
                0.5
            }
        })
        .collect();
    let vm_refined = refine_with_size_field(&cube, &carrier, &localized_sizes, &opts)
        .expect("refine_with_size_field must return Ok");

    // (b) More tets in marked region.
    let base_marked = count_tets_with_centroid_x_lt(&vm_baseline, 0.5);
    let refined_marked = count_tets_with_centroid_x_lt(&vm_refined, 0.5);
    assert!(
        refined_marked > base_marked,
        "marked region must have more tets after refinement: \
         baseline={base_marked}, refined={refined_marked}.\n\
         EQUAL counts mean the size field did not reach gmsh. Check, in \
         reify-kernel-gmsh's refine_volume.rs, that the PostView background field \
         is still installed (BackgroundFieldGuard) with Mesh.MeshSizeFromPoints = 0, \
         and that MeshSizeScope is still entered there: a Mesh.MeshSizeMin/Max \
         clamp leaked by a sibling entry point pins every element to one size. \
         That crate's size-option guards (mapped in its mesh_size_scope module \
         doc) go red on a scope regression; mid_span_size_reduction_refines_the_\
         mid_span_band here goes red on a background-field one."
    );

    // (c) The far unmarked band is not over-refined (±25%).
    let (base_far_count, base_far_mean) =
        mean_tet_edge_where(&vm_baseline, |cx| cx >= FAR_BAND_MIN_X);
    let (refined_far_count, refined_far_mean) =
        mean_tet_edge_where(&vm_refined, |cx| cx >= FAR_BAND_MIN_X);
    assert!(
        base_far_count > 0 && refined_far_count > 0,
        "both meshes must have tets in the far band x >= {FAR_BAND_MIN_X}: \
         baseline={base_far_count}, refined={refined_far_count}"
    );
    let ratio = refined_far_mean / base_far_mean;
    assert!(
        (0.75..=1.25).contains(&ratio),
        "far unmarked band (x >= {FAR_BAND_MIN_X}) mean tet edge ratio {ratio:.3} is outside \
         [0.75, 1.25] — refine_with_size_field over-refines away from the marked region \
         (baseline {base_far_mean:.4} over {base_far_count} tets, \
         refined {refined_far_mean:.4} over {refined_far_count} tets)"
    );
}

/// The size field's INTERIOR is honoured, not just its boundary values.
///
/// This is the case a 0D-corner-anchor size field provably cannot serve. A box
/// classifies to eight point entities, and eight corner scalars interpolate
/// MONOTONICALLY along each axis, so a field with an interior minimum is
/// structurally unrepresentable — no tuning of that path can express it.
///
/// Fixture: the requested field `0.04 + 0.9*|cx - 0.5|` is fine at mid-span and
/// coarse at both ends. Statistic: mean tet edge length by centroid band,
/// mid-span `0.42 < cx < 0.58` against both end bands pooled (`cx < 0.15`,
/// `cx > 0.85`).
///
/// Measured by THIS test on this exact path (discrete surface +
/// `classify_surfaces(PI/12)` + `create_geometry` + `Algorithm3D = 10`)
/// against libgmsh 4.15.2:
///
/// | sizing mechanism              | mid-span | ends   | ratio |
/// |-------------------------------|----------|--------|-------|
/// | 0D corner anchors (pre-#7447) | 0.6009   | 0.3173 | 1.893 |
/// | PostView background field     | 0.1328   | 0.2388 | 0.556 |
///
/// The two sit on OPPOSITE sides of 1.0: corner anchoring does not merely fail
/// to refine the mid-span, it leaves it COARSER than the ends. That sign change
/// is what the bound turns on, and no threshold between the two can be reached
/// by tuning the corner path.
///
/// Why 0.556 and not the ~0.30 a standalone C probe reads for the same analytic
/// field: this fixture cannot REQUEST 0.04. Per-element hints are sampled at tet
/// CENTROIDS, and on an n=4 lattice the centroids nearest `x = 0.5` sit at
/// `|cx - 0.5| ~= 0.0625`, so the finest hint the field ever carries is 0.0963.
/// The fixture's own requested mid/end ratio is 0.280 — which is the probe's
/// number. The remaining 0.280 -> 0.556 is gmsh's gradient limiter, which both
/// coarsens the mid-span (0.0963 -> 0.1328) and FINES the ends (0.3438 ->
/// 0.2388) as it bounds `|grad h|`.
///
/// So the margin below the bound is ~1.08x, not the ~2x the ratio alone
/// suggests, while the margin above is ~3.4x. Thin but not fragile: every input
/// is a fixed hand-built fixture and `deterministic: true` pins gmsh to one
/// thread, and the reading was bit-stable across three consecutive runs. A
/// future reading that drifts toward 0.6 is far more likely to be a gmsh
/// gradient-limiter change than a regression in this crate — check the
/// requested-vs-achieved pair above before touching the bound.
///
/// Deliberately NOT asserted: that the band reaches its requested size. Gmsh's
/// gradient limiter smooths any prescribed field, so "achieves the requested
/// size" would be a false premise no implementation could satisfy.
#[test]
fn mid_span_size_reduction_refines_the_mid_span_band() {
    if !reify_kernel_gmsh::GMSH_AVAILABLE {
        eprintln!("skipping: libgmsh not available in this build");
        return;
    }

    let cube = unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    // n = 4 gives 125 vertices / 384 tets. Interior vertices are the point:
    // the min-projection needs somewhere to put an interior minimum.
    let vm_seed = kuhn_lattice_unit_cube_vm(4);
    let n_seed_tets = vm_seed.tet_indices().expect("seed is tet-only").len() / 4;
    assert_eq!(n_seed_tets, 384, "fixture sanity: 6 tets per cell, 4^3 cells");

    let size_hints: Vec<f64> = (0..n_seed_tets)
        .map(|e| 0.04 + 0.9 * (tet_centroid_x(&vm_seed, e) - 0.5).abs())
        .collect();

    let refined = refine_with_size_field(&cube, &vm_seed, &size_hints, &opts)
        .expect("refine_with_size_field must succeed");

    let (mid_count, mid_mean) = mean_tet_edge_where(&refined, |cx| (0.42..0.58).contains(&cx));
    let (end_count, end_mean) = mean_tet_edge_where(&refined, |cx| !(0.15..=0.85).contains(&cx));

    // Non-vacuity first: an empty band would make the ratio below meaningless.
    assert!(
        mid_count > 0 && end_count > 0,
        "both bands must contain tets to compare: mid-span={mid_count}, ends={end_count}"
    );

    assert!(
        mid_mean < 0.6 * end_mean,
        "the mid-span band must be refined relative to the ends: \n\
         mid-span mean tet edge {mid_mean:.4} over {mid_count} tets, \
         ends {end_mean:.4} over {end_count} tets, ratio {:.3} (bound 0.6).\n\
         A ratio ABOVE 1.0 means the size field's interior never reached gmsh \
         and sizing fell back to boundary interpolation — check that the \
         PostView background field is installed and \
         `Mesh.MeshSizeFromPoints` is 0. A ratio between 0.6 and 1.0 with a \
         plausible tet count is the signature of a scrambled list-data buffer: \
         `gmshViewAddListData` accepts a per-POINT grouping with ierr=0, and \
         `BackgroundSizeField`'s byte-exact layout test is what tells the two \
         apart.",
        mid_mean / end_mean
    );
}

// ---- geometry helpers ----
/// Mean over the band's tets of each tet's own mean edge length, paired with
/// the band's tet count so a caller can reject a vacuous band before dividing.
///
/// Same statistic as `SplitStats` in
/// `reify-kernel-gmsh/tests/refine_volume_tests.rs` — a per-tet mean edge is
/// the quantity directly comparable to a requested characteristic length —
/// generalised from a single split point to an arbitrary centroid-x band so
/// both end bands can be pooled.
fn mean_tet_edge_where(vm: &VolumeMesh, keep: impl Fn(f64) -> bool) -> (usize, f64) {
    let tet_indices = vm.tet_indices().expect("fixture is tet-only");
    let n = tet_indices.len() / 4;
    let mut total = 0.0_f64;
    let mut count = 0usize;
    for e in 0..n {
        if !keep(tet_centroid_x(vm, e)) {
            continue;
        }
        let base = e * 4;
        let verts: Vec<[f64; 3]> = (0..4)
            .map(|k| {
                let vi = tet_indices[base + k] as usize;
                [
                    vm.vertices[vi * 3] as f64,
                    vm.vertices[vi * 3 + 1] as f64,
                    vm.vertices[vi * 3 + 2] as f64,
                ]
            })
            .collect();
        let mut tet_edge_total = 0.0_f64;
        for i in 0..4 {
            for j in (i + 1)..4 {
                let dx = verts[i][0] - verts[j][0];
                let dy = verts[i][1] - verts[j][1];
                let dz = verts[i][2] - verts[j][2];
                tet_edge_total += (dx * dx + dy * dy + dz * dz).sqrt();
            }
        }
        total += tet_edge_total / 6.0;
        count += 1;
    }
    let mean = if count == 0 { 0.0 } else { total / count as f64 };
    (count, mean)
}


fn tet_centroid_x(vm: &VolumeMesh, elem_idx: usize) -> f64 {
    let base = elem_idx * 4;
    let tet_indices = vm.tet_indices().expect("fixture is tet-only");
    (0..4)
        .map(|k| vm.vertices[(tet_indices[base + k] as usize) * 3] as f64)
        .sum::<f64>()
        / 4.0
}

fn count_tets_with_centroid_x_lt(vm: &VolumeMesh, threshold: f64) -> usize {
    let n = vm.tet_indices().expect("fixture is tet-only").len() / 4;
    (0..n).filter(|&e| tet_centroid_x(vm, e) < threshold).count()
}

// ---------------------------------------------------------------------------
// step-7/8: the gmsh re-export seam
// ---------------------------------------------------------------------------

/// `reify-solver-elastic` must re-export the two gmsh symbols its own PUBLIC
/// refine signatures require, so a downstream crate can call them without
/// naming `reify_kernel_gmsh::*`.
///
/// This closes a pre-existing API gap: `refine_with_size_field` and
/// `adaptive::refine_marked_elements` both take `&MeshingOptions` in their
/// public signature, but the crate re-exported neither that type nor the
/// availability const — so no downstream crate could construct the argument
/// or runtime-gate on gmsh presence.
///
/// It matters because `reify-eval` is FORBIDDEN to name the gmsh crate:
/// `reify-eval/Cargo.toml` makes `reify-kernel-gmsh` a DEV-dep with a
/// dead-strip invariant ("DO NOT reference any `reify_kernel_gmsh::*` symbol
/// from other reify-eval unit or integration tests — doing so would pull
/// gmsh's `inventory::submit!` into their binaries and break OCCT-only
/// `kernel_count` / registry-size assertions"). Re-exporting from
/// `reify-solver-elastic` — a NORMAL dep of reify-eval that already
/// normal-deps `reify-kernel-gmsh` — keeps that invariant literally true.
///
/// The `reify_kernel_gmsh::GMSH_AVAILABLE` reference below is the ONE place
/// the gmsh path is named, and it is legitimate here: this test lives INSIDE
/// `reify-solver-elastic`, where gmsh is a normal dep. Its purpose is to pin
/// that the re-export is the same const and cannot silently drift.
#[test]
fn solver_elastic_reexports_the_gmsh_types_its_public_refine_signature_requires() {
    let options = reify_solver_elastic::MeshingOptions {
        mesh_size: Some(0.25),
        deterministic: true,
        ..Default::default()
    };
    assert_eq!(options.mesh_size, Some(0.25));
    assert!(options.deterministic);

    assert_eq!(
        reify_solver_elastic::GMSH_AVAILABLE,
        reify_kernel_gmsh::GMSH_AVAILABLE,
        "the re-exported availability const must BE the kernel's, not a copy \
         that can drift from it",
    );
}

// ---------------------------------------------------------------------------
// step-9/10: the extracted boundary must be a USABLE refine surface
// ---------------------------------------------------------------------------

/// Characteristic edge length of a tet of volume `v`: `(6*v)^(1/3)`.
///
/// `6*v` undoes the canonical `V = |det J| / 6`, recovering a length on the
/// same scale as the mesh's actual element sizes. This is the definition
/// `aposteriori_validation.rs`'s `characteristic_size_from_volume` uses; the
/// SAME definition must be used everywhere `current_sizes` is derived, or the
/// per-element sizes handed to `refine_marked_elements` stop being comparable
/// across a refine.
fn characteristic_size_from_volume(v: f64) -> f64 {
    (6.0 * v).cbrt()
}

/// `(coords, tets)` of a P1 [`VolumeMesh`], widened to `f64`.
fn nodes_conns(vm: &VolumeMesh) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
    let coords: Vec<[f64; 3]> = vm
        .vertices
        .chunks_exact(3)
        .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
        .collect();
    let conns: Vec<[usize; 4]> = vm
        .tet_indices()
        .expect("P1 tet mesh")
        .chunks_exact(4)
        .map(|c| [c[0] as usize, c[1] as usize, c[2] as usize, c[3] as usize])
        .collect();
    (coords, conns)
}

/// Unsigned volume of the P1 tet `conn` over `nodes`.
fn tet_volume(nodes: &[[f64; 3]], conn: &[usize; 4]) -> f64 {
    let p = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
    let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    let w = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    (w[0] * cross[0] + w[1] * cross[1] + w[2] * cross[2]).abs() / 6.0
}

/// **The load-bearing contract of task 4909.**
///
/// The boundary EXTRACTED from a gmsh-produced tet mesh — not the original
/// hand-wound fixture surface — must itself be a usable refine surface: it
/// has to survive gmsh's `classify_surfaces` + `create_geometry` +
/// `geo_add_surface_loop` chain and drive a real, mark-driven remesh.
///
/// This is what makes the eval-side realized path possible at all. A
/// `RealizationReadHandle` carries exactly ONE `RealizedContent` variant, and
/// for `solver::elastic_static` that variant is the `VolumeMesh`, so no
/// surface `Mesh` reaches the trampoline. Reconstructing the boundary from
/// the realized tet mesh is the only route that does not require a second
/// realization demand — and it is also the tighter one, because that boundary
/// bounds exactly the tets the background size field is built over.
///
/// A failure here surfaces as `"no dim=2 entities after classify+create_geometry;
/// surface may be open or non-manifold"`.
#[test]
fn extracted_boundary_is_a_usable_refine_surface_for_the_mesh_it_came_from() {
    if !reify_kernel_gmsh::GMSH_AVAILABLE {
        eprintln!("skipping: libgmsh not available in this build");
        return;
    }

    let opts = MeshingOptions {
        mesh_size: Some(0.5),
        deterministic: true,
        ..Default::default()
    };

    // (1) Seed a volume from a hand-wound closed box surface under a UNIFORM
    // size field — the `seed_volume_from_surface` recipe. From here on the
    // hand-wound cube is NEVER used again: everything downstream goes through
    // the extracted boundary.
    let cube = unit_cube_mesh();
    // A uniform 0.5 field over a minimal sizing mesh spanning the same box.
    let sizing = kuhn_6tet_unit_cube_vm();
    let n_sizing_verts = sizing.vertices.len() / 3;
    let uniform_field = reify_kernel_gmsh::BackgroundSizeField::from_tet_mesh(
        &sizing,
        &vec![0.5_f64; n_sizing_verts],
    )
    .expect("a uniform field over the Kuhn cube must be constructible");
    let volume = reify_kernel_gmsh::refine_volume_with_size_field(
        &cube,
        &uniform_field,
        &opts,
        ElementOrderTag::P1,
    )
    .expect("seeding a volume from the hand-wound cube must succeed");

    let (nodes, conns) = nodes_conns(&volume);
    let n_before = conns.len();
    assert!(n_before > 0, "seed volume must have at least one tet");

    // (2) Extract the boundary from the gmsh-produced mesh.
    let extracted = reify_solver_elastic::boundary_surface_mesh(&volume)
        .expect("a gmsh-produced P1 tet mesh must have an extractable boundary");
    assert!(
        !extracted.indices.is_empty(),
        "the extracted boundary must not be empty",
    );

    // (3) Mark a spatially-coherent half of the mesh (x < 0.5) and derive the
    // per-element characteristic sizes `refine_marked_elements` expects.
    let current_sizes: Vec<f64> = conns
        .iter()
        .map(|c| characteristic_size_from_volume(tet_volume(&nodes, c)))
        .collect();
    let marked: Vec<usize> = conns
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let cx = c.iter().map(|&n| nodes[n][0]).sum::<f64>() / 4.0;
            cx < 0.5
        })
        .map(|(e, _)| e)
        .collect();
    assert!(
        !marked.is_empty(),
        "the x < 0.5 half of a unit-cube mesh must contain elements",
    );

    // (4) The extracted boundary must drive a real remesh that GROWS the mesh.
    let refined = refine_marked_elements(&extracted, &volume, &marked, &current_sizes, &opts)
        .expect(
            "refine_marked_elements must accept the EXTRACTED boundary as its \
             surface - if this fails with 'no dim=2 entities after \
             classify+create_geometry' the extracted surface is open or \
             non-manifold",
        );

    let n_after = refined.tet_indices().expect("refined is tet-only").len() / 4;
    assert!(
        n_after > n_before,
        "a mark-driven remesh through the extracted boundary must strictly \
         grow the element count: {n_before} -> {n_after}",
    );
}
