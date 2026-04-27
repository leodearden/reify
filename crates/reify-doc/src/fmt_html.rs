//! Self-contained HTML5 formatter for the DocModel.
//!
//! Public surface:
//! - [`render_html`] — the single entry point that renders a [`DocModel`]
//!   (and an optional [`crate::cross_refs::CrossRefs`] index) to one
//!   self-contained HTML5 document with an embedded stylesheet.
//!
//! The formatter mirrors the section structure of [`crate::fmt_markdown`]:
//! per-module H1 + doc paragraphs, a TOC with grouped links, one `<section>`
//! per item with H2 heading and kind-specific body, and a trailing `<h2>Tests</h2>`
//! subsection for `@test`-annotated items.

use crate::cross_refs::CrossRefs;
use crate::model::{AnnotationDoc, DocModel, ItemDoc};

/// Render a [`DocModel`] as one self-contained HTML5 document.
///
/// `cross_refs` is optional so callers that haven't yet computed the inverted
/// index can still produce documentation; when `None`, the "Conforms to" /
/// "Used by" sections are omitted from each item.
///
/// The output is a single string containing a complete, browser-renderable HTML5
/// document with no external resource references (no `<link>`, `<script>`,
/// `<iframe>`, `<img>`, `@import`, `url(http://…)`, or `url(https://…)`).
pub fn render_html(model: &DocModel, _cross_refs: Option<&CrossRefs>) -> String {
    let mut out = String::new();
    // Title: the first module's path when available, otherwise a neutral default.
    let title = model
        .modules
        .first()
        .map(|m| m.path.as_str())
        .unwrap_or("reify-doc");

    out.push_str("<!DOCTYPE html>\n");
    out.push_str("<html lang=\"en\">\n");
    out.push_str("<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<title>");
    out.push_str(&html_escape(title));
    out.push_str("</title>\n");
    out.push_str("<style>\n");
    out.push_str("/* embedded stylesheet placeholder; populated in step-30 */\n");
    out.push_str("</style>\n");
    out.push_str("</head>\n");
    out.push_str("<body>\n");

    for module in &model.modules {
        out.push_str("<h1>");
        out.push_str(&html_escape(&module.path));
        out.push_str("</h1>\n");
        if let Some(doc) = module.doc.as_deref() {
            emit_paragraphs(&mut out, doc);
        }
        let non_tests: Vec<&ItemDoc> = module.items.iter().collect();
        render_toc(&mut out, &non_tests);
        for item in &module.items {
            render_item(&mut out, item);
        }
    }

    out.push_str("</body>\n");
    out.push_str("</html>\n");
    out
}

/// Emit a doc-comment string as one or more `<p>...</p>` blocks.
///
/// Splits the input on blank lines (one or more `\n\n` sequences) and writes
/// each non-empty paragraph as `<p>{trimmed}</p>` followed by a newline.
/// All-whitespace segments leave the buffer untouched so we don't produce
/// dangling empty paragraphs.
///
/// Escape the five HTML metacharacters (`<`, `>`, `&`, `"`, `'`) to their
/// named or numeric entity references.
///
/// HTML5 only requires escaping `<`/`>`/`&` outside attributes and `"`/`'`
/// inside attributes, but uniform 5-char escaping is simpler, still
/// spec-compliant, and avoids attribute-vs-content branching at every
/// emission site.  All user-supplied strings (names, types, expressions, doc
/// text, member strings, default representations, …) pass through this helper
/// before insertion so no raw user content reaches the output stream.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// Stable group label for the TOC.  "Constants" buckets the long tail of
/// value-like declarations (Field, Unit, TypeAlias, ConstraintDef, Purpose)
/// per the PRD's six-group TOC.  Mirrors `fmt_markdown::item_group`.
fn item_group(item: &ItemDoc) -> &'static str {
    match item {
        ItemDoc::Trait { .. } => "Traits",
        ItemDoc::Structure { .. } => "Structures",
        ItemDoc::Occurrence { .. } => "Occurrences",
        ItemDoc::Enum { .. } => "Enums",
        ItemDoc::Function { .. } => "Functions",
        ItemDoc::Field { .. }
        | ItemDoc::Unit { .. }
        | ItemDoc::TypeAlias { .. }
        | ItemDoc::ConstraintDef { .. }
        | ItemDoc::Purpose { .. } => "Constants",
    }
}

/// Render the table of contents inside `<nav>` with a `<h2>Contents</h2>`
/// heading and one `<h3>{Group}</h3>` plus alphabetically-sorted
/// `<li><a href="#name">name</a></li>` per non-empty group.  No-op when
/// `items` is empty.
fn render_toc(out: &mut String, items: &[&ItemDoc]) {
    if items.is_empty() {
        return;
    }
    // Fixed group order matching the PRD spec.
    const GROUPS: &[&str] = &[
        "Traits",
        "Structures",
        "Occurrences",
        "Enums",
        "Functions",
        "Constants",
    ];
    out.push_str("<nav>\n");
    out.push_str("<h2>Contents</h2>\n");
    for &group in GROUPS {
        let mut in_group: Vec<&&ItemDoc> =
            items.iter().filter(|i| item_group(i) == group).collect();
        if in_group.is_empty() {
            continue;
        }
        in_group.sort_by(|a, b| item_name(a).cmp(item_name(b)));
        out.push_str("<h3>");
        out.push_str(group);
        out.push_str("</h3>\n");
        out.push_str("<ul>\n");
        for it in in_group {
            let n = item_name(it);
            let escaped = html_escape(n);
            out.push_str("<li><a href=\"#");
            out.push_str(&escaped);
            out.push_str("\">");
            out.push_str(&escaped);
            out.push_str("</a></li>\n");
        }
        out.push_str("</ul>\n");
    }
    out.push_str("</nav>\n");
}

/// Render a single `ItemDoc` to `out` as `<section id="{name}">…</section>`.
///
/// Emits the `<h2>` heading using the visibility/keyword/name convention
/// inherited from `fmt_markdown::item_keyword`.
fn render_item(out: &mut String, item: &ItemDoc) {
    let name = item_name(item);
    let kw = item_keyword(item);
    let vis = if item_is_pub(item) { "pub " } else { "" };

    out.push_str("<section id=\"");
    out.push_str(&html_escape(name));
    out.push_str("\">\n");
    out.push_str("<h2>");
    out.push_str(vis);
    out.push_str(kw);
    out.push(' ');
    out.push_str(&html_escape(name));
    out.push_str("</h2>\n");
    out.push_str("</section>\n");
}

/// Reify-source keyword displayed in the H2 heading for each `ItemDoc` variant.
///
/// Matches the conventions in `fmt_markdown::item_keyword` so the two
/// formatters present the same surface vocabulary.  Differences from the
/// JSON kind tag: `Field → "let"`, `TypeAlias → "type"`,
/// `ConstraintDef → "constraint"`.
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
#[allow(dead_code)]
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

/// Lookup the annotations attached to any `ItemDoc` variant.
#[allow(dead_code)]
fn item_annotations(item: &ItemDoc) -> &[AnnotationDoc] {
    match item {
        ItemDoc::Structure { annotations, .. }
        | ItemDoc::Occurrence { annotations, .. }
        | ItemDoc::Trait { annotations, .. }
        | ItemDoc::Function { annotations, .. }
        | ItemDoc::Field { annotations, .. }
        | ItemDoc::Purpose { annotations, .. }
        | ItemDoc::Enum { annotations, .. }
        | ItemDoc::Unit { annotations, .. }
        | ItemDoc::TypeAlias { annotations, .. }
        | ItemDoc::ConstraintDef { annotations, .. } => annotations,
    }
}

/// Mirrors the iteration logic of [`crate::fmt_markdown::emit_paragraphs`].
fn emit_paragraphs(out: &mut String, doc: &str) {
    for paragraph in doc.split("\n\n") {
        let p = paragraph.trim();
        if p.is_empty() {
            continue;
        }
        out.push_str("<p>");
        out.push_str(&html_escape(p));
        out.push_str("</p>\n");
    }
}
