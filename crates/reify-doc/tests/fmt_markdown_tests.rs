//! Behavioural tests for the GitHub-flavored Markdown formatter (`fmt_markdown`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_markdown.rs` so that golden snapshots can be loaded via `include_str!` from
//! sibling `tests/snapshots/` files without polluting the library binary.

use reify_doc::fmt_markdown::{render_markdown, MarkdownOptions, MarkdownOutput};
use reify_doc::model::DocModel;

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

/// An empty `DocModel` (no modules) renders to an empty single-file body and a
/// split-mode list containing exactly the `index.md` placeholder.
#[test]
fn empty_model_single_mode_yields_empty_body() {
    let model = DocModel::default();
    let out = render_markdown(&model, None, &MarkdownOptions::default());
    match out {
        MarkdownOutput::Single(s) => {
            assert!(s.trim().is_empty(), "expected empty single body, got: {s:?}");
        }
        MarkdownOutput::Split(_) => panic!("default options should yield Single"),
    }
}

#[test]
fn empty_model_split_mode_yields_index_only() {
    let model = DocModel::default();
    let out = render_markdown(&model, None, &MarkdownOptions { split: true });
    match out {
        MarkdownOutput::Single(_) => panic!("split: true should yield Split"),
        MarkdownOutput::Split(v) => {
            assert_eq!(v.len(), 1, "expected exactly one (index.md) entry, got {v:?}");
            assert_eq!(v[0].0, "index.md");
        }
    }
}
