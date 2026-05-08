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
//! # Feature-gated `inventory::submit!` decision (`stub_register`)
//!
//! The `inventory::submit!` and its helper `manifold_factory()` are gated on
//! `#[cfg(feature = "stub_register")]` for production safety:
//!
//! **Production builds** (no `stub_register` feature) — the submit is a
//! no-op; Manifold contributes no entry to
//! `reify_eval::kernel_registry::registry()`. This prevents the
//! lexicographic tie-break rule (`"manifold" < "occt"`) from silently
//! routing geometry ops through an unimplemented stub kernel when no
//! operator has explicitly requested Manifold.
//!
//! **Test builds** (`feature = "stub_register"`) — the submit fires so
//! the registry exercised by the integration tests includes the Manifold
//! entry.  The `stub_register` feature is activated for ALL test builds
//! (both `cargo test --lib` in-crate and cross-crate integration test
//! binaries in `tests/`) via the self-dev-dep in `[dev-dependencies]`
//! (see `Cargo.toml`).  Integration test binaries are SEPARATE
//! compilation units that do not inherit `cfg(test)` from the parent
//! crate, so the self-dev-dep is the only reliable activation path.
//! Note: `cfg(test)` is NOT used here — the `compile_error!` guard in
//! each integration test binary (see `tests/common/mod.rs`)
//! provides an actionable compile-time message if the self-dev-dep
//! activation is ever removed, making `cfg(test)` as a fallback
//! unnecessary.
//!
//! When real Manifold C++ FFI ships, rename `stub_register` to
//! `has_manifold` (matching OCCT's `has_occt` build.rs gate) and replace
//! the self-dev-dep with the build.rs detection mechanism, making the gate
//! structurally identical to OCCT's.
//!
//! # Design template
//!
//! `crates/reify-kernel-occt/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string, supports table contents, and the
//! cfg-key (`has_occt` vs `stub_register`) differ.

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

#[cfg(feature = "stub_register")]
use reify_types::{GeometryKernel, KernelRegistration};

/// Factory invoked by the engine once at startup, returning the stub
/// [`ManifoldKernel`](crate::kernel::ManifoldKernel).
///
/// Gated on `cfg(feature = "stub_register")` together with the
/// `inventory::submit!` below — the factory is only called from the submit,
/// so leaving it ungated in non-feature builds would emit a dead-code
/// warning. When real Manifold C++ FFI ships and the gate becomes
/// `cfg(has_manifold)`, this factory switches to the real implementation
/// without changing the registration shape.
#[cfg(feature = "stub_register")]
pub fn manifold_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::kernel::ManifoldKernel::new())
}

// Feature-gated submit — see "Feature-gated `inventory::submit!` decision"
// in the module doc.  `feature = "stub_register"` covers all test builds:
// both in-crate `cargo test --lib` (via Cargo's self-dev-dep feature
// unification) and cross-crate integration test binaries in `tests/`
// (separate compilation units that don't see the parent crate's
// `cfg(test)`).  A `cfg(test)` fallback is intentionally omitted to
// match OCCT's gate shape — the `compile_error!` guard in
// `tests/common/mod.rs` provides an actionable compile-time
// error if the self-dev-dep activation is ever removed.
//
// Both items are gated together: `manifold_factory` is only called from this
// submit, so a dead-code warning would fire if the factory were ungated while
// the submit were absent in non-feature builds.
//
// TODO(has_manifold): When real Manifold C++ FFI lands, rename `stub_register`
// to `has_manifold` (matching OCCT's `has_occt` build.rs gate) and replace
// the self-dev-dep in `Cargo.toml` with the build.rs detection mechanism.
// The gate shape — `#[cfg(has_manifold)]` on both items — stays identical
// to what it is today.
#[cfg(feature = "stub_register")]
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
