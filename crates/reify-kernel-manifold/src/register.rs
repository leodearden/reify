//! v0.2 multi-kernel registration surface for Manifold.
//!
//! # Design template
//!
//! Mirrors `crates/reify-kernel-occt/src/register.rs` — the same
//! `KERNEL_NAME` const + `*_capability_descriptor()` factory +
//! `*_factory()` returning `Box<dyn GeometryKernel>` + `inventory::submit!`
//! pattern. Differences: kernel name is `"manifold"`, supports table has
//! exactly three Mesh-Boolean entries (vs. OCCT's 35-entry BRep table), and
//! the submit is unconditional (no `cfg(has_manifold)` gate — see design
//! decisions below).
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions".
