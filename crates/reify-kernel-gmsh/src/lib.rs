//! `reify-kernel-gmsh` — Gmsh surface-to-volume tetrahedral mesher kernel
//! adapter for the v0.3 FEA pipeline.
//!
//! This crate registers a [`reify_types::KernelRegistration`] via
//! `inventory::submit!` declaring Gmsh's surface→volume mesh capability
//! (`Convert { from: Mesh } → VolumeMesh`). The registration is read at
//! engine startup by `reify_eval::kernel_registry::registry()` and plugged
//! into the dispatcher BFS, which routes surface-mesh → volume-mesh
//! conversion requests through this adapter.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/structural-analysis-fea.md` — the v0.3 structural-analysis
//! pipeline calls out three meshing-pipeline algorithms this crate ships:
//! surface-mesh repair pre-stage, auto mesh-size from smallest geometric
//! feature, and through-thickness element-count diagnostic.
//!
//! # Build modes
//!
//! - **`cfg(has_gmsh)`** (real FFI): `build.rs` detected `libgmsh.so` +
//!   `gmshc.h` (default search path: `/opt/reify-deps`). The crate links
//!   against libgmsh and exposes the real [`GmshKernel`] backed by
//!   hand-rolled `unsafe extern "C"` bindings in [`ffi`].
//! - **`cfg(not(has_gmsh))`** (stub): libgmsh was not found; the crate
//!   compiles in stub-only mode. The stub [`GmshKernel`] returns
//!   descriptive errors from every trait method.
//!
//! Both modes expose a single `reify_kernel_gmsh::GmshKernel` type via
//! cfg-conditional `pub use` — external callers do not change between modes.
//!
//! # Design templates
//!
//! `crates/reify-kernel-openvdb/src/lib.rs:36-58` — closest template for
//! the cfg-conditional module + `pub use` shape.
//! `crates/reify-kernel-occt/build.rs` — system-library detection pattern.

pub mod auto_size;
pub mod cache_key;
#[cfg(feature = "mesh-morph")]
pub mod mesh_boundary;
pub mod mesh_profile_2d;
pub mod mesh_volume;
pub mod options;
pub mod refine_volume;
pub mod register;
pub mod repair;
pub mod through_thickness;

// Real FFI bridge — only compiled when the build script detects libgmsh.
#[cfg(has_gmsh)]
pub mod ffi;

// Shared library-initialisation + process-global lock — only compiled when
// has_gmsh is set. Exposed as `pub` (rather than `pub(crate)`) so the
// integration test binaries in `tests/` can acquire `init::GMSH_LOCK` before
// touching the gmsh library; tests/ are separate compilation units that
// cannot reach `pub(crate)` symbols.
#[cfg(has_gmsh)]
pub mod init;

// Real kernel (FFI-backed) — only compiled when has_gmsh is set.
#[cfg(has_gmsh)]
pub mod kernel_real;

// Stub kernel — only compiled when has_gmsh is NOT set.
#[cfg(not(has_gmsh))]
pub mod kernel;

// Single public `GmshKernel` ident regardless of build mode (mirrors the
// pattern in crates/reify-kernel-openvdb/src/lib.rs:55-58).
#[cfg(not(has_gmsh))]
pub use kernel::GmshKernel;
#[cfg(has_gmsh)]
pub use kernel_real::GmshKernel;

pub use cache_key::volume_mesh_cache_key;
// MeshSurfaceToVolumeReport is the return type of the cfg(has_gmsh)-gated
// orchestrator; gating the re-export to match keeps the type and its sole
// constructor reachable from the same import path in real builds, and avoids
// exposing a non-constructible type at the crate root in stub builds.
//
// The pure helpers (apply_repair_if_requested, compute_thickness_warnings,
// resolve_mesh_size) are intentionally NOT re-exported at the crate root —
// callers should reach them via `reify_kernel_gmsh::mesh_volume::*`.
//
// mesh_surface_to_volume_with_diagnostics depends on GmshKernel::mesh_to_volume
// which only exists in the real FFI build (kernel_real.rs, cfg(has_gmsh)).
#[cfg(has_gmsh)]
pub use mesh_volume::{MeshSurfaceToVolumeReport, mesh_surface_to_volume_with_diagnostics};
// 2D plane-surface meshing primitive added by task 2987 — uniform signature
// across both `cfg(has_gmsh)` (real FFI) and `cfg(not(has_gmsh))` (stub
// returning `GeometryError::OperationFailed` containing
// `STUB_UNAVAILABLE_MARKER`), so the re-export is unconditional.
// `MeshPlane2dResult` is defined unconditionally in `mesh_profile_2d` — no
// cfg gate needed on the type either. `STUB_UNAVAILABLE_MARKER` is exported
// at the crate root so downstream orchestrators can pattern-match the stub
// error without re-declaring the literal.
pub use mesh_profile_2d::{MeshPlane2dResult, STUB_UNAVAILABLE_MARKER, mesh_plane_2d};
// NodeAttachment producer (task 3591 / task 3763, PRD mesh-morphing-phase-2 §3.3).
//
// `EntityAttribution` is the input type — constructible in both build modes
// (analogous to `MeshPlane2dResult`), so the re-export is unconditional.
//
// `BoundaryAttributedReport` (report type) and its sole constructor
// `mesh_surface_to_volume_with_attribution` are gated on `has_gmsh`, mirroring
// `MeshSurfaceToVolumeReport` / `mesh_surface_to_volume_with_diagnostics` above:
// the constructor calls into the real gmsh FFI build, and exposing a type whose
// only constructor is `has_gmsh`-gated at the crate root in stub builds would
// mislead callers.
//
// Note: `VolumeMesh` is an unconditional `reify_types` type (not `has_gmsh`-only);
// see the import comment in `mesh_boundary.rs` and the `mesh_volume.rs` precedent.
#[cfg(feature = "mesh-morph")]
pub use mesh_boundary::EntityAttribution;
#[cfg(all(has_gmsh, feature = "mesh-morph"))]
pub use mesh_boundary::{BoundaryAttributedReport, mesh_surface_to_volume_with_attribution};
pub use options::MeshingOptions;
// Task 2999: a-posteriori volume mesh refinement driven by per-element size
// hints (PRD docs/prds/v0_4/a-posteriori-error-estimation.md task #2).
// Unconditional re-export — uniform signature in both cfg(has_gmsh) (real FFI
// remesh) and cfg(not(has_gmsh)) (stub returning STUB_UNAVAILABLE_MARKER).
pub use refine_volume::refine_volume_with_size_field;

/// `true` when this crate was compiled with libgmsh detected at build time
/// (real FFI surface available); `false` otherwise (stub-only build).
///
/// Mirrors `reify_kernel_occt::OCCT_AVAILABLE` for runtime reflection — used
/// by tests / CLI tools that need to skip libgmsh-dependent assertions on
/// hosts without `/opt/reify-deps`.
pub const GMSH_AVAILABLE: bool = cfg!(has_gmsh);
