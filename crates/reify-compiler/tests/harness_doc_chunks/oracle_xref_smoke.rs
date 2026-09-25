//! Guard for the CROSS-REFERENCES that send an author from the chunk they
//! actually retrieved to the interference/clearance oracle in `geometry.md`.
//!
//! # Why this file exists
//!
//! Task 5389 gave the oracle a section in `geometry.md`, guarded by
//! `geometry_chunk_smoke.rs`. That fixed the destination and not the route.
//!
//! Chunks are retrieved PER TOPIC, and the printer_v01 dogfood session's entry
//! point was writing a CONSTRAINT — so an assistant pulling `constraints` to
//! answer "how do I gate on parts not fouling?" never saw `geometry.md` at all,
//! read the oracle as a missing capability, and hand-rolled a bounding-box
//! overlap test. A correct destination nobody is routed to is still a hole; the
//! two chunks a clearance question is actually asked from are `constraints` and
//! `stdlib`, so those two carry a pointer and this file is what keeps the
//! pointers true.
//!
//! # What this file guards
//!
//! Per referring chunk (`constraints.md`, `stdlib.md`): that the
//! `<!-- ORACLE-XREF -->` region EXISTS —
//! [`section_body`](crate::geometry_chunk_smoke::section_body) PANICS on an
//! absent marker, so deleting the pointer is RED rather than vacuously green,
//! and no anti-vacuity code is written here — plus the five violation classes
//! [`xref_region_violations`] decides.
//!
//! The DANGLING-POINTER direction is closed STRUCTURALLY rather than by a second
//! coverage test: [`REQUIRED_ORACLE_CALL_FORMS`] *is*
//! `geometry_chunk_smoke.rs`'s `GEOMETRY_ORACLE_NAMES`, and that module already
//! requires every entry of it to be documented as a call form inside
//! `geometry.md`'s `ORACLE-SECTION`. So a form the pointers must name is a form
//! the destination must document, retiring one is a single edit, and no pointer
//! can be left routing to a section that stopped answering the question. The one
//! tie the shared list does NOT carry is from the retrieval TOPIC to that chunk;
//! [`the_destination_topic_names_the_chunk_the_pointers_route_to`] is that tie.
//!
//! # What is NOT established
//!
//! THE CANONICAL SCOPE STATEMENT FOR THIS FILE — test docstrings point back here.
//!
//! - **No prose is pinned. Nowhere.** Nothing here matches a sentence, a
//!   heading, an ordering or a typography choice in any of the three chunks —
//!   including the size ceiling, which counts WORDS precisely so a rewrap decides
//!   nothing. The only structural pins are the two byte-exact inert HTML-comment
//!   markers, which are house convention so the shipped wording stays free to
//!   change — and that freedom is load-bearing in a second way: the pointers name
//!   the retrieval TOPIC and never `geometry.md`'s heading wording, exactly
//!   because that heading may be retitled at will.
//! - **Nothing here says the pointer is CORRECT, only that it is true and
//!   whole.** That the traps it defers to are accurate, that the destination
//!   section teaches what it should — those are `geometry.md`'s own guard's, and
//!   the eval/CLI tests its SYNC blocks map.
//! - **No ARITY, dimension or type claim is checked.** The pointers deliberately
//!   carry no ```` ```reify ```` fence: duplicating `geometry.md`'s worked
//!   `ClearanceGate` example is the SPOT/G7 lockstep duplication this file
//!   exists to prevent, so there is nothing here for a fence gate to compile.
//!
//! # No new chunk scanner
//!
//! `section_body`, `call_sites`, `called_names`, `registry_family` and
//! `phantom_name_panic` are all `geometry_chunk_smoke.rs`'s, already `pub(crate)`
//! and already parameterised by `chunk_path` so a `constraints.md` failure names
//! `constraints.md`. The ONE helper added here is [`strip_html_comments`], and it
//! is pinned directly by the unit tests at the foot of this file.
//!
//! That follows task 5759's precedent (`units_chunk_smoke.rs`) exactly: the
//! harness binary now holds FIVE chunk modules and STILL THREE scrapers. The
//! shared `chunk_io` extraction those three still owe is task **#5924** (ticket
//! `tkt_0RS9A7843SBQ4BZX1A2ACY5TC1`), which is `deferred` — reuse inside the
//! existing binary is what is available today, not a substitute for it.

use crate::geometry_chunk_smoke::{
    CHUNK_PATH as GEOMETRY_CHUNK_PATH, GEOMETRY_ORACLE_NAMES, call_sites, called_names,
    phantom_name_panic, registry_family, section_body,
};

/// Marker that OPENS the cross-reference region in each REFERRING chunk.
/// Matched BYTE-EXACTLY on the trimmed line.
///
/// Minted in the same shape as `geometry_chunk_smoke.rs`'s
/// `ORACLE_SECTION_MARKER` and for the identical reason: an inert HTML comment
/// costs the chunk one line, is invisible in rendered markdown, and leaves the
/// heading above it free to be retitled. Scoping by the heading instead would
/// make every check below a wording pin on shipped prose.
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

/// The oracle call forms every pointer must name — `geometry_chunk_smoke.rs`'s
/// list, ALIASED rather than copied.
///
/// Sharing the list is what closes the dangling-pointer direction without a
/// second coverage test; see this module's doc. A hand-kept second copy would
/// make retiring a form two edits, and a half-done retirement would leave the
/// pointers routing readers to a name the oracle no longer documents.
///
/// FORM B ONLY — the two-argument, let-bound-geometry pair a constraint author
/// reaches for. The FORM A snapshot trio (`geometry_chunk_smoke.rs`'s
/// `KINEMATIC_ORACLE_NAMES`) is deliberately outside this list: it needs a
/// mechanism, a body table and a snapshot before it answers anything, so
/// enumerating it in a POINTER would be teaching rather than routing.
const REQUIRED_ORACLE_CALL_FORMS: &[&str] = GEOMETRY_ORACLE_NAMES;

/// The retrieval TOPIC each pointer must route to.
///
/// A topic name, not a heading and not a file path: it is a live key in
/// `reify-mcp`'s topic list and the destination chunk's filename stem —
/// structured data rather than meaningful prose — and it is what the reader
/// actually types. The filename-stem half is executable here, in
/// [`the_destination_topic_names_the_chunk_the_pointers_route_to`]; the
/// topic-list half is not, because `reify-mcp` is not reachable from
/// `reify-compiler`'s dev-dependencies.
///
/// Matched BACKTICKED (see [`xref_region_violations`]), because "geometry"
/// unadorned is an ordinary noun these pointers use in prose.
const DESTINATION_TOPIC: &str = "geometry";

/// How many WORDS a cross-reference region may carry before it has stopped being
/// a pointer and started being a copy.
///
/// Whitespace-separated words of the region after HTML comments are removed; the
/// marker line is already excluded by `section_body`, which never emits it.
///
/// WORDS, NOT LINES. A content-LINE count is a pure function of where the author
/// hard-wrapped, so reflowing the identical words at a narrower column would go
/// RED with zero content change — a prose pin by the back door, and the one thing
/// the house rule in these files forbids.
///
/// WHY A SIZE CEILING AND NOT A WORD BLOCKLIST. The constraint being encoded is
/// G7, no lockstep duplication: `geometry.md` owns the trap catalogue, and a
/// second copy of it in a referring chunk is a second thing to keep in step with
/// the compiler — the rot this whole task exists to prevent. Forbidding the trap
/// list's WORDS would be the same prose pin from the other side: RED against a
/// correct rewording, green against a reworded copy. A size ceiling is
/// wording-blind and discriminates for the right reason — that catalogue is 574
/// words in the live chunk (measured 2026-09-17) and cannot fit under this by
/// construction.
///
/// MEASURED 2026-09-17: `constraints.md`'s region is 131 words and `stdlib.md`'s
/// is 100. The ceiling is set against the LARGER of the two, leaving it about one
/// clarifying sentence and no more. To RE-MEASURE rather than predict: set this
/// const to 1, run
/// `env cargo test -p reify-compiler --test harness_doc_chunks points_at_the_oracle`,
/// and read both sizes straight out of the two panics — each violation states its
/// own region's measured size — then restore the const and confirm `git diff` on
/// it is empty.
///
/// This is a CEILING, the INVERSE of `geometry_chunk_smoke.rs`'s `MINIMUM_*`
/// floors: it is raised only after re-reading WHY the pointer must not become a
/// copy, and NEVER to go green. Going RED here means an editor started answering
/// the question in the referring chunk instead of routing to where it is already
/// answered — the fix is almost always to cut.
const MAXIMUM_XREF_WORDS: usize = 150;

/// Everything wrong with a cross-reference region, as human-actionable
/// violation lines — each naming its own corrective action, so a failure tells
/// a maintainer what to DO rather than only what is wrong.
///
/// FIVE classes, emitted in the order a fixer should work them (a region that
/// names no call form cannot also be judged on size usefully):
///
/// 1. **Comment integrity.** No [`HTML_COMMENT_CLOSE`] may survive
///    [`strip_html_comments`]. FIRST deliberately: every class below reads the
///    same stripped `prose`, so a note that closed early can fail them as a size
///    or coverage defect, and a fixer who sees this line first is spared chasing
///    the symptom.
/// 2. **Call-form coverage.** Every [`REQUIRED_ORACLE_CALL_FORMS`] entry must
///    appear as a CALL, via
///    [`call_sites`](crate::geometry_chunk_smoke::call_sites) — the same
///    open-paren-and-balanced-parens rule `geometry.md`'s own coverage scan uses.
///    The paren is the whole discriminator: `distance` is an ordinary English
///    noun AND the argument name in `extrude(profile, distance)`, so a region
///    containing those eight letters has told a constraint author nothing about
///    the query.
/// 3. **Destination naming.** The region must carry [`DESTINATION_TOPIC`]
///    BACKTICKED. A pointer that names no destination points nowhere.
/// 4. **Registry truth.** Every call-shaped name must resolve through
///    [`registry_family`](crate::geometry_chunk_smoke::registry_family), and a
///    failure is reported in
///    [`phantom_name_panic`](crate::geometry_chunk_smoke::phantom_name_panic)'s
///    shared wording so the chunk modules cannot drift on what a reader is told
///    about a phantom name.
/// 5. **Pointer size.** See [`MAXIMUM_XREF_WORDS`].
///
/// HTML COMMENTS ARE STRIPPED FIRST, so an editor-facing note costs the pointer
/// nothing: house convention puts placement notes and SYNC blocks inside exactly
/// these marked regions, and charging them to the ceiling would penalise the note
/// that keeps a placement constraint legible, while scanning them for call names
/// would hold a note to a registry it makes no claim against. That promise holds
/// only for a WHOLE comment — the stripper is HTML-FAITHFUL rather than
/// nesting-aware, correctly, since that is what the reader's renderer does — so a
/// note that quotes its own terminator ends at the quote and its tail is prose.
/// Class 1 is what REPORTS that, instead of silently charging it.
///
/// PURE and fully parameterized over its input text — it does not read the
/// chunks — exactly as
/// `stdlib_chunk_geometry_ops_smoke.rs::geometry_op_doc_coverage_violations` is,
/// so the controls below pin every class with synthetic data.
fn xref_region_violations(region: &str, chunk_path: &str) -> Vec<String> {
    let prose = strip_html_comments(region);
    let mut out = Vec::new();

    if prose.contains(HTML_COMMENT_CLOSE) {
        out.push(format!(
            "{chunk_path}'s `{ORACLE_XREF_MARKER}` region still carries a bare \
             `{HTML_COMMENT_CLOSE}` after its HTML comments were stripped. A terminator that \
             survives stripping was never opened, so an editor note in this region CLOSED \
             EARLIER than its author intended: the tail of the note is now rendered text the \
             reader sees, and it is being charged to the pointer's word budget. HTML comments \
             do not nest and HTML defines no escape inside one, so backticks do not protect a \
             quoted terminator — writing a marker out in full is what ends the note. FIX: name \
             the marker WITHOUT its closing bracket (`ORACLE-XREF`, not the whole comment), or \
             move that sentence out of the comment. Do NOT reword the pointer; the pointer is \
             not what is wrong."
        ));
    }

    for name in REQUIRED_ORACLE_CALL_FORMS.iter().copied() {
        if call_sites(&prose, name).is_empty() {
            out.push(format!(
                "{chunk_path}'s `{ORACLE_XREF_MARKER}` region never names `{name}(` as a CALL \
                 FORM. The open paren is the point: a reader who has only seen the bare word \
                 cannot write the call. FIX: name it as a call with its return type, e.g. \
                 `{name}(a, b) -> …`, over let-bound geometry — or, if the oracle really did \
                 lose this form, retire it from geometry_chunk_smoke.rs's \
                 GEOMETRY_ORACLE_NAMES, which is the list both this region and the destination \
                 section are held to."
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

    let words = prose.split_whitespace().count();
    if words > MAXIMUM_XREF_WORDS {
        out.push(format!(
            "{chunk_path}'s `{ORACLE_XREF_MARKER}` region is {words} words, over the \
             {MAXIMUM_XREF_WORDS}-word pointer ceiling — it has stopped routing the reader and \
             started answering the question a second time, which is the lockstep duplication \
             this cross-reference exists to avoid. FIX: cut it back to a route. Read \
             MAXIMUM_XREF_WORDS' re-measurement protocol before raising the ceiling, and never \
             raise it merely to go green."
        ));
    }

    out
}

/// The HTML comment grammar, as ONE definition shared by the stripper below and
/// by [`xref_region_violations`]' comment-integrity class. A stripper and a
/// debris check that disagreed about what closes a comment would each be
/// reporting on a document the other never saw.
const HTML_COMMENT_OPEN: &str = "<!--";
const HTML_COMMENT_CLOSE: &str = "-->";

/// `markdown` with every `<!-- … -->` comment removed.
///
/// An UNTERMINATED comment consumes the remainder, which is exactly what a
/// markdown renderer does with it — so a region whose pointer has been swallowed
/// by a stray `<!--` reports as missing its call forms, which is the true
/// description of what the reader can now see.
fn strip_html_comments(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;

    while let Some(open) = rest.find(HTML_COMMENT_OPEN) {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find(HTML_COMMENT_CLOSE) else {
            return out;
        };
        rest = &rest[open + close + HTML_COMMENT_CLOSE.len()..];
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
    assert!(violations.is_empty(), "{}", violations.join("\n\n"));
}

/// The chunk a designer is in when they look up WHAT THE CALL IS CALLED must
/// route to the oracle too, and the route must be whole.
///
/// The second half of the printer_v01 entry point, and the one with a live
/// near-miss in it: this chunk already lists `intersection(a, b)`, the CSG
/// boolean that BUILDS a solid. An author scanning for a fouling check finds a
/// plausible-looking name, and nothing here would tell them it answers a
/// different question. Scope: this module's doc.
#[test]
fn the_stdlib_chunk_points_at_the_oracle() {
    let markdown = std::fs::read_to_string(STDLIB_CHUNK_PATH).unwrap_or_else(|e| {
        panic!(
            "{STDLIB_CHUNK_PATH} must be readable ({e}) — update STDLIB_CHUNK_PATH \
             if the chunk moved"
        )
    });

    let region = section_body(
        &markdown,
        ORACLE_XREF_MARKER,
        STDLIB_CHUNK_PATH,
        XREF_REGION_TITLE,
    );

    let violations = xref_region_violations(&region, STDLIB_CHUNK_PATH);
    assert!(violations.is_empty(), "{}", violations.join("\n\n"));
}

/// The topic both pointers route to must be the chunk the oracle lives in.
///
/// The executable half of [`DESTINATION_TOPIC`]'s claim, and the ONE tie to the
/// destination this module still owes once [`REQUIRED_ORACLE_CALL_FORMS`] is
/// shared with the destination's own guard. Renaming the chunk — or the topic —
/// leaves both shipped pointers routing readers at a topic that no longer
/// retrieves the oracle, while every string match in this file keeps matching,
/// because nothing else here reads the destination's path at all.
#[test]
fn the_destination_topic_names_the_chunk_the_pointers_route_to() {
    assert!(
        GEOMETRY_CHUNK_PATH.ends_with(&format!("/{DESTINATION_TOPIC}.md")),
        "both pointers route readers to topic `{DESTINATION_TOPIC}`, but the oracle now lives \
         in {GEOMETRY_CHUNK_PATH} — a chunk whose filename stem that topic no longer names. \
         Chunk retrieval is BY TOPIC, so both shipped pointers are dangling. FIX: rewrite \
         DESTINATION_TOPIC and both pointer regions to the topic that retrieves the oracle, in \
         the same commit."
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
// wholesale from `geometry_chunk_smoke.rs`, plus the size class's boundary.

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
/// link. That catalogue is 574 words in the live chunk, so it cannot fit under
/// the ceiling by construction; this control is the short version, at 166 words,
/// and is already over.
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
        violations[0].contains(&MAXIMUM_XREF_WORDS.to_string()),
        "the violation must quote the ceiling it broke so a reader can weigh raising \
         it against why it exists: {}",
        violations[0]
    );
}

/// [`POINTER_SIZED_REGION`] padded to exactly `words` whitespace-separated words,
/// and clean on every class but the size one.
///
/// Derived FROM [`MAXIMUM_XREF_WORDS`] by its two callers rather than from a
/// hand-copied number, so the boundary stays pinned when the ceiling is
/// re-measured. The filler is a bare word: no paren for `called_names` to find,
/// no backtick and no terminator, so nothing but the word count changes.
fn region_padded_to(words: usize) -> String {
    let base = POINTER_SIZED_REGION.split_whitespace().count();
    assert!(
        base <= words,
        "the clean control is already {base} words and cannot be padded down to {words} — \
         shorten POINTER_SIZED_REGION or raise MAXIMUM_XREF_WORDS"
    );
    let filler = vec!["padding"; words - base].join(" ");
    format!("{POINTER_SIZED_REGION}{filler}\n")
}

/// A region of EXACTLY the ceiling is clean...
#[test]
fn a_region_of_exactly_the_pointer_ceiling_is_clean() {
    assert_eq!(
        xref_region_violations(&region_padded_to(MAXIMUM_XREF_WORDS), "synthetic.md"),
        Vec::<String>::new(),
        "the ceiling is a MAXIMUM, not an exclusive bound: a region that spends all \
         of its budget is a pointer with no slack left, not a copy"
    );
}

/// ...and one word over it is reported.
///
/// The pair is what pins the COMPARISON. The trap-catalogue control above clears
/// the ceiling by sixteen words, so on its own it would stay RED under `>=` or
/// under an off-by-one — every test in this file would pass while the contract
/// had changed. Every other class here has a paired positive/negative control;
/// this is the size class's.
#[test]
fn a_region_one_word_over_the_pointer_ceiling_is_reported() {
    let violations =
        xref_region_violations(&region_padded_to(MAXIMUM_XREF_WORDS + 1), "synthetic.md");

    assert_eq!(
        violations.len(),
        1,
        "expected exactly the pointer-ceiling violation, got: {violations:#?}"
    );
    assert!(
        violations[0].contains(&MAXIMUM_XREF_WORDS.to_string()),
        "the violation must quote the ceiling it broke: {}",
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

/// An editor note that QUOTES its own terminator is not fully a comment, and
/// the debris that survives the stripper is REPORTED rather than silently
/// charged to the pointer's budget.
///
/// Shaped like the LIVE defect this class was minted for: `stdlib.md` shipped a
/// placement note whose closing sentence quoted the `ORACLE-XREF` marker in
/// full, terminator and all, so the note ended four lines early and two lines of
/// its own text landed in the rendered chunk — where they also ate pointer budget
/// that nothing was spending.
///
/// The class is decidable and wording-blind, which is what makes it worth
/// having: a terminator surviving into stripped prose is definitionally an
/// UNOPENED one, so it can only mean a comment closed earlier than its author
/// intended. Nothing here reads the note's words.
#[test]
fn a_region_whose_editor_note_quotes_its_own_terminator_is_reported_as_debris() {
    let region = "\
<!--
PLACEMENT IS LOAD-BEARING — do not tidy this pointer back into the ops table above.
The region below is guarded by oracle_xref_smoke.rs, matched on the
`<!-- ORACLE-XREF -->` marker line, never on this heading — retitling is free.
-->

Over let-bound geometry: `intersects(a, b) -> Bool` and `distance(a, b) -> Length`.

The posed form and the traps are in the `geometry` chunk — topic `geometry` of
`reify_language_reference`.
";

    let violations = xref_region_violations(region, "synthetic.md");

    assert_eq!(
        violations.len(),
        1,
        "expected exactly the comment-debris violation — the pointer itself is \
         well-formed, so anything else here is a class firing for the wrong \
         reason: {violations:#?}"
    );
    assert!(
        violations[0].contains(HTML_COMMENT_CLOSE),
        "the violation must quote the terminator that survived, because that \
         string is the whole evidence and is what the fixer searches the chunk \
         for: {}",
        violations[0]
    );
}

/// ...and a WELL-FORMED editor note still costs nothing.
///
/// The negative control that stops the class above from degenerating into a ban
/// on editor notes. [`POINTER_SIZED_REGION`] cannot cover this: it carries no
/// comment at all, so it would stay green against a stripper that had started
/// reporting every note. House convention puts placement notes and SYNC blocks
/// inside exactly these marked regions, and this is what keeps that free.
#[test]
fn a_region_with_a_well_formed_editor_note_is_clean() {
    let region = "\
<!--
PLACEMENT IS LOAD-BEARING — do not tidy this pointer back into the ops table above.
The region below is guarded by oracle_xref_smoke.rs, matched on the
`ORACLE-XREF` marker line, never on this heading — retitling is free.
-->

Over let-bound geometry: `intersects(a, b) -> Bool` and `distance(a, b) -> Length`.

The posed form and the traps are in the `geometry` chunk — topic `geometry` of
`reify_language_reference`.
";

    assert_eq!(
        xref_region_violations(region, "synthetic.md"),
        Vec::<String>::new(),
        "a multi-line editor note that never quotes its own terminator is a whole \
         comment: it must be stripped, cost the pointer ceiling nothing, and be \
         scanned for nothing"
    );
}

// ── Scanner unit tests ───────────────────────────────────────────────────────
//
// `strip_html_comments` is this module's only hand-rolled text helper, and every
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

/// A comment whose body QUOTES a terminator ends AT THE QUOTE, and the rest of
/// the author's note survives as visible text.
///
/// The third case, which the well-formed and unterminated cases above left
/// uncovered between them. It is pinned as CORRECT, not fixed: HTML comments do
/// not nest and HTML defines no escape sequence inside one, so the first `-->`
/// closes it. Backticks are MARKDOWN, and markdown is not processed inside a
/// comment — every real renderer does exactly this, and the chunk the assistant
/// is served is the rendered one.
///
/// A bespoke backtick-aware rule here would be strictly worse twice over: it
/// would invent a convention no renderer implements, so this module would
/// disagree with what the reader actually sees; and it would HIDE genuine
/// early-termination defects, which are how an editor note silently leaks into
/// a chunk. Reporting beats rewriting — the debris this leaves is what
/// `a_region_whose_editor_note_quotes_its_own_terminator_is_reported_as_debris`
/// pins as a violation class, which is what turns this property from a silent
/// hazard into a named, actionable one.
#[test]
fn a_quoted_terminator_closes_the_comment_early() {
    assert_eq!(
        strip_html_comments(
            "prose above\n\
             <!--\n\
             The region below is guarded by oracle_xref_smoke.rs, matched on the\n\
             `<!-- ORACLE-XREF -->` marker line — retitling is free.\n\
             -->\n\
             prose below\n"
        ),
        "prose above\n` marker line — retitling is free.\n-->\nprose below\n"
    );
}
