//! Guard for the CROSS-REFERENCES that send a constraint author from the chunk
//! they retrieved to the interference/clearance oracle in `geometry.md`.
//!
//! Module doc is completed in the next step; this file currently carries only
//! the synthetic controls that pin `xref_region_violations`' discriminating
//! power, written BEFORE the predicate exists.

use crate::geometry_chunk_smoke::phantom_name_panic;

// ── Synthetic controls ───────────────────────────────────────────────────────
//
// `xref_region_violations` is PURE and fully parameterized over its input text
// — it does not read the chunks — exactly as
// `stdlib_chunk_geometry_ops_smoke.rs::geometry_op_doc_coverage_violations` is,
// so every class it decides can be driven with synthetic data rather than only
// through the two live chunks. A live-chunk-only pin is self-concealing: a
// predicate that quietly stopped deciding anything would leave both per-chunk
// tests green while establishing nothing.
//
// One control per class, plus the registry-truth class the predicate borrows
// wholesale from `geometry_chunk_smoke.rs`.

/// A well-formed pointer: both FORM B call forms, the backticked destination
/// topic, and short enough to still be a pointer rather than a copy.
///
/// Written in the shipped house idiom (`the `geometry` chunk — topic `geometry`
/// of `reify_language_reference``) so the clean control is the thing the chunks
/// actually carry, not a reduced caricature of it.
const POINTER_SIZED_REGION: &str = "\
Ask the kernel — never hand-roll a bounding-box overlap test, and never hand-compute a gap
from parameters.

Over let-bound geometry: `intersects(a, b) -> Bool` and `distance(a, b) -> Length`.

The posed form and the traps are in the `geometry` chunk — topic `geometry` of
`reify_language_reference`.
";

#[test]
fn a_pointer_sized_region_naming_both_call_forms_and_the_destination_is_clean() {
    assert_eq!(
        xref_region_violations(POINTER_SIZED_REGION, "synthetic.md"),
        Vec::<String>::new(),
        "the clean control must decide NOTHING is wrong — otherwise every other \
         control below is passing for the wrong reason"
    );
}

/// Dropping a call FORM is RED even when the bare word survives.
///
/// THE OPEN PAREN IS THE NEEDLE, and this control is what proves it. `distance`
/// is an ordinary English noun and is also the ARGUMENT name in geometry.md's
/// `extrude(profile, distance)` row — a region that merely contains those eight
/// letters has taught a constraint author nothing about the query. So the
/// region below says "distance" twice, in both of those innocent senses, and
/// must still be reported.
#[test]
fn a_region_that_drops_a_call_form_is_reported_by_that_call_form() {
    let region = "\
Over let-bound geometry: `intersects(a, b) -> Bool` tells you whether two solids foul.

Mind the difference between this and an extrusion distance — `extrude(profile, distance)`
takes a distance too, and it is not a query.

See the `geometry` chunk — topic `geometry` of `reify_language_reference`.
";

    let violations = xref_region_violations(region, "synthetic.md");

    assert_eq!(
        violations.len(),
        1,
        "expected exactly the missing-call-form violation, got: {violations:#?}"
    );
    assert!(
        violations[0].contains("distance("),
        "the violation must name the missing CALL FORM `distance(`, so a reader is \
         not sent looking for the bare word (which this region already has, twice): \
         {}",
        violations[0]
    );
}

/// A pointer that names no destination points nowhere.
///
/// THE BACKTICKS ARE THE NEEDLE here for the same reason the open paren is
/// above: "geometry" is an ordinary noun this very region uses twice in prose.
/// The backticked form is the structured retrieval KEY — a live topic in
/// `reify-mcp`'s topic list and the destination chunk's filename stem — not a
/// word.
#[test]
fn a_region_that_never_names_the_destination_topic_is_reported() {
    let region = "\
Ask the kernel rather than hand-rolling an overlap test over geometry you built yourself.

Over let-bound geometry: `intersects(a, b) -> Bool` and `distance(a, b) -> Length`.
";

    let violations = xref_region_violations(region, "synthetic.md");

    assert_eq!(
        violations.len(),
        1,
        "expected exactly the no-destination violation, got: {violations:#?}"
    );
    assert!(
        violations[0].contains(DESTINATION_TOPIC),
        "the violation must name the destination topic the pointer is missing: {}",
        violations[0]
    );
}

/// The G7 constraint, in executable form: a pointer that has started copying
/// what it points at is RED.
///
/// The body below is recognisably a RESTATEMENT of geometry.md's
/// "Clearance-query traps" catalogue — which is what a well-meaning editor
/// actually writes when they decide the reader should not have to follow a
/// link. That catalogue is 44 content lines in the live chunk, so it cannot fit
/// under the ceiling by construction; this control is the short version and is
/// already over.
#[test]
fn a_region_that_restates_the_trap_catalogue_exceeds_the_pointer_ceiling() {
    let region = "\
Over let-bound geometry: `intersects(a, b) -> Bool` and `distance(a, b) -> Length`.
See the `geometry` chunk — topic `geometry` of `reify_language_reference`. The traps:
1. Needs a realized kernel; a kernel-less engine goes indeterminate and still exits 0.
2. Let-bind twice — the call AND its arguments; an inline call is never visited.
3. The posed trio takes a snapshot and two body ids only; plain geometry yields undef.
4. A sub's `at` pose is not carried into a snapshot, so the handle is unposed.
5. No penetration depth: overlap clamps to zero, so severity cannot be ranked.
6. The overlap predicate is a non-strict comparison, so touching faces read as fouling.
7. A self-pair yields undef rather than zero.
8. Full containment reads as fouling under one kernel and is unmeasured under the other.
9. Only one of these is pinned end-to-end by a CLI test.
10. Read the destination section before writing a gate.
11. Really: read it.
12. This is no longer a pointer.
";

    let violations = xref_region_violations(region, "synthetic.md");

    assert_eq!(
        violations.len(),
        1,
        "expected exactly the pointer-ceiling violation, got: {violations:#?}"
    );
    assert!(
        violations[0].contains(&MAXIMUM_XREF_CONTENT_LINES.to_string()),
        "the violation must quote the ceiling it broke so a reader can weigh raising \
         it against why it exists: {}",
        violations[0]
    );
}

/// Registry truth, borrowed wholesale rather than reimplemented.
///
/// Asserting the WHOLE shared panic text, not a substring: the point of reusing
/// `phantom_name_panic` is that this module and the two existing ones cannot
/// drift apart on what a reader is told to do about a phantom name, and only an
/// equality check holds that.
#[test]
fn a_region_that_calls_a_phantom_name_is_reported_in_the_shared_wording() {
    let region = "\
Over let-bound geometry: `intersects(a, b) -> Bool`, `distance(a, b) -> Length` and
`bogus_query(a, b) -> Bool`.

See the `geometry` chunk — topic `geometry` of `reify_language_reference`.
";

    let violations = xref_region_violations(region, "synthetic.md");

    assert!(
        violations.contains(&phantom_name_panic(
            "synthetic.md",
            &format!("its `{ORACLE_XREF_MARKER}` region"),
            "bogus_query"
        )),
        "expected the SHARED phantom-name wording verbatim, got: {violations:#?}"
    );
}
