//! v0.2 multi-kernel registration surface for Manifold.
//!
//! Declares Manifold's [`CapabilityDescriptor`] (the feasibility table that
//! enumerates every `(Operation, ReprKind)` pair Manifold supports) and
//! submits a [`KernelRegistration`] via `inventory::submit!` that the engine
//! collects via `reify_eval::kernel_registry::registry()` at startup.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions": each kernel
//! adapter lives in a separate crate, registering via a static linker-
//! collection mechanism (`inventory`) read once at engine startup. The
//! descriptor is feasibility-only — no `cost_hint`, no `error_factor`. The
//! dispatcher in `crates/reify-eval/src/dispatcher.rs` ranks plans by
//! conversion-stage count alone, with lexicographic tie-breaking on kernel
//! name.
//!
//! # Manifold's op surface
//!
//! Manifold consumes triangle meshes and produces mesh outputs. It does NOT
//! tessellate B-rep — that's OCCT's territory (a v0.3 forward-compat note in
//! `crates/reify-kernel-occt/src/register.rs:27-33` documents the planned
//! `(Convert { from: BRep }, Mesh)` entry that would let the dispatcher BFS
//! chain `BRep input → OCCT tessellate → Mesh BooleanUnion` automatically).
//! The descriptor therefore declares exactly three entries:
//! - `(BooleanUnion, Mesh)`
//! - `(BooleanDifference, Mesh)`
//! - `(BooleanIntersection, Mesh)`
//!
//! # Unconditional `inventory::submit!` decision
//!
//! OCCT gates its `inventory::submit!` on `cfg(has_occt)` because OCCT has
//! both a stub and a real impl and the gate selects between them. Manifold
//! has only a stub in this v0.2 task, so a `cfg(has_manifold)` gate would
//! never fire — the registration would be dead code, defeating the point of
//! "first integration". Submitting unconditionally keeps the cross-crate
//! integration test (step-7) clean and lets the dispatcher exercise lex-min
//! tie-break logic with a real two-kernel registry. A follow-up task
//! introducing real Manifold FFI can add `cfg(has_manifold)` gating to
//! switch the factory without changing the registration shape.
//!
//! # Design template
//!
//! `crates/reify-kernel-occt/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string, supports table contents, and the
//! dropped `cfg` gate differ.

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// Stable identifier for the Manifold kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::kernel_registry::registry()`).
///
/// # Lex-min note
///
/// `"manifold"` sorts before `"occt"` lexicographically. However, OCCT and
/// Manifold claim entirely disjoint `(op, repr)` pairs — OCCT claims BRep
/// ops, Manifold claims Mesh ops — so no tie-break conflict arises in the
/// current v0.2 descriptor tables. The lex-min tie-break only fires when two
/// kernels claim the _same_ `(op, repr)` pair; that is not the case here.
pub const MANIFOLD_KERNEL_NAME: &str = "manifold";

/// Construct the Manifold [`CapabilityDescriptor`].
///
/// Enumerates the three mesh-Boolean operations Manifold supports:
/// `BooleanUnion`, `BooleanDifference`, and `BooleanIntersection`, all paired
/// with `ReprKind::Mesh`. Called by the `KernelRegistration::descriptor`
/// function pointer at engine startup (once per `collect_registry()` call,
/// not per geometry op).
///
/// Owned return (`CapabilityDescriptor` by value) because the descriptor's
/// `supports: Vec<...>` field is non-const-constructible — see
/// `reify_types::KernelRegistration` doc for the full rationale.
pub fn manifold_capability_descriptor() -> CapabilityDescriptor {
    use Operation::*;
    let supports = vec![
        // Mesh Booleans ×3 — Manifold's complete capability surface in v0.2.
        (BooleanUnion, ReprKind::Mesh),
        (BooleanDifference, ReprKind::Mesh),
        (BooleanIntersection, ReprKind::Mesh),
    ];
    CapabilityDescriptor { supports }
}
