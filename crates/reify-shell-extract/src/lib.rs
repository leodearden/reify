//! `reify-shell-extract` — Voxel-medial mid-surface extraction for shell-element FEA.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md` task **T1** (per-voxel
//! medial-mask algorithm). This crate identifies the voxels that lie on the
//! medial axis (mid-surface) of a thin solid by querying each active voxel's
//! nearest surface point in two opposing directions and tagging it as medial
//! iff:
//!
//! 1. opposing distances are within ~5%, AND
//! 2. the two surface-hit points are geometrically distinct — observable as
//!    antiparallel SDF gradients at the two hit points (the gradient
//!    discontinuity at the medial axis itself).
//!
//! The follow-up tasks T2 (mid-surface mesh extraction), T3 (branch pruning),
//! and T4 (region segmentation) build on this mask.
//!
//! # Dependency relationship
//!
//! Input is `&reify_types::value::SampledField` (Regular3D narrow-band SDF).
//! The shipping `OpenVdbGridSource → SampledField` lowering pipeline in
//! `reify-kernel-openvdb::ingest::lower_to_sampled` is the eventual producer
//! once the OpenVDB FFI lands; until then, callers (and this crate's own
//! tests) construct `SampledField` instances directly from analytic SDFs.
//! This mirrors the `reify-solver-elastic` skeleton-crate template: ship the
//! algorithm against synthetic inputs, wire real producers in a follow-up.
//!
//! Output is a self-defined sparse [`MedialMask`] (`Vec<[i32; 3]>` of voxel
//! indices). The PRD permits `openvdb::BoolGrid OR EQUIVALENT`; a pure-Rust
//! sparse list is sufficient for downstream T2/T3/T4 consumers, all of which
//! iterate the mask voxels regardless of underlying storage. When the
//! OpenVDB FFI lands, the storage backing can be swapped behind the same
//! public API without changing T2/T3/T4 callers.

pub mod medial;
