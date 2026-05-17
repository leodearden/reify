//! v0.2 multi-kernel registration surface for OCCT.
//!
//! Declares OCCT's [`CapabilityDescriptor`] (the feasibility table that
//! enumerates every `(Operation, ReprKind)` pair OCCT supports) and — once
//! step 8 lands — submits an `inventory::submit!{ KernelRegistration { ... } }`
//! that the engine collects via `reify_eval::collect_registry()` at startup.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions": each kernel
//! adapter lives in a separate crate, registering via a static linker-
//! collection mechanism (`inventory`) read once at engine startup. The
//! descriptor is feasibility-only — no `cost_hint`, no `error_factor`, no
//! separate `conversions` field. The dispatcher in
//! `crates/reify-eval/src/dispatcher.rs` ranks plans by conversion-stage
//! count alone, with lexicographic tie-breaking on kernel name.
//!
//! # OCCT's op surface
//!
//! Every variant of `GeometryOp` currently routed through the `match op` in
//! `crate::OcctKernel::execute` (lib.rs lines 920-1640+) maps to one entry
//! here, paired with `ReprKind::BRep`. The grouping mirrors the `Operation`
//! enum's section comments: Booleans×3, Primitives×4, Modify×5,
//! Transform×4, Pattern×5, Sweep×8, Curve×6 — total 35 entries.
//!
//! # Convert ×1 (wired in PRD §8 task δ — task 3435)
//!
//! `(Operation::Convert { from: ReprKind::BRep }, ReprKind::Mesh)` was
//! added to the `supports` table in task 3435 (PRD §8 task δ). Combined
//! with a Mesh-native kernel like Manifold, the dispatcher's BFS can now
//! chain `BRep input → OCCT tessellate → Mesh BooleanUnion` automatically
//! without duplicating the union logic in OCCT.
//!
//! # Stub-mode behavior
//!
//! When compiled without `cfg(has_occt)`, the `inventory::submit!` at the
//! bottom of this module does not fire: `reify_eval::collect_registry()`
//! returns an empty set, and the OCCT kernel cannot be instantiated.
//! [`OCCT_KERNEL_NAME`] and [`occt_capability_descriptor`] remain publicly
//! reachable so diagnostic surfaces (e.g. `gui/src-tauri/src/kernel_status.rs`)
//! can display capability information without OCCT linked. Before treating
//! either as evidence of runtime dispatchability, callers must also check
//! `reify_eval::registry().contains_key(OCCT_KERNEL_NAME)`.

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

use reify_types::GeometryKernel;
#[cfg(has_occt)]
use reify_types::KernelRegistration;

/// Stable identifier for the OCCT kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::collect_registry()`'s return type).
/// Lexicographic ordering of registered kernel names provides the PRD's
/// deterministic tie-break — `"occt"` sorts after a hypothetical `"manifold"`
/// or `"fidget"`, so when OCCT and another kernel both claim the same
/// `(Op, BRep)` pair, the alphabetically earlier kernel wins per the
/// dispatcher's tie-break rule.
///
/// Must equal `KernelId::Occt.to_string()` (`"occt"`) so the project-pin
/// lookup in `reify-config` matches the registered adapter at runtime.
/// Enforced by
/// `crates/reify-config/tests/kernel_name_consistency.rs::occt_kernel_name_const_matches_kernel_id_display`.
///
/// # Stub-mode behavior
///
/// In stub mode (no `cfg(has_occt)`), no `inventory::submit!` fires; check
/// `reify_eval::registry().contains_key(OCCT_KERNEL_NAME)` before assuming
/// OCCT is dispatchable — see the [module-level stub-mode note](self).
pub const OCCT_KERNEL_NAME: &str = "occt";

/// Construct the OCCT [`CapabilityDescriptor`].
///
/// Enumerates every `Operation` that OCCT's `execute` body handles, paired
/// with `ReprKind::BRep`. Called by the `KernelRegistration::descriptor`
/// function pointer at engine startup (once per `collect_registry()` call,
/// not per geometry op).
///
/// Owned return (`CapabilityDescriptor` by value) because the descriptor's
/// `supports: Vec<...>` field is non-const-constructible — see
/// `reify_types::KernelRegistration` doc for the full rationale.
///
/// # Stub-mode behavior
///
/// In stub mode (no `cfg(has_occt)`), no `inventory::submit!` fires; check
/// `reify_eval::registry().contains_key(OCCT_KERNEL_NAME)` before assuming
/// OCCT is dispatchable — see the [module-level stub-mode note](self).
pub fn occt_capability_descriptor() -> CapabilityDescriptor {
    use Operation::*;
    let supports = vec![
        // Booleans ×3
        (BooleanUnion, ReprKind::BRep),
        (BooleanDifference, ReprKind::BRep),
        (BooleanIntersection, ReprKind::BRep),
        // Primitives ×4
        (PrimitiveBox, ReprKind::BRep),
        (PrimitiveCylinder, ReprKind::BRep),
        (PrimitiveSphere, ReprKind::BRep),
        (PrimitiveTube, ReprKind::BRep),
        // Modify ×5
        (ModifyFillet, ReprKind::BRep),
        (ModifyChamfer, ReprKind::BRep),
        (ModifyShell, ReprKind::BRep),
        (ModifyDraft, ReprKind::BRep),
        (ModifyThicken, ReprKind::BRep),
        // Transform ×4
        (TransformTranslate, ReprKind::BRep),
        (TransformRotate, ReprKind::BRep),
        (TransformScale, ReprKind::BRep),
        (TransformRotateAround, ReprKind::BRep),
        // Pattern ×5
        (PatternLinear, ReprKind::BRep),
        (PatternCircular, ReprKind::BRep),
        (PatternMirror, ReprKind::BRep),
        (PatternLinear2D, ReprKind::BRep),
        (PatternArbitrary, ReprKind::BRep),
        // Sweep ×8
        (SweepLoft, ReprKind::BRep),
        (SweepExtrude, ReprKind::BRep),
        (SweepRevolve, ReprKind::BRep),
        (SweepSweep, ReprKind::BRep),
        (SweepExtrudeSymmetric, ReprKind::BRep),
        (SweepSweepGuided, ReprKind::BRep),
        (SweepLoftGuided, ReprKind::BRep),
        (SweepPipe, ReprKind::BRep),
        // Curve ×6
        (CurveLineSegment, ReprKind::BRep),
        (CurveArc, ReprKind::BRep),
        (CurveHelix, ReprKind::BRep),
        (CurveInterpCurve, ReprKind::BRep),
        (CurveBezierCurve, ReprKind::BRep),
        (CurveNurbsCurve, ReprKind::BRep),
        // Convert ×1 — BRep→Mesh tessellation (PRD §8 task δ, task 3435)
        (Convert { from: ReprKind::BRep }, ReprKind::Mesh),
    ];
    CapabilityDescriptor { supports }
}

/// Factory invoked by `Engine::with_registered_kernel` once at startup.
///
/// Spawns the dedicated OCCT actor thread via [`crate::OcctKernelHandle::spawn`]
/// — preserving the single-threaded actor pattern that ensures OCCT's
/// process-global state stays on one OS thread (per CLAUDE.md and the
/// `gui/src-tauri/src/kernel_status.rs` "register only when available"
/// pattern this submit mirrors).
///
/// # Always-defined (regardless of `cfg(has_occt)`)
///
/// Unconditionally compiled (rather than `#[cfg(has_occt)]`) so that
/// `tests/inventory_registration.rs`, which references `register::occt_factory`
/// outside any cfg gate, *compiles* under stub-mode builds. This is a
/// compile-time requirement: the test binary must type-check in stub mode even
/// though the factory pin assertion is never reached at runtime (the test
/// early-returns when `OCCT_AVAILABLE` is false — stub-mode runs are a no-op).
///
/// Note the deliberate asymmetry with Manifold: Manifold twin-gates both
/// `manifold_factory` and its `inventory::submit!` under
/// `#[cfg(feature = "stub_register")]`. OCCT cannot follow that pattern because
/// `cfg(has_occt)` is set only when building the OCCT crate itself, not when
/// compiling the integration-test binary — so `occt_factory` must stay ungated
/// for the test binary to compile under stub mode.
///
/// The function body references `crate::OcctKernelHandle::spawn()`, which
/// resolves to the real handle under `cfg(has_occt)` and to the stub
/// (`crates/reify-kernel-occt/src/stubs.rs`) otherwise — both define
/// `pub fn spawn() -> Self` and `impl GeometryKernel for OcctKernelHandle`, so
/// the return type compiles in both modes. Under stub mode the returned handle's
/// every geometry operation surfaces an "OCCT not available" error; callers must
/// check `reify_eval::registry().contains_key(OCCT_KERNEL_NAME)` before invoking
/// the factory — the `inventory::submit!` gate already prevents stub-mode dispatch.
pub fn occt_factory() -> Box<dyn GeometryKernel> {
    Box::new(crate::OcctKernelHandle::spawn())
}

// `cfg(has_occt)` is the deliberate gate, mirroring the existing
// `gui/src-tauri/src/kernel_status.rs:48-57` "register only when available"
// pattern. When OCCT C++ libs are absent, the submit doesn't fire — the
// `reify_eval::collect_registry()` set is left empty and the engine surfaces
// "no geometry kernel registered" cleanly via the existing error paths,
// strictly preferable to a non-functional stub registration that would
// error on every operation while still appearing in the registry.
#[cfg(has_occt)]
inventory::submit! {
    KernelRegistration {
        name: OCCT_KERNEL_NAME,
        descriptor: occt_capability_descriptor,
        factory: occt_factory,
    }
}
