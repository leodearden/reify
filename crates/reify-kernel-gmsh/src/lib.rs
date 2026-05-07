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
//! # v0.3 scope
//!
//! Real Gmsh FFI is deferred to a follow-up task. This crate ships the
//! adapter scaffold — `CapabilityDescriptor` declaration, inventory
//! registration, the three pure-Rust pipeline algorithms, the cache-key
//! derivation, and a stub `GmshKernel` that returns descriptive errors —
//! so that the dispatcher BFS has a fifth real registered kernel
//! to exercise the surface→volume route, and so consumers (FEA solver,
//! cache layer) can depend on the public types today.
//!
//! # Design templates
//!
//! `crates/reify-kernel-openvdb/` — closest template (stub-only adapter,
//! unconditional `inventory::submit!`, single coherent op surface).
//! `crates/reify-kernel-occt/build.rs` — system-library detection pattern.

pub mod auto_size;
pub mod cache_key;
pub mod kernel;
pub mod options;
pub mod register;
pub mod repair;
pub mod through_thickness;
