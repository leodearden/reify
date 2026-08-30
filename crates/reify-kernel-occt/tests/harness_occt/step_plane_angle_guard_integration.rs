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

/// The three counts the refusal diagnostic reports, parsed back out of it.
///
/// The counts are part of the user-visible message on purpose: "which unit is
/// wrong" is only half the diagnosis, and "how many of the file's contexts are
/// still correct" is the half that tells a reader whether they are looking at
/// a whole-file regression or a partial flip.
fn parse_counts(msg: &str) -> (u32, u32, u32) {
    fn field(msg: &str, key: &str) -> u32 {
        let at = msg
            .find(key)
            .unwrap_or_else(|| panic!("refusal must report {key:?}; got: {msg}"));
        let rest = &msg[at + key.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits
            .parse()
            .unwrap_or_else(|_| panic!("{key:?} must be followed by a number; got: {msg}"))
    }
    (
        field(msg, "contexts="),
        field(msg, "plane_angle_units="),
        field(msg, "radian_ok="),
    )
}

/// Assert the refusal blames a specific context by its model entity index.
///
/// A diagnostic that only says "a bad plane-angle unit exists somewhere"
/// cannot be acted on: the file has several contexts and the reader needs to
/// know WHICH one to open.
fn assert_names_a_context_index(msg: &str) {
    let at = msg
        .find("context #")
        .unwrap_or_else(|| panic!("refusal must name the offending context as \"context #N\"; got: {msg}"));
    let rest = &msg[at + "context #".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    assert!(
        !digits.is_empty(),
        "\"context #\" must be followed by the context's entity index; got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Negative path — a wrong plane-angle unit
// ---------------------------------------------------------------------------

/// A non-radian plane-angle declaration is REFUSED, and the diagnostic is
/// actionable.
///
/// The fault flips exactly ONE plane-angle unit to steradian, leaving the
/// other contexts radian. That partial flip is the case a file-wide
/// `content.contains(".RADIAN.")` grep passes — the token is still there,
/// twice — and it is precisely what the per-context association walk exists to
/// catch. Assertion (d) is what pins that: `radian_ok` must be strictly less
/// than `plane_angle_units`, i.e. the guard noticed the difference between
/// "some context declares a radian" and "every context does".
#[test]
fn guard_refuses_a_non_radian_plane_angle_declaration() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) + (b): refused, with Reify attribution rather than OCCT's.
    let msg = refusal_message(&kernel, union_id, "non_radian");

    // (c) The offending unit is NAMED. `.STERADIAN.` is the STEP token for
    // what the fault installs; `sunSteradian` is the OCCT enumerator spelling.
    // Either is actionable; "bad unit" alone is not.
    assert!(
        msg.contains(".STERADIAN.") || msg.contains("sunSteradian"),
        "the refusal must name the offending unit — a reader cannot act on \
         \"some plane-angle unit is wrong\"; got: {msg}"
    );
    assert_names_a_context_index(&msg);

    // (d) The counts are reported, and they show a PARTIAL defect.
    let (contexts, plane_angle_units, radian_ok) = parse_counts(&msg);
    assert!(
        contexts >= 2,
        "the fixture must still carry several contexts for this to be a \
         partial flip; got contexts={contexts} in: {msg}"
    );
    assert!(
        radian_ok < plane_angle_units,
        "exactly one unit was corrupted, so radian_ok must be strictly less \
         than plane_angle_units — equal counts would mean the walk never \
         classified the corrupted unit; got radian_ok={radian_ok} \
         plane_angle_units={plane_angle_units} in: {msg}"
    );
    assert!(
        radian_ok > 0,
        "the OTHER contexts are untouched and must still classify as radian — \
         if radian_ok is 0 the fault corrupted more than the one unit it \
         claims to, and this test is no longer about a partial flip; got: {msg}"
    );
}

/// A PREFIXED radian — a MILLIRADIAN — is REFUSED, proving the guard is not a
/// name-only check.
///
/// This is the case that separates a real unit check from a token grep.
/// `Name() == StepBasic_sunRadian` is TRUE here: the only thing distinguishing
/// a milliradian from a radian is `HasPrefix()`. So a guard written as
/// `si->Name() == StepBasic_sunRadian` accepts this file, and
/// `content.contains(".RADIAN.")` accepts it too, because the emitted
/// `SI_UNIT(.MILLI.,.RADIAN.)` still contains the token. The declared unit
/// would then be 1/1000 of the payload's — silent, and off by exactly the
/// factor the #6186 length regime exists to prevent.
#[test]
fn guard_refuses_a_prefixed_radian_declaration() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, with the same Reify attribution as the non-radian arm.
    let msg = refusal_message(&kernel, union_id, "prefixed");

    // (b) The PREFIX is named. "wrong unit" is not enough here — the reader
    // has to be told the unit is a MILLIradian, or the natural next move is to
    // check the name, find RADIAN, and conclude the guard is broken.
    assert!(
        msg.contains(".MILLI.") || msg.contains("spMilli"),
        "the refusal must name the SI PREFIX that makes this unit wrong; \
         got: {msg}"
    );

    // (c) THE POINT OF THIS TEST. The name is still radian, and the message
    // says so. If this assertion ever fails because the message stopped
    // reporting the name, the diagnostic has lost the one detail that
    // explains why a file full of `.RADIAN.` tokens was refused.
    assert!(
        msg.contains(".RADIAN."),
        "the unit's NAME is still RADIAN — only the prefix is wrong — and the \
         refusal must report that, otherwise a reader who greps the file for \
         .RADIAN. and finds it cannot reconcile the refusal; got: {msg}"
    );

    // The counts still show a partial defect: only one unit was prefixed.
    let (_contexts, plane_angle_units, radian_ok) = parse_counts(&msg);
    assert!(
        radian_ok < plane_angle_units,
        "a prefixed radian must NOT count as radian_ok — that is exactly the \
         name-only check this test exists to reject; got radian_ok={radian_ok} \
         plane_angle_units={plane_angle_units} in: {msg}"
    );
}

/// A context that declares NO plane-angle unit at all is REFUSED — a MISSING
/// declaration, which is a different defect from a wrong one.
///
/// No "is the declared unit right?" check can see this: there is no declared
/// unit to be right or wrong about. Only quantifying over contexts — does
/// EVERY context reach an angular unit? — catches it, and only an association
/// walk can ask that question, because the unit entity itself is still sitting
/// in the model, still spelled `SI_UNIT($,.RADIAN.)`, merely unreferenced.
#[test]
fn guard_refuses_a_context_with_no_plane_angle_declaration() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, same attribution arms.
    let msg = refusal_message(&kernel, union_id, "missing");

    // (b) The offending context is named by entity index, and the units it DID
    // reach are listed. When a context declares nothing, the diagnostic
    // question is "then what did it reach?" — and the answer is what tells a
    // reader whether the reference list was truncated or replaced.
    assert_names_a_context_index(&msg);
    assert!(
        msg.contains("SiUnit") || msg.contains("NamedUnit") || msg.contains("Unit"),
        "the refusal must list the units the context DID reach — an empty \
         answer to \"then what did it reach?\" is useless; got: {msg}"
    );

    // (c) MISSING and WRONG are worded differently. They have different causes
    // and different fixes, so collapsing them into one "bad plane angle unit"
    // string would be a regression in the guard's only user-visible output.
    assert!(
        !msg.contains("is not the unprefixed SI radian"),
        "a MISSING declaration must not be reported with the WRONG-unit \
         phrasing steps 3 and 5 pinned — nothing here is a non-radian unit, \
         and a reader sent looking for one will not find it; got: {msg}"
    );

    // (d) Again a PARTIAL defect: the surviving contexts still reach radians,
    // so the file still contains `.RADIAN.` and a grep still passes.
    let (contexts, _plane_angle_units, radian_ok) = parse_counts(&msg);
    assert!(
        radian_ok < contexts,
        "one context lost its declaration, so the radian associations must no \
         longer cover every context — equal counts would mean the walk never \
         noticed the missing reference; got radian_ok={radian_ok} \
         contexts={contexts} in: {msg}"
    );
    assert!(
        radian_ok > 0,
        "the OTHER contexts are untouched and must still reach radians; if \
         radian_ok is 0 the fault did more than remove one reference; got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// The half-wired `step.angleunit.mode` trap — a SEPARATE arm
// ---------------------------------------------------------------------------

/// The degree regime of `step.angleunit.mode` is REFUSED, and the fault does
/// not leak out of the export.
///
/// THE DECLARATION WALK CANNOT CATCH THIS, and the guard does not pretend it
/// can. `step.angleunit.mode` is a registered `Interface_Static` whose only
/// write-side consumer is `TopoDSToStep_MakeStepFace::Init` ->
/// `GeomConvert_Units::RadianToDegree`, which rescales PCURVE PARAMETER space.
/// The unit declaration ignores it entirely: the #6184 measurement recorded in
/// `cpp/occt_wrapper.cpp` exported one cone under all three enum values and
/// found `#84 = ( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )`
/// byte-identical in every one, with the only difference a pcurve
/// `CARTESIAN_POINT` moving from `(-6.28318530718,0.)` to `(-360.,0.)`. The
/// payload moves; the declaration does not. So the four declaration arms above
/// provably cannot see this, and it needs its own check of the static —
/// which is also a far more actionable diagnostic than any unit walk could be.
///
/// Setting the static to Deg produces degree pcurves under a radian header:
/// a silently self-inconsistent file, NOT a degrees file.
#[test]
fn guard_refuses_the_half_wired_degree_angle_mode() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, same attribution arms as every other refusal.
    let msg = refusal_message(&kernel, union_id, "angle_mode_deg");

    // (b) The diagnostic names the static VERBATIM and explains the mechanism.
    // "some angle setting is wrong" would send the reader looking through the
    // unit declarations, where — by construction — they will find nothing.
    assert!(
        msg.contains("step.angleunit.mode"),
        "the refusal must name the `step.angleunit.mode` static verbatim — it \
         is the one string a reader can grep for; got: {msg}"
    );
    assert!(
        msg.contains("pcurve"),
        "the refusal must say WHAT the degree regime moves: pcurve parameter \
         space, not the declaration; got: {msg}"
    );
    assert!(
        msg.contains("RadianToDegree"),
        "the refusal must name the consumer that does the rescaling \
         (GeomConvert_Units::RadianToDegree), so the reader can confirm the \
         mechanism rather than take it on faith; got: {msg}"
    );
    assert!(
        msg.contains("declaration"),
        "the refusal must say the DECLARATION stays at radians — that is what \
         makes the file self-inconsistent rather than simply a degrees file; \
         got: {msg}"
    );

    // (c) NO LEAK. `step.angleunit.mode` is a process-global Interface_Static
    // and this harness runs its tests as threads in ONE process, so a fault
    // that failed to restore it would make unrelated sibling tests' exports
    // refuse. This also pins restoration on the THROWING path, which is the
    // only path this fault ever takes.
    kernel
        .export_step_with_injected_fault_for_test(union_id, "AP214", "none")
        .expect(
            "the injected `step.angleunit.mode` value must be restored even \
             though the export threw — it is a process-global Interface_Static \
             shared with every other test in this harness binary",
        );
}
