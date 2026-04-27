//! Deep dot-chain lint (spec §5.7).
//!
//! Walks left-to-right `MemberAccess` chains in the parsed AST and emits a
//! Warning diagnostic ([`DiagnosticCode::DeepDotChain`]) when the chain length
//! exceeds [`DEEP_DOT_CHAIN_THRESHOLD`].
//!
//! # Counting model
//!
//! Chain length is `1 (root) + N (number of MemberAccess wraps)`. Equivalently,
//! count `.field` hops as `N` and warn when `1 + N > THRESHOLD`. The task's own
//! examples pin this:
//!
//! - `a.b.c.d` → length 4 ⇒ no warn (at threshold).
//! - `a.b.c.d.e` → length 5 ⇒ warn.
//!
//! # Chain roots
//!
//! Any non-`MemberAccess` expression — `Ident`, `IndexAccess`, `FunctionCall`,
//! `EnumAccess`, `BinOp`, `Lambda`, etc. — acts as a chain root. The chain
//! continues only while the AST node is `ExprKind::MemberAccess`. This honours
//! the spec's "treat indexed expressions as fresh chain roots" rule and falls
//! out naturally from the AST shape (no special-casing per root variant).
//!
//! After detecting a maximal chain at its OUTERMOST `MemberAccess`, the walker
//! recurses into the chain's leaf root so that nested chains (e.g. a deep
//! chain inside `IndexAccess.index` or a `FunctionCall.args` element) are
//! detected too.
//!
//! This module is currently a syntactic gate only — emission of warnings is
//! added in step 6.

use reify_syntax::{Expr, ExprKind, ParsedModule};
use reify_types::{Diagnostic, DiagnosticCode, DiagnosticLabel};

/// Maximum allowed chain length before the lint fires.
///
/// A chain of length `N` is a root expression followed by `N - 1` `.field` hops
/// (i.e., `N - 1` levels of `MemberAccess` wrapping). At threshold = 4, the
/// chain `a.b.c.d` is OK; `a.b.c.d.e` (length 5) warns.
pub(crate) const DEEP_DOT_CHAIN_THRESHOLD: usize = 4;

/// Visit every expression-bearing position in `parsed` and emit a Warning for
/// each maximal `MemberAccess` chain whose length exceeds
/// [`DEEP_DOT_CHAIN_THRESHOLD`].
///
/// Pushed diagnostics use [`reify_types::DiagnosticCode::DeepDotChain`]. The
/// warning message and label are added in subsequent TDD steps; this skeleton
/// detects the chain but emits nothing.
pub(crate) fn lint_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for decl in &parsed.declarations {
        walk_declaration(decl, diagnostics);
    }
}

/// Recurse through every expression-bearing position of a top-level declaration.
///
/// Step 4 covers the `Structure`/`Occurrence`/`Trait`/`Purpose` member-list
/// positions for `MemberDecl::Let.value`. Step 16 expands this to every
/// `MemberDecl` variant and every other `Declaration` variant carrying an
/// expression.
fn walk_declaration(decl: &reify_syntax::Declaration, diagnostics: &mut Vec<Diagnostic>) {
    use reify_syntax::Declaration;
    match decl {
        Declaration::Structure(s) => walk_members(&s.members, diagnostics),
        Declaration::Occurrence(o) => walk_members(&o.members, diagnostics),
        Declaration::Trait(t) => walk_members(&t.members, diagnostics),
        Declaration::Purpose(p) => walk_members(&p.members, diagnostics),
        // Function/Field/Constraint/Unit/Enum/Import/TypeAlias visited in step 16.
        _ => {}
    }
}

/// Recurse through a member list. Step 4 handles `Let.value`; step 16 expands
/// to every `MemberDecl` variant and recurses into nested `GuardedGroup`/`Port`
/// member lists.
fn walk_members(members: &[reify_syntax::MemberDecl], diagnostics: &mut Vec<Diagnostic>) {
    use reify_syntax::MemberDecl;
    for member in members {
        match member {
            MemberDecl::Let(l) => walk_expr(&l.value, diagnostics),
            // Other variants visited in step 16.
            _ => {}
        }
    }
}

/// Recurse through an expression, looking for maximal `MemberAccess` chains.
///
/// On hitting an `ExprKind::MemberAccess` node we treat it as the OUTERMOST
/// `MemberAccess` of a chain (callers never invoke `walk_expr` on a chain's
/// inner `MemberAccess` directly). We descend through `object` while it
/// remains `MemberAccess` to count the chain length, then recurse into the
/// chain's leaf root for any nested chains.
///
/// For non-`MemberAccess` expressions, recurse into every `Box<Expr>` /
/// `Vec<Expr>` child.
fn walk_expr(expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        ExprKind::MemberAccess { object, .. } => {
            // Count chain length: 1 (root) + N (member-hops walked while
            // `object` remains `MemberAccess`).
            let mut hops: usize = 1; // for the outermost `.member`
            let mut cursor: &Expr = object;
            loop {
                match &cursor.kind {
                    ExprKind::MemberAccess { object: inner, .. } => {
                        hops += 1;
                        cursor = inner;
                    }
                    _ => break,
                }
            }
            // `cursor` now points at the chain's leaf root (a non-MemberAccess).
            let chain_len = 1 + hops; // 1 for root + N member-hops
            if chain_len > DEEP_DOT_CHAIN_THRESHOLD {
                let chain_text = render_chain_text(expr);
                diagnostics.push(
                    Diagnostic::warning(format!(
                        "deep dot-chain (depth {chain_len}): {chain_text} \
                         — consider intermediate let-bindings"
                    ))
                    .with_code(DiagnosticCode::DeepDotChain)
                    .with_label(DiagnosticLabel::new(expr.span, "deep dot-chain")),
                );
            }
            // Recurse into the chain's leaf root for nested chains.
            walk_expr(cursor, diagnostics);
        }
        ExprKind::BinOp { left, right, .. } => {
            walk_expr(left, diagnostics);
            walk_expr(right, diagnostics);
        }
        ExprKind::UnOp { operand, .. } => walk_expr(operand, diagnostics),
        ExprKind::FunctionCall { args, .. } => {
            for a in args {
                walk_expr(a, diagnostics);
            }
        }
        ExprKind::Conditional { condition, then_branch, else_branch } => {
            walk_expr(condition, diagnostics);
            walk_expr(then_branch, diagnostics);
            walk_expr(else_branch, diagnostics);
        }
        ExprKind::ListLiteral(elems) | ExprKind::SetLiteral(elems) => {
            for e in elems {
                walk_expr(e, diagnostics);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                walk_expr(k, diagnostics);
                walk_expr(v, diagnostics);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            walk_expr(object, diagnostics);
            walk_expr(index, diagnostics);
        }
        ExprKind::Match { discriminant, arms } => {
            walk_expr(discriminant, diagnostics);
            for arm in arms {
                walk_expr(&arm.body, diagnostics);
            }
        }
        ExprKind::Lambda { body, .. } => walk_expr(body, diagnostics),
        ExprKind::Quantifier { collection, predicate, .. } => {
            walk_expr(collection, diagnostics);
            walk_expr(predicate, diagnostics);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            walk_expr(base, diagnostics);
            for a in args {
                walk_expr(a, diagnostics);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => walk_expr(qualifier, diagnostics),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            walk_expr(object, diagnostics);
            walk_expr(qualified, diagnostics);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(l) = lower {
                walk_expr(l, diagnostics);
            }
            if let Some(u) = upper {
                walk_expr(u, diagnostics);
            }
        }
        // Leaf expressions — no children.
        ExprKind::NumberLiteral(_)
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Auto { .. } => {}
    }
}

/// Render a chain text (e.g. `"a.b.c.d.e"`) from an outermost `MemberAccess`.
///
/// Walks `outermost` from the outermost `MemberAccess` down to its leaf root,
/// collecting member names in reverse order (outermost-first), then formats
/// `"<root_repr>.<m_innermost>.....<m_outermost>"`.
///
/// Root rendering:
///   * `Ident(name)` → `name`
///   * `EnumAccess { type_name, variant }` → `"{type_name}.{variant}"`
///   * Anything else (IndexAccess, FunctionCall, BinOp, …) → `"<expr>"`
///
/// The `<expr>` placeholder is a deliberate v0.1 simplification — the
/// diagnostic span ALREADY covers the entire chain in source so editor
/// renderings (LSP/MCP) display the user's literal text via the squiggle.
///
/// # Panics
///
/// Panics in debug builds if `outermost.kind` is not `ExprKind::MemberAccess`,
/// since the caller (`walk_expr`) only invokes this from the MemberAccess arm.
fn render_chain_text(outermost: &Expr) -> String {
    let mut members_outer_to_inner: Vec<&str> = Vec::new();
    let mut cursor: &Expr = outermost;
    loop {
        match &cursor.kind {
            ExprKind::MemberAccess { object, member } => {
                members_outer_to_inner.push(member.as_str());
                cursor = object;
            }
            _ => break,
        }
    }
    debug_assert!(
        !members_outer_to_inner.is_empty(),
        "render_chain_text: outermost expression must be MemberAccess"
    );

    let root_repr: String = match &cursor.kind {
        ExprKind::Ident(name) => name.clone(),
        ExprKind::EnumAccess { type_name, variant } => format!("{type_name}.{variant}"),
        _ => "<expr>".to_string(),
    };

    let mut out = root_repr;
    for member in members_outer_to_inner.iter().rev() {
        out.push('.');
        out.push_str(member);
    }
    out
}
