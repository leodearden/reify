//! GitHub-flavored Markdown formatter for the DocModel.
//!
//! Public surface:
//! - [`MarkdownOptions`] — knobs that control rendering (currently only `split`).
//! - [`MarkdownOutput`] — `Single(String)` for one-blob mode, `Split(Vec<(name, body)>)`
//!   for the per-item-file mode.
//! - [`render_markdown`] — the single entry point that dispatches on
//!   [`MarkdownOptions::split`] to either single-file or split-file rendering.

use std::collections::BTreeMap;

use crate::cross_refs::CrossRefs;
use crate::model::{AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ParamDoc, PortDoc};

/// A `CrossRefs` plus a precomputed *inverse* map from conformer name to the
/// list of traits the conformer implements.
///
/// `CrossRefs::trait_to_conformers` answers "for trait T, which items conform?";
/// the inverse `conformer_to_traits` answers "for item N, which traits does it
/// conform to?".  Building the inverse once at the entry point of
/// [`render_single`] / [`render_split`] turns the per-item "Conforms to" lookup
/// from an O(traits × avg_conformers) scan into a single O(log N) BTreeMap
/// lookup.  See suggestion #3 in the reviewer's amendment notes.
struct CrossRefIndex<'a> {
    cross_refs: &'a CrossRefs,
    /// Inverse of `cross_refs.trait_to_conformers`.  Each value list is
    /// sorted and deduplicated so the rendered `### Conforms to` bullet list
    /// is deterministic without per-item resorting.
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

/// A precomputed name-to-kind-and-module index used by cross-reference link
/// resolvers in split-mode rendering.
///
/// `by_name` maps each item name to the list of `(kind_slug, module_path)`
/// pairs that declare an item with that name.  A name appearing in exactly one
/// location can be resolved unambiguously via [`NameIndex::unique_resolve`];
/// names that appear in multiple modules (or not at all) fall back to the
/// caller's default (typically the fragment form `#Name`).
///
/// **TOC rendering** does not use this index — the TOC resolver receives the
/// full `&ItemDoc` so the kind is known directly without any lookup.  This
/// index is used only by `render_cross_refs`, where only the referenced item's
/// name is available (not its `ItemDoc`).
struct NameIndex<'a> {
    by_name: BTreeMap<&'a str, Vec<(&'static str, &'a str)>>,
}

impl<'a> NameIndex<'a> {
    /// Walk all modules in `model` and build the index.
    fn new(model: &'a DocModel) -> Self {
        let mut by_name: BTreeMap<&'a str, Vec<(&'static str, &'a str)>> = BTreeMap::new();
        for module in &model.modules {
            for item in &module.items {
                let name = item.name();
                let kind = item.kind_slug();
                by_name
                    .entry(name)
                    .or_default()
                    .push((kind, module.path.as_str()));
            }
        }
        Self { by_name }
    }

    /// Returns `Some((kind_slug, module_path))` only when `name` maps to
    /// exactly one entry (unambiguous resolution).  Returns `None` on miss or
    /// multi-module name collision.
    ///
    /// On `None` the caller should fall back to a fragment link (`#Name`).
    /// Genuine multi-module collisions (same name in two modules) are
    /// ambiguous because `CrossRefs` carries only bare names; guessing one
    /// module would silently mislead the reader.  See the design decision note
    /// in plan.json for the rationale and `multi_module_split_cross_ref_ambiguous_name_falls_back_to_fragment`
    /// for a regression test that pins this fallback.
    fn unique_resolve(&self, name: &str) -> Option<(&'static str, &'a str)> {
        let entries = self.by_name.get(name)?;
        if entries.len() == 1 {
            Some(entries[0])
        } else {
            None
        }
    }
}

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
    let xref_index = cross_refs.map(CrossRefIndex::new);
    if opts.split {
        MarkdownOutput::Split(render_split(model, xref_index.as_ref()))
    } else {
        MarkdownOutput::Single(render_single(model, xref_index.as_ref()))
    }
}

/// Build the single-file concatenated Markdown body.
fn render_single(model: &DocModel, xrefs: Option<&CrossRefIndex<'_>>) -> String {
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
            .partition(|i| find_annotation(i.annotations(), "test").is_none());
        // Table of contents — sits between the H1/doc and the first item H2.
        // Single-file mode: all items live in the same document so fragment
        // links (`#Name`) are correct and stable.
        //
        // Two distinct resolvers are needed: `toc_resolver` receives `&ItemDoc`
        // (the new signature for render_toc/render_toc_groups so callers can
        // derive the kind without a name-index lookup), while `xref_resolver`
        // receives a bare `&str` name (render_cross_refs has only the name of
        // the referenced item, not its ItemDoc).
        let toc_resolver = |item: &ItemDoc| format!("#{}", item.name());
        let xref_resolver = |name: &str| format!("#{name}");
        render_toc(&mut out, &non_tests, &toc_resolver);
        for item in &non_tests {
            render_item(&mut out, item, xrefs, &xref_resolver);
        }
        if !tests.is_empty() {
            out.push_str("## Tests\n\n");
            for item in &tests {
                render_item(&mut out, item, xrefs, &xref_resolver);
            }
        }
    }
    out
}

/// Render only the `### {Kind}` groups with their bullet link lists.
///
/// Emits one H3 per non-empty group with alphabetically-sorted items inside.
/// Empty groups are omitted.  Each bullet calls `resolve_link(item)` to
/// obtain the link target string, so callers control whether that is a
/// fragment (`#Name`), a sibling filename (`kind-Name.md`), or a
/// module-qualified path (`module/kind-Name.md`).
///
/// The resolver receives the full `&ItemDoc` — not just the name — so it can
/// derive the exact kind slug directly without a name-index lookup.  This
/// avoids incorrect link targets when two items in the same module share a
/// name but differ by kind (e.g. a trait and a constant both named `Foo`).
///
/// This inner helper is called by [`render_toc`] (which wraps it with
/// `## Contents`) and — in multi-module split mode — by `render_split`
/// directly (which wraps it with `## {module}` instead of `## Contents`).
fn render_toc_groups(
    out: &mut String,
    items: &[&ItemDoc],
    resolve_link: &dyn Fn(&ItemDoc) -> String,
) {
    // Fixed group order matching the PRD spec.
    const GROUPS: &[&str] = &[
        "Traits",
        "Structures",
        "Occurrences",
        "Enums",
        "Functions",
        "Constants",
    ];
    for &group in GROUPS {
        let mut in_group: Vec<&&ItemDoc> =
            items.iter().filter(|i| i.group() == group).collect();
        if in_group.is_empty() {
            continue;
        }
        in_group.sort_by(|a, b| a.name().cmp(b.name()));
        out.push_str("### ");
        out.push_str(group);
        out.push_str("\n\n");
        for it in in_group {
            let n = it.name();
            out.push_str("- [`");
            out.push_str(n);
            out.push_str("`](");
            out.push_str(&resolve_link(it));
            out.push_str(")\n");
        }
        out.push('\n');
    }
}

/// Render the table of contents under a `## Contents` H2, delegating to
/// [`render_toc_groups`] for the kind-grouped bullet lists.
///
/// `resolve_link` receives the full `&ItemDoc` and returns the link target
/// string — use a fragment resolver (`|item| format!("#{}", item.name())`)
/// for single-file mode, or a filename resolver for split-mode index pages.
/// No-op when `items` is empty.
fn render_toc(
    out: &mut String,
    items: &[&ItemDoc],
    resolve_link: &dyn Fn(&ItemDoc) -> String,
) {
    if items.is_empty() {
        return;
    }
    out.push_str("## Contents\n\n");
    render_toc_groups(out, items, resolve_link);
}

/// Emit a doc-comment string as one or more Markdown paragraphs.
///
/// Splits the input on blank lines (one or more `\n\n` sequences) and writes
/// each non-empty paragraph followed by a blank line, so the next thing emitted
/// after the call starts on a fresh paragraph.  All-whitespace input leaves the
/// buffer untouched so we don't produce dangling blank lines.
fn emit_paragraphs(out: &mut String, doc: &str) {
    for paragraph in doc.split("\n\n") {
        let p = paragraph.trim();
        if p.is_empty() {
            continue;
        }
        out.push_str(p);
        out.push_str("\n\n");
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


/// Render a single `ItemDoc` to `out`.
///
/// Emits the H2 heading with explicit anchor, optional annotation callouts,
/// the doc paragraphs, the kind-specific body, then optional cross-reference
/// sections derived from `xrefs` (the precomputed [`CrossRefIndex`]).
///
/// `resolve_link` maps a referenced item name to the link target string for
/// cross-reference bullets — use a fragment resolver for single-file mode or
/// a filename resolver for split-mode per-item files.
fn render_item(
    out: &mut String,
    item: &ItemDoc,
    xrefs: Option<&CrossRefIndex<'_>>,
    resolve_link: &dyn Fn(&str) -> String,
) {
    let name = item.name();
    let kw = item.keyword();
    let vis = if item.is_pub() { "pub " } else { "" };

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
    let anns = item.annotations();
    if let Some(dep) = find_annotation(anns, "deprecated") {
        let msg = dep.args.first().map(|s| crate::util::unquote(s)).unwrap_or("");
        out.push_str("> **Deprecated:**");
        if !msg.is_empty() {
            out.push(' ');
            out.push_str(msg);
        }
        out.push_str("\n\n");
    }
    if let Some(opt) = find_annotation(anns, "optimized") {
        let target = opt.args.first().map(|s| crate::util::unquote(s)).unwrap_or("");
        out.push_str("*Optimized: `");
        out.push_str(target);
        out.push_str("`*\n\n");
    }

    if let Some(doc) = item.doc() {
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

    // Cross-reference sections, if any. Looks up `name` in both inverted
    // indices and emits "Conforms to" / "Used by" bullets when populated.
    render_cross_refs(out, name, xrefs, resolve_link);
}

/// Render the `### Conforms to` and `### Used by` sections from `xrefs`,
/// keyed on this item's name.  Each section is emitted only when its bullet
/// list is non-empty.  Bullet entries are sorted, deduplicated links.
///
/// `resolve_link` maps a referenced item name to the link target string
/// (between `](` and `)`) — callers supply a fragment resolver for single-file
/// mode or a filename resolver for split-mode per-item files.
///
/// "Conforms to" reads from `xrefs.conformer_to_traits` — the precomputed
/// inverse of `trait_to_conformers` — so each lookup is O(log N) instead of
/// the O(traits × avg_conformers) scan a naive renderer would perform.
fn render_cross_refs(
    out: &mut String,
    name: &str,
    xrefs: Option<&CrossRefIndex<'_>>,
    resolve_link: &dyn Fn(&str) -> String,
) {
    let Some(xrefs) = xrefs else {
        return;
    };
    // Conforms to: direct lookup in the precomputed inverse index. The list is
    // already sorted + deduplicated when CrossRefIndex was built.
    if let Some(traits) = xrefs.conformer_to_traits.get(name)
        && !traits.is_empty()
    {
        out.push_str("### Conforms to\n\n");
        for t in traits {
            out.push_str("- [`");
            out.push_str(t);
            out.push_str("`](");
            out.push_str(&resolve_link(t));
            out.push_str(")\n");
        }
        out.push('\n');
    }

    // Used by: direct lookup in entity_to_containers.
    if let Some(containers) = xrefs.cross_refs.entity_to_containers.get(name)
        && !containers.is_empty()
    {
        let mut sorted: Vec<&str> = containers.iter().map(|s| s.as_str()).collect();
        sorted.sort();
        sorted.dedup();
        out.push_str("### Used by\n\n");
        for c in sorted {
            out.push_str("- [`");
            out.push_str(c);
            out.push_str("`](");
            out.push_str(&resolve_link(c));
            out.push_str(")\n");
        }
        out.push('\n');
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
///
/// The fence length is the shortest valid one — at least three backticks, and
/// at least one more than the longest run of consecutive backticks inside the
/// signature.  Mirrors the trick `pulldown-cmark` uses so a signature
/// containing a literal triple-backtick (e.g. inside a string-literal default)
/// can't terminate the fence early.
fn render_function_signature(out: &mut String, signature: &str) {
    let fence_len = (max_consecutive_backticks(signature) + 1).max(3);
    let fence: String = "`".repeat(fence_len);
    out.push_str(&fence);
    out.push_str("reify\n");
    out.push_str(signature);
    out.push('\n');
    out.push_str(&fence);
    out.push_str("\n\n");
}

/// Count the longest run of consecutive backtick characters in `s`.  Returns
/// `0` when `s` contains no backticks.
fn max_consecutive_backticks(s: &str) -> usize {
    let mut max = 0usize;
    let mut cur = 0usize;
    for ch in s.chars() {
        if ch == '`' {
            cur += 1;
            if cur > max {
                max = cur;
            }
        } else {
            cur = 0;
        }
    }
    max
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
    out.push_str("**Type:** ");
    out.push_str(&md_inline_code(type_repr));
    out.push_str("\n\n");
    if let Some(d) = default_repr {
        out.push_str("**Default:** ");
        out.push_str(&md_inline_code(d));
        out.push_str("\n\n");
    }
}

/// Render the `**Direction:**` and `**Expression:**` lines for a `Purpose`.
fn render_purpose_body(out: &mut String, direction: &str, expr_repr: &str) {
    out.push_str("**Direction:** ");
    out.push_str(direction);
    out.push_str("\n\n");
    out.push_str("**Expression:** ");
    out.push_str(&md_inline_code(expr_repr));
    out.push_str("\n\n");
}

/// Render the `**Base:**` and `**Scale:**` lines for a `Unit`.
fn render_unit_body(out: &mut String, base_unit: &str, scale: &str) {
    out.push_str("**Base:** ");
    out.push_str(&md_inline_code(base_unit));
    out.push_str("\n\n");
    out.push_str("**Scale:** ");
    out.push_str(&md_inline_code(scale));
    out.push_str("\n\n");
}

/// Render the `= \`{type_repr}\`` line for a `TypeAlias`.
fn render_type_alias_body(out: &mut String, type_repr: &str) {
    out.push_str("= ");
    out.push_str(&md_inline_code(type_repr));
    out.push_str("\n\n");
}

/// Render the `\`{expr_repr}\`` line for a `ConstraintDef`.
fn render_constraint_def_body(out: &mut String, expr_repr: &str) {
    out.push_str(&md_inline_code(expr_repr));
    out.push_str("\n\n");
}

/// Wrap `s` in a Markdown inline-code span, picking a backtick fence longer
/// than the longest backtick run inside `s` and padding with a space when `s`
/// starts or ends with a backtick.  Sibling of [`md_inline_code_cell`] for
/// non-table contexts: skips pipe / newline escaping because those characters
/// have no special meaning outside a GFM table cell.
fn md_inline_code(s: &str) -> String {
    let fence_len = max_consecutive_backticks(s) + 1;
    let fence: String = "`".repeat(fence_len);
    let needs_pad = s.starts_with('`') || s.ends_with('`');
    let space = if needs_pad { " " } else { "" };
    format!("{fence}{space}{s}{space}{fence}")
}

/// Escape a single Markdown table cell value.
///
/// `|` characters are backslash-escaped (otherwise they'd be parsed as a
/// column boundary) and embedded newlines collapse to a single space so the
/// row stays on one line.  This helper is for *plain-text* cells; cells
/// rendered as inline code must go through [`md_inline_code_cell`] so literal
/// backticks in the value don't terminate the surrounding code-span fence.
fn md_cell_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Wrap `s` in a Markdown inline-code span suitable for use as a table cell.
///
/// Picks the shortest backtick fence that doesn't appear in `s` (so a value
/// containing a literal backtick still renders as a single span instead of
/// terminating the fence early) and pads with a space when `s` starts or ends
/// with a backtick — both standard CommonMark inline-code conventions.
/// Pipes / newlines inside `s` are escaped via [`md_cell_escape`] for the same
/// reason as plain-text cells.
fn md_inline_code_cell(s: &str) -> String {
    md_inline_code(&md_cell_escape(s))
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
            Some(d) => md_inline_code_cell(d),
            None => "—".to_string(),
        };
        // Description = doc text + optional `*hint: <solver_hint arg>*` suffix.
        let mut description = p.doc.as_deref().unwrap_or("").trim().to_string();
        if let Some(hint) = find_annotation(&p.annotations, "solver_hint") {
            let hint_arg = hint.args.first().map(|s| crate::util::unquote(s)).unwrap_or("");
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str("*hint: ");
            description.push_str(hint_arg);
            description.push('*');
        }
        out.push_str("| ");
        out.push_str(&md_inline_code_cell(&p.name));
        out.push_str(" | ");
        out.push_str(&md_inline_code_cell(&p.type_repr));
        out.push_str(" | ");
        // Dimension is not exposed on ParamDoc today; emit em-dash placeholder.
        out.push('—');
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
        out.push_str(&md_inline_code(&c.expr_repr));
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
        out.push_str("| ");
        out.push_str(&md_inline_code_cell(&p.name));
        out.push_str(" | ");
        out.push('—');
        out.push_str(" | ");
        out.push_str(&md_cell_escape(&p.direction));
        out.push_str(" | ");
        out.push_str(&md_inline_code_cell(&p.type_name));
        out.push_str(" | ");
        // Description: derived from members list, if present.
        if p.members.is_empty() {
            out.push('—');
        } else {
            let joined = p.members.join(", ");
            out.push_str(&md_cell_escape(&joined));
        }
        out.push_str(" |\n");
    }
    out.push('\n');
}

/// Build the per-item filename for split-mode output: `{kind_slug}-{name}.md`,
/// optionally prefixed by the module path when more than one module is being
/// rendered (so cross-module name clashes resolve to distinct files).
fn item_filename(item: &ItemDoc, module_prefix: Option<&str>) -> String {
    let base = format!("{}-{}.md", item.kind_slug(), item.name());
    match module_prefix {
        Some(p) => format!("{p}/{base}"),
        None => base,
    }
}

/// Build the split-file (filename, body) list.
///
/// Layout:
/// - `index.md` first — module H1 / doc paragraph + the table of contents.
/// - One per-item file per declared item (including `@test`-annotated ones),
///   each containing the module H1, a back-link to `index.md`, and the full
///   item rendering.
///
/// Filenames use the `{kind_slug}-{name}.md` shape; multi-module models are
/// disambiguated by prefixing each item file with the module path so e.g. a
/// `Board` structure in two different modules doesn't collide.  Single-module
/// models stay flat (matches the PRD example).
///
/// Always emits at least the `index.md` placeholder, so callers can rely on
/// its presence.
fn render_split(model: &DocModel, xrefs: Option<&CrossRefIndex<'_>>) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let multi_module = model.modules.len() > 1;

    // Build a name→(kind, module) index once so resolver closures below can
    // look up unique file targets without re-scanning the model each call.
    let name_index = NameIndex::new(model);

    // Build the index body first.
    //
    // Single-module: emit `# {module}` H1 + optional doc + `render_toc` (which
    // wraps with `## Contents`).  This keeps the pre-task shape for the common
    // case.
    //
    // Multi-module: emit `## {module}` H2 per module + optional doc + the
    // kind-grouped bullet lists directly (via `render_toc_groups`, no
    // `## Contents` wrapper).  The per-module resolver uses the current module
    // path to construct module-prefixed links (`{module}/{kind}-{name}.md`),
    // so same-named items in different modules resolve to distinct files.
    let mut index_body = String::new();
    for module in &model.modules {
        let items_for_toc: Vec<&ItemDoc> = module
            .items
            .iter()
            .filter(|i| find_annotation(i.annotations(), "test").is_none())
            .collect();
        if multi_module {
            // H2 per module — reader sees the module path as the section
            // heading and kind H3s nest underneath.
            index_body.push_str("## ");
            index_body.push_str(&module.path);
            index_body.push_str("\n\n");
            if let Some(doc) = module.doc.as_deref() {
                emit_paragraphs(&mut index_body, doc);
            }
            // Per-module index-rooted resolver: each item belongs to this
            // module (we're iterating module.items), so the kind is taken
            // directly from the ItemDoc — no name-index lookup needed.
            let current_module = module.path.as_str();
            render_toc_groups(&mut index_body, &items_for_toc, &|item: &ItemDoc| {
                format!(
                    "{}/{}-{}.md",
                    current_module,
                    item.kind_slug(),
                    item.name()
                )
            });
        } else {
            // Single-module: keep the existing H1 + `## Contents` shape.
            index_body.push_str("# ");
            index_body.push_str(&module.path);
            index_body.push_str("\n\n");
            if let Some(doc) = module.doc.as_deref() {
                emit_paragraphs(&mut index_body, doc);
            }
            // Single-module index-rooted resolver: all items are siblings in
            // the same flat directory.  Kind is taken directly from the
            // ItemDoc — no name-index lookup needed.
            render_toc(&mut index_body, &items_for_toc, &|item: &ItemDoc| {
                format!("{}-{}.md", item.kind_slug(), item.name())
            });
        }
    }
    files.push(("index.md".to_string(), index_body));

    // Per-item files.
    for module in &model.modules {
        let module_prefix = if multi_module {
            Some(module.path.as_str())
        } else {
            None
        };
        for item in &module.items {
            let mut body = String::new();
            // Module H1 + back-link give the page basic navigational context
            // when viewed in isolation (e.g. on GitHub blob view).
            body.push_str("# ");
            body.push_str(&module.path);
            body.push_str("\n\n");
            // Back-link to the TOC index.  Path is relative to the per-item
            // file, so single-module flat layout uses `index.md` directly and
            // multi-module nested layout walks up one directory.
            let back = if multi_module { "../index.md" } else { "index.md" };
            body.push_str("[← Index](");
            body.push_str(back);
            body.push_str(")\n\n");
            // Item-rooted link resolver for cross-reference bullets.
            // The resolver is relative to the *containing file's* location:
            //
            // Single-module: all items live in the same flat directory, so
            // `{kind}-{name}.md` siblings are always correct.
            //
            // Multi-module: the containing file is at `{current_module}/{...}`.
            // - Same-module referenced item → `{kind}-{name}.md` (sibling).
            // - Cross-module referenced item → `../{other_module}/{kind}-{name}.md`
            //   (up one directory, then into the other module's directory).
            // - Ambiguous or missing → `#{name}` fallback (as before).
            if multi_module {
                let current_module = module.path.as_str();
                render_item(&mut body, item, xrefs, &|name: &str| {
                    match name_index.unique_resolve(name) {
                        Some((kind, mod_path)) if mod_path == current_module => {
                            format!("{kind}-{name}.md")
                        }
                        Some((kind, mod_path)) => {
                            format!("../{mod_path}/{kind}-{name}.md")
                        }
                        None => format!("#{name}"),
                    }
                });
            } else {
                render_item(&mut body, item, xrefs, &|name: &str| {
                    match name_index.unique_resolve(name) {
                        Some((kind, _)) => format!("{kind}-{name}.md"),
                        None => format!("#{name}"),
                    }
                });
            }
            files.push((item_filename(item, module_prefix), body));
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

}
