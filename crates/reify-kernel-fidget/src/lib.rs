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
//! Real Fidget JIT is wired in: [`FidgetKernel`] backs the three SDF Booleans
//! the descriptor claims (`Union`/`Difference`/`Intersection`) plus the
//! `Sphere` and `Box` primitives needed to construct SDF inputs, and exposes a
//! public `evaluate_sdf_at(handle, x, y, z)` method that builds a
//! `fidget::jit::JitShape` per call for point evaluation (arch §10.8).
//! `tessellate` — SDF→Mesh feature-preserving meshing — remains the named
//! v0.2 follow-up; `query` / `export` on Sdf reps depend on it.
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
