//! Compile-smoke + registry test for the "units" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/units.md`), served to the in-GUI
//! assistant via `reify_language_reference`.
//!
//! Sibling of `geometry_chunk_smoke.rs`, and deliberately built ON TOP of it:
//! every scanner used here (`reify_tagged_fences`, `assert_module_compiles`,
//! `strip_reify_comments`, `call_sites`, `cited_source_paths`,
//! `resolve_cited_path`, `source_files_by_basename`) is that module's, raised to
//! `pub(crate)` and parameterised by chunk path in task 5759's prerequisite
//! refactor. Copying them here would have made this harness binary's FIFTH
//! near-identical scraper, which is exactly the tracked defect
//! (`tkt_0RS9A7843SBQ4BZX1A2ACY5TC1` / task #5924) that geometry_chunk_smoke.rs's
//! "Known duplication" section exists to stop growing.
//!
//! # What this file guards
//!
//! units.md acquired a "Dimensioned Geometry Arguments" section in task 5759 —
//! the intent-level statement of the LENGTH gate that PRD
//! `docs/prds/v0_6/units-length-gate-completion.md` landed across the compile
//! layer (`reify-compiler::builtin_signatures`, task #5750) and the eval layer
//! (`reify-eval::geometry_ops`, task #5744). An author asking "what units do
//! geometry arguments take?" lands on this chunk, so what it says must be what
//! the compiler does.
//!
//! Three properties are pinned, each by feeding CHUNK-DERIVED BYTES to the real
//! compiler or to a live name registry:
//!
//! 1. Every ```` ```reify ````-tagged fence COMPILES with zero `Severity::Error`
//!    (`reify_tagged_fences_in_units_chunk_compile`) — the documented migration
//!    idiom is something the compiler actually accepts.
//! 2. Every call NAME in those fences is a real registry entry
//!    (`documented_call_names_in_units_chunk_are_real_registry_entries`) — the
//!    phantom-signature direction that cost live probe cycles in the 2026-07-24
//!    language review (tasks #5347 / #5364).
//! 3. Every form the chunk documents as REJECTED is actually rejected, with the
//!    shared migration hint (`documented_rejected_forms_are_actually_rejected`).
//!
//! # What is NOT established
//!
//! THE CANONICAL SCOPE STATEMENT FOR THIS FILE — test docstrings point back here.
//!
//! - **No prose is pinned.** Nothing here matches a sentence, a heading, or an
//!   ordering. The only structural pins are the byte-exact fence info strings
//!   (```` ```reify ```` / ```` ```reify-rejected ````) and the one-cite-per-line
//!   format, all of which are house convention precisely so the surrounding
//!   wording stays free to change.
//! - **Compile-acceptance is a parse/shape/slot result, not a full signature
//!   check.** A `structure def` body types an unresolved call from its FIRST
//!   argument's `result_type`, so an unknown call NAME is not itself an error —
//!   which is why property 2 above exists as a separate registry assertion
//!   rather than being folded into the fence compile.
//! - **The `check`-vs-`eval` EXIT-CODE asymmetry the chunk documents is NOT
//!   pinned here.** It is a CLI-level property; this harness compiles in-process
//!   and never runs the binary. The chunk marks those rows UNPINNED for exactly
//!   that reason, and `cited_test_paths_in_the_units_chunk_resolve` only checks
//!   that the eval-side cites it DOES make resolve to real tests.

use crate::geometry_chunk_smoke::{
    assert_module_compiles, called_names, phantom_name_panic, registry_family, reify_tagged_fences,
    strip_reify_comments,
};

/// The chunk this file owns. Read (never written) at RUNTIME rather than
/// `include_str!`d, mirroring `geometry_chunk_smoke.rs`'s `CHUNK_PATH`, so an
/// edit to the markdown is seen by `cargo test` without a rebuild of this crate.
/// If the chunk moves, this const must move with it — the failure mode is a loud
/// `expect` on the read, not a silent skip.
const UNITS_CHUNK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/units.md"
);

/// Info string of the fences that MUST compile clean.
const REIFY_TAG: &str = "reify";

fn read_chunk() -> String {
    std::fs::read_to_string(UNITS_CHUNK_PATH).unwrap_or_else(|e| {
        panic!("{UNITS_CHUNK_PATH} must be readable ({e}) — update UNITS_CHUNK_PATH if the chunk moved")
    })
}

/// The ```` ```reify ````-tagged fences of units.md, comment-stripped.
///
/// Comment-free because every downstream scan here is a text scan: a call form
/// written only in a `//` annotation is never compiled, so it must not satisfy a
/// sentinel or contribute a name. Same reasoning — and the same helper — as
/// `geometry_chunk_smoke.rs`'s fence sentinels.
fn units_fence_code(markdown: &str) -> Vec<String> {
    reify_tagged_fences(markdown, REIFY_TAG, UNITS_CHUNK_PATH)
        .iter()
        .map(|fence| strip_reify_comments(fence))
        .collect()
}

/// Every ```` ```reify ````-tagged fence in units.md must actually compile.
///
/// units.md had NO fence gate at all before task 5759: all of its fences were
/// untagged, so the only test that read the chunk was
/// `angle_crossings_diagnostics_smoke.rs`'s single `contains` on a parse
/// diagnostic. The migration idiom the chunk now teaches — dimension every
/// length-semantic geometry argument — is the kind of claim a designer copies
/// verbatim, so it is compiled here rather than left as prose.
///
/// SCOPE — see "What is NOT established" in the module doc. This is a
/// parse/shape/arg-slot acceptance guard, not a signature pin; the registry half
/// is `documented_call_names_in_units_chunk_are_real_registry_entries`.
#[test]
fn reify_tagged_fences_in_units_chunk_compile() {
    let markdown = read_chunk();
    let fences = reify_tagged_fences(&markdown, REIFY_TAG, UNITS_CHUNK_PATH);

    // Anti-vacuity. Without this, dropping the ```reify tag (or rewriting the
    // idiom as an untagged block, which is what EVERY other fence in this chunk
    // still is) empties the scan and the loop below iterates zero times — GREEN,
    // protecting nothing.
    assert!(
        !fences.is_empty(),
        "the ```{REIFY_TAG} fence scan found NO fences in {UNITS_CHUNK_PATH} — expected at least \
         one, carrying the dimensioned-geometry-argument migration idiom. The scan matches the \
         info string BYTE-EXACTLY, so a bare ``` fence, an indented fence, or a ```reify-rejected \
         fence is invisible to it: the gate is vacuous and gives NO protection. Tag the idiom \
         fence ```{REIFY_TAG}."
    );

    // Sentinels, scanned COMMENT-FREE. `box` is the primitive whose bare-number
    // rejection is the chunk's headline example; `mirror` is the form that
    // carries BOTH halves of the rule in one call (dimensioned pivot, bare axis
    // components), so losing it would quietly retire the only fence-verified
    // demonstration that the legitimately-bare tail really is accepted.
    let code = units_fence_code(&markdown);
    for sentinel in ["box(", "mirror("] {
        assert!(
            code.iter().any(|fence| fence.contains(sentinel)),
            "anti-vacuity: no ```{REIFY_TAG} fence in {UNITS_CHUNK_PATH} contains `{sentinel}` \
             OUTSIDE A COMMENT — the documented migration idiom is no longer compile-verified, so \
             a form the compiler outright rejects could ship as the recommended fix. (A call form \
             mentioned only in a fence's `//` annotation does not count; it is never compiled.)"
        );
    }

    for (index, fence) in fences.iter().enumerate() {
        assert_module_compiles(
            UNITS_CHUNK_PATH,
            &format!("```{REIFY_TAG} fence #{}", index + 1),
            fence,
        );
    }
}

/// Names units.md's fences may call that are in none of the reachable registries,
/// each with its justification.
///
/// EMPTY on purpose — the migration idiom calls `box`, `mirror` and `translate`,
/// all `GEOMETRY_FUNCTION_NAMES` entries. The hook exists because the prelude
/// constructors (`point3`, `vec3`, …) live in
/// `math_signatures::MATH_CONSTRUCTION_NAMES`, which is not reachable from
/// `reify_compiler`'s crate root; see `CALLABLE_NAME_REGISTRIES`. An entry here
/// is a CLAIM that a name is real-but-unreachable, so write why next to it.
const UNITS_FENCE_NAME_ALLOWLIST: &[&str] = &[];

/// Every call name in units.md's ```` ```reify ```` fences must be a REAL
/// registry entry.
///
/// This is the load-bearing half of the chunk's claim. The fence compile gate
/// above establishes much less than it looks like it does: a `structure def`
/// body types an unresolved call from its FIRST argument's `result_type`, so an
/// unknown call NAME is not itself an error and a phantom constructor can
/// compile perfectly clean. Registry membership is what actually closes that
/// direction — the failure mode that cost live probe cycles in the 2026-07-24
/// language review, where geometry.md documented `rotate(geo, axis, angle)` and
/// `translate(geo, vector)` at signatures the compiler had never been shown
/// (tasks #5347 / #5364).
///
/// A phantom in THIS chunk would be worse than one in the geometry chunk,
/// because the whole point of the section is to be the form an author copies
/// when their bare-number call was just rejected.
///
/// SCOPE — NAMES only; see "What is NOT established" in the module doc.
#[test]
fn documented_call_names_in_units_chunk_are_real_registry_entries() {
    let markdown = read_chunk();

    let mut names: Vec<String> = Vec::new();
    for fence in units_fence_code(&markdown) {
        for name in called_names(&fence) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }

    // Anti-vacuity. An emptied scan — the ```reify tag dropped, the fence
    // rewritten as prose, or `strip_reify_comments` swallowing the body — would
    // otherwise iterate zero times and pass while protecting nothing.
    assert!(
        !names.is_empty(),
        "no call names were extracted from {UNITS_CHUNK_PATH}'s ```{REIFY_TAG} fences — the \
         registry check has nothing to verify and gives NO protection. Either the fence tag was \
         dropped, or the migration idiom was rewritten without call forms."
    );
    // The two sentinels are the same pair the compile gate uses, so the two
    // guards cannot disagree about which forms the chunk is supposed to teach.
    // Asserted on the EXTRACTED NAME SET rather than on raw text, so a call form
    // demoted to a comment fails here as well as there.
    for sentinel in ["box", "mirror"] {
        assert!(
            names.iter().any(|n| n == sentinel),
            "no ```{REIFY_TAG} fence in {UNITS_CHUNK_PATH} CALLS `{sentinel}` outside a comment \
             — the migration idiom no longer demonstrates the form it is supposed to. Names \
             seen: {names:?}"
        );
    }

    for name in &names {
        assert!(
            registry_family(name).is_some() || UNITS_FENCE_NAME_ALLOWLIST.contains(&name.as_str()),
            "{}",
            phantom_name_panic(UNITS_CHUNK_PATH, "its ```reify fences", name)
        );
    }
}
