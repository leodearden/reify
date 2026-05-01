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
//! # v0.3 forward-compat note
//!
//! When v0.3 exposes OCCT tessellation as a first-class registered
//! conversion, the `supports` table will gain
//! `(Operation::Convert { from: ReprKind::BRep }, ReprKind::Mesh)`. That
//! entry — combined with a Mesh-native kernel like Manifold — would let
//! the dispatcher's BFS chain `BRep input → OCCT tessellate → Mesh
//! BooleanUnion` automatically without duplicating the union logic in
//! OCCT.

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// Stable identifier for the OCCT kernel in the v0.2 multi-kernel registry.
///
/// Used as both the `KernelRegistration::name` and the BTreeMap key in the
/// dispatcher registry (`reify_eval::collect_registry()`'s return type).
/// Lexicographic ordering of registered kernel names provides the PRD's
/// deterministic tie-break — `"occt"` sorts after a hypothetical `"manifold"`
/// or `"fidget"`, so when OCCT and another kernel both claim the same
/// `(Op, BRep)` pair, the alphabetically earlier kernel wins per the
/// dispatcher's tie-break rule.
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
    ];
    CapabilityDescriptor { supports }
}
