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
//! # Unconditional `inventory::submit!` decision
//!
//! Gmsh has only a stub in this v0.3 task, so a `cfg(has_gmsh)` gate would
//! never fire — the registration would be dead code, defeating the point
//! of "fifth integration in the sequence". Submitting unconditionally
//! gives the dispatcher BFS a fifth real registered kernel that exercises
//! the surface→volume route end-to-end, and lets sibling consumers
//! (`reify-solver-elastic` #2914) depend on the public types and
//! dispatcher routing today rather than waiting for the FFI follow-up.
//!
//! Crucially, the lex-min tie-break risk that motivates `stub_register`
//! gates on other adapters does NOT apply here: Gmsh's
//! `(Convert{from: Mesh}, VolumeMesh)` claim is unique — no other v0.2/v0.3
//! kernel claims `VolumeMesh` as either input or output. There is no risk
//! of shadowing other kernels' lex-min picks because no other kernel
//! competes for this `(op, repr)` pair.
//!
//! TODO(has_gmsh): Follow-up task #3092 will switch the factory to a real-
//! FFI impl behind `cfg(has_gmsh)` without changing the registration shape.
//!
//! # Design template
//!
//! `crates/reify-kernel-openvdb/src/register.rs` — the closest design
//! template (stub-only adapter, unconditional `inventory::submit!`, single
//! coherent op surface). Only the kernel name, the supports table content
//! (`(Convert{from: Mesh}, VolumeMesh)` instead of three Voxel Booleans),
//! and the doc-comment references differ.

use reify_types::{CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind};

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
pub const GMSH_KERNEL_NAME: &str = "gmsh";

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

/// Factory invoked by the engine once at startup, returning the stub
/// [`GmshKernel`](crate::kernel::GmshKernel).
///
/// Real Gmsh FFI is deferred to follow-up task #3092; this stub factory
/// ensures the `inventory::submit!` below compiles and the registration
/// materialises in `reify_eval::kernel_registry::registry()`. When the
/// follow-up task adds real FFI, this function can switch behind
/// `cfg(has_gmsh)` without changing the registration shape.
fn gmsh_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::GmshKernel::new())
}

// Unconditional submit — no `cfg(has_gmsh)` gate. See module-level docs for
// the rationale (gmsh has a unique (op, repr) claim that cannot collide
// with other kernels' lex-min picks; the registration is needed today so
// the dispatcher BFS routes Mesh→VolumeMesh requests, even on a build
// where libgmsh is absent — the routing produces a `GeometryError` from
// the stub kernel rather than silently failing to plan).
//
// TODO(has_gmsh): Follow-up task #3092 will switch this submit to
// `#[cfg(any(has_gmsh, test))]` so the stub registers only when libgmsh is
// actually available or within this crate's own tests. The structural
// enforcement that gates the submit on FFI availability lands alongside
// the real Gmsh extern "C" surface.
inventory::submit! {
    KernelRegistration {
        name: GMSH_KERNEL_NAME,
        descriptor: gmsh_capability_descriptor,
        factory: gmsh_factory,
    }
}
