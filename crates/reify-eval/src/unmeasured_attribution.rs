//! ζ's vocabulary for a measurement a surface did not take (task 6169, extended
//! by #6667): which measurement, the reason an Indeterminate records for it, and
//! the `ConstraintIndeterminate` Info rendered from that reason. A leaf: the
//! `RepresentationWithin` dispatch and the viewport pass both import it.

use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode};
use reify_ir::{IndeterminateReason, TransientReason};

/// Which measurement a surface left untaken; selects the [`unmeasured_reason`]
/// wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnmeasuredMeasurement {
    RepresentationWithin,
    GdtConformance,
    DfmRule,
}

/// The two engine facts that select an unmeasured reason's remedy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeasurementCapability {
    /// A registered kernel claims a `(_, ReprKind::BRep)` pair
    /// (`Engine::has_repr_capable_kernel`).
    pub(crate) brep_kernel: bool,
    /// This surface captures achieved representation tolerance
    /// (`Engine::capture_repr_tol`).
    pub(crate) captures_repr_tol: bool,
}

/// The reason an Indeterminate records — and its diagnostic states — when a
/// run did not take the `kind` of measurement it needs (task-6169 ζ,
/// C-SURFACE 1). For a `RepresentationWithin` that is a run whose
/// `achieved_repr_tol` is empty.
///
/// A pure function of two engine properties, because an empty map has THREE
/// distinct causes and each has a different fix:
///   1. **No kernel able to tessellate the subject is registered** — this
///      run cannot measure anything at all (a stub-mode binary, whichever
///      subcommand asked) → the remedy is a kernel.
///   2. **Capable kernel present, capture OFF** — nobody ever asked for a
///      measurement on this surface (`reify build`, `reify eval`) → the
///      remedy is `reify check`.
///   3. **Capable kernel present, capture ON, still nothing measured** —
///      tessellation ran (or would have) but produced no achieved deviation
///      for THIS subject.
///
/// Kernel capability is tested FIRST, ahead of `capture_repr_tol`, so the
/// remedy handed to the user is always TERMINAL.  Testing capture first
/// sent a stub-mode `reify build` into arm 2 — "run `reify check`" — whose
/// only answer on that same binary is arm 1's "build with OCCT": a two-hop
/// dead end, and a milder instance of the very defect class C-SURFACE 1
/// exists to remove.
///
/// ## Why capability, not `default_kernel_name.is_none()`
///
/// An earlier revision discriminated on `default_kernel_name.is_none()`.
/// That is **never true on any shipped binary**, so arm 1 was dead code on
/// exactly the surfaces it was written for.  `reify-kernel-manifold`'s
/// `inventory::submit!` is UNCONDITIONAL (no `cfg` gate — its `manifold3d`
/// dep compiles its own C++ tree, so there is no "manifold absent" case),
/// and `crates/reify-cli/src/main.rs`'s `extern crate reify_kernel_manifold
/// as _;` states verbatim that the `"manifold"` key is always present in
/// the binary's registry.  `pick_lexmin_brep_kernel` ends in
/// `.or_else(|| registered.values().next())`, so a stub-mode (no-OCCT)
/// binary still gets `default_kernel_name == Some("manifold")` — a
/// Mesh-only kernel that cannot tessellate the B-rep subject.  The old test
/// coverage passed only because `Engine::new(checker, None)` (unit tests)
/// is the sole shape that reaches an empty registry.
///
/// So the question arm 1 must actually ask is not "is *a* kernel
/// registered" but "is a kernel that can produce the measurement
/// registered".  OCCT is the only BRep producer in the workspace
/// (`reify-kernel-occt`'s submit is `cfg(has_occt)`-gated; manifold claims
/// Mesh only, openvdb Voxel/Mesh), so `supports_any_repr(ReprKind::BRep)`
/// over this engine's own kernels is an exact test for "this binary was
/// built with OCCT".
///
/// Capability is read off the static registry keyed by this engine's own
/// `geometry_kernels` names, because `GeometryKernel` is a behavioural
/// trait with no descriptor accessor — see `Engine::has_repr_capable_kernel`.
/// Kernel-absence is still read as a fact about the BINARY rather than merely
/// this engine because every surface that both dispatches constraints and can
/// carry a `RepresentationWithin` builds its engine with
/// `Engine::with_registered_kernel` (`cmd_check`'s assertion branch,
/// `cmd_build`, the GUI session); `eval()` never dispatches constraints, so
/// the CLI's deliberately kernel-free `eval` engines cannot reach here.
///
/// Emitting arm 2's remedy unconditionally would also put "run `reify
/// check`" into the diagnostics of the pre-`check()` pass inside
/// `tessellate_realizations` (engine_build.rs), which runs before the map is
/// populated and is therefore precisely a surface that DOES measure.
///
/// Arm 3 is deliberately CAUSE-NEUTRAL and mentions neither `kernel` nor
/// `OCCT`.  Blaming the kernel there would be a false statement — one is
/// demonstrably registered — and exactly the INV-SF-4 misattribution this
/// task exists to remove, merely relocated from the operand kinds to the
/// kernel.  With a kernel present the engine genuinely cannot tell whether
/// the subject declares no realization at all, or declares one whose
/// type-name scan key could not be resolved, so it states the observable
/// fact and points at the actionable check rather than asserting a cause it
/// has not established.
///
/// ## GD&T conformance and DFM rules (task #6667)
///
/// The viewport surface measures neither, so their reasons have two arms:
/// capability first (keeping the remedy terminal, as above), then the
/// surface. Arm 3 has no analogue because no surface that emits these
/// reasons ever measures them, and their wording does not claim "no
/// realization ran": the viewport does tessellate for display.
///
/// BRep capability is their capability question too. `MaxDeviation`, the
/// GD&T query, is answered only by the OCCT adapter, and the DFM overhang and
/// draft selectors read B-rep face normals. A thickness-only DFM rule is the
/// one inexact case: its SDF path needs OpenVDB plus a default kernel that
/// tessellates the subject, not a BRep claim. One predicate for every
/// unmeasured posture is the S12 ruling
/// (check-surface-subject-and-measurement-context.md D6).
pub(crate) fn unmeasured_reason(
    kind: UnmeasuredMeasurement,
    capability: MeasurementCapability,
) -> &'static str {
    let MeasurementCapability {
        brep_kernel,
        captures_repr_tol,
    } = capability;
    match kind {
        UnmeasuredMeasurement::RepresentationWithin if !brep_kernel => {
            "this run does not measure representation tolerance because no \
             geometry kernel able to tessellate the subject is registered; \
             such a geometry kernel is required — build with OCCT to evaluate \
             this RepresentationWithin bound"
        }
        UnmeasuredMeasurement::RepresentationWithin if !captures_repr_tol => {
            "this evaluation surface does not measure representation tolerance \
             (no tessellation ran, so no achieved deviation exists for the \
             subject); run `reify check` to evaluate this RepresentationWithin \
             bound"
        }
        UnmeasuredMeasurement::RepresentationWithin => {
            "no realization of the subject was tessellated on this run, so no \
             achieved deviation exists for it and this run does not measure \
             representation tolerance; check that the subject declares a \
             realization"
        }
        UnmeasuredMeasurement::GdtConformance if !brep_kernel => {
            "this run does not measure geometric conformance because no \
             geometry kernel able to measure the subject is registered; such a \
             geometry kernel is required — build with OCCT to evaluate this \
             Conforms callout"
        }
        UnmeasuredMeasurement::GdtConformance => {
            "this evaluation surface does not measure geometric conformance \
             (no deviation of the `actual` geometry from the nominal feature is \
             computed here); run `reify check` to evaluate this Conforms callout"
        }
        UnmeasuredMeasurement::DfmRule if !brep_kernel => {
            "this run does not measure DFM rules because no geometry kernel \
             able to measure the rule's subject is registered; such a geometry \
             kernel is required — build with OCCT to evaluate this DFM rule"
        }
        UnmeasuredMeasurement::DfmRule => {
            "this evaluation surface does not measure DFM rules (no geometry \
             query is run against the rule's subject here); run `reify check` \
             to evaluate this DFM rule"
        }
    }
}

/// The recorded cause of an Indeterminate whose measurement was not taken.
pub(crate) fn measurement_unavailable(detail: &str) -> IndeterminateReason {
    IndeterminateReason::Transient(TransientReason::MeasurementUnavailable {
        detail: detail.to_string(),
    })
}

/// ζ's attribution of an unmeasured constraint, rendered from its recorded
/// `reason`. `id` is embedded so `Engine::labeled_diagnostics` can name it.
pub(crate) fn unmeasured_constraint_attribution(
    id: &ConstraintNodeId,
    reason: &IndeterminateReason,
) -> Diagnostic {
    Diagnostic::info(format!("constraint {id} indeterminate: {reason}"))
        .with_code(DiagnosticCode::ConstraintIndeterminate)
}

/// ζ's attribution of an unmeasured DFM rule, named by `rule_name`: a DFM rule
/// has no `ConstraintNodeId`.
pub(crate) fn unmeasured_dfm_attribution(name: &str, reason: &IndeterminateReason) -> Diagnostic {
    Diagnostic::info(format!("DFM rule `{name}` indeterminate: {reason}"))
        .with_code(DiagnosticCode::ConstraintIndeterminate)
}
