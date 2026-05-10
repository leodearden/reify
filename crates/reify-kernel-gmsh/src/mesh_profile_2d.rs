//! 2D plane-surface meshing via Gmsh's built-in CAD API.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! This module exposes [`mesh_plane_2d`] in both `cfg(has_gmsh)` (real FFI)
//! and `cfg(not(has_gmsh))` (stub returning `GeometryError::OperationFailed`)
//! arms — mirroring the kernel-wide single-signature convention so callers
//! in `reify-solver-elastic::mesher` don't need to cfg-gate at every
//! call-site.
//!
//! The real arm parallels [`mesh_volume`](crate::mesh_volume)'s
//! orchestrator template: acquire `init::GMSH_LOCK`, `ensure_initialized`,
//! `clear`, build the model via the built-in CAD API
//! (point → line → curve_loop → plane_surface), optionally enable
//! recombine, `mesh_generate(2)`, read back triangles (element type 2) and
//! quads (element type 3).
