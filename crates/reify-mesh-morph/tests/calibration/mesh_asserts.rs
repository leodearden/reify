//! General-purpose `VolumeMesh` topology assertions used by the PRD task
//! #13 calibration suite: boundary-manifold conformity (edge-degree,
//! single-component connectivity, Euler characteristic) and per-tet signed
//! volume (handedness). Neither depends on any specific fixture, so they
//! live in their own `#[path = …] mod mesh_asserts;` module (see
//! `calibration.rs`) rather than inline in the test file that happens to
//! exercise them first.

use std::collections::{HashMap, HashSet};

/// Asserts that `mesh` is a conforming simplicial complex: every interior
/// triangular face is shared by AT MOST two tets, and the boundary — the
/// faces that occur exactly once — is a SINGLE closed, orientable manifold
/// component whose Euler characteristic equals `expected_chi`.
///
/// The boundary checks (edge-degree, single-component connectivity, and
/// Euler characteristic) are the load-bearing ones: a face-multiplicity
/// check alone ("no face is shared by more than two tets") is GREEN on a
/// non-conforming mesh — e.g. measured on main for `bracket` at n=4, the
/// face-occurrence histogram is `{1: 636, 2: 2322}` — nothing occurs 3+
/// times. A block-interface non-conformity defect produces EXTRA
/// once-occurring faces, not over-shared ones: an interface quad is
/// bisected along one diagonal by the block on one side and along the
/// OTHER diagonal by the block on the other side, so the two
/// triangulations carry different sorted vertex keys, never cancel, and
/// both halves leak into the "boundary" this function extracts. Boundary
/// edge-degree (every boundary edge must be shared by exactly two boundary
/// faces) and Euler characteristic (`V - E + F`, which must match the
/// fixture's genus) are what catch that. Single-component connectivity is
/// checked too, because degree-2-everywhere plus a matching chi is still
/// satisfiable by a disjoint union (e.g. sphere ⊔ torus has
/// chi = 2 + 0 = 2). The face-multiplicity check below is included as
/// well, as a cheap guard against a DIFFERENT future regression (a
/// generator bug that over-shares a face 3+ times) — it would not by
/// itself have caught the defect this task repairs, which is why it is not
/// the only check here.
pub fn assert_boundary_is_conforming_manifold(
    mesh: &reify_ir::VolumeMesh,
    fixture_name: &str,
    case_desc: &str,
    expected_chi: i64,
) {
    let tets = mesh.tet_indices().unwrap();
    assert_eq!(
        tets.len() % 4,
        0,
        "{fixture_name} {case_desc}: tet_indices length {} is not a multiple of 4 — \
         chunks_exact(4) below would silently drop a trailing partial element",
        tets.len()
    );
    assert!(
        !tets.is_empty(),
        "{fixture_name} {case_desc}: mesh has no tets"
    );

    // Face table keyed on the SORTED vertex triple, counting occurrences
    // across every tet's four faces (omit-one-vertex: {0,1,2} {0,1,3}
    // {0,2,3} {1,2,3}).
    let mut face_count: HashMap<[u32; 3], usize> = HashMap::new();
    for tet in tets.chunks_exact(4) {
        let (t0, t1, t2, t3) = (tet[0], tet[1], tet[2], tet[3]);
        for mut face in [[t0, t1, t2], [t0, t1, t3], [t0, t2, t3], [t1, t2, t3]] {
            face.sort_unstable();
            *face_count.entry(face).or_insert(0) += 1;
        }
    }

    // Cheap guard against a DIFFERENT future regression than the one this
    // task repairs: no face may be shared by more than two tets (an
    // over-shared interior face). See the doc comment above for why this
    // check alone is insufficient to catch a block-interface non-conformity.
    let mut face_count_histogram: HashMap<usize, usize> = HashMap::new();
    for &count in face_count.values() {
        *face_count_histogram.entry(count).or_insert(0) += 1;
    }
    assert!(
        face_count.values().all(|&count| count <= 2),
        "{fixture_name} {case_desc}: a face is shared by more than two tets — \
         face-occurrence histogram {face_count_histogram:?} (every face may occur at most \
         twice: once if boundary, twice if interior)"
    );

    let boundary_faces: Vec<[u32; 3]> = face_count
        .into_iter()
        .filter(|&(_, count)| count == 1)
        .map(|(face, _)| face)
        .collect();
    assert!(
        !boundary_faces.is_empty(),
        "{fixture_name} {case_desc}: mesh has no boundary faces — the manifold checks below \
         would pass vacuously"
    );

    // (a) Closed manifold: every boundary edge (undirected, keyed (min,
    // max)) must have degree exactly 2.
    let mut edge_degree: HashMap<(u32, u32), usize> = HashMap::new();
    let mut boundary_vertices: HashSet<u32> = HashSet::new();
    for face in &boundary_faces {
        boundary_vertices.extend(face.iter().copied());
        for &(i, j) in &[(0usize, 1usize), (0, 2), (1, 2)] {
            let (a, b) = (face[i], face[j]);
            let key = if a < b { (a, b) } else { (b, a) };
            *edge_degree.entry(key).or_insert(0) += 1;
        }
    }
    let mut degree_histogram: HashMap<usize, usize> = HashMap::new();
    for &deg in edge_degree.values() {
        *degree_histogram.entry(deg).or_insert(0) += 1;
    }
    assert!(
        edge_degree.values().all(|&deg| deg == 2),
        "{fixture_name} {case_desc}: boundary is not a closed manifold — edge-degree \
         histogram {degree_histogram:?} (every boundary edge must be degree 2); a non-2 \
         degree means an interface quad's two triangulations failed to cancel (bisected \
         along different diagonals from each side)"
    );

    // (b) Single component: degree-2-everywhere plus a matching Euler
    // characteristic is still satisfiable by a disjoint union of manifolds
    // (e.g. sphere ⊔ torus has chi = 2 + 0 = 2), so walk the edge adjacency
    // from an arbitrary boundary vertex and confirm every boundary vertex is
    // reachable.
    let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in edge_degree.keys() {
        adjacency.entry(a).or_default().push(b);
        adjacency.entry(b).or_default().push(a);
    }
    let mut visited: HashSet<u32> = HashSet::new();
    if let Some(&start) = boundary_vertices.iter().next() {
        let mut stack = vec![start];
        visited.insert(start);
        while let Some(v) = stack.pop() {
            if let Some(neighbors) = adjacency.get(&v) {
                for &nb in neighbors {
                    if visited.insert(nb) {
                        stack.push(nb);
                    }
                }
            }
        }
    }
    assert_eq!(
        visited.len(),
        boundary_vertices.len(),
        "{fixture_name} {case_desc}: boundary is not a single connected component — reached \
         {} of {} boundary vertices from an arbitrary start (a disjoint union, e.g. \
         sphere ⊔ torus, can satisfy degree-2-everywhere and the expected Euler \
         characteristic simultaneously, so this check is required in addition to edge-degree \
         and chi)",
        visited.len(),
        boundary_vertices.len()
    );

    // (c) Euler characteristic: V - E + F must match the fixture's genus.
    let v = boundary_vertices.len() as i64;
    let e = edge_degree.len() as i64;
    let f = boundary_faces.len() as i64;
    let chi = v - e + f;
    assert_eq!(
        chi, expected_chi,
        "{fixture_name} {case_desc}: boundary Euler characteristic V-E+F = {v}-{e}+{f} = \
         {chi}, expected {expected_chi}"
    );
}

/// Asserts that every tet in `mesh` has strictly positive signed volume,
/// i.e. a positive scalar triple product `e1 · (e2 × e3)` from corner 0
/// (`e_i = p_i - p_0`) — the same right-handedness convention documented on
/// `element_scaled_jacobian`'s `CORNER_EDGE_INDICES[0] = [1, 2, 3]` in
/// `reify-mesh-morph`'s `quality.rs` ("Verified on the canonical unit tet …
/// each corner determinant = +1").
///
/// The repair applied to `bracket`'s block interfaces (step-2) is
/// orientation-sensitive: the polar zone's corner ordering is rotated a
/// half turn and arm 2's wedge tets have their first two vertices swapped,
/// either of which flips signed volume if done wrong. The only other guard
/// against an inverted tet,
/// `bracket_fixture_returns_valid_p1_mesh_with_fillet_radius_respected_and_positive_volume_tets`,
/// exercises exactly one configuration (n=4, fillet_radius=0.1), so this
/// helper is swept across the same n/radius resolutions the conformity
/// check above sweeps, pinning conformity and handedness together.
pub fn assert_all_tets_have_positive_signed_volume(
    mesh: &reify_ir::VolumeMesh,
    fixture_name: &str,
    case_desc: &str,
) {
    let tets = mesh.tet_indices().unwrap();
    for (i, tet) in tets.chunks_exact(4).enumerate() {
        let p: Vec<[f64; 3]> = tet
            .iter()
            .map(|&idx| {
                let v = idx as usize;
                [
                    mesh.vertices[v * 3] as f64,
                    mesh.vertices[v * 3 + 1] as f64,
                    mesh.vertices[v * 3 + 2] as f64,
                ]
            })
            .collect();
        let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let e3 = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
        let cross = [
            e2[1] * e3[2] - e2[2] * e3[1],
            e2[2] * e3[0] - e2[0] * e3[2],
            e2[0] * e3[1] - e2[1] * e3[0],
        ];
        let det = e1[0] * cross[0] + e1[1] * cross[1] + e1[2] * cross[2];
        assert!(
            det > 0.0,
            "{fixture_name} {case_desc}: tet #{i} {tet:?} has non-positive signed volume \
             (scalar triple product = {det}) — an inverted or degenerate element"
        );
    }
}
