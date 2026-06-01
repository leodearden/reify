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
//! # Unconditional registration
//!
//! With real Manifold C++ FFI wired via the `manifold3d` cargo dep, the
//! `inventory::submit!` and its helper `manifold_factory()` are unconditional —
//! `manifold3d` is a regular cargo dependency that pulls and compiles its
//! own C++ tree, so the dep is unconditionally present whenever this crate is
//! built. There is no "manifold absent" case to gate against. (Compare with
//! OCCT, which uses a `cfg(has_occt)` gate driven by `build.rs` system-library
//! detection — Manifold has no analogous absent case.)
//!
//! # Design template
//!
//! `crates/reify-kernel-occt/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string and supports table contents differ.

use reify_ir::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

/// Factory invoked by the engine once at startup, returning a boxed
/// [`ManifoldKernel`](crate::kernel::ManifoldKernel) backed by the
/// `manifold3d` C++ FFI.
///
/// `ingest_mesh` is now a production trait method on [`reify_ir::GeometryKernel`]
/// and is reachable through this boxed factory. Test code that needs
/// the `test-fixtures`-gated fixtures (`unit_cube_mesh`,
/// `manifold_factory_for_test`) may still construct a concrete kernel
/// directly via [`crate::kernel::ManifoldKernel::new`] for clarity, but
/// the production `ingest_mesh` path is no longer hidden behind boxing.
pub fn manifold_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::kernel::ManifoldKernel::new())
}

inventory::submit! {
    KernelRegistration {
        name: MANIFOLD_KERNEL_NAME,
        descriptor: manifold_capability_descriptor,
        factory: manifold_factory,
    }
}

/// Stable identifier for the Manifold kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::kernel_registry::registry()`).
///
/// Must equal `KernelId::Manifold.to_string()` (`"manifold"`) so the
/// project-pin lookup in `reify-config` matches the registered adapter at
/// runtime. Enforced by
/// `crates/reify-config/tests/kernel_name_consistency.rs::manifold_kernel_name_const_matches_kernel_id_display`.
///
/// # Lex-min note
///
/// `"manifold"` sorts before `"occt"` lexicographically. However, OCCT and
/// Manifold claim entirely disjoint `(op, repr)` pairs — OCCT claims BRep
/// ops, Manifold claims Mesh ops — so no tie-break conflict arises in the
/// current v0.2 descriptor tables. The lex-min tie-break only fires when two
/// kernels claim the _same_ `(op, repr)` pair; that is not the case here.
pub const MANIFOLD_KERNEL_NAME: &str = reify_core::KernelId::Manifold.as_registry_name();

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
