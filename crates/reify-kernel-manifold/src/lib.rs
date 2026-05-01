//! `reify-kernel-manifold` — Manifold mesh-Boolean kernel adapter for the
//! v0.2 multi-kernel system.
//!
//! This crate registers a [`KernelRegistration`] via `inventory::submit!`
//! declaring Manifold's mesh-Boolean capability surface
//! (`BooleanUnion/Difference/Intersection × Mesh`). The registration is
//! read at engine startup by `reify_eval::kernel_registry::registry()` and
//! plugged into the dispatcher BFS.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Sketch of approach" and "Resolved
//! design decisions": Manifold consumes triangle meshes and produces mesh
//! outputs. B-rep tessellation (BRep → Mesh) remains OCCT's responsibility
//! (a v0.3 forward-compat note lives in
//! `crates/reify-kernel-occt/src/register.rs:27-33`).
//!
//! # v0.2 scope
//!
//! Real Manifold C++ FFI is deferred to a follow-up task. This crate ships
//! the adapter scaffold — `CapabilityDescriptor` declaration, inventory
//! registration, and a stub `ManifoldKernel` that returns descriptive errors
//! — so that the downstream persistent-naming-v2 task 9 gate ("manifold
//! adapter exists") is satisfied.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/register.rs` — OCCT's registration pattern.
//! `crates/reify-kernel-occt/src/stubs.rs` — stub kernel pattern.
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.

pub mod kernel;
pub mod register;

pub use kernel::ManifoldKernel;
