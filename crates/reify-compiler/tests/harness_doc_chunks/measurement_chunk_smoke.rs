//! Truth checks for the "measurement" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/measurement.md`), served to the in-GUI
//! assistant via `reify_language_reference`: the measurement and
//! mass-property queries it documents must be the ones the compiler registers,
//! at the arities its compiling fences call them.
//!
//! Lives in `reify-compiler` (not `reify-mcp`, where the chunk itself lives)
//! because `reify-mcp` does not depend on `reify-compiler`, so only this crate
//! can read the compiler's registries and parse a fence. Split out of
//! geometry.md by task 7344, with the guards that cover it.
//!
//! What this file owns, each read off the chunk and checked against a live
//! registry rather than a hand-copied list:
//!
//! - every `reify_compiler::GEOMETRY_QUERY_NAMES` member is documented as a
//!   call form in the section's main body;
//! - every `reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES` member is called
//!   by a compiling ```` ```reify ```` fence, at exactly the arities its
//!   signature documents;
//! - the `undef` traps illustrate themselves with a query the inline-arg hoist
//!   does not cover;
//! - every test the SYNC note cites resolves.
//!
//! What compiling a fence does and does not establish is stated once, in
//! `geometry_chunk_smoke.rs`'s "What is NOT established". Argument DIMENSION and
//! the runtime behaviour of each query are pinned on the eval side, by the
//! tests the chunk's SYNC note cites, never here.
//!
//! # The one doc-FORMAT pin this file does impose
//!
//! [`measurement_signature_arities_match_the_compiling_fences`] requires each
//! of the four whole-handle names to carry a signature written as ONE code span
//! holding the whole `name(params) -> Type`, in the section's unfenced prose.
//! An un-backticked signature, one split across spans, or a markdown table with
//! the return type in its own column is therefore RED even though no
//! capability regressed. The `->` is what separates a documented SIGNATURE
//! from a prose mention of the same name.
//!
//! Every `MINIMUM_*` floor here is re-measured by the protocol stated once next
//! to `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.

use crate::chunk_cite_gate::assert_cited_paths_resolve;
use crate::chunk_io::{MEASUREMENT_CHUNK_PATH, read_chunk, report};
use crate::chunk_markdown::{marker_closed_region, section_body};
use crate::doc_forms::{
    arity_drift, documented_signature_arities, fence_arities, fence_call_forms,
};

/// Marker that OPENS the MEASUREMENT / mass-property section — the one that
/// documents [`reify_compiler::GEOMETRY_QUERY_NAMES`] as call forms. Matched
/// BYTE-EXACTLY on the trimmed line, as every section marker in this harness
/// is, and for the identical reason: the anchor must be inert so the heading's
/// wording stays free to change.
///
/// Scoping matters here more than anywhere else, because the names in this
/// family are the ones a corpus-wide scan cannot distinguish. `volume` and
/// `area` are ordinary English words that already appeared in this repo's
/// chunks as HAND-COMPUTED parameter arithmetic (`structures.md`'s
/// `let volume = thickness * width * width`), and `contains` is also the
/// `List`/`Set`/`Range` method documented in the `collections` chunk. An
/// unscoped word scan is satisfied by every one of those while teaching a reader
/// nothing about the kernel query — which is precisely the misdirection task
/// 5581 exists to remove, so the SECTION is what gets scanned.
///
/// The section ends at the chunk's `## Eval status…` heading, because
/// [`section_body`] stops at the next `##`. So only the main body (the signature
/// list and the worked fence) is scanned, and a call form that the traps show
/// only as an illustration does not count as documenting a query.
const MEASUREMENT_SECTION_MARKER: &str = "<!-- MEASUREMENT-SECTION -->";

/// Human-readable name of [`MEASUREMENT_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const MEASUREMENT_SECTION_TITLE: &str = "# Measurement & Mass-Property Queries";

/// Marker that OPENS the `undef`-TRAP region — the three traps (arg shape,
/// binder scope, no OCCT) that explain why a query silently yields
/// `Value::Undef`. Matched BYTE-EXACTLY on the trimmed line, as every other
/// marker here is, and for the identical reason: the anchor must be inert so
/// the heading's wording stays free.
///
/// This is the one marker scoping a region for a FORBIDDEN direction rather than
/// a required one, and the scoping is what makes that safe. The chunk writes
/// `volume(...)` legitimately all over — the signature list, the worked fence,
/// the fence's own hoist annotation — so a chunk-wide "no hoisted call form"
/// scan would be RED against entirely correct prose. Only these three traps
/// claim that an inline geometry argument yields `undef`, so only they are
/// scanned.
///
/// PAIRED WITH [`NOT_HOISTED_TRAP_END_MARKER`], and read through
/// [`marker_closed_region`] rather than [`section_body`], because "to the next
/// `##` heading" is NOT the extent this scan wants: unclosed, the region ran
/// past the traps and swallowed the chunk's closing **Worked reference**
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
const NOT_HOISTED_TRAP_TITLE: &str = "## Eval status, and when a query yields `undef`";

/// Every member of the geometry-QUERY registry must be documented as a call form
/// in the chunk's measurement section.
///
/// THE REGISTRY IS ITERATED DIRECTLY, not mirrored into a local list plus a
/// set-equality guard the way `geometry_chunk_smoke.rs`'s
/// `KINEMATIC_ORACLE_NAMES` is. That two-test shape exists so a failure can
/// distinguish "the registry grew" from "the doc shrank", and at three names it
/// is cheap. At fifteen — and at the thirty-one of `topology_chunk_smoke.rs`'s
/// `topology_selector_family_documented_in_topology_chunk` — the mirror becomes
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
/// No leading backtick is required, for the reason `geometry_chunk_smoke.rs`'s
/// `interference_oracle_names_documented_in_geometry_chunk` states at length: the
/// house rule these guards inherit forbids pinning doc TYPOGRAPHY. For the one
/// exception, see the module doc's "The one doc-FORMAT pin this file does
/// impose".
///
/// Anti-vacuity comes free from [`section_body`], which panics when its marker is
/// absent, so deleting the section is RED rather than silently green.
#[test]
fn measurement_query_family_documented_in_measurement_chunk() {
    let markdown = read_chunk(MEASUREMENT_CHUNK_PATH);
    let section = section_body(
        &markdown,
        MEASUREMENT_SECTION_MARKER,
        MEASUREMENT_CHUNK_PATH,
        MEASUREMENT_SECTION_TITLE,
    );

    for name in reify_compiler::GEOMETRY_QUERY_NAMES {
        let call_form = format!("{name}(");
        assert!(
            section.contains(&call_form),
            "{MEASUREMENT_CHUNK_PATH}'s `{MEASUREMENT_SECTION_TITLE}` section does not document \
             the geometry query `{name}` as a call form ({call_form}...). The chunk is what the \
             in-GUI assistant retrieves, so an undocumented query reads to it as a MISSING \
             CAPABILITY: asked for a part's mass or its centre of area it will hand-compute the \
             figure from the parameters instead of asking the kernel, and silently return a \
             number that no longer describes the realized geometry once a feature is added (task \
             5581). Either document the call form in that section, or — if the builtin itself is \
             gone — remove `{name}` from reify_compiler::GEOMETRY_QUERY_NAMES, which is iterated \
             directly here and is the sole source of this list."
        );
    }
}

/// The MEASUREMENT section's documented whole-handle signature arities and the
/// arities its compiling fences call those names at must be the SAME set, drift
/// in either direction reported together.
///
/// A name that no fence calls at all is drift too, so this is also the check
/// that each of the four is compile-verified. That matters for these four (task
/// 5581) because `volume`, `area` and `centroid` were previously in the chunk
/// corpus only as HAND-COMPUTED parameter arithmetic (`structures.md`'s
/// `let volume = thickness * width * width`). A documented call form the compiler
/// rejects would look to a reader just like the arithmetic it is meant to replace.
///
/// SCOPED TO `WHOLE_HANDLE_GEOMETRY_QUERY_NAMES` — four names, not the fifteen
/// `measurement_query_family_documented_in_measurement_chunk` covers. That is a
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
    let markdown = read_chunk(MEASUREMENT_CHUNK_PATH);
    let section = section_body(
        &markdown,
        MEASUREMENT_SECTION_MARKER,
        MEASUREMENT_CHUNK_PATH,
        MEASUREMENT_SECTION_TITLE,
    );
    let fence_forms = fence_call_forms(&markdown, MEASUREMENT_CHUNK_PATH);

    let mut drift = Vec::new();
    for name in reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES {
        let documented = documented_signature_arities(&section, name, MEASUREMENT_CHUNK_PATH);
        // Anti-vacuity, per name. Without it, a name that lost its `-> <Type>`
        // annotation or its backticks — or was dropped from the signature list
        // entirely — would silently contribute zero assertions instead of failing.
        assert!(
            !documented.is_empty(),
            "{MEASUREMENT_CHUNK_PATH}'s `{MEASUREMENT_SECTION_TITLE}` section documents no \
             `{name}(…) -> <Type>` signature form, so nothing pins that name's arity and this \
             check would pass vacuously for it. Restore the signature as ONE backticked \
             `{name}(…) -> <Type>` code span in the section's prose, return annotation included. \
             (The `->` is what separates a SIGNATURE from a prose mention; see the module doc's \
             \"The one doc-FORMAT pin this file does impose\".)"
        );

        // The measurement fence's own annotation writes
        // `volume(box(60mm, 40mm, 8mm))` to illustrate the inline-arg hoist, at
        // the same arity 1 as the real call. It is not an exercised form: comments
        // never reach the AST the fence forms are read from, so losing
        // `let v = volume(plate)` goes RED rather than leaving the annotation to
        // stand in for it.
        let in_fences = fence_arities(&fence_forms, name);

        drift.extend(arity_drift(
            MEASUREMENT_CHUNK_PATH,
            name,
            &documented,
            &in_fences,
        ));
    }
    report(
        &format!(
            "`{MEASUREMENT_SECTION_TITLE}` signature arities that drifted from the compiling \
             ```reify fences"
        ),
        &drift,
    );
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
/// THE DEFECT THIS CLOSES was live in this section. The arg-shape trap
/// illustrated "an inline geometry argument yields `undef`" with
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
/// geometry argument yields `undef`; the chunk's closing **Worked reference**
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
    let markdown = read_chunk(MEASUREMENT_CHUNK_PATH);
    let region = marker_closed_region(
        &markdown,
        NOT_HOISTED_TRAP_MARKER,
        NOT_HOISTED_TRAP_END_MARKER,
        MEASUREMENT_CHUNK_PATH,
        NOT_HOISTED_TRAP_TITLE,
    );

    for name in reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES {
        let call_form = format!("{name}(");
        assert!(
            !region.contains(&call_form),
            "{MEASUREMENT_CHUNK_PATH}'s `{NOT_HOISTED_TRAP_TITLE}` region exhibits \
             `{call_form}…` as its example of a call that yields `undef`, but `{name}` is one of \
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
        "{MEASUREMENT_CHUNK_PATH}'s `{NOT_HOISTED_TRAP_TITLE}` region no longer exhibits ANY query \
         call form at all, so the forbidden-direction check above passes vacuously — an empty \
         region satisfies it exactly as well as a correct one does. The region's job is to SHOW a \
         call that silently yields `undef`; a trap that names no call teaches nothing. Restore an \
         illustrative call drawn from one of {not_hoisted:?} (GEOMETRY_QUERY_NAMES minus the \
         hoisted whole-handle four, computed here from both registries so this list follows the \
         compiler)."
    );
}

/// Cite floors for [`cited_test_paths_in_the_chunk_resolve`]: `<path>::<fn>`
/// cites, then distinct `.rs` files, then distinct `.ri` files. The EXACT live
/// counts, never round numbers under them — why exact, and how to re-measure
/// one, is stated once next to `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
/// Raise them WITH the chunk; never lower one to go green.
const MINIMUM_FN_CITES: usize = 8;
const MINIMUM_RS_FILES: usize = 5;
const MINIMUM_RI_FILES: usize = 1;

/// Every test the chunk cites as PINNING a claim must still exist.
///
/// The chunk's SYNC note hand-inventories the chunk-side guards above and the
/// eval-side tests that pin which queries resolve to real numbers, and a reader
/// is invited to trust that inventory. A renamed or deleted test would silently
/// turn a pinned row into a false claim — worse than plain prose, because the
/// row still LOOKS load-bearing.
///
/// The mechanism is shared with every other chunk module that floors its cites;
/// see [`assert_cited_paths_resolve`] for what it checks and what it does NOT
/// (an existence check, never a semantic one).
#[test]
fn cited_test_paths_in_the_chunk_resolve() {
    assert_cited_paths_resolve(
        MEASUREMENT_CHUNK_PATH,
        &read_chunk(MEASUREMENT_CHUNK_PATH),
        MINIMUM_FN_CITES,
        MINIMUM_RS_FILES,
        MINIMUM_RI_FILES,
    );
}
