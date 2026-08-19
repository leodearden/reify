//! Unit tests for `reify_kernel_gmsh::fill_metrics` — the tet-mesh fill /
//! coverage metrics used to detect an incomplete tetrahedralization (#6200).
//!
//! Deliberately UNCONDITIONAL (no `#![cfg(has_gmsh)]` gate): `fill_metrics` is
//! pure `reify_ir` arithmetic with no libgmsh dependency, following the
//! precedent set by the unconditional helpers in `src/mesh_volume.rs`, so the
//! volume arithmetic stays verified on stub builds where libgmsh is absent.
//!
//! Every assertion here is on runtime behaviour against a closed-form
//! reference volume — never on a docstring, comment, or symbol name.

use reify_ir::{ElementOrderTag, Mesh, VolumeConnectivity, VolumeMesh};
use reify_kernel_gmsh::fill_metrics::{
    TetFillReport, enclosed_volume_of_surface, tet_fill_report, tet_signed_volume,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Inline copy of `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:13-48`
/// (itself a copy of `crates/reify-kernel-manifold/src/test_fixtures.rs:37-67`),
/// generalised to arbitrary extents.
///
/// 8 vertices / 12 outward-wound triangles. Enclosed volume is exactly
/// `lx * ly * lz`. The winding is vetted outward (unlike
/// `through_thickness_tests.rs`'s `slab_surface_mesh`), which is a precondition
/// for the signed-volume assertions below.
fn prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
    Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            lx, 0.0, 0.0, // 1
            lx, ly, 0.0, // 2
            0.0, ly, 0.0, // 3
            0.0, 0.0, lz, // 4
            lx, 0.0, lz, // 5
            lx, ly, lz, // 6
            0.0, ly, lz, // 7
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

/// The unit cube — `prismatic_box_mesh(1.0, 1.0, 1.0)`, enclosed volume 1.0.
fn unit_cube_mesh() -> Mesh {
    prismatic_box_mesh(1.0, 1.0, 1.0)
}

/// Per-face UNWELDED box: 24 vertices (each of the 6 faces carries its own 4
/// bit-identical corner copies) / 12 triangles, same outward winding as
/// [`prismatic_box_mesh`].
///
/// This is the shape `OcctKernel::tessellate` actually emits for a planar-faced
/// solid (see `kernel_real.rs:452`), so it is what the production
/// surface→volume path receives BEFORE the repair pre-stage welds it. Enclosed
/// volume must be identical to the welded form: the divergence-theorem sum is a
/// per-triangle integral against the origin and so is welding-independent.
fn unwelded_prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
    // Corner coordinates, indexed exactly as in `prismatic_box_mesh`.
    let c = [
        [0.0, 0.0, 0.0],
        [lx, 0.0, 0.0],
        [lx, ly, 0.0],
        [0.0, ly, 0.0],
        [0.0, 0.0, lz],
        [lx, 0.0, lz],
        [lx, ly, lz],
        [0.0, ly, lz],
    ];
    // Per-face corner quads, in the same cyclic order the welded fixture uses.
    let faces: [[usize; 4]; 6] = [
        [0, 1, 2, 3], // -Z
        [4, 5, 6, 7], // +Z
        [0, 1, 5, 4], // -Y
        [3, 7, 6, 2], // +Y
        [0, 4, 7, 3], // -X
        [1, 2, 6, 5], // +X
    ];
    // Per-face triangle pattern, expressed in face-local corner slots. Matches
    // the welded fixture's winding face for face.
    let tri_slots: [[usize; 6]; 6] = [
        [0, 2, 1, 0, 3, 2], // -Z: (0,2,1) (0,3,2)
        [0, 1, 2, 0, 2, 3], // +Z: (4,5,6) (4,6,7)
        [0, 1, 2, 0, 2, 3], // -Y: (0,1,5) (0,5,4)
        [0, 1, 2, 0, 2, 3], // +Y: (3,7,6) (3,6,2)
        [0, 1, 2, 0, 2, 3], // -X: (0,4,7) (0,7,3)
        [0, 1, 2, 0, 2, 3], // +X: (1,2,6) (1,6,5)
    ];

    let mut vertices = Vec::with_capacity(24 * 3);
    let mut indices = Vec::with_capacity(36);
    for (f, quad) in faces.iter().enumerate() {
        let base = (f * 4) as u32;
        for &corner in quad {
            vertices.extend_from_slice(&c[corner]);
        }
        for &slot in &tri_slots[f] {
            indices.push(base + slot as u32);
        }
    }
    Mesh {
        vertices,
        indices,
        normals: None,
    }
}

/// The reference tet [(0,0,0), (1,0,0), (0,1,0), (0,0,1)], signed volume +1/6.
const REFERENCE_TET: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// Kuhn/Freudenthal 6-tet decomposition of a `lx × ly × lz` box, with every
/// tet wound to POSITIVE signed volume (the odd-permutation tets have their
/// last two corners swapped).
///
/// A complete conforming decomposition, so `Σ|V_tet|` is exactly `lx*ly*lz`
/// and it partitions its own AABB — the geometric identity the #6200 guard
/// turns on. Node order matches [`prismatic_box_mesh`]'s corner numbering.
fn kuhn_box_volume_mesh(lx: f32, ly: f32, lz: f32) -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            lx, 0.0, 0.0, // 1
            lx, ly, 0.0, // 2
            0.0, ly, 0.0, // 3
            0.0, 0.0, lz, // 4
            lx, 0.0, lz, // 5
            lx, ly, lz, // 6
            0.0, ly, lz, // 7
        ],
        connectivity: VolumeConnectivity::Tet {
            #[rustfmt::skip]
            indices: vec![
                0, 1, 2, 6,
                0, 1, 6, 5,
                0, 3, 6, 2,
                0, 3, 7, 6,
                0, 4, 5, 6,
                0, 4, 6, 7,
            ],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// A "cone from the centre" decomposition of the unit cube: 9 nodes (8
/// corners + the body centre) and 12 tets, one per surface triangle of
/// [`unit_cube_mesh`], each wound to positive signed volume.
///
/// Also a complete conforming decomposition (`Σ|V_tet| == 1.0`), but unlike
/// the Kuhn form it has exactly ONE strictly-interior node — the discriminator
/// for `strictly_interior_nodes`.
fn centre_cone_unit_cube_volume_mesh() -> VolumeMesh {
    let surface = unit_cube_mesh();
    let centre_index = 8u32;
    let mut vertices = surface.vertices.clone();
    vertices.extend_from_slice(&[0.5, 0.5, 0.5]);

    let mut indices = Vec::with_capacity(12 * 4);
    for tri in surface.indices.chunks_exact(3) {
        // For an OUTWARD-wound triangle (a, b, c) and an interior apex, the
        // tet (a, b, c, apex) is negatively oriented; swapping b and c makes
        // it positive.
        indices.extend_from_slice(&[tri[0], tri[2], tri[1], centre_index]);
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

/// Build a P1 tet `VolumeMesh` from explicit f32 vertices + connectivity.
fn p1_tet_mesh(vertices: Vec<f32>, indices: Vec<u32>) -> VolumeMesh {
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

/// Assert `actual` matches `expected` to within `rel` RELATIVE error.
#[track_caller]
fn assert_rel(actual: f64, expected: f64, rel: f64, what: &str) {
    let err = (actual - expected).abs() / expected.abs().max(f64::MIN_POSITIVE);
    assert!(
        err <= rel,
        "{what}: got {actual:.12e}, expected {expected:.12e} (relative error {err:.3e} > {rel:.3e})"
    );
}

// ---------------------------------------------------------------------------
// (a) tet_signed_volume — sign and magnitude
// ---------------------------------------------------------------------------

#[test]
fn reference_tet_has_signed_volume_one_sixth() {
    assert_eq!(
        tet_signed_volume(&REFERENCE_TET),
        1.0 / 6.0,
        "the reference tet [(0,0,0),(1,0,0),(0,1,0),(0,0,1)] must measure exactly +1/6"
    );
}

#[test]
fn swapping_two_corners_flips_sign_and_preserves_magnitude() {
    let mut swapped = REFERENCE_TET;
    swapped.swap(1, 2);
    let v = tet_signed_volume(&swapped);
    assert_eq!(
        v,
        -1.0 / 6.0,
        "swapping two corners must flip the SIGN (this is what makes an \
         inverted tet detectable) while preserving the magnitude"
    );
    assert_eq!(v.abs(), tet_signed_volume(&REFERENCE_TET).abs());
}

#[test]
fn tet_volume_scales_cubically_under_uniform_dilation() {
    let scaled = REFERENCE_TET.map(|p| p.map(|x| x * 2.0));
    assert_rel(
        tet_signed_volume(&scaled),
        8.0 / 6.0,
        1e-15,
        "uniform 2x dilation must scale tet volume by 8",
    );
}

// ---------------------------------------------------------------------------
// (b) tet_fill_report over a complete decomposition
// ---------------------------------------------------------------------------

#[test]
fn kuhn_unit_cube_reports_complete_fill() {
    let vm = kuhn_box_volume_mesh(1.0, 1.0, 1.0);
    let r = tet_fill_report(&vm).expect("Kuhn cube is a well-formed P1 tet mesh");

    assert_eq!(r.n_tets, 6, "Kuhn decomposition has exactly 6 tets");
    assert_eq!(r.n_nodes, 8, "the cube's 8 corners and nothing else");
    assert_rel(r.abs_volume_sum, 1.0, 1e-12, "Σ|V_tet| of the unit cube");
    assert_rel(r.aabb_volume, 1.0, 1e-12, "AABB volume of the unit cube");
    assert_eq!(
        r.inverted_tets, 0,
        "every Kuhn tet was wound positive, so none may be reported inverted"
    );
    assert_rel(
        r.signed_volume_sum,
        1.0,
        1e-12,
        "consistently-wound mesh: signed sum equals abs sum",
    );
}

#[test]
fn kuhn_non_cubic_box_reports_complete_fill() {
    let vm = kuhn_box_volume_mesh(1.0, 0.1, 0.1);
    let r = tet_fill_report(&vm).expect("well-formed P1 tet mesh");
    // f32 storage of 0.1 makes this inexact; 1e-6 relative is the derived bound.
    assert_rel(
        r.abs_volume_sum,
        0.01,
        1e-6,
        "Σ|V_tet| of a 1.0x0.1x0.1 box",
    );
    assert_rel(
        r.aabb_volume,
        0.01,
        1e-6,
        "AABB volume of a 1.0x0.1x0.1 box",
    );
    assert_rel(
        r.aabb_fill_fraction(),
        1.0,
        1e-6,
        "a complete decomposition of a box fills its AABB exactly",
    );
}

#[test]
fn centre_cone_unit_cube_reports_complete_fill() {
    let vm = centre_cone_unit_cube_volume_mesh();
    let r = tet_fill_report(&vm).expect("well-formed P1 tet mesh");
    assert_eq!(r.n_tets, 12);
    assert_eq!(r.n_nodes, 9);
    assert_rel(
        r.abs_volume_sum,
        1.0,
        1e-12,
        "Σ|V_tet| of the centre-cone cube",
    );
    assert_rel(r.aabb_volume, 1.0, 1e-12, "AABB volume");
    assert_eq!(r.inverted_tets, 0);
}

// ---------------------------------------------------------------------------
// (c) inverted tets and the no-orientation-cancellation discriminator
// ---------------------------------------------------------------------------

#[test]
fn a_single_inverted_tet_is_counted() {
    // The reference tet with two corners swapped — negative signed volume.
    let vm = p1_tet_mesh(
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        vec![0, 2, 1, 3],
    );
    let r = tet_fill_report(&vm).expect("well-formed P1 tet mesh");
    assert_eq!(r.n_tets, 1);
    assert_eq!(
        r.inverted_tets, 1,
        "a negatively-oriented tet must be counted"
    );
    assert_rel(
        r.abs_volume_sum,
        1.0 / 6.0,
        1e-12,
        "magnitude is unaffected",
    );
    assert_rel(
        r.signed_volume_sum,
        -1.0 / 6.0,
        1e-12,
        "the signed sum carries the inversion",
    );
}

#[test]
fn mixed_orientation_mesh_shows_signed_sum_below_abs_sum() {
    // A Kuhn cube with ONE tet's winding flipped: |signed| = 1 - 2*(1/6) = 2/3,
    // while abs = 1. This is the "no orientation cancellation" discriminator.
    let mut vm = kuhn_box_volume_mesh(1.0, 1.0, 1.0);
    if let VolumeConnectivity::Tet { indices, .. } = &mut vm.connectivity {
        indices.swap(1, 2); // flip the first tet's winding
    }
    let r = tet_fill_report(&vm).expect("well-formed P1 tet mesh");

    assert_eq!(r.inverted_tets, 1);
    assert_rel(r.abs_volume_sum, 1.0, 1e-12, "magnitudes still sum to 1");
    assert_rel(
        r.signed_volume_sum,
        2.0 / 3.0,
        1e-12,
        "one tet cancels twice its volume",
    );
    assert!(
        r.signed_volume_sum.abs() < r.abs_volume_sum,
        "mixed orientation must make |signed sum| strictly less than the abs sum \
         (got signed={}, abs={})",
        r.signed_volume_sum,
        r.abs_volume_sum
    );
}

// ---------------------------------------------------------------------------
// (d) enclosed_volume_of_surface — divergence theorem, welding-independent
// ---------------------------------------------------------------------------

#[test]
fn enclosed_volume_of_unit_cube_is_exactly_one() {
    assert_eq!(
        enclosed_volume_of_surface(&unit_cube_mesh()),
        1.0,
        "the divergence-theorem sum is exact for polyhedra with integral coordinates"
    );
}

/// Non-dyadic extents get the f32-storage-derived 1e-6 bound, NOT the 1e-12
/// used for integral-coordinate fixtures.
///
/// `Mesh::vertices` is `Vec<f32>`, and 0.1 is not representable in binary: it
/// stores as 0.100000001490116…, so a 1.0x0.1x0.1 box measures
/// 1.000000029802e-2 against an exact 1e-2 — relative 2.98e-8, pure storage
/// round-off on the coordinate rather than any error in the summation. (#6154
/// independently measured the identical 1.0000000298e-2 for the realized
/// fixture's AABB.) The derived ceiling is ≈ 3 × 2^-23 ≈ 3.6e-7 for a
/// degree-3 form in the coordinates; 1e-6 clears it ~3x while sitting five
/// orders of magnitude below the ~26% defect #6200 is guarding against.
const F32_STORAGE_REL: f64 = 1e-6;

#[test]
fn enclosed_volume_of_thin_box_matches_closed_form() {
    assert_rel(
        enclosed_volume_of_surface(&prismatic_box_mesh(1.0, 0.1, 0.1)),
        0.01,
        F32_STORAGE_REL,
        "enclosed volume of a 1.0x0.1x0.1 box",
    );
}

#[test]
fn enclosed_volume_is_independent_of_vertex_welding() {
    // The production input from `OcctKernel::tessellate` is per-face unwelded;
    // the reference volume must not depend on the repair pre-stage having run.
    let welded = prismatic_box_mesh(1.0, 0.1, 0.1);
    let unwelded = unwelded_prismatic_box_mesh(1.0, 0.1, 0.1);
    assert_eq!(
        unwelded.vertices.len() / 3,
        24,
        "unwelded fixture is 24 vertices"
    );
    assert_eq!(welded.vertices.len() / 3, 8, "welded fixture is 8 vertices");

    let v_welded = enclosed_volume_of_surface(&welded);
    let v_unwelded = enclosed_volume_of_surface(&unwelded);
    assert_rel(
        v_unwelded,
        0.01,
        F32_STORAGE_REL,
        "unwelded box encloses the same volume",
    );
    // Welding-independence itself is EXACT — it is a statement about the
    // summation, not about coordinate storage, so it gets no tolerance at all.
    assert_eq!(
        v_welded, v_unwelded,
        "welding must not change the enclosed volume — the per-triangle integral \
         against the origin never references vertex identity"
    );
}

#[test]
fn enclosed_volume_of_unwelded_unit_cube_is_exactly_one() {
    assert_eq!(
        enclosed_volume_of_surface(&unwelded_prismatic_box_mesh(1.0, 1.0, 1.0)),
        1.0
    );
}

// ---------------------------------------------------------------------------
// (e) the two ratio accessors
// ---------------------------------------------------------------------------

#[test]
fn ratio_accessors_match_their_definitions() {
    let vm = kuhn_box_volume_mesh(2.0, 1.0, 1.0);
    let r = tet_fill_report(&vm).expect("well-formed P1 tet mesh");

    assert_eq!(
        r.aabb_fill_fraction(),
        r.abs_volume_sum / r.aabb_volume,
        "aabb_fill_fraction is abs_volume_sum / aabb_volume"
    );
    assert_eq!(
        r.surface_match_ratio(0.25),
        r.abs_volume_sum / 0.25,
        "surface_match_ratio(v) is abs_volume_sum / v"
    );
    // And against the geometry-agnostic reference for this very box.
    let surface_volume = enclosed_volume_of_surface(&prismatic_box_mesh(2.0, 1.0, 1.0));
    assert_rel(
        r.surface_match_ratio(surface_volume),
        1.0,
        1e-6,
        "a complete decomposition matches the enclosed surface volume",
    );
}

// ---------------------------------------------------------------------------
// (f) strictly_interior_nodes
// ---------------------------------------------------------------------------

#[test]
fn corners_only_cube_has_no_strictly_interior_nodes() {
    let r = tet_fill_report(&kuhn_box_volume_mesh(1.0, 1.0, 1.0)).expect("well-formed");
    assert_eq!(
        r.strictly_interior_nodes, 0,
        "every node of the Kuhn cube lies on the AABB boundary in all three axes"
    );
}

#[test]
fn centre_node_is_the_only_strictly_interior_node() {
    let r = tet_fill_report(&centre_cone_unit_cube_volume_mesh()).expect("well-formed");
    assert_eq!(
        r.strictly_interior_nodes, 1,
        "only the body centre (0.5,0.5,0.5) is strictly inside on all three axes"
    );
}

#[test]
fn a_node_on_one_face_is_not_strictly_interior() {
    // Node 8 sits at (0.5, 0.5, 0.0) — interior in x and y, but ON the -Z face,
    // so it must NOT count as strictly interior.
    let mut vm = centre_cone_unit_cube_volume_mesh();
    vm.vertices[24] = 0.5;
    vm.vertices[25] = 0.5;
    vm.vertices[26] = 0.0;
    let r = tet_fill_report(&vm).expect("well-formed");
    assert_eq!(
        r.strictly_interior_nodes, 0,
        "strict interiority requires being off the boundary on ALL three axes"
    );
}

// ---------------------------------------------------------------------------
// (g) degenerate inputs — sentinels, never a panic or a NaN/inf
// ---------------------------------------------------------------------------

#[test]
fn hex_connectivity_yields_none() {
    let vm = VolumeMesh {
        vertices: vec![0.0; 24],
        connectivity: VolumeConnectivity::Hex {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7],
        },
        normals: None,
        boundary: None,
    };
    assert!(
        tet_fill_report(&vm).is_none(),
        "the report is tet-only; Hex connectivity must yield None"
    );
}

#[test]
fn wedge_connectivity_yields_none() {
    let vm = VolumeMesh {
        vertices: vec![0.0; 18],
        connectivity: VolumeConnectivity::Wedge {
            indices: vec![0, 1, 2, 3, 4, 5],
        },
        normals: None,
        boundary: None,
    };
    assert!(
        tet_fill_report(&vm).is_none(),
        "Wedge connectivity must yield None"
    );
}

#[test]
fn stride_mismatch_yields_none() {
    // 5 indices is not a whole number of P1 tets.
    let vm = p1_tet_mesh(vec![0.0; 12], vec![0, 1, 2, 3, 0]);
    assert!(
        tet_fill_report(&vm).is_none(),
        "a malformed (non-stride-4) index buffer must yield None, not a panic"
    );
}

#[test]
fn out_of_range_index_yields_none() {
    let vm = p1_tet_mesh(vec![0.0; 12], vec![0, 1, 2, 9]);
    assert!(
        tet_fill_report(&vm).is_none(),
        "an out-of-range node index must yield None, not an out-of-bounds panic"
    );
}

#[test]
fn empty_mesh_reports_zeroes_with_finite_ratios() {
    let vm = p1_tet_mesh(Vec::new(), Vec::new());
    let r = tet_fill_report(&vm).expect("an empty tet mesh is well-formed, just empty");
    assert_eq!(r.n_nodes, 0);
    assert_eq!(r.n_tets, 0);
    assert_eq!(r.abs_volume_sum, 0.0);
    assert_eq!(r.aabb_volume, 0.0);
    assert_eq!(r.inverted_tets, 0);
    assert_eq!(r.strictly_interior_nodes, 0);
    assert!(
        r.aabb_fill_fraction().is_finite(),
        "a zero denominator must produce a finite sentinel, not NaN/inf (got {})",
        r.aabb_fill_fraction()
    );
    assert!(r.surface_match_ratio(0.0).is_finite());
}

#[test]
fn zero_tets_with_orphan_nodes_reports_zeroes_with_finite_ratios() {
    // Nodes present but referenced by no element: the AABB is taken over
    // TET-REFERENCED nodes only, so it stays empty.
    let vm = p1_tet_mesh(vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0], Vec::new());
    let r = tet_fill_report(&vm).expect("well-formed, just elementless");
    assert_eq!(r.n_tets, 0);
    assert_eq!(r.aabb_volume, 0.0, "orphan nodes must not inflate the AABB");
    assert!(r.aabb_fill_fraction().is_finite());
}

#[test]
fn planar_node_set_gives_zero_extent_aabb_without_nan() {
    // Four coplanar nodes (z == 0 throughout): a degenerate, zero-volume tet
    // whose AABB has zero extent in z.
    let vm = p1_tet_mesh(
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0],
        vec![0, 1, 2, 3],
    );
    let r = tet_fill_report(&vm).expect("well-formed");
    assert_eq!(r.aabb_volume, 0.0, "a planar node set has zero AABB volume");
    assert_eq!(r.abs_volume_sum, 0.0, "a coplanar tet has zero volume");
    let f = r.aabb_fill_fraction();
    assert!(
        f.is_finite(),
        "0/0 must produce a finite sentinel, not NaN (got {f})"
    );
    assert!(r.surface_match_ratio(0.0).is_finite());
}

#[test]
fn surface_match_ratio_is_finite_for_a_zero_reference_volume() {
    let r = tet_fill_report(&kuhn_box_volume_mesh(1.0, 1.0, 1.0)).expect("well-formed");
    assert!(
        r.surface_match_ratio(0.0).is_finite(),
        "dividing a nonzero fill by a zero reference must still be finite"
    );
}

// ---------------------------------------------------------------------------
// (h) P2 — measured off the first four corner nodes
// ---------------------------------------------------------------------------

/// The reference tet as a 10-node P2 element: 4 corners followed by the 6 edge
/// midpoints in Gmsh canonical order (01, 12, 02, 03, 13, 23).
fn p2_reference_tet_mesh() -> VolumeMesh {
    VolumeMesh {
        #[rustfmt::skip]
        vertices: vec![
            0.0, 0.0, 0.0, // 0 corner
            1.0, 0.0, 0.0, // 1 corner
            0.0, 1.0, 0.0, // 2 corner
            0.0, 0.0, 1.0, // 3 corner
            0.5, 0.0, 0.0, // 4 mid 0-1
            0.5, 0.5, 0.0, // 5 mid 1-2
            0.0, 0.5, 0.0, // 6 mid 0-2
            0.0, 0.0, 0.5, // 7 mid 0-3
            0.5, 0.0, 0.5, // 8 mid 1-3
            0.0, 0.5, 0.5, // 9 mid 2-3
        ],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            order: ElementOrderTag::P2,
        },
        normals: None,
        boundary: None,
    }
}

#[test]
fn p2_element_is_measured_off_its_corner_nodes() {
    let r = tet_fill_report(&p2_reference_tet_mesh()).expect("well-formed P2 tet mesh");
    assert_eq!(
        r.n_tets, 1,
        "10 indices at stride 10 is exactly one P2 element"
    );
    assert_eq!(r.n_nodes, 10);
    assert_rel(
        r.abs_volume_sum,
        1.0 / 6.0,
        1e-12,
        "a P2 tet measures the same as its P1 counterpart",
    );
    assert_eq!(r.inverted_tets, 0);
}

#[test]
fn p2_and_p1_agree_on_the_same_corner_geometry() {
    let p2 = tet_fill_report(&p2_reference_tet_mesh()).expect("well-formed");
    let p1 = tet_fill_report(&p1_tet_mesh(
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        vec![0, 1, 2, 3],
    ))
    .expect("well-formed");
    assert_eq!(p2.abs_volume_sum, p1.abs_volume_sum);
    assert_eq!(p2.signed_volume_sum, p1.signed_volume_sum);
    assert_eq!(p2.aabb_volume, p1.aabb_volume);
}

#[test]
fn p2_stride_mismatch_yields_none() {
    let mut vm = p2_reference_tet_mesh();
    if let VolumeConnectivity::Tet { indices, .. } = &mut vm.connectivity {
        indices.pop(); // 9 indices — not a whole P2 element
    }
    assert!(
        tet_fill_report(&vm).is_none(),
        "a non-stride-10 P2 index buffer must yield None"
    );
}

// ---------------------------------------------------------------------------
// The report is a plain value type (derives are part of the contract).
// ---------------------------------------------------------------------------

#[test]
fn report_is_copy_and_comparable() {
    let a: TetFillReport = tet_fill_report(&kuhn_box_volume_mesh(1.0, 1.0, 1.0)).expect("ok");
    let b = a; // Copy, not a move-out
    assert_eq!(a, b);
}
