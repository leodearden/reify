//! GitHub-flavored Markdown formatter for the DocModel.
//!
//! Public surface:
//! - [`MarkdownOptions`] — knobs that control rendering (currently only `split`).
//! - [`MarkdownOutput`] — `Single(String)` for one-blob mode, `Split(Vec<(name, body)>)`
//!   for the per-item-file mode.
//! - [`render_markdown`] — the single entry point that dispatches on
//!   [`MarkdownOptions::split`] to either single-file or split-file rendering.

use crate::cross_refs::CrossRefs;
use crate::model::DocModel;

/// Knobs controlling how the formatter emits Markdown.
///
/// `split == false` (the default) produces a single concatenated Markdown
/// document.  `split == true` produces one file per declared item plus an
/// `index.md` file holding the table of contents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// When true, render one file per item plus an `index.md`; when false,
    /// render a single concatenated document.
    pub split: bool,
}

/// The output shape of [`render_markdown`].
///
/// `Single(body)` is the concatenated single-file rendering.  `Split(files)`
/// is a list of `(filename, body)` pairs — `filename` is a basename without a
/// directory prefix (`index.md`, `structure-Board.md`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownOutput {
    /// Single concatenated Markdown document.
    Single(String),
    /// One file per declared item plus an `index.md`, in deterministic order.
    Split(Vec<(String, String)>),
}

/// Render a [`DocModel`] as GitHub-flavored Markdown.
///
/// `cross_refs` is optional so callers that haven't yet computed the inverted
/// index can still produce documentation; when `None`, the "Conforms to" /
/// "Used by" sections are omitted from each item.
///
/// Dispatches on [`MarkdownOptions::split`] to either [`render_single`] or
/// [`render_split`].
pub fn render_markdown(
    model: &DocModel,
    cross_refs: Option<&CrossRefs>,
    opts: &MarkdownOptions,
) -> MarkdownOutput {
    if opts.split {
        MarkdownOutput::Split(render_split(model, cross_refs))
    } else {
        MarkdownOutput::Single(render_single(model, cross_refs))
    }
}

/// Build the single-file concatenated Markdown body.
fn render_single(model: &DocModel, _cross_refs: Option<&CrossRefs>) -> String {
    let mut out = String::new();
    for _module in &model.modules {
        // Module rendering is added in subsequent impl steps; the empty-model
        // case yields an empty body.
    }
    out
}

/// Build the split-file (filename, body) list.
///
/// Always emits at least the `index.md` placeholder so callers can rely on its
/// presence.
fn render_split(model: &DocModel, _cross_refs: Option<&CrossRefs>) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let index_body = String::new();
    files.push(("index.md".to_string(), index_body));
    for _module in &model.modules {
        // Per-item files are added in subsequent impl steps; the empty-model
        // case yields just the index placeholder.
    }
    files
}
