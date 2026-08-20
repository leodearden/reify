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

use reify_core::ModulePath;
use reify_core::diagnostics::DiagnosticCode;
use reify_test_support::{
    compile_source_with_stdlib, compile_source_with_stdlib_allow_parse_errors, errors_only,
};

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

/// The renderer prefix the exemplar shows on its parse-layer transcription.
///
/// # This prefix is CLI presentation, not parser output
///
/// The split matters, because a reader of the exemplar sees one string where
/// the codebase has two producers:
///
/// - the **parser** produces the bare message — `syntax error: rad` — at
///   `crates/reify-syntax/src/ts_parser.rs:511` and its sibling ERROR arms
///   (`:2141`, `:2175`, `:2289`), all `format!("syntax error: {}", ...)`;
/// - the **CLI** adds `Parse error: ` when it prints, at
///   `crates/reify-cli/src/main.rs:195` (and the same `eprintln!` at `:254`);
/// - `reify-test-support`'s `parse_errors_as_diagnostics`
///   (`helpers.rs:261-270`) forwards `e.message` verbatim with **no** prefix,
///   so nothing at the library layer ever emits it either.
///
/// A test in the `reify-compiler` crate cannot invoke the CLI, so the prefix is
/// pinned here as a constant and reconstructed onto the live parser message;
/// that the CLI really prints it is pinned end-to-end at
/// `crates/reify-cli/tests/harness_cli/cli_check.rs:51`. Between the two, a
/// rename on either side is caught.
const CLI_PARSE_ERROR_PREFIX: &str = "Parse error";

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
///
/// # Panics
///
/// On a missing marker, on a block that scrapes to zero entries, and on a
/// `-> ` line with no declaration above it. All three are structural drift, and
/// all three would otherwise degrade this module to a vacuous pass rather than
/// a failure — which is the exact defect the module was written to remove. The
/// messages name the file and say what to do; each contains the literal
/// `CANONICAL COPY`, which `scraper_panics_when_the_block_is_absent` keys on.
fn canonical_copy_entries_from(text: &str) -> Vec<TranscribedDiagnostic> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| {
        line.strip_prefix("//")
            .is_some_and(|body| body.trim_start().starts_with(CANONICAL_COPY_MARKER))
    }) else {
        panic!(
            "no `{CANONICAL_COPY_MARKER}` block found in \
             examples/best_practices/angle_crossings.ri — the marker was renamed or the \
             block was removed. This module pins that block's four transcribed \
             diagnostics against the real compiler and parser; without the marker it \
             would scrape nothing and every test here would pass vacuously. Re-point the \
             scraper at the new marker rather than deleting the test."
        )
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
                let Some((_, fragments)) = current.as_mut() else {
                    panic!(
                        "malformed CANONICAL COPY block in \
                         examples/best_practices/angle_crossings.ri: the rendered \
                         diagnostic `{content}` has no declaration above it, so this \
                         module cannot tell which source provokes it. Every `-> ` line \
                         must sit under the declaration it belongs to."
                    )
                };
                fragments.push(rendered.to_string());
            }
        } else if indent == CONTINUATION_INDENT {
            if let Some((_, fragments)) = current.as_mut() {
                fragments.push(content.to_string());
            }
        }
        // Any other indent is prose inside the marker paragraph; ignored.
    }
    flush(&mut current, &mut entries);

    assert!(
        !entries.is_empty(),
        "the `{CANONICAL_COPY_MARKER}` marker is present in \
         examples/best_practices/angle_crossings.ri but its block scraped to zero \
         entries — the block was emptied, or its indentation was reflowed away from the \
         {DECLARATION_INDENT}/{DIAGNOSTIC_INDENT}/{CONTINUATION_INDENT}-column shape this \
         reader expects. Re-point the scraper at the new shape rather than deleting the \
         test: a block that scrapes to nothing pins nothing."
    );
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

/// Collapse every run of whitespace in `text` to a single space.
///
/// Used to key fixtures and expectations off a declaration whose exemplar form
/// carries column-alignment padding (`let   theta`, `arc   :`). The padding is
/// preserved in the scraped entry — a reflow must stay visible in the step-1
/// triple assertion — but it must not be load-bearing for lookup.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The typed [`DiagnosticCode`] a compile-layer entry must carry, derived from
/// the entry's own declaration keyword rather than from its position in the
/// block — so reordering the block cannot silently re-pair a message with the
/// wrong code.
///
/// `crates/reify-compiler/src/entity.rs:577` emits
/// `LetAnnotationTypeMismatch` for an annotated `let`; `:493` emits
/// `ParamDefaultTypeMismatch` for an annotated `param`. The two messages differ
/// only in their tail ("initializer **type** must agree" vs. "initializer
/// **dimension** must agree"), which is exactly the kind of near-collision a
/// message-only assertion can wave through.
fn expected_code_for(declaration: &str) -> DiagnosticCode {
    let normalized = normalize_whitespace(declaration);
    if normalized.starts_with("let ") {
        DiagnosticCode::LetAnnotationTypeMismatch
    } else if normalized.starts_with("param ") {
        DiagnosticCode::ParamDefaultTypeMismatch
    } else {
        panic!(
            "CANONICAL COPY declaration `{normalized}` is neither a `let` nor a `param` \
             binding, so this module cannot say which DiagnosticCode it should carry. \
             Extend `expected_code_for` alongside `fixture_for` when the block grows a \
             new declaration form."
        )
    }
}

/// The `s / r` ratio case, shared by the `let` and `param` transcriptions:
/// two `Length` params whose quotient is dimensionless, annotated `Angle`.
const RATIO_LET_FIXTURE: &str = "\
structure S {
  param r : Length = 2mm
  param s : Length = 5mm
  let theta : Angle = s / r
}";

/// As [`RATIO_LET_FIXTURE`], but the annotated binding is a `param` — which is
/// a different compiler check (`entity.rs:486` vs. `:570`) reached by a
/// different path, hence a separate fixture rather than a reused one.
const RATIO_PARAM_FIXTURE: &str = "\
structure S {
  param r : Length = 2mm
  param s : Length = 5mm
  param theta : Angle = s / r
}";

/// The arc-length case: `Length * Angle` is `Scalar[m·rad]`, not a `Length`.
const ARC_LENGTH_FIXTURE: &str = "\
structure S {
  param r : Length = 2mm
  param theta : Angle = 0.5rad
  let arc : Length = r * theta
}";

/// The spaced-unit-literal case. Unlike the other three this is rejected at the
/// PARSE layer, so it never reaches the type checker at all.
const SPACED_UNIT_LITERAL_FIXTURE: &str = "\
structure S {
  let x = 2.5 * 1 rad
}";

/// The minimal source that provokes the diagnostic transcribed for
/// `declaration`.
///
/// These are the fixtures measured against a built `reify check` when the
/// exemplar's wording was verified, minus their `module` line: the helpers here
/// parse under `ModulePath::single("test")`, so a `module` header naming
/// anything else injects an unrelated `E_MODULE_PATH_MISMATCH` error into
/// `errors_only`. The absent header costs only a `W_MODULE_DECL_MISSING`
/// *warning*, which `errors_only` filters out.
///
/// Keyed on the whitespace-NORMALIZED declaration, because the exemplar pads
/// its declarations for column alignment (`let   theta`, `arc   :`) and that
/// padding must not be load-bearing for lookup.
///
/// # Panics
///
/// On a declaration with no fixture — deliberately, and this is the point:
/// adding a fifth diagnostic to the CANONICAL COPY block without adding its
/// fixture must fail loudly rather than be skipped silently. A gate that
/// quietly ignores what it does not recognise is the failure mode this whole
/// module exists to remove.
fn fixture_for(declaration: &str) -> &'static str {
    let normalized = normalize_whitespace(declaration);
    match normalized.as_str() {
        "let theta : Angle = s / r" => RATIO_LET_FIXTURE,
        "param theta : Angle = s / r" => RATIO_PARAM_FIXTURE,
        "let arc : Length = r * theta" => ARC_LENGTH_FIXTURE,
        "let x = 2.5 * 1 rad" => SPACED_UNIT_LITERAL_FIXTURE,
        other => panic!(
            "the CANONICAL COPY block in examples/best_practices/angle_crossings.ri \
             transcribes a diagnostic for `{other}`, but this module has no fixture that \
             provokes it, so it would go unchecked. Add one to `fixture_for` (and its \
             row to TRANSCRIBED) in the same diff that added the transcription."
        ),
    }
}

/// Every compile-layer transcription equals what the compiler actually emits.
///
/// This is the assertion the exemplar's "compile-gated" claim was making on
/// credit. Each `error:` entry is scraped, its minimal fixture compiled for
/// real, and the produced `Diagnostic::message` compared for EXACT equality —
/// not `contains`, which would let a truncated or reworded transcription pass.
#[test]
fn transcribed_compile_diagnostics_match_the_real_compiler() {
    let compile_layer: Vec<TranscribedDiagnostic> = canonical_copy_entries()
        .into_iter()
        .filter(|entry| entry.renderer == "error")
        .collect();
    assert_eq!(
        compile_layer.len(),
        3,
        "expected three compile-layer transcriptions in the CANONICAL COPY block; \
         got {compile_layer:#?}"
    );

    for entry in &compile_layer {
        let module = compile_source_with_stdlib(fixture_for(&entry.declaration));
        let produced = errors_only(&module);
        let messages: Vec<&str> = produced.iter().map(|d| d.message.as_str()).collect();

        let matched = produced.iter().find(|d| d.message == entry.message);
        let Some(matched) = matched else {
            panic!(
                "the CANONICAL COPY block in examples/best_practices/angle_crossings.ri \
                 transcribes, for `{}`:\n  {}\nbut the compiler produced:\n  {:#?}\n\
                 The compiler wording changed: re-measure with `reify check` and update \
                 the CANONICAL COPY block in examples/best_practices/angle_crossings.ri, \
                 then let crates/reify-mcp/src/tools/chunks/units.md follow — that is the \
                 precedence the exemplar itself states.",
                entry.declaration, entry.message, messages
            )
        };

        let expected_code = expected_code_for(&entry.declaration);
        assert_eq!(
            matched.code,
            Some(expected_code),
            "`{}` produces the transcribed wording but under code {:?}, not {:?}. Same \
             text, different cause — the transcription is stale even though it still \
             matches by string.",
            entry.declaration,
            matched.code,
            expected_code
        );
    }
}

/// The one parse-layer transcription equals what the real parser emits, and
/// the exemplar's rendered form round-trips from it.
///
/// This entry is structurally different from the other three and the
/// difference is easy to get wrong: `Parse error: ` is **not** parser output.
/// The parser produces the bare `syntax error: rad`
/// (`crates/reify-syntax/src/ts_parser.rs:511` and its sibling ERROR arms), and
/// the prefix is CLI presentation added at `crates/reify-cli/src/main.rs:195`.
/// So this test asserts the bare message against the library layer and then
/// RECONSTRUCTS the exemplar's rendered line from it — rather than expecting a
/// prefix the library never emits.
#[test]
fn transcribed_parse_diagnostic_matches_the_real_parser() {
    // Filter by "not a compile-layer entry" rather than by the prefix itself,
    // so the prefix assertion below stays non-vacuous: if the CLI renamed its
    // prefix and the exemplar followed, this still selects the same entry and
    // the assertion fires.
    let parse_layer: Vec<TranscribedDiagnostic> = canonical_copy_entries()
        .into_iter()
        .filter(|entry| entry.renderer != "error")
        .collect();
    assert_eq!(
        parse_layer.len(),
        1,
        "expected exactly one parse-layer transcription in the CANONICAL COPY block; \
         got {parse_layer:#?}"
    );
    let entry = &parse_layer[0];

    assert_eq!(
        entry.renderer, CLI_PARSE_ERROR_PREFIX,
        "the exemplar renders this entry with the `{}` prefix, which is emitted by \
         crates/reify-cli/src/main.rs:195 — not by the parser. If the CLI renamed it, \
         the transcription at examples/best_practices/angle_crossings.ri is now showing \
         a form no `reify check` run produces.",
        CLI_PARSE_ERROR_PREFIX
    );

    let source = fixture_for(&entry.declaration);
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    let messages: Vec<&str> = parsed.errors.iter().map(|e| e.message.as_str()).collect();
    assert!(
        !parsed.errors.is_empty(),
        "`{}` is transcribed as a parse failure, but it parsed clean. If the spaced \
         unit literal became legal, the CANONICAL COPY block in \
         examples/best_practices/angle_crossings.ri must be rewritten in this same diff.",
        entry.declaration
    );

    let Some(matched) = parsed.errors.iter().find(|e| e.message == entry.message) else {
        panic!(
            "the CANONICAL COPY block transcribes, for `{}`:\n  {}\nbut the parser \
             produced:\n  {:#?}\nThe parser wording changed: re-measure with \
             `reify check` and update the CANONICAL COPY block in \
             examples/best_practices/angle_crossings.ri, then let \
             crates/reify-mcp/src/tools/chunks/units.md follow.",
            entry.declaration, entry.message, messages
        )
    };

    // Round trip: the parser's bare message plus the CLI's presentation prefix
    // must reconstruct, byte for byte, the line the exemplar actually shows.
    // This is what justifies transcribing a CLI-rendered form in a file whose
    // gate runs at the library layer.
    let rendered = format!(
        "{DIAGNOSTIC_ARROW}{CLI_PARSE_ERROR_PREFIX}: {}",
        matched.message
    );
    assert!(
        ANGLE_CROSSINGS_EXEMPLAR.contains(&rendered),
        "the exemplar should transcribe `{rendered}` — the parser's own message under \
         the CLI's prefix — but that exact text is not in the file."
    );

    // And the rejection must survive to the caller as a Severity::Error, i.e. a
    // reader who runs this source actually sees it. Mirrors
    // `enums_chunk_option_smoke.rs:571-581`: the plain
    // `compile_source_with_stdlib` panics on parse errors, so a negative test
    // must use the `_allow_parse_errors` variant to observe them rather than
    // die on them.
    let compiled = compile_source_with_stdlib_allow_parse_errors(source);
    assert!(
        !errors_only(&compiled).is_empty(),
        "the parse error must reach the caller as a Severity::Error diagnostic; \
         parse errors seen: {messages:#?}"
    );
}

// ---------------------------------------------------------------------------
// Anti-vacuity guards
//
// These matter more here than in a typical suite, because this module exists
// precisely because a silent no-op had been passing for a gate. A doc-truth
// test that quietly scrapes nothing is not a weaker gate — it is the same
// absence of one, wearing a green tick.
// ---------------------------------------------------------------------------

/// Renaming or deleting the block must fail loudly, not scrape to nothing.
///
/// Without this, a rename of the marker reduces every other test in this module
/// to a vacuous pass: zero entries scraped, zero comparisons made, all green.
/// The input is the REAL exemplar with only its marker renamed, so the guard is
/// exercised against the exact drift it is meant to catch rather than against
/// unrelated text.
///
/// The fallback `panic!` below must NOT contain the `expected` substring, or it
/// would satisfy `should_panic` on its own and this guard would pass without
/// the scraper ever having guarded anything — the very vacuity it is here to
/// prevent, reproduced inside the test that prevents it.
#[test]
#[should_panic(expected = "CANONICAL COPY")]
fn scraper_panics_when_the_block_is_absent() {
    let renamed = ANGLE_CROSSINGS_EXEMPLAR.replace(CANONICAL_COPY_MARKER, "AUTHORITATIVE COPY:");
    let entries = canonical_copy_entries_from(&renamed);
    panic!(
        "the scraper returned {} entries from an exemplar whose marker was renamed, \
         instead of failing loudly — every other test in this module would have silently \
         gone vacuous",
        entries.len()
    );
}

/// The negative control: a reworded transcription really does fail.
///
/// Every other assertion in this module compares scraped text to compiler
/// output, and a scraper bug that dropped the message body (or a comparison
/// that accidentally compared scraped text to itself) would make all of them
/// pass unconditionally. This perturbs the exemplar's wording — `must agree` to
/// `must concur`, a genuinely different string — and proves that the equality
/// assertion in `transcribed_compile_diagnostics_match_the_real_compiler` would
/// then have gone red.
#[test]
fn scraper_discriminates_a_reworded_diagnostic() {
    let perturbed = ANGLE_CROSSINGS_EXEMPLAR.replace("must agree", "must concur");
    assert_ne!(
        perturbed, ANGLE_CROSSINGS_EXEMPLAR,
        "the perturbation must actually change the exemplar's text, or this control \
         proves nothing. If the diagnostics no longer end in `must agree`, pick a \
         substring they do contain."
    );

    let entries = canonical_copy_entries_from(&perturbed);
    assert_eq!(
        entries.len(),
        TRANSCRIBED.len(),
        "perturbing the wording must not change how many entries scrape; got {entries:#?}"
    );

    let mut still_matching: Vec<&str> = Vec::new();
    let mut discriminated = 0usize;
    for entry in entries.iter().filter(|entry| entry.renderer == "error") {
        let module = compile_source_with_stdlib(fixture_for(&entry.declaration));
        if errors_only(&module)
            .iter()
            .any(|d| d.message == entry.message)
        {
            still_matching.push(entry.declaration.as_str());
        } else {
            discriminated += 1;
        }
    }

    assert!(
        discriminated > 0,
        "a reworded transcription still matched the compiler for every entry \
         ({still_matching:?}), so the equality assertions in this module are not \
         discriminating — they would pass against text the compiler never emits."
    );
}
