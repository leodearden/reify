//! Shared body-shape walkers for `MemberDecl::ForallConnect` /
//! `MemberDecl::ForallConstraint` (spec §5.4 statement-form forall).
//!
//! Both `shadow_lint` and `dot_chain_lint` need to traverse every
//! expression-bearing position inside a forall body, but with different
//! per-expression actions:
//!
//!   * `shadow_lint` walks each expression under the forall's child
//!     `FrameStack` (so the bound variable is in scope).
//!   * `dot_chain_lint` runs the deep-chain detection lint on each
//!     expression (no scope tracking).
//!
//! The body shape itself is identical between the two walkers, so it lives
//! here as a pair of `FnMut(&Expr)`-taking helpers. Adding a new body
//! variant (e.g. a future `ForallConnectBody::Bridge`) becomes a single
//! grep target rather than two parallel sites that can drift independently.
//!
//! Note: these helpers walk EVERY expression-bearing position the body
//! exposes, but they don't recurse into sub-expressions. The visitor
//! closure is responsible for whatever sub-expression traversal it needs
//! (frame-aware in `shadow_lint`, chain-counting in `dot_chain_lint`).

use reify_syntax::{Expr, ForallConnectBody, ForallConstraintBody};

/// Walk every expression-bearing position inside a [`ForallConnectBody`].
///
/// Visits, in source order:
///   * [`ForallConnectBody::Connect`]: `c.left.expr`, `c.right.expr`, then
///     each `c.params[*].1`.
///   * [`ForallConnectBody::Chain`]: each `c.elements[*]`.
pub(crate) fn walk_forall_connect_body(body: &ForallConnectBody, mut visit: impl FnMut(&Expr)) {
    match body {
        ForallConnectBody::Connect(c) => {
            visit(&c.left.expr);
            visit(&c.right.expr);
            for (_, expr) in &c.params {
                visit(expr);
            }
        }
        ForallConnectBody::Chain(c) => {
            for elem in &c.elements {
                visit(elem);
            }
        }
    }
}

/// Walk every expression-bearing position inside a [`ForallConstraintBody`].
///
/// Visits, in source order:
///   * [`ForallConstraintBody::Constraint`]: `c.expr`, then
///     `c.where_clause.condition` if present.
///   * [`ForallConstraintBody::Instantiation`]: each `ci.args[*].1`, then
///     `ci.where_clause.condition` if present.
pub(crate) fn walk_forall_constraint_body(
    body: &ForallConstraintBody,
    mut visit: impl FnMut(&Expr),
) {
    match body {
        ForallConstraintBody::Constraint(c) => {
            visit(&c.expr);
            if let Some(wc) = &c.where_clause {
                visit(&wc.condition);
            }
        }
        ForallConstraintBody::Instantiation(ci) => {
            for (_, expr) in &ci.args {
                visit(expr);
            }
            if let Some(wc) = &ci.where_clause {
                visit(&wc.condition);
            }
        }
    }
}
