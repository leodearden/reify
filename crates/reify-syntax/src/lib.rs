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

/// Visit the root-level expression of each `Param` default and `Let` value
/// in all `Structure` declarations of a parsed module.
///
/// The name **`visit_structure_member_root_exprs`** is intentional: this function
/// visits only the *root* `Expr` node of each qualifying member — it does **not**
/// recurse into sub-expressions (operands of `BinOp`, arguments of `FunctionCall`,
/// branches of `Conditional`, etc.).  If `EnumAccess` (or any other node of
/// interest) appears inside a nested expression such as `foo(CorrosionClass.C5)`,
/// this function will *not* find it.
///
/// For each [`Declaration::Structure`] in `module.declarations`, this function
/// iterates the structure's `members` and calls `visit` with a reference to:
///
/// - [`MemberDecl::Param`] — the `default` expression, **when it is `Some`**
///   (`param` without a default is skipped entirely).
/// - [`MemberDecl::Let`] — the `value` expression (always present).
///
/// Members of other kinds (Constraint, ConstraintInst, Sub, Minimize, Maximize,
/// GuardedGroup, AssociatedType, Port, Connect, Chain, MetaBlock, ForallConnect,
/// ForallConstraint) are silently skipped.
///
/// # Scope limitations (intentional)
///
/// The following are **not** covered by this function:
///
/// - Other declaration kinds (Occurrence, Trait, Function, ConstraintDef, Purpose,
///   Field, Unit, TypeAlias, Enum, Import) — only `Structure` is visited.
/// - Sub-expression recursion — only the top-level `Expr` of each qualifying member
///   is visited, not the operands of `BinOp`, `FunctionCall`, `Conditional`, etc.
///
/// # Example
///
/// ```ignore
/// let mut enum_accesses: Vec<(String, String)> = Vec::new();
/// visit_structure_member_root_exprs(&parsed, |expr| {
///     if let reify_ast::ExprKind::EnumAccess { type_name, variant } = &expr.kind {
///         enum_accesses.push((type_name.clone(), variant.clone()));
///     }
/// });
/// ```
///
/// # Note
///
/// This function lives in reify-syntax (rather than reify-test-support) to make
/// it accessible from inside reify-syntax's own `#[cfg(test)]` module without
/// triggering the reify-syntax ↔ reify-test-support dev-dep double-instantiation
/// (E0308).  It is a test-support helper and is not part of the parser's
/// production API.
// Test-support helper kept in-crate to avoid the reify-syntax↔reify-test-support
// dev-dep cycle; not intended as a general parser API.
#[doc(hidden)]
pub fn visit_structure_member_root_exprs<F: FnMut(&reify_ast::Expr)>(
    module: &reify_ast::ParsedModule,
    mut visit: F,
) {
    for decl in &module.declarations {
        if let reify_ast::Declaration::Structure(s) = decl {
            for member in &s.members {
                match member {
                    reify_ast::MemberDecl::Param(p) => {
                        if let Some(default) = &p.default {
                            visit(default);
                        }
                    }
                    reify_ast::MemberDecl::Let(l) => {
                        visit(&l.value);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Parse a source string into a `ParsedModule`, pre-seeding the lowering's
/// `known_enums` set with `prelude_enum_names`. See [`ts_parser::parse_with_prelude_enums`].
pub fn parse_with_prelude_enums<'a>(
    source: &'a str,
    module_path: ModulePath,
    prelude_enum_names: &[&'a str],
) -> ParsedModule {
    ts_parser::parse_with_prelude_enums(source, module_path, prelude_enum_names)
}

#[cfg(test)]
mod tests {
    // ── visit_structure_member_root_exprs ─────────────────────────────────

    /// visit_structure_member_root_exprs visits the visitor exactly once for a
    /// structure containing one Param with a default expression.  The visited
    /// Expr's kind must be a NumberLiteral (the default value 1.5).
    #[test]
    fn visit_structure_member_root_exprs_visits_param_default() {
        let source = "structure S { param x: Real = 1.5 }";
        let module = crate::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let mut visited: Vec<reify_ast::Expr> = vec![];
        crate::visit_structure_member_root_exprs(&module, |expr| {
            visited.push(expr.clone());
        });
        assert_eq!(
            visited.len(),
            1,
            "expected exactly one visit for param default, got {:?}",
            visited.len()
        );
        assert!(
            matches!(
                visited[0].kind,
                reify_ast::ExprKind::NumberLiteral { .. }
            ),
            "expected NumberLiteral kind for param default, got {:?}",
            visited[0].kind
        );
    }

    /// visit_structure_member_root_exprs visits the visitor exactly once for a
    /// structure containing one Let binding.  The visited Expr's kind must be a
    /// StringLiteral matching the bound value.
    #[test]
    fn visit_structure_member_root_exprs_visits_let_value() {
        let source = r#"structure S { let x = "hello" }"#;
        let module = crate::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let mut visited: Vec<reify_ast::Expr> = vec![];
        crate::visit_structure_member_root_exprs(&module, |expr| {
            visited.push(expr.clone());
        });
        assert_eq!(
            visited.len(),
            1,
            "expected exactly one visit for let value, got {}",
            visited.len()
        );
        assert!(
            matches!(&visited[0].kind, reify_ast::ExprKind::StringLiteral(s) if s == "hello"),
            "expected StringLiteral(\"hello\") for let value, got {:?}",
            visited[0].kind
        );
    }

    /// visit_structure_member_root_exprs must NOT call the visitor for a Param
    /// that has no default expression (type-annotated-only param, `default == None`).
    #[test]
    fn visit_structure_member_root_exprs_skips_param_without_default() {
        let source = "structure S { param x: Real }";
        let module = crate::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let mut call_count = 0usize;
        crate::visit_structure_member_root_exprs(&module, |_expr| {
            call_count += 1;
        });
        assert_eq!(
            call_count, 0,
            "expected no visits for param without default"
        );
    }

    /// visit_structure_member_root_exprs visits members in declaration order and
    /// covers both Param defaults and Let values in a mixed-member structure.
    /// Asserts count == 3 (two param defaults + one let value) and that the
    /// NumberLiteral values match in source order.
    #[test]
    fn visit_structure_member_root_exprs_visits_each_member_in_declaration_order() {
        let source =
            "structure S {\n    param a: Real = 1.0\n    let b = 2.0\n    param c: Real = 3.0\n}";
        let module = crate::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let mut values: Vec<f64> = vec![];
        crate::visit_structure_member_root_exprs(&module, |expr| {
            if let reify_ast::ExprKind::NumberLiteral { value: v, .. } = &expr.kind {
                values.push(*v);
            }
        });
        assert_eq!(
            values.len(),
            3,
            "expected 3 visits (2 param defaults + 1 let value), got {:?}",
            values
        );
        assert_eq!(
            values[0], 1.0,
            "first visited expr must be param a default (1.0)"
        );
        assert_eq!(
            values[1], 2.0,
            "second visited expr must be let b value (2.0)"
        );
        assert_eq!(
            values[2], 3.0,
            "third visited expr must be param c default (3.0)"
        );
    }

    /// visit_structure_member_root_exprs is a no-op (visitor never called) when
    /// the module contains only non-Structure declarations (here, a top-level enum).
    #[test]
    fn visit_structure_member_root_exprs_no_op_when_module_has_no_structure() {
        let source = "enum Foo { Bar }";
        let module = crate::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let mut call_count = 0usize;
        crate::visit_structure_member_root_exprs(&module, |_expr| {
            call_count += 1;
        });
        assert_eq!(
            call_count, 0,
            "expected no visits for module with no Structure declarations"
        );
    }

    /// visit_structure_member_root_exprs skips Constraint members as documented.
    /// A structure with one param (no default), one bare `constraint`, and one
    /// `let` should produce exactly one visitor call — for the `let` value only.
    /// This pins the documented contract that other member kinds are silently ignored.
    #[test]
    fn visit_structure_member_root_exprs_skips_non_targeted_member_kinds() {
        // param has no default → skipped; constraint → skipped; let → visited.
        let source = "structure S {\n    param x : Real\n    constraint x > 0\n    let y = 2.0\n}";
        let module = crate::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let mut call_count = 0usize;
        crate::visit_structure_member_root_exprs(&module, |_expr| {
            call_count += 1;
        });
        assert_eq!(
            call_count, 1,
            "expected exactly 1 visit (let value only; constraint and no-default param are skipped)"
        );
    }
}
