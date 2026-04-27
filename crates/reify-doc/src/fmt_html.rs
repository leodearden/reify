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
use crate::model::{AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ParamDoc, PortDoc};

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

    // Kind-specific body.
    match item {
        ItemDoc::Structure { params, ports, constraints, meta, .. }
        | ItemDoc::Occurrence { params, ports, constraints, meta, .. } => {
            render_params_table(out, params);
            render_ports_table(out, ports);
            render_constraints(out, constraints);
            render_meta(out, meta);
        }
        ItemDoc::Trait { members, .. } => {
            render_trait_members(out, members);
        }
        ItemDoc::Function { signature, .. } => {
            render_function_signature(out, signature);
        }
        ItemDoc::Enum { variants, .. } => {
            render_enum_variants(out, variants);
        }
        // Scalar-bodied variants (Field/Purpose/Unit/TypeAlias/ConstraintDef)
        // get their bodies wired up in step-22.
        ItemDoc::Field { .. }
        | ItemDoc::Purpose { .. }
        | ItemDoc::Unit { .. }
        | ItemDoc::TypeAlias { .. }
        | ItemDoc::ConstraintDef { .. } => {}
    }

    out.push_str("</section>\n");
}

/// Find the first annotation matching `name` in `anns`. Returns `None` if no
/// such annotation exists.  Mirrors `fmt_markdown::find_annotation`.
#[allow(dead_code)]
fn find_annotation<'a>(
    anns: &'a [AnnotationDoc],
    name: &str,
) -> Option<&'a AnnotationDoc> {
    anns.iter().find(|a| a.name == name)
}

/// Strip surrounding `"`s from a rendered string-literal annotation argument.
/// Mirrors `fmt_markdown::unquote`.
#[allow(dead_code)]
fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Render the `<h3>Parameters</h3>` table.  No-op when `params` is empty.
///
/// Columns: Name | Type | Dimension | Default | Description.  Name and Type
/// cells wrap in `<code>` for visual distinction; Dimension is an em-dash
/// placeholder (mirroring markdown — the model has no `dimension` field
/// today); Default uses `<code>` when `Some`, em-dash when `None`; Description
/// is the `doc` text plus an `<em>hint: {arg}</em>` suffix when a
/// `solver_hint` annotation is present on the parameter.
fn render_params_table(out: &mut String, params: &[ParamDoc]) {
    if params.is_empty() {
        return;
    }
    out.push_str("<h3>Parameters</h3>\n");
    out.push_str("<table>\n");
    out.push_str("<thead><tr>");
    out.push_str("<th>Name</th><th>Type</th><th>Dimension</th><th>Default</th><th>Description</th>");
    out.push_str("</tr></thead>\n");
    out.push_str("<tbody>\n");
    for p in params {
        out.push_str("<tr>");
        // Name
        out.push_str("<td><code>");
        out.push_str(&html_escape(&p.name));
        out.push_str("</code></td>");
        // Type
        out.push_str("<td><code>");
        out.push_str(&html_escape(&p.type_repr));
        out.push_str("</code></td>");
        // Dimension placeholder (em-dash).
        out.push_str("<td>—</td>");
        // Default
        match p.default_repr.as_deref() {
            Some(d) => {
                out.push_str("<td><code>");
                out.push_str(&html_escape(d));
                out.push_str("</code></td>");
            }
            None => out.push_str("<td>—</td>"),
        }
        // Description = doc text + optional <em>hint: ...</em> suffix.
        out.push_str("<td>");
        let doc_text = p.doc.as_deref().unwrap_or("").trim();
        if !doc_text.is_empty() {
            out.push_str(&html_escape(doc_text));
        }
        if let Some(hint) = find_annotation(&p.annotations, "solver_hint") {
            let hint_arg = hint.args.first().map(|s| unquote(s)).unwrap_or("");
            if !doc_text.is_empty() {
                out.push(' ');
            }
            out.push_str("<em>hint: ");
            out.push_str(&html_escape(hint_arg));
            out.push_str("</em>");
        }
        out.push_str("</td>");
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n");
    out.push_str("</table>\n");
}

/// Render the `<h3>Ports</h3>` table.  No-op when `ports` is empty.
///
/// Columns: Name | Kind | Role | Type | Description.  Name and Type wrap
/// in `<code>`; Kind has no `PortDoc` field so it's an em-dash placeholder
/// (mirrors markdown); Role is the direction; Description joins members
/// with `, ` and uses an em-dash when empty.
fn render_ports_table(out: &mut String, ports: &[PortDoc]) {
    if ports.is_empty() {
        return;
    }
    out.push_str("<h3>Ports</h3>\n");
    out.push_str("<table>\n");
    out.push_str("<thead><tr>");
    out.push_str("<th>Name</th><th>Kind</th><th>Role</th><th>Type</th><th>Description</th>");
    out.push_str("</tr></thead>\n");
    out.push_str("<tbody>\n");
    for p in ports {
        out.push_str("<tr>");
        out.push_str("<td><code>");
        out.push_str(&html_escape(&p.name));
        out.push_str("</code></td>");
        out.push_str("<td>—</td>");
        out.push_str("<td>");
        out.push_str(&html_escape(&p.direction));
        out.push_str("</td>");
        out.push_str("<td><code>");
        out.push_str(&html_escape(&p.type_name));
        out.push_str("</code></td>");
        out.push_str("<td>");
        if p.members.is_empty() {
            out.push_str("—");
        } else {
            let joined = p.members.join(", ");
            out.push_str(&html_escape(&joined));
        }
        out.push_str("</td>");
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n");
    out.push_str("</table>\n");
}

/// Render the `<h3>Members</h3>` bullet list for a `Trait`.  No-op when empty.
///
/// Each member is one rendered signature string (e.g. `"voltage: Voltage"`)
/// emitted as `<li>{escaped-member}</li>`.
fn render_trait_members(out: &mut String, members: &[String]) {
    if members.is_empty() {
        return;
    }
    out.push_str("<h3>Members</h3>\n");
    out.push_str("<ul>\n");
    for m in members {
        out.push_str("<li>");
        out.push_str(&html_escape(m));
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
}

/// Render the `<h3>Variants</h3>` bullet list for an `Enum`.  No-op when empty.
///
/// Each variant name is emitted as `<li>{escaped-name}</li>`.
fn render_enum_variants(out: &mut String, variants: &[String]) {
    if variants.is_empty() {
        return;
    }
    out.push_str("<h3>Variants</h3>\n");
    out.push_str("<ul>\n");
    for v in variants {
        out.push_str("<li>");
        out.push_str(&html_escape(v));
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
}

/// Render a function signature inside `<pre><code>…</code></pre>`.
///
/// The signature passes through `html_escape` so embedded `<` / `>` / `&`
/// characters (notably the `>` in `->`) survive as their entity references.
/// Per PRD §HTML there is no syntax highlighting in v0.1, so no class hint
/// is set on `<code>`.
fn render_function_signature(out: &mut String, signature: &str) {
    out.push_str("<pre><code>");
    out.push_str(&html_escape(signature));
    out.push_str("</code></pre>\n");
}

/// Render the `<h3>Meta</h3>` definition list, sorted alphabetically by key.
/// No-op when `meta` is empty.
///
/// Emits `<h3>Meta</h3><dl>` then `<dt>{escaped-key}</dt><dd>{escaped-value}</dd>`
/// pairs sorted by key, then `</dl>`.  Mirrors `fmt_markdown::render_meta`'s
/// alphabetical-by-key contract so the two formatters present meta entries in
/// the same order regardless of insertion order in the model.
fn render_meta(out: &mut String, meta: &[(String, String)]) {
    if meta.is_empty() {
        return;
    }
    let mut sorted: Vec<&(String, String)> = meta.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    out.push_str("<h3>Meta</h3>\n");
    out.push_str("<dl>\n");
    for (k, v) in sorted {
        out.push_str("<dt>");
        out.push_str(&html_escape(k));
        out.push_str("</dt><dd>");
        out.push_str(&html_escape(v));
        out.push_str("</dd>\n");
    }
    out.push_str("</dl>\n");
}

/// Render the `<h3>Constraints</h3>` bullet list.  No-op when `cs` is empty.
///
/// Mirrors the markdown formatter's three entry shapes:
/// - `<li><code>{escaped-expr}</code></li>` — labelless, no line
/// - `<li>{escaped-label}: <code>{escaped-expr}</code></li>` — labelled
/// - either of the above with a trailing ` <em>(line N)</em>` when
///   `line.is_some()`.
fn render_constraints(out: &mut String, cs: &[ConstraintDoc]) {
    if cs.is_empty() {
        return;
    }
    out.push_str("<h3>Constraints</h3>\n");
    out.push_str("<ul>\n");
    for c in cs {
        out.push_str("<li>");
        if let Some(label) = c.label.as_deref() {
            out.push_str(&html_escape(label));
            out.push_str(": ");
        }
        out.push_str("<code>");
        out.push_str(&html_escape(&c.expr_repr));
        out.push_str("</code>");
        if let Some(line) = c.line {
            out.push_str(" <em>(line ");
            out.push_str(&line.to_string());
            out.push_str(")</em>");
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
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
