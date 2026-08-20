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
