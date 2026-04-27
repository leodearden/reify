//! GitHub-flavored Markdown formatter for the DocModel.
//!
//! Public surface:
//! - [`MarkdownOptions`] — knobs that control rendering (currently only `split`).
//! - [`MarkdownOutput`] — `Single(String)` for one-blob mode, `Split(Vec<(name, body)>)`
//!   for the per-item-file mode.
//! - `render_markdown(&DocModel, Option<&CrossRefs>, &MarkdownOptions) -> MarkdownOutput`
//!   (added in a later step).

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
