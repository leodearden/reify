//! `reify-kernel-fidget` — Fidget SDF-Boolean kernel adapter for the
//! v0.2 multi-kernel system.
//!
//! This crate registers a [`KernelRegistration`] via `inventory::submit!`
//! declaring Fidget's SDF-Boolean capability surface
//! (`BooleanUnion/Difference/Intersection × Sdf`). The registration is
//! read at engine startup by `reify_eval::kernel_registry::registry()` and
//! plugged into the dispatcher BFS.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Sketch of approach" and "Resolved
//! design decisions": Fidget routes `field def`-as-geometry SDF realizations
//! directly through Fidget rather than meshing through OCCT (arch §10.6
//! geometry-field bidirectionality). SDF→Mesh feature-preserving meshing
//! (Fidget's signature feature per arch §10.8) is a v0.2 follow-up task.
//!
//! # v0.2 scope
//!
//! Real Fidget Rust JIT FFI is deferred to a follow-up task. This crate
//! ships the adapter scaffold — `CapabilityDescriptor` declaration, inventory
//! registration, and a stub `FidgetKernel` that returns descriptive errors
//! — so that the dispatcher BFS has a third real registered kernel to exercise
//! with a clean zero-conversion test target on the `Sdf` repr family.
//!
//! # Design templates
//!
//! `crates/reify-kernel-manifold/` — canonical template for this adapter.
//! `crates/reify-kernel-occt/src/register.rs` — OCCT's registration pattern.
//! `crates/reify-kernel-occt/src/stubs.rs` — stub kernel pattern.
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.

pub mod kernel;
pub mod register;

pub use kernel::FidgetKernel;
