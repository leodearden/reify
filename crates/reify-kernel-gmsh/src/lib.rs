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
pub mod mesh_volume;
pub mod options;
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
#[cfg(has_gmsh)]
pub use kernel_real::GmshKernel;
#[cfg(not(has_gmsh))]
pub use kernel::GmshKernel;

pub use cache_key::volume_mesh_cache_key;
pub use mesh_volume::{compute_thickness_warnings, resolve_mesh_size, MeshSurfaceToVolumeReport};
// mesh_surface_to_volume_with_diagnostics depends on GmshKernel::mesh_to_volume
// which only exists in the real FFI build (kernel_real.rs, cfg(has_gmsh)).
#[cfg(has_gmsh)]
pub use mesh_volume::mesh_surface_to_volume_with_diagnostics;
pub use options::MeshingOptions;

/// `true` when this crate was compiled with libgmsh detected at build time
/// (real FFI surface available); `false` otherwise (stub-only build).
///
/// Mirrors `reify_kernel_occt::OCCT_AVAILABLE` for runtime reflection — used
/// by tests / CLI tools that need to skip libgmsh-dependent assertions on
/// hosts without `/opt/reify-deps`.
pub const GMSH_AVAILABLE: bool = cfg!(has_gmsh);
