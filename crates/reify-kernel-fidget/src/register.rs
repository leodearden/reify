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
//! territory). The descriptor therefore declares exactly three entries:
//! - `(BooleanUnion, Sdf)`
//! - `(BooleanDifference, Sdf)`
//! - `(BooleanIntersection, Sdf)`
//!
//! Deliberately excluded from the v0.2 descriptor:
//! - SDF primitives: deferred to avoid routing `field def` evaluations through
//!   the stub kernel on every primitive build.
//! - SDF→Mesh feature-preserving meshing (`Convert { from: Sdf } → Mesh`):
//!   Fidget's signature feature (arch §10.8), deferred as a follow-up task
//!   that requires a Mesh-native consumer.
//! - BRep→SDF distance-field sampling: deferred to a separate follow-up.
//!
//! # Unconditional `inventory::submit!` decision
//!
//! Fidget has only a stub in this v0.2 task, so a `cfg(has_fidget)` gate
//! would never fire — the registration would be dead code, defeating the point
//! of "second integration in the sequence". Submitting unconditionally keeps
//! the cross-crate integration test (step-7) clean and gives the dispatcher
//! BFS a third real registered kernel. A follow-up task introducing real
//! Fidget Rust JIT FFI can add `cfg(has_fidget)` gating to switch the factory
//! without changing the registration shape.
//!
//! # Design template
//!
//! `crates/reify-kernel-manifold/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string, supports table contents (Sdf vs Mesh),
//! the stub error string, and the doc comments' references differ.

use reify_types::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

/// Factory invoked by the engine once at startup, returning the stub
/// [`FidgetKernel`](crate::kernel::FidgetKernel).
///
/// Real Fidget Rust JIT FFI is deferred to a follow-up task; this stub factory
/// ensures the `inventory::submit!` below compiles and the registration
/// materialises in `reify_eval::kernel_registry::registry()`. When the
/// follow-up task adds real FFI, this function can switch behind
/// `cfg(has_fidget)` without changing the registration shape.
fn fidget_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::kernel::FidgetKernel::new())
}

// Unconditional submit — no `cfg(has_fidget)` gate (see design decisions in
// the module doc). Fidget has only a stub in this v0.2 task, so a
// `cfg(has_fidget)` gate would never fire and the registration would be dead
// code. Submitting unconditionally keeps the cross-crate integration test
// (step-7) clean and gives the dispatcher BFS a third real registered kernel
// to exercise on the Sdf repr family.
//
// TODO(has_fidget): When real Fidget Rust JIT FFI lands (follow-up task), flip
// this submit to `#[cfg(any(has_fidget, test))]` so the stub registers only
// when Fidget is actually available or within this crate's own tests. Without
// that gate, any binary that adds `reify-kernel-fidget` as a non-dev dep will
// unconditionally register the stub kernel — which will, lex-min-wise, win over
// manifold/occt for any future `(op, Sdf)` claim added during implementation
// drift. The cross-crate isolation in the test layout (fidget dev-deps on
// reify-eval, not the reverse) blocks that today, but the gate is the
// structural enforcement that must land alongside the real FFI.
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
///
/// # Lex-min note
///
/// `"fidget"` sorts before `"manifold"` and `"occt"` lexicographically.
/// However, no tie-break conflict arises in v0.2 because Fidget, Manifold, and
/// OCCT claim entirely disjoint `(op, repr)` pairs (Sdf vs Mesh vs BRep). The
/// lex-min tie-break only fires when two kernels claim the _same_ `(op, repr)`
/// pair; that is not the case here.
pub const FIDGET_KERNEL_NAME: &str = "fidget";

/// Construct the Fidget [`CapabilityDescriptor`].
///
/// Enumerates the three SDF-Boolean operations Fidget supports:
/// `BooleanUnion`, `BooleanDifference`, and `BooleanIntersection`, all paired
/// with `ReprKind::Sdf`. Called by the `KernelRegistration::descriptor`
/// function pointer at engine startup (once per `collect_registry()` call,
/// not per geometry op).
///
/// Owned return (`CapabilityDescriptor` by value) because the descriptor's
/// `supports: Vec<...>` field is non-const-constructible — see
/// `reify_types::KernelRegistration` doc for the full rationale.
pub fn fidget_capability_descriptor() -> CapabilityDescriptor {
    use Operation::*;
    let supports = vec![
        // SDF Booleans ×3 — Fidget's complete capability surface in v0.2.
        (BooleanUnion, ReprKind::Sdf),
        (BooleanDifference, ReprKind::Sdf),
        (BooleanIntersection, ReprKind::Sdf),
    ];
    CapabilityDescriptor { supports }
}
