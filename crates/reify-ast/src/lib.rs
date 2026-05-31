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
//!
//! The `decl.rs` reference graph was audited during ε (PRD task ε): it references
//! only reify-core primitives and the in-crate `ast.rs` Expr/TypeExpr. No ir-tier
//! type references exist in decl.rs — confirmed by `cargo build -p reify-ast`.

// Mirrors the reify-core lint-attribute prelude for parity across the
// core stack; reify-ast itself has no current trigger.
#![allow(clippy::mutable_key_type)]

pub mod ast;
pub mod decl;

// ── flat root re-exports ─────────────────────────────────────────────────────
// Flat re-export so code using `reify_ast::Expr` (etc.) resolves alongside the
// module-path form `reify_ast::ast::Expr`.
pub use ast::{
    DimOp, Expr, ExprKind, LambdaParam, MatchArm, MatchPattern, QuantifierKind, TypeExpr,
    TypeExprKind, UnitExpr,
};

// ── declaration AST flat re-exports ─────────────────────────────────────────
// Mirrors the flat surface previously in reify-syntax::lib so that code using
// `reify_ast::ParsedModule` (etc.) resolves correctly alongside the module-path
// form `reify_ast::decl::ParsedModule`.
pub use decl::{
    Annotation, AssociatedTypeDecl, ChainDecl, ConnectDecl, ConnectOp, ConstraintDecl,
    ConstraintDef, ConstraintInstDecl, Declaration, EnumDecl, EnumVariantDecl, FieldDef,
    FieldSource, FnBody, FnDef, FnParam, ForallConnectBody, ForallConnectDecl,
    ForallConstraintBody, ForallConstraintDecl, GuardedGroupDecl, ImportDecl, ImportKind,
    LetDecl, MAX_MEMBER_NESTING_DEPTH, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MaximizeDecl,
    MemberDecl, MemberSpanInfo, MetaBlockDecl, MinimizeDecl, ModuleDecl, NumberClass,
    OccurrenceDef, ParamDecl, ParseError, ParsedModule, PortDecl, PortRef, Pragma, PragmaArg,
    PragmaValue, KeyedSubMemberEntry, PurposeDef, PurposeParam, StructureDef, SubDecl,
    TraitBoundRef, TraitDecl, TypeAliasDecl, TypeParamDecl, UnitDecl, VariantPayload, WhereClause,
    classify_number_literal, find_named_member_span, has_test_annotation,
    walk_specialization_scope_members,
};
