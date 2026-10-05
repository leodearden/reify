//! Truth checks for the "geometry" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/geometry.md`), served to the in-GUI
//! assistant via `reify_language_reference`: what the chunk documents must be
//! what the compiler accepts, so a phantom signature is caught here rather than
//! by a designer typing it into a `.ri` file.
//!
//! Lives in `reify-compiler` (not `reify-mcp`, where `geometry.md` itself
//! lives) because `reify-mcp` does not depend on `reify-compiler` and so
//! cannot invoke `compile_source_with_stdlib` — the same cross-crate
//! placement sibling task 5347 used for its stdlib-chunk smoke test.
//!
//! # Coverage is derived from the chunk
//!
//! No list of the chunk's forms is kept by hand. Each part of geometry.md is read
//! by the gate that owns its notation:
//!
//! - ```` ```reify-schematic ```` signature listings (Prelude, Solid Primitives,
//!   2D profiles, GD&T zones, Free-form) — `schematic_listing_gate.rs` pairs
//!   every listed form with a call in a compile-verified signature fixture.
//! - Signatures in unfenced prose and tables (`extrude`, `revolve`, the queries,
//!   the topology selectors) — `unfenced_signature_gate.rs`, the same way.
//! - ```` ```reify ```` worked examples — `fence_gate.rs` compiles every one
//!   verbatim and holds their count exact;
//!   `geometry_reify_fences_call_every_worked_example_form` requires the forms
//!   that need a worked example to be called in one.
//! - Cited tests and examples — `chunk_cite_gate.rs` resolves every cite;
//!   `geometry_chunk_example_citations_hold_against_the_real_examples` holds the
//!   GD&T section's claim about its cited example.
//!
//! This file owns the rest: the oracle, measurement and topology-selector
//! families checked against the compiler's registries, and their documented
//! arities cross-checked against the worked fences.
//!
//! # What is NOT established
//!
//! THE CANONICAL SCOPE STATEMENT FOR THIS FILE — the test docstrings point back
//! here rather than restating it, so there is exactly one place to correct.
//!
//! - **The compiler checks nothing about the five oracle names.** None has an
//!   arg-slot entry in the private `builtin_signatures` table, so neither arity
//!   nor argument dimension is rejected; and an unknown call NAME is not itself
//!   an error, because a `structure def` body types an unresolved call from its
//!   FIRST argument's `result_type`, a permissive fallback. Compile-acceptance of
//!   a fence is therefore a parse/shape result, not a signature check.
//! - **Arity is pinned anyway — by cross-check, not by the compiler.**
//!   `oracle_signature_arities_match_the_compiling_fences` requires the arities
//!   the `name(…) -> Type` signatures document and the arities the fences call
//!   each name at to be the SAME set, so an arity edit on either side that the
//!   other does not mirror is RED here even though `min_clearance(s)` compiles
//!   clean. Argument DIMENSION
//!   stays unchecked in both directions. (Adopted from the sibling suite's
//!   `every_documented_geometry_op_form_is_exercised_by_the_fixture`.)
//! - **The fence guard's residual power over call NAMES is indirect and narrow.**
//!   Mutating `distance(` → `distanceZZZ(` is caught only downstream, by the
//!   fence's own `constraint gap > 1mm` (`CmpOperandKind`); the same typo on a
//!   name whose result merely feeds `not` produces no error at all.
//!   `bogus_query_name_feeding_a_comparison_is_an_error` is the negative control
//!   pinning the half that works, so it cannot rot away silently.
//!
//! The registry-membership assertions in
//! `interference_oracle_names_documented_in_geometry_chunk` are what close the
//! phantom-NAME direction; the fence compile is a parse/shape guard.
//!
//! # The one doc-FORMAT pin this file does impose
//!
//! Stated here rather than left implicit, because it cuts against the house rule
//! the coverage check in `interference_oracle_names_documented_in_geometry_chunk`
//! invokes (that comment declines to require a leading backtick precisely so the
//! call form may be rewrapped, bolded, or tabulated freely).
//!
//! `oracle_signature_arities_match_the_compiling_fences` and
//! `measurement_signature_arities_match_the_compiling_fences` require each of
//! the five oracle names and the four whole-handle names to carry a signature
//! written as ONE code span holding the whole `name(params) -> Type`, in the
//! section's unfenced prose. **That single span is therefore a PINNED notation
//! for those nine names: an un-backticked signature, one split across spans, or
//! a markdown table with the return type in its own column
//! (`| min_clearance(s, id_a, id_b) | Length |`) is RED even though no
//! capability regressed.** That is a deliberate trade, not an oversight: the
//! `->` is the ONLY thing separating a documented SIGNATURE from the traps
//! subsection's prose mentions of unsupported forms (`min_clearance(a, b)`,
//! `min_clearance(s, id, id)`), which must not be held to the fences. Widening
//! the scan to accept a table cell would re-admit those. If the section is ever
//! tabulated, widen `documented_signature_arities` in the same commit — and
//! re-check that the trap prose still reads as prose.

use reify_test_support::{compile_source_with_stdlib, errors_only};

use crate::callable_registries::{phantom_name_panic, registry_family};
use crate::chunk_cite_gate::{assert_cited_paths_resolve, cited_source_paths};
use crate::chunk_io::{GEOMETRY_CHUNK_PATH as CHUNK_PATH, read_chunk, repo_root, report};
use crate::chunk_markdown::{
    catalogue_table_names, catalogue_table_rows, marker_closed_region, section_body,
};
use crate::doc_forms::{
    Arity, call_forms, callee_names, documented_unfenced_forms, fence_call_forms,
};

// --- Interference & clearance oracle: chunk <-> compiler-registry guard ---
//
// Task 5389. The five static interference/clearance query names were entirely
// absent from every `reify_language_reference` chunk, so the in-GUI assistant
// (which retrieves only those chunks) concluded "there is no interference
// oracle" and hand-rolled bbox arithmetic instead. The knowledge existed in the
// examples corpus (`examples/best_practices/clearance_oracle.ri`,
// `examples/tolerancing/vc_bolt_pattern_clearance.ri`) but was unreachable from
// the chunks.
//
// This guard is BIDIRECTIONAL, and neither half is sufficient alone:
//
//   (a) COVERAGE — geometry.md still documents all five names as call forms.
//       Catches a doc regression that silently reopens the discoverability hole.
//   (b) REGISTRY TRUTH — each documented name is a live member of the compiler
//       registry that gives it its meaning, so a rename in `units.rs` fails HERE
//       rather than leaving the chunk pointing at a phantom builtin.
//
// Per the house rule stated in `stdlib_chunk_geometry_ops_smoke.rs`'s module
// doc ("a name-existence check against code registries, deliberately NOT a
// wording/content pin on the chunk's prose") nothing below asserts on prose,
// headings, or ordering. That rule has NO carve-out here: the coverage scan is
// scoped by a dedicated HTML-comment marker the chunk carries for this purpose
// (see `ORACLE_SECTION_MARKER`), never by the section's title, so retitling
// `## Interference & Clearance Queries` is free.

/// Marker that OPENS the chunk section the coverage scan is scoped to. Matched
/// BYTE-EXACTLY against the trimmed line; the chunk carries it directly under
/// the section heading for exactly this purpose.
///
/// Scoping matters: without it the scan is satisfied by ANY backticked mention
/// of a name anywhere in the 280-line chunk, so deleting the whole oracle
/// section would still pass on incidental hits elsewhere. The regression this
/// guards (the in-GUI assistant not discovering the oracle at all) is about the
/// SECTION existing, so the section is what gets scanned.
///
/// A MARKER RATHER THAN THE HEADING TEXT, deliberately. Scoping by the heading —
/// what `stdlib_chunk_geometry_ops_smoke.rs`'s `CHUNK_SECTION` still does — makes
/// the scan a wording pin on shipped prose: `&` → `and`, reordering the nouns, or
/// dropping a word all go RED with a panic claiming the oracle is undocumented
/// when it plainly is. That is the one thing the house rule in this file's
/// preamble forbids. An inert HTML comment costs the chunk one line, is invisible
/// in rendered markdown, and leaves the title free to change.
///
/// That retitling freedom is load-bearing beyond this file: task 6258's pointers
/// in `constraints.md` and `stdlib.md` name the retrieval TOPIC rather than this
/// section's heading precisely because the heading may change.
const ORACLE_SECTION_MARKER: &str = "<!-- ORACLE-SECTION -->";

/// Human-readable name of the marked section. Used ONLY in panic text, so a
/// reader is told which part of the chunk to look at; nothing matches on it.
/// A retitle may update this for legibility but need not — no test reads it.
const ORACLE_SECTION_TITLE: &str = "## Interference & Clearance Queries";

/// Marker that OPENS the section cataloguing WHICH ARGUMENT of which geometry
/// constructor is length-semantic. Matched BYTE-EXACTLY on the trimmed line,
/// exactly as [`ORACLE_SECTION_MARKER`] is, and for the identical reason: the
/// scan must be anchored to something inert so the heading's wording stays free.
///
/// Scoping matters here for a second reason too. This chunk mentions geometry
/// call forms everywhere — the primitives block, the anchoring table, the oracle
/// fences — so an UNSCOPED name scan would be satisfied by any of them and would
/// say nothing about whether the length-argument catalogue still exists. The
/// catalogue is what an author consults before dimensioning an unfamiliar
/// signature, so the catalogue is what gets scanned.
const LENGTH_ARGS_SECTION_MARKER: &str = "<!-- LENGTH-ARGS-SECTION -->";

/// Human-readable name of [`LENGTH_ARGS_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const LENGTH_ARGS_SECTION_TITLE: &str = "### Dimensioned arguments";

/// Marker that OPENS the MEASUREMENT / mass-property section — the one that
/// documents [`reify_compiler::GEOMETRY_QUERY_NAMES`] as call forms. Matched
/// BYTE-EXACTLY on the trimmed line, exactly as [`ORACLE_SECTION_MARKER`] is,
/// and for the identical reason: the anchor must be inert so the heading's
/// wording stays free to change.
///
/// Scoping matters here more than anywhere else in this file, because the names
/// in this family are the ones a chunk-wide scan cannot distinguish. `volume`
/// and `area` are ordinary English words that already appeared in this repo's
/// chunks as HAND-COMPUTED parameter arithmetic (`structures.md`'s
/// `let volume = thickness * width * width`), and `contains` is also the
/// `List`/`Set`/`Range` method documented in the `collections` chunk. An
/// unscoped word scan is satisfied by every one of those while teaching a reader
/// nothing about the kernel query — which is precisely the misdirection task
/// 5581 exists to remove, so the SECTION is what gets scanned.
const MEASUREMENT_SECTION_MARKER: &str = "<!-- MEASUREMENT-SECTION -->";

/// Human-readable name of [`MEASUREMENT_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const MEASUREMENT_SECTION_TITLE: &str = "## Measurement & Mass-Property Queries";

/// Marker that OPENS the `undef`-TRAP region inside the measurement section —
/// the three traps (arg shape, binder scope, no OCCT) that explain why a query
/// silently yields `Value::Undef`. Matched BYTE-EXACTLY on the trimmed line,
/// exactly as every other marker here is, and for the identical reason: the
/// anchor must be inert so the heading's wording stays free.
///
/// This is the one marker scoping a region for a FORBIDDEN direction rather than
/// a required one, and the scoping is what makes that safe. The chunk writes
/// `volume(...)` legitimately all over the measurement section — the signature
/// list, the worked fence, the fence's own hoist annotation — so a chunk-wide
/// "no hoisted call form" scan would be RED against entirely correct prose. Only
/// these three traps claim that an inline geometry argument yields `undef`, so
/// only they are scanned.
///
/// PAIRED WITH [`NOT_HOISTED_TRAP_END_MARKER`], and read through
/// [`marker_closed_region`] rather than [`section_body`], because "to the next
/// `##` heading" is NOT the extent this scan wants. `section_body` does not stop
/// at a `### ` heading and has no end-marker notion, so the unclosed region ran
/// past the traps and swallowed the section's closing **Worked reference**
/// paragraph — text that is not about `undef` at all, where a perfectly correct
/// future sentence ("it calls `volume(part)` on a filleted body") would have
/// gone RED. The closing marker is what makes the docstring above a description
/// of the real extent instead of an aspiration.
const NOT_HOISTED_TRAP_MARKER: &str = "<!-- NOT-HOISTED-TRAP -->";

/// Marker that CLOSES [`NOT_HOISTED_TRAP_MARKER`]'s region, immediately after the
/// third trap. Matched byte-exactly on the trimmed line, and its ABSENCE panics
/// — see [`marker_closed_region`].
const NOT_HOISTED_TRAP_END_MARKER: &str = "<!-- /NOT-HOISTED-TRAP -->";

/// Human-readable name of [`NOT_HOISTED_TRAP_MARKER`]'s region. Panic text only;
/// nothing matches on it.
const NOT_HOISTED_TRAP_TITLE: &str = "### Eval status, and when a query yields `undef`";

/// Marker that OPENS the TOPOLOGY-SELECTOR catalogue section — the one that
/// documents [`reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`] as a table.
/// Matched BYTE-EXACTLY on the trimmed line, as every other marker here is.
///
/// Scoping is what makes the scan mean anything for THIS family in particular:
/// `edges` and `faces` already appear all over this chunk as the fillet / chamfer
/// / shell ARGUMENT name (`fillet(solid, edges, radius)`), where they say nothing
/// about the selector that produces such a list. The catalogue is the one place
/// they are documented as callable selectors, so the catalogue is what is
/// scanned.
const TOPOLOGY_SECTION_MARKER: &str = "<!-- TOPOLOGY-SECTION -->";

/// Human-readable name of [`TOPOLOGY_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const TOPOLOGY_SECTION_TITLE: &str = "## Topology Selectors";

/// FORM A oracle names — dispatched through the kinematic-query post-process
/// and taking `(Snapshot, ...)` arguments. `is_geometry_kinematic_query` is a
/// bare `.contains()` over `GEOMETRY_KINEMATIC_QUERY_NAMES`, so membership in
/// that slice IS the compiler's recognition semantics.
///
/// EXHAUSTIVE, not a subset: this list must equal
/// `GEOMETRY_KINEMATIC_QUERY_NAMES` entry-for-entry, and
/// `documented_oracle_list_covers_the_whole_kinematic_registry` enforces that.
/// The whole kinematic family is interference/clearance, so any member of it
/// that the chunk does not document reopens exactly the discoverability hole
/// task 5389 closed.
const KINEMATIC_ORACLE_NAMES: &[&str] = &["interferes", "interferes_with", "min_clearance"];

/// FORM B oracle names — plain geometry queries over let-bound geometry
/// operands. `is_geometry_query` is likewise a bare `.contains()` over
/// `GEOMETRY_QUERY_NAMES`.
///
/// A DELIBERATE SUBSET, unlike [`KINEMATIC_ORACLE_NAMES`], and therefore NOT
/// set-equality-checked. `GEOMETRY_QUERY_NAMES` is a much larger and
/// heterogeneous family (`volume`, `area`, `centroid`, `bounding_box`,
/// `normal`, `feature`, …); only these two answer an interference/clearance
/// question, so only these two belong in the oracle section. Names outside this
/// pair are documented elsewhere in the chunk corpus and are the sibling
/// `stdlib_chunk_geometry_ops_smoke.rs`'s coverage concern, not this file's.
///
/// `pub(crate)` since task 6258, and read by TWO suites rather than one.
/// `oracle_xref_smoke.rs` requires `constraints.md`'s and `stdlib.md`'s pointer
/// regions to name every entry here as a call form, while
/// [`interference_oracle_names_documented_in_geometry_chunk`] requires the
/// destination section to document the same entries — so ONE edit here retires a
/// form from both sides at once, and neither can be left pointing at a name the
/// other dropped.
pub(crate) const GEOMETRY_ORACLE_NAMES: &[&str] = &["intersects", "distance"];

/// A kinematic query added to the compiler but never documented must be RED.
///
/// The bidirectional guard below covers doc → registry (no phantom names) and
/// documented-name → chunk (no silent doc deletion), but neither direction sees
/// a FOURTH kinematic query landing in `units.rs` and never reaching the chunk —
/// which is the discoverability regression task 5389 exists to prevent, arriving
/// from the compiler side instead of the doc side. Set equality closes it: a new
/// registry entry fails here until it is added to `KINEMATIC_ORACLE_NAMES`, and
/// adding it there immediately fails the coverage half until the chunk documents
/// it. Mirrors the sibling suite's
/// `every_implemented_geometry_op_is_documented_in_a_chunk`.
///
/// SET equality, not sequence equality — both sides are sorted before comparing.
/// Order in `GEOMETRY_KINEMATIC_QUERY_NAMES` carries no meaning
/// (`is_geometry_kinematic_query` is a bare `.contains()` over it), so a no-op
/// reordering there — alphabetising it, say — must not fail a test whose whole
/// subject is which names EXIST. Order-sensitivity here would be a false
/// positive whose message actively misdirects, telling the reader to add or
/// document a name when nothing was added or removed.
#[test]
fn documented_oracle_list_covers_the_whole_kinematic_registry() {
    let mut documented = KINEMATIC_ORACLE_NAMES.to_vec();
    let mut registry = reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES.to_vec();
    documented.sort_unstable();
    registry.sort_unstable();
    assert_eq!(
        documented, registry,
        "KINEMATIC_ORACLE_NAMES must list the kinematic-query registry exactly (as a SET — both \
         sides are sorted here, so a pure reordering is fine). A query was added to (or removed \
         from) reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES without updating this list — so \
         {CHUNK_PATH}'s `{ORACLE_SECTION_TITLE}` section is not required to document it, and the in-GUI \
         assistant will keep reading it as a MISSING CAPABILITY (task 5389). Add the name here \
         AND document its call form in the chunk."
    );
}

#[test]
fn interference_oracle_names_documented_in_geometry_chunk() {
    let markdown = read_chunk(CHUNK_PATH);
    // Scoped to the oracle section, NOT the whole file: a backticked `distance(`
    // elsewhere in the chunk (or an incidental mention that survives the
    // section's deletion) must not satisfy this. `section_body` panics if the
    // section is gone, so gutting it is RED rather than vacuously green.
    let section = section_body(
        &markdown,
        ORACLE_SECTION_MARKER,
        CHUNK_PATH,
        ORACLE_SECTION_TITLE,
    );

    // (a) COVERAGE. Each name must appear as a CALL form (`name(`) rather than a
    // bare word — geometry.md already contained the word "distance" before task
    // 5389, but only as the unrelated `extrude(profile, distance)` parameter
    // name, which taught a reader nothing about the query. The open paren is what
    // excludes that false positive (`extrude(profile, distance)` has no
    // `distance(` in it), and it is all this needle asks for.
    //
    // NO leading backtick is required, deliberately. Demanding one would pin doc
    // TYPOGRAPHY: writing the same call form as `**`min_clearance`**(s, …)`, or
    // moving it into a markdown table cell, would go RED with zero capability
    // regression and a panic claiming the oracle is undocumented. Per the house
    // rule this file inherits, prose formatting is not the subject. (The ONE
    // exception, `-> <Type>`, is stated in the module doc's "The one doc-FORMAT
    // pin this file does impose" — read it before tabulating this section.) The real
    // weight is carried by (b) below, by `section_body`'s anti-vacuity panic, and
    // by `geometry_reify_fences_call_every_worked_example_form`'s per-name
    // sentinels, which require each call form inside a COMPILING fence — a strictly stronger
    // property than any string match here.
    //
    // (Placement WITHIN the section is not further constrained: a name mentioned
    // only in the traps subsection would still satisfy this. Section presence is
    // the property under test.)
    for name in KINEMATIC_ORACLE_NAMES.iter().chain(GEOMETRY_ORACLE_NAMES) {
        let call_form = format!("{name}(");
        assert!(
            section.contains(&call_form),
            "{CHUNK_PATH}'s `{ORACLE_SECTION_TITLE}` section does not document the \
             interference/clearance query `{name}` as a call form ({call_form}...). The chunk \
             is what the in-GUI assistant retrieves, so an undocumented oracle reads to it as a \
             MISSING CAPABILITY and it will hand-roll bbox arithmetic instead (task 5389). \
             Re-add the call form, or delete this name from \
             KINEMATIC_ORACLE_NAMES/GEOMETRY_ORACLE_NAMES if the builtin itself is gone."
        );
    }

    // (b) REGISTRY TRUTH, kinematic trio.
    for name in KINEMATIC_ORACLE_NAMES {
        assert!(
            reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
            "`{name}` is documented in {CHUNK_PATH} as a kinematic interference/clearance \
             query but is NOT in reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES ({:?}) — the \
             chunk is served verbatim to the assistant, so it now documents a phantom \
             builtin. Rename the doc's call form to match the registry.",
            reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES
        );
    }

    // (b) REGISTRY TRUTH, plain-geometry pair.
    for name in GEOMETRY_ORACLE_NAMES {
        assert!(
            reify_compiler::GEOMETRY_QUERY_NAMES.contains(name),
            "`{name}` is documented in {CHUNK_PATH} as a geometry query but is NOT in \
             reify_compiler::GEOMETRY_QUERY_NAMES ({:?}) — the chunk now documents a phantom \
             builtin. Rename the doc's call form to match the registry.",
            reify_compiler::GEOMETRY_QUERY_NAMES
        );
    }
}

/// Every member of the geometry-QUERY registry must be documented as a call form
/// in the chunk's measurement section.
///
/// THE REGISTRY IS ITERATED DIRECTLY, not mirrored into a local list plus a
/// set-equality guard the way [`KINEMATIC_ORACLE_NAMES`] is. That two-test shape
/// exists so a failure can distinguish "the registry grew" from "the doc
/// shrank", and at three names it is cheap. At fifteen — and at the thirty-one of
/// `topology_selector_family_documented_in_geometry_chunk` — the mirror becomes
/// its own drift surface: a hand-copied list that must be edited in lockstep with
/// the registry it claims to reproduce. Iterating the registry collapses both
/// halves into one check with strictly less to maintain, and keeps the
/// distinguishing power where it is actually read: the panic message below names
/// the offending registry member and both remedies.
///
/// A CALL FORM (`name(`) rather than a bare word, and that needle is doing real
/// work here rather than mirroring a convention. `volume`, `area`, `centroid` and
/// `contains` all already appear as bare words elsewhere in the chunk corpus —
/// `volume` as `structures.md`'s hand-computed `thickness * width * width`,
/// `contains` as the `List`/`Set`/`Range` method in the `collections` chunk — so
/// a word-boundary scan (which is exactly what the out-of-band PDOCCOVER detector
/// in `crates/reify-audit/src/pdoccover.rs` performs) reports them DOCUMENTED
/// while a reader learns nothing about the kernel query. The open paren is what
/// separates a query from a noun.
///
/// No leading backtick is required, for the reason
/// `interference_oracle_names_documented_in_geometry_chunk` states at length: the
/// house rule this file inherits forbids pinning doc TYPOGRAPHY. The one
/// exception — the `-> <Type>` notation — is imposed only on the whole-handle
/// four, by `measurement_signature_arities_match_the_compiling_fences`.
///
/// Anti-vacuity comes free from [`section_body`], which panics when its marker is
/// absent, so deleting the section is RED rather than silently green.
#[test]
fn measurement_query_family_documented_in_geometry_chunk() {
    let markdown = read_chunk(CHUNK_PATH);
    let section = section_body(
        &markdown,
        MEASUREMENT_SECTION_MARKER,
        CHUNK_PATH,
        MEASUREMENT_SECTION_TITLE,
    );

    for name in reify_compiler::GEOMETRY_QUERY_NAMES {
        let call_form = format!("{name}(");
        assert!(
            section.contains(&call_form),
            "{CHUNK_PATH}'s `{MEASUREMENT_SECTION_TITLE}` section does not document the geometry \
             query `{name}` as a call form ({call_form}...). The chunk is what the in-GUI \
             assistant retrieves, so an undocumented query reads to it as a MISSING CAPABILITY: \
             asked for a part's mass or its centre of area it will hand-compute the figure from \
             the parameters instead of asking the kernel, and silently return a number that no \
             longer describes the realized geometry once a feature is added (task 5581). Either \
             document the call form in that section, or — if the builtin itself is gone — remove \
             `{name}` from reify_compiler::GEOMETRY_QUERY_NAMES, which is iterated directly here \
             and is the sole source of this list."
        );
    }
}

/// The `undef` traps must illustrate themselves with a query the compile-time
/// inline-arg hoist does NOT cover.
///
/// A registry-driven CONSISTENCY guard, not a prose pin: nothing here asserts on
/// wording. The only claim is that whatever call the trap region exhibits is
/// drawn from OUTSIDE `reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES`, which
/// is iterated directly (no local mirror) so a name entering or leaving the
/// hoisted set moves this check with it.
///
/// THE DEFECT THIS CLOSES was live in this chunk. The arg-shape trap illustrated
/// "an inline geometry argument yields `undef`" with
/// `volume(box(10mm, 10mm, 10mm))` — but `volume` is one of the four names the
/// very next sentence exempts, and the worked fence twenty lines above says that
/// same inline form WORKS. So the section contradicted itself twice over, and the
/// chunk is served VERBATIM to the in-GUI assistant: a trap in this shape does not
/// merely confuse, it asserts a behaviour the compiler does not have, and the
/// assistant then rewrites working code to dodge an imaginary hazard.
///
/// THE CALL FORM is the needle, deliberately. The carve-out sentence names all
/// four as bare backticked identifiers (`` `volume` ``, `` `area` ``, …) and must
/// stay exactly as it is — naming them is the carve-out's whole job. Only a call
/// form `name(` makes the false claim, so only a call form is forbidden.
///
/// SCOPED TO THE THREE TRAPS, and closed at both ends by
/// [`marker_closed_region`]. The region is exactly what claims that an inline
/// geometry argument yields `undef`; the section's closing **Worked reference**
/// paragraph sits outside it deliberately, because a correct sentence there
/// naming `volume(part)` is not this test's business.
///
/// ANTI-VACUITY IS EXPLICIT here rather than inherited. The two markers'
/// panic-on-absent covers a DELETED region, but a forbidden-direction assertion
/// is trivially satisfied by an EMPTY one, so gutting the illustrative call
/// would otherwise go green. The floor therefore requires at least one surviving
/// call form drawn from `GEOMETRY_QUERY_NAMES` MINUS the whole-handle four —
/// computed from the two registries rather than hardcoded, so swapping
/// `perimeter` for `max_deviation` in the prose keeps this correct.
#[test]
fn the_undef_trap_example_is_a_query_the_hoist_does_not_cover() {
    let markdown = read_chunk(CHUNK_PATH);
    let region = marker_closed_region(
        &markdown,
        NOT_HOISTED_TRAP_MARKER,
        NOT_HOISTED_TRAP_END_MARKER,
        CHUNK_PATH,
        NOT_HOISTED_TRAP_TITLE,
    );

    for name in reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES {
        let call_form = format!("{name}(");
        assert!(
            !region.contains(&call_form),
            "{CHUNK_PATH}'s `{NOT_HOISTED_TRAP_TITLE}` region exhibits `{call_form}…` as its \
             example of a call that yields `undef`, but `{name}` is one of \
             reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES: the task-5345 inline-arg hoist \
             (crates/reify-compiler/src/units.rs:937, pinned by \
             geometry_query_inline_arg_tests.rs::compile_inline_volume_torus_hoists_into_realization) \
             desugars an inline geometry argument to `{name}` into a synthetic let, so that call \
             resolves. The example therefore contradicts the very carve-out the next sentence \
             grants — and this chunk is served verbatim to the in-GUI assistant, which would read \
             it as a behaviour the compiler does not have and steer designers away from a form \
             that works. Illustrate the trap with a query the hoist does not cover (any member of \
             GEOMETRY_QUERY_NAMES outside that four — `perimeter`, `curvature`, `normal`, \
             `length`, `feature`, or any of the multi-arg queries). Naming `{name}` as a bare \
             backticked identifier in the carve-out sentence is fine and untouched by this check; \
             only the call form is forbidden."
        );
    }

    let not_hoisted: Vec<&str> = reify_compiler::GEOMETRY_QUERY_NAMES
        .iter()
        .copied()
        .filter(|name| !reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES.contains(name))
        .collect();
    assert!(
        not_hoisted
            .iter()
            .any(|name| region.contains(&format!("{name}("))),
        "{CHUNK_PATH}'s `{NOT_HOISTED_TRAP_TITLE}` region no longer exhibits ANY query call form \
         at all, so the forbidden-direction check above passes vacuously — an empty region \
         satisfies it exactly as well as a correct one does. The region's job is to SHOW a call \
         that silently yields `undef`; a trap that names no call teaches nothing. Restore an \
         illustrative call drawn from one of {not_hoisted:?} (GEOMETRY_QUERY_NAMES minus the \
         hoisted whole-handle four, computed here from both registries so this list follows the \
         compiler)."
    );
}

/// Minimum CATALOGUE ROWS the TOPOLOGY-SELECTOR table must carry.
///
/// The EXACT live length of `reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`
/// (31 today), not a round number under it. The coverage half of
/// [`topology_selector_family_documented_in_geometry_chunk`] cannot catch a
/// gutted table on its own — reformatting the table into prose bullets empties
/// [`catalogue_table_rows`] and the coverage loop then compares against an empty
/// set. The floor is what makes that RED.
///
/// A DERIVED value would be better and is deliberately not used: asserting
/// `rows.len() >= GEOMETRY_TOPOLOGY_SELECTOR_NAMES.len()` reads as tighter but is
/// strictly weaker as a floor, because a row documenting one selector under two
/// names (the shared-cell shape `catalogue_table_rows` supports) legitimately
/// makes rows FEWER than names. Keeping the number literal keeps the two claims
/// independent, which is the point of having both.
///
/// RE-MEASUREMENT PROTOCOL: see [`MINIMUM_FN_CITES`], which states it once for
/// every `MINIMUM_*` floor in this file. Raise this WITH the table; never lower
/// it to go green.
const MINIMUM_TOPOLOGY_CATALOGUE_ROWS: usize = 31;

/// Every member of the topology-selector registry must have a CATALOGUE ROW, and
/// every catalogue row must name a real selector.
///
/// BOTH DIRECTIONS, because each catches a different rot. Coverage (registry →
/// table) catches a selector landing in `units.rs` that the chunk never learns
/// about — the in-GUI assistant then cannot reach it and falls back to indexing
/// `faces(...)` by hand. Registry truth (table → registry) catches the failure
/// that has actually happened in this repo: the 2026-07-24 language review found
/// `rotate(geo, axis, angle)` and `translate(geo, vector)` documented at
/// signatures the compiler had never been shown (tasks #5347 / #5364). A phantom
/// selector is worse than a missing one, because the reader has no reason to
/// doubt it.
///
/// A TABLE rather than the measurement section's call-form prose, and that is a
/// scanner decision rather than a style one: [`catalogue_table_rows`] already
/// exists to read exactly this shape (it backs the length-argument catalogue),
/// and a 31-entry family rendered as prose is unreadable for the human as well as
/// unscannable for the test. The registry is iterated DIRECTLY here for the same
/// reason `measurement_query_family_documented_in_geometry_chunk` iterates its
/// own: at 31 names a hand-copied mirror is a bigger drift surface than the thing
/// it guards.
#[test]
fn topology_selector_family_documented_in_geometry_chunk() {
    let markdown = read_chunk(CHUNK_PATH);
    // Panics if the marker is gone, so deleting the section is RED rather than
    // vacuously green — the same anti-vacuity guarantee the oracle scan relies on.
    let section = section_body(
        &markdown,
        TOPOLOGY_SECTION_MARKER,
        CHUNK_PATH,
        TOPOLOGY_SECTION_TITLE,
    );

    let rows = catalogue_table_rows(&section);
    assert!(
        rows.len() >= MINIMUM_TOPOLOGY_CATALOGUE_ROWS,
        "only {} catalogue row(s) found in {CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` table — \
         expected at least {MINIMUM_TOPOLOGY_CATALOGUE_ROWS}. Either rows were deleted, or the \
         table was reformatted into a shape this scan cannot read: a catalogue row is a \
         `|`-leading line whose FIRST cell backticks the selector it is about. Without the rows \
         the coverage check below compares against an empty set and protects nothing. Rows seen: \
         {rows:?}",
        rows.len()
    );

    let table_names = catalogue_table_names(&rows);

    // (a) COVERAGE — registry → table.
    for name in reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES {
        assert!(
            table_names.iter().any(|n| n == name),
            "{CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` catalogue has no row for the topology \
             selector `{name}`. The chunk is what the in-GUI assistant retrieves, so a selector \
             missing from this table reads to it as a MISSING CAPABILITY: it will index \
             `faces(...)` positionally, or hand-roll a filter, instead of calling the selector \
             that exists (task 5581). Add a row whose FIRST cell backticks `{name}`, or — if the \
             builtin itself is gone — remove it from \
             reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES, which is iterated directly here \
             and is the sole source of this list. Names the table does carry: {table_names:?}"
        );
    }

    // (b) REGISTRY TRUTH — table → registry. A row naming something that is not
    // a selector sends an author to write a call the compiler will not accept,
    // and — because a `structure def` body types an unresolved call from its
    // first argument — often will not even say so.
    for name in &table_names {
        if reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name.as_str()) {
            continue;
        }
        // Split by WHY it is not a selector, so the panic names the actual
        // remedy. `phantom_name_panic` is the shared voice for "no registry has
        // this name at all"; a name that IS real but lives in a sibling family
        // is a different (and more confusing) mistake, and gets told so.
        match registry_family(name) {
            None => panic!(
                "{}",
                phantom_name_panic(
                    CHUNK_PATH,
                    "the topology-selector catalogue table's first column",
                    name
                )
            ),
            Some(family) => panic!(
                "{CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` catalogue has a row for `{name}`, \
                 which is a real builtin but belongs to {family}, NOT \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES. The families are disjoint by construction and \
                 dispatch differently, so documenting one here teaches a reader the wrong \
                 result type and the wrong resolution stage. Move the row to the section that \
                 owns {family}, or drop it."
            ),
        }
    }
}

/// Every form the chunk teaches by worked example — the query family, and the
/// constructors whose argument shapes prose alone cannot convey — is CALLED,
/// in parsed code, by one of geometry.md's ```` ```reify ```` fences.
///
/// That is what makes each form compile-verified: the fence gate
/// (`fence_gate.rs::every_reify_tagged_fence_compiles_clean`) compiles every
/// bare ```` ```reify ```` fence of every chunk VERBATIM, and its
/// `REIFY_FENCE_FLOORS` entry for geometry.md, held EXACT, is the one floor on
/// how many there are. This test owns only the per-name half: a fence that
/// silently stops calling a documented form would leave that gate green. The
/// sentinel set is its own anti-vacuity.
///
/// SCOPE — this is a call-presence guard, NOT a signature pin. The module doc's
/// "What is NOT established" is the canonical statement of what compile
/// acceptance does and does not buy (arity, argument dimension, unknown call
/// names); read it before relying on this test.
#[test]
fn geometry_reify_fences_call_every_worked_example_form() {
    let markdown = read_chunk(CHUNK_PATH);

    // ALL FIVE oracle names, so coverage is symmetric. Before task 5389's
    // amendment pass, `interferes(`/`interferes_with(` appeared only in the FORM A
    // bullet list, so their sole guard was a `section.contains(...)` string match —
    // which the module doc characterises as establishing essentially nothing. They
    // are now bound in the FORM A fence and sentinelled here like the rest.
    //
    // `distance(` is included even though the module doc singles it out as the
    // only name with (indirect) discriminating power — precisely BECAUSE of
    // that: without a sentinel the FORM B fence could lose its `distance(` call
    // and this check would still pass, quietly retiring the one claim
    // `bogus_query_name_feeding_a_comparison_is_an_error` is the control for.
    //
    // Read through the PARSER, so only a call the compiler sees counts. The FORM
    // A fence's own `// MUST be let-bound. Writing `constraint min_clearance(s,
    // id_a, id_b) > 2mm`` annotation is a comment, never a call form, so
    // deleting the fence's real call goes RED here rather than leaving the
    // annotation to stand in for it; a string literal holding call text is no
    // call either. The match is by exact callee name, so `area` is never
    // satisfied by a `surface_area` call.
    //
    // The whole-handle measurement four join the list for a reason specific to
    // them (task 5581): `volume`, `area` and `centroid` are the names this chunk
    // corpus previously carried only as HAND-COMPUTED parameter arithmetic
    // (`structures.md`'s `let volume = thickness * width * width`), so a
    // documented call form that the compiler rejects would be indistinguishable,
    // to a reader, from the arithmetic it is meant to replace. Requiring each
    // inside a COMPILING fence is what makes the replacement credible.
    //
    // The three constructors are the ones whose argument SHAPES the chunk
    // teaches by worked example because a signature listing cannot convey them
    // (task #5926): `half_space`'s unbounded result intersected back to a
    // bounded solid, `nurbs_surface`'s nested control net beside flat knot
    // vectors, and `nurbs`'s bare-number and Length slots side by side.
    let called = callee_names(&fence_call_forms(&markdown, CHUNK_PATH));
    for sentinel in [
        "min_clearance",
        "interferes",
        "interferes_with",
        "intersects",
        "distance",
        "volume",
        "area",
        "centroid",
        "bounding_box",
        "half_space",
        "nurbs_surface",
        "nurbs",
    ] {
        assert!(
            called.iter().any(|name| name == sentinel),
            "anti-vacuity: no ```reify fence in {CHUNK_PATH} calls `{sentinel}` — the worked \
             examples no longer compile-verify that form, so a documented call form the compiler \
             outright rejects would ship unnoticed. (A call form mentioned only in a fence's `//` \
             annotation or a string literal does not count; it is never compiled.) Fence call \
             names: {called:?}"
        );
    }
}

/// The arities `name` is DOCUMENTED at in `section`, read off its
/// `name(<args>) -> <Type>` signatures: code spans of the section's UNFENCED
/// prose that [`doc_form_of_span`](crate::doc_forms::doc_form_of_span) reads as
/// signature-shaped whole. Fence bodies (the fence side's jurisdiction) and HTML
/// maintainer notes are never read.
///
/// The `->` is what separates a SIGNATURE from a mere mention, and the
/// distinction is load-bearing: the traps subsection deliberately writes
/// `min_clearance(a, b)` (the unsupported 2-arg overload) and
/// `min_clearance(s, id, id)` (the self-pair rider) as prose. Neither is a
/// contract the fences should be held to.
///
/// PANICS on unreadable markup, and on a variadic signature, which has no fixed
/// arity for the fences to mirror.
fn documented_signature_arities(section: &str, name: &str) -> Vec<usize> {
    documented_unfenced_forms(section)
        .unwrap_or_else(|e| panic!("{CHUNK_PATH}: {e} — so no signature in it can be read"))
        .into_iter()
        .filter(|documented| documented.form.name == name && documented.span.contains("->"))
        .map(|documented| match documented.form.arity {
            Arity::Exact(arity) => arity,
            Arity::AtLeast(_) => panic!(
                "{CHUNK_PATH}: `{}` is a variadic signature, which has no fixed arity for the \
                 ```reify fences to mirror — document `{name}` at each arity a fence calls it at",
                documented.span
            ),
        })
        .collect()
}

/// The arities the compiling fences call `name` at, out of their
/// [`fence_call_forms`].
fn fence_arities(fence_forms: &[(String, usize)], name: &str) -> Vec<usize> {
    fence_forms
        .iter()
        .filter(|(callee, _)| callee == name)
        .map(|(_, arity)| *arity)
        .collect()
}

/// Every way the arities `name` is `documented` at and the arities the
/// compiling fences call it at (`exercised`) have drifted apart, in either
/// direction, one actionable line each.
fn arity_drift(name: &str, documented: &[usize], exercised: &[usize]) -> Vec<String> {
    const FIX: &str = "The fences are what actually compile, so fix whichever of the two is \
                       wrong — a designer copies whichever they read first.";
    let distinct = |arities: &[usize]| {
        let mut arities = arities.to_vec();
        arities.sort_unstable();
        arities.dedup();
        arities
    };
    let documented = distinct(documented);
    let exercised = distinct(exercised);

    let mut drift = Vec::new();
    for arity in documented.iter().filter(|arity| !exercised.contains(arity)) {
        drift.push(format!(
            "{CHUNK_PATH} documents `{name}` at {arity} argument(s), but no ```reify fence calls \
             it at that arity (fence call arities: {exercised:?}). Either the documented \
             signature is a phantom the compiler was never shown, or a fence drifted off the form \
             it demonstrates. {FIX}"
        ));
    }
    for arity in exercised.iter().filter(|arity| !documented.contains(arity)) {
        drift.push(format!(
            "a ```reify fence in {CHUNK_PATH} calls `{name}` at {arity} argument(s), but no \
             `{name}(…) -> <Type>` signature documents that arity (documented arities: \
             {documented:?}). Either the fence demonstrates a form the section never states, or \
             the section lost the signature. {FIX}"
        ));
    }
    drift
}

/// The oracle section's documented signature arities and the arities its
/// compiling fences call those names at must be the SAME set: a documented arity
/// no fence exercises and a fence arity no signature documents are both drift,
/// reported together, one line per drifted form.
///
/// This is the doc↔fence half of the arity story; the compiler half does not
/// exist (see the module doc). Adopted from the sibling suite's
/// `every_documented_geometry_op_form_is_exercised_by_the_fixture`, which pairs
/// documented (name, arity) forms against fixture calls for exactly this reason:
/// a documented signature and the worked example that is supposed to demonstrate
/// it must not be able to drift apart, since a designer copies whichever one they
/// read first.
#[test]
fn oracle_signature_arities_match_the_compiling_fences() {
    let markdown = read_chunk(CHUNK_PATH);
    let section = section_body(
        &markdown,
        ORACLE_SECTION_MARKER,
        CHUNK_PATH,
        ORACLE_SECTION_TITLE,
    );
    let fence_forms = fence_call_forms(&markdown, CHUNK_PATH);

    let mut drift = Vec::new();
    for name in KINEMATIC_ORACLE_NAMES.iter().chain(GEOMETRY_ORACLE_NAMES) {
        let documented = documented_signature_arities(&section, name);
        // Anti-vacuity. A signature form that loses its `-> <Type>` annotation,
        // or stops being one whole code span, would otherwise drop out of this
        // check silently instead of failing it.
        assert!(
            !documented.is_empty(),
            "{CHUNK_PATH}'s `{ORACLE_SECTION_TITLE}` section documents no `{name}(…) -> <Type>` \
             signature form, so nothing pins that name's arity and this check would pass \
             vacuously for it. Restore the signature as ONE backticked `{name}(…) -> <Type>` \
             code span in the section's prose, return annotation included."
        );

        let in_fences = fence_arities(&fence_forms, name);

        drift.extend(arity_drift(name, &documented, &in_fences));
    }
    report(
        &format!(
            "`{ORACLE_SECTION_TITLE}` signature arities that drifted from the compiling ```reify \
             fences"
        ),
        &drift,
    );
}

/// The MEASUREMENT section's documented whole-handle signature arities and the
/// arities its compiling fences call those names at must be the SAME set, drift
/// in either direction reported together.
///
/// Twin of [`oracle_signature_arities_match_the_compiling_fences`], and
/// deliberately the same shape rather than a generalisation of it: the two scope
/// different sections and different name sets, and folding them into one
/// parameterised helper would put the section marker, the name set and the
/// panic's subject all behind arguments, which is how a failure ends up naming
/// the wrong section.
///
/// SCOPED TO `WHOLE_HANDLE_GEOMETRY_QUERY_NAMES` — four names, not the fifteen
/// `measurement_query_family_documented_in_geometry_chunk` covers. That is a
/// deliberate stop, not an oversight. The property needs a fence that actually
/// CALLS the name, and the whole-handle four share one realized-handle dispatch
/// path, so a single `structure def` over one let-bound solid exercises all four.
/// The other eleven do not fit that fence: `curvature`/`normal` need a hydrated
/// face or edge sub-handle, `geo_equiv` needs a dimensioned Length tolerance at
/// arity 3, and `max_deviation` needs two let-bound geometries on a separate
/// dispatch path. Covering them would need several fences of setup and would
/// couple this doc gate to argument-DIMENSION semantics, which the oracle guard
/// explicitly leaves unchecked. Name coverage for all fifteen is already total;
/// this adds arity precision where it is cheap and unambiguous.
///
/// The registry const is reached through the crate root
/// (`reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES`, re-exported at
/// `lib.rs`), never `reify_compiler::units::…` — `mod units` is private, so the
/// latter does not compile from an integration test.
#[test]
fn measurement_signature_arities_match_the_compiling_fences() {
    let markdown = read_chunk(CHUNK_PATH);
    let section = section_body(
        &markdown,
        MEASUREMENT_SECTION_MARKER,
        CHUNK_PATH,
        MEASUREMENT_SECTION_TITLE,
    );
    let fence_forms = fence_call_forms(&markdown, CHUNK_PATH);

    let mut drift = Vec::new();
    for name in reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES {
        let documented = documented_signature_arities(&section, name);
        // Anti-vacuity, per name. Without it, a name that lost its `-> <Type>`
        // annotation or its backticks — or was dropped from the signature list
        // entirely — would silently contribute zero assertions instead of failing.
        assert!(
            !documented.is_empty(),
            "{CHUNK_PATH}'s `{MEASUREMENT_SECTION_TITLE}` section documents no `{name}(…) -> \
             <Type>` signature form, so nothing pins that name's arity and this check would pass \
             vacuously for it. Restore the signature as ONE backticked `{name}(…) -> <Type>` code \
             span in the section's prose, return annotation included. (The `->` is what \
             separates a SIGNATURE from a prose mention; see the module doc's \"The one \
             doc-FORMAT pin this file does impose\".)"
        );

        // The measurement fence's own annotation writes
        // `volume(box(60mm, 40mm, 8mm))` to illustrate the inline-arg hoist, at
        // the same arity 1 as the real call. It is not an exercised form: comments
        // never reach the AST the fence forms are read from, so losing
        // `let v = volume(plate)` goes RED rather than leaving the annotation to
        // stand in for it.
        let in_fences = fence_arities(&fence_forms, name);

        drift.extend(arity_drift(name, &documented, &in_fences));
    }
    report(
        &format!(
            "`{MEASUREMENT_SECTION_TITLE}` signature arities that drifted from the compiling \
             ```reify fences"
        ),
        &drift,
    );
}

/// Minimum CATALOGUE ROWS the LENGTH-ARGUMENTS table must carry.
///
/// The EXACT live count (`translate`, `rotate_around`, `revolve`,
/// `line_segment`, `arc`, `helix`, `interp`/`bezier`, `nurbs`, `polygon`), not a
/// round number under it — at a lower floor a whole row could be deleted while
/// this stayed green, which is precisely the regression the floor claims to
/// catch. Raise it WITH the table; never lower it to go green.
const MINIMUM_CATALOGUE_ROWS: usize = 9;

/// Every geometry constructor the LENGTH-ARGUMENTS section names as taking a
/// dimensioned argument must be a REAL registry entry.
///
/// Twin of `units_chunk_smoke.rs`'s
/// `documented_call_names_in_units_chunk_are_real_registry_entries`: units.md
/// owns the RULE and the migration idiom, this chunk owns the per-position
/// CATALOGUE, and each is registry-verified on its own side rather than one
/// restating the other.
///
/// This is the phantom-NAME direction, and it is the one that has actually
/// failed in this repo: the 2026-07-24 language review found `rotate(geo, axis,
/// angle)` and `translate(geo, vector)` documented at signatures the compiler
/// had never been shown (tasks #5347 / #5364). A catalogue that names a
/// constructor which does not exist sends an author to write a call that cannot
/// compile, and — because a `structure def` body types an unresolved call from
/// its first argument — often will not even say so.
///
/// # What is actually scanned
///
/// TWO INDEPENDENT SETS, because a call scan alone did not reach the catalogue
/// this test is named for. The table writes eight of its nine constructors as a
/// bare backticked name, not a call — so when the section was read for call
/// forms only, DELETING EVERY TABLE ROW left this test green, all three
/// sentinels still satisfied by the ```reify fence below the table (measured;
/// only the `helix(radius, pitch, height)` row was ever visible). The advertised
/// reach was the reach a future editor would rely on, so the scan was widened
/// rather than the docstring narrowed:
///
///   - [`catalogue_table_rows`] harvests the TABLE's first column, floored at
///     [`MINIMUM_CATALOGUE_ROWS`] so deleting rows is RED;
///   - [`fence_call_forms`] harvests the CALLS of the section's ```reify fence,
///     read off its parsed AST, floored at [`MINIMUM_SECTION_CALL_NAMES`] so
///     rewriting the fence into prose is RED. A call written in the section's
///     prose or in a fence comment is never compiled, so it never counts.
///
/// Every name from either set faces the same registry assertion, and the three
/// sentinels are asserted against BOTH — the table must NAME them and the fence
/// must CALL them. Union-sentinels would let one half lose a constructor while
/// the other half covered for it, which is the failure this amendment exists to
/// close.
///
/// SCOPE — this checks NAMES, not arities and not argument dimensions. Arity for
/// the oracle names is cross-checked separately by
/// `oracle_signature_arities_match_the_compiling_fences`; argument
/// DIMENSION is pinned on the eval side by the tests the section's SYNC block
/// cites, not here.
#[test]
fn documented_call_names_in_the_length_section_are_real_registry_entries() {
    let markdown = read_chunk(CHUNK_PATH);
    // `section_body` panics if the marker is gone, so gutting the catalogue is
    // RED rather than vacuously green.
    let section = section_body(
        &markdown,
        LENGTH_ARGS_SECTION_MARKER,
        CHUNK_PATH,
        LENGTH_ARGS_SECTION_TITLE,
    );

    let table_rows = catalogue_table_rows(&section);
    let table_names = catalogue_table_names(&table_rows);
    let called = callee_names(&fence_call_forms(&section, CHUNK_PATH));

    // Anti-vacuity, one floor per set. The ROW floor catches a deleted or
    // gutted catalogue table — the case that used to pass silently. The CALL
    // floor catches a section rewritten into prose without call forms, so the
    // worked fence stops demonstrating anything.
    assert!(
        table_rows.len() >= MINIMUM_CATALOGUE_ROWS,
        "only {} catalogue row(s) found in {CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` table — \
         expected at least {MINIMUM_CATALOGUE_ROWS}. Either rows were deleted, or the table was \
         reformatted into a shape this scan cannot read: a catalogue row is a `|`-leading line \
         whose FIRST cell backticks the constructor it is about. Rows seen: {table_rows:?}",
        table_rows.len()
    );
    assert!(
        called.len() >= MINIMUM_SECTION_CALL_NAMES,
        "only {} distinct call name(s) found in the ```reify fence of {CHUNK_PATH}'s \
         `{LENGTH_ARGS_SECTION_TITLE}` section — expected at least {MINIMUM_SECTION_CALL_NAMES}. \
         The worked forms were cut from the fence or rewritten into prose, so the section \
         demonstrates nothing a compiler has seen. Names seen: {called:?}",
        called.len()
    );

    // Sentinels, against BOTH sets. `translate` is the headline case (every
    // component length-semantic, bare `0` included); `polygon` is the one with
    // NO dimensionless position at all; `nurbs` is the one that mixes both in a
    // single argument list, so it is where a catalogue is load-bearing.
    for sentinel in ["translate", "polygon", "nurbs"] {
        assert!(
            table_names.iter().any(|n| n == sentinel),
            "{CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` TABLE no longer has a row for \
             `{sentinel}`. That position is one of the three the catalogue exists to \
             distinguish — `translate` (every component length-semantic, bare `0` included), \
             `polygon` (no dimensionless position at all) and `nurbs` (lengths and counts in one \
             argument list). A worked fence below the table is NOT a substitute: the table is \
             what an author scans to find their constructor. Table names seen: {table_names:?}"
        );
        assert!(
            called.iter().any(|n| n == sentinel),
            "{CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` section's ```reify fence no longer \
             CALLS `{sentinel}`, so the row that names it is no longer demonstrated by anything \
             the compiler has accepted. Call names seen: {called:?}"
        );
    }

    for name in table_names.iter().chain(called.iter()) {
        assert!(
            registry_family(name).is_some()
                || LENGTH_SECTION_NAME_ALLOWLIST.contains(&name.as_str()),
            "{}",
            phantom_name_panic(
                CHUNK_PATH,
                &format!("its `{LENGTH_ARGS_SECTION_TITLE}` section"),
                name
            )
        );
    }
}

/// Minimum distinct CALL names the LENGTH-ARGUMENTS section's ```reify fence
/// must carry, as the anti-vacuity floor on the [`fence_call_forms`] half.
///
/// SEPARATE from [`MINIMUM_CATALOGUE_ROWS`], and deliberately so: this floors
/// the FENCE, which is a different claim from the table's coverage — a section
/// can tabulate a constructor it never demonstrates, and demonstrate one it
/// never tabulates — so it keeps its own number instead of tracking the table's.
///
/// That number is the EXACT live count of 7 calls in the fence (`helix`,
/// `translate`, `cylinder`, `rotate_around`, `polygon`, `interp`, `nurbs`),
/// under the same contract as [`MINIMUM_FN_CITES`]: at a floor under live,
/// calls can be deleted from the worked forms while this stays green. See that
/// constant for the re-measurement protocol.
const MINIMUM_SECTION_CALL_NAMES: usize = 7;

/// Names the length-arguments section may call that are in none of
/// `callable_registries.rs`'s `CALLABLE_NAME_REGISTRIES`, each with its
/// justification.
///
/// EMPTY, and kept empty on purpose — every constructor the catalogue names is a
/// `GEOMETRY_FUNCTION_NAMES` entry. The hook exists because the prelude
/// constructors this chunk documents elsewhere (`point3`, `vec3`, …) live in
/// `math_signatures::MATH_CONSTRUCTION_NAMES`, which is not reachable from the
/// crate root; see `CALLABLE_NAME_REGISTRIES`. Adding an entry here is a claim
/// that a name is real-but-unreachable, so write the justification next to it.
const LENGTH_SECTION_NAME_ALLOWLIST: &[&str] = &[];

/// NEGATIVE CONTROL for the fence guard — pins the part of it that discriminates.
///
/// The guard's power over call NAMES is real but narrow and entirely indirect,
/// and this test exists so that narrow part cannot rot away silently. A
/// `structure def` body accepts an unresolved function with no diagnostic at any
/// severity, typing the call from its first argument's `result_type`. So a
/// bogus `distanceZZZ(housing, bracket)` types as `Geometry`, and it is the
/// DOWNSTREAM `constraint … > 1mm` that fails with `CmpOperandKind` — not name
/// resolution.
///
/// The corollary is the hole documented in the module doc: the same typo on a
/// name whose result only feeds `not` (`intersectsZZZ`) produces zero errors.
/// Only the positive direction is asserted here; asserting the hole would go RED
/// the day someone fixes it, which is the wrong incentive.
#[test]
fn bogus_query_name_feeding_a_comparison_is_an_error() {
    // Same shape as the chunk's FORM B fence, inline so it is stable against
    // ordinary doc edits (mirrors the inline-repro posture of
    // `bogus_geometry_op_names_are_reported_as_unrecognised` in
    // `stdlib_chunk_geometry_ops_smoke.rs`).
    let source = r#"structure def BogusClearanceGate {
    let housing = cylinder_centered(10mm, 40mm)
    let bracket = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)

    let gap = distanceZZZ(housing, bracket)

    constraint gap > 1mm
}"#;

    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    // Assert the diagnostic IDENTITY, not mere presence. A bare
    // `!errors.is_empty()` is satisfied by any unrelated Error — a future
    // `cylinder_centered` signature change, a `translate` arity change, a typo
    // introduced while editing this inline source — so the control would keep
    // reading as live while no longer pinning the mechanism it names. The
    // mechanism is specifically: the misspelled call is NOT itself resolved as
    // an error; it types as `Geometry` from its first argument, and the
    // DOWNSTREAM `constraint gap > 1mm` is what rejects it.
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(reify_core::DiagnosticCode::CmpOperandKind)),
        "a fence-shaped source whose clearance query is misspelled must be rejected by the \
         downstream comparison with DiagnosticCode::CmpOperandKind. Got these Error \
         diagnostics instead: {:#?}\n\
         If the list is EMPTY, the fence compile gate \
         (fence_gate.rs::every_reify_tagged_fence_compiles_clean) has lost even its indirect \
         discriminating power over query names and is a pure parse check. If it \
         is non-empty but carries a different code, the rejection mechanism moved (e.g. \
         unresolved names became an error in their own right) — either way, re-verify what the \
         fence guard still establishes and update the module doc's \"What is NOT established\" \
         section.",
        errors
    );
}

// --- Cited-test resolution ---------------------------------------------------
//
// The chunk's two `<!-- SYNC ... -->` blocks hand-inventory the eval/CLI tests
// that pin each clearance-query trap, and that inventory is explicitly written to
// be read as the AUTHORITY on which traps are safe to rely on. Nothing else in
// this file looks at it: the guards above cover names, arities and fence
// compilation only. So a renamed or deleted test silently turns a PINNED row into
// a false claim — a rot mode strictly worse than plain prose, because the row
// still LOOKS load-bearing. The check below closes that, through
// `chunk_cite_gate.rs`'s resolver — the binary's one cite machinery, shared with
// `units_chunk_smoke.rs` and the corpus-wide cite gate.

/// Cite floors for [`cited_test_paths_in_the_chunk_resolve`].
///
/// The EXACT live counts, not round numbers under them. The three constants
/// below ARE the measurement — `<path>::<fn>` cites, then distinct `.rs` files,
/// then distinct `.ri` files — and this prose deliberately does not restate
/// them. A restated count is a second number that has to agree with the
/// constant, and the first draft of this docstring already did not: it read 22
/// against a floor of 24, both written in the same commit. That is the exact
/// mechanism the paragraph below warns about — a later editor re-derives from
/// the sentence and lands a floor under live. The panic text prints the whole
/// live cite list, so the failure itself is the re-measurement surface.
///
/// Task 5581's two new sections raised all three (from 13/6/4) by citing their
/// own chunk guards, the eval tests that pin the measurement family's runtime
/// behaviour, and the two worked `.ri` walks a reader is sent to next.
///
/// WHY EXACT — a measured incident, not a principle. With the cite floor one
/// below live, dropping trap 5's `single_body_self_pair_excluded` row left
/// fn_cites=8, rs_paths=5 and ri_paths=4 — every floor still green while that
/// row went on reading "PINNED by", a SYNC row silently claiming a pin it had
/// lost. Any gap between floor and live re-opens exactly that hole, which is why
/// these track the tree rather than sitting at a round number under it.
///
/// The `.ri` floor covers the worked references a designer is sent to next;
/// losing one is the same discoverability regression task 5389 closed.
///
/// RE-MEASUREMENT PROTOCOL, for every `MINIMUM_*` floor in this file and in
/// `units_chunk_smoke.rs` — stated once, here, and cross-referenced rather than
/// copied. These floors are hand-maintained and NOTHING detects their drift:
/// adding a SYNC row, a cite, a catalogue row or a fence call name silently
/// widens the gap between floor and live, and the check stays green while its
/// own docstring still claims to be exact. To re-measure one, set it to 999, run
/// `env cargo test -p reify-compiler --test harness_doc_chunks`, read the live
/// count out of the panic text, and restore it. Use `env cargo`, never bare
/// `cargo`: the skim PreToolUse wrapper condenses the run to PASS/FAIL counts
/// and swallows the panic the measurement depends on. MEASURE ONE FLOOR AT A
/// TIME — floors inside a single test assert sequentially, so a failing early
/// floor MASKS every later one in that test, and a batched sweep under-reports.
/// That masking is not hypothetical: it is why a first pass over these ten
/// floors found two of the four that had gone stale, and a one-at-a-time sweep
/// found all four.
const MINIMUM_FN_CITES: usize = 26;
const MINIMUM_RS_FILES: usize = 12;
const MINIMUM_RI_FILES: usize = 10;

/// Every test the chunk cites as PINNING a runtime claim must still exist.
///
/// WHY THIS CHUNK NEEDS IT. geometry.md's two `<!-- SYNC ... -->` blocks
/// hand-inventory the eval/CLI tests that pin each clearance-query trap, and
/// that inventory is explicitly written to be read as the AUTHORITY on which
/// traps are safe to rely on. Nothing else in this file looks at it — the guards
/// above cover names, arities and fence compilation only — so a renamed or
/// deleted test would silently turn a PINNED row into a false claim, a rot mode
/// strictly worse than plain prose because the row still LOOKS load-bearing.
///
/// The mechanism is shared with `units_chunk_smoke.rs`'s twin; see
/// [`assert_cited_paths_resolve`] for what it checks and, importantly, what it
/// does NOT (an existence check, never a semantic one).
#[test]
fn cited_test_paths_in_the_chunk_resolve() {
    assert_cited_paths_resolve(
        CHUNK_PATH,
        &read_chunk(CHUNK_PATH),
        MINIMUM_FN_CITES,
        MINIMUM_RS_FILES,
        MINIMUM_RI_FILES,
    );
}

// ── geometry.md → examples/ worked-example claim ─────────────────────────────
//
// Everything above checks what geometry.md says about the COMPILER. This checks
// one thing it says about the REPOSITORY: the chunk points designers at a
// runnable `.ri` file as the worked example of a constructor family, and a
// pointer to a file not containing what the prose promises sends a designer
// looking for a constructor they will never find. Same
// authoritative-doc-is-wrong failure class as the guards above, one artifact
// over. That every cited path EXISTS is `chunk_cite_gate.rs`'s corpus-wide
// job, so a dangling cite reds one test, not two.
//
// Deliberately NOT a wording pin (house rule: no doc-content meta-tests). The
// assertion reads a CLAIM out of the chunk and checks it against the real file
// on disk; either side may be reworded freely so long as the claim stays true.

/// The example geometry.md cites as the worked example of ALL FOUR GD&T zone
/// constructors, and the four names that claim has to cover.
const GDT_ZONES_EXAMPLE: &str = "examples/tolerancing/gdt_zones.ri";
const GDT_ZONE_CONSTRUCTORS: &[&str] =
    &["zone_slab", "zone_cylinder", "zone_annulus", "zone_profile"];

/// geometry.md's GD&T section cites [`GDT_ZONES_EXAMPLE`] as the worked example
/// of all four zone constructors, so that example must really call each of them.
///
/// This claim is what motivated the guard — `zone_slab` had no worked example
/// anywhere under `examples/` until task #5700 added a cell for it to the cited
/// file, so the one constructor the prose promised an example for was the one
/// that had none. This assertion is what keeps it that way. Whether the cite
/// RESOLVES is `chunk_cite_gate.rs`'s `every_path_cited_by_any_chunk_resolves`.
///
/// "Calls" is read off the example's parsed AST — [`call_forms`] — so a header
/// comment that merely names a constructor cannot satisfy the claim: describing
/// a call instead of making one is the laundering-a-gap-into-a-coverage-claim
/// failure this guard exists to catch.
#[test]
fn geometry_chunk_example_citations_hold_against_the_real_examples() {
    let geometry_md = read_chunk(CHUNK_PATH);
    let cited: Vec<String> = cited_source_paths(&geometry_md)
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| path.starts_with("examples/") && path.ends_with(".ri"))
        .collect();

    assert!(
        cited.iter().any(|path| path == GDT_ZONES_EXAMPLE),
        "{CHUNK_PATH} no longer cites {GDT_ZONES_EXAMPLE} — FIX: repoint this guard at \
         whatever example the GD&T section now cites, so the all-four claim stays checked \
         against the file it is actually made about. Cited: {cited:?}"
    );

    let example_src =
        std::fs::read_to_string(repo_root().join(GDT_ZONES_EXAMPLE)).unwrap_or_else(|e| {
            panic!("{GDT_ZONES_EXAMPLE} must be readable ({e}) — it is cited by {CHUNK_PATH}")
        });
    let called = callee_names(&call_forms(&example_src, GDT_ZONES_EXAMPLE));
    let absent: Vec<&str> = GDT_ZONE_CONSTRUCTORS
        .iter()
        .copied()
        .filter(|name| !called.iter().any(|call| call == name))
        .collect();
    assert!(
        absent.is_empty(),
        "{CHUNK_PATH} cites {GDT_ZONES_EXAMPLE} as the worked example of all four GD&T \
         zone constructors, but the example never calls: {}. A designer following that pointer \
         to learn one of them finds nothing — FIX: add a cell calling the missing constructor(s) \
         to {GDT_ZONES_EXAMPLE} (preferred: the example is the artifact designers actually run), \
         or narrow the chunk's claim to the constructors the example does exercise.",
        absent.join(", ")
    );
}

// --- arity_drift unit tests --------------------------------------------------

#[test]
fn arity_drift_reports_a_documented_arity_no_fence_calls() {
    let drift = arity_drift("interferes", &[1, 2], &[1]);

    assert_eq!(drift.len(), 1, "got {drift:#?}");
    assert!(
        drift[0].contains("`interferes`") && drift[0].contains("at 2 argument(s)"),
        "the line must name the documented form no fence exercises, got: {}",
        drift[0]
    );
}

#[test]
fn arity_drift_reports_a_fence_arity_no_signature_documents() {
    let drift = arity_drift("distance", &[2], &[2, 3]);

    assert_eq!(drift.len(), 1, "got {drift:#?}");
    for needle in ["`distance`", "at 3 argument(s)", "[2]"] {
        assert!(
            drift[0].contains(needle),
            "the line must name the undocumented fence arity and the documented set \
             (`{needle}`), got: {}",
            drift[0]
        );
    }
}

#[test]
fn arity_drift_is_silent_on_equal_sets_whatever_the_duplicates() {
    assert_eq!(arity_drift("volume", &[1], &[1]), Vec::<String>::new());
    assert_eq!(
        arity_drift("volume", &[1, 1], &[1, 1, 1]),
        Vec::<String>::new(),
        "a form documented twice or called by several fences is still one arity"
    );
    assert_eq!(
        arity_drift("min_clearance", &[3, 1], &[1, 3]),
        Vec::<String>::new(),
        "order does not matter"
    );
}

#[test]
fn arity_drift_reports_both_directions_at_once() {
    let drift = arity_drift("min_clearance", &[3], &[2]);

    assert_eq!(
        drift.len(),
        2,
        "a documented 3 no fence calls AND a fence 2 nothing documents, got {drift:#?}"
    );
}

// --- documented_signature_arities unit tests ---------------------------------

#[test]
fn documented_signature_arities_reads_only_signature_spans_in_unfenced_prose() {
    let section = "\
<!-- SYNC note: `distance(a, b, c) -> Length` was the old form -->

The gap is `distance(a, b) -> Length`; the one-argument `distance(a)` is a trap.

Written bare, distance(x, y, z, w) -> Length is not a code span.

```reify
structure def Gap {
    // distance(a, b, c, d, e) -> Length
    let g = distance(x, y)
}
```
";

    assert_eq!(
        documented_signature_arities(section, "distance"),
        vec![2],
        "only a whole `name(params) -> Type` code span in unfenced prose is a documented \
         signature: an HTML maintainer note, an un-backticked form, a mention with no `->` and \
         anything inside a fence (comment or call) are never read as one"
    );
}

#[test]
#[should_panic(expected = "variadic signature")]
fn documented_signature_arities_panics_on_a_variadic_signature() {
    let section = "The gap is `distance(a, …) -> Length`.\n";

    let _ = documented_signature_arities(section, "distance");
}
