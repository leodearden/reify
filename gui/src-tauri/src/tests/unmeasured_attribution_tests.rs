//! Task 6667 (η′): attributable-Indeterminate status for GD&T `Conforms` and
//! DFM rules on the **GUI evaluation surface**.
//!
//! Sibling of task 6169 (ζ), which established the attribution channel for
//! `RepresentationWithin`: a `Severity::Info` [`reify_core::Diagnostic`] carrying
//! [`reify_core::DiagnosticCode::ConstraintIndeterminate`] and a reason clause
//! that names the SURFACE and a terminal REMEDY, rather than blaming the model.
//! This module extends that ruling by analogy to the two other kinds the GUI
//! surface leaves unmeasured — and it deliberately reuses ζ's channel rather
//! than inventing a parallel one.
//!
//! ## Why these pins live in the GUI crate
//!
//! The claim under test is about a SURFACE, not about an arbitrary engine, so
//! every test here boots the production GUI configuration —
//! [`crate::engine::EngineSession::with_registered_kernel`], the exact call
//! `main.rs:842` makes (`kernel_status_tests.rs:106` is the in-repo precedent
//! for driving it from a test). A hand-built `Engine::new` would prove nothing
//! about the GUI.
//!
//! Each test asserts `realization_handles_len() == 0` as an explicit SURFACE
//! PRECONDITION. The GUI's production geometry path is
//! `Engine::tessellate_snapshot`, never `build()`/`realize_for_check`, so the
//! handle map stays empty on every production GUI path. Pinning that fact makes
//! these tests fail loudly if a future change starts realizing (the separately
//! chartered on-demand-measurement work would do exactly that) instead of
//! silently passing for the wrong reason.
//!
//! The cheap RED/GREEN engine-level loop lives in
//! `crates/reify-eval/tests/unmeasured_surface_attribution.rs`; THIS module is
//! the contract.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};

use crate::engine::EngineSession;

/// A module carrying exactly ONE geometric `Conforms` callout.
///
/// The η detection signal `measure_gdt_conformance` keys on is an EXPLICIT
/// `actual:` arg binding — the `Conforms` predicate never references `actual`,
/// so the binding, not the body, is the only trace of geometric intent. The
/// shape is copied from `engine_constraints.rs`'s `MIXED_SOURCE`
/// (`check_weaves_geometric_conforms_override_only`), which is already proven
/// to compile and dispatch without a geometry kernel.
const GDT_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

/// The terminal-remedy tokens acceptable on the binary in hand.
///
/// `unmeasured_reason_for` tests kernel CAPABILITY before it tests the surface,
/// so whatever remedy it names can actually work here: with OCCT linked the
/// terminal remedy is `reify check` (it registers the kernel and measures); in
/// stub mode `reify check` is a two-hop dead end, so the remedy jumps straight
/// to the kernel. Asserting `"reify check"` unconditionally would PASS in stub
/// mode while recommending something that cannot answer there — precisely the
/// misattribution class this work removes. Rationale and shape copied from
/// `cli_representation_within.rs:136-159`.
#[cfg(feature = "gui")]
fn acceptable_remedy_tokens() -> &'static [&'static str] {
    if crate::kernel_status::current_kernel_status().available {
        &["reify check"]
    } else {
        &["OCCT"]
    }
}

/// Without the `gui` feature `reify-kernel-occt` is not a dependency of this
/// crate at all, so there is no `OCCT_AVAILABLE` to discriminate on. Assert the
/// DISJUNCTION rather than silently narrowing to the wrong arm — the exact-arm
/// coverage is carried by the `gui`-featured build above.
#[cfg(not(feature = "gui"))]
fn acceptable_remedy_tokens() -> &'static [&'static str] {
    &["reify check", "OCCT"]
}

/// Assert `message` names a terminal remedy reachable on this binary.
fn assert_names_a_terminal_remedy(message: &str) {
    let accepted = acceptable_remedy_tokens();
    assert!(
        accepted.iter().any(|t| message.contains(t)),
        "the reason must end in a TERMINAL remedy reachable on this binary \
         (one of {accepted:?}); got: {message}"
    );
}

/// Collect the surface-attribution diagnostics out of the session's last check:
/// `Severity::Info` + `DiagnosticCode::ConstraintIndeterminate`, ζ's channel.
fn attribution_messages(session: &EngineSession) -> Vec<String> {
    session
        .core_state_for_test()
        .last_check()
        .expect("load_from_source must populate last_check")
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Info && d.code == Some(DiagnosticCode::ConstraintIndeterminate)
        })
        .map(|d| d.message.clone())
        .collect()
}

/// Step-1 / the GD&T half of the task's named signal.
///
/// On the GUI surface a geometric `Conforms` comes back `Indeterminate` — and
/// that verdict is correct, because nothing was realized. What is NOT correct
/// today is the only thing said about it: `measure_gdt_conformance` emits a
/// **Warning** reading `Conforms INDETERMINATE: could not resolve the nominal
/// \`feature\` geometry handle`, which blames the MODEL's handle and names
/// neither the surface nor a remedy. That is the INV-SF-4 class ζ exists to
/// remove, one hop over.
///
/// The pin asserts the ζ-shaped `Severity::Info` attribution is present
/// alongside it, pinning only the stable TOKENS and never a whole sentence —
/// ζ's deliberate pinning contract (`representation_within_assertion.rs:185`).
#[test]
fn gui_surface_gdt_conforms_indeterminate_is_attributable() {
    let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
    let state = session
        .load_from_source(GDT_SOURCE, "gdt_surface")
        .expect("the GD&T probe module must compile on the GUI surface");

    // (a) SURFACE PRECONDITION. The GUI's production geometry path is
    // `tessellate_snapshot`, never `build()`/`realize_for_check`, so nothing
    // populates `realization_handles`. Pinning it here makes this a statement
    // about the SURFACE rather than about an arbitrary engine: a future change
    // that starts realizing fails loudly instead of passing for a new reason.
    assert_eq!(
        session.core_state_for_test().engine().realization_handles_len(),
        0,
        "the GUI surface must not realize — if this fires, the premise of this \
         whole test has changed and the assertions below no longer mean what \
         they say"
    );

    // (b) The verdict half, already true today.
    let callout = state
        .constraints
        .iter()
        .find(|c| c.node_id.contains("Probe#constraint["))
        .expect("the geometric Conforms callout must reach the panel");
    assert_eq!(
        callout.status, "Indeterminate",
        "nothing was realized, so the honest verdict is Indeterminate (never a \
         false Satisfied/Violated); got {callout:?}"
    );

    // (c) THE ATTRIBUTION. Counted, not `find`-ed, so a regression that emitted
    // the attribution twice fails here rather than passing on the first match.
    let attributions = attribution_messages(&session);
    assert_eq!(
        attributions.len(),
        1,
        "exactly one Info + ConstraintIndeterminate attribution is expected for \
         the single geometric Conforms on this surface; got {attributions:?}"
    );
    let message = &attributions[0];
    assert!(
        message.contains(&callout.node_id),
        "the attribution must name the constraint so `labeled_diagnostics` can \
         substitute a user-facing label, exactly as ζ's does; wanted {}, got: {message}",
        callout.node_id
    );
    assert!(
        message.contains("does not measure"),
        "`does not measure` is ζ's stable cross-kind token — the reason must \
         name the SURFACE, not the model; got: {message}"
    );
    assert!(
        message.contains("geometric conformance"),
        "the reason must name THIS kind, so a module carrying several unmeasured \
         kinds does not read as one repeated sentence; got: {message}"
    );

    // (d) The remedy, OCCT-conditionally.
    assert_names_a_terminal_remedy(message);
}
