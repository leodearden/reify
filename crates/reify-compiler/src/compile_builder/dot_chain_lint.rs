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

use reify_ast::{Expr, ExprKind, MemberDecl, ParsedModule, walk_all_member_bodies};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel};

use crate::recursion_guard::MAX_COMPILE_RECURSION_DEPTH;

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
///   * `Structure`/`Occurrence`/`Trait`/`Purpose`: `annotations[*].args` +
///     `members` (delegates to [`walk_members`]).
///   * `Purpose` also: each of its `structures`, walked like a top-level
///     `Structure` (delegates to [`walk_structure`]).
///   * `Function`: `annotations[*].args` + `body.let_bindings[*].value` +
///     `body.result_expr`.
///   * `Field`: `annotations[*].args` + `source` (Analytical/Composed `expr`,
///     Sampled config values).
///   * `Constraint` (named def): `annotations[*].args` +
///     `params[*].(default | where_clause.condition)` + `predicates[*]`.
///   * `Unit`: `annotations[*].args` + `conversion` + `offset`.
///   * `Enum`/`Import`/`TypeAlias`: `annotations[*].args` only (no other
///     embedded expressions today).
fn walk_declaration(decl: &reify_ast::Declaration, diagnostics: &mut Vec<Diagnostic>) {
    use reify_ast::{Declaration, FieldSource};
    match decl {
        Declaration::Structure(s) => walk_structure(s, diagnostics),
        Declaration::Occurrence(o) => {
            walk_annotations(&o.annotations, diagnostics);
            walk_members(&o.members, diagnostics);
        }
        Declaration::Trait(t) => {
            walk_annotations(&t.annotations, diagnostics);
            walk_members(&t.members, diagnostics);
        }
        Declaration::Purpose(p) => {
            walk_annotations(&p.annotations, diagnostics);
            walk_members(&p.members, diagnostics);
            for s in &p.structures {
                walk_structure(s, diagnostics);
            }
        }
        Declaration::Function(f) => {
            walk_annotations(&f.annotations, diagnostics);
            // Top-level fns always have Some body; bodyless trait fns (None)
            // are deferred to task δ and have no expressions to walk.
            if let Some(body) = &f.body {
                for binding in &body.let_bindings {
                    walk_expr(&binding.value, diagnostics);
                    if let Some(wc) = &binding.where_clause {
                        walk_expr(&wc.condition, diagnostics);
                    }
                }
                walk_expr(&body.result_expr, diagnostics);
            }
        }
        Declaration::Field(f) => {
            walk_annotations(&f.annotations, diagnostics);
            match &f.source {
                FieldSource::Analytical { expr } | FieldSource::Composed { expr } => {
                    walk_expr(expr, diagnostics);
                }
                FieldSource::Sampled { config } => {
                    for (_, expr) in config {
                        walk_expr(expr, diagnostics);
                    }
                }
                FieldSource::Imported { .. } => {}
            }
        }
        Declaration::Constraint(c) => {
            walk_annotations(&c.annotations, diagnostics);
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
            walk_annotations(&u.annotations, diagnostics);
            if let Some(conv) = &u.conversion {
                walk_expr(conv, diagnostics);
            }
            if let Some(off) = &u.offset {
                walk_expr(off, diagnostics);
            }
        }
        // These declarations have no embedded body expressions, but their
        // annotation args are full expressions and must be walked.
        Declaration::Enum(e) => walk_annotations(&e.annotations, diagnostics),
        Declaration::Import(i) => walk_annotations(&i.annotations, diagnostics),
        Declaration::TypeAlias(t) => walk_annotations(&t.annotations, diagnostics),
        // A module declaration has no annotations and no embedded expressions.
        Declaration::Module(_) => {}
        // A default declaration has no annotations and no embedded dot-chain expressions
        // to walk in task A (the RHS is accepted but not compiled; task B).
        Declaration::Default(_) => {}
        // Grammar producer only (task α 4395). Body/DOF dot-chain lint is
        // deferred to task β (DOF self-check + body type enforcement).
        Declaration::Joint(_) => {}
    }
}

/// Walk a structure's `annotations[*].args` and `members`. It is the one walk for
/// both a top-level structure and a structure nested in a purpose body.
fn walk_structure(s: &reify_ast::StructureDef, diagnostics: &mut Vec<Diagnostic>) {
    walk_annotations(&s.annotations, diagnostics);
    walk_members(&s.members, diagnostics);
}

/// Walk every member under `members`; recursion and depth bound are reify-ast's
/// ([`walk_all_member_bodies`]).
fn walk_members(members: &[MemberDecl], diagnostics: &mut Vec<Diagnostic>) {
    walk_all_member_bodies(members, &mut |member| {
        walk_member_own_exprs(member, diagnostics)
    });
}

/// Walk the expressions `member` carries itself. Nested member lists are NOT
/// walked here; [`walk_members`] reaches them.
///
/// Visits:
///   * `Param`: `annotations[*].args` + `default` + `where_clause.condition`.
///   * `Let`: `annotations[*].args` + `value` + `where_clause.condition`.
///   * `Constraint` (bare-expression form): `expr` + `where_clause.condition`.
///   * `ConstraintInst`: `args[*].1` + `where_clause.condition`.
///   * `Sub`: `args[*].1` + `where_clause.condition`.
///   * `Minimize`/`Maximize`: `expr` + `where_clause.condition`.
///   * `GuardedGroup`: `condition`.
///   * `Port`: `frame_expr`.
///   * `MatchArmDeclGroup`: `discriminant`.
///   * `Connect`: `left.expr`, `right.expr`, `params[*].1`.
///   * `Chain`: each `elements[*]`.
///   * `ForallConnect`: `collection` + every body expr (delegated to
///     [`super::forall_walk::walk_forall_connect_body`]).
///   * `ForallConstraint`: `collection` + every body expr (delegated to
///     [`super::forall_walk::walk_forall_constraint_body`]).
///   * `Sketch`: no own expressions; its body is reached via [`walk_members`].
///   * `AssociatedType`/`MetaBlock`: no expressions, no-op.
///
/// The match has no `_` wildcard: reify-ast's walker forces the recursion
/// decision for a new `MemberDecl` variant, and this match forces its
/// expression decision.
fn walk_member_own_exprs(member: &MemberDecl, diagnostics: &mut Vec<Diagnostic>) {
    match member {
        // Relate-block relations are ordinary expressions — walk them so
        // dot-chain lints apply inside `relate { … }` too (task δ 4384).
        MemberDecl::Relate(r) => {
            for rel in &r.relations {
                walk_expr(rel, diagnostics);
            }
        }
        MemberDecl::Param(p) => {
            walk_annotations(&p.annotations, diagnostics);
            if let Some(default) = &p.default {
                walk_expr(default, diagnostics);
            }
            if let Some(wc) = &p.where_clause {
                walk_expr(&wc.condition, diagnostics);
            }
        }
        MemberDecl::Let(l) => {
            walk_annotations(&l.annotations, diagnostics);
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
        MemberDecl::GuardedGroup(g) => walk_expr(&g.condition, diagnostics),
        MemberDecl::Port(p) => {
            if let Some(frame) = &p.frame_expr {
                walk_expr(frame, diagnostics);
            }
        }
        MemberDecl::MatchArmDeclGroup(g) => walk_expr(&g.discriminant, diagnostics),
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
        MemberDecl::ForallConnect(f) => {
            walk_expr(&f.collection, diagnostics);
            super::forall_walk::walk_forall_connect_body(&f.body, |expr| {
                walk_expr(expr, diagnostics);
            });
        }
        MemberDecl::ForallConstraint(f) => {
            walk_expr(&f.collection, diagnostics);
            super::forall_walk::walk_forall_constraint_body(&f.body, |expr| {
                walk_expr(expr, diagnostics);
            });
        }
        // Trait associated-fn bodies (task ζ 3941). ζ makes trait-fn bodies
        // live, so the deep-dot-chain lint now recurses into every
        // expression-bearing position of the fn: each param default, each
        // let-binding value (and its where-clause condition), and the result
        // expression. A bodyless required trait fn (`fn req(self) -> Real`
        // with no `{ … }`) has `body: None` and contributes no expressions.
        //
        // NOTE: `shadow_lint.rs::walk_members_depth` makes the same
        // per-member expression decisions; a newly added expression-bearing
        // member kind must be handled in both. Only those decisions are
        // duplicated: this lint takes its recursion from reify-ast, but
        // shadow_lint still hand-rolls its own, because it pushes a scope
        // frame per nested body.
        MemberDecl::Fn(f) => {
            for param in &f.params {
                if let Some(default) = &param.default {
                    walk_expr(default, diagnostics);
                }
            }
            if let Some(body) = &f.body {
                for let_decl in &body.let_bindings {
                    walk_expr(&let_decl.value, diagnostics);
                    if let Some(wc) = &let_decl.where_clause {
                        walk_expr(&wc.condition, diagnostics);
                    }
                }
                walk_expr(&body.result_expr, diagnostics);
            }
        }
        // No expression of its own; `walk_members` reaches its body.
        MemberDecl::Sketch(_) => {}
        // Members with no embedded expressions.
        MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => {}
    }
}

/// Walk every arg of every annotation in `annotations`, emitting a diagnostic
/// for each deep `MemberAccess` chain found.
///
/// Annotation args are arbitrary `Expr` values (spec §5.7 covers all user
/// expressions), so they require the same chain-detection pass as any other
/// expression-bearing position. Depth-bounding is inherited for free from the
/// existing [`walk_expr`] → [`walk_expr_depth`] entry point.
fn walk_annotations(annotations: &[reify_ast::Annotation], diagnostics: &mut Vec<Diagnostic>) {
    for ann in annotations {
        for arg in &ann.args {
            // Each annotation arg is an independent expression root:
            // walk_expr starts a fresh depth budget (walk_expr_depth(_, _, 0))
            // per arg, not a continuation of any surrounding expression context.
            walk_expr(arg, diagnostics);
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
/// Recursion into expression children is bounded by
/// [`MAX_COMPILE_RECURSION_DEPTH`] for stack safety on pathological input.
fn walk_expr(expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
    walk_expr_depth(expr, diagnostics, 0);
}

fn walk_expr_depth(expr: &Expr, diagnostics: &mut Vec<Diagnostic>, depth: usize) {
    // Past the shared cap the lint stops silently; for compiled expressions the
    // compiler's guard reports E_EXPR_NESTING_TOO_DEEP.
    if depth > MAX_COMPILE_RECURSION_DEPTH {
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
        ExprKind::TraitMethodCall { object, args, .. } => {
            walk_expr_depth(object, diagnostics, next);
            for a in args {
                walk_expr_depth(a, diagnostics, next);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for a in args {
                walk_expr_depth(a, diagnostics, next);
            }
        }
        // VariantConstruct — recurse into field value exprs.
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, v) in fields {
                walk_expr_depth(v, diagnostics, next);
            }
        }
        // InterpolatedString — recurse into each Hole expr; Literal parts are leaves.
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let reify_ast::StringPart::Hole(e) = part {
                    walk_expr_depth(e, diagnostics, next);
                }
            }
        }
        // `auto(seed = a.b.c.d, …)` params carry arbitrary value expressions, so
        // recurse into each so deep dot-chains nested inside them are still
        // linted (geometric-relations δ, 4384).
        ExprKind::Auto { params, .. } => {
            for (_, v) in params {
                walk_expr_depth(v, diagnostics, next);
            }
        }
        // Leaf expressions — no children. `EnumAccess`, like `IndexAccess` and
        // `FunctionCall`, acts as a chain root simply by virtue of not being
        // `ExprKind::MemberAccess` — chain detection in the MemberAccess arm
        // stops as soon as `cursor.kind` is no longer `MemberAccess`. Pinned
        // by `enum_access_root_within_threshold_does_not_warn`.
        ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Undef => {}
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
/// Root rendering (shape-hinting placeholders):
///   * `Ident(name)` → `name`
///   * `EnumAccess { type_name, variant }` → `"{type_name}.{variant}"`
///   * `IndexAccess { .. }` → `"_[…]"` — echoes `expr[i]` index-access syntax
///   * `FunctionCall { .. }` → `"_(…)"` — echoes `f(args)` call syntax
///   * All other variants (BinOp, UnOp, Lambda, Conditional, literals, …) → `"_"`
///
/// The placeholders are the contract for bare-text consumers (CLI output,
/// tools that print `Diagnostic.message` without span context). Span-aware
/// editor renderings (LSP/MCP) display the user's literal source text via
/// the diagnostic squiggle and are unaffected by the placeholder text.
fn render_chain_text(root: &Expr, members_outer_to_inner: &[&str]) -> String {
    let root_repr: String = match &root.kind {
        ExprKind::Ident(name) => name.clone(),
        ExprKind::EnumAccess { type_name, variant } => format!("{type_name}.{variant}"),
        ExprKind::IndexAccess { .. } => "_[\u{2026}]".to_string(),
        ExprKind::FunctionCall { .. } => "_(\u{2026})".to_string(),
        _ => "_".to_string(),
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
    use crate::recursion_guard::MAX_COMPILE_RECURSION_DEPTH as CAP;
    use crate::recursion_guard::test_support::{NestingArm, on_tokio_worker_stack};
    use reify_core::SourceSpan;

    /// Every recursion arm of `walk_expr_depth` forwards `depth + 1`, and the
    /// lint covers nesting up to the shared compile cap and stops silently past
    /// it: a deep chain nested so it sits AT the cap is reported, one level
    /// deeper it is skipped. Runs on a 2 MiB stack, the GUI's compile stack.
    #[test]
    fn deep_chain_is_linted_through_the_nesting_cap_and_skipped_past_it_for_every_arm() {
        /// `a.b.c.d.e`: chain length 5, one past DEEP_DOT_CHAIN_THRESHOLD.
        fn deep_chain() -> Expr {
            let span = SourceSpan::empty(0);
            let mut chain = Expr {
                kind: ExprKind::Ident("a".to_string()),
                span,
            };
            for member in ["b", "c", "d", "e"] {
                chain = Expr {
                    kind: ExprKind::MemberAccess {
                        object: Box::new(chain),
                        member: member.to_string(),
                    },
                    span,
                };
            }
            chain
        }

        fn deep_dot_chain_count(arm: NestingArm, depth: usize) -> usize {
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            walk_expr(&arm.nest_at_depth(deep_chain(), depth), &mut diagnostics);
            assert!(
                diagnostics
                    .iter()
                    .all(|d| d.code == Some(DiagnosticCode::DeepDotChain)),
                "arm {arm:?} with the chain at depth {depth} emitted a non-DeepDotChain \
                 diagnostic: {diagnostics:?}"
            );
            diagnostics.len()
        }

        on_tokio_worker_stack(|| {
            for &arm in NestingArm::ALL {
                assert_eq!(
                    deep_dot_chain_count(arm, CAP),
                    1,
                    "arm {arm:?}: a deep chain at depth {CAP} sits at the cap and must \
                     be linted"
                );
                assert_eq!(
                    deep_dot_chain_count(arm, CAP + 1),
                    0,
                    "arm {arm:?}: a deep chain at depth {} sits past the cap and must be \
                     skipped — a non-zero count means this arm did not forward depth + 1",
                    CAP + 1
                );
            }
        });
    }

    /// `render_chain_text` falls back to `_` for root variants that are
    /// neither `Ident`, `EnumAccess`, `IndexAccess`, nor `FunctionCall` (e.g.
    /// `BinOp`). The chain text is `_.c.d.e` when `members_outer_to_inner` is
    /// `["e", "d", "c"]` (outermost first, reversed on output).
    ///
    /// Done as a unit test rather than an integration test because constructing
    /// a `BinOp`-rooted chain via parsed source depends on `parenthesized_expression`
    /// lowering specifics that are orthogonal to this task. The existing
    /// `deep_chain_is_linted_through_the_nesting_cap_and_skipped_past_it_for_every_arm`
    /// test already builds `Expr` trees by hand with `SourceSpan::empty(0)` —
    /// same pattern here.
    #[test]
    fn render_chain_text_uses_underscore_for_other_root_variants() {
        let span = SourceSpan::empty(0);
        let root = Expr {
            kind: ExprKind::BinOp {
                op: "+".to_string(),
                left: Box::new(Expr {
                    kind: ExprKind::Ident("a".to_string()),
                    span,
                }),
                right: Box::new(Expr {
                    kind: ExprKind::Ident("b".to_string()),
                    span,
                }),
            },
            span,
        };
        // members_outer_to_inner = ["e", "d", "c"] → output is "_.c.d.e"
        let result = render_chain_text(&root, &["e", "d", "c"]);
        assert_eq!(
            result, "_.c.d.e",
            "render_chain_text must fall back to `_` for a BinOp root, got: {:?}",
            result
        );
    }
}
