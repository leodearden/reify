//! Concrete-syntax parser: tree-sitter CST → reify_ast::ParsedModule.
//!
//! After PRD task ε (docs/prds/core-ast-ir-layering.md §10 Phase 2), the parsed
//! AST data types live in `reify-ast`; this crate is the behaviour layer that
//! produces them. The `pub use reify_ast::{…}` block below is a TRANSIENT
//! re-export so the 7 reify-syntax dependents (reify-cli, reify-compiler,
//! reify-doc-build, reify-eval, reify-expr, reify-lsp, reify-test-support)
//! keep resolving `reify_syntax::ParsedModule` etc. through PRD task η, which
//! atomically rewrites every `reify_syntax::<AST type>` → `reify_ast::<…>`
//! and then removes this re-export block.

mod ts_parser;

use reify_ast::ParsedModule;
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
