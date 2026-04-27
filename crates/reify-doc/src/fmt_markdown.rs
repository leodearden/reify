//! GitHub-flavored Markdown formatter for the DocModel.
//!
//! Public surface:
//! - [`MarkdownOptions`] — knobs that control rendering (currently only `split`).
//! - [`MarkdownOutput`] — `Single(String)` for one-blob mode, `Split(Vec<(name, body)>)`
//!   for the per-item-file mode.
//! - [`render_markdown`] — the single entry point that dispatches on
//!   [`MarkdownOptions::split`] to either single-file or split-file rendering.

use crate::cross_refs::CrossRefs;
use crate::model::{AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ParamDoc, PortDoc};

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
        // Partition items: `@test`-annotated items are deferred to a `## Tests`
        // subsection at the bottom of the module so the main flow stays focused
        // on the public API surface.
        let (non_tests, tests): (Vec<&ItemDoc>, Vec<&ItemDoc>) = module
            .items
            .iter()
            .partition(|i| find_annotation(item_annotations(i), "test").is_none());
        for item in &non_tests {
            render_item(&mut out, item, cross_refs);
        }
        if !tests.is_empty() {
            out.push_str("## Tests\n\n");
            for item in &tests {
                render_item(&mut out, item, cross_refs);
            }
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

/// Lookup the annotations attached to any `ItemDoc` variant.
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

/// Find the first annotation matching `name` in `anns`. Returns `None` if no
/// such annotation exists.
fn find_annotation<'a>(
    anns: &'a [AnnotationDoc],
    name: &str,
) -> Option<&'a AnnotationDoc> {
    anns.iter().find(|a| a.name == name)
}

/// Strip surrounding `"`s from a rendered string-literal annotation argument.
///
/// Annotations source-render `@deprecated("msg")` as the arg `"\"msg\""` —
/// the literal quote characters are *part of* the rendered representation.
/// Markdown output should display the message text without those wrapping
/// quotes; non-string-literal args (calls, identifiers, numbers) are returned
/// unchanged.
fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Render a single `ItemDoc` to `out`.
///
/// Emits the H2 heading with explicit anchor, then the optional doc paragraphs.
/// For container variants (Structure / Occurrence) renders the parameters and
/// ports tables.  Other kind-specific bodies are added in subsequent impl steps.
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

    // Annotation-driven prefix sections, emitted BETWEEN the heading and the
    // doc-comment paragraphs so the most operationally significant tags appear
    // first to the reader.
    let anns = item_annotations(item);
    if let Some(dep) = find_annotation(anns, "deprecated") {
        let msg = dep.args.first().map(|s| unquote(s)).unwrap_or("");
        out.push_str("> **Deprecated:**");
        if !msg.is_empty() {
            out.push(' ');
            out.push_str(msg);
        }
        out.push_str("\n\n");
    }
    if let Some(opt) = find_annotation(anns, "optimized") {
        let target = opt.args.first().map(|s| unquote(s)).unwrap_or("");
        out.push_str("*Optimized: `");
        out.push_str(target);
        out.push_str("`*\n\n");
    }

    if let Some(doc) = item_doc(item) {
        emit_paragraphs(out, doc);
    }

    // Kind-specific body. Container variants get parameter / port / constraint
    // / meta sections; the simpler variants emit a tiny body that mirrors the
    // language surface (members list, signature fence, type/default lines, …).
    match item {
        ItemDoc::Structure {
            params, ports, constraints, meta, ..
        }
        | ItemDoc::Occurrence {
            params, ports, constraints, meta, ..
        } => {
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
}

/// Render the `### Members` bullet list for a `Trait`. No-op when empty.
fn render_trait_members(out: &mut String, members: &[String]) {
    if members.is_empty() {
        return;
    }
    out.push_str("### Members\n\n");
    for m in members {
        out.push_str("- ");
        out.push_str(m);
        out.push('\n');
    }
    out.push('\n');
}

/// Render a fenced `reify` code block containing a `Function`'s rendered
/// signature.
fn render_function_signature(out: &mut String, signature: &str) {
    out.push_str("```reify\n");
    out.push_str(signature);
    out.push_str("\n```\n\n");
}

/// Render the `### Variants` bullet list for an `Enum`. No-op when empty.
fn render_enum_variants(out: &mut String, variants: &[String]) {
    if variants.is_empty() {
        return;
    }
    out.push_str("### Variants\n\n");
    for v in variants {
        out.push_str("- ");
        out.push_str(v);
        out.push('\n');
    }
    out.push('\n');
}

/// Render the `**Type:**` (and optional `**Default:**`) lines for a `Field`.
fn render_field_body(out: &mut String, type_repr: &str, default_repr: Option<&str>) {
    out.push_str("**Type:** `");
    out.push_str(type_repr);
    out.push_str("`\n\n");
    if let Some(d) = default_repr {
        out.push_str("**Default:** `");
        out.push_str(d);
        out.push_str("`\n\n");
    }
}

/// Render the `**Direction:**` and `**Expression:**` lines for a `Purpose`.
fn render_purpose_body(out: &mut String, direction: &str, expr_repr: &str) {
    out.push_str("**Direction:** ");
    out.push_str(direction);
    out.push_str("\n\n");
    out.push_str("**Expression:** `");
    out.push_str(expr_repr);
    out.push_str("`\n\n");
}

/// Render the `**Base:**` and `**Scale:**` lines for a `Unit`.
fn render_unit_body(out: &mut String, base_unit: &str, scale: &str) {
    out.push_str("**Base:** `");
    out.push_str(base_unit);
    out.push_str("`\n\n");
    out.push_str("**Scale:** `");
    out.push_str(scale);
    out.push_str("`\n\n");
}

/// Render the `= \`{type_repr}\`` line for a `TypeAlias`.
fn render_type_alias_body(out: &mut String, type_repr: &str) {
    out.push_str("= `");
    out.push_str(type_repr);
    out.push_str("`\n\n");
}

/// Render the `\`{expr_repr}\`` line for a `ConstraintDef`.
fn render_constraint_def_body(out: &mut String, expr_repr: &str) {
    out.push('`');
    out.push_str(expr_repr);
    out.push_str("`\n\n");
}

/// Escape a single Markdown table cell value.
///
/// `|` characters are backslash-escaped (otherwise they'd be parsed as a
/// column boundary) and embedded newlines collapse to a single space so the
/// row stays on one line.
fn md_cell_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Render the `### Parameters` GFM table.  No-op when `params` is empty.
fn render_params_table(out: &mut String, params: &[ParamDoc]) {
    if params.is_empty() {
        return;
    }
    out.push_str("### Parameters\n\n");
    out.push_str("| Name | Type | Dimension | Default | Description |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for p in params {
        let default_cell = match p.default_repr.as_deref() {
            Some(d) => format!("`{}`", md_cell_escape(d)),
            None => "—".to_string(),
        };
        // Description = doc text + optional `*hint: <solver_hint arg>*` suffix.
        let mut description = p.doc.as_deref().unwrap_or("").trim().to_string();
        if let Some(hint) = find_annotation(&p.annotations, "solver_hint") {
            let hint_arg = hint.args.first().map(|s| unquote(s)).unwrap_or("");
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str("*hint: ");
            description.push_str(hint_arg);
            description.push('*');
        }
        out.push_str("| `");
        out.push_str(&md_cell_escape(&p.name));
        out.push_str("` | `");
        out.push_str(&md_cell_escape(&p.type_repr));
        out.push_str("` | ");
        // Dimension is not exposed on ParamDoc today; emit em-dash placeholder.
        out.push_str("—");
        out.push_str(" | ");
        out.push_str(&default_cell);
        out.push_str(" | ");
        out.push_str(&md_cell_escape(&description));
        out.push_str(" |\n");
    }
    out.push('\n');
}

/// Render the `### Constraints` bullet list.  No-op when `cs` is empty.
///
/// Each entry is one of three shapes:
/// - `- `{expr}`` — no label
/// - `- {label}: `{expr}`` — labeled
/// - either of the above with a trailing ` *(line N)*` when `line.is_some()`.
fn render_constraints(out: &mut String, cs: &[ConstraintDoc]) {
    if cs.is_empty() {
        return;
    }
    out.push_str("### Constraints\n\n");
    for c in cs {
        out.push_str("- ");
        if let Some(label) = c.label.as_deref() {
            out.push_str(label);
            out.push_str(": ");
        }
        out.push('`');
        out.push_str(&c.expr_repr);
        out.push('`');
        if let Some(line) = c.line {
            out.push_str(" *(line ");
            out.push_str(&line.to_string());
            out.push_str(")*");
        }
        out.push('\n');
    }
    out.push('\n');
}

/// Render the `### Meta` bullet list, sorted alphabetically by key.
/// No-op when `meta` is empty.
fn render_meta(out: &mut String, meta: &[(String, String)]) {
    if meta.is_empty() {
        return;
    }
    out.push_str("### Meta\n\n");
    let mut sorted: Vec<&(String, String)> = meta.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in sorted {
        out.push_str("- **");
        out.push_str(k);
        out.push_str("**: ");
        out.push_str(v);
        out.push('\n');
    }
    out.push('\n');
}

/// Render the `### Ports` GFM table.  No-op when `ports` is empty.
fn render_ports_table(out: &mut String, ports: &[PortDoc]) {
    if ports.is_empty() {
        return;
    }
    out.push_str("### Ports\n\n");
    out.push_str("| Name | Kind | Role | Type | Description |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for p in ports {
        // Kind column has no PortDoc field today; emit em-dash placeholder.
        out.push_str("| `");
        out.push_str(&md_cell_escape(&p.name));
        out.push_str("` | ");
        out.push_str("—");
        out.push_str(" | ");
        out.push_str(&md_cell_escape(&p.direction));
        out.push_str(" | `");
        out.push_str(&md_cell_escape(&p.type_name));
        out.push_str("` | ");
        // Description: derived from members list, if present.
        if p.members.is_empty() {
            out.push_str("—");
        } else {
            let joined = p.members.join(", ");
            out.push_str(&md_cell_escape(&joined));
        }
        out.push_str(" |\n");
    }
    out.push('\n');
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
