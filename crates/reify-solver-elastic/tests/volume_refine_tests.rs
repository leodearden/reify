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
/// This is a *seed* only: `refine_with_size_field` needs a `VolumeMesh` to
/// attach per-element size hints to, and this supplies one without invoking
/// gmsh. See `localized_size_reduction_refines_marked_region_only` for why the
/// seed must not come from `mesh_to_volume`.
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

/// Localized size reduction refines only the marked (x < 0.5) region.
///
/// Baseline: the unit cube run through `refine_with_size_field` itself with a
/// uniform 0.5 size field. Refinement: the same call, with marked tets
/// (centroid x < 0.5) given size 0.125 (4× finer) and unmarked tets keeping
/// 0.5.
///
/// Skipped at runtime when libgmsh is not present (`GMSH_AVAILABLE = false`).
/// On stub builds `refine_with_size_field` returns `GmshUnavailable`, so this
/// test exits early rather than asserting on a stub result.
///
/// Assertions when gmsh IS available:
/// (a) `refine_with_size_field` returns `Ok`.
/// (b) Refined mesh has strictly more tets with centroid x < 0.5 than baseline.
/// (c) Average tet edge length in unmarked region (centroid x ≥ 0.5) is
///     within ±25% of baseline average (not over-refined; generous tolerance
///     for gmsh's spatial smoothing extent).
///
/// # Why the baseline is `refine_with_size_field`, not `mesh_to_volume`
///
/// It used to be `GmshKernel::mesh_to_volume(mesh_size = 0.5)`, and #6200
/// exposed that control as invalid: it compared two *different* producers that
/// do not share sizing semantics — `mesh_to_volume` applies a global target,
/// while this path sets per-corner sizes with `Mesh.MeshSizeFromPoints=1` and
/// lets gmsh interpolate — so no inequality between them pins a property of
/// the function under test. It passed only because the pre-#6200
/// `mesh_to_volume` baseline was an *undersized* mesh (a 90° `classify_surfaces`
/// feature angle left the box only ~74–86% tetrahedralized). Completing the box
/// roughly doubles that baseline at the same nominal size and the assertion
/// inverts against an unchanged refine result: measured on this branch,
/// (b) failed with `baseline=99, refined=95`.
///
/// Re-based on this function's own coarser output, both sides come from one
/// producer and the test checks what its name claims. Measured after re-basing:
/// 141 tets (baseline, uniform 0.5) → 238 tets (refined); marked-region tets
/// 76 → 177; unmarked-region average tet edge 0.4487 → 0.4414, ratio 0.9837.
///
/// This is the same remedy applied one crate over to
/// `reify-kernel-gmsh/tests/refine_volume_tests.rs::uniform_smaller_size_field_produces_more_tets`
/// (commit 187e3751f27d, plan step-7) for the identical cause.
///
/// # Why the seed is hand-built (`kuhn_6tet_unit_cube_vm`, not `mesh_to_volume`)
///
/// `refine_with_size_field` needs *some* `VolumeMesh` to attach per-element
/// hints to, and this one is written out by hand. The reason it was originally
/// hand-built has since been closed at the consumer; the reasons it stays that
/// way are independent of it.
///
/// **The original reason, closed by #6211.** `mesh_to_volume` sets the
/// **global** gmsh options `Mesh.MeshSizeMin` and `Mesh.MeshSizeMax` to its
/// resolved size (`kernel_real.rs:203-206`), and `ffi::clear()` clears
/// *models*, not *options*. Before task #6211 `refine_volume_with_size_field`
/// wrote neither option, so every later per-corner `SetSize` in the same
/// process was squeezed into `[size, size]` and the size field silently became
/// a no-op. That inbound squeeze can no longer happen: the function now writes
/// the pair itself on entry — `MeshSizeMin` to gmsh's `0.0` default,
/// `MeshSizeMax` to `max(vertex_sizes)`, at the "Mesh-size clamp: set
/// explicitly, never inherited" block in `refine_volume.rs` — so its
/// output is a function of its own arguments rather than of whatever a sibling
/// entry point last left behind.
///
/// **Why it stays hand-built anyway.** (i) *Producer symmetry*: the section
/// above re-based the baseline onto this same function precisely so both sides
/// of the comparison come from one producer, and a `mesh_to_volume` seed would
/// put a second producer's sizing semantics back on one side of it. (ii)
/// *Determinism*: this 6-tet Kuhn partition is fixed in source, so the seed
/// cannot drift under a gmsh version bump and needs no gmsh at all to build.
/// (iii) *#6298 is still open*: #6211 defended this **consumer**, it did not fix
/// the **producer**, so `mesh_to_volume` still leaves its clamp behind for any
/// later caller that writes no clamp of its own.
///
/// Measured **before #6211**, one process per reading (unit cube, P1) — the two
/// `mesh_to_volume →` rows record the inbound leak as it behaved then:
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
/// refined call returned bit-identical meshes (181 vs 181, equal average edge
/// length), so assertion (b) could not pass no matter how the field was built —
/// which is why re-basing alone was not sufficient here. Kept as measured: it
/// is the evidence for the defect #6211 fixed, not a description of today's
/// behaviour.
///
/// Filed as **task #6298** — a producer-side defect, out of #6200's scope
/// (#6200 owns the `classify_surfaces` feature angle; the leak is a distinct
/// bug in a different function). Tasks #6211, #6212 and #6262 cover adjacent
/// facets of the same global-option leak.
///
/// # The constraint this test needs, until #6298 is fixed
///
/// **No test in this binary may call `GmshKernel::mesh_to_volume`.** Not "this
/// one must be the only one", and not "no sibling may do so *before* it" —
/// `cargo` runs a binary's tests in ONE process with no guaranteed order, so
/// any sibling that meshes via `mesh_to_volume` could leak its clamp into this
/// test whatever order they run in. As of this commit no test here calls it,
/// which is why the constraint reads as a prohibition rather than a
/// reservation. Since #6211 it is *also* enforced mechanically for the clamp
/// pair: `refine_volume.rs` sets `Mesh.MeshSizeMin` / `MeshSizeMax` on entry
/// and restores gmsh's defaults on every exit path — early `?` returns
/// included — via its `MeshSizeClampReset` RAII guard, so a `[size, size]`
/// leaked by a sibling `mesh_to_volume` can no longer reach this test's refine
/// calls. (Both directions are pinned by fixtures in
/// `reify-kernel-gmsh/tests/refine_volume_tests.rs`, not only by this note.)
///
/// What that does *not* cover is any global option `refine_volume.rs` does not
/// itself write on entry. Today it writes `General.Terminal`,
/// `General.NumThreads`, `Mesh.ElementOrder`, `Mesh.Algorithm3D`,
/// `Mesh.MeshSizeFromPoints` / `FromCurvature` / `ExtendFromBoundary` and the
/// clamp pair, which happens to be a superset of everything `mesh_to_volume`
/// writes — but that is a coincidence of two option sets, not an invariant
/// anything checks, and it says nothing about a *future* producer-side write.
/// So the prohibition stays, alongside the self-diagnosing failure message on
/// assertion (b) below, and both still-open directions keep their owners:
/// **#6298** for the producer-side leak (`mesh_to_volume` leaving its clamp
/// behind), **#6212** for this function's own outbound
/// `MeshSizeFromPoints` / `FromCurvature` / `ExtendFromBoundary` leak.
#[test]
fn localized_size_reduction_refines_marked_region_only() {
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

    // Seed mesh. Its ONLY role is to carry a size field into
    // `refine_with_size_field`, which needs a `VolumeMesh` to attach
    // per-element hints to. Because that field is UNIFORM, the seed cannot
    // influence the baseline: `project_per_element_sizes_to_vertices` takes a
    // min over incident elements (0.5 everywhere regardless of density or
    // topology) and the nearest-neighbour surface projection then hands gmsh
    // `[0.5; 8]` whatever the seed looked like. That is what removes the
    // cross-producer confound — the baseline below is produced entirely by the
    // function under test.
    //
    // The seed is hand-built rather than meshed by `mesh_to_volume`, which
    // would silently disable the size field for the rest of the process — see
    // "Why the seed is hand-built" in the doc comment above.
    let vm_seed = kuhn_6tet_unit_cube_vm();
    let n_seed_tets = vm_seed.tet_indices().expect("seed is tet-only").len() / 4;
    assert!(n_seed_tets > 0, "seed must have at least one tet");

    // Baseline: same producer as the refinement, uniform 0.5 field.
    let vm_baseline = refine_with_size_field(&cube, &vm_seed, &vec![0.5_f64; n_seed_tets], &opts)
        .expect("baseline refine_with_size_field must succeed");

    let n_base_tets = vm_baseline.tet_indices().expect("baseline is tet-only").len() / 4;
    assert!(n_base_tets > 0, "baseline must have at least one tet");

    // Build per-element size hints: 4× finer in marked region (x < 0.5).
    // Derived per BASELINE tet — `refine_with_size_field` validates the hint
    // count against the mesh it is handed, so the field must be rebuilt against
    // whichever mesh serves as the baseline.
    let per_element_sizes: Vec<f64> = (0..n_base_tets)
        .map(|e| {
            let cx = tet_centroid_x(&vm_baseline, e);
            if cx < 0.5 { 0.125 } else { 0.5 }
        })
        .collect();

    let result = refine_with_size_field(&cube, &vm_baseline, &per_element_sizes, &opts);
    let vm_refined = result.expect("refine_with_size_field must return Ok");

    assert!(
        vm_refined.tet_indices().expect("refined is tet-only").len() / 4 > 0,
        "refined mesh must have at least one tet"
    );

    // (b) More tets in marked region.
    let base_marked = count_tets_with_centroid_x_lt(&vm_baseline, 0.5);
    let refined_marked = count_tets_with_centroid_x_lt(&vm_refined, 0.5);
    assert!(
        refined_marked > base_marked,
        "marked region must have more tets after refinement: \
         baseline={base_marked}, refined={refined_marked}.\n\
         If those two counts are EQUAL and the whole meshes are bit-identical, \
         suspect the #6298 global-option leak before suspecting the size field: \
         some test in this binary called `GmshKernel::mesh_to_volume`, which \
         leaves `Mesh.MeshSizeMin`/`Mesh.MeshSizeMax` pinned to its own resolved \
         size process-wide (gmsh's option table survives `gmshClear`), squeezing \
         every later per-corner `SetSize` into [size, size]. See the \
         'Why the seed is hand-built' note on this test."
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

fn avg_tet_edge_in_region_x_ge(vm: &VolumeMesh, threshold: f64) -> f64 {
    let tet_indices = vm.tet_indices().expect("fixture is tet-only");
    let n = tet_indices.len() / 4;
    let mut total_edge = 0.0_f64;
    let mut count = 0usize;
    for e in 0..n {
        if tet_centroid_x(vm, e) < threshold {
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
