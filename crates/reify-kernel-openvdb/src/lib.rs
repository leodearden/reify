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

pub mod kernel;
pub mod register;

pub use kernel::OpenVdbKernel;
