//! Whether the import lines [`header_edit`](super::header_edit) wrote
//! re-parse as written: each one a plain import of its module, carrying
//! nothing that was pending above it.

use std::fmt;

use reify_ast::{Declaration, ImportDecl};
use reify_core::{ModulePath, SourceSpan};

use super::header_edit::Insertion;

/// Why an edit adding import lines does not hold them as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementDefect {
    /// The added import of `module` would be gated by the `#cfg` pragmas
    /// pending above it, whose source text `pragmas` holds.
    Gated {
        module: ModulePath,
        pragmas: Vec<String>,
    },
    /// The added import of `module` would take the annotations pending above
    /// it, whose source text `annotations` holds, from the declaration they
    /// precede.
    TakesAnnotations {
        module: ModulePath,
        annotations: Vec<String>,
    },
    /// The line added for `module` would not re-parse as its import where it
    /// is written, for example inside a block comment.
    NotAnImport { module: ModulePath },
    /// The lines added for `modules` would leave the module failing to
    /// parse; `first_error` is the first parse error's message.
    BreaksParse {
        modules: Vec<ModulePath>,
        first_error: String,
    },
}

impl fmt::Display for PlacementDefect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gated { module, pragmas } => write!(
                f,
                "the added `import {}` would be gated by {}",
                module.to_dotted(),
                pragmas.join(" ")
            ),
            Self::TakesAnnotations {
                module,
                annotations,
            } => write!(
                f,
                "the added `import {}` would take {} from the declaration it precedes",
                module.to_dotted(),
                annotations.join(" ")
            ),
            Self::NotAnImport { module } => write!(
                f,
                "the added `import {}` would not parse as an import where it is written",
                module.to_dotted()
            ),
            Self::BreaksParse {
                modules,
                first_error,
            } => {
                let imports: Vec<String> = modules
                    .iter()
                    .map(|module| format!("`import {}`", module.to_dotted()))
                    .collect();
                write!(
                    f,
                    "the added {} would break the parse: {first_error}",
                    imports.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for PlacementDefect {}

/// Re-parses `insertion.text` as the module at `path` and judges the imports
/// that start inside `insertion.lines`, one per added module in line order.
/// A parse error comes first: no import of a broken parse can be trusted, and
/// `insert_imports` edits only a parse-clean module, so the edit caused it.
pub(crate) fn check(insertion: &Insertion<'_>, path: &ModulePath) -> Result<(), PlacementDefect> {
    if insertion.modules.is_empty() {
        return Ok(());
    }
    let reparsed = reify_syntax::parse(&insertion.text, path.clone());
    if let Some(error) = reparsed.errors.first() {
        return Err(PlacementDefect::BreaksParse {
            modules: insertion.modules.iter().copied().cloned().collect(),
            first_error: error.message.clone(),
        });
    }
    let added: Vec<&ImportDecl> = reparsed
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Import(import)
                if insertion.lines.contains(&(import.span.start as usize)) =>
            {
                Some(import)
            }
            _ => None,
        })
        .collect();
    for &module in &insertion.modules {
        let dotted = module.to_dotted();
        let Some(import) = added.iter().find(|import| import.path == dotted) else {
            return Err(PlacementDefect::NotAnImport {
                module: module.clone(),
            });
        };
        check_unattached(import, module, &insertion.text)?;
    }
    Ok(())
}

/// Whether the added `import`, written for `module`, carries no `#cfg`
/// pragma and no annotation; `text` is the source its spans index.
fn check_unattached(
    import: &ImportDecl,
    module: &ModulePath,
    text: &str,
) -> Result<(), PlacementDefect> {
    if !import.cfg_predicates.is_empty() {
        return Err(PlacementDefect::Gated {
            module: module.clone(),
            pragmas: source_texts(text, import.cfg_predicates.iter().map(|pragma| pragma.span)),
        });
    }
    if !import.annotations.is_empty() {
        return Err(PlacementDefect::TakesAnnotations {
            module: module.clone(),
            annotations: source_texts(
                text,
                import.annotations.iter().map(|annotation| annotation.span),
            ),
        });
    }
    Ok(())
}

fn source_texts(text: &str, spans: impl Iterator<Item = SourceSpan>) -> Vec<String> {
    spans
        .map(|span| text[span.start as usize..span.end as usize].to_string())
        .collect()
}
