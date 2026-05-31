//! `reify-kernel-openvdb` — OpenVDB voxel-Boolean kernel adapter for the
//! v0.2 multi-kernel system.
//!
//! This crate registers a [`KernelRegistration`] via `inventory::submit!`
//! declaring OpenVDB's voxel-Boolean capability surface
//! (`BooleanUnion/Difference/Intersection × Voxel`). The registration is
//! read at engine startup by `reify_eval::kernel_registry::registry()` and
//! plugged into the dispatcher BFS.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Sketch of approach" and "Resolved
//! design decisions": OpenVDB routes voxel-grid Boolean operations through
//! an OpenVDB-native kernel. Voxel→Mesh surfacing (level-set → marching
//! cubes) and the imported-field-source ingestion path are v0.2 follow-ups.
//!
//! # v0.2 scope
//!
//! Real OpenVDB FFI is deferred to a follow-up task. This crate ships the
//! adapter scaffold — `CapabilityDescriptor` declaration, inventory
//! registration, and a stub `OpenVdbKernel` that returns descriptive errors
//! — so that the dispatcher BFS has a fourth real registered kernel to
//! exercise with a clean zero-conversion test target on the `Voxel` repr
//! family.
//!
//! # Design templates
//!
//! `crates/reify-kernel-fidget/` — canonical template for this adapter.
//! `crates/reify-kernel-occt/src/register.rs` — OCCT's registration pattern.
//! `crates/reify-kernel-occt/src/stubs.rs` — stub kernel pattern.
//! `crates/reify-test-support/src/mocks.rs` — `FailingMockGeometryKernel`.

pub mod ingest;
pub mod mesh_to_voxel_options;
pub mod register;

pub use mesh_to_voxel_options::MeshToVoxelOptions;

// Real FFI bridge — only compiled when the build script detects OpenVDB.
#[cfg(has_openvdb)]
pub mod ffi;

// Shared library-initialisation helper used by both `kernel_real` and `ingest`
// (cfg(has_openvdb) read path) — only compiled when has_openvdb is set.
#[cfg(has_openvdb)]
mod init;

// Real kernel (FFI-backed) — only compiled when has_openvdb is set.
#[cfg(has_openvdb)]
pub mod kernel_real;

// Stub kernel — only compiled when has_openvdb is NOT set.
#[cfg(not(has_openvdb))]
pub mod kernel;

// Single public `OpenVdbKernel` ident regardless of build mode (mirrors OCCT's
// pub use pattern from crates/reify-kernel-occt/src/lib.rs).
#[cfg(not(has_openvdb))]
pub use kernel::OpenVdbKernel;
#[cfg(has_openvdb)]
pub use kernel_real::OpenVdbKernel;

pub use ingest::{
    IngestError, IngestOutcome, KNOWN_UNITS, OpenVdbGridKind, OpenVdbGridSource,
    OpenVdbInterpolation, lower_to_sampled, read_vdb_file, validate_grid_units,
};
