//! Concrete-syntax parser: tree-sitter CST → reify_ast::ParsedModule.
//!
//! After PRD task ε (docs/prds/core-ast-ir-layering.md §10 Phase 2), the parsed
//! AST data types live in `reify-ast`; this crate is the behaviour layer that
//! produces them. The `pub use reify_ast::*` block below is a TRANSIENT
//! re-export so the integration tests in `tests/` (which use `use reify_syntax::*`)
//! keep resolving `Declaration`, `ExprKind`, etc. until PRD task η's follow-up
//! sweeps the reify-syntax test suite to import from `reify_ast` directly.

mod ts_parser;

// TRANSIENT: re-export all AST types so `use reify_syntax::*` in integration tests
// continues to resolve `Declaration`, `ExprKind`, `TypeExprKind`, etc.
// Remove once task η follow-up updates reify-syntax/tests/*.rs to `use reify_ast::*`.
pub use reify_ast::*;

use reify_core::ModulePath;

/// Parse a source string into a `ParsedModule` (re-exported from reify-ast).
///
/// Backed by a Tree-sitter grammar parser with CST→AST lowering.
pub fn parse(source: &str, module_path: ModulePath) -> ParsedModule {
    ts_parser::parse(source, module_path)
}

/// Parse a source string into a `ParsedModule`, pre-seeding the lowering's
/// `known_enums` set with `prelude_enum_names`. See [`ts_parser::parse_with_prelude_enums`].
pub fn parse_with_prelude_enums(
    source: &str,
    module_path: ModulePath,
    prelude_enum_names: &[&'static str],
) -> ParsedModule {
    ts_parser::parse_with_prelude_enums(source, module_path, prelude_enum_names)
}
