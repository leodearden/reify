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
//! # Persistent-naming-v2 task 9: `KernelAttributeHook`
//!
//! Per `docs/prds/v0_2/persistent-naming-v2.md` line 70, [`ManifoldKernel`]
//! is the first concrete impl of [`reify_types::KernelAttributeHook`]:
//!
//! - [`reify_types::GeometryKernel::attribute_hook`] is overridden to return
//!   `Some(self)`, opting Manifold into native attribute propagation through
//!   the engine-side `reify_eval::propagate_via_kernel_attribute_hook`
//!   dispatcher.
//! - [`reify_types::KernelAttributeHook::propagate_attributes`] in this
//!   v0.2 stub returns `Ok(KernelAttributeOutcome::Discarded)` and emits a
//!   `tracing::warn!(target = "reify_kernel_manifold", reason =
//!   "deferred_ffi", ...)` event regardless of inputs — real `MeshGL` /
//!   `faceID` / `originalID` walking lands when the FFI does. The trait
//!   surface is stable across that swap.
//! - Fidget / OpenVDB structurally inherit the [`reify_types::GeometryKernel::attribute_hook`]
//!   `None` default and therefore fall through to computed selectors per the
//!   PRD contract — no per-kernel opt-out is required there.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/register.rs` — OCCT's registration pattern.
//! `crates/reify-kernel-occt/src/stubs.rs` — stub kernel pattern.
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.

pub mod kernel;
pub mod register;

pub use kernel::ManifoldKernel;
