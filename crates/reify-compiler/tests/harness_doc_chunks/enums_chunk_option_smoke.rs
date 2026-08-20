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
//!
//! # Coverage boundary
//!
//! What is pinned against the real compiler/evaluator here:
//!
//! - every fenced example the section serves compiles with zero `Error`
//!   diagnostics (`option_section_fences_compile_clean`);
//! - the scraped example's own evaluated values, on BOTH the `some(x)` and the
//!   `none` path (`option_section_example_evaluates_to_documented_values`);
//! - the section's combinator table rows that the scraped example does not
//!   itself exercise — `or_default`, `fallback`, `is_none`, `ok_or` — plus the
//!   "an `undef` subject propagates `undef` through every combinator" claim
//!   (`option_combinator_table_rows_hold_as_documented`,
//!   `undef_subject_propagates_through_every_documented_combinator`);
//! - the two failure modes the "Do not `match` an Option" warning states
//!   (`option_payload_binding_pattern_still_fails_to_parse`,
//!   `binderless_option_match_still_evaluates_to_undef`).
//!
//! What is NOT pinned: the section's prose *wording*, and its citations to
//! stdlib/PRD/example paths. Those are read by humans and agents, not executed;
//! a reader must not assume every sentence in the section is under test.

use reify_core::ModulePath;
use reify_core::dimension::DimensionVector;
use reify_ir::Value;
use reify_test_support::{
    cell_value, collect_errors, compile_source_with_stdlib,
    compile_source_with_stdlib_allow_parse_errors, errors_only, make_engine,
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

/// The BODY of every fenced code block inside the `## Option Type` section, in
/// document order. A fence delimiter is a line whose trimmed form starts with
/// three backticks and is excluded from the body; an opening delimiter may
/// carry any info string, which is deliberately discarded rather than asserted
/// on here — fence-tag discipline is #5477 leaf β's repo-wide `fence_gate.rs`
/// gate, not this module's job. This module's subject is what the fences
/// EVALUATE to, not how they are spelled.
///
/// Panics if the section contains no fence at all, so a section that loses its
/// example does not silently pass.
fn option_section_fences() -> Vec<String> {
    let mut fences: Vec<String> = Vec::new();
    let mut open: Option<Vec<&str>> = None;
    for line in option_section_lines() {
        if line.trim_start().strip_prefix("```").is_some() {
            match open.take() {
                // Closing delimiter: emit the accumulated body.
                Some(body) => fences.push(body.join("\n")),
                // Opening delimiter: start accumulating, info string discarded.
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

/// Assert `actual` is a `Result` enum of variant `variant` carrying exactly one
/// payload field named `field`.
///
/// The field NAME is asserted, not just the variant: the section tells readers
/// `ok_or` yields "a `Result` you *can* match on", and a named-field pattern
/// (`Ok { value: v }`) only matches if the payload key is what the table says.
#[track_caller]
fn assert_result_variant(actual: &Value, variant: &str, field: &str, label: &str) {
    assert!(
        !matches!(actual, Value::Undef),
        "{label}: evaluated to `undef` — the documented form was accepted but \
         produced no value"
    );
    match actual {
        Value::Enum {
            type_name,
            variant: got_variant,
            payload,
        } => {
            assert_eq!(type_name, "Result", "{label}: expected a Result");
            assert_eq!(got_variant, variant, "{label}: wrong Result variant");
            let keys: Vec<&str> = payload.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys,
                vec![field],
                "{label}: expected payload field `{field}` (the name a \
                 `{variant} {{ {field}: v }}` pattern binds against)"
            );
        }
        other => panic!("{label}: expected `Result::{variant}`, got {other:?}"),
    }
}

/// How the section's example declares its present `Option` subject, and the
/// same declaration with the subject absent.
///
/// Anchored to the WHOLE `param` declaration rather than to a bare
/// `some(0.25mm)` literal: the example also passes `some(0.05mm)` to `or_else`,
/// and a future edit could easily make the first textual occurrence of the
/// present-subject literal something other than the subject declaration (an
/// `or_else` alternative, a `let` above the param). A bare-literal swap would
/// then rewrite the wrong occurrence, leaving the "none path" module still
/// carrying a present subject — assertions that fail confusingly, or worse,
/// pass by coincidence.
const SOME_SUBJECT_DECL: &str = "param coating : Option<Length> = some(0.25mm)";
const NONE_SUBJECT_DECL: &str = "param coating : Option<Length> = none";

/// The section's own fence with its `Option` subject swapped to `none`, so both
/// arms of every combinator are exercised against the SAME documented source.
fn none_path_variant(fence: &str) -> String {
    assert_eq!(
        fence.matches(SOME_SUBJECT_DECL).count(),
        1,
        "the `{SECTION_HEADING}` example must declare its Option subject exactly once as \
         `{SOME_SUBJECT_DECL}`; found {} occurrence(s), so the none-path rewrite would \
         either be a no-op (silently re-testing the some path) or ambiguous. Re-point \
         SOME_SUBJECT_DECL at the example's new subject declaration.\n\
         --- fence source ---\n{fence}",
        fence.matches(SOME_SUBJECT_DECL).count()
    );
    let swapped = fence.replace(SOME_SUBJECT_DECL, NONE_SUBJECT_DECL);
    assert_ne!(swapped, *fence, "none-path rewrite was a no-op");
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
    // `option_section_fences_compile_clean` iterates every fence, but the
    // evaluation assertions below are written against exactly one example. If
    // the section ever grows a second fence (e.g. some-path and none-path shown
    // separately), pinning only fence #0 would silently drop half the
    // evaluation truth — and evaluation truth is this module's whole thesis,
    // since binder-less arms compile clean and still yield `undef`.
    assert_eq!(
        fences.len(),
        1,
        "the `{SECTION_HEADING}` section grew a second fenced example; extend the \
         evaluation assertions to cover it rather than leaving fence #0 the only one \
         pinned against the evaluator"
    );
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

// --- The rest of the section's combinator table ------------------------------
//
// The scraped example exercises four of the section's six table rows —
// `unwrap_or`, `or_else`, `is_some`, `map_or`. The remaining rows
// (`or_default`/`fallback` as aliases of `unwrap_or`, `is_none`, `ok_or`) and
// the "an `undef` subject propagates `undef` through every combinator" sentence
// are normative claims the section makes on its own authority, and nothing
// scraped from the chunk pins them. Curated-claim-diverges-from-behaviour is
// precisely the drift class task #6047 exists to close, so they are pinned here
// too — against the same stdlib-aware pipeline, just with fixtures rather than
// scraped source, since the section states them in prose rather than in code.

/// Every `## Option Type` table row the scraped example does not itself
/// exercise, on both the `some(x)` and the `none` path.
#[test]
fn option_combinator_table_rows_hold_as_documented() {
    let result = eval_module(
        r#"structure def Table {
  param present : Option<Length> = some(0.25mm)
  param absent  : Option<Length> = none

  let or_default_present = or_default(present, 7mm)
  let or_default_absent  = or_default(absent, 7mm)
  let fallback_present   = fallback(present, 7mm)
  let fallback_absent    = fallback(absent, 7mm)
  let is_none_present    = is_none(present)
  let is_none_absent     = is_none(absent)
  let ok_or_present      = ok_or(present, "no coating")
  let ok_or_absent       = ok_or(absent, "no coating")
}"#,
    );

    // Row: `or_default(o, dflt)`, `fallback(o, dflt)` — documented as aliases of
    // `unwrap_or`, i.e. `x` / `dflt`.
    for name in ["or_default", "fallback"] {
        assert_length_mm(
            &cell_value(&result, "Table", &format!("{name}_present")),
            0.25,
            &format!("`{name}(some(0.25mm), 7mm)` — documented as an alias of unwrap_or"),
        );
        assert_length_mm(
            &cell_value(&result, "Table", &format!("{name}_absent")),
            7.0,
            &format!("`{name}(none, 7mm)` — documented as an alias of unwrap_or"),
        );
    }

    // Row: `is_none(o)` — `false` / `true`.
    assert_bool(
        &cell_value(&result, "Table", "is_none_present"),
        false,
        "`is_none(some(0.25mm))`",
    );
    assert_bool(
        &cell_value(&result, "Table", "is_none_absent"),
        true,
        "`is_none(none)`",
    );

    // Row: `ok_or(o, err)` — the Option→Result bridge, documented as producing
    // `Ok { value: x }` / `Err { error: err }`. Those exact field names are what
    // make the section's "a Result you *can* match on" claim usable: a reader
    // writes `Ok { value: v }` in a pattern, so the payload key must be `value`.
    assert_result_variant(
        &cell_value(&result, "Table", "ok_or_present"),
        "Ok",
        "value",
        "`ok_or(some(0.25mm), \"no coating\")`",
    );
    assert_result_variant(
        &cell_value(&result, "Table", "ok_or_absent"),
        "Err",
        "error",
        "`ok_or(none, \"no coating\")`",
    );
}

/// The section's closing claim about `undef`: "An `undef` subject propagates
/// `undef` through every combinator (Kleene three-valued)."
///
/// "Every" is the load-bearing word, so every combinator the section's table
/// names is checked — including `map_or`, which is evaluated by a separate
/// ctx-aware branch rather than by the pure `option_recovery` dispatcher and so
/// could regress independently of the rest. Upstream contract: INV-2 in
/// `crates/reify-expr/src/option_recovery.rs`.
#[test]
fn undef_subject_propagates_through_every_documented_combinator() {
    let result = eval_module(
        r#"structure def Undefs {
  param base   : Length = 1mm
  param broken : Option<Length> = undef

  let via_unwrap_or  = unwrap_or(broken, 9mm)
  let via_or_default = or_default(broken, 9mm)
  let via_fallback   = fallback(broken, 9mm)
  let via_or_else    = or_else(broken, some(9mm))
  let via_is_some    = is_some(broken)
  let via_is_none    = is_none(broken)
  let via_map_or     = map_or(broken, base, |c: Length| base + c)
  let via_ok_or      = ok_or(broken, "no coating")
}"#,
    );

    for combinator in [
        "unwrap_or",
        "or_default",
        "fallback",
        "or_else",
        "is_some",
        "is_none",
        "map_or",
        "ok_or",
    ] {
        assert_eq!(
            cell_value(&result, "Undefs", &format!("via_{combinator}")),
            Value::Undef,
            "`{combinator}` must propagate an `undef` subject as `undef` (Kleene \
             three-valued, option_recovery.rs INV-2). It now recovers instead — the \
             `## Option Type` section in crates/reify-mcp/src/tools/chunks/enums.md \
             claims propagation holds for EVERY combinator, so that sentence must be \
             rewritten in this same diff."
        );
    }
}

// --- Two-way ratchet: pin the compiler behaviour the chunk's warning asserts ---
//
// The `## Option Type` section now warns that `match` cannot read an `Option`
// payload in EITHER form. The two tests below make that warning executable, so
// the doc and the compiler cannot drift apart in either direction: if the
// grammar/eval gap ever closes, these flip RED and force the warning to be
// rewritten in the SAME diff, rather than leaving the chunk asserting a
// restriction that no longer exists.

/// `some(v) => ...` — a positional payload-binding pattern — must still fail to
/// parse.
///
/// Authority for the gap being open and DELIBERATE:
/// `docs/prds/v0_6/data-carrying-enums.md` §10 and design fork **F4**, which
/// records the `some(IDENT)`/`none` parse gap as explicitly not closed by that
/// PRD and reserves the decision ("Recommend Leo decide"). Mechanically, the
/// grammar has no positional production at all — `tree-sitter-reify/grammar.js`
/// `match_pattern` is `variant_binding_pattern | identifier ('|' identifier)* |
/// '_'`, and `variant_binding_pattern` is brace-only.
///
/// **If this test goes RED, F4 landed.** Rewrite the `## Option Type` warning in
/// `crates/reify-mcp/src/tools/chunks/enums.md` in the same diff.
///
/// The assertions are deliberately specific to the PARSE layer. A bare
/// "some `Severity::Error` was produced" check would be satisfied by any error
/// from any layer — a future typing error on `param c : Option<Length>`, a
/// resolution error on the wrapper, or a partially-closed F4 in which `some(v)`
/// *parses* and the arm is rejected downstream for an unrelated reason — and so
/// could hold this ratchet green past the very event it exists to catch. The
/// chunk's warning says specifically "`some(c) => ...` is a **parse error**", so
/// that is what gets pinned: `parsed.errors` (the parse layer by construction,
/// no message-shape guessing), naming the offending arm.
///
/// The final assertion uses the non-panicking `_allow_parse_errors` variant
/// deliberately: the plain `compile_source_with_stdlib` panics on parse errors,
/// so a negative test must observe the diagnostics rather than die on them.
#[test]
fn option_payload_binding_pattern_still_fails_to_parse() {
    let source = "structure def D {\n\
                  param c : Option<Length> = some(3mm)\n\
                  let m = match c { some(v) => v, none => 0mm }\n\
                  }";

    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    let messages: Vec<&str> = parsed.errors.iter().map(|e| e.message.as_str()).collect();
    assert!(
        !parsed.errors.is_empty(),
        "`some(v) => ...` is expected to be rejected AT THE PARSE LAYER \
         (data-carrying-enums.md §10 / fork F4 leave the positional-pattern gap open, \
         and `tree-sitter-reify/grammar.js` `match_pattern` has no positional \
         production), but the source parsed clean. If F4 has landed, the \
         `## Option Type` warning in enums.md must be rewritten in this same diff."
    );
    assert!(
        parsed
            .errors
            .iter()
            .any(|e| e.message.contains("some(v)") || e.message.contains("match")),
        "the parse error must be about the payload-binding match arm, not some \
         unrelated part of the wrapper — the sibling positive tests already prove \
         `param c : Option<Length> = ...` parses and compiles clean. \
         Parse errors seen: {messages:#?}"
    );

    // And that rejection must survive into the compiled module's diagnostics as
    // a Severity::Error, i.e. a reader who runs this source actually sees it.
    let compiled = compile_source_with_stdlib_allow_parse_errors(source);
    assert!(
        !errors_only(&compiled).is_empty(),
        "the parse error must reach the caller as a Severity::Error diagnostic; \
         parse errors seen: {messages:#?}"
    );
}

/// Binder-less `some`/`none` match arms must still evaluate to `undef`.
///
/// This is the pin that makes "binder-less arms are not a workaround" an
/// executable fact rather than a claim, and it is why the chunk documents the
/// recovery combinators instead. `Option` is intrinsic — `Value::Option`, not
/// `Value::Enum` — and eval's match-arm selection is gated exclusively on
/// `Value::Enum` (`reify-expr/src/lib.rs:635`), so an `Option` scrutinee falls
/// through to the `Value::Undef` catch-all (`:663-670`). The compiler does not
/// diagnose it either, so the arms compile clean and silently produce nothing —
/// strictly worse for a reader than the loud parse error above.
///
/// Both subjects are checked: a `some(x)` subject yields `undef` just as a
/// `none` subject does, so a passing-looking spot check on one path cannot
/// mislead.
///
/// The compile/eval pipeline is spelled out inline rather than routed through
/// `eval_module` (or `parse_and_compile_with_stdlib`), both of which abort on
/// ANY diagnostic with a generic `compile errors: {..}` / `eval-phase errors:
/// {..}` panic. The most likely first step toward closing this gap is exactly
/// that the compiler or evaluator starts *diagnosing* a binder-less arm over an
/// `Option` scrutinee — in which case those generic panics would fire and say
/// nothing about enums.md, defeating the doc-pointing failure messages this
/// ratchet exists to deliver. Each of the section's three claims — compiles
/// clean, yields `undef`, emits no diagnostic ("silently") — therefore gets its
/// own assertion and its own message.
#[test]
fn binderless_option_match_still_evaluates_to_undef() {
    for subject in ["some(3mm)", "none"] {
        let source = format!(
            "structure def D {{\n\
             param c : Option<Length> = {subject}\n\
             let m = match c {{ some => 111mm, none => 222mm }}\n\
             }}"
        );

        // `compile_source_with_stdlib` panics only on PARSE errors, which is the
        // right precondition here: this form is documented as one that *parses*.
        let compiled = compile_source_with_stdlib(&source);
        let compile_errors = errors_only(&compiled);
        assert!(
            compile_errors.is_empty(),
            "binder-less `some`/`none` arms over subject `{subject}` are expected to \
             compile clean — the chunk's warning calls this failure mode `silent`. The \
             compiler now diagnoses it: {compile_errors:#?}\n\
             That is an improvement, but the `## Option Type` warning in \
             crates/reify-mcp/src/tools/chunks/enums.md must be rewritten in this same \
             diff to stop claiming the form is accepted without complaint."
        );

        let mut engine = make_engine();
        let result = engine.eval(&compiled);

        assert_eq!(
            cell_value(&result, "D", "m"),
            Value::Undef,
            "binder-less `some`/`none` arms over subject `{subject}` are expected to \
             evaluate to undef (Option is Value::Option, not Value::Enum). If this \
             now yields a real value, the `## Option Type` warning in enums.md must \
             be rewritten in this same diff."
        );

        let eval_errors = collect_errors(&result.diagnostics);
        assert!(
            eval_errors.is_empty(),
            "binder-less `some`/`none` arms over subject `{subject}` are expected to \
             yield undef *silently* — that word in the `## Option Type` warning is why \
             this form is worse than the loud parse error. The evaluator now diagnoses \
             it: {eval_errors:#?}\n\
             Rewrite the warning in crates/reify-mcp/src/tools/chunks/enums.md in this \
             same diff."
        );
    }
}
