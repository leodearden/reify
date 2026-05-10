//! 2D cross-section meshing for the hex/wedge swept-body pipeline.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! This module is the typed orchestrator that turns a 2D profile boundary
//! (outer ring + optional holes) into a triangle or quad surface mesh,
//! routing the actual Gmsh call through
//! [`reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d`]. Pure-Rust quality
//! helpers ([`compute_quad_skew`], [`recombine_quality_ok`],
//! [`auto_mesh_size_from_boundary`]) live here so they remain unit-testable
//! in stub builds without libgmsh present.
//!
//! types and helpers will follow
