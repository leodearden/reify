//! Guard for the CROSS-REFERENCES that send an author from the chunk they
//! actually retrieved to the interference/clearance oracle in `geometry.md`.
//!
//! # Why this file exists
//!
//! Task 5389 closed the hole in `geometry.md` itself: the oracle now has a
//! section there, with worked fences and a trap catalogue, guarded by
//! `geometry_chunk_smoke.rs`. That fixed the destination and not the route.
//!
//! The printer_v01 dogfood session's entry point was writing a CONSTRAINT, and
//! chunks are retrieved PER TOPIC — so an assistant pulling `constraints` to
//! answer "how do I gate on parts not fouling?" never saw `geometry.md` at all,
//! read the oracle as a missing capability, and hand-rolled a bounding-box
//! overlap test. A correct destination nobody is routed to is still a hole; the
//! two chunks a clearance question is actually asked from are `constraints` and
//! `stdlib`, so those two carry a pointer and this file is what keeps the
//! pointers true.
//!
//! # What this file guards
//!
//! Per referring chunk (`constraints.md`, `stdlib.md`), five properties —
//! the first inherited, the next four decided by [`xref_region_violations`]:
//!
//! 1. **Region presence.** The `<!-- ORACLE-XREF -->` region EXISTS.
//!    [`section_body`](crate::geometry_chunk_smoke::section_body) PANICS on an
//!    absent marker, so deleting the pointer is RED rather than vacuously
//!    green. No anti-vacuity code is written here.
//! 2. **Call-form coverage.** The region names `intersects(` and `distance(` as
//!    CALL FORMS — an open paren, never a bare word.
//! 3. **Destination naming.** The region names the backticked retrieval topic
//!    it routes to.
//! 4. **Registry truth.** Every call-shaped name in the region is a live member
//!    of a compiler name registry, so a rename goes RED at the REFERRER and not
//!    only at the destination.
//! 5. **Pointer size.** The region stays under a content-line ceiling. This is
//!    the executable form of "a pointer, not a copy" — see
//!    [`MAXIMUM_XREF_CONTENT_LINES`].
//!
//! And in the other direction, once, from the referrer side:
//! [`the_xref_destination_still_answers_the_question`] pins that the section
//! both pointers promise still exists and still documents what they promise.
//!
//! # What is NOT established
//!
//! THE CANONICAL SCOPE STATEMENT FOR THIS FILE — test docstrings point back here.
//!
//! - **No prose is pinned. Nowhere.** Nothing here matches a sentence, a
//!   heading, an ordering or a typography choice in any of the three chunks.
//!   The only structural pins are the two byte-exact inert HTML-comment
//!   markers, which are house convention precisely so the shipped wording stays
//!   free to change — and that freedom is now load-bearing in a second way: the
//!   pointers name the retrieval TOPIC rather than `geometry.md`'s heading
//!   wording, exactly because that heading may be retitled at will.
//! - **Nothing here says the pointer is CORRECT, only that it is true and
//!   whole.** That the traps it defers to are accurate, that the destination
//!   section teaches what it should — those are `geometry.md`'s own guard's,
//!   and the eval/CLI tests its SYNC blocks map.
//! - **No ARITY, dimension or type claim is checked.** The pointers deliberately
//!   carry no ```` ```reify ```` fence: duplicating `geometry.md`'s worked
//!   `ClearanceGate` example is the SPOT/G7 lockstep duplication this file
//!   exists to prevent, so there is nothing here for a fence gate to compile.
//!
//! # No new scanner
//!
//! This module adds NONE. `section_body`, `called_names`, `registry_family` and
//! `phantom_name_panic` are all `geometry_chunk_smoke.rs`'s, already
//! `pub(crate)` and already parameterised by `chunk_path` so a `constraints.md`
//! failure names `constraints.md`. That follows task 5759's precedent
//! (`units_chunk_smoke.rs`) exactly: the harness binary now holds FIVE chunk
//! modules and STILL THREE scrapers.
//!
//! The shared `chunk_io` extraction those three still owe is task **#5924**
//! (ticket `tkt_0RS9A7843SBQ4BZX1A2ACY5TC1`), which is `deferred` — reuse
//! inside the existing binary is what is available today, not a substitute for
//! it.

use crate::geometry_chunk_smoke::{
    call_sites, called_names, phantom_name_panic, registry_family, section_body,
};

/// Marker that OPENS the cross-reference region in each REFERRING chunk.
/// Matched BYTE-EXACTLY on the trimmed line.
///
/// Minted in the same shape as `geometry_chunk_smoke.rs`'s
/// [`ORACLE_SECTION_MARKER`](crate::geometry_chunk_smoke::ORACLE_SECTION_MARKER)
/// and for the identical reason: an inert HTML comment costs the chunk one
/// line, is invisible in rendered markdown, and leaves the heading above it free
/// to be retitled. Scoping by the heading instead would make every check below a
/// wording pin on shipped prose.
const ORACLE_XREF_MARKER: &str = "<!-- ORACLE-XREF -->";

/// First referring chunk: where a designer writes the GATE.
///
/// Read (never written) at RUNTIME rather than `include_str!`d, mirroring
/// `geometry_chunk_smoke.rs`'s `CHUNK_PATH`, so an edit to the markdown is seen
/// by `cargo test` without a rebuild of this crate. If the chunk moves, this
/// const must move with it — the failure mode is a loud `expect` on the read,
/// not a silent skip.
const CONSTRAINTS_CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/constraints.md"
);

/// Second referring chunk: where a designer looks up WHAT THE CALL IS CALLED.
/// Same runtime-read contract as [`CONSTRAINTS_CHUNK_PATH`].
const STDLIB_CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/stdlib.md"
);

/// The oracle call forms every pointer must name.
///
/// FORM B ONLY — the two-argument, let-bound-geometry pair a constraint author
/// reaches for. The FORM A snapshot trio (`min_clearance` / `interferes_with` /
/// `interferes`) is deliberately absent: it needs a mechanism, a body table and
/// a snapshot before it answers anything, so enumerating it in a POINTER would
/// be teaching rather than routing. Documenting it stays `geometry.md`'s job,
/// and [`the_xref_destination_still_answers_the_question`] is what checks the
/// pointer's destination still does that job.
const REQUIRED_ORACLE_CALL_FORMS: &[&str] = &["intersects", "distance"];

/// The retrieval TOPIC each pointer must route to.
///
/// A topic name, not a heading and not a file path: it is a live key in
/// `reify-mcp`'s topic list and the destination chunk's filename stem —
/// structured data rather than meaningful prose — and it is what the reader
/// actually types. Matched BACKTICKED (see [`xref_region_violations`]), because
/// "geometry" unadorned is an ordinary noun these pointers use in prose.
const DESTINATION_TOPIC: &str = "geometry";

/// How many CONTENT lines a cross-reference region may carry before it has
/// stopped being a pointer and started being a copy.
///
/// Non-blank lines of the region, after HTML comments are removed; the marker
/// line is already excluded by `section_body`, which never emits it.
///
/// WHY A SIZE CEILING AND NOT A WORD BLOCKLIST. The constraint being encoded is
/// G7, no lockstep duplication: `geometry.md` owns the trap catalogue, and a
/// second copy of it in a referring chunk is a second thing to keep in step with
/// the compiler — which is the rot this whole task exists to prevent. Forbidding
/// the trap list's WORDS would be a prose pin, the one thing the house rule in
/// these files forbids: it would go RED against a correct rewording and green
/// against a reworded copy. A size ceiling is wording-blind and discriminates
/// for the right reason — that catalogue is 44 content lines in the live chunk
/// (measured 2026-09-14) and cannot fit under this by construction.
///
/// RE-MEASUREMENT PROTOCOL — the INVERSE of the one stated once on
/// `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES` for every `MINIMUM_*` floor.
/// This is a CEILING: it is raised only after re-reading WHY the pointer must
/// not become a copy, and NEVER to go green. Going RED here means an editor
/// started answering the question in the referring chunk instead of routing to
/// where it is already answered — the fix is almost always to cut, not to raise.
/// Set against the live pointers, with room for one clarifying sentence each and
/// no more: constraints.md's region measures 10 content lines.
const MAXIMUM_XREF_CONTENT_LINES: usize = 12;

/// Everything wrong with a cross-reference region, as human-actionable
/// violation lines — each naming its own corrective action, so a failure tells
/// a maintainer what to DO rather than only what is wrong.
///
/// FOUR classes, emitted in the order a fixer should work them (a region that
/// names no call form cannot also be judged on size usefully):
///
/// 1. **Call-form coverage.** Every [`REQUIRED_ORACLE_CALL_FORMS`] entry must
///    appear as a CALL, via
///    [`call_sites`](crate::geometry_chunk_smoke::call_sites) — the same
///    open-paren-and-balanced-parens rule `geometry.md`'s own coverage scan
///    uses. The paren is the whole discriminator: `distance` is an ordinary
///    English noun AND the argument name in `extrude(profile, distance)`, so a
///    region containing those eight letters has told a constraint author
///    nothing about the query.
/// 2. **Destination naming.** The region must carry [`DESTINATION_TOPIC`]
///    BACKTICKED. A pointer that names no destination points nowhere.
/// 3. **Registry truth.** Every call-shaped name must resolve through
///    [`registry_family`](crate::geometry_chunk_smoke::registry_family), and a
///    failure is reported in
///    [`phantom_name_panic`](crate::geometry_chunk_smoke::phantom_name_panic)'s
///    shared wording so the three chunk modules cannot drift on what a reader is
///    told about a phantom name.
/// 4. **Pointer size.** See [`MAXIMUM_XREF_CONTENT_LINES`].
///
/// HTML COMMENTS ARE STRIPPED FIRST, by [`strip_html_comments`]. They are
/// editor-facing notes the assistant never renders — house convention puts SYNC
/// blocks inside exactly these marked regions — so counting them against the
/// pointer ceiling would penalise the note that keeps a placement constraint
/// legible, and scanning them for call names would hold an editor note to a
/// registry it makes no claim against.
///
/// PURE and fully parameterized over its input text — it does not read the
/// chunks — exactly as
/// `stdlib_chunk_geometry_ops_smoke.rs::geometry_op_doc_coverage_violations` is,
/// so the controls below pin every class with synthetic data.
fn xref_region_violations(region: &str, chunk_path: &str) -> Vec<String> {
    let prose = strip_html_comments(region);
    let mut out = Vec::new();

    for name in REQUIRED_ORACLE_CALL_FORMS.iter().copied() {
        if call_sites(&prose, name).is_empty() {
            out.push(format!(
                "{chunk_path}'s `{ORACLE_XREF_MARKER}` region never names `{name}(` as a CALL \
                 FORM. The open paren is the point: a reader who has only seen the bare word \
                 cannot write the call. FIX: name it as a call with its return type, e.g. \
                 `{name}(a, b) -> …`, over let-bound geometry — or, if the oracle really did \
                 lose this form, fix the destination section first and change \
                 REQUIRED_ORACLE_CALL_FORMS with it."
            ));
        }
    }

    let topic_key = format!("`{DESTINATION_TOPIC}`");
    if !prose.contains(&topic_key) {
        out.push(format!(
            "{chunk_path}'s `{ORACLE_XREF_MARKER}` region never names the destination topic \
             {topic_key} — a pointer that names no destination points nowhere, which is the \
             very regression this region exists to close. FIX: route the reader in the house \
             idiom the other chunks use — the {topic_key} chunk, topic {topic_key} of \
             `reify_language_reference` — and write that tool name WITHOUT parentheses (a \
             call-shaped spelling is reported as a phantom builtin by reify-audit's PDOCCOVER \
             fabrication lane)."
        ));
    }

    for name in called_names(&prose) {
        if registry_family(&name).is_none() {
            out.push(phantom_name_panic(
                chunk_path,
                &format!("its `{ORACLE_XREF_MARKER}` region"),
                &name,
            ));
        }
    }

    let content_lines = prose.lines().filter(|line| !line.trim().is_empty()).count();
    if content_lines > MAXIMUM_XREF_CONTENT_LINES {
        out.push(format!(
            "{chunk_path}'s `{ORACLE_XREF_MARKER}` region is {content_lines} content lines, over \
             the {MAXIMUM_XREF_CONTENT_LINES}-line pointer ceiling — it has stopped routing the \
             reader and started answering the question a second time, which is the lockstep \
             duplication this cross-reference exists to avoid. FIX: cut it back to a route. Read \
             MAXIMUM_XREF_CONTENT_LINES' re-measurement protocol before raising the ceiling, and \
             never raise it merely to go green."
        ));
    }

    out
}

/// `markdown` with every `<!-- … -->` comment removed.
///
/// An UNTERMINATED comment consumes the remainder, which is exactly what a
/// markdown renderer does with it — so a region whose pointer has been swallowed
/// by a stray `<!--` reports as missing its call forms, which is the true
/// description of what the reader can now see.
fn strip_html_comments(markdown: &str) -> String {
    const OPEN: &str = "<!--";
    const CLOSE: &str = "-->";

    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;

    while let Some(open) = rest.find(OPEN) {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find(CLOSE) else {
            return out;
        };
        rest = &rest[open + close + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

// ── The live pointers ────────────────────────────────────────────────────────
//
// TWO separate `#[test]` fns rather than one loop over a table of chunks, so a
// failure names its own chunk in the TEST NAME as well as in the panic. The
// shared logic already lives in `xref_region_violations`, so this is not
// duplication — it is the one thing a table would cost.

/// Human-readable name of the marked region, for PANIC TEXT ONLY. Nothing
/// matches on it, so each chunk may title its section however reads best —
/// which is the entire reason the region is scoped by an inert marker.
const XREF_REGION_TITLE: &str = "interference/clearance cross-reference";

/// The chunk a designer is in when they write the GATE must route to the
/// oracle, and the route must be whole.
///
/// This is the printer_v01 entry point: the question "how do I constrain these
/// two parts not to foul?" is asked while writing a `constraint`, and chunk
/// retrieval is per topic — so `geometry.md` being correct is no help unless
/// something here points at it. Scope of what "whole" means, and of what is
/// deliberately NOT checked: this module's doc.
#[test]
fn the_constraints_chunk_points_at_the_oracle() {
    let markdown = std::fs::read_to_string(CONSTRAINTS_CHUNK_PATH).unwrap_or_else(|e| {
        panic!(
            "{CONSTRAINTS_CHUNK_PATH} must be readable ({e}) — update \
             CONSTRAINTS_CHUNK_PATH if the chunk moved"
        )
    });

    let region = section_body(
        &markdown,
        ORACLE_XREF_MARKER,
        CONSTRAINTS_CHUNK_PATH,
        XREF_REGION_TITLE,
    );

    let violations = xref_region_violations(&region, CONSTRAINTS_CHUNK_PATH);
    assert!(
        violations.is_empty(),
        "{}",
        violations.join("\n\n")
    );
}

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

// ── Scanner unit tests ───────────────────────────────────────────────────────
//
// `strip_html_comments` is this module's ONLY hand-rolled text helper, and every
// class of `xref_region_violations` runs downstream of it. It is pinned DIRECTLY
// here rather than only through the controls above, following the posture
// `geometry_chunk_smoke.rs`'s own "Scanner unit tests" block establishes: the
// failure it guards against is self-concealing. A stripper that quietly returned
// nothing would empty every scan, and the call-form class would then blame the
// chunk for a defect in this function.

#[test]
fn html_comments_are_removed_and_the_prose_around_them_is_kept() {
    assert_eq!(
        strip_html_comments("before\n<!-- an editor note\n   spanning lines -->\nafter\n"),
        "before\n\nafter\n"
    );
}

#[test]
fn an_unterminated_html_comment_consumes_the_remainder() {
    // What a markdown renderer does with it, so what the reader sees. The
    // region then reports as missing its call forms, which is TRUE of the
    // rendered chunk — not a scanner defect to be worked around.
    assert_eq!(
        strip_html_comments("visible\n<!-- swallowed\n`intersects(a, b)`\n"),
        "visible\n"
    );
}
