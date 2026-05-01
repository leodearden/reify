//! v0.2 multi-kernel registration surface for Fidget.
//!
//! Declares Fidget's [`CapabilityDescriptor`] (the feasibility table that
//! enumerates every `(Operation, ReprKind)` pair Fidget supports) and
//! will submit a [`KernelRegistration`] via `inventory::submit!` that the
//! engine collects via `reify_eval::kernel_registry::registry()` at startup.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions" and the
//! integration-sequence note: Fidget unblocks `field def`-as-geometry per
//! arch §10.6 (geometry-field bidirectionality).
//!
//! # Design template
//!
//! `crates/reify-kernel-manifold/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, and `inventory::submit!` pattern.

use reify_types::CapabilityDescriptor;

/// Stable identifier for the Fidget kernel in the v0.2 multi-kernel registry.
///
/// Must equal `KernelId::Fidget.to_string()` (`"fidget"`) so the project-pin
/// lookup in `reify-config` matches the registered adapter at runtime.
///
/// # Lex-min note
///
/// `"fidget"` sorts before `"manifold"` and `"occt"` lexicographically.
/// However, no tie-break conflict arises in v0.2 because the three kernels
/// claim entirely disjoint `(op, repr)` pairs (Sdf vs Mesh vs BRep).
pub const FIDGET_KERNEL_NAME: &str = "fidget";

/// Construct the Fidget [`CapabilityDescriptor`].
///
/// Placeholder — returns an empty `supports` vec. The descriptor surface test
/// in step-1 (`fidget_capability_descriptor_lists_sdf_booleans`) will fail
/// against this empty vec until step-2 fills in the three SDF-Boolean entries.
pub fn fidget_capability_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor { supports: vec![] }
}
