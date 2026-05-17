//! NodeAttachment producer — B-rep attribution threading through the
//! surface→volume meshing path.
//!
//! Implements task γ (M-005) from PRD
//! `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.3: emit a
//! [`BoundaryAssociation`] alongside the produced [`VolumeMesh`], threading
//! caller-supplied per-input-vertex B-rep attribution through the HXT meshing
//! path.
//!
//! # Design
//!
//! All types and functions in this module are feature-gated behind
//! `#[cfg(feature = "mesh-morph")]` (applied at the `pub mod mesh_boundary`
//! declaration in `lib.rs`). The `#[cfg(has_gmsh)]`-gated orchestrating
//! function `mesh_surface_to_volume_with_attribution` is additionally gated on
//! `has_gmsh` because it calls `mesh_surface_to_volume_with_diagnostics` which
//! only exists in the real FFI build.
