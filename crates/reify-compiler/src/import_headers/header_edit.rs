//! Adding `import <module>` lines to a module's source text without
//! disturbing anything else in it. Imports `reify_ast`/`reify_core`, the
//! [`CfgSet`] type and the sibling [`coverage`](super::coverage) only.

use std::collections::HashSet;
use std::ops::Range;

use reify_ast::{Declaration, ParsedModule};
use reify_core::ModulePath;

use super::coverage::ImportCoverage;
use crate::cfg::CfgSet;

/// A `#cfg(…)` pragma stays pending across pragmas until the next
/// declaration, which it gates only if that declaration is an import; so a
/// new import written below a leading `#cfg` would be gated by it, and a
/// module's leading pragma header ends at the line holding the first one.
const CFG_PRAGMA: &str = "cfg";

/// An edited source and what the edit added to it.
pub(crate) struct Insertion<'m> {
    pub(crate) text: String,
    /// The byte range of `text` the added text occupies, blank lines
    /// included; empty when nothing was added.
    pub(crate) added_bytes: Range<usize>,
    /// The modules an `import` line was added for, in line order.
    pub(crate) modules: Vec<&'m ModulePath>,
}

/// [`super::insert_imports`], which states the contract; [`header_anchor`]
/// decides where the new lines go.
pub(crate) fn insert_imports<'m>(
    source: &str,
    parsed: &ParsedModule,
    cfg: &CfgSet,
    modules: &'m [ModulePath],
) -> Insertion<'m> {
    let coverage = ImportCoverage::of(parsed, cfg);
    let mut emitted: HashSet<&ModulePath> = HashSet::new();
    let modules: Vec<&ModulePath> = modules
        .iter()
        .filter(|module| !coverage.covers_whole_surface(module))
        .filter(|module| emitted.insert(*module))
        .collect();
    if modules.is_empty() {
        return Insertion {
            text: source.to_string(),
            added_bytes: 0..0,
            modules,
        };
    }
    let new_lines: String = modules
        .iter()
        .map(|module| format!("import {}\n", module.to_dotted()))
        .collect();
    let (offset, text) = match header_anchor(source, parsed) {
        Anchor::AfterImports(span_end) => after_line(source, span_end, new_lines),
        Anchor::AfterHeaderLine(span_end) => after_line(source, span_end, format!("\n{new_lines}")),
        Anchor::BeforeBody(offset) => (offset, format!("{new_lines}\n")),
    };
    let mut edited = String::with_capacity(source.len() + text.len());
    edited.push_str(&source[..offset]);
    edited.push_str(&text);
    edited.push_str(&source[offset..]);
    Insertion {
        text: edited,
        added_bytes: offset..offset + text.len(),
        modules,
    }
}

/// Where new import lines go, in priority order.
enum Anchor {
    /// Directly after the line holding the last `import`.
    AfterImports(usize),
    /// After the `module` declaration or the last pragma of the leading
    /// pragma header, separated from it by a blank line.
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
    let header_end = pragma_header_end(source, parsed);
    let last_header_pragma = parsed
        .pragmas
        .iter()
        .filter(|pragma| pragma.name != CFG_PRAGMA)
        .map(|pragma| pragma.span.end as usize)
        .filter(|&end| end <= header_end)
        .max();
    if let Some(end) = last_header_pragma {
        return Anchor::AfterHeaderLine(end);
    }
    Anchor::BeforeBody(leading_comment_block_end(source))
}

/// The end of the leading pragma header: the start of the line holding the
/// first `#cfg` before the first declaration, else that declaration's start.
fn pragma_header_end(source: &str, parsed: &ParsedModule) -> usize {
    let first_declaration_start = parsed
        .declarations
        .first()
        .map_or(source.len(), declaration_start);
    parsed
        .pragmas
        .iter()
        .filter(|pragma| pragma.name == CFG_PRAGMA)
        .map(|pragma| pragma.span.start as usize)
        .filter(|&start| start < first_declaration_start)
        .min()
        .map_or(first_declaration_start, |start| line_start(source, start))
}

/// The offset just past the last blank line of the run of blank and `//`
/// lines that opens `source`; 0 when that run has no blank line.
fn leading_comment_block_end(source: &str) -> usize {
    let mut offset = 0;
    let mut block_end = 0;
    for line in source.split_inclusive('\n') {
        let content = line.trim();
        if content.is_empty() {
            block_end = offset + line.len();
        } else if !content.starts_with("//") {
            break;
        }
        offset += line.len();
    }
    block_end
}

/// The offset where the line containing `offset` starts.
fn line_start(source: &str, offset: usize) -> usize {
    source[..offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1)
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
