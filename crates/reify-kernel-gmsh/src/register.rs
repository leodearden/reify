//! v0.3 multi-kernel registration surface for Gmsh.
//!
//! Declares Gmsh's [`CapabilityDescriptor`] (the feasibility table that
//! enumerates every `(Operation, ReprKind)` pair Gmsh supports) and submits
//! a [`KernelRegistration`] via `inventory::submit!` that the engine
//! collects via `reify_eval::kernel_registry::registry()` at startup.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/structural-analysis-fea.md` "FEA mesher" + the v0.2
//! multi-kernel doc's "Resolved design decisions" carry forward here: the
//! descriptor is feasibility-only — no `cost_hint`, no `error_factor`.
//! The dispatcher in `crates/reify-eval/src/dispatcher.rs` ranks plans by
//! conversion-stage count alone, with lexicographic tie-breaking on kernel
//! name.
//!
//! # Gmsh's op surface
//!
//! Gmsh operates on the surface→volume tetrahedral meshing axis. It does
//! NOT tessellate B-rep (OCCT's territory), perform mesh Booleans
//! (Manifold's territory), evaluate SDFs (Fidget's territory), or perform
//! voxel Booleans (OpenVDB's territory). The descriptor therefore declares
//! exactly one entry:
//! - `(Convert { from: Mesh }, VolumeMesh)`
//!
//! # `cfg(any(has_gmsh, feature = "stub_register"))` gate on `inventory::submit!`
//!
//! The registration is gated on `cfg(any(has_gmsh, feature = "stub_register"))`
//! so that:
//!
//! - When `/opt/reify-deps` is present (`cfg(has_gmsh)`), the real FFI-backed
//!   kernel is registered and the dispatcher can route
//!   `(Convert{from: Mesh}, VolumeMesh)`.
//! - When the `stub_register` Cargo feature is on, the stub kernel is
//!   registered so this crate's own integration tests
//!   (`tests/dispatcher_integration.rs`, `tests/inventory_registration.rs`)
//!   can exercise the inventory-submit → registry-materialise →
//!   dispatcher-select pipeline in stub-only `cargo test` runs. The
//!   `stub_register` feature is activated for ALL test binaries via the
//!   self-dev-dep in `Cargo.toml` — `cfg(test)` alone cannot do this because
//!   integration tests in `tests/` are separate compilation units that link
//!   the library built WITHOUT the parent crate's `cfg(test)` flag.
//! - In production stub builds (no `/opt/reify-deps`, no `stub_register`)
//!   the registration is OMITTED entirely — preventing the stub from winning
//!   the `(Convert{from: Mesh}, VolumeMesh)` routing it cannot actually
//!   execute. Downstream consumers get a clean "no kernel available"
//!   dispatcher error rather than a confusing stub `OperationFailed`.
//!
//! This mirrors the pattern in `crates/reify-kernel-openvdb/src/register.rs`
//! (`feature = "stub_register"` + self-dev-dep activation).
//!
//! # Design template
//!
//! `crates/reify-kernel-openvdb/src/register.rs` — the closest design
//! template (cfg-gated `inventory::submit!`, single coherent op surface).
//! Only the kernel name, the supports table content
//! (`(Convert{from: Mesh}, VolumeMesh)` instead of three Voxel Booleans),
//! and the doc-comment references differ.

use reify_ir::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

/// Stable identifier for the Gmsh kernel in the v0.3 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::kernel_registry::registry()`).
///
/// Must equal `KernelId::Gmsh.to_string()` (`"gmsh"`) so the project-pin
/// lookup in `reify-config` matches the registered adapter at runtime.
/// Enforced by
/// `crates/reify-config/tests/kernel_name_consistency.rs::gmsh_kernel_name_const_matches_kernel_id_display`.
///
/// # Lex-min note
///
/// `"gmsh"` sorts BEFORE `"manifold"`, `"occt"`, `"openvdb"` and AFTER
/// `"fidget"` lexicographically. However, Gmsh's `(Convert{from: Mesh},
/// VolumeMesh)` claim is unique to gmsh — no other v0.2/v0.3 kernel claims
/// `VolumeMesh` as either input or output — so the lex-min tie-break in
/// `dispatcher::dispatch` (which fires only on identical `(op, repr)`
/// pairs) cannot route a non-gmsh kernel to gmsh's territory.
pub const GMSH_KERNEL_NAME: &str = reify_core::KernelId::Gmsh.as_registry_name();

/// Construct the Gmsh [`CapabilityDescriptor`].
///
/// Enumerates the singular surface→volume tet meshing operation gmsh
/// supports: `(Convert{from: Mesh}, VolumeMesh)`. Called by the
/// `KernelRegistration::descriptor` function pointer at engine startup
/// (once per `collect_registry()` call, not per geometry op).
///
/// Owned return (`CapabilityDescriptor` by value) because the
/// descriptor's `supports: Vec<...>` field is non-const-constructible —
/// see `reify_types::KernelRegistration` doc for the full rationale.
pub fn gmsh_capability_descriptor() -> CapabilityDescriptor {
    let supports = vec![
        // Surface→volume tet meshing — gmsh's complete capability surface in v0.3.
        (
            Operation::Convert {
                from: ReprKind::Mesh,
            },
            ReprKind::VolumeMesh,
        ),
    ];
    CapabilityDescriptor { supports }
}

/// Factory invoked by the engine once at startup, returning a `GmshKernel`.
///
/// When `cfg(has_gmsh)` is set, this creates the real FFI-backed kernel
/// (`kernel_real::GmshKernel`). Otherwise it creates the stub kernel
/// (`kernel::GmshKernel`). The single `crate::GmshKernel` ident resolves to
/// the correct type in both cases via the `pub use` cfg-gate in `lib.rs`.
///
/// Gated on `cfg(any(has_gmsh, feature = "stub_register"))` together with
/// the `inventory::submit!` below — the factory is only called from the
/// submit, so leaving it ungated in production stub builds (no
/// `stub_register`, no `has_gmsh`) would emit a dead-code warning.
#[cfg(any(has_gmsh, feature = "stub_register"))]
fn gmsh_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::GmshKernel::new())
}

// Gate on `cfg(any(has_gmsh, feature = "stub_register"))`:
// - has_gmsh: real FFI kernel is available; register it.
// - stub_register: activated by the self-dev-dep in `Cargo.toml` for ALL of
//   this crate's integration test binaries (separate compilation units that
//   do NOT inherit `cfg(test)` from the parent crate). Without the feature,
//   `tests/dispatcher_integration.rs` and `tests/inventory_registration.rs`
//   would link a stub-mode lib with no `inventory::submit!` and the registry
//   would not contain `"gmsh"`.
//
// In production stub builds (no `has_gmsh`, no `stub_register`) the submit
// is OMITTED — preventing the stub from silently winning the
// `(Convert{from: Mesh}, VolumeMesh)` routing it cannot actually execute.
//
// See module-level doc for full rationale; mirrors the pattern in
// `crates/reify-kernel-openvdb/src/register.rs:142`.
#[cfg(any(has_gmsh, feature = "stub_register"))]
inventory::submit! {
    KernelRegistration {
        name: GMSH_KERNEL_NAME,
        descriptor: gmsh_capability_descriptor,
        factory: gmsh_factory,
    }
}
