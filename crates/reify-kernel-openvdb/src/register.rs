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
//! # `cfg(any(has_openvdb, feature = "stub_register"))` gate on `inventory::submit!`
//!
//! The registration is gated on `cfg(any(has_openvdb, feature = "stub_register"))`
//! so that:
//!
//! - When `/opt/reify-deps` is present (`cfg(has_openvdb)`), the real FFI-backed
//!   kernel is registered and the dispatcher can route `(op, Voxel)` pairs.
//! - When the `stub_register` Cargo feature is on, the stub kernel is registered
//!   so this crate's own integration tests (`tests/dispatcher_integration.rs`,
//!   `tests/inventory_registration.rs`) can exercise the inventory-submit →
//!   registry-materialise → dispatcher-select pipeline in stub-only `cargo test`
//!   runs.  The `stub_register` feature is activated for ALL test binaries via
//!   the self-dev-dep in `Cargo.toml` — `cfg(test)` alone cannot do this because
//!   integration tests in `tests/` are separate compilation units that link the
//!   library built WITHOUT the parent crate's `cfg(test)` flag.
//! - In production stub builds (no `/opt/reify-deps`, no `stub_register`) the
//!   registration is OMITTED entirely — preventing the stub from winning a
//!   future `(op, Voxel)` lex-min tie-break it cannot actually execute.
//!
//! This mirrors the pattern in `crates/reify-kernel-manifold/src/register.rs`
//! (`feature = "stub_register"` + self-dev-dep activation).  When real OpenVDB
//! FFI is the only build path (i.e. `cfg(has_openvdb)` is universally set in
//! supported environments), the `feature = "stub_register"` arm can be removed
//! and the gate simplified to `#[cfg(has_openvdb)]`.
//!
//! # Design template
//!
//! `crates/reify-kernel-fidget/src/register.rs` — same `KERNEL_NAME` const,
//! `*_capability_descriptor()` factory, `*_factory()`, and `inventory::submit!`
//! pattern. Only the kernel name string, supports table contents (Voxel vs
//! Sdf), the stub error string, and the doc comments' references differ.

use reify_ir::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

/// Stable identifier for the OpenVDB kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::kernel_registry::registry()`).
///
/// Must equal `KernelId::OpenVdb.to_string()` (`"openvdb"`) so the
/// project-pin lookup in `reify-config` matches the registered adapter at
/// runtime. Enforced by
/// `crates/reify-config/tests/kernel_name_consistency.rs::openvdb_kernel_name_const_matches_kernel_id_display`.
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

/// Factory invoked by the engine once at startup, returning an `OpenVdbKernel`.
///
/// When `cfg(has_openvdb)` is set, this creates the real FFI-backed kernel
/// (`kernel_real::OpenVdbKernel`). Otherwise it creates the stub kernel
/// (`kernel::OpenVdbKernel`). The single `crate::OpenVdbKernel` ident resolves
/// to the correct type in both cases via the `pub use` cfg-gate in `lib.rs`.
///
/// Gated on `cfg(any(has_openvdb, feature = "stub_register"))` together with
/// the `inventory::submit!` below — the factory is only called from the
/// submit, so leaving it ungated in production stub builds (no
/// `stub_register`, no `has_openvdb`) would emit a dead-code warning.
#[cfg(any(has_openvdb, feature = "stub_register"))]
fn openvdb_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::OpenVdbKernel::new())
}

// Gate on `cfg(any(has_openvdb, feature = "stub_register"))`:
// - has_openvdb: real FFI kernel is available; register it.
// - stub_register: activated by the self-dev-dep in `Cargo.toml` for ALL of
//   this crate's integration test binaries (separate compilation units that
//   do NOT inherit `cfg(test)` from the parent crate).  Without the feature,
//   `tests/dispatcher_integration.rs` and `tests/inventory_registration.rs`
//   would link a stub-mode lib with no `inventory::submit!` and the registry
//   would not contain `"openvdb"`.
//
// In production stub builds (no `has_openvdb`, no `stub_register`) the submit
// is OMITTED — preventing the stub from silently winning a future `(*, Voxel)`
// lex-min routing it cannot actually execute.
//
// See module-level doc for full rationale; mirrors the pattern in
// `crates/reify-kernel-manifold/src/register.rs:106-112`.
#[cfg(any(has_openvdb, feature = "stub_register"))]
inventory::submit! {
    KernelRegistration {
        name: OPENVDB_KERNEL_NAME,
        descriptor: openvdb_capability_descriptor,
        factory: openvdb_factory,
    }
}
