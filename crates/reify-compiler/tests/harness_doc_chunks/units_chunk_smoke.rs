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

use reify_core::units::LENGTH_MIGRATION_HINT;
use reify_test_support::{compile_source_with_stdlib, errors_only};

use crate::geometry_chunk_smoke::{
    assert_module_compiles, called_names, cited_source_paths, phantom_name_panic, registry_family,
    reify_tagged_fences, resolve_cited_path, source_files_by_basename, strip_reify_comments,
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

/// Minimum rows the eval-only block must carry. The exact live set: `mirror`,
/// `helix` and `polygon`, the three constructors with no compile-layer LENGTH
/// slot that the chunk names by hand.
const MINIMUM_EVAL_ONLY_ROWS: usize = 3;

/// Minimum rows the rejected-forms block must carry, and the anti-vacuity floor.
///
/// The EXACT set the chunk is required to document, not a round number under it:
/// bare dimensions, the D1 bare-zero row, the mirror row that doubles as the
/// legitimately-bare illustration, and a modify-op row. At a lower floor any one
/// of those could be deleted while this still passed — the regression these
/// floors exist to catch. Raise it with the block; never lower it to go green.
const MINIMUM_REJECTED_ROWS: usize = 4;

/// The bare-zero form PRD decision D1 refuses to special-case. Pinned by name so
/// deleting that row from the chunk is RED at the doc surface, not merely
/// untested.
const BARE_ZERO_FORM: &str = "box(0, 0, 0)";

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
    format!("structure def RejectedForm {{\n    let g = box(10mm, 10mm, 10mm)\n    let subject = {form}\n}}")
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
    let named: Vec<&String> = messages
        .iter()
        .filter(|m| m.contains(" argument expects Length"))
        .collect();
    assert!(
        !named.is_empty(),
        "{UNITS_CHUNK_PATH} documents `{form}` as rejected at a LENGTH argument slot, but no \
         Error message contains ` argument expects Length`. Diagnostics seen: {messages:?}"
    );
    for message in &named {
        let arg = message
            .split(" argument expects Length")
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .next_back()
            .unwrap_or_default();
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

    let normalize = |form: &str| form.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        rows.iter()
            .any(|(rejected, _)| normalize(rejected) == normalize(BARE_ZERO_FORM)),
        "{UNITS_CHUNK_PATH}'s ```{REJECTED_TAG} block no longer documents `{BARE_ZERO_FORM}`. \
         PRD decision D1 is that bare `0` gets NO special case, and it is the one an author \
         assumes is exempt, so it is the row the table most needs. Rows seen: {rows:?}"
    );

    assert_rejected_as_documented(BARE_ZERO_FORM);
}

/// Cites units.md must carry, as the anti-vacuity floors for
/// [`cited_test_paths_in_the_units_chunk_resolve`].
///
/// The EXACT live counts, not round numbers under them — the reasoning
/// `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve` spells out:
/// at a floor below the live count one whole cite can be deleted while the check
/// stays green, which is precisely the regression the floor claims to catch. Six
/// `<path>::<fn>` cites across three `.rs` files (this module's own self-cites,
/// plus the primitive-profile and modify-sweep eval pins), and one `.ri`
/// exemplar (`angle_crossings.ri`, which predates this task).
///
/// Raise these WITH the chunk when a cite is added. Never lower one to go green:
/// a lowered floor is a SYNC row that has quietly stopped claiming anything.
const MINIMUM_FN_CITES: usize = 6;
const MINIMUM_RS_FILES: usize = 3;
const MINIMUM_RI_FILES: usize = 1;

/// Every test units.md cites as PINNING a claim must still exist.
///
/// Twin of `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve`,
/// built on the same three helpers rather than a new path resolver. Both halves
/// are checked: the cited file resolves against the tree, and it declares the
/// cited `fn`.
///
/// THIS IS WHAT MAKES A "PINNED" ROW HONEST. units.md's check-visibility note
/// marks some claims PINNED and others UNPINNED, and a reader is invited to
/// trust the distinction. Without this test a PINNED row survives the deletion
/// or rename of the test it names, and the chunk — served verbatim to the in-GUI
/// assistant — goes on asserting a guarantee that no longer exists. A dangling
/// cite is not a broken link; it is a false claim.
///
/// SCOPE — an EXISTENCE check, not a semantic one. It cannot tell that a
/// still-named test stopped asserting what the row claims, it says nothing about
/// rows marked UNPINNED, and it does not verify the fn is a `#[test]`. What it
/// buys is that every `path::fn` cite in the chunk resolves to something real.
#[test]
fn cited_test_paths_in_the_units_chunk_resolve() {
    let markdown = read_chunk();
    let index = source_files_by_basename();
    let cites = cited_source_paths(&markdown);

    // Keyed by the RESOLVED path, not the cite token: a file cited both by bare
    // basename and by full repo-relative path must count ONCE, or a floor could
    // stay satisfied by a duplicate while a real reference disappeared.
    let mut rs_paths: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    let mut ri_paths: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    let mut fn_cites = 0usize;

    for (path_token, fn_name) in &cites {
        let resolved = resolve_cited_path(path_token, &index).unwrap_or_else(|why| {
            panic!(
                "{UNITS_CHUNK_PATH} cites `{path_token}`, which does not resolve: {why}. The \
                 chunk is served verbatim to the in-GUI assistant and its PINNED/UNPINNED rows \
                 are written to be read as the authority on which claims a real test guards — a \
                 dangling cite is a false claim. Update the cite, or mark the row UNPINNED."
            )
        });

        if path_token.ends_with(".ri") {
            ri_paths.insert(resolved.clone());
        } else {
            rs_paths.insert(resolved.clone());
        }

        let Some(fn_name) = fn_name else { continue };
        fn_cites += 1;
        let body = std::fs::read_to_string(&resolved)
            .unwrap_or_else(|e| panic!("{resolved:?} must be readable ({e})"));
        assert!(
            body.contains(&format!("fn {fn_name}(")),
            "{UNITS_CHUNK_PATH} cites `{path_token}::{fn_name}` as pinning one of its claims, \
             but {resolved:?} declares no `fn {fn_name}(`. The test was renamed or deleted, so \
             that row now claims a pin that does not exist. Re-point the cite, or downgrade the \
             row to UNPINNED."
        );
    }

    // Anti-vacuity. Reformatting a SYNC block into a shape this scan cannot read
    // — a path wrapped across two lines is exactly invisible here — would empty
    // the loop above and pass.
    assert!(
        fn_cites >= MINIMUM_FN_CITES,
        "only {fn_cites} `<path>::<fn>` cite(s) found in {UNITS_CHUNK_PATH} — expected at least \
         {MINIMUM_FN_CITES}. CITES MUST BE WRITTEN WHOLE ON ONE LINE, never wrapped and never \
         tabulated into a two-column layout; a wrapped path is invisible to this scan. Either \
         the chunk was reformatted into a shape it cannot read, or a row lost its cite while \
         still claiming to pin something. Cites seen: {cites:?}"
    );
    assert!(
        rs_paths.len() >= MINIMUM_RS_FILES,
        "only {} distinct `.rs` FILE(s) cited in {UNITS_CHUNK_PATH} (distinct after resolution — \
         the same file cited two ways counts once), expected at least {MINIMUM_RS_FILES}: this \
         chunk's own guard plus the two eval-layer pins the check-visibility note relies on. \
         Losing one of those turns a PINNED row into prose. Cites seen: {cites:?}",
        rs_paths.len()
    );
    assert!(
        ri_paths.len() >= MINIMUM_RI_FILES,
        "only {} distinct `.ri` example FILE(s) cited in {UNITS_CHUNK_PATH}, expected at least \
         {MINIMUM_RI_FILES} — `examples/best_practices/angle_crossings.ri` is the compile-gated \
         exemplar this chunk defers to as the canonical copy of the angle-crossing diagnostics, \
         so losing the cite orphans that deferral. Cites seen: {cites:?}",
        ri_paths.len()
    );
}

/// The constructors units.md names as caught only at build/eval time really are
/// INVISIBLE to the compile layer.
///
/// The complement of `documented_rejected_forms_are_actually_rejected`, and the
/// executable half of the chunk's check-visibility note. `mirror`, `helix` and
/// `polygon` have no `CheckableArg` LENGTH slot — `polygon` deliberately so, per
/// `builtin_signatures.rs`'s own
/// `polygon_stays_slot_free_because_its_positions_are_arity_open` — so a bare
/// number in one of them compiles CLEAN and is rejected later, which is why
/// `reify check` prints `error:` for these and still exits 0.
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
         {MINIMUM_EVAL_ONLY_ROWS} (`mirror`, `helix`, `polygon`). Either the \
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
