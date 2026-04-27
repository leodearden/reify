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
use crate::model::DocModel;

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
