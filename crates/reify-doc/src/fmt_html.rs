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

use std::collections::BTreeMap;

use crate::cross_refs::CrossRefs;
use crate::model::{AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ParamDoc, PortDoc};

/// Hand-written CSS embedded inside the document's `<style>` block.
///
/// Constraints (asserted by `embedded_stylesheet_meets_constraints`):
/// - sticky TOC (`position: sticky; top: 0;`)
/// - bounded body width (`max-width: 900px`)
/// - explicit monospace fallback for code/pre
/// - `line-height` ≥ 1.4 for readable paragraphs
/// - no `@import`, no remote `url(...)` references (self-contained)
/// - ≤100 non-empty lines (so the document inlining cost stays modest)
const EMBEDDED_STYLESHEET: &str = "\
body {
  max-width: 900px;
  margin: 2em auto;
  padding: 0 1em;
  font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif;
  line-height: 1.6;
  color: #222;
}
nav {
  position: sticky;
  top: 0;
  background: #fff;
  padding: 0.5em 0;
  border-bottom: 1px solid #eee;
  margin-bottom: 1.5em;
}
nav h2 {
  margin: 0 0 0.25em 0;
}
nav h3 {
  margin: 0.5em 0 0.25em 0;
}
nav ul {
  margin: 0 0 0.5em 1em;
  padding: 0;
}
code, pre {
  font-family: ui-monospace, Menlo, Consolas, monospace;
}
pre {
  padding: 0.5em;
  background: #f6f8fa;
  overflow-x: auto;
}
table {
  border-collapse: collapse;
  width: 100%;
  margin: 1em 0;
}
th, td {
  border: 1px solid #ddd;
  padding: 0.4em 0.6em;
  text-align: left;
  vertical-align: top;
}
th {
  background: #f6f8fa;
}
dl {
  margin: 0.5em 0 1em 0;
}
dt {
  font-weight: bold;
}
dd {
  margin: 0 0 0.5em 1em;
}
h1, h2, h3 {
  line-height: 1.2;
}
section {
  margin: 1.5em 0;
}
aside.deprecated {
  padding: 0.5em 1em;
  background: #fff8e1;
  border-left: 4px solid #f0ad4e;
  margin: 0.5em 0;
}
p.optimized {
  color: #555;
  margin: 0.25em 0;
}
";

/// A `CrossRefs` plus a precomputed *inverse* map from conformer name to the
/// list of traits the conformer implements.
///
/// Mirrors `fmt_markdown::CrossRefIndex`: building the inverse once at the
/// entry point of [`render_html`] turns the per-item "Conforms to" lookup
/// from an O(traits × avg_conformers) scan into a single O(log N) BTreeMap
/// lookup.
struct CrossRefIndex<'a> {
    cross_refs: &'a CrossRefs,
    /// Inverse of `cross_refs.trait_to_conformers`.  Each value list is
    /// sorted and deduplicated so the rendered `<h3>Conforms to</h3>` bullet
    /// list is deterministic without per-item resorting.
    conformer_to_traits: BTreeMap<&'a str, Vec<&'a str>>,
}

impl<'a> CrossRefIndex<'a> {
    fn new(cross_refs: &'a CrossRefs) -> Self {
        let mut conformer_to_traits: BTreeMap<&'a str, Vec<&'a str>> = BTreeMap::new();
        for (trait_name, conformers) in &cross_refs.trait_to_conformers {
            for conformer in conformers {
                conformer_to_traits
                    .entry(conformer.as_str())
                    .or_default()
                    .push(trait_name.as_str());
            }
        }
        for v in conformer_to_traits.values_mut() {
            v.sort();
            v.dedup();
        }
        Self {
            cross_refs,
            conformer_to_traits,
        }
    }
}

/// Render a [`DocModel`] as one self-contained HTML5 document.
///
/// `cross_refs` is optional so callers that haven't yet computed the inverted
/// index can still produce documentation; when `None`, the "Conforms to" /
/// "Used by" sections are omitted from each item.
///
/// The output is a single string containing a complete, browser-renderable HTML5
/// document with no external resource references (no `<link>`, `<script>`,
/// `<iframe>`, `<img>`, `@import`, `url(http://…)`, or `url(https://…)`).
pub fn render_html(model: &DocModel, cross_refs: Option<&CrossRefs>) -> String {
    let xref_index = cross_refs.map(CrossRefIndex::new);
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
    out.push_str(EMBEDDED_STYLESHEET);
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
        // Partition items: `@test`-annotated items are deferred to a
        // `<h2>Tests</h2>` subsection at the bottom of the module so the main
        // flow stays focused on the public API surface.  Mirrors
        // `fmt_markdown::render_single`'s partition step.
        let (non_tests, tests): (Vec<&ItemDoc>, Vec<&ItemDoc>) = module
            .items
            .iter()
            .partition(|i| find_annotation(item_annotations(i), "test").is_none());
        // Table of contents covers non-tests only.
        render_toc(&mut out, &non_tests);
        for item in &non_tests {
            render_item(&mut out, item, xref_index.as_ref());
        }
        if !tests.is_empty() {
            out.push_str("<h2>Tests</h2>\n");
            for item in &tests {
                render_item(&mut out, item, xref_index.as_ref());
            }
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
fn render_item(out: &mut String, item: &ItemDoc, xrefs: Option<&CrossRefIndex<'_>>) {
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

    // Annotation-driven prefix sections, emitted BETWEEN the heading and the
    // doc-comment paragraphs so the most operationally significant tags appear
    // first to the reader.  Mirrors `fmt_markdown::render_item`'s ordering.
    let anns = item_annotations(item);
    if let Some(dep) = find_annotation(anns, "deprecated") {
        let msg = dep.args.first().map(|s| unquote(s)).unwrap_or("");
        out.push_str("<aside class=\"deprecated\"><strong>Deprecated:</strong>");
        if !msg.is_empty() {
            out.push(' ');
            out.push_str(&html_escape(msg));
        }
        out.push_str("</aside>\n");
    }
    if let Some(opt) = find_annotation(anns, "optimized") {
        let target = opt.args.first().map(|s| unquote(s)).unwrap_or("");
        out.push_str("<p class=\"optimized\"><em>Optimized: <code>");
        out.push_str(&html_escape(target));
        out.push_str("</code></em></p>\n");
    }

    // Item-level doc paragraphs (split on blank lines, emitted as `<p>...</p>`).
    if let Some(doc) = item_doc(item) {
        emit_paragraphs(out, doc);
    }

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
        ItemDoc::Field { type_repr, default_repr, .. } => {
            render_field_body(out, type_repr, default_repr.as_deref());
        }
        ItemDoc::Purpose { direction, expr_repr, .. } => {
            render_purpose_body(out, direction, expr_repr);
        }
        ItemDoc::Unit { base_unit, scale, .. } => {
            render_unit_body(out, base_unit, scale);
        }
        ItemDoc::TypeAlias { type_repr, .. } => {
            render_type_alias_body(out, type_repr);
        }
        ItemDoc::ConstraintDef { expr_repr, .. } => {
            render_constraint_def_body(out, expr_repr);
        }
    }

    // Cross-ref sections (Conforms to / Used by) come AFTER the kind-specific
    // body, so the most-relevant declaration data renders first and the
    // outward-pointing links sit at the bottom of the section.
    render_cross_refs(out, name, xrefs);

    out.push_str("</section>\n");
}

/// Render the `<h3>Conforms to</h3>` and `<h3>Used by</h3>` sections from
/// `xrefs`, keyed on this item's name.  Each section is emitted only when its
/// link list is non-empty.
///
/// "Conforms to" reads from `xrefs.conformer_to_traits` — the precomputed
/// inverse of `trait_to_conformers` — so each lookup is O(log N).  "Used by"
/// reads from `xrefs.cross_refs.entity_to_containers` directly.  Both lists
/// are sorted and deduplicated before emission.  Mirrors
/// `fmt_markdown::render_cross_refs`.
fn render_cross_refs(out: &mut String, name: &str, xrefs: Option<&CrossRefIndex<'_>>) {
    let Some(xrefs) = xrefs else {
        return;
    };
    if let Some(traits) = xrefs.conformer_to_traits.get(name)
        && !traits.is_empty()
    {
        out.push_str("<h3>Conforms to</h3>\n");
        out.push_str("<ul>\n");
        for t in traits {
            let escaped = html_escape(t);
            out.push_str("<li><a href=\"#");
            out.push_str(&escaped);
            out.push_str("\">");
            out.push_str(&escaped);
            out.push_str("</a></li>\n");
        }
        out.push_str("</ul>\n");
    }

    if let Some(containers) = xrefs.cross_refs.entity_to_containers.get(name)
        && !containers.is_empty()
    {
        let mut sorted: Vec<&str> = containers.iter().map(|s| s.as_str()).collect();
        sorted.sort();
        sorted.dedup();
        out.push_str("<h3>Used by</h3>\n");
        out.push_str("<ul>\n");
        for c in sorted {
            let escaped = html_escape(c);
            out.push_str("<li><a href=\"#");
            out.push_str(&escaped);
            out.push_str("\">");
            out.push_str(&escaped);
            out.push_str("</a></li>\n");
        }
        out.push_str("</ul>\n");
    }
}

/// Find the first annotation matching `name` in `anns`. Returns `None` if no
/// such annotation exists.  Mirrors `fmt_markdown::find_annotation`.
fn find_annotation<'a>(
    anns: &'a [AnnotationDoc],
    name: &str,
) -> Option<&'a AnnotationDoc> {
    anns.iter().find(|a| a.name == name)
}

/// Strip surrounding `"`s from a rendered string-literal annotation argument.
/// Mirrors `fmt_markdown::unquote`.
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
            out.push('—');
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

/// Render the Type / optional Default rows for a `Field`.
///
/// `<dl><dt>Type</dt><dd><code>{type_repr}</code></dd>` always; the
/// `<dt>Default</dt><dd><code>{default_repr}</code></dd>` row is appended
/// only when `default_repr` is `Some`.  Mirrors `fmt_markdown::render_field_body`.
fn render_field_body(out: &mut String, type_repr: &str, default_repr: Option<&str>) {
    out.push_str("<dl>\n");
    out.push_str("<dt>Type</dt><dd><code>");
    out.push_str(&html_escape(type_repr));
    out.push_str("</code></dd>\n");
    if let Some(d) = default_repr {
        out.push_str("<dt>Default</dt><dd><code>");
        out.push_str(&html_escape(d));
        out.push_str("</code></dd>\n");
    }
    out.push_str("</dl>\n");
}

/// Render the Direction / Expression rows for a `Purpose`.
///
/// `<dl><dt>Direction</dt><dd>{direction}</dd><dt>Expression</dt><dd><code>{expr}</code></dd></dl>`.
/// Direction is plain text; the expression wraps in `<code>`.
fn render_purpose_body(out: &mut String, direction: &str, expr_repr: &str) {
    out.push_str("<dl>\n");
    out.push_str("<dt>Direction</dt><dd>");
    out.push_str(&html_escape(direction));
    out.push_str("</dd>\n");
    out.push_str("<dt>Expression</dt><dd><code>");
    out.push_str(&html_escape(expr_repr));
    out.push_str("</code></dd>\n");
    out.push_str("</dl>\n");
}

/// Render the Base / Scale rows for a `Unit`.
///
/// `<dl><dt>Base</dt><dd><code>{base_unit}</code></dd><dt>Scale</dt><dd><code>{scale}</code></dd></dl>`.
fn render_unit_body(out: &mut String, base_unit: &str, scale: &str) {
    out.push_str("<dl>\n");
    out.push_str("<dt>Base</dt><dd><code>");
    out.push_str(&html_escape(base_unit));
    out.push_str("</code></dd>\n");
    out.push_str("<dt>Scale</dt><dd><code>");
    out.push_str(&html_escape(scale));
    out.push_str("</code></dd>\n");
    out.push_str("</dl>\n");
}

/// Render the `= <code>{type}</code>` line for a `TypeAlias`.
fn render_type_alias_body(out: &mut String, type_repr: &str) {
    out.push_str("<p>= <code>");
    out.push_str(&html_escape(type_repr));
    out.push_str("</code></p>\n");
}

/// Render the `<code>{expr}</code>` line for a `ConstraintDef`.
fn render_constraint_def_body(out: &mut String, expr_repr: &str) {
    out.push_str("<p><code>");
    out.push_str(&html_escape(expr_repr));
    out.push_str("</code></p>\n");
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
