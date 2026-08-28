//! Repo-wide fence gate for the MCP language-reference chunks
//! (`crates/reify-mcp/src/tools/chunks/*.md`).
//!
//! Task #5479 (PRD `docs/prds/v0_6/doc-chunk-truth-enforcement.md`, leaf β).
//! The parser and the checks land across the following steps; the module
//! docstring proper (tag vocabulary + cross-harness notes) arrives with the
//! parser implementation.

// ---------------------------------------------------------------------------
// Fence parser
// ---------------------------------------------------------------------------

// (implementation lands in the next step)

// ---------------------------------------------------------------------------
// Hermetic parser tests
//
// Every case below runs on SYNTHETIC in-memory markdown. No chunk file on disk
// is read or mutated, so the parser's own contract is pinned independently of
// whatever the real corpus happens to contain today.
// ---------------------------------------------------------------------------

/// A bare ``` opening delimiter yields `tag == None`, and its bare closing
/// delimiter is consumed as a delimiter rather than mistaken for a second
/// untagged opening.
///
/// This open/close state discrimination is the whole reason the gate cannot be
/// a `grep`: in markdown a CLOSING delimiter is bare by syntax, so a stateless
/// scan for `^```$` would flag every well-tagged fence in the corpus.
#[test]
fn bare_opening_fence_parses_untagged_and_its_closer_is_not_a_second_block() {
    let md = "intro prose\n\
              ```\n\
              bare body\n\
              ```\n\
              trailing prose\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        1,
        "the closing ``` must be consumed as a delimiter, not parsed as a \
         second untagged block; got {fences:#?}"
    );
    assert_eq!(fences[0].ordinal, 1, "ordinals are 1-based");
    assert_eq!(fences[0].open_line, 2, "open_line is 1-based");
    assert_eq!(fences[0].tag, None, "a bare opening delimiter carries no tag");
    assert_eq!(
        fences[0].body, "bare body",
        "body excludes BOTH delimiter lines"
    );
}

/// The bare closing delimiter of a TAGGED fence never surfaces as an untagged
/// block. Stated separately from the case above because this is the shape the
/// bare-fence ban must not false-positive on: every compliant fence in the
/// corpus ends with a bare ```.
#[test]
fn the_bare_closer_of_a_tagged_fence_is_not_reported_as_untagged() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
    assert!(
        fences.iter().all(|f| f.tag.is_some()),
        "the closing delimiter must not appear as an untagged fence"
    );
}

/// EXACT tag semantics: `reify-fragment` is its own tag and must never be read
/// as a bare `reify` by prefix matching.
///
/// This is the single most load-bearing parser property. `reify_fence_violations`
/// selects on `tag.as_deref() == Some("reify")`; if the tag were captured (or
/// compared) by prefix, every `reify-fragment` / `reify-schematic` fence in the
/// corpus would be trial-compiled, and the whole exempt-tag vocabulary would
/// collapse.
#[test]
fn hyphenated_tags_are_exact_and_never_collapse_to_bare_reify() {
    for tag in ["reify-fragment", "reify-schematic", "reify-invalid"] {
        let md = format!("```{tag}\nlet x = 1mm\n```\n");
        let fences = parse_fences(&md).expect("well-formed markdown must parse");

        assert_eq!(fences.len(), 1, "tag `{tag}`: got {fences:#?}");
        assert_eq!(
            fences[0].tag.as_deref(),
            Some(tag),
            "tag `{tag}` must be captured verbatim"
        );
        assert_ne!(
            fences[0].tag.as_deref(),
            Some("reify"),
            "tag `{tag}` must NOT be readable as bare `reify` — prefix matching \
             here would trial-compile every exempt fence in the corpus"
        );
    }
}

/// Trailing whitespace after an info string is trimmed, so a fence tagged
/// `` ```reify `` with a stray trailing space is still exactly `reify`.
#[test]
fn trailing_whitespace_after_a_tag_is_trimmed() {
    let md = "```reify   \nstructure def S { let n = 1 }\n```\n";
    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
}

/// A delimiter-looking line that is INDENTED is body content, not a delimiter.
///
/// The gate's rule is "fences at line start" — deliberately stricter than
/// `enums_chunk_option_smoke.rs`'s `line.trim_start().strip_prefix("```")`, so
/// an indented ``` inside a body cannot silently close the block and desync the
/// parser for the whole rest of the file.
#[test]
fn an_indented_delimiter_is_body_content_not_a_delimiter() {
    let md = "```text\n\
              outer\n\
              \x20   ```\n\
              still outer\n\
              ```\n\
              after\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        1,
        "the indented ``` must not open or close a block; got {fences:#?}"
    );
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
    assert_eq!(fences[0].body, "outer\n    ```\nstill outer");
}

/// Ordinals are assigned in document order across the whole file, and each
/// fence records its own 1-based opening line.
#[test]
fn ordinals_and_open_lines_follow_document_order() {
    let md = "# Title\n\
              ```reify\n\
              a\n\
              ```\n\
              prose\n\
              ```\n\
              b\n\
              ```\n\
              ```reify-fragment\n\
              c\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    let seen: Vec<(usize, usize, Option<&str>)> = fences
        .iter()
        .map(|f| (f.ordinal, f.open_line, f.tag.as_deref()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (1, 2, Some("reify")),
            (2, 6, None),
            (3, 9, Some("reify-fragment")),
        ],
        "ordinals must be 1-based and in document order, open_line 1-based"
    );
}

/// An unterminated final fence is an `Err` naming the opening line — never a
/// silently dropped block.
///
/// Dropping it would be the worst possible failure mode for an omission-drift
/// gate: the offending fence would vanish from the scan and the corpus test
/// would go green precisely because the file is malformed.
#[test]
fn an_unterminated_final_fence_is_an_error_naming_its_opening_line() {
    let md = "prose\n\
              ```reify\n\
              structure def S { let n = 1 }\n";

    let err = parse_fences(md).expect_err("an unterminated fence must not parse clean");

    assert!(
        err.contains('2'),
        "the error must name the OPENING line (2) of the unterminated fence, got: {err}"
    );
    assert!(
        err.to_lowercase().contains("unterminated"),
        "the error must say what went wrong, got: {err}"
    );
}

/// An empty fence body is `""`, not a parse failure.
#[test]
fn an_empty_fence_body_parses_as_the_empty_string() {
    let md = "```reify-schematic\n```\n";
    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].body, "");
}
