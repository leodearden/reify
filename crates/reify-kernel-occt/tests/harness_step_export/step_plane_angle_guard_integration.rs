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
//! WHY THESE TESTS NEED FAULT INJECTION. The guard's failure arms are
//! unreachable from ordinary inputs; the argument is on the `StepGuardFault`
//! enum in `src/ffi.rs` and is not restated here.
//! `export_step_with_injected_fault_for_test` runs the SAME
//! `export_step_locked` body under the SAME mutex, corrupting exactly one
//! thing first. Which thing is a `StepGuardFault`, a shared cxx enum, so a
//! misspelled fault is a compile error here rather than a runtime rejection.
//!
//! WHAT THE INJECTED PATH CANNOT PIN, and what covers it. Every test below
//! enters through a `*_for_test` hook, so the production entry point's own
//! `StepGuardDisposition::Refuse` argument is not exercised by any of them —
//! and it is not observable from outside either, because the two dispositions
//! differ only on a model the guard refuses, which no production input
//! produces. `the_production_export_path_runs_the_guard` covers the half that
//! IS observable (the real `OcctKernel::export` reaches the guard and is
//! accepted by it); the half that is not is closed structurally instead, by
//! `export_step` re-throwing any non-empty refusal whatever disposition it
//! asked for.
//!
//! ASSERTING ON A REFUSAL. Every violation line is prefixed with an
//! identifier-shaped `[INV-AD-4/Vn]` arm tag (`V1`..`V5` for the declaration
//! walk, `MODE` for the separate `step.angleunit.mode` arm), and the tests pin
//! THOSE rather than the English around them. The distinction between arms —
//! a MISSING declaration is a different defect, with a different fix, from a
//! WRONG one — is the guard's most valuable output, and pinning it via a
//! negative assertion on prose fails OPEN: reword the sentence and the
//! assertion becomes trivially true while silently ceasing to check anything.
//!
//! `UNVERIFIABLE` is a QUALIFIER carried in the same bracketed form, appended
//! to whichever arm fired. It separates "the guard read the declaration and it
//! is wrong" from "the guard could not read the declaration at all" — a fork
//! the contract draws repeatedly, spanning both V3 (a referenced unit) and V4
//! (an orphan), whose two message branches word it differently. Asserting the
//! qualifier in both directions is what catches the branches being collapsed
//! into one wording; the English phrasings (`cannot verify` / `cannot be
//! verified`) are deliberately no longer asserted.
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
use reify_kernel_occt::{OcctKernel, StepGuardFault, StepGuardProbeResult};

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
        .export_step_with_injected_fault_for_test(union_id, "AP214", StepGuardFault::None)
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

    // (d2) No orphans on a correct file: every angular unit entity the model
    // carries is reachable from some context. This is what makes the V4 arm's
    // own negative test (`guard_refuses_an_orphaned_non_radian_plane_angle_unit`)
    // meaningful — if a real export already shipped orphaned angular units,
    // "this unit is unreferenced" would not be a signal at all.
    assert_eq!(
        audit.orphan_angular_units, 0,
        "a legitimate export must carry NO orphaned angular unit — every one \
         the model declares must be reachable from a context; got \
         orphan_angular_units={}",
        audit.orphan_angular_units
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
fn refusal_message(
    kernel: &OcctKernel,
    id: GeometryHandleId,
    fault: StepGuardFault,
) -> String {
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

/// Run one fault through BOTH dispositions and return the production
/// diagnostic next to the guard's structured audit of that same corruption.
///
/// WHY BOTH, AND WHY NEITHER ALONE. The refusing hook is where the production
/// behaviour lives: a violation must surface as `ExportError::FormatError`
/// carrying Reify's own attribution, and must yield no file. The reporting
/// probe is what makes the audit available as `u32` fields. Before it, every
/// negative test re-extracted those four numbers from the English header with
/// a hand-rolled digit scanner — the meaningful-strings-instead-of-structured-
/// data shape, and load-bearing rather than merely ugly: the scanner keyed on
/// the FIRST `"contexts="` anywhere in the message, so prepending any line that
/// mentioned a count silently changed what every assertion read.
///
/// THE EQUALITY ASSERTION IS THE SEAM. Both dispositions render from one
/// `step_export_guard_refusal` call, so the probe's `refusal` must be the
/// thrown diagnostic minus its op prefix. Pinning that is what stops the
/// reported text — and therefore the counts asserted against it — from drifting
/// away from what a user actually sees.
fn refusal(
    kernel: &OcctKernel,
    id: GeometryHandleId,
    fault: StepGuardFault,
) -> (String, StepGuardProbeResult) {
    let msg = refusal_message(kernel, id, fault);
    let probe = kernel
        .step_guard_probe_for_test(id, "AP214", fault)
        .unwrap_or_else(|e| {
            panic!(
                "the reporting probe must reach the same finding the refusing \
                 hook did for fault {fault:?} — an error here means the two \
                 dispositions no longer run the same body; got {e:?}"
            )
        });
    assert_eq!(
        msg,
        format!("export_step: {}", probe.refusal),
        "the probe must REPORT byte-for-byte what the production path THROWS, \
         or the counts read off the probe describe a different run from the \
         message asserted beside them"
    );
    assert!(
        probe.content.is_empty(),
        "a refused export must produce NO bytes — reporting a refusal instead \
         of throwing it must not weaken it into a warning; got {} bytes",
        probe.content.len()
    );
    (msg, probe)
}

/// Assert exactly which arms and qualifiers the guard emitted.
///
/// PIN THE TAG, NOT THE PROSE. Each violation line is prefixed with an
/// identifier-shaped `[INV-AD-4/Vn]` marker precisely so these assertions
/// survive any rewording of the sentence that follows it. The earlier form of
/// this check — a NEGATIVE assertion that the MISSING message did not contain
/// the WRONG message's English phrasing — failed OPEN: reword the WRONG
/// message and the assertion becomes trivially true, silently ceasing to
/// distinguish the two cases it exists to separate. A missing tag reds the
/// positive assertion first, which is the correct failure direction.
///
/// Takes arms (`V1`..`V5`, `MODE`) and the `UNVERIFIABLE` qualifier
/// interchangeably: both are spelled `[INV-AD-4/<tag>]`, and the qualifier is
/// as much a behavioural claim as the arm is.
fn assert_arms(msg: &str, expected: &[&str], forbidden: &[&str]) {
    for arm in expected {
        let tag = format!("[INV-AD-4/{arm}]");
        assert!(
            msg.contains(&tag),
            "the refusal must be attributed to arm {tag} — the tag is the \
             machine-readable half of the diagnostic and the only part of it \
             that survives a rewording; got: {msg}"
        );
    }
    for arm in forbidden {
        let tag = format!("[INV-AD-4/{arm}]");
        assert!(
            !msg.contains(&tag),
            "arm {tag} must NOT fire here: the arms describe defects with \
             different causes and different fixes, and collapsing them would \
             send a reader looking for a defect that is not there; got: {msg}"
        );
    }
}

/// Assert the refusal blames a specific context by its model entity index.
///
/// A diagnostic that only says "a bad plane-angle unit exists somewhere"
/// cannot be acted on: the file has several contexts and the reader needs to
/// know WHICH one to open.
fn assert_names_a_context_index(msg: &str) {
    positive_number_after(msg, "context #");
}

/// Pull the number a message reports after `marker`, requiring it to be > 0.
///
/// The counts moved to `StepGuardProbeResult`, but WHICH entity a violation
/// blames — and how many entities V1 walked — is not among them: it is the
/// located half of the diagnostic, and it exists only in the text a user
/// reads. So this one scan stays, shared by every arm that reports a number
/// rather than reimplemented per test.
///
/// The `> 0` requirement is load-bearing in both readings.
/// `Interface_InterfaceModel::Number` returns 0 for an entity the model does
/// not carry, so "plane-angle unit #0" sends a reader looking for something
/// that is not in the file; and a walked count of 0 means V1 fired on its
/// null-model branch, which says nothing about a real export.
fn positive_number_after(msg: &str, marker: &str) -> u32 {
    let at = msg
        .find(marker)
        .unwrap_or_else(|| panic!("refusal must report {marker:?} + a number; got: {msg}"));
    let digits: String = msg[at + marker.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    assert!(
        !digits.is_empty(),
        "{marker:?} must be followed by a number; got: {msg}"
    );
    let value: u32 = digits.parse().expect("digits parse");
    assert!(
        value > 0,
        "the number after {marker:?} must be > 0 — an entity index of 0 names \
         nothing in the file (Interface_InterfaceModel::Number returns 0 for \
         an entity the model does not carry), and a walked count of 0 means \
         the null-model branch fired instead; got: {msg}"
    );
    value
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

    // (a) + (b): refused, with Reify attribution rather than OCCT's, and the
    // same finding read back structurally.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::NonRadian);

    // (c) The offending unit is NAMED. `.STERADIAN.` is the STEP token for
    // what the fault installs; `sunSteradian` is the OCCT enumerator spelling.
    // Either is actionable; "bad unit" alone is not.
    assert!(
        msg.contains(".STERADIAN.") || msg.contains("sunSteradian"),
        "the refusal must name the offending unit — a reader cannot act on \
         \"some plane-angle unit is wrong\"; got: {msg}"
    );
    assert_names_a_context_index(&msg);

    // (c2) A REFERENCED wrong unit is arm V3, not the unreferenced-unit arm
    // V4 and not the missing-declaration arm V2. UNVERIFIABLE must stay
    // silent too: the guard READ this declaration and found it wrong, which
    // is a stronger and differently-actionable claim than "could not read it".
    assert_arms(&msg, &["V3"], &["V1", "V2", "V4", "UNVERIFIABLE"]);

    // (d) THE COUNTS ARE IN THE USER-VISIBLE HEADER. Asserted here and nowhere
    // else: the numbers ARE part of the message on purpose ("how many of the
    // file's contexts are still correct" is what tells a reader whether they
    // are looking at a whole-file regression or a partial flip), so one test
    // pins that they are rendered, and the rest read them off the struct. The
    // expected text is BUILT from the struct rather than scanned out of the
    // message, so this is a containment check and not a second parser.
    assert!(
        msg.contains(&format!(
            "contexts={} plane_angle_units={} radian_ok={} orphan_angular_units={}",
            probe.contexts, probe.plane_angle_units, probe.radian_ok,
            probe.orphan_angular_units
        )),
        "the refusal header must report the audit counts the guard computed, \
         in that order — they are the half of the diagnosis that says how much \
         of the file is still correct; got: {msg}"
    );

    // (e) And they show a PARTIAL defect.
    let StepGuardProbeResult {
        contexts,
        plane_angle_units,
        radian_ok,
        ..
    } = probe;
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
    // `radian_ok > 0` is deliberately NOT asserted beside it. It reads as "the
    // other contexts are untouched", but it holds only while OCCT emits a
    // distinct unit entity per context: dedup those entities upstream and one
    // mutation legitimately flips every association, reddening this test on a
    // perfectly correct guard. That is the same OCCT detail
    // `audit_step_plane_angle_units` refuses to depend on (an association walk,
    // not a count proxy), and the assertion above needs no such assumption.
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
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::Prefixed);

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

    // (d) Same arm as the non-radian flip: the unit is still REFERENCED, it is
    // simply wrong, so this is V3 and nothing else — and it was READ, so not
    // UNVERIFIABLE either.
    assert_arms(&msg, &["V3"], &["V1", "V2", "V4", "UNVERIFIABLE"]);

    // The counts still show a partial defect: only one unit was prefixed.
    let StepGuardProbeResult {
        plane_angle_units,
        radian_ok,
        ..
    } = probe;
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
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::Missing);

    // (b) The offending context is named by entity index, and the units it DID
    // reach are listed. When a context declares nothing, the diagnostic
    // question is "then what did it reach?" — and the answer is what tells a
    // reader whether the reference list was truncated or replaced.
    assert_names_a_context_index(&msg);
    // The listed types are OCCT class names — identifier-shaped, so this
    // survives any rewording of the sentence around them. (A bare `"Unit"`
    // disjunct was dropped: it is a substring of both of these and of most
    // English the message could ever carry, so it asserted nothing.)
    assert!(
        msg.contains("SiUnit") || msg.contains("NamedUnit"),
        "the refusal must list the units the context DID reach, by OCCT class \
         name — an empty answer to \"then what did it reach?\" is useless; \
         got: {msg}"
    );

    // (c) MISSING is arm V2, and it is NOT reported as a wrong unit. These are
    // different defects with different causes and different fixes, and the arm
    // tag is what pins the distinction: a negative assertion on the WRONG
    // arm's English phrasing would fail open the moment that phrasing changed.
    //
    // V4 must also stay silent. The stripped unit ENTITY is still a perfectly
    // good unprefixed radian — it is merely unreferenced now — and V4 refuses
    // only orphans that are NOT the radian. A V4 hit here would mean the guard
    // had started treating "unreferenced" as a defect in itself.
    assert_arms(&msg, &["V2"], &["V1", "V3", "V4", "UNVERIFIABLE"]);

    // (d) Again a PARTIAL defect: the surviving contexts still reach radians,
    // so the file still contains `.RADIAN.` and a grep still passes.
    let StepGuardProbeResult {
        contexts,
        radian_ok,
        ..
    } = probe;
    assert!(
        radian_ok < contexts,
        "one context lost its declaration, so the radian associations must no \
         longer cover every context — equal counts would mean the walk never \
         noticed the missing reference; got radian_ok={radian_ok} \
         contexts={contexts} in: {msg}"
    );
    // No `radian_ok > 0` here either, for the reason spelled out in
    // `guard_refuses_a_non_radian_plane_angle_declaration`: it would pin OCCT's
    // per-context unit-entity layout, which this suite must not depend on.
}

/// A CONVERSION_BASED plane-angle declaration — the spelling a DEGREE unit
/// takes — is REFUSED.
///
/// THE ARM CLOSEST TO THE REAL DEFECT. Every other wrong-unit case this suite
/// injects is a corruption no writer would plausibly produce; a degrees file
/// under a radian payload is the concrete outcome INV-AD-4 exists to prevent,
/// and `StepBasic_ConversionBasedUnitAndPlaneAngleUnit` is exactly how STEP
/// spells it. It is also the classification with the most silent failure mode:
/// mistype the downcast in `classify_step_angle_unit` (to
/// `…ConversionBasedUnitAndSolidAngleUnit`, say) and a real degree unit demotes
/// to `UnrecognisedAngular` or, worse, to `NotAngular` — where the containing
/// context then looks like it declares nothing and V2 blames a MISSING
/// declaration that is sitting right there in the file.
///
/// The fault replaces the unit IN PLACE, so the context still reaches as many
/// units as before and V2 stays silent — this is a test of the classifier, not
/// a second test of the missing-declaration arm.
#[test]
fn guard_refuses_a_conversion_based_degree_declaration() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, with the same Reify attribution as every other arm.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::ConversionBased);

    // (b) V3 — the unit is REFERENCED, so this is the association arm. And
    // NOT UNVERIFIABLE: the guard recognised this form and rejected it, which
    // is a stronger claim than "could not read it" and the one that tells a
    // reader the file really is declaring degrees.
    assert_arms(&msg, &["V3"], &["V1", "V2", "V4", "UNVERIFIABLE"]);

    // (c) The refusal names the FORM that makes the unit wrong. A degree unit
    // carries no SI name and no SI prefix, so neither of the two details the
    // other V3 tests pin is available here — `CONVERSION_BASED_UNIT` is the
    // Part-21 keyword a reader greps the emitted file for.
    assert!(
        msg.contains("CONVERSION_BASED_UNIT"),
        "the refusal must name the unit's FORM — CONVERSION_BASED_UNIT is the \
         Part-21 keyword a degree or grad declaration takes, and it is the one \
         token that tells a reader what they are looking at; got: {msg}"
    );
    assert_names_a_context_index(&msg);
    positive_number_after(&msg, "reaches plane-angle unit #");

    // (d) The walk COUNTED the substitute as angular but not as a radian. If
    // the ConversionBased downcast were mistyped, the unit would fall through
    // to a later branch: to `UnrecognisedAngular` (which would red the
    // UNVERIFIABLE assertion above) or to `NotAngular`, which would drop it out
    // of `plane_angle_units` entirely and make these two equal again.
    let StepGuardProbeResult {
        plane_angle_units,
        radian_ok,
        ..
    } = probe;
    assert!(
        radian_ok < plane_angle_units,
        "the conversion-based unit must be COUNTED as a plane-angle unit and \
         must not count as radian_ok — equal counts mean the classifier \
         dropped it as non-angular, which is what a mistyped downcast looks \
         like; got radian_ok={radian_ok} \
         plane_angle_units={plane_angle_units} in: {msg}"
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
/// `GeomConvert_Units::RadianToDegree`, which rescales PCURVE PARAMETER space;
/// the unit declaration ignores it entirely. The payload moves; the
/// declaration does not. So the five declaration arms above provably cannot
/// see this, and it needs its own check of the static — which is also a far
/// more actionable diagnostic than any unit walk could be. The dated
/// three-mode measurement behind that claim lives in exactly one place, the
/// OBSERVATION LOG in `export_step_locked` (`cpp/occt_wrapper.cpp`); it is not
/// restated here, because a copy with no date beside it cannot be judged
/// stale after an OCCT bump.
///
/// Setting the static to Deg produces degree pcurves under a radian header:
/// a silently self-inconsistent file, NOT a degrees file.
#[test]
fn guard_refuses_the_half_wired_degree_angle_mode() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, same attribution arms as every other refusal.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::AngleModeDeg);

    // (b) The diagnostic names the static VERBATIM and explains the mechanism.
    // "some angle setting is wrong" would send the reader looking through the
    // unit declarations, where — by construction — they will find nothing.
    assert!(
        msg.contains("step.angleunit.mode"),
        "the refusal must name the `step.angleunit.mode` static verbatim — it \
         is the one string a reader can grep for; got: {msg}"
    );
    assert!(
        msg.contains("TopoDSToStep_MakeStepFace"),
        "the refusal must name WHERE the degree regime is consumed — it is a \
         face-writer concern, not a unit-declaration one; got: {msg}"
    );
    assert!(
        msg.contains("RadianToDegree"),
        "the refusal must name the consumer that does the rescaling \
         (GeomConvert_Units::RadianToDegree), so the reader can confirm the \
         mechanism rather than take it on faith; got: {msg}"
    );
    // The DECLARATION stays at radians — that is what makes the file
    // self-inconsistent rather than simply a degrees file. Pinned as the
    // literal Part-21 spelling rather than the English word "declaration":
    // this token is the thing a reader greps the emitted file for, and it
    // cannot be reworded out of the message the way an ordinary word can.
    assert!(
        msg.contains("SI_UNIT($,.RADIAN.)"),
        "the refusal must show that the emitted declaration is STILL the \
         unprefixed SI radian, which is precisely why degree pcurves make the \
         file self-inconsistent; got: {msg}"
    );

    // (b2) This is the MODE arm. The five declaration arms provably cannot see
    // this defect (the declaration is byte-identical under every enum value),
    // and a diagnostic claiming one of them fired would be claiming something
    // the guard's own evidence log contradicts.
    assert_arms(&msg, &["MODE"], &["V1", "V2", "V3", "V4", "UNVERIFIABLE"]);

    // (b3) The declaration walk did not even RUN. The mode arm is ordered
    // ahead of it and short-circuits, which is what keeps these counts honest:
    // all-zero says "not walked", not "walked and found nothing". A reader who
    // saw `contexts=3 radian_ok=3` beside a MODE refusal would reasonably
    // conclude the walk had cleared the file, which is a claim nobody made.
    assert_eq!(
        (
            probe.contexts,
            probe.plane_angle_units,
            probe.radian_ok,
            probe.orphan_angular_units
        ),
        (0, 0, 0, 0),
        "the MODE arm must short-circuit the declaration walk; got \
         contexts={} plane_angle_units={} radian_ok={} orphan_angular_units={}",
        probe.contexts,
        probe.plane_angle_units,
        probe.radian_ok,
        probe.orphan_angular_units
    );

    // (c) NO LEAK. `step.angleunit.mode` is a process-global Interface_Static
    // and this harness runs its tests as threads in ONE process, so a fault
    // that failed to restore it would make unrelated sibling tests' exports
    // refuse. This also pins restoration on the THROWING path, which is the
    // only path this fault ever takes.
    kernel
        .export_step_with_injected_fault_for_test(union_id, "AP214", StepGuardFault::None)
        .expect(
            "the injected `step.angleunit.mode` value must be restored even \
             though the export threw — it is a process-global Interface_Static \
             shared with every other test in this harness binary",
        );
}

// ---------------------------------------------------------------------------
// The V4 arm — an angular unit entity NO context references
// ---------------------------------------------------------------------------

/// An ORPHANED non-radian plane-angle unit is REFUSED.
///
/// V4 is the only arm that quantifies over unit ENTITIES rather than over
/// (context, unit) associations, and it needs its own fault because no other
/// one can reach it. `Missing` orphans a unit too — but a still-correct
/// unprefixed radian, which V4 skips by design; `NonRadian` produces a wrong
/// unit that is still REFERENCED, so V3 claims it first. Only dropping every
/// reference AND making the unit wrong lands here.
///
/// WHY THE ARM EXISTS AT ALL. The entity is in the emitted bytes, spelled as a
/// plane-angle unit, while being reachable from no context — so a consumer
/// that resolves units differently than this walk does (or a human reading the
/// file) sees a declaration this file's own contexts do not. Refusing is the
/// conservative posture: reify emits exactly one plane-angle regime, and an
/// unreachable second one in the same file is not something to ship.
#[test]
fn guard_refuses_an_orphaned_non_radian_plane_angle_unit() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, with the same Reify attribution as every other arm.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::OrphanNonRadian);

    // (b) V4 fired, and it is the arm that names the finding. V3 must NOT:
    // no context reaches this unit any more, so blaming a context would point
    // the reader at a file location where the defect is not.
    //
    // V2 is deliberately NOT forbidden. Whether the emitted contexts share one
    // plane-angle unit entity or each hold their own is an OCCT implementation
    // detail; when they hold their own, the context that lost its reference
    // legitimately reaches no angular unit any more and V2 is a true finding.
    // Forbidding it would pin an OCCT detail this test has no business
    // depending on.
    assert_arms(&msg, &["V4"], &["V1", "V3", "UNVERIFIABLE"]);

    // (c) The offending ENTITY is named by index, and by unit name. This is
    // V4's own message-formatting branch — distinct from V3's, which names a
    // context as well — so it needs its own pin.
    positive_number_after(&msg, "unreferenced plane-angle unit #");
    assert!(
        msg.contains(".STERADIAN.") || msg.contains("sunSteradian"),
        "the refusal must name what the orphan actually is; got: {msg}"
    );

    // (d) THE COUNTS ARE NOT SELF-CONTRADICTING — the whole reason
    // `orphan_angular_units` is in the header. Why the other three cannot
    // carry this: `StepPlaneAngleAuditCounts::orphan_angular_units`.
    assert!(
        probe.orphan_angular_units > 0,
        "the refusal must report the orphan the violation line blames, \
         otherwise the counts describe a healthy file directly above a line \
         saying it is not; got orphan_angular_units={} in: {msg}",
        probe.orphan_angular_units
    );
    assert_eq!(
        probe.radian_ok, probe.plane_angle_units,
        "the surviving ASSOCIATIONS are untouched by this fault — every \
         context still reaches a radian. If these diverge the fault corrupted \
         a referenced unit too, and this test is no longer about an orphan; \
         got radian_ok={} plane_angle_units={} in: {msg}",
        probe.radian_ok, probe.plane_angle_units
    );
}

// ---------------------------------------------------------------------------
// The UNVERIFIABLE-form arms — a plane-angle unit this guard cannot read
// ---------------------------------------------------------------------------

/// A REFERENCED plane-angle unit in a form the guard cannot inspect is
/// REFUSED — and reported as unverifiable, not as verified-wrong.
///
/// Part 21 permits a bare `NAMED_UNIT`/`PLANE_ANGLE_UNIT` pair, which is
/// neither of the two `…And…` composites OCCT actually emits. That form is the
/// reason `classify_step_angle_unit` performs a THIRD downcast to
/// `StepBasic_PlaneAngleUnit` after the two composites: without it the unit
/// classifies as NotAngular, its context then reaches zero recognised angular
/// units, and the guard refuses with V2's "reaches NO plane-angle unit" — which
/// is FALSE. A declaration was made; the guard just could not read it. Sending
/// a reader to look for a missing declaration that is sitting right there is a
/// worse outcome than the refusal itself.
///
/// The fault REPLACES the unit in place rather than dropping it, so the
/// context still reaches exactly as many units as before. That is what keeps
/// this a test of the unverifiable-form branch and not an accidental second
/// test of V2.
#[test]
fn guard_refuses_an_unverifiable_plane_angle_declaration() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, with the same Reify attribution as every other arm.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::UnrecognisedAngular);

    // (b) V3 — the unit is still REFERENCED by a context, so this is the
    // association arm. V2 must stay silent: something IS declared here.
    //
    // (c) …and the finding is qualified UNVERIFIABLE. That is the claim this
    // whole arm exists to make: "not the unprefixed SI radian" would assert
    // something the guard never established and point at a defect that may not
    // exist. Pinned by tag rather than by the English `cannot verify`, so a
    // reword is noise-free while a COLLAPSE of the two V3 branches into the
    // verified-wrong wording still reds here — which the substring check could
    // not distinguish from a rename.
    assert_arms(&msg, &["V3", "UNVERIFIABLE"], &["V1", "V2", "V4"]);

    // (d) The actual OCCT class name is echoed, from `DynamicType()->Name()`.
    // That string is the whole point of the arm: it tells a reader what
    // spelling turned up, which is what they need to decide whether the
    // classifier should learn it or the writer should stop emitting it.
    assert!(
        msg.contains("StepBasic_PlaneAngleUnit"),
        "the refusal must name the unrecognised entity's OCCT class, so a \
         reader can see WHICH spelling the classifier could not read; got: {msg}"
    );

    // (e) Both the context and the unit are located by entity index.
    assert_names_a_context_index(&msg);
    positive_number_after(&msg, "reaches plane-angle unit #");

    // (f) The counts show the walk classified the substitute as ANGULAR (it is
    // counted) but not as a radian. If the third downcast were removed, the
    // bare unit would classify as NotAngular, drop out of `plane_angle_units`
    // entirely, and these two would be equal again.
    let StepGuardProbeResult {
        plane_angle_units,
        radian_ok,
        ..
    } = probe;
    assert!(
        plane_angle_units > radian_ok,
        "the substituted unit must be COUNTED as a plane-angle unit and must \
         not count as radian_ok — equal counts mean the classifier dropped it \
         as non-angular, which is the exact misclassification the third \
         downcast exists to prevent; got radian_ok={radian_ok} \
         plane_angle_units={plane_angle_units} in: {msg}"
    );
}

/// An ORPHANED plane-angle unit in an unverifiable form is REFUSED — V4's own
/// formatting branch for the same defect.
///
/// V4 formats its finding separately from V3 (it names an entity, not a
/// context), so the unverifiable case has a second message branch that V3's
/// test cannot reach. This fault adds the bare unit and touches nothing else,
/// so every context stays perfectly radian and V4 is the ONLY arm that can
/// fire — which is what makes this a clean pin rather than a by-product of
/// some other corruption.
#[test]
fn guard_refuses_an_orphaned_unverifiable_plane_angle_unit() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, same attribution.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::OrphanUnrecognised);

    // (b) V4 alone, qualified UNVERIFIABLE. Nothing existing was touched, so a
    // hit on any other arm means the fault did more than it claims to; and V4
    // must report an unreadable orphan as unverifiable rather than as a unit it
    // checked and rejected. V4 formats its finding separately from V3 (it names
    // an entity, not a context), so this is a second branch that V3's test
    // cannot reach and that needs its own tag pin.
    assert_arms(&msg, &["V4", "UNVERIFIABLE"], &["V1", "V2", "V3"]);

    // (c) The orphan's OCCT class name.
    assert!(
        msg.contains("StepBasic_PlaneAngleUnit"),
        "the refusal must name the orphan's OCCT class; got: {msg}"
    );
    positive_number_after(&msg, "unreferenced plane-angle unit #");

    // (d) The counts localise the finding to the orphan and nowhere else: the
    // three association counts must be untouched (see
    // `StepPlaneAngleAuditCounts::orphan_angular_units` for why they cannot
    // move), and `orphan_angular_units` must be exactly the one this fault
    // added, because the accept-path test pins a clean export at 0.
    assert_eq!(
        probe.orphan_angular_units, 1,
        "this fault adds exactly ONE unreferenced angular unit to a model that \
         `guard_accepts_a_real_multi_context_export` pins at zero orphans; a \
         different number means the fault or the fixture changed shape; got \
         orphan_angular_units={} in: {msg}",
        probe.orphan_angular_units
    );
    assert_eq!(
        probe.radian_ok, probe.plane_angle_units,
        "every CONTEXT is untouched by this fault and must still reach only \
         radians; got radian_ok={} plane_angle_units={} in: {msg}",
        probe.radian_ok, probe.plane_angle_units
    );
}

// ---------------------------------------------------------------------------
// Context RESOLUTION — every complex spelling must be unwrapped
// ---------------------------------------------------------------------------

/// A wrong unit reached through the OTHER complex context spelling is still
/// attributed to its CONTEXT.
///
/// Which complex context spellings exist, and why every one of them must be
/// unwrapped, is documented on `step_unit_assigned_context`
/// (`cpp/occt_wrapper.cpp`). What this test adds is coverage of the spelling
/// reify's own solid export never emits:
/// `guard_accepts_a_real_multi_context_export` cross-checks the resolved count
/// against the file text, but only for the ONE fixture it exports, so it
/// cannot speak for a spelling that fixture never produces.
///
/// THE FAILURE MODE IS SILENT AND MISLEADING, which is why this is pinned by
/// behaviour rather than by the count alone. Drop the two-part downcast and
/// this context resolves to nothing: V3 never runs for it, and its steradian —
/// now reachable from no context the walk can see — is reported by V4 as an
/// ORPHAN. The refusal still happens, so a count-only test would pass; the
/// diagnostic just blames the wrong thing, sending a reader to look for a
/// stray unit entity when the real defect is a context declaring the wrong
/// unit. Asserting V3 fires and V4 does not is what separates those.
#[test]
fn guard_resolves_the_two_part_complex_context_spelling() {
    let (kernel, union_id) = two_cone_union_kernel();

    // Baseline from the SAME kernel and shape, so the only difference between
    // the two runs is the injected context.
    let clean = kernel
        .export_step_with_injected_fault_for_test(union_id, "AP214", StepGuardFault::None)
        .expect("the uncorrupted export must be accepted");

    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::TwoPartContext);

    // (a) The finding is attributed to the CONTEXT that declares the unit —
    // arm V3 — and NOT to V4, which is where an unresolved context's unit
    // lands once nothing is seen to reference it.
    assert_arms(&msg, &["V3"], &["V1", "V2", "V4", "UNVERIFIABLE"]);
    assert_names_a_context_index(&msg);
    assert!(
        msg.contains(".STERADIAN.") || msg.contains("sunSteradian"),
        "the refusal must name the unit the added context declares; got: {msg}"
    );

    // (b) The walk RESOLVED the new context: exactly one more than the clean
    // export saw. A guard that skips this spelling reports the same count as
    // the clean run while still refusing (via V4), which is why the count and
    // the arm are both pinned.
    assert_eq!(
        probe.contexts,
        clean.contexts + 1,
        "the two-part complex context must be resolved and counted like any \
         other — an unchanged count means `step_unit_assigned_context` skipped \
         the spelling entirely; clean run saw {}, refusal reports {} in: {msg}",
        clean.contexts,
        probe.contexts
    );

    // (c) Nothing became an orphan. This is the direct discriminator: if the
    // context were skipped, its steradian would be referenced by no *visible*
    // context and would be counted here instead.
    assert_eq!(
        probe.orphan_angular_units, 0,
        "the added unit IS referenced — by the added context. Counting it as \
         an orphan means the context it hangs off was not resolved; got \
         orphan_angular_units={} in: {msg}",
        probe.orphan_angular_units
    );
}

// ---------------------------------------------------------------------------
// The V1 arm — a model that declares no unit-assigned context at all
// ---------------------------------------------------------------------------

/// A model the walk resolves NO unit-assigned context from is REFUSED.
///
/// V1 IS THE ANTI-VACUITY ARM, and it is the one arm whose absence is
/// invisible: a guard that resolves zero contexts satisfies every per-context
/// arm trivially and reports a clean bill of health on a file it never looked
/// at. That is the exact failure a naive direct
/// `DownCast<StepRepr_GlobalUnitAssignedContext>` produces on every real
/// export (the emitted entity is a COMPLEX composite), so this arm is what
/// converts "saw nothing" into a loud refusal.
///
/// The fault nulls the `GlobalUnitAssignedContext` each complex context
/// composes, which is the only way to express "no context" — an
/// `Interface_InterfaceModel` has no entity-removal API. The unit ENTITIES
/// stay in the model, so this also pins that a file still full of
/// `SI_UNIT($,.RADIAN.)` tokens is refused when nothing reaches them.
#[test]
fn guard_refuses_a_model_with_no_unit_assigned_context() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, same attribution.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::NoContext);

    // (b) V1 alone. V2 cannot fire (it quantifies over contexts, and there are
    // none); V3 likewise; V4 sees the now-unreferenced units but they are all
    // still correct unprefixed radians, which it skips by design.
    assert_arms(&msg, &["V1"], &["V2", "V3", "V4", "UNVERIFIABLE"]);

    // (c) The NON-NULL-model branch, distinguished from the null-model one by
    // the entity count it reports. Both are V1, and the two say different
    // things about where to look: "the model is null" is a wrapper bug, while
    // "walked N entities and found no context" is a defect in the file.
    positive_number_after(&msg, "walked ");

    // (d) The counts agree with the finding: nothing was resolved, so nothing
    // could be checked.
    assert_eq!(
        probe.contexts, 0,
        "V1 fires precisely when no unit-assigned context resolved; got \
         contexts={} in: {msg}",
        probe.contexts
    );
    assert_eq!(
        probe.plane_angle_units, 0,
        "with no context there are no (context, unit) associations to count; \
         got plane_angle_units={} in: {msg}",
        probe.plane_angle_units
    );
    assert_eq!(
        probe.radian_ok, 0,
        "with no associations none of them can be radian; got radian_ok={} \
         in: {msg}",
        probe.radian_ok
    );

    // (e) THE UNITS ARE STILL IN THE FILE. Only the references were removed,
    // so the emitted bytes would still be full of `SI_UNIT($,.RADIAN.)`
    // tokens. A guard built on a file-wide grep passes this model; only
    // quantifying over CONTEXTS refuses it.
    assert!(
        probe.orphan_angular_units > 0,
        "the plane-angle unit entities must still be present and merely \
         unreferenced — if they vanished too, this test would no longer show \
         that a file full of .RADIAN. tokens is refused when nothing reaches \
         them; got orphan_angular_units={} in: {msg}",
        probe.orphan_angular_units
    );
}

// ---------------------------------------------------------------------------
// The V5 arm — a representation context whose spelling the walk cannot resolve
// ---------------------------------------------------------------------------

/// A representation context of an UNKNOWN spelling is REFUSED, even though
/// every context the walk *did* resolve is a perfectly good radian.
///
/// V5 IS THE PARTIAL-BLINDNESS ARM, and it is the only one that can see this
/// case. `step_unit_assigned_context` unwraps three spellings; a fourth — a
/// later OCCT release, or a STEPCAFControl/XCAF writer path — returns null
/// from all three and would otherwise be skipped in silence. V1 cannot catch
/// that: it fires on a TOTAL of zero, and here the total is still three. So
/// without V5 the file exports cleanly with `contexts=3 plane_angle_units=3
/// radian_ok=3` and one context, which could be declaring degrees, never
/// looked at.
///
/// The assertions below are written to pin exactly that: the healthy counts
/// must SURVIVE (assertion (d)) while the refusal still fires (b). A test that
/// let the counts collapse would no longer distinguish V5 from V1.
#[test]
fn guard_refuses_an_unrecognised_representation_context_spelling() {
    let (kernel, union_id) = two_cone_union_kernel();

    // (a) Refused, with Reify's own attribution.
    let (msg, probe) = refusal(&kernel, union_id, StepGuardFault::UnrecognisedContext);

    // (b) V5 alone. Every other arm is quantified over things this fault did
    // not touch: the real contexts still resolve and still reach radians (V1,
    // V2, V3 silent), and it adds no angular unit at all (V4 silent).
    assert_arms(&msg, &["V5"], &["V1", "V2", "V3", "V4", "MODE", "UNVERIFIABLE"]);

    // (c) The diagnostic names the DYNAMIC TYPE it could not resolve. That is
    // the only actionable thing here — the fix is either to teach
    // `step_unit_assigned_context` the spelling or to add it to
    // `step_context_carries_no_units`, and neither is possible without knowing
    // which type it was.
    assert!(
        msg.contains("StepGuardUnknownContext"),
        "the refusal must name the unresolvable context's OCCT class, or a \
         reader cannot tell which spelling to teach the guard; got: {msg}"
    );
    assert!(
        msg.contains("entity #"),
        "the refusal must locate the entity in the model; got: {msg}"
    );

    // (d) THE COUNTS PROVE THIS IS THE PARTIAL CASE, not V1's total blindness.
    // The genuine contexts were untouched, so a guard that only measured the
    // total would see a completely healthy file here.
    assert_eq!(
        probe.unrecognised_contexts, 1,
        "this fault adds exactly ONE unresolvable context; a different number \
         means the fault or the allow-list changed shape; got \
         unrecognised_contexts={} in: {msg}",
        probe.unrecognised_contexts
    );
    assert!(
        probe.contexts >= 2,
        "the REAL contexts must still resolve — that is what makes this the \
         PARTIAL-blindness case V1 cannot see; got contexts={} in: {msg}",
        probe.contexts
    );
    assert_eq!(
        probe.radian_ok, probe.plane_angle_units,
        "every association this walk DID resolve is still a correct radian, \
         so the refusal rests on the skipped context alone; got radian_ok={} \
         of plane_angle_units={} in: {msg}",
        probe.radian_ok, probe.plane_angle_units
    );

    // (e) The header carries the count the violation line blames. Without it
    // the numbers above — three healthy contexts — sit directly over a line
    // saying the file was not verified.
    assert!(
        msg.contains("unrecognised_contexts="),
        "a V5-only refusal must report `unrecognised_contexts` in its header, \
         or the counts describe a healthy file directly above a line saying \
         they could not be trusted; got: {msg}"
    );
}

/// A clean export is ACCEPTED by the guard when it is NOT in the allow-list's
/// blind spot — i.e. the allow-list does not over-reach.
///
/// The companion to the test above, and the reason the allow-list is an
/// allow-list rather than a deny-list. `guard_accepts_a_real_multi_context_
/// export` already pins that a real export is accepted; this pins the
/// specifically V5-shaped way that could stop being true, by requiring the
/// clean model to report ZERO unresolvable contexts. If some real OCCT
/// spelling were dropped from both the downcast chain and the allow-list, the
/// counts there would still look healthy and only this assertion would red.
#[test]
fn a_clean_export_resolves_every_context_spelling_it_meets() {
    let (kernel, union_id) = two_cone_union_kernel();

    let probe = kernel
        .step_guard_probe_for_test(union_id, "AP214", StepGuardFault::None)
        .expect("a legitimate export must be accepted");

    assert!(
        probe.refusal.is_empty(),
        "a legitimate export must produce no refusal; got: {}",
        probe.refusal
    );
    assert_eq!(
        probe.unrecognised_contexts, 0,
        "every representation context a real export emits must resolve — \
         either to a unit assignment or to a known unit-free spelling. A \
         non-zero count means OCCT emits a spelling the guard does not know, \
         and the other counts CANNOT show that; got unrecognised_contexts={}",
        probe.unrecognised_contexts
    );
}

// ---------------------------------------------------------------------------
// The production entry point
// ---------------------------------------------------------------------------

/// The REAL `OcctKernel::export` path runs the guard and is accepted by it.
///
/// Every other test in this file enters through a `*_for_test` hook, so the
/// claim the whole design rests on — that the hooks run the production body —
/// is asserted nowhere against production itself. This test closes the half of
/// that gap which is observable from outside: the user-facing export reaches
/// the guard, is accepted, and returns the bytes the guard approved.
///
/// WHAT IT DELIBERATELY DOES NOT CLAIM. It cannot pin `export_step`'s
/// `StepGuardDisposition::Refuse` argument, because the two dispositions
/// differ only on a model the guard REFUSES, and no production input can
/// produce one (see `StepGuardFault` for why). That half is closed
/// structurally instead: `export_step` re-throws any non-empty refusal
/// whatever disposition it asked for, so a flip of that argument cannot
/// silently weaken a refusal into an empty file. Do not "strengthen" this test
/// by asserting a refusal here — there is no input that produces one.
#[test]
fn the_production_export_path_runs_the_guard() {
    let (kernel, union_id) = two_cone_union_kernel();

    let mut buf = Vec::new();
    kernel
        .export(union_id, reify_ir::ExportFormat::Step, &mut buf)
        .expect(
            "the production export path must ACCEPT a legitimate model — a \
             guard that refuses a correct file through the user-facing entry \
             point is worse than no guard at all",
        );

    assert!(
        !buf.is_empty(),
        "the production path must return the file the guard approved. An \
         empty-but-Ok result is exactly what a refusal reported rather than \
         thrown would look like from here"
    );

    let content = String::from_utf8(buf).expect("STEP output must be UTF-8");
    let stripped: String = content
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    assert!(
        stripped.contains("SI_UNIT($,.RADIAN.)"),
        "the accepted file must carry the unprefixed SI radian the guard \
         verified; without this the assertion above would pass on any \
         non-empty output"
    );
    assert!(
        stripped.contains("GLOBAL_UNIT_ASSIGNED_CONTEXT"),
        "the accepted file must carry the unit contexts the guard walks — \
         otherwise this test would pass on a file the guard had nothing to \
         look at"
    );
}
