//! Unit pins for `reify_test_support::rust_fixture_scan` — the comment-aware
//! walk that finds Reify-DSL snippets embedded in Rust test source (task
//! #7543).
//!
//! Every expected host line below is COMPUTED from its fixture by
//! [`line_containing`] rather than hand-counted, so editing a fixture cannot
//! silently invalidate an assertion: it either still finds its anchor or fails
//! loudly on the anchor itself.

use reify_test_support::rust_fixture_scan::{RawStringLiteral, raw_string_literals};

/// The 1-based line of the one line of `host` that contains `needle`.
///
/// Panics unless exactly one line matches, so a needle that a fixture edit made
/// ambiguous (or absent) fails as a fixture defect rather than as a silently
/// wrong expectation.
fn line_containing(host: &str, needle: &str) -> u32 {
    let hits: Vec<u32> = host
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(i, _)| i as u32 + 1)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "fixture anchor {needle:?} must appear on exactly one line, found {hits:?}"
    );
    hits[0]
}

fn texts(lits: &[RawStringLiteral]) -> Vec<&str> {
    lits.iter().map(|l| l.text.as_str()).collect()
}

fn host_lines(lits: &[RawStringLiteral]) -> Vec<u32> {
    lits.iter().map(|l| l.host_line).collect()
}

// --- (1) the `let source = r#"…"#;` shape both confirmed sites actually use ---

/// Verbatim shape of `purpose_compile_tests.rs:1450`
/// (`guarded_where_arm_lowers_to_implies`) — a fn-body `let` binding, NOT a
/// `const SOURCE: &str = …` item. An enumerator keyed on `const` would miss
/// exactly the sites task #7543 must see.
const LET_BOUND_HOST: &str = r##"fn guarded_where_arm_lowers_to_implies() {
    let source = r#"
structure Frame {
    param material : Length = 1.0
}
"#;
    let module = compile_module_with_diagnostics(source);
    assert!(module.diagnostics.is_empty());
}
"##;

#[test]
fn collects_a_let_bound_raw_string_at_its_first_content_line() {
    let lits = raw_string_literals(LET_BOUND_HOST);

    assert_eq!(
        texts(&lits),
        vec!["structure Frame {\n    param material : Length = 1.0\n}\n"],
        "a fn-body `let source = r#\"…\"#;` binding is a collected literal"
    );
    assert_eq!(
        lits[0].host_line,
        line_containing(LET_BOUND_HOST, "structure Frame {"),
        "host_line is the host line of the snippet's FIRST CONTENT line"
    );
}

#[test]
fn an_opener_followed_by_a_newline_starts_the_snippet_on_the_next_host_line() {
    let lits = raw_string_literals(LET_BOUND_HOST);

    // The off-by-one that matters: `r#"` is immediately followed by a newline,
    // so the literal's own line 1 is the empty remainder of the opener line and
    // the first line a reader cares about is the NEXT host line.
    assert_eq!(
        lits[0].host_line,
        line_containing(LET_BOUND_HOST, "let source = r#") + 1,
        "an opener followed directly by a newline yields host_line = opener line + 1"
    );
    assert!(
        lits[0].text.starts_with("structure Frame {"),
        "the snippet's line 1 is its first CONTENT line, so host_line + n - 1 \
         addresses snippet line n uniformly; got {:?}",
        lits[0].text
    );
}

// --- (2) a module-level `const` with a doubled hash count ---

const CONST_HOST: &str = r###"const SOURCE: &str = r##"module a.b
param x : Real = 1.0
"##;
"###;

#[test]
fn collects_a_module_level_const_with_a_doubled_hash_count() {
    let lits = raw_string_literals(CONST_HOST);

    assert_eq!(
        texts(&lits),
        vec!["module a.b\nparam x : Real = 1.0\n"],
        "a doubled-hash `r##\"…\"##` item-level const is a collected literal"
    );
    // Content that does NOT start with a newline begins on the opener's own
    // line — the complement of the case pinned above.
    assert_eq!(
        lits[0].host_line,
        line_containing(CONST_HOST, "const SOURCE"),
        "content starting on the opener line reports the opener line"
    );
}

// --- (3) zero hashes, and source order across hash counts ---

const ORDER_HOST: &str = r###"fn f() {
    let a = r"module m.zero";
    let b = r#"module m.one"#;
    let c = r##"module m.two"##;
}
"###;

#[test]
fn collects_every_hash_count_in_source_order() {
    let lits = raw_string_literals(ORDER_HOST);

    assert_eq!(
        texts(&lits),
        vec!["module m.zero", "module m.one", "module m.two"],
        "zero-, single- and double-hash literals are all collected, in source order"
    );
    assert_eq!(
        host_lines(&lits),
        vec![
            line_containing(ORDER_HOST, "m.zero"),
            line_containing(ORDER_HOST, "m.one"),
            line_containing(ORDER_HOST, "m.two"),
        ],
    );
}

// --- the closing delimiter needs the FULL hash count ---

const NESTED_HASH_HOST: &str = r###"const S: &str = r##"alpha
"# is not the closer
omega"##;
"###;

#[test]
fn a_shorter_hash_run_inside_the_body_does_not_close_the_literal() {
    let lits = raw_string_literals(NESTED_HASH_HOST);

    assert_eq!(
        texts(&lits),
        vec!["alpha\n\"# is not the closer\nomega"],
        "`\"#` inside an `r##\"…\"##` body is body text, not a terminator"
    );
}

// --- hazard (a): openers inside `//` and `///` comments ---

const COMMENT_HOST: &str = r##"// a line comment mentioning r#"NOT_COLLECTED"#
/// a doc comment mentioning r#"ALSO_NOT_COLLECTED"#
fn f() {
    let real = r#"module m.real"#;
}
"##;

#[test]
fn an_opener_inside_a_line_comment_is_not_collected() {
    assert_eq!(
        texts(&raw_string_literals(COMMENT_HOST)),
        vec!["module m.real"],
        "`r#\"` inside a `//` or `///` comment is prose (5 such occurrences \
         measured live in this workspace), not a fixture"
    );
}

// --- hazard (b): openers inside a NESTING block comment ---

const BLOCK_COMMENT_HOST: &str = r##"/* outer /* inner r#"NOT_COLLECTED"# */ still outer r#"ALSO_NOT"# */
fn f() {
    let real = r#"module m.real"#;
}
"##;

#[test]
fn an_opener_inside_a_nesting_block_comment_is_not_collected() {
    assert_eq!(
        texts(&raw_string_literals(BLOCK_COMMENT_HOST)),
        vec!["module m.real"],
        "Rust block comments nest: the inner `*/` must not end the outer comment"
    );
}

// --- hazard (c): raw IDENTIFIERS are not openers ---

const RAW_IDENT_HOST: &str = r##"fn f() {
    let r#struct = 1;
    let r#type = r#"module m.real"#;
    let _ = (r#struct, r#type);
}
"##;

#[test]
fn a_raw_identifier_is_not_mistaken_for_a_raw_string_opener() {
    assert_eq!(
        texts(&raw_string_literals(RAW_IDENT_HOST)),
        vec!["module m.real"],
        "`r#type` / `r#struct` are identifiers; only `r` + N `#` + `\"` opens a literal"
    );
}

// --- hazard (d): an opener sequence inside an ordinary string with escapes ---

const ESCAPED_STRING_HOST: &str = r##"fn f() {
    let decoy = "escaped \" then r#";
    let real = r#"module m.real"#;
}
"##;

#[test]
fn an_opener_inside_an_ordinary_string_literal_is_not_collected() {
    // A scanner that does not skip `\"` closes `decoy` early, then reads the
    // trailing `r#"` as a real opener and swallows the fixture that follows.
    assert_eq!(
        texts(&raw_string_literals(ESCAPED_STRING_HOST)),
        vec!["module m.real"],
        "`\\\"` inside an ordinary string must not end it"
    );
}

// --- hazard (e): char literals and lifetimes must not desync the scan ---

const CHAR_AND_LIFETIME_HOST: &str = r##"fn f<'a>(s: &'a str) -> char {
    let q = '"';
    let e = 'é';
    let _ = (q, e, s);
    let real = r#"module m.real"#;
    q
}
"##;

#[test]
fn a_char_literal_and_a_lifetime_do_not_desync_the_scan() {
    let lits = raw_string_literals(CHAR_AND_LIFETIME_HOST);

    assert_eq!(
        texts(&lits),
        vec!["module m.real"],
        "`'\"'` is one char (not a quote), `&'a str` is a lifetime (no closing \
         quote), and a multi-byte `'é'` is measured by UTF-8 lead-byte width"
    );
    assert_eq!(
        lits[0].host_line,
        line_containing(CHAR_AND_LIFETIME_HOST, "let real"),
        "a multi-byte char earlier in the file must not shift the reported line"
    );
}

// ---------------------------------------------------------------------------
// Admission predicates: which collected literals are embedded Reify source
// ---------------------------------------------------------------------------

use reify_test_support::rust_fixture_scan::{
    inline_ri_snippets, is_format_template, is_inline_fixture_host, looks_like_reify_source,
};
use std::path::Path;

/// `purpose_compile_tests.rs:1451` verbatim — the first of the two sites task
/// #7543's VERIFY criterion names.
const CONFIRMED_WHERE_ARM: &str = "structure Frame {
    param material : Length = 1.0
    param youngs_modulus : Length = 200.0
}

purpose p(subject : Structure) {
    where subject.material > 0.0 {
        constraint subject.youngs_modulus > 0.0
    }
}
";

/// `purpose_compile_tests.rs:1563` verbatim — the second named site.
const CONFIRMED_ELSE_ARM: &str = "structure Frame {
    param z : Length = 5.0
}

purpose p(subject : Structure) {
    where 0.0 > 1.0 {
    } else {
        constraint subject.z > 0.0
    }
}
";

/// `math_construction_signatures_tests.rs:21` verbatim — a `const`-bound
/// snippet, so the accept filter is pinned against both binding shapes.
const CONFIRMED_CONSTRUCT: &str = "structure def Constructed {
    let v = vec([1.0, 2.0, 3.0, 4.0])
    let m = matrix([[1.0, 2.0], [3.0, 4.0]])
    let d = diag([3.0, 5.0, 7.0])
    let i = identity(4)
}
";

#[test]
fn looks_like_reify_source_accepts_the_real_confirmed_snippets() {
    for (name, snippet) in [
        ("where-arm", CONFIRMED_WHERE_ARM),
        ("else-arm", CONFIRMED_ELSE_ARM),
        ("construct", CONFIRMED_CONSTRUCT),
    ] {
        assert!(
            looks_like_reify_source(snippet),
            "{name}: a real inline fixture must be admitted as Reify source"
        );
    }
}

#[test]
fn looks_like_reify_source_accepts_a_single_line_declaration() {
    assert!(
        looks_like_reify_source("trait T { #fast param x : Real }"),
        "a whole declaration on one line is still a line-anchored declaration"
    );
}

#[test]
fn looks_like_reify_source_reads_visibility_attribute_and_indent_prefixes() {
    for form in [
        "pub structure def Actuator {\n}\n",
        "priv structure def Hidden {\n}\n",
        "@test structure def Rig {\n}\n",
        "    param x : Real = 1.0\n",
        "#precision(0.001m)\n",
        "module a.b\n",
        "import stdlib.fea\n",
        "occurrence def Bolt {\n}\n",
        "constraint def InRange {\n}\n",
        "enum Shape {\n}\n",
        "purpose p(subject : Structure) {\n}\n",
    ] {
        assert!(
            looks_like_reify_source(form),
            "declaration form {form:?} must be admitted"
        );
    }
}

/// The shape of the eight `r###"` literals in
/// `crates/reify-builtins/tests/common/seed_name_scan.rs` — Rust source carried
/// as a fixture. A bare `contains("let ")` heuristic admits it, and admitting it
/// would poison the census with rows that describe no Reify site at all.
const RUST_SOURCE_FIXTURE: &str = "fn wrap_tensor_field(name: &str) -> u8 {
    let x = 1;
    match name {
        \"von_mises\" => x,
        _ => 0,
    }
}
";

#[test]
fn looks_like_reify_source_rejects_non_reify_blobs() {
    for (name, blob) in [
        ("rust source fixture", RUST_SOURCE_FIXTURE),
        // `lsp_fixtures.rs:5`, `MINIMAL_INIT_PARAMS_JSON`.
        ("json payload", "{\"capabilities\":{}}"),
        (
            "expected-diagnostic prose",
            "warning: argument 'z' has type 'Real' but param 'z' requires type 'Scalar[m]'\n  \
             --> test.ri:3:5\n",
        ),
    ] {
        assert!(
            !looks_like_reify_source(blob),
            "{name}: must be rejected — only LINE-ANCHORED Reify declaration \
             grammar admits, never a bare substring match"
        );
    }
}

// --- (B) format! templates ---

/// `ambient_default_injection_tests.rs:136-144` verbatim — Reify-shaped, but a
/// `format!` template: the `{{`/`}}` braces and the `{STEEL_CTOR}` placeholder
/// are not Reify syntax, so compiling it would record a noise `parse-error`.
const AMBIENT_TEMPLATE: &str = "default Material = {STEEL_CTOR}

structure def Bracket : Physical {{
    param geometry : Solid = box(10mm, 20mm, 30mm)
}}
";

#[test]
fn is_format_template_flags_doubled_braces_and_bare_placeholders() {
    assert!(
        is_format_template(AMBIENT_TEMPLATE),
        "the live ambient-default template must be flagged"
    );
    assert!(
        is_format_template("structure def B {{\n}}\n"),
        "doubled braces"
    );
    assert!(
        is_format_template("default Material = {STEEL_CTOR}\n"),
        "a bare `{{ident}}` placeholder"
    );
}

#[test]
fn is_format_template_leaves_ordinary_reify_braces_alone() {
    for snippet in [CONFIRMED_WHERE_ARM, CONFIRMED_ELSE_ARM, CONFIRMED_CONSTRUCT] {
        assert!(
            !is_format_template(snippet),
            "single braces and `{{}}` empty bodies are ordinary Reify syntax"
        );
    }
    assert!(
        !is_format_template("structure def Empty {}\n"),
        "an empty body is `{{}}`, not a placeholder"
    );
}

// --- (C) which .rs files can host an inline fixture ---

#[test]
fn is_inline_fixture_host_admits_every_rust_source_shape_under_crates() {
    for rel in [
        // A `tests` directory component.
        "crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs",
        "crates/reify-test-support/tests/rust_fixture_scan.rs",
        // `src/**/tests.rs` — 2 of the 12 tracked `crates/*/src/**/*tests.rs`.
        "crates/reify-eval/src/engine_build/tests.rs",
        // `src/**/*_tests.rs` — the other 10, which a `tests.rs`-EXACT file-name
        // clause excluded wholesale.
        "crates/reify-eval/src/tolerance_combine/compute_representation_bounds_tests.rs",
        // `#[cfg(test)] mod tests` inside a production `src/*.rs` — the largest
        // excluded shape: 409 of the 561 tracked `crates/*/src/**/*.rs` carry a
        // `#[cfg(test)]` module, and their fixtures are Reify source like any
        // other.
        "crates/reify-lsp/src/analysis.rs",
    ] {
        assert!(
            is_inline_fixture_host(Path::new(rel)),
            "{rel} must be an in-scope host"
        );
    }
}

#[test]
fn is_inline_fixture_host_decides_on_path_shape_alone() {
    assert!(
        is_inline_fixture_host(Path::new("crates/reify-nonexistent/src/no_such_file.rs")),
        "the predicate must be a pure path-shape test with no I/O: a path that is \
         absent from disk is still admitted on shape, because CONTENT admission is \
         `looks_like_reify_source`'s job. A non-fixture source simply contributes \
         zero snippets; excluding it by file NAME is what missed 10 of the 12 \
         `src/**/*_tests.rs` hosts"
    );
}

#[test]
fn is_inline_fixture_host_rejects_non_rust_build_output_and_the_excluded_roots() {
    for rel in [
        "crates/reify-compiler/tests/fixtures/variant_construct_valid.ri",
        "crates/reify-compiler/target/debug/build/x/out/tests/generated.rs",
        // The scope anchor is `crates/`. The Tauri sidecar and the grammar
        // crate are separate cargo/grammar projects, excluded by a DISCLOSED
        // decision (named limitation 1 in the survey artifact) rather than by
        // silent construction — note the second would otherwise be admitted by
        // its own `tests` directory component.
        "gui/src-tauri/src/commands.rs",
        "tree-sitter-reify/tests/aux_at_grammar_tests.rs",
    ] {
        assert!(
            !is_inline_fixture_host(Path::new(rel)),
            "{rel} must not be an in-scope host"
        );
    }
}

// --- (D) the composition ---

const COMPOSED_HOST: &str = r###"fn f() {
    let reify = r#"
structure def Constructed {
    let v = vec([1.0, 2.0])
}
"#;
    let json = r#"{"capabilities":{}}"#;
    let rust_fixture = r#"
fn production(name: &str) -> u8 {
    let x = 1;
    match name {
        "von_mises" => x,
        _ => 0,
    }
}
"#;
    let templated = format!(r#"
default Material = {STEEL_CTOR}

structure def Bracket : Physical {{
    param geometry : Solid = box(10mm)
}}
"#);
    let _ = (reify, json, rust_fixture, templated);
}
"###;

#[test]
fn inline_ri_snippets_admits_reify_drops_blobs_and_discloses_templates() {
    let scan = inline_ri_snippets(COMPOSED_HOST);

    assert_eq!(
        scan.snippets.len(),
        1,
        "only the Reify-shaped, non-templated literal is admitted; got {:#?}",
        scan.snippets
    );
    assert!(
        scan.snippets[0]
            .text
            .starts_with("structure def Constructed {"),
        "got {:?}",
        scan.snippets[0].text
    );
    assert_eq!(
        scan.snippets[0].host_line,
        line_containing(COMPOSED_HOST, "structure def Constructed {"),
        "the collector's host_line survives the admission filter unchanged"
    );

    assert_eq!(
        scan.format_templates.len(),
        1,
        "a Reify-shaped `format!` template is REPORTED, not silently dropped, \
         so the caller can disclose it as a coverage reason; got {:#?}",
        scan.format_templates
    );
    assert_eq!(
        scan.format_templates[0].host_line,
        line_containing(COMPOSED_HOST, "default Material = {STEEL_CTOR}"),
    );

    // The JSON payload and the Rust-source fixture are not Reify at all: they
    // are neither admitted nor disclosed, because there is nothing to survey.
    let disclosed: Vec<&str> = scan
        .snippets
        .iter()
        .chain(scan.format_templates.iter())
        .map(|s| s.text.as_str())
        .collect();
    for absent in ["capabilities", "von_mises"] {
        assert!(
            !disclosed.iter().any(|t| t.contains(absent)),
            "{absent:?} is not Reify source and must not reach either list"
        );
    }
}
