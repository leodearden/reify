//! Cross-file declaration-name references and rename over the import graph:
//! resolve the cursor to a declaration home, then union the home document's
//! use sites with each importing open document's. Where `local` and `rename`
//! resolve VALUE-member bindings (param/let/auto/sub/port) within one entity
//! body, this layer resolves a DECLARATION name across files.
//!
//! Pure — the open-document set arrives as `workspace_docs` and import
//! resolution as an injected `resolve_import` closure (mirroring goto_def's
//! pattern), so the scope logic is unit-testable with an in-memory workspace
//! and a mock resolver. The server handlers assemble both from the live
//! DocumentStore.

#[cfg(test)]
mod tests;
