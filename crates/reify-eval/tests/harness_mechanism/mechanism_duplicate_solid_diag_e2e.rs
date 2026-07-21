//! E2E tests for the `E_MECHANISM_DUPLICATE_SOLID` diagnostic-emission seam
//! (task 4308 — mechanism δ).
//!
//! Covers:
//! - Single duplicate-solid event (baseline): exactly one diagnostic emitted.
//! - Propagation dedup: a propagated errored Map across multiple cells still
//!   yields exactly one diagnostic (structural-equality dedup collapses copies).
//! - Two independent duplicate-solid mechanisms: two diagnostics emitted
//!   (structurally distinct error Maps produce separate diagnostics).
//!
//! The propagation-dedup and structural-dedup behaviors are documented as
//! known v0.1 limitations in `detect_mechanism_errors` in `engine_eval.rs`.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// A `.ri` source where solid `"a"` is attached to two separate body() calls —
/// the canonical duplicate-solid recipe (mirrors `ERRORED_SOURCE` in
/// `forward_kinematics_e2e.rs`).
const DUPLICATE_SOLID_SOURCE: &str = r#"
structure def DuplicateSolid {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)

    let m0 = mechanism()
    let m1 = body(m0, "a", j_a)
    // Attaching solid "a" a second time (different joint) triggers duplicate_solid.
    let m2 = body(m1, "a", j_b)
}
"#;

/// `Engine::eval` must emit exactly one `E_MECHANISM_DUPLICATE_SOLID` Error
/// diagnostic when the source contains a duplicate-solid mechanism.
///
/// Assertions:
/// 1. The diagnostics list contains EXACTLY ONE entry with
///    `severity == Severity::Error` AND
///    `code == Some(DiagnosticCode::MechanismDuplicateSolid)`.
///
/// The test keys on the `DiagnosticCode`, NOT the message text, to remain
/// stable as the wording evolves.
#[test]
fn eval_emits_mechanism_duplicate_solid_error_diagnostic() {
    let compiled = parse_and_compile_with_stdlib(DUPLICATE_SOLID_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::MechanismDuplicateSolid)
        })
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "Engine::eval must emit exactly one E_MECHANISM_DUPLICATE_SOLID Error diagnostic \
         for a duplicate-solid source; got {} matching diagnostic(s) out of {} total.\n\
         All diagnostics: {:#?}",
        matching.len(),
        result.diagnostics.len(),
        result.diagnostics,
    );
}

/// A `.ri` source where the errored mechanism Map propagates to a downstream
/// cell: `m3 = body(m2, "b", j_c)` where `m2` is already errored.
///
/// `body()` on an already-errored mechanism returns the error Map verbatim
/// (mechanism.rs:88), so `m2` and `m3` hold the structurally identical Map.
/// The dedup-by-structural-equality pass in `detect_mechanism_errors` must
/// collapse the two copies to a SINGLE diagnostic.
const PROPAGATED_ERROR_SOURCE: &str = r#"
structure def PropagatedDuplicateSolid {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)
    let j_c = prismatic(vec3(0, 0, 1), 0mm .. 1000mm)

    let m0 = mechanism()
    let m1 = body(m0, "a", j_a)
    // m2: duplicate solid "a" → carries error="duplicate_solid"
    let m2 = body(m1, "a", j_b)
    // m3: body() on already-errored m2 returns m2 verbatim (same error Map)
    let m3 = body(m2, "b", j_c)
}
"#;

/// Propagation dedup: when an errored mechanism Map propagates to downstream
/// cells (`m3` holds the same error Map as `m2`), `detect_mechanism_errors`
/// must emit exactly ONE diagnostic — not one per propagated cell.
///
/// Documents the structural-equality dedup behaviour: the dedup is necessary
/// because a single duplicate-solid event can propagate to many downstream
/// cells.  The counterpart limitation (two genuinely distinct errors that
/// happen to be structurally identical collapse to one) is documented as a
/// known v0.1 limitation in `detect_mechanism_errors`.
#[test]
fn eval_deduplicates_propagated_mechanism_error() {
    let compiled = parse_and_compile_with_stdlib(PROPAGATED_ERROR_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::MechanismDuplicateSolid)
        })
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "propagated errored Map (m2 and m3 hold identical error Maps) must collapse to \
         exactly ONE E_MECHANISM_DUPLICATE_SOLID diagnostic; \
         got {} matching diagnostic(s) out of {} total.\n\
         All diagnostics: {:#?}",
        matching.len(),
        result.diagnostics.len(),
        result.diagnostics,
    );
}

/// A `.ri` source with TWO independent mechanisms, each with a distinct
/// duplicate-solid error.  The two error Maps are structurally different
/// (their `bodies` lists differ because one recorded solid "a" and the other
/// recorded solid "b"), so dedup must retain BOTH and emit two diagnostics.
const TWO_INDEPENDENT_ERRORS_SOURCE: &str = r#"
structure def TwoIndependentDuplicates {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)

    // Mechanism A: duplicate solid "a"
    let ma0 = mechanism()
    let ma1 = body(ma0, "a", j_a)
    let ma2 = body(ma1, "a", j_b)

    // Mechanism B: duplicate solid "b" (distinct mechanism state → distinct error Map)
    let mb0 = mechanism()
    let mb1 = body(mb0, "b", j_a)
    let mb2 = body(mb1, "b", j_b)
}
"#;

/// Two independent duplicate-solid mechanisms must yield TWO diagnostics.
///
/// The two error Maps are structurally different (their `bodies` lists contain
/// different solid names), so structural-equality dedup keeps both.
/// Documents the dedup boundary: propagation copies collapse, but genuinely
/// distinct mechanisms do not.
///
/// Note: if the two mechanisms happened to be structurally identical (same
/// joint values, same solid name, same internal state) they would collapse to
/// one diagnostic — that is the known v0.1 limitation documented in
/// `detect_mechanism_errors`.  This test uses different solid names ("a" vs
/// "b") to ensure the Maps are structurally distinct.
#[test]
fn eval_emits_two_diagnostics_for_two_independent_duplicate_solid_mechanisms() {
    let compiled = parse_and_compile_with_stdlib(TWO_INDEPENDENT_ERRORS_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::MechanismDuplicateSolid)
        })
        .collect();

    assert_eq!(
        matching.len(),
        2,
        "two structurally-distinct duplicate-solid mechanisms must yield exactly TWO \
         E_MECHANISM_DUPLICATE_SOLID Error diagnostics; \
         got {} matching diagnostic(s) out of {} total.\n\
         All diagnostics: {:#?}",
        matching.len(),
        result.diagnostics.len(),
        result.diagnostics,
    );
}
