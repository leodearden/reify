//! Scope-aware reference collection over the parsed AST (task α, 4201).
//!
//! This module is the foundation producer for find-references (β), rename (γ),
//! and occurrence-highlight (δ). It walks the **parsed** AST
//! ([`reify_ast::ParsedModule`]) rather than the compiled IR: the compiled IR is
//! value-only (it discards both source spans and identifier names), whereas every
//! parsed [`reify_ast::Expr`] carries a [`SourceSpan`](reify_core::SourceSpan) and
//! [`reify_ast::ExprKind::Ident`] gives the exact identifier-token span. Reference
//! collection is therefore a scope-aware walk over the parsed AST.
//!
//! Three public entry points:
//! - [`collect_references`] — every span (declaration ∪ uses) of the binding under
//!   the cursor, scoped to a single entity body.
//! - [`prepare_rename`] — the rename target token + placeholder, or `None` when the
//!   cursor is not on a renameable local value-member binding.
//! - [`compute_rename`] — a [`WorkspaceEdit`](tower_lsp::lsp_types::WorkspaceEdit)
//!   covering declaration ∪ references.
//!
//! Declaration and reference spans are **name-token spans** (just the identifier),
//! not full member-statement spans, so rename edits stay minimal and uniform. This
//! deliberately differs from `goto_def`, which returns full-member spans for
//! navigation.
//!
//! Layers — every import points downward:
//! - `walk` — the AST traversal skeleton every reference walker shares (leaf).
//! - `local` — value-member reference resolution within one entity body; on `walk`.
//! - `rename` — single-file rename and occurrence highlight; on `local`.
//! - `decl_uses` — every use site of a declaration name in one document; on `walk`.
//! - `cross_file` — declaration-name references and rename across the import
//!   graph; on `local`, `rename` and `decl_uses`.

mod cross_file;
mod decl_uses;
mod local;
mod rename;
mod walk;

pub use cross_file::{
    compute_references_cross_file, compute_rename_cross_file, prepare_rename_cross_file,
};
pub use local::{
    RefSymbolKind, ReferenceSet, RenameTarget, collect_references, compute_references,
};
pub use rename::{compute_document_highlights, compute_rename, prepare_rename};
