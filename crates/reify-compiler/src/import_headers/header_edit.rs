//! Adding `import <module>` lines to a module's source text without
//! disturbing anything else in it. Imports `reify_ast`/`reify_core` and the
//! sibling [`coverage`](super::coverage) only.

use std::collections::HashSet;

use reify_ast::{Declaration, ParsedModule};
use reify_core::ModulePath;

use super::coverage::ImportCoverage;

/// A `#cfg(…)` pragma gates the declaration that follows it, so it is never
/// part of a module's leading pragma header.
const CFG_PRAGMA: &str = "cfg";

/// `source` with an `import` line for every module of `modules` it does not
/// already import whole, through a namespace import, in the order given; an
/// entity import of the module does not count. See [`header_anchor`] for
/// where they go.
pub(crate) fn insert_imports(
    source: &str,
    parsed: &ParsedModule,
    modules: &[ModulePath],
) -> String {
    let coverage = ImportCoverage::of(parsed);
    let mut emitted: HashSet<&ModulePath> = HashSet::new();
    let new_lines: String = modules
        .iter()
        .filter(|module| !coverage.covers_whole_surface(module))
        .filter(|module| emitted.insert(*module))
        .map(|module| format!("import {}\n", module.to_dotted()))
        .collect();
    if new_lines.is_empty() {
        return source.to_string();
    }
    let (offset, text) = match header_anchor(source, parsed) {
        Anchor::AfterImports(span_end) => after_line(source, span_end, new_lines),
        Anchor::AfterHeaderLine(span_end) => after_line(source, span_end, format!("\n{new_lines}")),
        Anchor::BeforeBody(offset) => (offset, format!("{new_lines}\n")),
    };
    let mut edited = String::with_capacity(source.len() + text.len());
    edited.push_str(&source[..offset]);
    edited.push_str(&text);
    edited.push_str(&source[offset..]);
    edited
}

/// Where new import lines go, in priority order.
enum Anchor {
    /// Directly after the line holding the last `import`.
    AfterImports(usize),
    /// After the `module` declaration or the last leading module pragma,
    /// separated from it by a blank line.
    AfterHeaderLine(usize),
    /// At this offset, followed by a blank line: the end of the leading
    /// comment block's last blank line, so a comment hugging the first
    /// declaration stays attached to it.
    BeforeBody(usize),
}

fn header_anchor(source: &str, parsed: &ParsedModule) -> Anchor {
    let last_import = parsed
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Import(import) => Some(import.span),
            _ => None,
        })
        .max_by_key(|span| span.end);
    if let Some(span) = last_import {
        return Anchor::AfterImports(span.end as usize);
    }
    let module_decl = parsed
        .declarations
        .iter()
        .find_map(|declaration| match declaration {
            Declaration::Module(module) => Some(module.span),
            _ => None,
        });
    if let Some(span) = module_decl {
        return Anchor::AfterHeaderLine(span.end as usize);
    }
    let first_declaration_start = parsed
        .declarations
        .first()
        .map_or(source.len(), declaration_start);
    let last_leading_pragma = parsed
        .pragmas
        .iter()
        .filter(|pragma| pragma.name != CFG_PRAGMA)
        .map(|pragma| pragma.span.end as usize)
        .filter(|&end| end <= first_declaration_start)
        .max();
    if let Some(end) = last_leading_pragma {
        return Anchor::AfterHeaderLine(end);
    }
    Anchor::BeforeBody(leading_comment_block_end(source))
}

/// The offset just past the last blank line of the run of blank and `//`
/// lines that opens `source`; 0 when that run has no blank line.
fn leading_comment_block_end(source: &str) -> usize {
    let mut line_start = 0;
    let mut block_end = 0;
    for line in source.split_inclusive('\n') {
        let content = line.trim();
        if content.is_empty() {
            block_end = line_start + line.len();
        } else if !content.starts_with("//") {
            break;
        }
        line_start += line.len();
    }
    block_end
}

/// The insertion point at the end of the line containing `offset`, with
/// `text` prefixed by a newline when that line is the unterminated last one.
fn after_line(source: &str, offset: usize, text: String) -> (usize, String) {
    match source[offset..].find('\n') {
        Some(newline) => (offset + newline + 1, text),
        None => (source.len(), format!("\n{text}")),
    }
}

fn declaration_start(declaration: &Declaration) -> usize {
    let span = match declaration {
        Declaration::Structure(d) => d.span,
        Declaration::Occurrence(d) => d.span,
        Declaration::Import(d) => d.span,
        Declaration::Enum(d) => d.span,
        Declaration::Function(d) => d.span,
        Declaration::Trait(d) => d.span,
        Declaration::Field(d) => d.span,
        Declaration::Purpose(d) => d.span,
        Declaration::Constraint(d) => d.span,
        Declaration::Unit(d) => d.span,
        Declaration::TypeAlias(d) => d.span,
        Declaration::Default(d) => d.span,
        Declaration::Module(d) => d.span,
        Declaration::Joint(d) => d.span,
    };
    span.start as usize
}
