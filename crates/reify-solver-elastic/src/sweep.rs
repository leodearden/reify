//! Sweep step: 2D mesh × K layers → 3D wedge/hex connectivity.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #7.
//!
//! This module turns a 2D cross-section mesh (produced by task 2987's
//! [`crate::mesh_swept_profile_2d`] → [`crate::Mesh2d`]) into a 3D
//! wedge/hex mesh by replicating the 2D node grid across K+1 layers and
//! emitting one element per (face, layer) pair.
//!
//! # Canonical element node orderings
//!
//! - **Wedge6 (PRI6):** `[b0, b1, b2, t0, t1, t2]` — bottom-face CCW,
//!   then top-face in the same cyclic order. Matches
//!   `elements::wedge_p1::WedgeP1` node numbering.
//! - **Hex8:** `[b0, b1, b2, b3, t0, t1, t2, t3]` — bottom-face CCW,
//!   then top-face in the same cyclic order. Matches
//!   `elements::hex_p1::HexP1` node numbering.
//!
//! Both orderings produce det J > 0 when the 2D mesher emits CCW faces
//! (as Gmsh does by convention).
//!
//! # Node layout
//!
//! Node `(layer ℓ, base i)` lives at global flat index `ℓ * n_base + i`.
//! Layer ℓ=0 is the "bottom" (origin) plane; layer ℓ=K is the "top" plane.

#[cfg(test)]
mod tests {}
