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

/// Marker line that opens the block this module scrapes.
const CANONICAL_COPY_MARKER: &str = "CANONICAL COPY:";

/// Column at which a *declaration* line sits inside the comment body (i.e.
/// after the leading `//` has been stripped): `//     let   theta : ...`.
const DECLARATION_INDENT: usize = 5;

/// Column at which the `-> ` that opens a rendered diagnostic sits:
/// `//         -> error: ...`.
const DIAGNOSTIC_INDENT: usize = 9;

/// Column at which a wrapped continuation of a rendered diagnostic sits:
/// `//            initializer evaluates to ...`.
const CONTINUATION_INDENT: usize = 12;

/// Token that introduces a rendered diagnostic under its declaration.
const DIAGNOSTIC_ARROW: &str = "-> ";

/// One `(declaration, rendered diagnostic)` pair transcribed in the exemplar's
/// `CANONICAL COPY` block, with the renderer prefix already split off.
///
/// `declaration` keeps the exemplar's column-alignment padding verbatim;
/// `message` has the block's cosmetic line-wrapping normalized away (see
/// [`canonical_copy_entries_from`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscribedDiagnostic {
    declaration: String,
    renderer: String,
    message: String,
}

/// Scrape the `CANONICAL COPY` block out of `text`.
///
/// The block's shape (measured at `examples/best_practices/angle_crossings.ri`
/// lines 34-53) is regular, and this reader is deliberately strict about it so
/// that a reflow is *visible* rather than silently absorbed:
///
/// - the block opens at the line whose comment body starts with
///   [`CANONICAL_COPY_MARKER`];
/// - a body indented [`DECLARATION_INDENT`] opens a new entry's declaration;
/// - a body indented [`DIAGNOSTIC_INDENT`] and beginning [`DIAGNOSTIC_ARROW`]
///   opens that entry's rendered diagnostic;
/// - a body indented [`CONTINUATION_INDENT`] continues it;
/// - the block ends at the first blank comment line *after* at least one entry
///   has been collected — the marker paragraph itself is followed by a blank
///   comment line, which must not terminate the scan — or at the first line
///   that is not a `//` comment at all.
///
/// Continuation fragments are joined with a single space and internal
/// whitespace runs are collapsed, so the comment's wrap points (cosmetic)
/// vanish while the text (not cosmetic) survives. Each rendered diagnostic is
/// then split once on the first `": "` into its renderer prefix (`error`,
/// `Parse error`) and the message body.
fn canonical_copy_entries_from(text: &str) -> Vec<TranscribedDiagnostic> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| {
        line.strip_prefix("//")
            .is_some_and(|body| body.trim_start().starts_with(CANONICAL_COPY_MARKER))
    }) else {
        return Vec::new();
    };

    let mut entries: Vec<TranscribedDiagnostic> = Vec::new();
    // The entry under construction: its declaration, plus the rendered
    // diagnostic's fragments in wrap order.
    let mut current: Option<(String, Vec<String>)> = None;

    for line in &lines[start + 1..] {
        let Some(body) = line.strip_prefix("//") else {
            break;
        };
        let content = body.trim();
        if content.is_empty() {
            // Blank comment line. Before the first entry this is the gap
            // between the marker paragraph and the block; after it, the end.
            if entries.is_empty() && current.is_none() {
                continue;
            }
            break;
        }
        let indent = body.len() - body.trim_start().len();

        if indent == DECLARATION_INDENT && !content.starts_with(DIAGNOSTIC_ARROW) {
            flush(&mut current, &mut entries);
            current = Some((content.to_string(), Vec::new()));
        } else if indent == DIAGNOSTIC_INDENT {
            if let Some(rendered) = content.strip_prefix(DIAGNOSTIC_ARROW) {
                if let Some((_, fragments)) = current.as_mut() {
                    fragments.push(rendered.to_string());
                }
            }
        } else if indent == CONTINUATION_INDENT {
            if let Some((_, fragments)) = current.as_mut() {
                fragments.push(content.to_string());
            }
        }
        // Any other indent is prose inside the marker paragraph; ignored.
    }
    flush(&mut current, &mut entries);
    entries
}

/// Finish the entry under construction (if any) and append it to `entries`.
///
/// Splitting the rendered diagnostic here — rather than at each fragment —
/// is what lets the renderer prefix survive a wrap: `error: ` is only ever
/// recoverable from the *joined* text.
fn flush(current: &mut Option<(String, Vec<String>)>, entries: &mut Vec<TranscribedDiagnostic>) {
    let Some((declaration, fragments)) = current.take() else {
        return;
    };
    let rendered = fragments.join(" ");
    let rendered = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    let (renderer, message) = match rendered.split_once(": ") {
        Some(split) => split,
        // No renderer prefix at all. Recorded rather than dropped, so the
        // step-1 triple assertion reports it against the expected wording
        // instead of the entry vanishing from the count.
        None => ("", rendered.as_str()),
    };
    entries.push(TranscribedDiagnostic {
        declaration,
        renderer: renderer.to_string(),
        message: message.to_string(),
    });
}

/// The four entries of the real exemplar's `CANONICAL COPY` block.
fn canonical_copy_entries() -> Vec<TranscribedDiagnostic> {
    canonical_copy_entries_from(ANGLE_CROSSINGS_EXEMPLAR)
}

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
