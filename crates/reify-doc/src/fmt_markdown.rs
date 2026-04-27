//! GitHub-flavored Markdown formatter for the DocModel.
//!
//! Public surface:
//! - [`MarkdownOptions`] — knobs that control rendering (currently only `split`).
//! - [`MarkdownOutput`] — `Single(String)` for one-blob mode, `Split(Vec<(name, body)>)`
//!   for the per-item-file mode.
//! - [`render_markdown`] — the single entry point that dispatches on
//!   [`MarkdownOptions::split`] to either single-file or split-file rendering.

use crate::cross_refs::CrossRefs;
use crate::model::{DocModel, ItemDoc};

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
fn render_single(model: &DocModel, cross_refs: Option<&CrossRefs>) -> String {
    let mut out = String::new();
    for module in &model.modules {
        // Module H1 header.
        out.push_str("# ");
        out.push_str(&module.path);
        out.push_str("\n\n");
        // Optional module doc.
        if let Some(doc) = module.doc.as_deref() {
            emit_paragraphs(&mut out, doc);
        }
        for item in &module.items {
            render_item(&mut out, item, cross_refs);
        }
    }
    out
}

/// Emit a doc-comment string as one or more Markdown paragraphs.
///
/// Splits the input on blank lines (one or more `\n\n` sequences) and writes
/// each non-empty paragraph followed by a blank line, so the next thing emitted
/// after the call starts on a fresh paragraph.
fn emit_paragraphs(out: &mut String, doc: &str) {
    let mut wrote_any = false;
    for paragraph in doc.split("\n\n") {
        let p = paragraph.trim();
        if p.is_empty() {
            continue;
        }
        out.push_str(p);
        out.push_str("\n\n");
        wrote_any = true;
    }
    // If the doc was all whitespace, leave the buffer untouched so we don't
    // produce dangling blank lines.
    let _ = wrote_any;
}

/// Language keyword displayed in the H2 heading for each `ItemDoc` variant.
///
/// Matches the snake_case kind tag used by `#[serde(tag="kind", rename_all="snake_case")]`
/// on `ItemDoc`, except for variants whose Reify-source keyword differs from the
/// JSON tag (e.g. `Field` → `let`, `TypeAlias` → `type`, `ConstraintDef` →
/// `constraint`).
fn item_keyword(item: &ItemDoc) -> &'static str {
    match item {
        ItemDoc::Structure { .. } => "structure",
        ItemDoc::Occurrence { .. } => "occurrence",
        ItemDoc::Trait { .. } => "trait",
        ItemDoc::Function { .. } => "fn",
        ItemDoc::Field { .. } => "let",
        ItemDoc::Purpose { .. } => "purpose",
        ItemDoc::Enum { .. } => "enum",
        ItemDoc::Unit { .. } => "unit",
        ItemDoc::TypeAlias { .. } => "type",
        ItemDoc::ConstraintDef { .. } => "constraint",
    }
}

/// Lookup the `name` field of any `ItemDoc` variant.
fn item_name(item: &ItemDoc) -> &str {
    match item {
        ItemDoc::Structure { name, .. }
        | ItemDoc::Occurrence { name, .. }
        | ItemDoc::Trait { name, .. }
        | ItemDoc::Function { name, .. }
        | ItemDoc::Field { name, .. }
        | ItemDoc::Purpose { name, .. }
        | ItemDoc::Enum { name, .. }
        | ItemDoc::Unit { name, .. }
        | ItemDoc::TypeAlias { name, .. }
        | ItemDoc::ConstraintDef { name, .. } => name,
    }
}

/// Lookup the `is_pub` field of any `ItemDoc` variant.
fn item_is_pub(item: &ItemDoc) -> bool {
    match item {
        ItemDoc::Structure { is_pub, .. }
        | ItemDoc::Occurrence { is_pub, .. }
        | ItemDoc::Trait { is_pub, .. }
        | ItemDoc::Function { is_pub, .. }
        | ItemDoc::Field { is_pub, .. }
        | ItemDoc::Purpose { is_pub, .. }
        | ItemDoc::Enum { is_pub, .. }
        | ItemDoc::Unit { is_pub, .. }
        | ItemDoc::TypeAlias { is_pub, .. }
        | ItemDoc::ConstraintDef { is_pub, .. } => *is_pub,
    }
}

/// Lookup the optional doc-comment of any `ItemDoc` variant.
fn item_doc(item: &ItemDoc) -> Option<&str> {
    match item {
        ItemDoc::Structure { doc, .. }
        | ItemDoc::Occurrence { doc, .. }
        | ItemDoc::Trait { doc, .. }
        | ItemDoc::Function { doc, .. }
        | ItemDoc::Field { doc, .. }
        | ItemDoc::Purpose { doc, .. }
        | ItemDoc::Enum { doc, .. }
        | ItemDoc::Unit { doc, .. }
        | ItemDoc::TypeAlias { doc, .. }
        | ItemDoc::ConstraintDef { doc, .. } => doc.as_deref(),
    }
}

/// Render a single `ItemDoc` to `out`.
///
/// Emits the H2 heading with explicit anchor, then the optional doc paragraphs.
/// Kind-specific bodies (params/ports tables, signatures, members, etc.) are
/// added in subsequent impl steps; this stub focuses on the heading shape.
fn render_item(out: &mut String, item: &ItemDoc, _cross_refs: Option<&CrossRefs>) {
    let name = item_name(item);
    let kw = item_keyword(item);
    let vis = if item_is_pub(item) { "pub " } else { "" };

    out.push_str("## `");
    out.push_str(vis);
    out.push_str(kw);
    out.push(' ');
    out.push_str(name);
    out.push_str("` <a id=\"");
    out.push_str(name);
    out.push_str("\"></a>\n\n");

    if let Some(doc) = item_doc(item) {
        emit_paragraphs(out, doc);
    }
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
