//! Doc-truth test for the `CANONICAL COPY` block in the best-practices
//! exemplar `examples/best_practices/angle_crossings.ri`.
//!
//! That block transcribes four compiler diagnostics verbatim and declares
//! itself the canonical copy — "the verbatim compiler wording lives here, in
//! the compile-gated file" — with
//! `crates/reify-mcp/src/tools/chunks/units.md` deferring to it as the
//! authority. But the only gate over the exemplar,
//! `crates/reify-compiler/tests/examples_smoke.rs`, checks the POSITIVE path
//! (the file parses and produces zero `Severity::Error` diagnostics). The four
//! transcriptions sit inside a `//` comment, which nothing executes, so
//! "compile-gated" was doing work no gate did. This module is that gate.
//!
//! # Mechanism: scrape, don't curate
//!
//! Like its sibling `enums_chunk_option_smoke.rs` — and unlike
//! `geometry_chunk_smoke.rs`, which must curate because its chunk intermixes
//! schematic notation with real call forms — this module SCRAPES the
//! transcriptions out of the exemplar's own bytes and compares them to what
//! the real compiler emits. A curated fixture can drift away from the doc it
//! claims to pin; scraping makes the documented bytes themselves the thing
//! under test. Reword a diagnostic in `reify-compiler` and this test goes red,
//! instead of the exemplar rotting silently.
//!
//! Lives in `reify-compiler` because that is the crate that emits three of the
//! four diagnostics and the one that can invoke `compile_source_with_stdlib`;
//! `reify-mcp` (where `units.md` lives) does not depend on it, which is the
//! same cross-crate placement `enums_chunk_option_smoke.rs` arrived at.
//!
//! # Coverage boundary
//!
//! What is pinned here:
//!
//! - the scraped block's shape and content — exactly four entries, each with
//!   the measured declaration / renderer / message triple;
//! - the three compile-layer transcriptions, asserted EQUAL to the real
//!   compiler's `Diagnostic::message` for a minimal fixture, together with the
//!   expected `DiagnosticCode` — `LetAnnotationTypeMismatch` for the two `let`
//!   cases, `ParamDefaultTypeMismatch` for the `param` case — so a
//!   same-text-different-cause regression is caught too;
//! - the one parse-layer transcription, asserted EQUAL to the real parser's
//!   `ParseError::message`, plus the round trip that reconstructs the
//!   exemplar's CLI-rendered `Parse error: ` form;
//! - `units.md`'s single verbatim re-quote of that parse diagnostic, checked
//!   against the SCRAPED canonical copy rather than a literal typed here — one
//!   chain, no independent literal free to drift;
//! - two anti-vacuity guards: a missing block panics loudly, and a reworded
//!   transcription genuinely fails the equality assertion.
//!
//! What is NOT pinned: the exemplar's surrounding prose, its `ANTI-PATTERN`
//! sketch (lines 26-29, which are abbreviated by design, not verbatim), and
//! `units.md`'s paraphrase of the error SHAPE. Those are read, not executed.

/// The best-practices exemplar whose `CANONICAL COPY` block this module pins,
/// read out of the repo's `examples/` tree at compile time.
///
/// `include_str!` (not `fs::read_to_string`) so a moved or renamed exemplar is
/// a BUILD error rather than a runtime panic — the same choice
/// `enums_chunk_option_smoke.rs:58-61` makes for the chunk it pins.
const ANGLE_CROSSINGS_EXEMPLAR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/best_practices/angle_crossings.ri"
));

/// The exemplar's `CANONICAL COPY` block transcribes exactly these four
/// diagnostics, in this order, with this wording.
///
/// This table is the ONE place in the module where the expected text is typed
/// out rather than scraped. It exists so that a silent *deletion* from the
/// block cannot pass: the scraper alone would happily return three entries and
/// every downstream test would still be green. Everything else compares
/// scraped text against the real compiler, never against a literal.
///
/// Each row is `(declaration, renderer, message)`. `declaration` keeps the
/// exemplar's column-alignment padding verbatim (`let   theta`, `arc   :`) —
/// the scraper does not normalize it away, so a reflow of the block is visible
/// here rather than silently absorbed.
const TRANSCRIBED: [(&str, &str, &str); 4] = [
    (
        "let   theta : Angle = s / r",
        "error",
        "let binding 'theta' declared `Scalar[rad]` but its initializer evaluates to \
         `Real`; declared type and initializer type must agree",
    ),
    (
        "param theta : Angle = s / r",
        "error",
        "parameter 'theta' declared `Scalar[rad]` but its initializer evaluates to \
         `Real`; declared type and initializer dimension must agree",
    ),
    (
        "let   arc   : Length = r * theta",
        "error",
        "let binding 'arc' declared `Scalar[m]` but its initializer evaluates to \
         `Scalar[m·rad]`; declared type and initializer type must agree",
    ),
    ("let   x = 2.5 * 1 rad", "Parse error", "syntax error: rad"),
];

/// The `CANONICAL COPY` block scrapes to exactly the four measured entries.
///
/// This is the module's foundation: every other test consumes the scraper's
/// output, so if the block's shape drifts (a reflow, a fifth entry, a deleted
/// one) it must surface HERE, loudly, rather than as a quietly shrinking set
/// of downstream assertions.
#[test]
fn canonical_copy_block_yields_the_four_transcribed_diagnostics() {
    let entries = canonical_copy_entries_from(ANGLE_CROSSINGS_EXEMPLAR);

    assert_eq!(
        entries.len(),
        TRANSCRIBED.len(),
        "the CANONICAL COPY block in examples/best_practices/angle_crossings.ri must \
         transcribe exactly {} diagnostics; scraped {}: {:#?}. If a diagnostic was \
         genuinely added or removed, update TRANSCRIBED here and add its fixture to \
         `fixture_for` in the same diff.",
        TRANSCRIBED.len(),
        entries.len(),
        entries
    );

    for (index, (declaration, renderer, message)) in TRANSCRIBED.iter().enumerate() {
        let entry = &entries[index];
        assert_eq!(
            entry.declaration, *declaration,
            "CANONICAL COPY entry {index}: declaration text drifted"
        );
        assert_eq!(
            entry.renderer, *renderer,
            "CANONICAL COPY entry {index}: renderer prefix drifted"
        );
        assert_eq!(
            entry.message, *message,
            "CANONICAL COPY entry {index}: transcribed message drifted"
        );
    }
}
