//! Statement-form `forall` per-element elaboration tests (task 2364, spec §5.4).
//!
//! Task 2363 introduced `MemberDecl::ForallConnect` /
//! `MemberDecl::ForallConstraint` AST nodes with stub error diagnostics in the
//! compiler. Task 2364 lifts those stubs and emits one `CompiledConnection` /
//! `CompiledConstraint` per collection element, with each generated decl
//! carrying a span back to the source `forall` and a label encoding the
//! bound-variable name and element index.
//!
//! These tests pin:
//!   * Per-element emission for `ListLiteral` and `Ident`-resolved-to-collection-sub
//!     collections (PRD criteria 5, 8).
//!   * Empty-collection: zero decls, no error (PRD criterion 6).
//!   * Undef-count collection: zero decls, no error (PRD criterion 7,
//!     first half — re-elaboration on count change is out of scope).
//!   * Label format `forall@<var>[<idx>]` (PRD criterion 10).
//!   * Span anchored at the source forall declaration.
//!   * Body-where-clause routing through guarded groups (PRD criterion 9).
//!   * Constraint-instantiation body shape.
//!   * Chain body shape (pairwise per element).
//!   * Non-iterable collection diagnostic.
