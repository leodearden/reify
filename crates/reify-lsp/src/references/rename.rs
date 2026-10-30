//! Single-file rename and occurrence highlight over the local reference set:
//! the renameability gate, identifier validation, and the WorkspaceEdit /
//! DocumentHighlight producers.

use std::collections::HashMap;

use reify_ast::ParsedModule;
use reify_core::SourceSpan;
use reify_syntax::{Keyword, lookup_keyword};
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Position, TextEdit, Url, WorkspaceEdit,
};

use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

use super::local::{RefSymbolKind, RenameTarget, collect_references, collect_references_at};

/// Compute the rename target for the symbol under the cursor.
///
/// Conservative: returns `Some` only when the cursor resolves to a renameable
/// local value-member binding (`Param`/`Let`/`Auto`/`Sub`/`Port`); `None`
/// otherwise (keywords, literals, builtins, type names, structure names, and
/// imported/cross-module symbols).
pub fn prepare_rename(source: &str, parsed: &ParsedModule, pos: Position) -> Option<RenameTarget> {
    // Resolve the cursor word ONCE and reuse it for both the renameability gate
    // and the returned range. Going through `collect_references_at` (rather than
    // the public `collect_references`) avoids mapping the position to an
    // offset/word a second time per request.
    let offset = position_to_offset(source, pos);
    let (word_start, word) = find_word_at_offset(source, offset)?;

    // collect_references is the renameability gate. It returns None for anything
    // that does not resolve to a local value-member binding — keywords, numeric/
    // quantity literals, builtins/function names, type names, structure/
    // declaration names, and imported/cross-module symbols all fail to resolve —
    // which yields every Invariant-4 refusal for free.
    let refset = collect_references_at(source, parsed, offset, word, true)?;
    if !is_renameable(refset.kind) {
        return None;
    }

    // The rename target is the identifier token UNDER THE CURSOR (a use token, or
    // the declaration name-token when the cursor is on the declaration). Both are
    // name-token spans, so span_to_range over the cursor word gives the range the
    // editor should highlight and pre-fill.
    let range = span_to_range(
        source,
        SourceSpan::new(word_start as u32, (word_start + word.len()) as u32),
    );
    Some(RenameTarget {
        range,
        placeholder: word.to_string(),
    })
}

/// Whether a symbol kind is renameable in this single-file foundation phase: the
/// value-member kinds (`Param`/`Let`/`Auto`/`Sub`/`Port`). The declaration-name
/// kinds (`Structure`/`Occurrence`/`Trait`/`Enum`/`Variant`/`Fn`) lack precise
/// reference spans at sub-/type-reference sites in the current AST, so robust
/// cross-declaration + cross-file rename for them is deferred to phase κ.
///
/// Shared by [`prepare_rename`] and [`compute_rename`] so the two agree exactly
/// on what is renameable.
pub(super) fn is_renameable(kind: RefSymbolKind) -> bool {
    matches!(
        kind,
        RefSymbolKind::Param
            | RefSymbolKind::Let
            | RefSymbolKind::Auto
            | RefSymbolKind::Sub
            | RefSymbolKind::Port
    )
}

/// Whether `new_name` is a legal Reify identifier suitable as a rename target.
///
/// `compute_rename` writes `new_name` verbatim into every declaration ∪ reference
/// name-token span, so a `new_name` that is not a legal identifier — empty,
/// containing whitespace or punctuation (`foo bar`, `foo-bar`), starting with a
/// digit (`2x`), or a rename-refused keyword — would produce source that no
/// longer re-parses cleanly, violating the re-parse-clean half of
/// Invariant 5. LSP clients do not generally validate the proposed name against a
/// language's identifier grammar, so `compute_rename` (the producer the later
/// server wiring consumes) is the contract surface that must reject these before
/// emitting any edit.
///
/// Grammar: a non-empty token whose first byte is ASCII alphabetic or `_` and
/// whose remaining bytes are ASCII alphanumeric or `_` (mirroring
/// `convert::is_ident_byte`, which defines what `find_word_at_offset` treats as an
/// identifier), and which is not a rename-refused row of [`reify_syntax::KEYWORDS`]
/// (PRD D15): every `Reserved` word plus the `Contextual` rows marked
/// `rename: Refused`.
pub(super) fn is_valid_rename_identifier(new_name: &str) -> bool {
    let mut bytes = new_name.bytes();
    match bytes.next() {
        // First byte must be a letter or underscore (rejects empty and `2x`).
        Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
        _ => return false,
    }
    // Remaining bytes must all be identifier bytes (rejects whitespace and
    // punctuation: `foo bar`, `foo-bar`).
    if !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return false;
    }
    !lookup_keyword(new_name).is_some_and(Keyword::rename_refused)
}

/// Compute a [`WorkspaceEdit`] that renames the binding under the cursor.
///
/// The edit covers declaration ∪ references with one `TextEdit` per name-token
/// span, all keyed by `uri`. Returns `None` when the cursor does not resolve to a
/// renameable local value-member binding.
pub fn compute_rename(
    source: &str,
    parsed: &ParsedModule,
    uri: &Url,
    pos: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    // A `new_name` that is not a legal Reify identifier — empty, whitespace,
    // punctuation, digit-leading, or a rename-refused keyword — would corrupt the
    // identifier tokens so the buffer no longer re-parses cleanly (violating the
    // re-parse-clean half of Invariant 5), so refuse it outright before producing
    // any edits.
    if !is_valid_rename_identifier(new_name) {
        return None;
    }

    // Guard with the same renameability check as prepare_rename so the two never
    // disagree on what is renameable.
    let refset = collect_references(source, parsed, pos, true)?;
    if !is_renameable(refset.kind) {
        return None;
    }

    // One TextEdit per declaration∪reference name-token span. `references`
    // already contains the declaration token (include_declaration=true) and is
    // sorted ascending by span.start, so the edits are ascending and
    // non-overlapping.
    let edits: Vec<TextEdit> = refset
        .references
        .iter()
        .map(|&span| TextEdit {
            range: span_to_range(source, span),
            new_text: new_name.to_string(),
        })
        .collect();

    let changes = HashMap::from([(uri.clone(), edits)]);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

/// Compute the occurrence-highlight set for the symbol under the cursor.
///
/// Delegates to [`collect_references`] with `include_declaration = true` and maps
/// each name-token [`SourceSpan`] through [`span_to_range`] into a
/// [`DocumentHighlight`] tagged [`DocumentHighlightKind::TEXT`] (read/write-agnostic
/// — this foundation phase does not distinguish read from write occurrences).
/// Returns `None` when the cursor does not resolve to a local value-member binding
/// (keywords, literals, builtins, type/structure names, imported symbols),
/// mirroring the [`compute_rename`] producer split so the boundary invariant has a
/// pure unit-test home.
///
/// The PRD's "highlight set == references set restricted to the active document"
/// (boundary row 7) holds for free with no extra filtering: [`collect_references`]
/// walks a single [`ParsedModule`] scoped to one entity body, so every span it
/// returns is inherently in-document.
pub fn compute_document_highlights(
    source: &str,
    parsed: &ParsedModule,
    pos: Position,
) -> Option<Vec<DocumentHighlight>> {
    let refset = collect_references(source, parsed, pos, /* include_declaration = */ true)?;
    Some(
        refset
            .references
            .iter()
            .map(|&span| DocumentHighlight {
                range: span_to_range(source, span),
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;
