//! Behavioural tests for the GitHub-flavored Markdown formatter (`fmt_markdown`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_markdown.rs` so that golden snapshots can be loaded via `include_str!` from
//! sibling `tests/snapshots/` files without polluting the library binary.

use reify_doc::fmt_markdown::{MarkdownOptions, MarkdownOutput};

/// `MarkdownOptions::default()` must yield single-file (non-split) mode so that
/// callers who don't care about splitting can rely on the obvious default.
#[test]
fn options_default_is_single_file() {
    let opts = MarkdownOptions::default();
    assert!(!opts.split, "default MarkdownOptions.split must be false");
}

/// `MarkdownOutput` must expose `Single(String)` and `Split(Vec<(String, String)>)`
/// variants and be matchable.
#[test]
fn output_variants_can_be_matched() {
    let single = MarkdownOutput::Single("hello".to_string());
    match single {
        MarkdownOutput::Single(s) => assert_eq!(s, "hello"),
        MarkdownOutput::Split(_) => panic!("expected Single"),
    }

    let split = MarkdownOutput::Split(vec![("index.md".to_string(), "body".to_string())]);
    match split {
        MarkdownOutput::Single(_) => panic!("expected Split"),
        MarkdownOutput::Split(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].0, "index.md");
            assert_eq!(v[0].1, "body");
        }
    }
}
