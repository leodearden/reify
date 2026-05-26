//! Tests for the NodeAttachment producer: `EntityAttribution` construction and
//! (when `has_gmsh`) the `mesh_surface_to_volume_with_attribution` entity-
//! membership attribution pipeline (PRD `mesh-morphing-phase-2.md` §3.3 task γ).
//!
//! File-level gate: requires the `mesh-morph` feature.  The self-dev-dep in
//! `Cargo.toml` activates it for all integration test binaries automatically.
#![cfg(feature = "mesh-morph")]

use reify_kernel_gmsh::mesh_boundary::EntityAttribution;
use reify_types::{GeometryHandleId, Mesh, NodeAttachment};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn h(n: u64) -> GeometryHandleId {
    GeometryHandleId(n)
}

/// Build a 2×2-subdivided unit cube (side 1.0, centred at origin):
/// 8 corners + 12 edge midpoints + 6 face centres = 26 unique vertices, 48 triangles.
/// Shared with gmsh_classify_diagnostics.rs (duplicated — separate compilation units).
fn subdivided_unit_cube_surface() -> Mesh {
    #[rustfmt::skip]
    let corners: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5], [ 0.5, -0.5, -0.5],
        [-0.5,  0.5, -0.5], [ 0.5,  0.5, -0.5],
        [-0.5, -0.5,  0.5], [ 0.5, -0.5,  0.5],
        [-0.5,  0.5,  0.5], [ 0.5,  0.5,  0.5],
    ];
    #[rustfmt::skip]
    let edges: [[f32; 3]; 12] = [
        [ 0.0, -0.5, -0.5], [-0.5,  0.0, -0.5], [ 0.5,  0.0, -0.5], [ 0.0,  0.5, -0.5],
        [ 0.0, -0.5,  0.5], [-0.5,  0.0,  0.5], [ 0.5,  0.0,  0.5], [ 0.0,  0.5,  0.5],
        [-0.5, -0.5,  0.0], [ 0.5, -0.5,  0.0], [-0.5,  0.5,  0.0], [ 0.5,  0.5,  0.0],
    ];
    #[rustfmt::skip]
    let face_centers: [[f32; 3]; 6] = [
        [ 0.0,  0.0, -0.5], [ 0.0,  0.0,  0.5],
        [ 0.0, -0.5,  0.0], [ 0.0,  0.5,  0.0],
        [-0.5,  0.0,  0.0], [ 0.5,  0.0,  0.0],
    ];
    let mut vertices: Vec<f32> = Vec::with_capacity(26 * 3);
    for c in &corners { vertices.extend_from_slice(c); }
    for e in &edges   { vertices.extend_from_slice(e); }
    for f in &face_centers { vertices.extend_from_slice(f); }
    #[rustfmt::skip]
    let indices: Vec<u32> = vec![
        // Bottom (z=-0.5): vertex indices 8=edge[0], 9=edge[1], 10=edge[2], 11=edge[3], 20=fc[0]
        0, 9,20,  0,20, 8,  8,20,10,  8,10, 1,
        9, 2,11,  9,11,20, 20,11, 3, 20, 3,10,
        // Top (z=0.5)
        4,12,21,  4,21,13, 12, 5,14, 12,14,21,
       13,21,15, 13,15, 6, 21,14, 7, 21, 7,15,
        // Front (y=-0.5)
        0, 8,22,  0,22,16,  8, 1,17,  8,17,22,
       16,22,12, 16,12, 4, 22,17, 5, 22, 5,12,
        // Back (y=0.5)
        2,18,23,  2,23,11, 11,23,19, 11,19, 3,
       18, 6,15, 18,15,23, 23,15, 7, 23, 7,19,
        // Left (x=-0.5)
        0,16,24,  0,24, 9,  9,24,18,  9,18, 2,
       16, 4,13, 16,13,24, 24,13, 6, 24, 6,18,
        // Right (x=0.5)
        1,10,25,  1,25,17, 10, 3,19, 10,19,25,
       17,25,14, 17,14, 5, 25,19, 7, 25, 7,14,
    ];
    Mesh { vertices, indices, normals: None }
}

// ---------------------------------------------------------------------------
// EntityAttribution construction
// ---------------------------------------------------------------------------

/// Verify that `EntityAttribution` can be constructed with empty entity lists.
#[test]
fn entity_attribution_can_be_constructed_with_empty_lists() {
    let ea = EntityAttribution {
        faces: vec![],
        edges: vec![],
        vertices: vec![],
        match_tolerance: 0.0,
    };
    assert_eq!(ea.faces.len(), 0);
    assert_eq!(ea.edges.len(), 0);
    assert_eq!(ea.vertices.len(), 0);
    assert_eq!(ea.match_tolerance, 0.0);
}

/// Verify that `EntityAttribution` stores face/edge/vertex anchors correctly.
#[test]
fn entity_attribution_stores_anchor_positions_and_handles() {
    let ea = EntityAttribution {
        faces:    vec![(h(101), [0.0, 0.0, -0.5])],
        edges:    vec![(h(201), [0.0, -0.5, -0.5])],
        vertices: vec![(h(301), [-0.5, -0.5, -0.5])],
        match_tolerance: 1e-3,
    };
    assert_eq!(ea.faces[0].0, h(101));
    assert_eq!(ea.faces[0].1, [0.0, 0.0, -0.5]);
    assert_eq!(ea.edges[0].0, h(201));
    assert_eq!(ea.vertices[0].0, h(301));
    assert_eq!(ea.match_tolerance, 1e-3);
}

// ---------------------------------------------------------------------------
// Full integration: mesh_surface_to_volume_with_attribution on a unit cube
// ---------------------------------------------------------------------------

/// Full integration test: `mesh_surface_to_volume_with_attribution` on a unit cube.
///
/// Verifies the PRD §7.1 user-observable signals:
///   1. `boundary.len() > 0` — some surface nodes are attributed.
///   2. `boundary.len() < total_nodes` — interior tet nodes are absent.
///   3. All 8 cube corners attach as `OnVertex` with 8 *distinct* handles, and
///      no other handle appears as `OnVertex` (spurious gmsh seam points fall
///      outside `match_tolerance` of every corner anchor).
#[cfg(has_gmsh)]
#[test]
fn mesh_surface_to_volume_with_attribution_attributes_surface_nodes_by_brep_entity() {
    use reify_kernel_gmsh::mesh_boundary::mesh_surface_to_volume_with_attribution;
    use reify_kernel_gmsh::MeshingOptions;
    use reify_types::ElementOrderTag;
    use std::collections::BTreeSet;

    let surface = subdivided_unit_cube_surface();

    // Unit cube: 6 B-rep faces, 12 B-rep edges, 8 B-rep vertices.
    // Anchor positions match gmsh classify_surfaces output for the unit cube
    // at the producer's FRAC_PI_4 feature angle. Tolerance 0.3 is generous
    // relative to the cube's unit side length yet rejects gmsh's spurious
    // edge-midpoint seam points (which sit 0.5 from any corner).
    let attribution = EntityAttribution {
        faces: vec![
            (h(101), [ 0.0,  0.0, -0.5]),  // bottom face
            (h(102), [ 0.0,  0.0,  0.5]),  // top face
            (h(103), [ 0.0, -0.5,  0.0]),  // front face
            (h(104), [ 0.0,  0.5,  0.0]),  // back face
            (h(105), [-0.5,  0.0,  0.0]),  // left face
            (h(106), [ 0.5,  0.0,  0.0]),  // right face
        ],
        edges: vec![
            (h(201), [ 0.0, -0.5, -0.5]),  // bottom-front
            (h(202), [-0.5,  0.0, -0.5]),  // bottom-left
            (h(203), [ 0.5,  0.0, -0.5]),  // bottom-right
            (h(204), [ 0.0,  0.5, -0.5]),  // bottom-back
            (h(205), [ 0.0, -0.5,  0.5]),  // top-front
            (h(206), [-0.5,  0.0,  0.5]),  // top-left
            (h(207), [ 0.5,  0.0,  0.5]),  // top-right
            (h(208), [ 0.0,  0.5,  0.5]),  // top-back
            (h(209), [-0.5, -0.5,  0.0]),  // left-front vertical
            (h(210), [ 0.5, -0.5,  0.0]),  // right-front vertical
            (h(211), [-0.5,  0.5,  0.0]),  // left-back vertical
            (h(212), [ 0.5,  0.5,  0.0]),  // right-back vertical
        ],
        vertices: vec![
            (h(301), [-0.5, -0.5, -0.5]),
            (h(302), [ 0.5, -0.5, -0.5]),
            (h(303), [-0.5,  0.5, -0.5]),
            (h(304), [ 0.5,  0.5, -0.5]),
            (h(305), [-0.5, -0.5,  0.5]),
            (h(306), [ 0.5, -0.5,  0.5]),
            (h(307), [-0.5,  0.5,  0.5]),
            (h(308), [ 0.5,  0.5,  0.5]),
        ],
        match_tolerance: 0.3,
    };

    let report = mesh_surface_to_volume_with_attribution(
        &surface,
        &MeshingOptions { mesh_size: None, deterministic: true, ..Default::default() },
        ElementOrderTag::P1,
        None,
        None,
        None,
        &attribution,
    )
    .expect("mesh_surface_to_volume_with_attribution must succeed on a closed unit cube");

    // PRD §7.1 assertion 1: some surface nodes are attributed.
    assert!(
        !report.boundary.is_empty(),
        "BoundaryAssociation must be non-empty for a unit-cube input"
    );

    // PRD §7.1 assertion 2: interior tet nodes are absent.
    let total_nodes = report.volume.vertices.len() / 3;
    assert!(
        report.boundary.len() < total_nodes,
        "BoundaryAssociation contains {len} entries but volume has {total} nodes; \
         interior tet nodes must be absent",
        len = report.boundary.len(),
        total = total_nodes,
    );

    // PRD §7.1 assertion 3 (tightened during task-3591 unblock): exactly the 8
    // cube-corner handles 301..=308 appear as OnVertex, each distinct. gmsh's
    // classify_surfaces emits spurious extra dim-0 entities at vertical-edge
    // midpoints, but those sit 0.5 from any corner anchor (> match_tolerance
    // 0.3) and so must NOT produce attributions. This pins the PRD §7.1
    // "8 corner nodes attach as OnVertex with distinct handles per corner"
    // guarantee. Edge/face attribution precision is validated separately
    // (follow-up task — see task-3591 unblock notes).
    let on_vertex_handles: BTreeSet<u64> = report
        .boundary
        .iter()
        .filter_map(|(_, a)| match a {
            NodeAttachment::OnVertex(GeometryHandleId(id)) => Some(id),
            _ => None,
        })
        .collect();
    let expected_corners: BTreeSet<u64> = (301..=308).collect();
    assert_eq!(
        on_vertex_handles, expected_corners,
        "expected exactly the 8 cube-corner handles 301..=308 as OnVertex, \
         got {on_vertex_handles:?}"
    );
}

// ---------------------------------------------------------------------------
// Edge/face attribution geometric correctness (characterization, has_gmsh)
// ---------------------------------------------------------------------------

/// Edge/face attribution geometric correctness: every attributed boundary
/// node must lie on the geometric locus of its attributed B-rep handle.
///
/// For the unit cube: faces fix one coordinate axis at ±0.5, edges fix two
/// axes, vertices all three.  The locus predicate: for each axis `i` where
/// `|anchor[i]| > 0.25` (i.e., the anchor sits at ≈ ±0.5), the attributed
/// node's `coord[i]` must be within 1e-3 of `anchor[i]`.
///
/// Characterization test: expected GREEN on the current producer (gmsh's
/// classify_surfaces sub-entities are genuine B-rep entities).  If RED, a
/// real mis-attribution has been found (e.g. a gmsh dim-1 seam spanning a
/// face matched to an edge handle) — investigate matching; do NOT relax the
/// locus predicate to hide the failure.
///
/// Also pins the complete distinct handle sets: OnEdge ≡ {201..=212} and
/// OnFace ≡ {101..=106}.  A gmsh version change that produces a different
/// topology should be reflected here with a re-characterization comment
/// (task 3763).
#[cfg(has_gmsh)]
#[test]
fn attributed_boundary_nodes_lie_on_locus_of_attributed_handle() {
    use reify_kernel_gmsh::mesh_boundary::mesh_surface_to_volume_with_attribution;
    use reify_kernel_gmsh::MeshingOptions;
    use reify_types::ElementOrderTag;
    use std::collections::{BTreeSet, HashMap};

    let surface = subdivided_unit_cube_surface();

    // Same attribution as the signal test.
    let attribution = EntityAttribution {
        faces: vec![
            (h(101), [ 0.0,  0.0, -0.5]),
            (h(102), [ 0.0,  0.0,  0.5]),
            (h(103), [ 0.0, -0.5,  0.0]),
            (h(104), [ 0.0,  0.5,  0.0]),
            (h(105), [-0.5,  0.0,  0.0]),
            (h(106), [ 0.5,  0.0,  0.0]),
        ],
        edges: vec![
            (h(201), [ 0.0, -0.5, -0.5]),
            (h(202), [-0.5,  0.0, -0.5]),
            (h(203), [ 0.5,  0.0, -0.5]),
            (h(204), [ 0.0,  0.5, -0.5]),
            (h(205), [ 0.0, -0.5,  0.5]),
            (h(206), [-0.5,  0.0,  0.5]),
            (h(207), [ 0.5,  0.0,  0.5]),
            (h(208), [ 0.0,  0.5,  0.5]),
            (h(209), [-0.5, -0.5,  0.0]),
            (h(210), [ 0.5, -0.5,  0.0]),
            (h(211), [-0.5,  0.5,  0.0]),
            (h(212), [ 0.5,  0.5,  0.0]),
        ],
        vertices: vec![
            (h(301), [-0.5, -0.5, -0.5]),
            (h(302), [ 0.5, -0.5, -0.5]),
            (h(303), [-0.5,  0.5, -0.5]),
            (h(304), [ 0.5,  0.5, -0.5]),
            (h(305), [-0.5, -0.5,  0.5]),
            (h(306), [ 0.5, -0.5,  0.5]),
            (h(307), [-0.5,  0.5,  0.5]),
            (h(308), [ 0.5,  0.5,  0.5]),
        ],
        match_tolerance: 0.3,
    };

    // Build handle → anchor lookup for the locus predicate.
    let mut handle_to_anchor: HashMap<u64, [f64; 3]> = HashMap::new();
    for (hid, anchor) in attribution
        .faces
        .iter()
        .chain(&attribution.edges)
        .chain(&attribution.vertices)
    {
        handle_to_anchor.insert(hid.0, *anchor);
    }

    let report = mesh_surface_to_volume_with_attribution(
        &surface,
        &MeshingOptions { mesh_size: None, deterministic: true, ..Default::default() },
        ElementOrderTag::P1,
        None,
        None,
        None,
        &attribution,
    )
    .expect("mesh_surface_to_volume_with_attribution must succeed on a closed unit cube");

    let verts = &report.volume.vertices;
    let mut on_edge_handles: BTreeSet<u64> = BTreeSet::new();
    let mut on_face_handles: BTreeSet<u64> = BTreeSet::new();

    for (idx, attachment) in report.boundary.iter() {
        // Extract node position from the volume mesh (f32 → f64 for comparison).
        let node = [
            verts[idx as usize * 3] as f64,
            verts[idx as usize * 3 + 1] as f64,
            verts[idx as usize * 3 + 2] as f64,
        ];

        let handle_id = match attachment {
            NodeAttachment::OnFace(hid) => {
                on_face_handles.insert(hid.0);
                hid.0
            }
            NodeAttachment::OnEdge(hid) => {
                on_edge_handles.insert(hid.0);
                hid.0
            }
            NodeAttachment::OnVertex(hid) => hid.0,
            _ => continue,
        };

        let anchor = *handle_to_anchor.get(&handle_id).unwrap_or_else(|| {
            panic!("node idx={idx} attributed to unrecognised handle {handle_id}");
        });

        // Locus predicate: for each axis fixed by this handle (|anchor| ≈ 0.5),
        // the node must lie on that axis value within 1e-3.
        for i in 0..3 {
            if anchor[i].abs() > 0.25 {
                let diff = (node[i] - anchor[i]).abs();
                assert!(
                    diff < 1e-3,
                    "node idx={idx} attributed to handle {handle_id} \
                     (anchor={anchor:?}): axis {i} \
                     node[{i}]={:.6} anchor[{i}]={:.6} diff={:.6} > 1e-3. \
                     Node does not lie on the attributed handle's geometric locus. \
                     This is a real mis-attribution — investigate matching or escalate; \
                     do NOT relax this predicate (task 3763).",
                    node[i],
                    anchor[i],
                    diff,
                );
            }
        }
    }

    // Pin complete distinct handle sets (characterization: current producer /
    // current gmsh version).  If a future gmsh version produces a different
    // topology, update these expected sets and add a re-characterization comment.
    let expected_edges: BTreeSet<u64> = (201..=212).collect();
    let expected_faces: BTreeSet<u64> = (101..=106).collect();
    assert_eq!(
        on_edge_handles, expected_edges,
        "expected OnEdge handles {{201..=212}}, got {on_edge_handles:?} (task 3763)"
    );
    assert_eq!(
        on_face_handles, expected_faces,
        "expected OnFace handles {{101..=106}}, got {on_face_handles:?} (task 3763)"
    );
}

// ---------------------------------------------------------------------------
// suggested_match_tolerance (pure — no gmsh, no cfg gate)
// ---------------------------------------------------------------------------

/// `suggested_match_tolerance` returns 0.5 × the minimum same-dim pairwise
/// anchor distance. Two face anchors separated by 1.0 → 0.5.
/// A single edge anchor (no pair) and empty vertices do not constrain the
/// minimum, so the result is driven entirely by the face pair.
#[test]
fn suggested_match_tolerance_two_faces_returns_half_min_distance() {
    let ea = EntityAttribution {
        faces: vec![(h(1), [0.0, 0.0, 0.0]), (h(2), [1.0, 0.0, 0.0])],
        edges: vec![(h(3), [0.0, 0.0, 0.0])], // single — no same-dim pair
        vertices: vec![],
        match_tolerance: 0.0,
    };
    let tol = ea.suggested_match_tolerance();
    let expected = 0.5_f64; // 0.5 × min-same-dim-pairwise (1.0 from faces)
    assert!(
        (tol - expected).abs() < 1e-12,
        "expected {expected}, got {tol}"
    );
}

/// `suggested_match_tolerance` returns `f64::INFINITY` when no dimension
/// has ≥ 2 anchors (no same-dim ambiguity is possible).
#[test]
fn suggested_match_tolerance_all_dims_single_anchor_returns_infinity() {
    let ea = EntityAttribution {
        faces:    vec![(h(1), [0.0, 0.0, 0.0])],
        edges:    vec![(h(2), [1.0, 0.0, 0.0])],
        vertices: vec![(h(3), [2.0, 0.0, 0.0])],
        match_tolerance: 0.0,
    };
    assert!(
        ea.suggested_match_tolerance().is_infinite(),
        "expected INFINITY when no dim has ≥ 2 anchors"
    );
}

/// `suggested_match_tolerance` returns `f64::INFINITY` when every dimension
/// is empty.
#[test]
fn suggested_match_tolerance_empty_returns_infinity() {
    let ea = EntityAttribution {
        faces:    vec![],
        edges:    vec![],
        vertices: vec![],
        match_tolerance: 0.0,
    };
    assert!(
        ea.suggested_match_tolerance().is_infinite(),
        "expected INFINITY for empty EntityAttribution"
    );
}

/// `suggested_match_tolerance` for the unit-cube anchor set yields ≈ 0.354
/// (= 0.5 × √0.5), confirming that the hand-picked 0.3 is within the safe
/// bound (no mis-assignment possible on the cube given 0.3 < 0.354).
#[test]
fn suggested_match_tolerance_unit_cube_validates_hand_picked_0_3() {
    let ea = EntityAttribution {
        faces: vec![
            (h(101), [ 0.0,  0.0, -0.5]),
            (h(102), [ 0.0,  0.0,  0.5]),
            (h(103), [ 0.0, -0.5,  0.0]),
            (h(104), [ 0.0,  0.5,  0.0]),
            (h(105), [-0.5,  0.0,  0.0]),
            (h(106), [ 0.5,  0.0,  0.0]),
        ],
        edges: vec![
            (h(201), [ 0.0, -0.5, -0.5]),
            (h(202), [-0.5,  0.0, -0.5]),
            (h(203), [ 0.5,  0.0, -0.5]),
            (h(204), [ 0.0,  0.5, -0.5]),
            (h(205), [ 0.0, -0.5,  0.5]),
            (h(206), [-0.5,  0.0,  0.5]),
            (h(207), [ 0.5,  0.0,  0.5]),
            (h(208), [ 0.0,  0.5,  0.5]),
            (h(209), [-0.5, -0.5,  0.0]),
            (h(210), [ 0.5, -0.5,  0.0]),
            (h(211), [-0.5,  0.5,  0.0]),
            (h(212), [ 0.5,  0.5,  0.0]),
        ],
        vertices: vec![
            (h(301), [-0.5, -0.5, -0.5]),
            (h(302), [ 0.5, -0.5, -0.5]),
            (h(303), [-0.5,  0.5, -0.5]),
            (h(304), [ 0.5,  0.5, -0.5]),
            (h(305), [-0.5, -0.5,  0.5]),
            (h(306), [ 0.5, -0.5,  0.5]),
            (h(307), [-0.5,  0.5,  0.5]),
            (h(308), [ 0.5,  0.5,  0.5]),
        ],
        match_tolerance: 0.3,
    };
    let tol = ea.suggested_match_tolerance();
    // Adjacent face-centre pair distance = √0.5 ≈ 0.7071; same for edge pairs.
    // Vertex pair min = 1.0. Overall min = √0.5 → 0.5 × √0.5 ≈ 0.35355.
    let expected = 0.5 * 0.5_f64.sqrt();
    assert!(
        (tol - expected).abs() < 1e-6,
        "expected ≈{expected:.6} (0.5 × √0.5), got {tol:.6}"
    );
    // 0.3 must be within the safe bound
    assert!(
        0.3 < tol,
        "hand-picked tolerance 0.3 must be < suggested_match_tolerance ({tol:.6})"
    );
}
