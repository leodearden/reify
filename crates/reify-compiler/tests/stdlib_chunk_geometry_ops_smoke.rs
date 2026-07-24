//! Compile-smoke for the "Key Geometry Operations" (+ "Curves") table served by
//! the `reify_language_reference` MCP tool (topic="stdlib") — the chunk
//! `crates/reify-mcp/src/tools/chunks/stdlib.md`.
//!
//! The chunk is the AUTHORITATIVE language reference shown verbatim to the
//! in-GUI assistant, so a documented signature that does not match the
//! compiler's real geometry-op arms silently misleads designers (task 5347,
//! same stale-reference class as task 5203).
//!
//! ## What is actually enforced
//!
//! The fixture `tests/fixtures/stdlib_geometry_ops_smoke.ri` is an EXECUTABLE
//! TRANSCRIPTION of that chunk's Key Geometry Operations + Curves sections,
//! concretized over real primitives/profiles. Two complementary properties are
//! asserted over it, and NEITHER ALONE IS SUFFICIENT:
//!
//! 1. **Arity/type.** The fixture compiles with ZERO Error-severity
//!    diagnostics — i.e. every documented form type-checks, *for the names the
//!    compiler resolves*.
//! 2. **Name existence.** Every call name in the fixture is a real entry in the
//!    compiler's own registries (`GEOMETRY_FUNCTION_NAMES` /
//!    `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`). This is NOT implied by (1): an
//!    unknown call name in a `structure def` body is silently ignored by the
//!    compiler — NO diagnostic at ANY severity — so only names that already
//!    have a geometry-op arm are ever arity-checked, and a
//!    documented-but-nonexistent op (the phantom `offset_surface` task 5347
//!    removed by hand) passes (1) untouched.
//!    `bogus_geometry_op_names_are_reported_as_unrecognised` is the negative
//!    control that pins this guard's discriminating power, so the hole cannot
//!    silently reopen.
//!
//! Together these establish that each documented form names a real op and is
//! accepted as written. They do NOT establish semantic correctness: a
//! permutation of two same-dimension arguments would still satisfy both.
//!
//! The fixture and the chunk MUST be kept in lockstep: if you change a
//! signature in one, change it in the other (there is no mechanical cross-crate
//! markdown-parse test — reify-mcp has no reify-compiler dependency, and
//! doc-content meta-tests are discouraged by the house TDD rules).
//!
//! The parse→compile→filter-`Severity::Error` sequence mirrors
//! `examples_smoke.rs::smoke_one`; the fixture-path-const + named-test shape
//! mirrors `reify-eval/tests/topology_selector_smoke_tests.rs`.

use reify_compiler::{
    GEOMETRY_FUNCTION_NAMES, GEOMETRY_TOPOLOGY_SELECTOR_NAMES, compile_with_stdlib,
    parse_with_stdlib,
};
use reify_core::{ModulePath, Severity};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stdlib_geometry_ops_smoke.ri"
);

/// Every geometry-op / curve-constructor form documented in stdlib.md's
/// "Key Geometry Operations" (+ "Curves") table must compile with no
/// Error-severity diagnostics.
#[test]
fn stdlib_chunk_geometry_ops_compile_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("tests/fixtures/stdlib_geometry_ops_smoke.ri should exist");

    // Parse phase — prelude-aware parsing (matches the compile_with_stdlib
    // companion below). A parse error is a fixture bug, not the property under
    // test, so surface it distinctly.
    let parsed = parse_with_stdlib(&source, ModulePath::single("stdlib_geometry_ops_smoke"));
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse cleanly, got parse errors:\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    );

    // Compile phase — filter to Error severity only (warnings are allowed).
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<String> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();

    assert!(
        errors.is_empty(),
        "stdlib.md's documented geometry-op forms must all compile with zero \
         Error-severity diagnostics (fixture: stdlib_geometry_ops_smoke.ri); got {} error(s):\n{}",
        errors.len(),
        errors.join("\n")
    );
}

// ── Name-existence guard ─────────────────────────────────────────────────────
//
// The compile smoke above is only HALF the contract. The compiler silently
// ignores an unknown call name in a `structure def` body — it emits NO
// diagnostic at ANY severity — so only names that already have a geometry-op
// arm ever get arity-checked. That means a documented-but-nonexistent op (the
// phantom `offset_surface` this task removed by hand) sails through the
// zero-Error assertion untouched. The tests below close that hole by checking
// every call name in the fixture against the compiler's OWN name registries.

/// Argument-scaffolding constructors the fixture uses to BUILD inputs for the
/// documented ops. Exact-match and deliberately minimal: these are datum/value
/// constructors, NOT entries in stdlib.md's "Key Geometry Operations"/"Curves"
/// table, and none of them is reachable from an integration test through an
/// exported name slice (`vec3` lives in the private `math_signatures` module's
/// `MATH_CONSTRUCTION_NAMES`; the `point3`/`plane_xy`/`axis_z`/`orient_identity`
/// datum constructors are resolved by an arg-aware resolver with no exported
/// slice at all). Widening more `pub use` exports purely to satisfy a test
/// would enlarge reify-compiler's public API for names that are not under
/// review here. Because the allowlist is exact-match, a typo'd scaffold call
/// (`point33`) is still reported.
const SCAFFOLD_CTORS: &[&str] = &["point3", "vec3", "plane_xy", "axis_z", "orient_identity"];

/// DSL keywords that can lexically precede a `(` and would otherwise be
/// extracted as call names. `structure`/`def` head a declaration whose
/// parameter list may open with `(`; `let` can bind a parenthesised
/// expression; `if`/`for` head parenthesised conditions/iterables. None is a
/// geometry op, so all five are skipped.
const DSL_KEYWORDS: &[&str] = &["structure", "def", "let", "if", "for"];

/// Every call name in `source` that the compiler does not recognise as a
/// geometry op, topology selector, or known input-scaffold constructor.
///
/// Membership is tested against the compiler's own public registries
/// (`GEOMETRY_FUNCTION_NAMES` ∪ `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`) plus
/// [`SCAFFOLD_CTORS`]. Those two registries ARE the compiler's recognition
/// semantics: `is_geometry_function` / `is_geometry_topology_selector` are
/// defined as bare `.contains(&name)` over exactly these slices, so checking
/// the slices reproduces them with no production-code change.
///
/// The scan is lexical, not a parse: for each `(`, walk back over the run of
/// `[A-Za-z0-9_]` immediately preceding it and take that as the callee. `//`
/// line comments are stripped first — prose in a comment is not a call site,
/// and scanning it would make the guard fire on incidental parenthesised text.
/// A run that does not start with `[a-z_]` is skipped: a leading digit means a
/// numeric literal was grabbed (`2(`), and a leading uppercase means a
/// type/struct constructor rather than a geometry op.
///
/// Returned names are deduped and sorted so assertion output is deterministic.
fn unrecognised_geometry_call_names(source: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();

    for line in source.lines() {
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        let bytes = code.as_bytes();

        for (open, ch) in code.char_indices() {
            if ch != '(' {
                continue;
            }

            // Walk back over the identifier run. Only ASCII bytes are consumed
            // (a UTF-8 continuation byte is >= 0x80, so it fails the test and
            // stops the walk), which keeps `start` on a char boundary.
            let mut start = open;
            while start > 0 {
                let prev = bytes[start - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    start -= 1;
                } else {
                    break;
                }
            }
            if start == open {
                continue; // `(` with no identifier before it — a grouping paren.
            }

            let name = &code[start..open];
            if !name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_') {
                continue;
            }
            if DSL_KEYWORDS.contains(&name)
                || GEOMETRY_FUNCTION_NAMES.contains(&name)
                || GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name)
                || SCAFFOLD_CTORS.contains(&name)
            {
                continue;
            }

            names.push(name.to_string());
        }
    }

    names.sort();
    names.dedup();
    names
}

/// NEGATIVE CONTROL — the reviewer's exact repro, inline (no fixture edit).
///
/// `scal`/`thikken` are typo'd renames of real ops and `offset_surface` is the
/// phantom signature this task deleted from stdlib.md BY HAND. None of the
/// three produces a diagnostic at any severity, so
/// `stdlib_chunk_geometry_ops_compile_with_stdlib_no_errors` cannot see them.
/// This test pins the guard's discriminating power so that hole cannot silently
/// reopen.
#[test]
fn bogus_geometry_op_names_are_reported_as_unrecognised() {
    let source = r#"
structure def BogusOps {
    let solid   = box(10mm, 10mm, 10mm)
    let surface = rectangle(10mm, 10mm)
    let sc = scal(solid, 2.0)
    let th = thikken(solid, 1mm)
    let of = offset_surface(surface, 1mm)
}
"#;

    assert_eq!(
        unrecognised_geometry_call_names(source),
        vec![
            "offset_surface".to_string(),
            "scal".to_string(),
            "thikken".to_string(),
        ],
        "the guard must report exactly the three names that have no arm in the \
         compiler's geometry-op / topology-selector registries"
    );
}

/// The control's control — with the CORRECT names the guard must report
/// nothing, so it is not merely "reports everything".
#[test]
fn known_geometry_op_names_are_not_reported() {
    let source = r#"
structure def KnownOps {
    let solid   = box(10mm, 10mm, 10mm)
    let surface = rectangle(10mm, 10mm)
    let sc = scale(solid, 2.0)
    let th = thicken(solid, 1mm)
    let of = offset_solid(solid, 1mm)
}
"#;

    assert_eq!(
        unrecognised_geometry_call_names(source),
        Vec::<String>::new(),
        "every name in this snippet has a real compiler arm, so the guard must \
         report none of them"
    );
}

/// POSITIVE EXISTENCE, over the real fixture: every geometry-op / curve
/// constructor form transcribed from stdlib.md names a function the compiler
/// actually has.
///
/// The names are EXTRACTED FROM the fixture rather than hardcoded, so this
/// auto-tracks fixture edits: renaming a call to something with no compiler arm
/// goes RED even though the compile smoke stays green.
#[test]
fn fixture_geometry_call_names_all_exist_in_the_compiler() {
    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("tests/fixtures/stdlib_geometry_ops_smoke.ri should exist");

    let unrecognised = unrecognised_geometry_call_names(&source);

    assert!(
        unrecognised.is_empty(),
        "stdlib.md documents a geometry op the compiler does not have — the \
         chunk crates/reify-mcp/src/tools/chunks/stdlib.md must be corrected \
         (an unknown call name in a `structure def` body compiles silently, so \
         the zero-Error compile smoke cannot catch this). Unrecognised name(s): {}",
        unrecognised.join(", ")
    );
}
