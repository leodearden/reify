// Triangle index block of the workspace's axis-aligned box fixture: 12
// outward-wound triangles, two per face.
//
// Corner contract: corners 0..3 are the min-z ring, starting at the min
// corner and running CCW seen from +Z; corners 4..7 are the same ring at
// max z. The winding is outward only for that corner order.
//
// This file is not a module. It is a bare array expression that box
// builders consume with `include!` and type themselves.
// crates/reify-test-support/tests/box_index_block_ratchet.rs keeps it the
// only copy of the block in the workspace.
[
    0, 2, 1,  0, 3, 2, // -Z
    4, 5, 6,  4, 6, 7, // +Z
    0, 1, 5,  0, 5, 4, // -Y
    3, 7, 6,  3, 6, 2, // +Y
    0, 4, 7,  0, 7, 3, // -X
    1, 2, 6,  1, 6, 5, // +X
]
