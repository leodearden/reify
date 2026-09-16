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
