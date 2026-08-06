//! Doc-chunk truth test for the "## Option Type" section of the `enums`
//! language-reference chunk (`crates/reify-mcp/src/tools/chunks/enums.md`),
//! served verbatim as data to design agents by the `reify_language_reference`
//! MCP tool (`crates/reify-mcp/src/tools/reference.rs`, via
//! `language_chunks::get_chunk`).
//!
//! Unlike its sibling `geometry_chunk_smoke.rs` — which curates fixtures
//! because `geometry.md` intermixes non-compilable schematic notation with
//! real call forms — this module SCRAPES the fenced `.ri` source out of the
//! served bytes and runs it through the real compiler. The Option Type
//! section is a single complete example, so scraping is feasible here, and it
//! is strictly stronger: a curated fixture can drift away from the doc it
//! claims to pin (which is exactly how the `some(c) => base + c.thickness`
//! defect survived), whereas scraping makes the served bytes themselves the
//! thing under test. This is the mechanism
//! `docs/prds/v0_6/doc-chunk-truth-enforcement.md` §Sketch (a) specifies for
//! task β, applied narrowly to the one section task #6047 owns.
//!
//! Lives in `reify-compiler` (not `reify-mcp`, where `enums.md` itself lives)
//! because `reify-mcp` does not depend on `reify-compiler` and so cannot
//! invoke `compile_source_with_stdlib`; `language_chunks::get_chunk` is
//! unreachable from here in turn, so the chunk is read by path — the same
//! cross-crate placement #5347 and #5364 arrived at independently.

use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The served `enums` chunk, read from `reify-mcp`'s source tree at compile
/// time. `include_str!` (not `fs::read_to_string`) so a moved/renamed chunk
/// file is a build error rather than a runtime panic.
const ENUMS_CHUNK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks/enums.md"
));

/// Heading that opens the section under test.
const SECTION_HEADING: &str = "## Option Type";

/// The lines of the `## Option Type` section body: everything after the
/// heading up to the next `## ` heading (or EOF).
///
/// Panics if the heading is absent, so renaming the section fails loudly here
/// instead of silently reducing every test in this module to a vacuous pass.
fn option_section_lines() -> Vec<&'static str> {
    let all: Vec<&str> = ENUMS_CHUNK.lines().collect();
    let start = all
        .iter()
        .position(|l| l.trim_end() == SECTION_HEADING)
        .unwrap_or_else(|| {
            panic!(
                "`{SECTION_HEADING}` section not found in \
                 crates/reify-mcp/src/tools/chunks/enums.md — the section was renamed or \
                 removed. This module pins that section's examples against the real \
                 compiler; re-point it at the new heading rather than deleting it."
            )
        });
    let rest = &all[start + 1..];
    let end = rest
        .iter()
        .position(|l| l.starts_with("## "))
        .unwrap_or(rest.len());
    rest[..end].to_vec()
}

/// The bodies of every fenced code block inside the `## Option Type` section,
/// in document order. A fence delimiter is a line whose trimmed form starts
/// with three backticks; the opening delimiter may carry any info string.
///
/// Panics if the section contains no fence at all, so a section that loses its
/// example does not silently pass.
fn option_section_fences() -> Vec<String> {
    let mut fences: Vec<String> = Vec::new();
    let mut open: Option<Vec<&str>> = None;
    for line in option_section_lines() {
        if line.trim_start().starts_with("```") {
            match open.take() {
                // Closing delimiter: emit the accumulated body.
                Some(body) => fences.push(body.join("\n")),
                // Opening delimiter: start accumulating.
                None => open = Some(Vec::new()),
            }
        } else if let Some(body) = open.as_mut() {
            body.push(line);
        }
    }
    assert!(
        open.is_none(),
        "unterminated code fence in the `{SECTION_HEADING}` section of enums.md"
    );
    assert!(
        !fences.is_empty(),
        "the `{SECTION_HEADING}` section of enums.md contains no fenced code block — \
         this module exists to pin that example against the compiler, so an empty \
         section is a failure, not a pass"
    );
    fences
}

/// Wrap a scraped fence body in the minimal compilable module shape.
fn as_module(fence: &str) -> String {
    format!("structure def OptionDemo {{\n{fence}\n}}")
}

/// Every fenced example the `## Option Type` section serves must compile with
/// zero `Severity::Error` diagnostics when wrapped in a minimal
/// `structure def OptionDemo { ... }` module.
///
/// This is the gate task #6047 exists to close: the section shipped
/// `match coating { some(c) => base + c.thickness \n none => base }`, which is
/// a hard parse error — the pattern grammar has no positional production
/// (`tree-sitter-reify/grammar.js` `match_pattern`), so `some(c)` cannot be
/// written at all. `compile_source_with_stdlib` panics on parse errors, so the
/// pre-fix failure surfaces as a parse-error panic naming the snippet.
#[test]
fn option_section_fences_compile_clean() {
    for (ordinal, fence) in option_section_fences().iter().enumerate() {
        let compiled = compile_source_with_stdlib(&as_module(fence));
        let errors = errors_only(&compiled);
        assert!(
            errors.is_empty(),
            "`{SECTION_HEADING}` fence #{ordinal} must compile with zero Error diagnostics.\n\
             --- fence source ---\n{fence}\n\
             --- Error diagnostics ---\n{errors:#?}"
        );
    }
}
