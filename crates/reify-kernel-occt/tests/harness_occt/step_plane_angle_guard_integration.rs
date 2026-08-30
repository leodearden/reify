//! Integration tests for the STEP export PLANE-ANGLE refusal guard (#6344).
//!
//! INV-AD-4's third arm. `export_step` checks, on the transferred
//! `Interface_InterfaceModel` and BEFORE any bytes reach a file, that every
//! representation context declares the *unprefixed SI radian* for plane
//! angles. A violation is REFUSED (`ContractViolation` → `ExportError::
//! FormatError`), not warned about: a mislabelled angular unit is a
//! correctness defect in the emitted bytes, and a warning on stderr does not
//! stop the wrong file from reaching an external CAD tool.
//!
//! WHY THESE TESTS NEED FAULT INJECTION. `STEPConstruct_UnitContext::Init` —
//! the sole builder of the write-side unit context — emits
//! `SI_UNIT($,.RADIAN.)` as an immediate constant with no branch on any writer
//! option (measured for #6184; see the PLANE-ANGLE UNIT REGIME comment in
//! `cpp/occt_wrapper.cpp`). So NO input shape and NO `Interface_Static` can
//! make a real export produce a non-radian declaration, and the guard's
//! failure arms are unreachable from ordinary inputs. Without injection the
//! guard would be decorative. `export_step_with_injected_fault_for_test`
//! therefore runs the SAME `export_step_locked` body under the SAME mutex,
//! corrupting exactly one thing first — the established `*_for_test`
//! fixture-hook pattern this crate already uses for `make_null_shape_for_test`
//! ("the exact crash input … it cannot be built from Rust because `OcctShape`
//! is opaque"; the same argument applies to a STEP model with a corrupted unit
//! context).
//!
//! FIXTURE. Two disjoint 30 mm cones, unioned — the same fixture the #6184
//! text-level pin uses (`src/handle.rs`), because it is already MEASURED to
//! emit THREE `GLOBAL_UNIT_ASSIGNED_CONTEXT` entities. That multiplicity is
//! what makes the per-context arms non-vacuous and what makes a PARTIAL flip
//! (one context corrupted, the others still radian) expressible — the case a
//! file-wide `contains(".RADIAN.")` grep passes and an association walk must
//! fail.

#![cfg(all(has_occt, feature = "test-fixtures"))]

use reify_ir::{ExportError, GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Two disjoint 30 mm cones, unioned — a compound, which is what makes OCCT
/// emit more than one representation context.
///
/// Built through `OcctKernel` directly rather than through `OcctKernelHandle`
/// (the style `conformance_integration.rs` uses) because the `*_for_test`
/// helpers hang off `OcctKernel`, not off the actor handle.
///
/// Dimensions mirror the #6184 BRep pin exactly (`src/handle.rs`): SI metres,
/// 15 mm base radius, 30 mm height, second cone translated 100 mm in +x.
fn two_cone_union_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let cone = GeometryOp::Cone {
        bottom_radius: Value::Real(0.015),
        top_radius: Value::Real(0.0),
        height: Value::Real(0.030),
    };
    let left = kernel.execute(&cone).expect("left cone should build");
    let right_untranslated = kernel.execute(&cone).expect("right cone should build");
    let right = kernel
        .execute(&GeometryOp::Translate {
            target: right_untranslated.id,
            dx: 0.100,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate should succeed");
    let union = kernel
        .execute(&GeometryOp::Union {
            left: left.id,
            right: right.id,
        })
        .expect("union should succeed");
    (kernel, union.id)
}

// ---------------------------------------------------------------------------
// Positive path — the guard must not refuse a legitimate export
// ---------------------------------------------------------------------------

/// A real multi-context export is ACCEPTED, and the guard SEES every context.
///
/// The load-bearing assertion is (c): the entity OCCT actually emits is the
/// COMPLEX `StepGeom_GeomRepContextAndGlobUnitAssCtxAndGlobUncertaintyAssCtx`,
/// so a naive direct `DownCast<StepRepr_GlobalUnitAssignedContext>` reports
/// ZERO contexts on a file that demonstrably carries three. Cross-checking the
/// model-level count against the text-level `GLOBAL_UNIT_ASSIGNED_CONTEXT`
/// count from the SAME export is what reddens that naive implementation —
/// without it, a guard that sees nothing would pass every other arm vacuously.
#[test]
fn guard_accepts_a_real_multi_context_export() {
    let (kernel, union_id) = two_cone_union_kernel();

    let audit = kernel
        .export_step_with_injected_fault_for_test(union_id, "AP214", "none")
        .expect("a legitimate export must be ACCEPTED — a guard that refuses a \
                 correct file is worse than no guard at all");

    // (b) The compound really does carry more than one representation
    // context, so the per-context arms below are not vacuously satisfied.
    assert!(
        audit.contexts >= 2,
        "the two-cone union must emit more than one unit-assigned context, \
         otherwise every per-context arm of the guard is vacuous; got \
         contexts={}",
        audit.contexts
    );

    // (c) THE ANTI-NAIVE-DOWNCAST ARM. Count the text-level entities in the
    // very bytes this export produced and require the model walk to have seen
    // exactly as many.
    let stripped: String = audit
        .content
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let text_contexts = stripped.matches("GLOBAL_UNIT_ASSIGNED_CONTEXT").count();
    assert_eq!(
        audit.contexts as usize, text_contexts,
        "the guard must resolve EVERY unit-assigned context the file declares. \
         A direct DownCast<StepRepr_GlobalUnitAssignedContext> finds none of \
         them — the emitted entity is the COMPLEX \
         GeomRepContextAndGlobUnitAssCtxAndGlobUncertaintyAssCtx, which must be \
         unwrapped via GlobalUnitAssignedContext(). Model walk saw {}, file \
         declares {}",
        audit.contexts, text_contexts
    );

    // (d) Every context reached at least one angular unit, and every angular
    // unit the walk classified is the accepted radian.
    assert!(
        audit.plane_angle_units >= audit.contexts,
        "each context must reach at least one plane-angle unit; got \
         plane_angle_units={} for contexts={}",
        audit.plane_angle_units,
        audit.contexts
    );
    assert_eq!(
        audit.radian_ok, audit.plane_angle_units,
        "on an uncorrupted export EVERY plane-angle unit must classify as the \
         unprefixed SI radian; got radian_ok={} of plane_angle_units={}",
        audit.radian_ok, audit.plane_angle_units
    );

    // (e) The declaration is spelled the way OCCT actually spells it. `$` is
    // the NULL SI PREFIX; the `*` in the sibling `NAMED_UNIT(*)` is the
    // redeclared marker, not a prefix, so a pin written for
    // `SI_UNIT(*,.RADIAN.)` fails immediately (see the #6184 note in
    // `src/handle.rs`).
    assert!(
        stripped.contains("SI_UNIT($,.RADIAN.)"),
        "the accepted export must declare the unprefixed SI radian as \
         SI_UNIT($,.RADIAN.) — `$` is the null SI prefix"
    );
}

/// Helper: assert a refusal carries Reify attribution, not OCCT's.
///
/// `wrap_occt_call` renders a Reify-detected `ContractViolation` as
/// `"<op>: <message>"` and reserves `"OCCT <op>: unexpected: …"` for genuine
/// OCCT-originated or unforeseen failures. Asserting all three properties —
/// the `"export_step: "` prefix, the absence of a leading `"OCCT "`, and the
/// absence of `"unexpected"` — is what proves the refusal is the guard's own
/// deliberate diagnostic rather than an OCCT crash that happened to fire. This
/// is the idiom `loft_guided_integration.rs` uses.
#[allow(dead_code)]
fn assert_reify_authored_refusal(msg: &str) {
    assert!(
        msg.starts_with("export_step: "),
        "a Reify contract violation must surface as \"export_step: \" + \
         message; got: {msg}"
    );
    assert!(
        !msg.starts_with("OCCT "),
        "the refusal must not be attributed to OCCT — it is Reify's own \
         postcondition check; got: {msg}"
    );
    assert!(
        !msg.contains("unexpected"),
        "\"unexpected\" framing is reserved for unforeseen exceptions; a \
         deliberate guard refusal must not use it; got: {msg}"
    );
}

/// Helper: run one fault and require it to be REFUSED, returning the message.
#[allow(dead_code)]
fn refusal_message(kernel: &OcctKernel, id: GeometryHandleId, fault: &str) -> String {
    match kernel.export_step_with_injected_fault_for_test(id, "AP214", fault) {
        Err(ExportError::FormatError(msg)) => {
            assert_reify_authored_refusal(&msg);
            msg
        }
        Err(other) => panic!(
            "fault {fault:?} must be refused as ExportError::FormatError; got \
             Err({other:?})"
        ),
        Ok(_) => panic!(
            "fault {fault:?} must be REFUSED — the guard returned a STEP file \
             for a model whose plane-angle declaration is corrupt"
        ),
    }
}
