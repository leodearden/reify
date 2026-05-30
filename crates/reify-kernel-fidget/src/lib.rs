//! `reify-kernel-fidget` — Fidget SDF-Boolean + SDF→Mesh kernel adapter.
//!
//! This crate registers a [`KernelRegistration`] via `inventory::submit!`
//! declaring Fidget's capability surface
//! (`BooleanUnion/Difference/Intersection × Sdf` and
//! `Convert{Sdf} → Mesh`). The registration is read at engine startup by
//! `reify_eval::kernel_registry::registry()` and plugged into the
//! dispatcher BFS.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Sketch of approach" and "Resolved
//! design decisions": Fidget routes `field def`-as-geometry SDF realizations
//! directly through Fidget rather than meshing through OCCT (arch §10.6
//! geometry-field bidirectionality).
//!
//! PRD §8 task κ (`docs/prds/v0_3/multi-kernel-phase-3.md:508`): SDF→Mesh
//! iso-surface meshing via fidget-mesh Manifold Dual Contouring is now wired.
//! The `tessellate` trait method delegates to `iso_mesh`; `query` / `export`
//! on Sdf reps remain out of scope (require downstream φ/υ wiring).
//!
//! # v0.3 scope (PRD §8 task κ)
//!
//! Real Fidget JIT is wired in: [`FidgetKernel`] backs the three SDF Booleans
//! the descriptor claims (`Union`/`Difference`/`Intersection`) plus the
//! `Sphere` and `Box` primitives needed to construct SDF inputs, exposes a
//! public `evaluate_sdf_at(handle, x, y, z)` method for point evaluation
//! (arch §10.8), and now exposes `iso_mesh(handle, &IsoMeshOptions)` for
//! SDF→Mesh iso-surface meshing. The `tessellate` trait method delegates
//! to `iso_mesh`. `IsoMeshOptions` provides a domain-tagged `content_hash()`
//! as the cache-key hash producer for the per-op Mesh cache (φ's wiring job).
//!
//! Fidget is a pure-Rust crate (no FFI, no native lib, no `build.rs`), so the
//! crate compiles unconditionally on every supported target — no `cfg`-gate
//! mirroring OCCT's `has_occt` is needed.
//!
//! # Design templates
//!
//! `crates/reify-kernel-manifold/` — canonical template for this adapter.
//! `crates/reify-kernel-occt/src/register.rs` — OCCT's registration pattern.
//! `crates/reify-test-support/src/mocks.rs` — `FailingMockGeometryKernel`.

pub mod iso_mesh_options;
pub mod kernel;
pub mod register;

pub use iso_mesh_options::IsoMeshOptions;
pub use kernel::FidgetKernel;
