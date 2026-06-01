//! v0.2 multi-kernel registration surface for Fidget.
//!
//! Declares Fidget's [`CapabilityDescriptor`] (the feasibility table that
//! enumerates every `(Operation, ReprKind)` pair Fidget supports) and
//! submits a [`KernelRegistration`] via `inventory::submit!` that the engine
//! collects via `reify_eval::kernel_registry::registry()` at startup.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions" and the
//! integration-sequence note: Fidget unblocks `field def`-as-geometry per
//! arch §10.6 (geometry-field bidirectionality). The descriptor is
//! feasibility-only — no `cost_hint`, no `error_factor`. The dispatcher in
//! `crates/reify-eval/src/dispatcher.rs` ranks plans by conversion-stage
//! count alone, with lexicographic tie-breaking on kernel name.
//!
//! # Fidget's op surface
//!
//! Fidget operates on SDF (Signed Distance Field) representations. It does NOT
//! tessellate B-rep (OCCT's territory) or perform mesh Booleans (Manifold's
//! territory). The descriptor declares four entries:
//! - `(BooleanUnion, Sdf)`
//! - `(BooleanDifference, Sdf)`
//! - `(BooleanIntersection, Sdf)`
//! - `(Convert { from: Sdf }, Mesh)` — SDF→Mesh iso-surface meshing (PRD §8 task κ)
//!
//! Deliberately excluded from the descriptor:
//! - SDF primitives: kernel-only support (callers can `execute(Sphere|Box)`
//!   directly to build SDF inputs), but not declared on the descriptor — the
//!   task spec keeps descriptor side unchanged so the dispatcher does not
//!   route primitive builds through fidget.
//! - BRep→SDF distance-field sampling: deferred to a separate follow-up.
//!
//! # Unconditional `inventory::submit!` decision
//!
//! Fidget is pure-Rust with no FFI, no system lib, and no platform without it
//! — the historical `cfg(has_fidget)` gate that would have mirrored OCCT's
//! `has_occt` is unnecessary; submitting unconditionally is correct. The
//! cross-crate integration test (`fidget_dispatches_for_sdf_boolean_when_only_kernel`)
//! pins this and gives the dispatcher BFS a third real registered kernel to
//! exercise on the Sdf repr family.
//!
//! # Design template
//!
//! `crates/reify-kernel-manifold/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string, supports table contents (Sdf vs Mesh),
//! the kernel implementation, and the doc comments' references differ.

use reify_ir::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

/// Factory invoked by the engine once at startup, returning a fresh
/// [`FidgetKernel`](crate::kernel::FidgetKernel) backed by fidget 0.4's
/// pure-Rust JIT (Tree storage + per-call `JitShape` evaluation).
fn fidget_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::kernel::FidgetKernel::new())
}

// No `cfg(has_fidget)` gate needed — fidget is pure-Rust and always builds
// (see the "Unconditional `inventory::submit!` decision" rationale in the
// module doc).
inventory::submit! {
    KernelRegistration {
        name: FIDGET_KERNEL_NAME,
        descriptor: fidget_capability_descriptor,
        factory: fidget_factory,
    }
}

/// Stable identifier for the Fidget kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::kernel_registry::registry()`).
///
/// Must equal `KernelId::Fidget.to_string()` (`"fidget"`) so the project-pin
/// lookup in `reify-config` matches the registered adapter at runtime.
/// Enforced by
/// `crates/reify-config/tests/kernel_name_consistency.rs::fidget_kernel_name_const_matches_kernel_id_display`.
///
/// # Lex-min note
///
/// `"fidget"` sorts before `"manifold"` and `"occt"` lexicographically.
/// However, no tie-break conflict arises in v0.2 because Fidget, Manifold, and
/// OCCT claim entirely disjoint `(op, repr)` pairs (Sdf vs Mesh vs BRep). The
/// lex-min tie-break only fires when two kernels claim the _same_ `(op, repr)`
/// pair; that is not the case here.
pub const FIDGET_KERNEL_NAME: &str = reify_core::KernelId::Fidget.as_registry_name();

/// Construct the Fidget [`CapabilityDescriptor`].
///
/// Enumerates the SDF-Boolean operations and the Sdf→Mesh Convert edge
/// Fidget supports. Called by the `KernelRegistration::descriptor` function
/// pointer at engine startup (once per `collect_registry()` call, not per
/// geometry op).
///
/// Owned return (`CapabilityDescriptor` by value) because the descriptor's
/// `supports: Vec<...>` field is non-const-constructible — see
/// `reify_types::KernelRegistration` doc for the full rationale.
pub fn fidget_capability_descriptor() -> CapabilityDescriptor {
    use Operation::*;
    let supports = vec![
        // SDF Booleans ×3 — Fidget's Sdf Boolean surface.
        (BooleanUnion, ReprKind::Sdf),
        (BooleanDifference, ReprKind::Sdf),
        (BooleanIntersection, ReprKind::Sdf),
        // Convert ×1 — Sdf→Mesh iso-surface meshing (PRD §8 task κ).
        (Convert { from: ReprKind::Sdf }, ReprKind::Mesh),
    ];
    CapabilityDescriptor { supports }
}
