//! `reify-kernel-manifold` — Manifold mesh-Boolean kernel adapter for the
//! v0.2 multi-kernel system.
//!
//! Backed by `manifold3d` 0.1 (`zmerlynn/manifold-csg` fork). This crate
//! registers a [`KernelRegistration`] via `inventory::submit!` declaring
//! Manifold's mesh-Boolean capability surface
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
//! # Persistent-naming-v2 task 9: `KernelAttributeHook`
//!
//! Per `docs/prds/v0_2/persistent-naming-v2.md` line 70, [`ManifoldKernel`]
//! is the first concrete impl of [`reify_types::KernelAttributeHook`]:
//!
//! - [`reify_types::GeometryKernel::attribute_hook`] is overridden to return
//!   `Some(self)`, opting Manifold into native attribute propagation through
//!   the engine-side `reify_eval::propagate_via_kernel_attribute_hook`
//!   dispatcher.
//! - [`reify_types::KernelAttributeHook::propagate_attributes`] stays a
//!   `Discarded`-with-WARN stub until persistent-naming-v2 PRD task 9 lands
//!   the real `MeshGL` walk over `originalID` / `faceID` / merge vectors.
//!   The trait surface is stable across that swap; only the body changes.
//! - Fidget / OpenVDB structurally inherit the
//!   [`reify_types::GeometryKernel::attribute_hook`] `None` default and
//!   therefore fall through to computed selectors per the PRD contract — no
//!   per-kernel opt-out is required there.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/register.rs` — OCCT's registration pattern.
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # Manifold-face = mesh triangle (semantic gap)
//!
//! Topology selectors enumerate **mesh triangles/edges**, not BRep patches:
//! `faces(mesh_box)` yields 12 sub-handles vs a BRep box's 6, and
//! `adjacent_faces` / `shared_edges` index triangles. See the "Manifold
//! mesh-face vs BRep-face semantic gap" section of the [`queries`] module doc
//! (and PRD Open Question §10.5).

pub mod kernel;
pub mod queries;
pub mod register;

/// Shared test-only mesh fixtures (e.g. [`test_fixtures::unit_cube_mesh`]).
///
/// Gated on `cfg(any(test, feature = "test-fixtures"))` so the module is
/// reachable from in-crate `mod tests` and from cross-crate integration
/// test binaries that pick up the `test-fixtures` feature via the
/// self-dev-dep in `Cargo.toml` — never compiled into production link
/// closures.
#[cfg(any(test, feature = "test-fixtures"))]
pub mod test_fixtures;

pub use kernel::ManifoldKernel;
