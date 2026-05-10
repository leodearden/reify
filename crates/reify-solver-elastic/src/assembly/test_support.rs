//! Test-only helpers shared between the `assembly::*` test modules.
//!
//! Lives under `#[cfg(test)] pub(crate) mod test_support;` in
//! [`crate::assembly`] so both `assembly::tests` and `assembly::tet::tests`
//! can pull from a single source of truth. Putting the shared helpers in
//! one place keeps the EDGES traversal driven directly off
//! [`crate::elements::tet_p2::EDGES`] (the production constant), so a
//! reordering of edges in production can never silently desynchronise the
//! test fixtures from the indexing the assembly code expects.

use crate::elements::tet_p2::EDGES;

/// Build the 8 physical nodes of a scaled unit hex in canonical Hughes/Gmsh hex8
/// order: bottom face (ζ = −s) counter-clockwise when viewed from +ζ, then
/// top face (ζ = +s) in the same cyclic order.
///
/// `s = 1.0` recovers the canonical reference cube `[−1, 1]³`; other scales are
/// used by the volume-scaling tests.
pub(crate) fn scaled_unit_hex_phys_nodes(s: f64) -> [[f64; 3]; 8] {
    [
        [-s, -s, -s], // node 0: (ξ,η,ζ) = (−1,−1,−1)
        [s, -s, -s],  // node 1: (+1,−1,−1)
        [s, s, -s],   // node 2: (+1,+1,−1)
        [-s, s, -s],  // node 3: (−1,+1,−1)
        [-s, -s, s],  // node 4: (−1,−1,+1)
        [s, -s, s],   // node 5: (+1,−1,+1)
        [s, s, s],    // node 6: (+1,+1,+1)
        [-s, s, s],   // node 7: (−1,+1,+1)
    ]
}

/// Build the canonical 10-node P2 phys-node layout for a uniformly scaled
/// reference tet: 4 vertices at `(0,0,0), (s,0,0), (0,s,0), (0,0,s)`
/// followed by the 6 edge-midpoint nodes in the production
/// [`crate::elements::tet_p2::EDGES`] order.
///
/// `s = 1.0` recovers the canonical unit reference tet; other scales are
/// used by the volume-scaling tests.
pub(crate) fn scaled_p2_phys_nodes(s: f64) -> [[f64; 3]; 10] {
    let v: [[f64; 3]; 4] = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
    let mid = |a: usize, b: usize| {
        [
            0.5 * (v[a][0] + v[b][0]),
            0.5 * (v[a][1] + v[b][1]),
            0.5 * (v[a][2] + v[b][2]),
        ]
    };

    let mut nodes = [[0.0_f64; 3]; 10];
    for (i, vert) in v.iter().enumerate() {
        nodes[i] = *vert;
    }
    // Drive midpoints off the production EDGES table — never re-list the
    // pairs as literals here, so an off-by-one in EDGES surfaces as a
    // production-test mismatch rather than silently aligning.
    for (i, &(a, b)) in EDGES.iter().enumerate() {
        nodes[4 + i] = mid(a, b);
    }
    nodes
}
