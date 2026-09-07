//! Compile-smoke + registry test for the "units" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/units.md`), served to the in-GUI
//! assistant via `reify_language_reference`.
//!
//! Sibling of `geometry_chunk_smoke.rs`, and deliberately built ON TOP of it:
//! every scanner used here (`reify_tagged_fences`, `assert_module_compiles`,
//! `strip_reify_comments`, `called_names`, `registry_family`) is that module's,
//! raised to `pub(crate)` and parameterised by chunk path in task 5759's
//! prerequisite refactor — as is the whole cited-path loop
//! (`assert_cited_paths_resolve`), extracted in the same spirit once this file
//! and its sibling had grown two copies of it. Copying any of them here would
//! have made this harness binary's FIFTH near-identical scraper, which is
//! exactly the tracked defect (`tkt_0RS9A7843SBQ4BZX1A2ACY5TC1` / task #5924)
//! that geometry_chunk_smoke.rs's "Known duplication" section exists to stop
//! growing.
//!
//! What this file DOES own is the handful of helpers no sibling has a use for —
//! `rejected_form_rows`, `wrap_form`, `named_length_argument`,
//! `squash_whitespace` — and those are pinned directly by the "Scanner unit
//! tests" block at the bottom, following the same convention.
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

use reify_core::units::LENGTH_MIGRATION_HINT;
use reify_test_support::{compile_source_with_stdlib, errors_only};

use crate::geometry_chunk_smoke::{
    assert_cited_paths_resolve, assert_module_compiles, called_names, phantom_name_panic,
    registry_family, reify_tagged_fences, strip_reify_comments,
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

/// Info string of the rejected-forms block. DELIBERATELY NOT `reify`.
///
/// `reify_tagged_fences` matches the whole info string byte-exactly and its
/// consumer asserts ZERO `Severity::Error` per fence, so a deliberately-invalid
/// form inside a ```` ```reify ```` fence would fail the compile gate — and the
/// failure would read as "the documented migration does not compile", which is
/// the opposite of what is wrong. Any other explicit tag is exempt by the same
/// convention the repo-wide fence gate specifies
/// (`docs/prds/v0_6/doc-chunk-truth-enforcement.md`: any other explicit tag ->
/// exempt), so this is both safe today and forward-compatible.
const REJECTED_TAG: &str = "reify-rejected";

/// Separator between a rejected form and its accepted migration, on one row.
///
/// `-->` and not `=>`: `=>` is Reify match-arm syntax and would be ambiguous
/// inside a form, whereas `-->` cannot occur in either column.
const ROW_SEPARATOR: &str = "-->";

/// Info string of the block listing forms the COMPILE layer does not see.
///
/// A separate block, not a column of the first one, because the two carry
/// OPPOSITE compile-layer assertions: the forms in `REJECTED_TAG` must produce
/// an Error, and the forms here must produce NONE. Merging them would need a
/// per-row marker the scraper reads, which is a wording pin on the chunk; two
/// byte-exact tags cost the same and stay inert.
const EVAL_ONLY_TAG: &str = "reify-rejected-at-eval";

/// Minimum rows the eval-only block must carry. The exact live set: `helix` and
/// `polygon`, the two constructors with no compile-layer LENGTH slot that the
/// chunk names by hand.
///
/// WAS 3. `mirror` was the third until its arity-7 pivot triple gained a
/// compile-layer LENGTH slot (`builtin_signatures.rs`, `#5662`); this test's own
/// panic message is what demanded the row move to the ```` ```reify-rejected ````
/// block, and the floor came down with it in the same commit. That is the
/// intended lifecycle of this constant, NOT a "lower it to go green" — every
/// remaining row is still an exact, named member. Raise it again only by adding
/// a row; lower it again only by moving one to the compile-layer block.
const MINIMUM_EVAL_ONLY_ROWS: usize = 2;

/// Minimum rows the rejected-forms block must carry, and the anti-vacuity floor.
///
/// The EXACT set the chunk is required to document, not a round number under it:
/// bare dimensions, the D1 bare-zero row, a `translate` row, a modify-op
/// (`fillet`) row, and the `mirror` row that doubles as the legitimately-bare
/// illustration. At a lower floor any one of those could be deleted while this
/// still passed — the regression these floors exist to catch. Raise it with the
/// block; never lower it to go green.
///
/// WAS 4, raised when `mirror` moved here from the eval-only block (see
/// [`MINIMUM_EVAL_ONLY_ROWS`]).
const MINIMUM_REJECTED_ROWS: usize = 5;

/// The bare-zero form PRD decision D1 refuses to special-case. Pinned by name so
/// deleting that row from the chunk is RED at the doc surface, not merely
/// untested.
///
/// Compared WHITESPACE-INSENSITIVELY (see [`squash_whitespace`]) — the claim is
/// "the D1 row is still there", never "it is spelled with this spacing".
const BARE_ZERO_FORM: &str = "box(0, 0, 0)";

/// `s` with EVERY whitespace character removed, so two spellings of the same
/// call form compare equal.
///
/// Not `split_whitespace().join(" ")`, which only collapses RUNS and so still
/// distinguishes `box(0, 0, 0)` from the equally-valid `box(0,0,0)`. A doc edit
/// to the tighter spelling would then fail with "the chunk no longer documents
/// `box(0, 0, 0)`", sending the reader hunting for a row that is in fact still
/// there — a false negative dressed as a deletion.
fn squash_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The compile layer's marker phrase for a LENGTH-slot rejection, as emitted by
/// `reify_compiler::builtin_signatures` (see its
/// `"box: width argument expects Length, got Int; …"` template).
///
/// A LOCAL CONST, NOT AN IMPORTED ONE, and knowably second-best: the sibling
/// migration-hint assertion is made BY REFERENCE to
/// `reify_core::units::LENGTH_MIGRATION_HINT`, so a D9 rewording moves the
/// constant and every consumer together. This phrase has no such constant behind
/// it, so a rewording to an equally-valid shape — e.g. the eval layer's own
/// `missing or non-Length argument '<arg>' for <op>` — would make
/// [`assert_rejected_as_documented`] fail with "no Error message contains …"
/// while the gate and the chunk were both perfectly correct.
///
/// Exporting a const from `builtin_signatures.rs` is the real fix and is left
/// UNDONE ON PURPOSE: that file is outside task 5759's locked scope. What is
/// done here instead is to name the phrase once and unit-test the extraction
/// that reads it ([`named_length_argument_reads_the_argument_out_of_a_message`]),
/// so the brittleness is at least visible and localised to one line.
const LENGTH_SLOT_DIAGNOSTIC_MARKER: &str = " argument expects Length";

/// The ARGUMENT NAME a LENGTH-slot rejection blames, or `None` if `message` is
/// not one.
///
/// The token immediately before [`LENGTH_SLOT_DIAGNOSTIC_MARKER`]: in
/// `"box: width argument expects Length, got Int; …"` that is `width`. Split out
/// of [`assert_rejected_as_documented`] so it can be pinned directly by a unit
/// test over a synthetic message rather than only through the live compiler,
/// which is this file's convention for every hand-rolled text scan (see
/// `geometry_chunk_smoke.rs`'s "Scanner unit tests" block).
fn named_length_argument(message: &str) -> Option<&str> {
    let before = message.split(LENGTH_SLOT_DIAGNOSTIC_MARKER).next()?;
    if before.len() == message.len() {
        // No marker present — `split` yielded the whole string unchanged.
        return None;
    }
    before.split_whitespace().next_back()
}

/// Wrap one documented FORM in the minimal compilable module the rejected-forms
/// block is written against.
///
/// `g` IS BOUND FOR THE FORM. The chunk writes `mirror(g, 0, 0, 0, 1, 0, 0)`
/// because that is the shape an author recognises from their own file; a free
/// `g` would fail to resolve and the rejection assertion below would pass for
/// entirely the wrong reason. Binding it here keeps the documented row readable
/// AND keeps the assertion about the units gate. This contract is restated in
/// the chunk's own SYNC note, so a row author knows `g` is available.
fn wrap_form(form: &str) -> String {
    format!(
        "structure def RejectedForm {{\n    let g = box(10mm, 10mm, 10mm)\n    let subject = {form}\n}}"
    )
}

/// The `(rejected, accepted)` rows of the ```` ```reify-rejected ```` block.
///
/// PANICS on a non-empty row that is not a pair, rather than skipping it. A
/// scraper that silently drops what it cannot parse is how a gate goes vacuous
/// while still looking like it is doing work.
fn rejected_form_rows(markdown: &str, tag: &str) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();

    for fence in reify_tagged_fences(markdown, tag, UNITS_CHUNK_PATH) {
        for line in strip_reify_comments(&fence).lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((rejected, accepted)) = line.split_once(ROW_SEPARATOR) else {
                panic!(
                    "{UNITS_CHUNK_PATH}'s ```{tag} block has a row this scan cannot \
                     read: {line:?}. Every non-blank row must pair a rejected form with its \
                     accepted migration, separated by `{ROW_SEPARATOR}`, WHOLE ON ONE LINE — a \
                     wrapped row is invisible here. Annotate with `//` if a row needs prose."
                )
            };
            rows.push((rejected.trim().to_string(), accepted.trim().to_string()));
        }
    }
    rows
}

/// Compile one documented form and return its Error messages.
fn error_messages(form: &str) -> Vec<String> {
    let compiled = compile_source_with_stdlib(&wrap_form(form));
    errors_only(&compiled)
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// Assert `form` is REJECTED the way the chunk promises: at least one Error,
/// whose message names the offending ARGUMENT and carries the shared migration
/// hint.
fn assert_rejected_as_documented(form: &str) {
    let messages = error_messages(form);

    assert!(
        !messages.is_empty(),
        "{UNITS_CHUNK_PATH} documents `{form}` as REJECTED, but it compiles with ZERO Error \
         diagnostics. Either the gate regressed (a bare number is being accepted at a \
         length-semantic position again) or the chunk is now teaching a form that is fine. The \
         chunk is served verbatim to the in-GUI assistant, so a stale rejection row sends a \
         designer to `fix` code that was never broken."
    );

    // (a) The message must NAME THE OFFENDING ARGUMENT — the token immediately
    //     before ` argument expects Length`. A diagnostic that said only "wrong
    //     type somewhere" would satisfy a naive non-empty check while leaving an
    //     author with no idea which of six coordinates to fix.
    //
    //     THIS IS THE ONE DIAGNOSTIC-WORDING PIN IN THIS FILE, and it is a
    //     second-best: see `LENGTH_SLOT_DIAGNOSTIC_MARKER` for why the phrase is
    //     a local const rather than one imported from the compile layer, and for
    //     the failure mode a rewording produces here.
    let named: Vec<(&String, &str)> = messages
        .iter()
        .filter_map(|m| named_length_argument(m).map(|arg| (m, arg)))
        .collect();
    assert!(
        !named.is_empty(),
        "{UNITS_CHUNK_PATH} documents `{form}` as rejected at a LENGTH argument slot, but no \
         Error message contains `{LENGTH_SLOT_DIAGNOSTIC_MARKER}`. If the gate is working and \
         only the WORDING moved, this test is what has to follow it — update \
         LENGTH_SLOT_DIAGNOSTIC_MARKER, do not delete the row. Diagnostics seen: {messages:?}"
    );
    for (message, arg) in &named {
        assert!(
            !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "the rejection for `{form}` does not NAME the offending argument — read \
             {arg:?} out of {message:?}, which is not a bare identifier. The chunk promises the \
             diagnostic tells an author WHICH argument to dimension."
        );
    }

    // (b) The shared migration hint must be present. Asserted BY REFERENCE to
    //     `reify_core::units::LENGTH_MIGRATION_HINT` and never as a hardcoded
    //     string, so a PRD-D9 wording change moves the constant, the compile
    //     layer, the eval layer and this test together — instead of leaving the
    //     chunk quoting wording that no longer ships.
    assert!(
        messages.iter().any(|m| m.contains(LENGTH_MIGRATION_HINT)),
        "the rejection for `{form}` carries no `{LENGTH_MIGRATION_HINT}` migration hint, which \
         PRD decision D9 promises on every LENGTH-slot rejection and which the chunk quotes as \
         the template. Diagnostics seen: {messages:?}"
    );
}

fn read_chunk() -> String {
    std::fs::read_to_string(UNITS_CHUNK_PATH).unwrap_or_else(|e| {
        panic!(
            "{UNITS_CHUNK_PATH} must be readable ({e}) — update UNITS_CHUNK_PATH if the chunk moved"
        )
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

/// Every form units.md documents as REJECTED must actually be rejected.
///
/// The chunk's central claim is a NEGATIVE one — "this is rejected" — and a
/// negative claim is exactly what a compile gate cannot check, because the
/// offending form must never enter the zero-Error fence scan. This test closes
/// that half: it scrapes the documented rows and runs the real compiler over
/// both columns, so the table is executable rather than asserted.
///
/// Three properties per row, all over CHUNK-DERIVED BYTES:
///   - the rejected column really produces at least one `Severity::Error`;
///   - the message NAMES the offending argument, and carries
///     `reify_core::units::LENGTH_MIGRATION_HINT` (PRD decision D9);
///   - the accepted column really compiles CLEAN, so a migration that stopped
///     working cannot sit in the chunk as advice.
///
/// The third is what makes a stale table RED in both directions. A rejection
/// test alone would stay green while the recommended fix rotted.
#[test]
fn documented_rejected_forms_are_actually_rejected() {
    let markdown = read_chunk();
    let rows = rejected_form_rows(&markdown, REJECTED_TAG);

    // Anti-vacuity #1: the floor. Without it, deleting the block (or retagging
    // it) empties the scan and the loop below iterates zero times — GREEN,
    // protecting nothing.
    assert!(
        rows.len() >= MINIMUM_REJECTED_ROWS,
        "only {} rejected-form row(s) scraped from {UNITS_CHUNK_PATH} — expected at least \
         {MINIMUM_REJECTED_ROWS}. Either the ```{REJECTED_TAG} block was deleted or retagged \
         (the info string is matched BYTE-EXACTLY), or a row was removed while the prose still \
         claims the form is rejected. Rows seen: {rows:?}",
        rows.len()
    );

    // Anti-vacuity #2: the POSITIVE control, through the same helper. An
    // over-eager assertion — or a `wrap_form` that produced garbage — would make
    // every form `reject` and pass the loop below for a reason that has nothing
    // to do with the units gate.
    assert_module_compiles(
        UNITS_CHUNK_PATH,
        "positive control for the rejected-forms scan",
        &wrap_form("box(20mm, 20mm, 10mm)"),
    );

    for (rejected, accepted) in &rows {
        assert_ne!(
            rejected, accepted,
            "{UNITS_CHUNK_PATH} has a rejected-forms row whose two columns are IDENTICAL, so it \
             teaches no migration and would pass any assertion trivially"
        );
        assert_rejected_as_documented(rejected);
        assert_module_compiles(
            UNITS_CHUNK_PATH,
            &format!("accepted migration for `{rejected}`"),
            &wrap_form(accepted),
        );
    }
}

/// PRD decision D1, pinned at the doc surface: bare `0` is not special-cased.
///
/// Its own test rather than one row of the loop above, because it is the row an
/// author is most likely to think is an oversight and delete — a zero length
/// "obviously" needs no unit. Both halves are asserted: that the chunk still
/// DOCUMENTS the form, and that the compiler still rejects it. Either one alone
/// rots — a table row nothing executes, or an executable claim nothing documents.
///
/// The eval-layer twin is
/// crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs::bare_zero_box_dimensions_are_not_special_cased
#[test]
fn bare_zero_is_not_special_cased() {
    let markdown = read_chunk();
    let rows = rejected_form_rows(&markdown, REJECTED_TAG);

    // WHITESPACE-INSENSITIVE on both sides: `box(0,0,0)` and `box( 0, 0, 0 )` are
    // the same row, and what is pinned is the row's PRESENCE, not its spacing.
    assert!(
        rows.iter()
            .any(|(rejected, _)| squash_whitespace(rejected) == squash_whitespace(BARE_ZERO_FORM)),
        "{UNITS_CHUNK_PATH}'s ```{REJECTED_TAG} block no longer documents `{BARE_ZERO_FORM}` \
         (compared ignoring all whitespace, so this is a real deletion and not a respacing). \
         PRD decision D1 is that bare `0` gets NO special case, and it is the one an author \
         assumes is exempt, so it is the row the table most needs. Rows seen: {rows:?}"
    );

    assert_rejected_as_documented(BARE_ZERO_FORM);
}

/// Cites units.md must carry, as the anti-vacuity floors for
/// [`cited_test_paths_in_the_units_chunk_resolve`].
///
/// The EXACT live counts, not round numbers under them — the reasoning
/// `geometry_chunk_smoke.rs::MINIMUM_FN_CITES` spells out: at a floor below the
/// live count one whole cite can be deleted while the check stays green, which
/// is precisely the regression the floor claims to catch. Seven `<path>::<fn>`
/// cites across three `.rs` files (this module's own self-cites, plus the
/// primitive-profile and modify-sweep eval pins), and one `.ri` exemplar
/// (`angle_crossings.ri`, which predates this task).
///
/// Raise these WITH the chunk when a cite is added. Never lower one to go green:
/// a lowered floor is a SYNC row that has quietly stopped claiming anything. How
/// to re-measure one — and why a batched sweep under-reports — is stated once,
/// next to `geometry_chunk_smoke.rs::MINIMUM_FN_CITES`, not repeated here.
const MINIMUM_FN_CITES: usize = 7;
const MINIMUM_RS_FILES: usize = 3;
const MINIMUM_RI_FILES: usize = 1;

/// Every test units.md cites as PINNING a claim must still exist.
///
/// WHY THIS CHUNK NEEDS IT. units.md's check-visibility note marks some claims
/// PINNED and others UNPINNED, and a reader is invited to trust the distinction.
/// Without this test a PINNED row survives the deletion or rename of the test it
/// names, and the chunk — served verbatim to the in-GUI assistant — goes on
/// asserting a guarantee that no longer exists. A dangling cite is not a broken
/// link; it is a false claim. The three `.rs` files behind the floor are this
/// module's own guard plus the two eval-layer pins the note relies on, and the
/// one `.ri` is `examples/best_practices/angle_crossings.ri`, the compile-gated
/// exemplar this chunk defers to for the angle-crossing diagnostics.
///
/// Twin of `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve`, and
/// SHARES ITS BODY: the loop was extracted to
/// [`assert_cited_paths_resolve`] rather than copied, so a fix to the resolution
/// or existence rule lands once. See that helper for what it checks and what it
/// deliberately does not.
#[test]
fn cited_test_paths_in_the_units_chunk_resolve() {
    assert_cited_paths_resolve(
        UNITS_CHUNK_PATH,
        &read_chunk(),
        MINIMUM_FN_CITES,
        MINIMUM_RS_FILES,
        MINIMUM_RI_FILES,
    );
}

/// The constructors units.md names as caught only at build/eval time really are
/// INVISIBLE to the compile layer.
///
/// The complement of `documented_rejected_forms_are_actually_rejected`, and the
/// executable half of the chunk's check-visibility note. `helix` and `polygon`
/// have no `CheckableArg` LENGTH slot — `polygon` deliberately so, per
/// `builtin_signatures.rs`'s own
/// `polygon_stays_slot_free_because_its_positions_are_arity_open` — so a bare
/// number in one of them compiles CLEAN and is rejected later, which is why
/// `reify check` prints `error:` for these and still exits 0.
///
/// `mirror` USED TO BE A THIRD MEMBER and no longer is: its arity-7 pivot triple
/// gained `length_arg(1..=3, "ox"/"oy"/"oz")` slots, so a bare origin is now a
/// COMPILE error and `reify check` exits 1 on it (pinned at the CLI seam by
/// `crates/reify-cli/tests/harness_cli/cli_check.rs`'s
/// `check_rejects_bare_scalar_mirror_origin_before_reaching_build`). This test
/// went red exactly as its panic message promised, and the row moved to the
/// ```` ```reify-rejected ```` block in the commit that recorded the move — the
/// lifecycle "WHY PIN THE NEGATIVE" below describes, actually exercised.
///
/// WHY PIN THE NEGATIVE. The chunk tells an author to gate on `reify eval`
/// rather than `reify check`, and that advice is only worth following while the
/// gap is real. If a compile-layer slot is later added for one of these, this
/// test goes RED and the row must move to the ```` ```reify-rejected ```` block
/// — so the doc is corrected by the same commit that closes the gap, instead of
/// warning about a hazard that no longer exists.
///
/// SCOPE — this pins the COMPILE-layer half only. That these forms are rejected
/// at eval time is pinned on the eval side by the tests the chunk cites; that
/// `reify check` EXITS 0 on them is a CLI-level property this harness never
/// observes, and the chunk marks it UNPINNED for exactly that reason.
#[test]
fn documented_eval_only_rejections_are_invisible_to_the_compile_layer() {
    let markdown = read_chunk();
    let rows = rejected_form_rows(&markdown, EVAL_ONLY_TAG);

    assert!(
        rows.len() >= MINIMUM_EVAL_ONLY_ROWS,
        "only {} eval-only row(s) scraped from {UNITS_CHUNK_PATH} — expected at least \
         {MINIMUM_EVAL_ONLY_ROWS} (`helix`, `polygon`). Either the \
         ```{EVAL_ONLY_TAG} block was deleted or retagged, or a row was removed while the \
         check-visibility note still names the constructor. Rows seen: {rows:?}",
        rows.len()
    );

    // NEGATIVE CONTROL. "Invisible to the compile layer" is trivially true of
    // everything if `error_messages` has stopped seeing rejections at all — a
    // broken wrapper, a helper that swallows diagnostics. Prove it still sees
    // one, using a form the OTHER block documents as compile-layer rejected.
    assert!(
        !error_messages(BARE_ZERO_FORM).is_empty(),
        "negative control failed: `{BARE_ZERO_FORM}` should be REJECTED by the compile layer, \
         but this helper reports no Error diagnostics for it. Every assertion below would then \
         pass vacuously, since it only checks that a form compiles clean."
    );

    for (rejected, accepted) in &rows {
        assert_ne!(
            rejected, accepted,
            "{UNITS_CHUNK_PATH} has an eval-only row whose two columns are IDENTICAL. Both sides \
             compile clean here by construction, so such a row passes trivially while teaching \
             no migration."
        );

        let messages = error_messages(rejected);
        assert!(
            messages.is_empty(),
            "{UNITS_CHUNK_PATH} lists `{rejected}` as caught only at build/eval time, but the \
             COMPILE layer now rejects it: {messages:?}. That is good news — a slot was added — \
             but the chunk is now wrong twice over: the form belongs in the ```{REJECTED_TAG} \
             block, and the check-visibility note must stop naming this constructor as one \
             `reify check` does not gate. Move the row and update the note in the same commit."
        );

        assert_module_compiles(
            UNITS_CHUNK_PATH,
            &format!("accepted migration for `{rejected}`"),
            &wrap_form(accepted),
        );
    }
}

// --- Scanner unit tests ------------------------------------------------------
//
// `rejected_form_rows`, `named_length_argument`, `squash_whitespace` and
// `wrap_form` are this module's own hand-rolled text helpers, and every
// rejection assertion above is downstream of one of them. They are pinned
// DIRECTLY here rather than only through the chunk, which is the posture
// `geometry_chunk_smoke.rs`'s own "Scanner unit tests" block establishes for the
// scanners this file imports. The failure these guard against is
// self-concealing: a helper that quietly stopped extracting anything would leave
// every floor and sentinel above satisfied, because those are drawn from the
// same helpers' output.

/// The argument-name extraction reads the blamed argument out of a SYNTHETIC
/// message, so it is pinned independently of whatever the compiler emits today.
///
/// The input is the byte-exact template `builtin_signatures.rs` documents. If
/// the live diagnostic ever diverges from it, `documented_rejected_forms_are_
/// actually_rejected` goes red while THIS stays green — which is the signal that
/// the wording moved rather than the gate breaking. See
/// [`LENGTH_SLOT_DIAGNOSTIC_MARKER`].
#[test]
fn named_length_argument_reads_the_argument_out_of_a_message() {
    assert_eq!(
        named_length_argument(
            "box: width argument expects Length, got Int; pass a dimensioned length such as `5mm`"
        ),
        Some("width")
    );
    // A multi-word prefix must still yield the LAST token, not the first.
    assert_eq!(
        named_length_argument("linear_pattern: spacing argument expects Length, got Int"),
        Some("spacing")
    );
}

/// A message that is not a LENGTH-slot rejection yields `None` rather than a
/// bogus name.
///
/// The regression this catches is the one the marker const's doc names: if the
/// compile layer reworded to the eval layer's shape, a scanner that "recovered"
/// some token anyway would let `assert_rejected_as_documented`'s per-message
/// identifier assertion pass on a message it never actually parsed.
#[test]
fn named_length_argument_rejects_a_message_without_the_marker() {
    assert_eq!(
        named_length_argument("missing or non-Length argument 'ox' for mirror"),
        None
    );
    assert_eq!(named_length_argument(""), None);
}

/// A row missing its `-->` separator PANICS rather than being skipped.
///
/// The documented safety property of [`rejected_form_rows`] — "a scraper that
/// silently drops what it cannot parse is how a gate goes vacuous while still
/// looking like it is doing work" — asserted directly. Without this control that
/// property is only prose: a `continue` in place of the `panic!` would make a
/// malformed row invisible, and every floor above would still be satisfied by
/// the well-formed rows around it.
#[test]
#[should_panic(expected = "has a row this scan cannot read")]
fn rejected_form_rows_panics_on_a_row_missing_its_separator() {
    let markdown = format!("```{REJECTED_TAG}\nbox(0, 0, 0)\n```\n");
    let _ = rejected_form_rows(&markdown, REJECTED_TAG);
}

/// A well-formed synthetic block scrapes to trimmed `(rejected, accepted)`
/// pairs, and `//` annotations are not mistaken for rows.
///
/// The POSITIVE half of the control above: `#[should_panic]` alone would still
/// pass if the scraper panicked on everything.
#[test]
fn rejected_form_rows_pairs_the_two_columns_and_ignores_annotations() {
    let markdown = format!(
        "```{REJECTED_TAG}\n// an annotation, not a row\nbox(0, 0, 0)  {ROW_SEPARATOR}  box(0mm, 0mm, 0mm)\n```\n"
    );
    assert_eq!(
        rejected_form_rows(&markdown, REJECTED_TAG),
        vec![("box(0, 0, 0)".to_string(), "box(0mm, 0mm, 0mm)".to_string())]
    );
}

/// `squash_whitespace` equates the spellings a doc author may legitimately
/// choose, and still distinguishes different forms.
///
/// The second assertion is what stops the fix for the over-strict comparison
/// from over-correcting into a check that passes on any row at all.
#[test]
fn squash_whitespace_equates_respacings_but_not_different_forms() {
    for spelling in ["box(0,0,0)", "box( 0, 0, 0 )", "box(0,\n0,\t0)"] {
        assert_eq!(
            squash_whitespace(spelling),
            squash_whitespace(BARE_ZERO_FORM),
            "`{spelling}` is the same row as `{BARE_ZERO_FORM}`, respaced"
        );
    }
    assert_ne!(
        squash_whitespace("box(0mm, 0mm, 0mm)"),
        squash_whitespace(BARE_ZERO_FORM)
    );
}

/// `wrap_form` really BINDS `g`, which is the precondition every documented row
/// naming `g` depends on.
///
/// Asserted through the compiler, not by string match: a wrapper that emitted a
/// module where `g` was undefined would make `mirror(g, …)` "reject" for name
/// resolution rather than for the units gate, and
/// `documented_rejected_forms_are_actually_rejected` would pass for entirely the
/// wrong reason. The dimensioned form must compile CLEAN for that to be ruled
/// out.
#[test]
fn wrap_form_binds_g_so_a_row_naming_it_compiles_clean() {
    assert_module_compiles(
        UNITS_CHUNK_PATH,
        "wrap_form unit test: a dimensioned form referencing `g`",
        &wrap_form("mirror(g, 0mm, 0mm, 0mm, 1, 0, 0)"),
    );
}
