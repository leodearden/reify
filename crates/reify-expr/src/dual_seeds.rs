//! The ordered seed columns a dual traversal differentiates against, with the
//! memoised per-subtree predicates that gate its seed-independence fast path.

use std::cell::RefCell;
use std::collections::HashMap;

use reify_core::{ContentHash, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind};

use crate::dual_builtin_rules::{is_bounded_reduction_shaped, is_kink_builtin};
use crate::non_differentiable::NonDifferentiable;

/// The ordered column vector a dual evaluation is differentiated against.
///
/// Column `j` of every tangent row corresponds to `columns()[j]`.  Consumers
/// (η's Jacobian, μ's reduced gradient) rely on that ordering being *theirs* —
/// it is the caller's `auto_params` order, not an internal one.
pub struct Seeds {
    columns: Vec<ValueCellId>,
    index: HashMap<ValueCellId, usize>,
    /// Memos for the two per-subtree predicates, keyed on
    /// `CompiledExpr::content_hash`.
    ///
    /// Without them, asking "does this subtree touch a seed?" (and "does it
    /// contain a kink?") at every node walks each node's whole subtree —
    /// quadratic in depth.  Both predicates are pure functions of
    /// `(subtree, seed set)`, and a `Seeds` *is* the seed set, so this is the
    /// correct place to cache them.
    depends: RefCell<HashMap<ContentHash, bool>>,
    kinky: RefCell<HashMap<ContentHash, bool>>,
    /// The FIRST reason a tangent could not be produced during the current
    /// traversal, with the site that produced it.
    ///
    /// The traversal cannot return this alongside the tangent without either
    /// widening [`Tangent::None`] (which would make `Tangent` unusable as a
    /// plain comparison value) or threading an eighth parameter through every
    /// arm, so it is collected here and drained by
    /// [`jacobian_row`].  First-wins: the innermost, earliest refusal is the
    /// one that actually explains the failure; later ones are its consequences.
    ///
    /// [`Tangent::None`]: crate::Tangent::None
    /// [`jacobian_row`]: crate::jacobian_row
    refusal: RefCell<Option<NonDifferentiable>>,
}

impl Seeds {
    /// Build a seed set from an ordered column vector.
    ///
    /// Duplicate cells keep their FIRST column: a repeated auto param would
    /// otherwise split one variable's derivative across two columns.
    pub fn new(columns: &[ValueCellId]) -> Self {
        let mut index = HashMap::with_capacity(columns.len());
        for (j, id) in columns.iter().enumerate() {
            index.entry(id.clone()).or_insert(j);
        }
        Seeds {
            columns: columns.to_vec(),
            index,
            depends: RefCell::new(HashMap::new()),
            kinky: RefCell::new(HashMap::new()),
            refusal: RefCell::new(None),
        }
    }

    /// Record why a tangent could not be produced.  The FIRST call in a
    /// traversal wins; later refusals are downstream consequences of it.
    pub(crate) fn note_refusal(&self, reason: NonDifferentiable) {
        let mut slot = self.refusal.borrow_mut();
        if slot.is_none() {
            *slot = Some(reason);
        }
    }

    /// Drain the recorded refusal, leaving the sink empty for the next row.
    ///
    /// A caller that drives MORE THAN ONE traversal through one `Seeds` — the
    /// solver's dependent-cell fold does — must drain between them.
    /// [`eval_dual_with_env`] clears the slot on entry, so a reason left in it
    /// is a reason the next traversal discards.
    ///
    /// [`eval_dual_with_env`]: crate::eval_dual_with_env
    pub fn take_refusal(&self) -> Option<NonDifferentiable> {
        self.refusal.borrow_mut().take()
    }

    /// Number of columns — the width of every tangent row.
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    /// The column vector, in order.
    pub fn columns(&self) -> &[ValueCellId] {
        &self.columns
    }

    /// The column index of `id`, or `None` when `id` is not a seed.
    pub fn column_of(&self, id: &ValueCellId) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// True when `expr`'s subtree references at least one seed cell.
    ///
    /// A `false` answer is a *proof* that the whole subtree is constant with
    /// respect to every seed.
    ///
    /// # Why `contains_value_ref_matching` and not `expr.walk`
    ///
    /// `walk` sits right here in [`Self::subtree_has_kink`], so reaching for it
    /// is the obvious shortcut — and it answers a DIFFERENT question.  On a
    /// `Lambda`, the dependency traversal reports the CAPTURES and does not
    /// descend into the body, while `walk` descends into the body and never
    /// sees the captures.  A walk-based predicate would therefore answer
    /// `false` for a lambda that captures a seed — and `false` here is a PROOF
    /// the fast path acts on, so the subtree would be handed to `eval_expr` and
    /// lifted with `Tangent::Zero`: a silently flat row for a residual that
    /// genuinely moves.
    ///
    /// Those dependency semantics live on `CompiledExpr` in reify-ir and are
    /// deliberately not restated here: a second copy of its arms one level
    /// down would drift from the first.
    pub fn depends_on_seed(&self, expr: &CompiledExpr) -> bool {
        if let Some(hit) = self.depends.borrow().get(&expr.content_hash) {
            return *hit;
        }
        let answer = expr.contains_value_ref_matching(|id| self.index.contains_key(id));
        self.depends.borrow_mut().insert(expr.content_hash, answer);
        answer
    }

    /// True when `expr`'s subtree contains at least one non-smooth node, **or
    /// any node this walk cannot see through**.
    ///
    /// This gates the seed-independence fast path.  Skipping a seed-independent
    /// subtree is sound for *tangents*, but it would also skip any kink inside
    /// it — and then "the record is empty" would mean "we did not look" rather
    /// than "there is no kink here", which is precisely the guarantee λ (#6679)
    /// builds on.  `node_is_kink` therefore also claims the two node kinds
    /// whose interior is not in the tree at all (a user-function call's body, an
    /// arity-1 call a `__field__` cell may shadow with a lambda), so the fast
    /// path can never be the thing that swallowed a branch.
    ///
    /// A `true` here keeps the node off the fast path, and every kind the
    /// traversal DIFFERENTIATES then records its own kinks; a node it delegates
    /// whole records one [`KinkKind::Unplaced`](crate::KinkKind::Unplaced)
    /// entry instead.
    pub fn subtree_has_kink(&self, expr: &CompiledExpr) -> bool {
        if let Some(hit) = self.kinky.borrow().get(&expr.content_hash) {
            return *hit;
        }
        let mut found = false;
        expr.walk(&mut |e| {
            if !found && node_is_kink(e) {
                found = true;
            }
        });
        self.kinky.borrow_mut().insert(expr.content_hash, found);
        found
    }
}

/// Is THIS node (not its subtree) non-smooth, **or opaque to the walk that asks**?
/// O(1) on the node's kind.
///
/// Sole caller: [`Seeds::subtree_has_kink`], which gates the seed-independence
/// fast path.  So a `true` here only ever costs a fast path, and the two
/// `true`s that are not really about smoothness — the calls below — are there
/// because a fast path taken over a kink is the one failure mode λ (#6679) has
/// no defence against.
fn node_is_kink(e: &CompiledExpr) -> bool {
    match &e.kind {
        CompiledExprKind::Conditional { .. } | CompiledExprKind::Match { .. } => true,
        // A CALL IS OPAQUE TO THE WALK, so "is there a kink in here?" is not a
        // question `subtree_has_kink` can answer about one — and the honest
        // answer to an unanswerable question, for a predicate whose `false` is
        // acted on as a PROOF, is the conservative one.
        //
        // `CompiledExpr::walk` visits a `UserFunctionCall`'s arguments only:
        // the callee body lives in the function table, which a bare expression
        // cannot reach.  An arity-1 `FunctionCall` has the same problem by a
        // different route — a `Value::Field` cell named `__field__::<name>` may
        // shadow the builtin with a LAMBDA (see `eval_dual_builtin`), and that
        // lambda is in the value map, not in the tree at all.
        //
        // Cost: one extra node visit per call on an otherwise fast-pathed
        // subtree.  Its own arguments keep the fast path (they are ordinary
        // subtrees), the shadow-cell probe is one `eval_expr` pays for every
        // arity-1 call anyway, and a traversed call records its branches
        // properly — `eval_dual_fn_body` and `eval_dual_lambda_apply` both do.
        // Under a delegated node it costs a possibly spurious `Unplaced` entry.
        CompiledExprKind::UserFunctionCall { .. } => true,
        CompiledExprKind::FunctionCall { args, .. } if args.len() == 1 => true,
        CompiledExprKind::BinOp { op, .. } => matches!(
            op,
            BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::And
                | BinOp::Or
                | BinOp::Implies
                | BinOp::Mod
        ),
        CompiledExprKind::FunctionCall { function, args } => {
            is_kink_builtin(&function.name, args.len())
                || is_bounded_reduction_shaped(&function.name, args.len())
        }
        _ => false,
    }
}
