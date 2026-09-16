//! Boundary-surface extraction for tetrahedral meshes.
//!
//! Sibling of `fixtures.rs` rather than part of it: the fixtures module
//! *generates* procedural geometry, while this one *reads* a finished
//! [`VolumeMesh`] and derives its boundary — a general tet-mesh topology
//! operation with no fixture knowledge in it. Only the binaries that
//! actually need a surface `#[path]`-include this file, so an unused
//! function here shows up as dead code instead of being absorbed by
//! `fixtures.rs`'s blanket `#![allow(dead_code)]`.

use reify_ir::{ElementOrderTag, Mesh, VolumeMesh};
use std::collections::HashMap;

/// Extract the outward-wound boundary triangle surface of a tetrahedral
/// [`VolumeMesh`], compacted onto only the vertices it references.
///
/// A face of a tet mesh is on the boundary exactly when it is shared by no
/// second tet, so the extractor keys every tet face on its *sorted* vertex
/// triple, counts occurrences, and keeps the singletons. Each kept face is
/// then wound so its normal points away from the opposite vertex of the tet
/// that owns it — i.e. out of the solid. The orientation is computed from
/// the actual coordinates rather than assumed from index order, so a
/// negatively-oriented tet in the input still yields an outward face.
///
/// Finally the survivors are compacted: an old -> new index remap is built
/// over exactly the referenced vertices, so the ~interior nodes (the bulk of
/// the table at the 100K scale) are not carried through. Handing those to a
/// volume mesher would be pure waste.
///
/// The result satisfies [`reify_ir::Mesh::validate`] — finite, index-valid,
/// non-degenerate, closed and consistently wound — which is exactly the
/// producer-obligation set a volume mesher's preflight demands.
///
/// `normals` is `None`: the consumer is a mesher that derives its own.
///
/// ## Panics
///
/// Panics if `mesh` does not carry tet connectivity, or carries it at an
/// element order other than P1. Both are caller bugs rather than recoverable
/// conditions, and both fail loudly in this module's existing style.
pub fn boundary_surface(mesh: &VolumeMesh) -> Mesh {
    let tets = mesh.tet_indices().unwrap_or_else(|| {
        panic!(
            "boundary_surface: mesh has no tet connectivity (got {:?}); only \
             tetrahedral meshes have a well-defined tet-face boundary",
            std::mem::discriminant(&mesh.connectivity)
        )
    });

    // P1 only. `tet_indices` yields 4 indices per element at P1 but 10 at P2
    // (`crates/reify-ir/src/geometry.rs`, `VolumeMesh::tet_indices`), and the
    // enumeration below walks the table in 4-index chunks unconditionally. A
    // P2 table would be re-grouped into bogus 4-tuples spanning element
    // boundaries, yielding a garbage face multiset whose "boundary" could
    // still satisfy `Mesh::validate` by accident. Assert the order the walk
    // depends on rather than let that pass silently.
    assert_eq!(
        mesh.element_order(),
        Some(ElementOrderTag::P1),
        "boundary_surface: P2 tet connectivity carries 10 nodes/elem and cannot \
         be walked in 4-index chunks"
    );

    // The four faces of tet [0,1,2,3], each paired with the local index of the
    // vertex it is opposite to. Winding within a triple is provisional — it is
    // corrected against the opposite vertex below.
    const TET_FACES: [([usize; 3], usize); 4] = [
        ([0, 1, 2], 3),
        ([0, 1, 3], 2),
        ([0, 2, 3], 1),
        ([1, 2, 3], 0),
    ];

    // sorted key -> (occurrence count, first-sighted raw triple, the vertex
    // that triple is opposite to in the tet that first sighted it). Storing
    // the opposing vertex rather than an already-oriented triple is what lets
    // orientation be deferred to the survivors below.
    // Pre-sized: a T-tet mesh has at most 4T distinct faces, and `tets.len()`
    // IS 4T, so the table never rehashes. From zero capacity it would grow and
    // rehash ~18 times over the ~220K entries this takes at the harness's n=18
    // scale — setup cost only (extraction sits outside both timed regions),
    // but the one nontrivial allocation profile in this module.
    let mut faces: HashMap<[u32; 3], (usize, [u32; 3], u32)> =
        HashMap::with_capacity(tets.len());

    for tet in tets.chunks_exact(4) {
        for (local, opposite) in TET_FACES {
            let tri = [tet[local[0]], tet[local[1]], tet[local[2]]];
            let mut key = tri;
            key.sort_unstable();

            faces.entry(key).or_insert((0, tri, tet[opposite])).0 += 1;
        }
    }

    // Boundary = the faces no second tet claimed. Winding is computed HERE,
    // inside the survivor filter, rather than at insertion: orienting on
    // first sighting would run once per DISTINCT face, and the overwhelming
    // majority of distinct faces are interior and about to be discarded. At
    // the harness's n=18 scale that is ~9K orientations instead of ~220K
    // (~96% of the work was feeding triangles that never get emitted). The
    // raw triple plus its opposing vertex are all `orient_outward` needs, so
    // deferring costs one extra `u32` per map entry and nothing else.
    let mut boundary: Vec<[u32; 3]> = faces
        .into_values()
        .filter(|&(count, _, _)| count == 1)
        .map(|(_, tri, opposite)| orient_outward(mesh, tri, opposite))
        .collect();
    // HashMap iteration order is nondeterministic; sort so the emitted
    // triangle order (and hence anything downstream keyed on it) is stable
    // run to run.
    boundary.sort_unstable();

    // Compact: old -> new index over exactly the referenced vertices. All three
    // tables are pre-sized off the survivor count — a closed triangulated
    // surface has V = F/2 + 2 by Euler, so `boundary.len()` comfortably
    // over-estimates the distinct vertices about to be interned.
    let vertex_estimate = boundary.len();
    let mut remap: HashMap<u32, u32> = HashMap::with_capacity(vertex_estimate);
    let mut vertices: Vec<f32> = Vec::with_capacity(vertex_estimate * 3);
    let mut indices: Vec<u32> = Vec::with_capacity(boundary.len() * 3);
    for tri in &boundary {
        for &old in tri {
            let new = *remap.entry(old).or_insert_with(|| {
                let new = (vertices.len() / 3) as u32;
                let base = old as usize * 3;
                vertices.extend_from_slice(&mesh.vertices[base..base + 3]);
                new
            });
            indices.push(new);
        }
    }

    Mesh {
        vertices,
        indices,
        normals: None,
    }
}

/// Wind `tri` so its normal points away from `opposite` — i.e. out of the
/// tet that owns the face.
///
/// Returns `tri` unchanged when `cross(q-p, r-p)` already points away from
/// `opposite`, and with its last two vertices swapped otherwise. A
/// degenerate face (zero cross product, or a coplanar opposite vertex)
/// leaves the winding untouched: there is no outward direction to point,
/// and `Mesh::validate`'s non-degeneracy obligation is the right place for
/// that to surface, not a silent guess here.
fn orient_outward(mesh: &VolumeMesh, tri: [u32; 3], opposite: u32) -> [u32; 3] {
    let read = |i: u32| -> [f64; 3] {
        mesh.vertex_f64(i).unwrap_or_else(|| {
            panic!(
                "boundary_surface: tet index {i} out of range for a {}-vertex mesh",
                mesh.vertices.len() / 3
            )
        })
    };
    let p = read(tri[0]);
    let q = read(tri[1]);
    let r = read(tri[2]);
    let o = read(opposite);

    let qp = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let rp = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
    let op = [o[0] - p[0], o[1] - p[1], o[2] - p[2]];
    let n = [
        qp[1] * rp[2] - qp[2] * rp[1],
        qp[2] * rp[0] - qp[0] * rp[2],
        qp[0] * rp[1] - qp[1] * rp[0],
    ];
    // > 0 means the provisional normal points *toward* the opposite vertex,
    // i.e. into the tet — flip it.
    if n[0] * op[0] + n[1] * op[1] + n[2] * op[2] > 0.0 {
        [tri[0], tri[2], tri[1]]
    } else {
        tri
    }
}
