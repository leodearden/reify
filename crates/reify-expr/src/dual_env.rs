//! The tangents bound to cells that are not seed columns (callee parameters
//! and `let`s, solver-derived cells) while a dual traversal runs.

use std::cell::RefCell;
use std::collections::HashMap;

use reify_core::{ContentHash, ValueCellId};
use reify_ir::CompiledExpr;

use crate::dual::Tangent;
use crate::non_differentiable::NonDifferentiable;

/// Tangents bound to cells that are **not** seed columns: a user function's or
/// lambda's parameters, and a callee's `let` bindings, while its body is being
/// evaluated.
///
/// A parameter cell is not a seed — it is an *interior* cell whose tangent was
/// computed from the seeds at the call site — so `Seeds` alone cannot resolve
/// it.  Without this overlay a function body would look seed-independent and
/// every user-defined function would differentiate to zero.
///
/// An *unbound* lookup falls through to [`Tangent::Zero`] — the right answer
/// for a cell that genuinely does not move with the seeds — but a cell bound
/// to `Zero` is still stored, because a binding is AUTHORITATIVE over every
/// earlier binding of the same cell.  Reify lets a `let` shadow a parameter
/// (a `Shadowing` warning, not an error) and lets two same-named `let`s
/// coexist with no diagnostic at all, so a cell IS bound more than once; a
/// skipped zero rebind would leave the shadowed binding's tangent live and
/// fabricate a non-zero derivative.  "Is this cell bound?" is therefore a
/// weaker question than "does it carry a tangent?", and only the former gates
/// the `DualEnv::carries_tangent` fast path — erring towards descending.
#[derive(Default)]
pub struct DualEnv {
    bindings: HashMap<ValueCellId, Tangent>,
    /// Memo for [`DualEnv::carries_tangent`], scoped to one body evaluation,
    /// which is exactly where it pays.
    ///
    /// Each answer carries the [`DualEnv::generation`] that produced it,
    /// because the two answers age differently.  Bindings only ever GROW, so a
    /// `true` is permanent: an expression that reaches a bound cell cannot stop
    /// reaching it.  A `false` is only a claim about the binding set at the
    /// time, so it is re-checked once that set changes.  Clearing the whole
    /// memo per binding instead makes `eval_dual_fn_body`'s let-binding loop
    /// re-walk the remaining body once per `let`.
    carries: RefCell<HashMap<ContentHash, (u32, bool)>>,
    /// Why a cell was bound as [`Tangent::None`], for the cells whose binder
    /// knew.  Read at the point a residual READS the cell, so a row that never
    /// touches it is never told about it.
    causes: HashMap<ValueCellId, NonDifferentiable>,
    /// Bumped once per newly bound cell — never on a rebind, which cannot
    /// change a membership answer.
    generation: u32,
}

impl DualEnv {
    /// The empty overlay — the state at the residual root.
    pub fn new() -> Self {
        DualEnv::default()
    }

    /// Bind `cell` to `tangent`, displacing any earlier binding of the same
    /// cell — including with a [`Tangent::Zero`], which is what makes a
    /// shadowing `let` able to FLATTEN the tangent of the parameter or `let`
    /// it shadows rather than inherit it.
    ///
    /// This is also the only binder that can retire a recorded refusal: a cell
    /// rebound to a real tangent is differentiable again, so its stale cause
    /// must not outlive it and condemn the read.
    pub fn bind(&mut self, cell: ValueCellId, tangent: Tangent) {
        self.causes.remove(&cell);
        self.insert(cell, tangent);
    }

    /// Bind `cell` as NON-differentiable, keeping the reason.
    ///
    /// `bind(cell, Tangent::None)` already says the tangent is unavailable;
    /// this also says WHY, so a residual that reads the cell refuses with the
    /// construct responsible instead of the generic root-sited fallback.  The
    /// reason is raised at the READ, not here, which is what keeps a poisoned
    /// derived cell from condemning rows that never look at it.
    pub fn bind_refused(&mut self, cell: ValueCellId, cause: NonDifferentiable) {
        self.causes.insert(cell.clone(), cause);
        self.insert(cell, Tangent::None);
    }

    /// The one write to `bindings`, so the [`DualEnv::generation`] counter
    /// cannot drift from the key set it describes.
    ///
    /// It is also where the tangent is CANONICALISED.  Everywhere inside the
    /// traversal an all-zero row is already collapsed by
    /// [`Tangent::from_row`], so `is_zero()` means "provably flat" rather than
    /// "happens to be spelled `Zero`" — and several rules read it that way,
    /// `pow`'s constant-exponent power rule most sharply.  [`DualEnv::bind`] is
    /// public, so the overlay is the one door a `Tangent::Scalar(vec![0.0; w])`
    /// can come through; collapsing it here keeps that equivalence true of
    /// every tangent in the module rather than of most of them.
    fn insert(&mut self, cell: ValueCellId, tangent: Tangent) {
        let tangent = match tangent {
            Tangent::Scalar(row) => Tangent::from_row(row),
            other => other,
        };
        if self.bindings.insert(cell, tangent).is_none() {
            self.generation += 1;
        }
    }

    /// The tangent bound to `cell`, or `None` when it carries no tangent.
    pub fn get(&self, cell: &ValueCellId) -> Option<&Tangent> {
        self.bindings.get(cell)
    }

    /// Why `cell` was bound as non-differentiable, when its binder recorded it.
    pub(crate) fn refusal_for(&self, cell: &ValueCellId) -> Option<&NonDifferentiable> {
        self.causes.get(cell)
    }

    /// True when no cell is bound at all.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// True when `expr` references at least one bound cell.
    ///
    /// A cell bound to [`Tangent::Zero`] counts, so this can answer `true` for
    /// a provably flat expression.  That is a missed fast path, never a wrong
    /// tangent — and it is what keeps "bindings only ever GROW" true, which is
    /// what the `carries` memo's permanent-`true` rule rests on.
    ///
    /// Uses `contains_value_ref_matching` rather than `expr.walk` for the reason
    /// [`Seeds::depends_on_seed`] states: the two traversals disagree on
    /// `Lambda`, and this predicate's `false` also gates the fast path.
    ///
    /// [`Seeds::depends_on_seed`]: crate::Seeds::depends_on_seed
    pub(crate) fn carries_tangent(&self, expr: &CompiledExpr) -> bool {
        if self.bindings.is_empty() {
            return false;
        }
        if let Some(&(generation, hit)) = self.carries.borrow().get(&expr.content_hash)
            && (hit || generation == self.generation)
        {
            return hit;
        }
        let answer = expr.contains_value_ref_matching(|id| self.bindings.contains_key(id));
        self.carries.borrow_mut().insert(expr.content_hash, (self.generation, answer));
        answer
    }
}
