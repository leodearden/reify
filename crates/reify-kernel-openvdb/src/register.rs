//! v0.2 multi-kernel registration surface for OpenVDB.
//!
//! Declares OpenVDB's [`CapabilityDescriptor`] (the feasibility table that
//! enumerates every `(Operation, ReprKind)` pair OpenVDB supports) and
//! submits a [`KernelRegistration`] via `inventory::submit!` that the engine
//! collects via `reify_eval::kernel_registry::registry()` at startup.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions" and the
//! integration-sequence note: OpenVDB handles voxel-grid Boolean operations.
//! The descriptor is feasibility-only — no `cost_hint`, no `error_factor`.
//! The dispatcher in `crates/reify-eval/src/dispatcher.rs` ranks plans by
//! conversion-stage count alone, with lexicographic tie-breaking on kernel
//! name.
//!
//! # OpenVDB's op surface
//!
//! OpenVDB operates on Voxel (volumetric grid) representations. It does NOT
//! tessellate B-rep (OCCT's territory), perform mesh Booleans (Manifold's
//! territory), or evaluate SDFs (Fidget's territory). The descriptor therefore
//! declares exactly three entries:
//! - `(BooleanUnion, Voxel)`
//! - `(BooleanDifference, Voxel)`
//! - `(BooleanIntersection, Voxel)`
//!
//! Deliberately excluded from the v0.2 descriptor:
//! - Voxel primitives: deferred to avoid routing `field def` evaluations
//!   through the stub kernel on every primitive build.
//! - Voxel→Mesh surfacing (`Convert { from: Voxel } → Mesh`): marching-cubes
//!   / level-set surfacing, Fidget's signature feature (arch §10.8), deferred
//!   as a follow-up task gated by the imported-field-source PRD.
//! - BRep→Voxel sampling: deferred to a separate follow-up.
//!
//! # Unconditional `inventory::submit!` decision
//!
//! OpenVDB has only a stub in this v0.2 task, so a `cfg(has_openvdb)` gate
//! would never fire — the registration would be dead code, defeating the
//! point of "fourth integration in the sequence". Submitting unconditionally
//! keeps the cross-crate integration test (step-7) clean and gives the
//! dispatcher BFS a fourth real registered kernel. A follow-up task
//! introducing real OpenVDB FFI can add `cfg(has_openvdb)` gating to switch
//! the factory without changing the registration shape.
//!
//! # Design template
//!
//! `crates/reify-kernel-fidget/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string, supports table contents (Voxel vs
//! Sdf), the stub error string, and the doc comments' references differ.

use reify_types::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

/// Stable identifier for the OpenVDB kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::kernel_registry::registry()`).
///
/// Must equal `KernelId::OpenVdb.to_string()` (`"openvdb"`) so the
/// project-pin lookup in `reify-config` matches the registered adapter at
/// runtime.
///
/// # Lex-min note
///
/// `"openvdb"` sorts AFTER `"fidget"`, `"manifold"`, and `"occt"`
/// lexicographically (because `'p' > 'c'` in `"oc.."` vs `"op.."`). However,
/// no tie-break conflict arises in v0.2 because OpenVDB claims entirely
/// disjoint `(op, repr)` pairs on `Voxel` — not shared with BRep, Mesh, or
/// Sdf. The lex-min tie-break only fires when two kernels claim the _same_
/// `(op, repr)` pair; that is not the case here.
pub const OPENVDB_KERNEL_NAME: &str = "openvdb";

/// Construct the OpenVDB [`CapabilityDescriptor`].
///
/// Enumerates the three Voxel-Boolean operations OpenVDB supports:
/// `BooleanUnion`, `BooleanDifference`, and `BooleanIntersection`, all paired
/// with `ReprKind::Voxel`. Called by the `KernelRegistration::descriptor`
/// function pointer at engine startup (once per `collect_registry()` call,
/// not per geometry op).
///
/// Owned return (`CapabilityDescriptor` by value) because the descriptor's
/// `supports: Vec<...>` field is non-const-constructible — see
/// `reify_types::KernelRegistration` doc for the full rationale.
pub fn openvdb_capability_descriptor() -> CapabilityDescriptor {
    use Operation::*;
    let supports = vec![
        // Voxel Booleans ×3 — OpenVDB's complete capability surface in v0.2.
        (BooleanUnion, ReprKind::Voxel),
        (BooleanDifference, ReprKind::Voxel),
        (BooleanIntersection, ReprKind::Voxel),
    ];
    CapabilityDescriptor { supports }
}

/// Factory invoked by the engine once at startup, returning the stub
/// [`OpenVdbKernel`](crate::kernel::OpenVdbKernel).
///
/// Real OpenVDB FFI is deferred to a follow-up task; this stub factory
/// ensures the `inventory::submit!` below compiles and the registration
/// materialises in `reify_eval::kernel_registry::registry()`. When the
/// follow-up task adds real FFI, this function can switch behind
/// `cfg(has_openvdb)` without changing the registration shape.
fn openvdb_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::kernel::OpenVdbKernel::new())
}

// Unconditional submit — no `cfg(has_openvdb)` gate (see design decisions in
// the module doc). OpenVDB has only a stub in this v0.2 task, so a
// `cfg(has_openvdb)` gate would never fire and the registration would be dead
// code. Submitting unconditionally keeps the cross-crate integration test
// (step-7) clean and gives the dispatcher BFS a fourth real registered kernel
// to exercise on the Voxel repr family.
//
// TODO(has_openvdb): When real OpenVDB FFI lands (follow-up task), flip this
// submit to `#[cfg(any(has_openvdb, test))]` so the stub registers only when
// OpenVDB is actually available or within this crate's own tests. Without that
// gate, any binary that adds `reify-kernel-openvdb` as a non-dev dep will
// unconditionally register the stub kernel — which will, lex-min-wise, win
// over fidget/manifold/occt for any future `(op, Voxel)` claim added during
// implementation drift. The cross-crate isolation in the test layout (openvdb
// dev-deps on reify-eval, not the reverse) blocks that today, but the gate is
// the structural enforcement that must land alongside the real FFI.
//
// TODO(registry-uniqueness): File a follow-up to add a debug-time uniqueness
// assertion in `reify_eval::kernel_registry::collect_registry()` that panics
// if two kernels claim the same `(Operation, ReprKind)` pair. Currently the
// lex-min tie-break in the dispatcher silently picks a winner; a
// `debug_assert!` in the registry collector would surface the conflict at
// startup (in debug builds) rather than after a regression. This guard should
// land alongside the real OpenVDB FFI introduction so that the structural
// enforcement is in place before any `(op, Voxel)` claim can collide.
inventory::submit! {
    KernelRegistration {
        name: OPENVDB_KERNEL_NAME,
        descriptor: openvdb_capability_descriptor,
        factory: openvdb_factory,
    }
}
