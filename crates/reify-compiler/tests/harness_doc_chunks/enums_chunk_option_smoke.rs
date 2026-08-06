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

use reify_core::dimension::DimensionVector;
use reify_ir::Value;
use reify_test_support::{
    cell_value, collect_errors, compile_source_with_stdlib, errors_only, make_engine,
    parse_and_compile_with_stdlib,
};

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

// --- Evaluation truth -------------------------------------------------------
//
// Compiling clean is NOT sufficient. Binder-less `some`/`none` match arms also
// compile clean and still evaluate to `undef` (`Option` is `Value::Option`, not
// `Value::Enum`, and eval's match-arm selection fires only on `Value::Enum`), so
// a parse/compile-only gate would happily accept a rewrite that produces
// nothing. The assertions below pin the VALUES the section documents, and reject
// `Value::Undef` explicitly and first — `undef` is Reify's universal
// quiet-failure value, so a loose comparison could otherwise pass by accident.

/// Compile `source` WITH the stdlib prelude and evaluate it.
///
/// Deliberately not `reify_test_support::eval_source`: that helper routes
/// through `parse_and_compile` → `compile_source`, i.e. `reify_compiler::compile`
/// WITHOUT the stdlib, so the prelude recovery combinators would not resolve.
/// They would not error either — `expr.rs`'s permissive unresolved-function
/// fallback types an `>= 1`-arg call from its first argument's `result_type` and
/// emits no diagnostic — so the test would "pass" against nothing. This mirrors
/// the stdlib-aware pipeline `crates/reify-eval/tests/fallback_recovery_e2e.rs`
/// uses for these same combinators.
fn eval_module(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(eval_errors.is_empty(), "eval-phase errors: {eval_errors:?}");
    result
}

/// Assert `actual` is a LENGTH scalar worth `expected_mm` millimetres.
#[track_caller]
fn assert_length_mm(actual: &Value, expected_mm: f64, label: &str) {
    assert!(
        !matches!(actual, Value::Undef),
        "{label}: evaluated to `undef` — the documented form was accepted but \
         produced no value"
    );
    match actual {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "{label}: expected a Length, got dimension {dimension:?}"
            );
            let expected_si = expected_mm * 1e-3;
            assert!(
                (si_value - expected_si).abs() <= 1e-12,
                "{label}: expected {expected_mm}mm ({expected_si} m), got {si_value} m"
            );
        }
        other => panic!("{label}: expected a Length scalar, got {other:?}"),
    }
}

/// Assert `actual` is `some(<Length>)` worth `expected_mm` millimetres.
#[track_caller]
fn assert_some_length_mm(actual: &Value, expected_mm: f64, label: &str) {
    assert!(
        !matches!(actual, Value::Undef),
        "{label}: evaluated to `undef` — the documented form was accepted but \
         produced no value"
    );
    match actual {
        Value::Option(Some(inner)) => assert_length_mm(inner, expected_mm, label),
        other => panic!("{label}: expected `some(<Length>)`, got {other:?}"),
    }
}

/// Assert `actual` is exactly `Value::Bool(expected)`.
#[track_caller]
fn assert_bool(actual: &Value, expected: bool, label: &str) {
    assert!(
        !matches!(actual, Value::Undef),
        "{label}: evaluated to `undef` — the documented form was accepted but \
         produced no value"
    );
    assert_eq!(
        actual,
        &Value::Bool(expected),
        "{label}: expected Bool({expected})"
    );
}

/// How the section's example declares its present `Option` subject, and the
/// literal that swaps it for the absent one.
const SOME_SUBJECT: &str = "some(0.25mm)";
const NONE_SUBJECT: &str = "none";

/// The section's own fence with its `Option` subject swapped to `none`, so both
/// arms of every combinator are exercised against the SAME documented source.
fn none_path_variant(fence: &str) -> String {
    let swapped = fence.replacen(SOME_SUBJECT, NONE_SUBJECT, 1);
    assert_ne!(
        swapped, *fence,
        "the `{SECTION_HEADING}` example no longer declares its Option subject as \
         `{SOME_SUBJECT}`, so the none-path assertions would silently re-test the \
         some path. Re-point SOME_SUBJECT at the example's new subject."
    );
    swapped
}

/// The `## Option Type` example must not merely compile — it must evaluate to
/// the values the section claims, on BOTH the `some(x)` and the `none` path.
///
/// These are the assertions that separate a working documented form from one
/// that is accepted and silently yields `undef`; they are precisely what would
/// have caught rewriting the broken `some(c) => ...` arm into a binder-less
/// `some`/`none` arm.
#[test]
fn option_section_example_evaluates_to_documented_values() {
    let fences = option_section_fences();
    let fence = &fences[0];

    // --- subject = some(0.25mm), base = 1mm ---
    let present = eval_module(&as_module(fence));
    assert_length_mm(
        &cell_value(&present, "OptionDemo", "bare"),
        0.25,
        "some-path `bare` = unwrap_or(coating, 0mm)",
    );
    assert_bool(
        &cell_value(&present, "OptionDemo", "has"),
        true,
        "some-path `has` = is_some(coating)",
    );
    assert_some_length_mm(
        &cell_value(&present, "OptionDemo", "alt"),
        0.25,
        "some-path `alt` = or_else(coating, some(0.05mm)) keeps the present subject",
    );
    assert_length_mm(
        &cell_value(&present, "OptionDemo", "total"),
        1.25,
        "some-path `total` = map_or(coating, base, |c: Length| base + c) — the only \
         combinator that binds the payload to a name",
    );

    // --- subject = none, base = 1mm ---
    let absent = eval_module(&as_module(&none_path_variant(fence)));
    assert_length_mm(
        &cell_value(&absent, "OptionDemo", "bare"),
        0.0,
        "none-path `bare` = unwrap_or(none, 0mm) falls back to the default",
    );
    assert_bool(
        &cell_value(&absent, "OptionDemo", "has"),
        false,
        "none-path `has` = is_some(none)",
    );
    assert_some_length_mm(
        &cell_value(&absent, "OptionDemo", "alt"),
        0.05,
        "none-path `alt` = or_else(none, some(0.05mm)) substitutes the alternative",
    );
    assert_length_mm(
        &cell_value(&absent, "OptionDemo", "total"),
        1.0,
        "none-path `total` = map_or(none, base, ...) returns base without applying f",
    );
}
