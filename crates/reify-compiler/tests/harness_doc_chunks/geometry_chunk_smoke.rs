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
//! - Signatures in unfenced prose and tables (`extrude`, `revolve`, the oracle
//!   queries) — `unfenced_signature_gate.rs`, the same way.
//! - ```` ```reify ```` worked examples — `fence_gate.rs` compiles every one
//!   verbatim and holds their count exact;
//!   `geometry_reify_fences_call_every_worked_example_form` requires the forms
//!   that need a worked example to be called in one. This file reads their
//!   calls through `doc_forms.rs`'s `fence_call_forms`, which holds every fence
//!   to a shape stricter than compiling clean — see its doc.
//! - Cited tests and examples — `chunk_cite_gate.rs` resolves every cite;
//!   `geometry_chunk_example_citations_hold_against_the_real_examples` holds the
//!   GD&T section's claim about its cited example.
//!
//! This file owns the rest: the interference/clearance oracle checked against
//! the compiler's registries with its documented arities cross-checked against
//! the worked fences, the length-argument catalogue's names, and the GD&T
//! section's worked-example claim. The measurement queries and the topology
//! selectors have their own chunks and their own modules,
//! `measurement_chunk_smoke.rs` and `topology_chunk_smoke.rs`.
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
//! `oracle_signature_arities_match_the_compiling_fences` requires each of the
//! five oracle names to carry a signature written as ONE code span holding the
//! whole `name(params) -> Type`, in the section's unfenced prose. **That single
//! span is therefore a PINNED notation for those five names: an un-backticked
//! signature, one split across spans, or a markdown table with the return type
//! in its own column (`| min_clearance(s, id_a, id_b) | Length |`) is RED even
//! though no capability regressed.** That is a deliberate trade, not an
//! oversight: the `->` is the ONLY thing separating a documented SIGNATURE from
//! the traps subsection's prose mentions of unsupported forms
//! (`min_clearance(a, b)`, `min_clearance(s, id, id)`), which must not be held
//! to the fences. Widening the scan to accept a table cell would re-admit those.
//! If the section is ever tabulated, widen `documented_signature_arities` in the
//! same commit — and re-check that the trap prose still reads as prose.

use reify_test_support::{compile_source_with_stdlib, errors_only};

use crate::callable_registries::{phantom_name_panic, registry_family};
use crate::chunk_cite_gate::{assert_cited_paths_resolve, cited_source_paths};
use crate::chunk_io::{GEOMETRY_CHUNK_PATH as CHUNK_PATH, read_chunk, repo_root, report};
use crate::chunk_markdown::{catalogue_table_names, catalogue_table_rows, section_body};
use crate::doc_forms::{
    arity_drift, call_forms, callee_names, documented_signature_arities, fence_arities,
    fence_call_forms,
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
    // rule this file inherits, prose formatting is not the subject. (For the ONE
    // exception, see the module doc's "The one doc-FORMAT pin this file does
    // impose" before re-typesetting this section.) The real weight is carried by
    // (b) below, by `section_body`'s anti-vacuity panic, and
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

/// Every form the chunk teaches by worked example — the oracle family, and the
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
        let documented = documented_signature_arities(&section, name, CHUNK_PATH);
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

        drift.extend(arity_drift(CHUNK_PATH, name, &documented, &in_fences));
    }
    report(
        &format!(
            "`{ORACLE_SECTION_TITLE}` signature arities that drifted from the compiling ```reify \
             fences"
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
    let section_label = format!("{CHUNK_PATH}'s `{LENGTH_ARGS_SECTION_TITLE}` section");
    let called = callee_names(&fence_call_forms(&section, &section_label));

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
/// RE-MEASUREMENT PROTOCOL, for every `MINIMUM_*` floor in this file and in its
/// sibling chunk modules and corpus gates — stated once, here, and
/// cross-referenced rather than copied. These floors are hand-maintained and NOTHING detects their drift:
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
const MINIMUM_FN_CITES: usize = 14;
const MINIMUM_RS_FILES: usize = 8;
const MINIMUM_RI_FILES: usize = 8;

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
