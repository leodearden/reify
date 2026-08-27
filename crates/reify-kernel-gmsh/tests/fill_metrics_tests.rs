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

mod common;
use common::{
    F32_STORAGE_REL, assert_rel, prismatic_box_mesh, unit_cube_mesh, unwelded_prismatic_box_mesh,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

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
// (b′) the DETECTION direction — an incomplete decomposition must read < 1.0
// ---------------------------------------------------------------------------
//
// Every fixture in (b) is a COMPLETE decomposition and reads fill 1.0, and the
// accessor tests in (e) only restate the formula. Neither direction pins that
// the metric actually FIRES. Without the two tests below, a refactor that
// normalised `abs_volume_sum` against the referenced-node hull, or accumulated
// the AABB over only the elements it visited, would keep every other assertion
// in this file green while making the metric report 1.0 unconditionally — and
// only the `has_gmsh`-gated `tests/volume_fill_fraction.rs` would notice, which
// does not run on stub hosts.

/// Keep the first `keep` elements of a P1 tet mesh, dropping the rest and
/// leaving the vertex buffer untouched.
///
/// This is the pre-#6200 signature in miniature: gmsh handed back a mesh whose
/// nodes spanned the whole box while its elements filled only part of it.
fn with_tets_truncated(vm: &VolumeMesh, keep: usize) -> VolumeMesh {
    let mut out = vm.clone();
    if let VolumeConnectivity::Tet { indices, .. } = &mut out.connectivity {
        indices.truncate(keep * 4);
    }
    out
}

#[test]
fn a_half_complete_decomposition_reads_fill_one_half() {
    // The Kuhn cube's first three tets reference nodes {0,1,2,3,5,6}, which
    // still span [0,1] on all three axes — so the AABB is the whole unit cube
    // while only 3 of the 6 unit-volume-sixths remain.
    let partial = with_tets_truncated(&kuhn_box_volume_mesh(1.0, 1.0, 1.0), 3);
    let r = tet_fill_report(&partial).expect("a truncated tet buffer is still well-formed");

    assert_eq!(r.n_tets, 3, "three of the six Kuhn tets survive");
    assert_eq!(r.n_nodes, 8, "the vertex buffer is untouched");
    assert_rel(
        r.aabb_volume,
        1.0,
        1e-12,
        "the surviving tets still span the full unit cube, so the AABB is unchanged",
    );
    assert_rel(
        r.abs_volume_sum,
        0.5,
        1e-12,
        "three Kuhn tets of volume 1/6 each",
    );
    assert_rel(
        r.aabb_fill_fraction(),
        0.5,
        1e-12,
        "aabb_fill_fraction must FIRE on an incomplete decomposition, not read 1.0",
    );
    assert_rel(
        r.surface_match_ratio(
            enclosed_volume_of_surface(&unit_cube_mesh()).expect("well-formed"),
        ),
        0.5,
        1e-12,
        "surface_match_ratio must fire on the same input, against the \
         geometry-agnostic reference",
    );
}

#[test]
fn dropping_one_tet_moves_the_fill_fraction_by_exactly_that_tet() {
    // Monotonicity, on the same fixture: each element removed costs exactly its
    // own share. A metric that saturated at 1.0, or that rescaled itself to the
    // surviving elements, could not produce this ladder.
    let full = kuhn_box_volume_mesh(1.0, 1.0, 1.0);
    for keep in [6usize, 5, 4] {
        let r = tet_fill_report(&with_tets_truncated(&full, keep)).expect("well-formed");
        assert_rel(
            r.aabb_fill_fraction(),
            keep as f64 / 6.0,
            1e-12,
            &format!("fill fraction with {keep} of 6 Kuhn tets"),
        );
    }
}

#[test]
fn an_orphan_node_outside_the_mesh_does_not_inflate_the_aabb() {
    // The AABB is documented as taken over TET-REFERENCED nodes only, "because
    // including [orphans] would inflate the box and understate the fill
    // fraction on an otherwise-correct mesh". The existing orphan test has ZERO
    // tets, so it exercises the `indices.is_empty()` early return rather than
    // the exclusion inside the AABB accumulation — this one has both.
    let mut vm = kuhn_box_volume_mesh(1.0, 1.0, 1.0);
    vm.vertices.extend_from_slice(&[10.0, 10.0, 10.0]);

    let r = tet_fill_report(&vm).expect("an unreferenced node is not a malformed mesh");
    assert_eq!(r.n_nodes, 9, "n_nodes counts the vertex buffer, orphans included");
    assert_eq!(r.n_tets, 6, "the element buffer is untouched");
    assert_rel(
        r.aabb_volume,
        1.0,
        1e-12,
        "a (10,10,10) orphan would make the AABB 1000x if it were counted",
    );
    assert_rel(
        r.aabb_fill_fraction(),
        1.0,
        1e-12,
        "excluding the orphan keeps an otherwise-complete mesh reading 1.0",
    );
    assert_eq!(
        r.strictly_interior_nodes, 0,
        "the orphan is unreferenced and outside the box; it must not be counted \
         as an interior node either"
    );
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
        Some(1.0),
        "the divergence-theorem sum is exact for polyhedra with integral coordinates"
    );
}

#[test]
fn enclosed_volume_of_thin_box_matches_closed_form() {
    assert_rel(
        enclosed_volume_of_surface(&prismatic_box_mesh(1.0, 0.1, 0.1)).expect("well-formed"),
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

    let v_welded = enclosed_volume_of_surface(&welded).expect("well-formed");
    let v_unwelded = enclosed_volume_of_surface(&unwelded).expect("well-formed");
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
        Some(1.0)
    );
}

/// A malformed surface must be `None`, NOT a quiet partial sum.
///
/// This is the asymmetry the reviewer of #6200 flagged: `enclosed_volume_of_surface`
/// used to `continue` past any triangle carrying an out-of-range index. Because
/// its result is the REFERENCE that `surface_match_ratio` divides by, dropping
/// triangles shrinks the reference in the same direction as an under-filled
/// volume mesh — so a real under-fill could divide out to ≈ 1.0 and read as
/// healthy. `tet_fill_report` already returned `None` on the same class of
/// input; these tests pin that the two are now symmetric.
#[test]
fn out_of_range_triangle_index_yields_none() {
    let mut m = unit_cube_mesh();
    m.indices[0] = 99;
    assert_eq!(
        enclosed_volume_of_surface(&m),
        None,
        "an out-of-range triangle index must be None, not a sum over the \
         triangles that happened to be in range"
    );
}

#[test]
fn ragged_triangle_buffer_yields_none() {
    let mut m = unit_cube_mesh();
    m.indices.pop(); // 35 indices — not a whole number of triangles
    assert_eq!(
        enclosed_volume_of_surface(&m),
        None,
        "a ragged index buffer must be None; chunks_exact(3) would otherwise \
         silently drop the tail"
    );
}

#[test]
fn ragged_vertex_buffer_yields_none() {
    let mut m = unit_cube_mesh();
    m.vertices.pop(); // 23 floats — not a whole number of XYZ triples
    assert_eq!(enclosed_volume_of_surface(&m), None);
}

/// An empty surface is well-formed, just empty — `Some(0.0)`, not `None`.
#[test]
fn empty_surface_encloses_zero_volume() {
    let m = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        normals: None,
    };
    assert_eq!(enclosed_volume_of_surface(&m), Some(0.0));
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
    let surface_volume =
        enclosed_volume_of_surface(&prismatic_box_mesh(2.0, 1.0, 1.0)).expect("well-formed");
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
