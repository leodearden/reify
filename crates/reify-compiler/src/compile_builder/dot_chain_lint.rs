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

use reify_syntax::{Expr, ExprKind, ParsedModule};
use reify_types::{Diagnostic, DiagnosticCode, DiagnosticLabel};

/// Maximum allowed chain length before the lint fires.
///
/// A chain of length `N` is a root expression followed by `N - 1` `.field` hops
/// (i.e., `N - 1` levels of `MemberAccess` wrapping). At threshold = 4, the
/// chain `a.b.c.d` is OK; `a.b.c.d.e` (length 5) warns.
pub(crate) const DEEP_DOT_CHAIN_THRESHOLD: usize = 4;

/// Stack-safety bound on the structural recursion in [`walk_expr`].
///
/// The chain-counting `while let` loop in the `MemberAccess` arm is iterative
/// and unbounded (a single chain of N segments is one frame). The exposure is
/// the structural recursion through children of every other `ExprKind` variant
/// — a deeply-nested `Conditional`/`BinOp`/`Lambda` tree, possibly synthesised
/// by a fuzzer or a future parser bug, would burn one Rust frame per level.
/// 256 is generous (typical hand-written code never exceeds ~20) and stops well
/// short of overflowing default Rust stacks.
///
/// Exceeding this bound is treated as a "should never happen" invariant
/// violation: in debug/test builds (i.e. when `debug_assertions` are enabled)
/// `walk_expr_depth` fires a `debug_assert!(false, …)` so that fuzzers and the
/// test suite catch a regression immediately via a panic. Release builds keep
/// the existing silent-`return` fast-path — the `debug_assert!` compiles out —
/// leaving production behaviour unchanged.
const MAX_EXPR_DEPTH: usize = 256;

/// Visit every expression-bearing position in `parsed` and emit a Warning for
/// each maximal `MemberAccess` chain whose length exceeds
/// [`DEEP_DOT_CHAIN_THRESHOLD`].
///
/// Pushed diagnostics use [`reify_types::DiagnosticCode::DeepDotChain`], with a
/// human-readable message of the form
/// `"deep dot-chain (depth N): <chain text> — consider intermediate let-bindings"`
/// and a [`DiagnosticLabel`] anchored to the chain's full source span.
pub(crate) fn lint_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for decl in &parsed.declarations {
        walk_declaration(decl, diagnostics);
    }
}

/// Recurse through every expression-bearing position of a top-level declaration.
///
/// Visits:
///   * `Structure`/`Occurrence`/`Trait`/`Purpose`: `members` (delegates to
///     [`walk_members`]).
///   * `Function`: `body.let_bindings[*].value` + `body.result_expr`.
///   * `Field`: `source` (Analytical/Composed `expr`, Sampled config values).
///   * `Constraint` (named def): `params[*].default` + `predicates[*]`.
///   * `Unit`: `conversion` + `offset`.
///   * `Enum`/`Import`/`TypeAlias`: no expressions, no-op.
fn walk_declaration(decl: &reify_syntax::Declaration, diagnostics: &mut Vec<Diagnostic>) {
    use reify_syntax::{Declaration, FieldSource};
    match decl {
        Declaration::Structure(s) => walk_members(&s.members, diagnostics, 0),
        Declaration::Occurrence(o) => walk_members(&o.members, diagnostics, 0),
        Declaration::Trait(t) => walk_members(&t.members, diagnostics, 0),
        Declaration::Purpose(p) => walk_members(&p.members, diagnostics, 0),
        Declaration::Function(f) => {
            for binding in &f.body.let_bindings {
                walk_expr(&binding.value, diagnostics);
                if let Some(wc) = &binding.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            walk_expr(&f.body.result_expr, diagnostics);
        }
        Declaration::Field(f) => match &f.source {
            FieldSource::Analytical { expr } | FieldSource::Composed { expr } => {
                walk_expr(expr, diagnostics);
            }
            FieldSource::Sampled { config } => {
                for (_, expr) in config {
                    walk_expr(expr, diagnostics);
                }
            }
            FieldSource::Imported { .. } => {}
        },
        Declaration::Constraint(c) => {
            for p in &c.params {
                if let Some(default) = &p.default {
                    walk_expr(default, diagnostics);
                }
                if let Some(wc) = &p.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            for predicate in &c.predicates {
                walk_expr(predicate, diagnostics);
            }
        }
        Declaration::Unit(u) => {
            if let Some(conv) = &u.conversion {
                walk_expr(conv, diagnostics);
            }
            if let Some(off) = &u.offset {
                walk_expr(off, diagnostics);
            }
        }
        // Declarations with no embedded expressions.
        Declaration::Enum(_) | Declaration::Import(_) | Declaration::TypeAlias(_) => {}
    }
}

/// Recurse through a member list, walking every expression-bearing position
/// of every `MemberDecl` variant.
///
/// Visits:
///   * `Param`: `default` + `where_clause.condition`.
///   * `Let`: `value` + `where_clause.condition`.
///   * `Constraint` (bare-expression form): `expr` + `where_clause.condition`.
///   * `ConstraintInst`: `args[*].1` + `where_clause.condition`.
///   * `Sub`: `args[*].1` + `where_clause.condition`.
///   * `Minimize`/`Maximize`: `expr` + `where_clause.condition`.
///   * `GuardedGroup`: `condition` + nested `members`/`else_members` (recursive).
///   * `Port`: `frame_expr` + nested `members` (recursive).
///   * `Connect`: `left.expr`, `right.expr`, `params[*].1`.
///   * `Chain`: each `elements[*]`.
///   * `AssociatedType`/`MetaBlock`: no expressions, no-op.
///
/// Recursion into nested `GuardedGroup`/`Port` member lists is bounded by
/// [`reify_syntax::MAX_MEMBER_NESTING_DEPTH`] to prevent stack overflow on
/// pathological input — mirrors `find_named_member_span_depth` in
/// `reify-syntax`.
fn walk_members(
    members: &[reify_syntax::MemberDecl],
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    use reify_syntax::MemberDecl;
    if depth > reify_syntax::MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Param(p) => {
                if let Some(default) = &p.default {
                    walk_expr(default, diagnostics);
                }
                if let Some(wc) = &p.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::Let(l) => {
                walk_expr(&l.value, diagnostics);
                if let Some(wc) = &l.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::Constraint(c) => {
                walk_expr(&c.expr, diagnostics);
                if let Some(wc) = &c.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::ConstraintInst(c) => {
                for (_, expr) in &c.args {
                    walk_expr(expr, diagnostics);
                }
                if let Some(wc) = &c.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::Sub(s) => {
                for (_, expr) in &s.args {
                    walk_expr(expr, diagnostics);
                }
                if let Some(wc) = &s.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::Minimize(m) => {
                walk_expr(&m.expr, diagnostics);
                if let Some(wc) = &m.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::Maximize(m) => {
                walk_expr(&m.expr, diagnostics);
                if let Some(wc) = &m.where_clause {
                    walk_expr(&wc.condition, diagnostics);
                }
            }
            MemberDecl::GuardedGroup(g) => {
                walk_expr(&g.condition, diagnostics);
                walk_members(&g.members, diagnostics, depth + 1);
                walk_members(&g.else_members, diagnostics, depth + 1);
            }
            MemberDecl::Port(p) => {
                if let Some(frame) = &p.frame_expr {
                    walk_expr(frame, diagnostics);
                }
                walk_members(&p.members, diagnostics, depth + 1);
            }
            MemberDecl::Connect(c) => {
                walk_expr(&c.left.expr, diagnostics);
                walk_expr(&c.right.expr, diagnostics);
                for (_, expr) in &c.params {
                    walk_expr(expr, diagnostics);
                }
            }
            MemberDecl::Chain(c) => {
                for elem in &c.elements {
                    walk_expr(elem, diagnostics);
                }
            }
            // Members with no embedded expressions.
            MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => {}
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
///
/// Recursion into expression children is bounded by [`MAX_EXPR_DEPTH`] for
/// stack safety on pathological input (see the constant doc).
fn walk_expr(expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
    walk_expr_depth(expr, diagnostics, 0);
}

fn walk_expr_depth(expr: &Expr, diagnostics: &mut Vec<Diagnostic>, depth: usize) {
    if depth > MAX_EXPR_DEPTH {
        // Should never happen on real source: 256 is generous (typical hand-
        // written code never exceeds ~20). Hitting this guard means a fuzzer
        // input or upstream parser bug produced a pathologically deep AST and
        // the dot-chain lint silently lost coverage on that subtree. Panic in
        // debug/test builds so a regression bisects directly to this site;
        // keep the silent `return` in release so end-users never see the
        // unactionable diagnostic.
        debug_assert!(
            false,
            "dot_chain_lint walk_expr_depth exceeded MAX_EXPR_DEPTH = {}; \
             dot-chain lint coverage truncated at this subtree — likely \
             upstream parser bug or fuzzer input producing pathologically \
             deep AST",
            MAX_EXPR_DEPTH
        );
        return;
    }
    let next = depth + 1;
    match &expr.kind {
        ExprKind::MemberAccess { object, member } => {
            // Count chain length AND collect member names in a single walk so
            // `render_chain_text` doesn't have to re-traverse the chain.
            // Members are pushed outermost-first: index 0 is the outermost
            // `.member`, the last entry is the innermost member-hop directly
            // above the root.
            let mut members_outer_to_inner: Vec<&str> = vec![member.as_str()];
            let mut cursor: &Expr = object;
            while let ExprKind::MemberAccess {
                object: inner,
                member: m,
            } = &cursor.kind
            {
                members_outer_to_inner.push(m.as_str());
                cursor = inner;
            }
            // `cursor` now points at the chain's leaf root (a non-MemberAccess).
            // chain_len = 1 (root) + N (member-hops)
            let chain_len = 1 + members_outer_to_inner.len();
            if chain_len > DEEP_DOT_CHAIN_THRESHOLD {
                let chain_text = render_chain_text(cursor, &members_outer_to_inner);
                diagnostics.push(
                    Diagnostic::warning(format!(
                        "deep dot-chain (depth {chain_len}): {chain_text} \
                         — consider intermediate let-bindings"
                    ))
                    .with_code(DiagnosticCode::DeepDotChain)
                    .with_label(DiagnosticLabel::new(expr.span, "deep dot-chain")),
                );
            }
            // Recurse into the chain's leaf root for nested chains. The chain
            // walk above is iterative (one frame regardless of N), so the
            // depth bound here only applies to the structural descent below.
            walk_expr_depth(cursor, diagnostics, next);
        }
        ExprKind::BinOp { left, right, .. } => {
            walk_expr_depth(left, diagnostics, next);
            walk_expr_depth(right, diagnostics, next);
        }
        ExprKind::UnOp { operand, .. } => walk_expr_depth(operand, diagnostics, next),
        ExprKind::FunctionCall { args, .. } => {
            for a in args {
                walk_expr_depth(a, diagnostics, next);
            }
        }
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expr_depth(condition, diagnostics, next);
            walk_expr_depth(then_branch, diagnostics, next);
            walk_expr_depth(else_branch, diagnostics, next);
        }
        ExprKind::ListLiteral(elems) | ExprKind::SetLiteral(elems) => {
            for e in elems {
                walk_expr_depth(e, diagnostics, next);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                walk_expr_depth(k, diagnostics, next);
                walk_expr_depth(v, diagnostics, next);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            // IndexAccess (and FunctionCall, EnumAccess, BinOp, etc.) are
            // chain-roots: a chain stops at any non-MemberAccess node. We must
            // still recurse into BOTH children so deep chains nested inside
            // (e.g. a long chain inside `index`) are detected.
            // Verified end-to-end by `index_access_resets_chain_root_emits_one_warning_post_index`.
            walk_expr_depth(object, diagnostics, next);
            walk_expr_depth(index, diagnostics, next);
        }
        ExprKind::Match { discriminant, arms } => {
            walk_expr_depth(discriminant, diagnostics, next);
            for arm in arms {
                walk_expr_depth(&arm.body, diagnostics, next);
            }
        }
        ExprKind::Lambda { body, .. } => walk_expr_depth(body, diagnostics, next),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            walk_expr_depth(collection, diagnostics, next);
            walk_expr_depth(predicate, diagnostics, next);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            walk_expr_depth(base, diagnostics, next);
            for a in args {
                walk_expr_depth(a, diagnostics, next);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => {
            walk_expr_depth(qualifier, diagnostics, next)
        }
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            walk_expr_depth(object, diagnostics, next);
            walk_expr_depth(qualified, diagnostics, next);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(l) = lower {
                walk_expr_depth(l, diagnostics, next);
            }
            if let Some(u) = upper {
                walk_expr_depth(u, diagnostics, next);
            }
        }
        // Leaf expressions — no children. `EnumAccess`, like `IndexAccess` and
        // `FunctionCall`, acts as a chain root simply by virtue of not being
        // `ExprKind::MemberAccess` — chain detection in the MemberAccess arm
        // stops as soon as `cursor.kind` is no longer `MemberAccess`. Pinned
        // by `enum_access_root_within_threshold_does_not_warn`.
        ExprKind::NumberLiteral(_)
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Auto { .. } => {}
    }
}

/// Render a chain text (e.g. `"a.b.c.d.e"`) from a pre-collected member list
/// and the chain's leaf root.
///
/// `members_outer_to_inner` is the chain's `.field` hops in outermost-first
/// order — exactly what the counting loop in [`walk_expr_depth`]'s
/// `MemberAccess` arm collects. The output format is
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
/// Bare-text consumers (CLI output, tools that print `Diagnostic.message`
/// without span context) will see the placeholder; pinned by
/// `index_access_resets_chain_root_emits_one_warning_post_index`.
fn render_chain_text(root: &Expr, members_outer_to_inner: &[&str]) -> String {
    let root_repr: String = match &root.kind {
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::SourceSpan;

    /// Table-driven depth-guard test: asserts that every structural-recursion arm
    /// in `walk_expr_depth` correctly forwards `next = depth + 1` to child nodes.
    ///
    /// A regression that accidentally passes `depth` (unchanged) instead of `next`
    /// in any single arm would silently truncate dot-chain lint coverage on that
    /// subtree. This test loops over all `(variant, target-field)` pairs so that a
    /// failing arm surfaces as a named `ArmKind` in the test output rather than a
    /// generic "walked silently" failure.
    ///
    /// `MemberAccess` is excluded: its iterative `while let` chain walk does NOT
    /// increment `depth` per layer (one frame regardless of N segments). Its only
    /// depth-incrementing call is the trailing `walk_expr_depth(cursor, ..., next)`
    /// on the leaf root — a different shape than the structural-recursion arms here.
    ///
    /// Depth arithmetic: outermost wrapper at depth 0, innermost at depth
    /// `MAX_EXPR_DEPTH`, leaf `NumberLiteral` called at `MAX_EXPR_DEPTH + 1` (= 257)
    /// — satisfies `depth > MAX_EXPR_DEPTH` and fires the `debug_assert!`.
    #[test]
    #[cfg(debug_assertions)]
    fn walk_expr_depth_panics_for_every_recursion_arm() {
        #[derive(Debug, Clone, Copy)]
        enum ArmKind {
            UnOp,
            BinOpLeft,
            BinOpRight,
            FunctionCallFirstArg,
            ConditionalCondition,
            ConditionalThen,
            ConditionalElse,
            ListLiteralFirst,
            SetLiteralFirst,
            MapLiteralFirstKey,
            MapLiteralFirstValue,
            IndexAccessObject,
            IndexAccessIndex,
            MatchScrutinee,
            MatchFirstArmBody,
            LambdaBody,
            QuantifierCollection,
            QuantifierPredicate,
            AdHocSelectorBase,
            AdHocSelectorFirstArg,
            QualifiedAccessQualifier,
            InstanceQualifiedAccessObject,
            InstanceQualifiedAccessQualified,
            RangeLower,
            RangeUpper,
            // Second-element entries for variadic arms: guards against a
            // regression where only the first iteration of a for-loop forwards
            // `next` while subsequent iterations capture the wrong variable.
            FunctionCallSecondArg,
            ListLiteralSecond,
            SetLiteralSecond,
            MapLiteralSecondKey,
            MapLiteralSecondValue,
            AdHocSelectorSecondArg,
            MatchSecondArmBody,
        }

        fn shallow_leaf(span: SourceSpan) -> Expr {
            Expr {
                kind: ExprKind::NumberLiteral(0.0),
                span,
            }
        }

        fn wrap_in_arm(arm: ArmKind, leaf: Expr, span: SourceSpan) -> Expr {
            use reify_syntax::{MatchArm, QuantifierKind};
            let kind = match arm {
                ArmKind::UnOp => ExprKind::UnOp {
                    op: "-".to_string(),
                    operand: Box::new(leaf),
                },
                ArmKind::BinOpLeft => ExprKind::BinOp {
                    op: "+".to_string(),
                    left: Box::new(leaf),
                    right: Box::new(shallow_leaf(span)),
                },
                ArmKind::BinOpRight => ExprKind::BinOp {
                    op: "+".to_string(),
                    left: Box::new(shallow_leaf(span)),
                    right: Box::new(leaf),
                },
                ArmKind::FunctionCallFirstArg => ExprKind::FunctionCall {
                    name: "f".to_string(),
                    args: vec![leaf],
                },
                ArmKind::ConditionalCondition => ExprKind::Conditional {
                    condition: Box::new(leaf),
                    then_branch: Box::new(shallow_leaf(span)),
                    else_branch: Box::new(shallow_leaf(span)),
                },
                ArmKind::ConditionalThen => ExprKind::Conditional {
                    condition: Box::new(shallow_leaf(span)),
                    then_branch: Box::new(leaf),
                    else_branch: Box::new(shallow_leaf(span)),
                },
                ArmKind::ConditionalElse => ExprKind::Conditional {
                    condition: Box::new(shallow_leaf(span)),
                    then_branch: Box::new(shallow_leaf(span)),
                    else_branch: Box::new(leaf),
                },
                ArmKind::ListLiteralFirst => ExprKind::ListLiteral(vec![leaf]),
                ArmKind::SetLiteralFirst => ExprKind::SetLiteral(vec![leaf]),
                ArmKind::MapLiteralFirstKey => {
                    ExprKind::MapLiteral(vec![(leaf, shallow_leaf(span))])
                }
                ArmKind::MapLiteralFirstValue => {
                    ExprKind::MapLiteral(vec![(shallow_leaf(span), leaf)])
                }
                ArmKind::IndexAccessObject => ExprKind::IndexAccess {
                    object: Box::new(leaf),
                    index: Box::new(shallow_leaf(span)),
                },
                ArmKind::IndexAccessIndex => ExprKind::IndexAccess {
                    object: Box::new(shallow_leaf(span)),
                    index: Box::new(leaf),
                },
                ArmKind::MatchScrutinee => ExprKind::Match {
                    discriminant: Box::new(leaf),
                    arms: vec![MatchArm {
                        patterns: vec![],
                        body: shallow_leaf(span),
                        span,
                    }],
                },
                ArmKind::MatchFirstArmBody => ExprKind::Match {
                    discriminant: Box::new(shallow_leaf(span)),
                    arms: vec![MatchArm {
                        patterns: vec![],
                        body: leaf,
                        span,
                    }],
                },
                ArmKind::LambdaBody => ExprKind::Lambda {
                    params: vec![],
                    body: Box::new(leaf),
                },
                ArmKind::QuantifierCollection => ExprKind::Quantifier {
                    kind: QuantifierKind::ForAll,
                    variable: "x".into(),
                    collection: Box::new(leaf),
                    predicate: Box::new(shallow_leaf(span)),
                },
                ArmKind::QuantifierPredicate => ExprKind::Quantifier {
                    kind: QuantifierKind::ForAll,
                    variable: "x".into(),
                    collection: Box::new(shallow_leaf(span)),
                    predicate: Box::new(leaf),
                },
                ArmKind::AdHocSelectorBase => ExprKind::AdHocSelector {
                    base: Box::new(leaf),
                    selector: "_".into(),
                    args: vec![],
                },
                ArmKind::AdHocSelectorFirstArg => ExprKind::AdHocSelector {
                    base: Box::new(shallow_leaf(span)),
                    selector: "_".into(),
                    args: vec![leaf],
                },
                ArmKind::QualifiedAccessQualifier => ExprKind::QualifiedAccess {
                    qualifier: Box::new(leaf),
                    member: "m".to_string(),
                },
                ArmKind::InstanceQualifiedAccessObject => ExprKind::InstanceQualifiedAccess {
                    object: Box::new(leaf),
                    qualified: Box::new(shallow_leaf(span)),
                },
                ArmKind::InstanceQualifiedAccessQualified => ExprKind::InstanceQualifiedAccess {
                    object: Box::new(shallow_leaf(span)),
                    qualified: Box::new(leaf),
                },
                ArmKind::RangeLower => ExprKind::Range {
                    lower: Some(Box::new(leaf)),
                    upper: Some(Box::new(shallow_leaf(span))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
                ArmKind::RangeUpper => ExprKind::Range {
                    lower: Some(Box::new(shallow_leaf(span))),
                    upper: Some(Box::new(leaf)),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
                ArmKind::FunctionCallSecondArg => ExprKind::FunctionCall {
                    name: "f".to_string(),
                    args: vec![shallow_leaf(span), leaf],
                },
                ArmKind::ListLiteralSecond => {
                    ExprKind::ListLiteral(vec![shallow_leaf(span), leaf])
                }
                ArmKind::SetLiteralSecond => {
                    ExprKind::SetLiteral(vec![shallow_leaf(span), leaf])
                }
                ArmKind::MapLiteralSecondKey => ExprKind::MapLiteral(vec![
                    (shallow_leaf(span), shallow_leaf(span)),
                    (leaf, shallow_leaf(span)),
                ]),
                ArmKind::MapLiteralSecondValue => ExprKind::MapLiteral(vec![
                    (shallow_leaf(span), shallow_leaf(span)),
                    (shallow_leaf(span), leaf),
                ]),
                ArmKind::AdHocSelectorSecondArg => ExprKind::AdHocSelector {
                    base: Box::new(shallow_leaf(span)),
                    selector: "_".into(),
                    args: vec![shallow_leaf(span), leaf],
                },
                ArmKind::MatchSecondArmBody => ExprKind::Match {
                    discriminant: Box::new(shallow_leaf(span)),
                    arms: vec![
                        MatchArm {
                            patterns: vec![],
                            body: shallow_leaf(span),
                            span,
                        },
                        MatchArm {
                            patterns: vec![],
                            body: leaf,
                            span,
                        },
                    ],
                },
            };
            Expr { kind, span }
        }

        let span = SourceSpan::empty(0);
        let arms = [
            ArmKind::UnOp,
            ArmKind::BinOpLeft,
            ArmKind::BinOpRight,
            ArmKind::FunctionCallFirstArg,
            ArmKind::ConditionalCondition,
            ArmKind::ConditionalThen,
            ArmKind::ConditionalElse,
            ArmKind::ListLiteralFirst,
            ArmKind::SetLiteralFirst,
            ArmKind::MapLiteralFirstKey,
            ArmKind::MapLiteralFirstValue,
            ArmKind::IndexAccessObject,
            ArmKind::IndexAccessIndex,
            ArmKind::MatchScrutinee,
            ArmKind::MatchFirstArmBody,
            ArmKind::LambdaBody,
            ArmKind::QuantifierCollection,
            ArmKind::QuantifierPredicate,
            ArmKind::AdHocSelectorBase,
            ArmKind::AdHocSelectorFirstArg,
            ArmKind::QualifiedAccessQualifier,
            ArmKind::InstanceQualifiedAccessObject,
            ArmKind::InstanceQualifiedAccessQualified,
            ArmKind::RangeLower,
            ArmKind::RangeUpper,
            ArmKind::FunctionCallSecondArg,
            ArmKind::ListLiteralSecond,
            ArmKind::SetLiteralSecond,
            ArmKind::MapLiteralSecondKey,
            ArmKind::MapLiteralSecondValue,
            ArmKind::AdHocSelectorSecondArg,
            ArmKind::MatchSecondArmBody,
        ];
        for arm in arms {
            // Sanity pass: a single-layer wrap must NOT panic. Guards against
            // `wrap_in_arm` silently constructing a broken or depth-saturated
            // AST, which would make the depth-saturation assertion below
            // trivially vacuous (or erroneously attribute a construction panic
            // to a missing `next` forward).
            let shallow_wrapped = wrap_in_arm(arm, shallow_leaf(span), span);
            let sanity = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut diagnostics: Vec<Diagnostic> = Vec::new();
                walk_expr(&shallow_wrapped, &mut diagnostics);
            }));
            assert!(
                sanity.is_ok(),
                "arm {:?} panicked on a single-layer (depth-1) wrap — \
                 wrap_in_arm may construct a broken AST or a leaf-equivalent node",
                arm
            );

            let mut expr = Expr {
                kind: ExprKind::NumberLiteral(0.0),
                span,
            };
            for _ in 0..(MAX_EXPR_DEPTH + 1) {
                expr = wrap_in_arm(arm, expr, span);
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut diagnostics: Vec<Diagnostic> = Vec::new();
                walk_expr(&expr, &mut diagnostics);
            }));
            match result {
                Ok(_) => panic!(
                    "arm {:?} did NOT trip MAX_EXPR_DEPTH guard — depth was not forwarded",
                    arm
                ),
                Err(payload) => {
                    let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        String::new()
                    };
                    assert!(
                        msg.contains("MAX_EXPR_DEPTH"),
                        "arm {:?} panicked but message {:?} did not mention MAX_EXPR_DEPTH",
                        arm,
                        msg
                    );
                }
            }
        }
    }
}
