//! Parsed expression/type AST for Reify.
//!
//! This is the Phase 2 crate introduced in
//! `docs/prds/core-ast-ir-layering.md` (task δ). It contains the parsed
//! AST types that model *source as written* — identifiers unresolved,
//! operators as strings, no types attached.
//!
//! # B2 invariant
//!
//! This crate MUST have exactly one `reify-*` dependency: `reify-core`.
//! No tree-sitter dependency is permitted. The structural invariant is
//! locked in by `crates/reify-ast/tests/dag_invariant.rs`, which reads
//! `Cargo.toml` directly and asserts both conditions. The workspace-wide
//! assertion (`scripts/assert-crate-dag.sh`) arrives under task η per PRD §10.

// Mirrors reify-core/reify-types preludes for lint-attribute parity across the
// core stack; reify-ast itself has no current trigger.
#![allow(clippy::mutable_key_type)]

pub mod ast;

// ── flat root re-exports ─────────────────────────────────────────────────────
// Mirrors the flat surface previously in reify-types::ast so that code using
// `reify_ast::Expr` (etc.) resolves correctly alongside the module-path form
// `reify_ast::ast::Expr`.
pub use ast::{DimOp, Expr, ExprKind, LambdaParam, MatchArm, QuantifierKind, TypeExpr, TypeExprKind};
