//! Forward-mode dual-number evaluation of a `CompiledExpr`.
//!
//! Task #6672 (solver-unification ε).  Design reference:
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7.
//!
//! # The one rule that shapes this whole module
//!
//! **This is not a second evaluator.**  [`eval_dual`] computes *only tangents*.
//! Every primal it returns comes from the existing implementation:
//!
//! - a subtree that references no seed cell *and contains no kink* is handed
//!   wholesale to [`crate::eval_expr`] and lifted with [`Tangent::Zero`];
//! - a node that is traversed obtains its own primal from the crate-private
//!   value-semantics helpers ([`crate::eval_add`], [`crate::eval_sub`],
//!   [`crate::eval_mul`], [`crate::eval_div`], [`crate::eval_pow`],
//!   [`crate::eval_mod`], [`crate::negate_value`], [`crate::eval_eq`],
//!   [`crate::eval_ne`], [`crate::eval_cmp`]), from [`crate::kleene`], from
//!   `reify_stdlib::eval_builtin`, or from `crate::field_reductions`.
//!
//! So reify's value semantics — dimension algebra, strict-`Undef` propagation,
//! `sanitize`, the Invariant V `Scalar{DIMENSIONLESS} → Real` collapse,
//! `Point + Point` rejection — have exactly ONE implementation, and
//! `eval_dual(e).value == eval_expr(e)` holds by construction rather than by
//! diligence.  Branch *selection* and branch *value* can never disagree for the
//! same reason: both come from the same helper call.
//!
//! # The `Undef` cliff
//!
//! When the primal is `Undef`, the tangent is [`Tangent::None`], never a
//! finite row.  `Undef` means the value semantics refused to produce a number;
//! attaching a derivative to that refusal would be inventing one.

use std::cell::RefCell;
use std::collections::HashMap;

use reify_core::{ContentHash, FIELD_ENTITY_PREFIX, ValueCellId};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledMatchArm, CompiledPattern, UnOp, Value,
};

use crate::EvalContext;
use crate::branch_signature::{
    BranchChoice, BranchEntry, BranchRecord, KinkKind, KinkSite, ReductionKind,
};
use crate::dual::{DualValue, Tangent};
use crate::kleene::{self, KBool};

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
    fn note_refusal(&self, reason: NonDifferentiable) {
        let mut slot = self.refusal.borrow_mut();
        if slot.is_none() {
            *slot = Some(reason);
        }
    }

    /// Drain the recorded refusal, leaving the sink empty for the next row.
    fn take_refusal(&self) -> Option<NonDifferentiable> {
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
    pub fn depends_on_seed(&self, expr: &CompiledExpr) -> bool {
        if let Some(hit) = self.depends.borrow().get(&expr.content_hash) {
            return *hit;
        }
        let answer = expr.collect_value_refs().iter().any(|id| self.index.contains_key(id));
        self.depends.borrow_mut().insert(expr.content_hash, answer);
        answer
    }

    /// True when `expr`'s subtree contains at least one non-smooth node.
    ///
    /// This gates the seed-independence fast path.  Skipping a seed-independent
    /// subtree is sound for *tangents*, but it would also skip any kink inside
    /// it — and then "the record is empty" would mean "we did not look" rather
    /// than "there is no kink here", which is precisely the guarantee λ (#6679)
    /// builds on.
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

/// Tangents bound to cells that are **not** seed columns: a user function's or
/// lambda's parameters, and a callee's `let` bindings, while its body is being
/// evaluated.
///
/// A parameter cell is not a seed — it is an *interior* cell whose tangent was
/// computed from the seeds at the call site — so `Seeds` alone cannot resolve
/// it.  Without this overlay a function body would look seed-independent and
/// every user-defined function would differentiate to zero.
///
/// Only NON-zero tangents are stored, so "is this cell bound?" and "does this
/// cell carry a tangent?" are the same question, and an unbound lookup falls
/// through to [`Tangent::Zero`] — which is the right answer for a parameter
/// that genuinely does not move with the seeds.
#[derive(Default)]
pub struct DualEnv {
    bindings: HashMap<ValueCellId, Tangent>,
    /// Memo for [`DualEnv::carries_tangent`], invalidated on every new binding.
    /// Scoped to one body evaluation, which is exactly where it pays.
    carries: RefCell<HashMap<ContentHash, bool>>,
}

impl DualEnv {
    /// The empty overlay — the state at the residual root.
    pub fn new() -> Self {
        DualEnv::default()
    }

    /// Bind `cell` to `tangent`.  A [`Tangent::Zero`] is deliberately NOT
    /// stored: unbound already means zero.
    fn bind(&mut self, cell: ValueCellId, tangent: Tangent) {
        if tangent.is_zero() {
            return;
        }
        self.bindings.insert(cell, tangent);
        self.carries.borrow_mut().clear();
    }

    fn get(&self, cell: &ValueCellId) -> Option<&Tangent> {
        self.bindings.get(cell)
    }

    /// True when `expr` references at least one bound cell.
    fn carries_tangent(&self, expr: &CompiledExpr) -> bool {
        if self.bindings.is_empty() {
            return false;
        }
        if let Some(hit) = self.carries.borrow().get(&expr.content_hash) {
            return *hit;
        }
        let answer = expr.collect_value_refs().iter().any(|id| self.bindings.contains_key(id));
        self.carries.borrow_mut().insert(expr.content_hash, answer);
        answer
    }
}

/// Path segment marking a descent OUT of a call site and INTO the callee's
/// body.  Argument children occupy `0..arity`, so `u16::MAX` cannot collide
/// with them, and the call site's own path prefix keeps two call sites of the
/// same function distinguishable — which is what lets λ tell which call site's
/// kink moved.
const CALLEE_MARKER: u16 = u16::MAX;

/// Is THIS node (not its subtree) non-smooth?  O(1) on the node's kind.
fn node_is_kink(e: &CompiledExpr) -> bool {
    match &e.kind {
        CompiledExprKind::Conditional { .. } | CompiledExprKind::Match { .. } => true,
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
        }
        _ => false,
    }
}

/// Forward-mode dual evaluation of `expr` at the point held in `ctx`.
///
/// Returns the primal (identical to `eval_expr(expr, ctx)`) paired with
/// `∂primal/∂x_j` for each seed column `j`.  Every non-smooth node traversed on
/// the way appends one entry to `record`.
pub fn eval_dual(
    expr: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
) -> DualValue {
    seeds.take_refusal();
    let mut path: Vec<u16> = Vec::new();
    eval_dual_at(expr, ctx, seeds, &DualEnv::new(), record, &mut path)
}

/// Recursive worker.  `path` is the structural child-index path from the
/// residual root, pushed on descent and popped on return, so a kink's
/// [`KinkSite`] is simply a snapshot of it.
#[allow(clippy::too_many_arguments)]
fn eval_dual_at(
    expr: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    // Fast path: a subtree that can reach neither a seed nor a bound parameter,
    // and hides no kink, has a provably zero derivative and nothing to record —
    // so evaluate it exactly as the rest of the system would and lift it.
    if !seeds.depends_on_seed(expr) && !env.carries_tangent(expr) && !seeds.subtree_has_kink(expr) {
        return DualValue::constant(crate::eval_expr(expr, ctx));
    }

    match &expr.kind {
        CompiledExprKind::Literal(v) => DualValue::constant(v.clone()),

        CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
            let value = ctx.values.get_or_undef(id);
            match seeds.column_of(id) {
                Some(j) if value.as_f64().is_some() => {
                    let mut row = vec![0.0; seeds.width()];
                    row[j] = 1.0;
                    DualValue { value, tangent: Tangent::Scalar(row) }
                }
                // A seeded cell holding a NON-scalar (or Undef) value cannot
                // carry a scalar tangent.  Refuse loudly rather than emit a
                // zero row, which would claim the residual is flat in a
                // variable it may well depend on.
                Some(_) => {
                    seeds.note_refusal(NonDifferentiable::UnsupportedKind {
                        kind: "a seeded cell holding a non-scalar value",
                        site: KinkSite::new(path.to_vec()),
                    });
                    DualValue::opaque(value)
                }
                // Resolution order is seed column → overlay → zero.  A bound
                // parameter's tangent is already expressed in seed columns, so
                // it is adopted as-is.
                None => match env.get(id) {
                    Some(tangent) => DualValue { value, tangent: tangent.clone() },
                    None => DualValue::constant(value),
                },
            }
        }

        CompiledExprKind::UnOp { op, operand } => match op {
            UnOp::Neg => {
                let inner = descend(operand, 0, ctx, seeds, env, record, path);
                let value = crate::negate_value(inner.value.clone());
                finish_unary(value, &inner, |_| -1.0)
            }
            // `not` is Bool-valued: there is no scalar derivative to take.
            UnOp::Not => {
                refuse_or_constant(expr, ctx, seeds, env, path, "unary `not` (Bool-valued)")
            }
        },

        CompiledExprKind::BinOp { op, left, right } => {
            eval_dual_binop(*op, left, right, ctx, seeds, env, record, path)
        }

        CompiledExprKind::FunctionCall { function, args } => {
            // Dispatch on the BARE name — that is what `eval_expr` does.  The
            // only `qualified_name` comparison in the evaluator is the
            // `std::__interp_render` intercept, and that name is in neither
            // table, so it falls through to the refusal path.
            eval_dual_builtin(expr, &function.name, args, ctx, seeds, env, record, path)
        }

        CompiledExprKind::Conditional { condition, then_branch, else_branch } => {
            eval_dual_conditional(condition, then_branch, else_branch, ctx, seeds, env, record, path)
        }

        CompiledExprKind::Match { discriminant, arms } => {
            eval_dual_match(discriminant, arms, ctx, seeds, env, record, path)
        }

        CompiledExprKind::UserFunctionCall { function_name, args } => {
            eval_dual_user_fn(function_name, args, ctx, seeds, env, record, path)
        }

        // Everything else is a kind this task does not differentiate.
        _ => refuse_or_constant(expr, ctx, seeds, env, path, kind_name(expr)),
    }
}

/// Descend into child `index`, keeping the structural path in sync.
#[allow(clippy::too_many_arguments)]
fn descend(
    child: &CompiledExpr,
    index: usize,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    path.push(index as u16);
    let result = eval_dual_at(child, ctx, seeds, env, record, path);
    path.pop();
    result
}

/// Evaluate `expr` with the real evaluator and attach the only honest tangent:
/// [`Tangent::None`] when the node could move with a seed, [`Tangent::Zero`]
/// when it provably cannot.
fn refuse_or_constant(
    expr: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    path: &[u16],
    kind: &'static str,
) -> DualValue {
    let value = crate::eval_expr(expr, ctx);
    if seeds.depends_on_seed(expr) || env.carries_tangent(expr) {
        seeds.note_refusal(NonDifferentiable::UnsupportedKind {
            kind,
            site: KinkSite::new(path.to_vec()),
        });
        DualValue::opaque(value)
    } else {
        // Provably seed-independent: the derivative exists and is zero.  An
        // unsupported construct that cannot reach a seed must not poison the
        // whole residual.
        DualValue::constant(value)
    }
}

/// A human-readable name for an expression kind, for refusal messages.
fn kind_name(expr: &CompiledExpr) -> &'static str {
    match &expr.kind {
        CompiledExprKind::Literal(_) => "Literal",
        CompiledExprKind::ValueRef(_) => "ValueRef",
        CompiledExprKind::CrossSubGeometryRef(_) => "CrossSubGeometryRef",
        CompiledExprKind::BinOp { .. } => "BinOp",
        CompiledExprKind::UnOp { .. } => "UnOp",
        CompiledExprKind::FunctionCall { .. } => "FunctionCall",
        CompiledExprKind::Conditional { .. } => "Conditional",
        CompiledExprKind::Match { .. } => "Match",
        CompiledExprKind::UserFunctionCall { .. } => "UserFunctionCall",
        CompiledExprKind::Lambda { .. } => "Lambda",
        CompiledExprKind::ListLiteral(_) => "ListLiteral",
        CompiledExprKind::SetLiteral(_) => "SetLiteral",
        CompiledExprKind::MapLiteral(_) => "MapLiteral",
        CompiledExprKind::IndexAccess { .. } => "IndexAccess",
        CompiledExprKind::MethodCall { .. } => "MethodCall",
        CompiledExprKind::Quantifier { .. } => "Quantifier",
        CompiledExprKind::OptionSome(_) => "OptionSome",
        CompiledExprKind::OptionNone => "OptionNone",
        CompiledExprKind::MetaAccess { .. } => "MetaAccess",
        CompiledExprKind::DeterminacyPredicate { .. } => "DeterminacyPredicate",
        CompiledExprKind::RangeConstructor { .. } => "RangeConstructor",
        CompiledExprKind::AdHocSelector { .. } => "AdHocSelector",
        CompiledExprKind::PurposeReflectiveAggregation { .. } => "PurposeReflectiveAggregation",
        CompiledExprKind::ReflectiveCellList(_) => "ReflectiveCellList",
        CompiledExprKind::StructureInstanceCtor { .. } => "StructureInstanceCtor",
        _ => "unsupported expression kind",
    }
}

/// A human-readable name for a `Value` variant, for refusal messages.
fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Real(_) => "Real",
        Value::Scalar { .. } => "Scalar",
        Value::String(_) => "String",
        Value::Point(_) => "Point",
        Value::Vector(_) => "Vector",
        Value::Direction { .. } => "Direction",
        Value::Frame { .. } => "Frame",
        Value::Complex { .. } => "Complex",
        Value::Matrix { .. } => "Matrix",
        Value::Tensor(_) => "Tensor",
        Value::List(_) => "List",
        Value::Set(_) => "Set",
        Value::Map(_) => "Map",
        Value::Option(_) => "Option",
        Value::Enum { .. } => "Enum",
        Value::Lambda { .. } => "Lambda",
        Value::Field { .. } => "Field",
        Value::Range { .. } => "Range",
        Value::Undef => "Undef",
        _ => "non-scalar value",
    }
}

fn note(record: &mut BranchRecord, path: &[u16], kind: KinkKind, choice: BranchChoice) {
    record.push(BranchEntry { site: KinkSite::new(path.to_vec()), kind, choice });
}

/// Complete a unary node: the tangent is `df/dx · inner_tangent`, subject to
/// the `Undef` cliff.
fn finish_unary(value: Value, inner: &DualValue, dfdx: impl Fn(f64) -> f64) -> DualValue {
    if value.is_undef() {
        return DualValue::opaque(value);
    }
    match &inner.tangent {
        Tangent::Zero => DualValue::constant(value),
        Tangent::None => DualValue::opaque(value),
        Tangent::Scalar(row) => {
            let Some(x) = inner.value.as_f64() else {
                return DualValue::opaque(value);
            };
            let k = dfdx(x);
            DualValue::with_row(value, row.iter().map(|t| t * k).collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Conditional and Match — descend ONLY into the taken branch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn eval_dual_conditional(
    condition: &CompiledExpr,
    then_branch: &CompiledExpr,
    else_branch: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let cond = descend(condition, 0, ctx, seeds, env, record, path);
    // The entry is pushed BEFORE descending into the taken branch, so a flip
    // shows up as a differing `choice` at THIS site rather than as a
    // realignment of every entry that follows.  That is what makes
    // `BranchRecord::differs_from` name the kink that actually moved.
    match cond.value {
        Value::Bool(true) => {
            note(record, path, KinkKind::Conditional, BranchChoice::Then);
            descend(then_branch, 1, ctx, seeds, env, record, path)
        }
        Value::Bool(false) => {
            note(record, path, KinkKind::Conditional, BranchChoice::Else);
            descend(else_branch, 2, ctx, seeds, env, record, path)
        }
        // Undef condition, or a non-bool type error: `eval_expr` yields Undef
        // without evaluating either branch.
        _ => {
            note(record, path, KinkKind::Conditional, BranchChoice::Unresolved);
            DualValue::opaque(Value::Undef)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_dual_match(
    discriminant: &CompiledExpr,
    arms: &[CompiledMatchArm],
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let disc = descend(discriminant, 0, ctx, seeds, env, record, path);
    // §9.2.5 — a wholly-undef discriminant short-circuits before any arm is
    // tried, exactly as in `eval_expr`.
    if disc.value.is_undef() {
        note(record, path, KinkKind::Match, BranchChoice::Unresolved);
        return DualValue::opaque(Value::Undef);
    }
    let Value::Enum { variant, payload, .. } = &disc.value else {
        note(record, path, KinkKind::Match, BranchChoice::Unresolved);
        return DualValue::opaque(Value::Undef);
    };

    for (i, arm) in arms.iter().enumerate() {
        // INV-3 — arm selection is by TAG only; payload determinacy is
        // irrelevant.  `find` (not `any`) so the selecting pattern is captured.
        let Some(pattern) = arm.patterns.iter().find(|p| p.selects(variant)) else {
            continue;
        };
        note(record, path, KinkKind::Match, BranchChoice::Arm(i));
        return match pattern {
            // INV-2 — crack the payload fields into a child scope so the
            // binders are confined to exactly this arm's body, mirroring
            // `eval_variant_bind_arm`.
            CompiledPattern::VariantBind { binders, .. } => {
                let mut child = ctx.values.clone();
                for (field_name, cell) in binders {
                    let val = payload
                        .iter()
                        .find(|(f, _)| f == field_name)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Undef);
                    child.insert(cell.clone(), val);
                }
                let child_ctx = ctx.with_scope(&child);
                descend(&arm.body, 1 + i, &child_ctx, seeds, env, record, path)
            }
            CompiledPattern::Variant { .. } | CompiledPattern::Wildcard => {
                descend(&arm.body, 1 + i, ctx, seeds, env, record, path)
            }
        };
    }
    // No matching arm.
    note(record, path, KinkKind::Match, BranchChoice::Unresolved);
    DualValue::opaque(Value::Undef)
}

// ---------------------------------------------------------------------------
// Binary operators
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn eval_dual_binop(
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    match op {
        BinOp::And | BinOp::Or | BinOp::Implies => {
            return eval_dual_kleene(op, left, right, ctx, seeds, env, record, path);
        }
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            return eval_dual_comparison(op, left, right, ctx, seeds, env, record, path);
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow | BinOp::Mod => {}
    }

    let ld = descend(left, 0, ctx, seeds, env, record, path);
    let rd = descend(right, 1, ctx, seeds, env, record, path);

    // THE PRIMAL COMES FROM THE EXISTING CODE.
    let value = match op {
        BinOp::Add => crate::eval_add(&ld.value, &rd.value),
        BinOp::Sub => crate::eval_sub(&ld.value, &rd.value),
        BinOp::Mul => crate::eval_mul(&ld.value, &rd.value),
        BinOp::Div => crate::eval_div(&ld.value, &rd.value),
        BinOp::Pow => crate::eval_pow(&ld.value, &rd.value),
        BinOp::Mod => crate::eval_mod(&ld.value, &rd.value),
        _ => unreachable!("guarded above"),
    };

    // `%` is a kink at every wrap point, so it is recorded even though it is
    // differentiable in between.
    if op == BinOp::Mod {
        note(record, path, KinkKind::Mod, mod_quotient_choice(&ld.value, &rd.value));
    }

    // The Undef cliff: no derivative is attached to a refusal.
    if value.is_undef() {
        seeds.note_refusal(NonDifferentiable::UndefPrimal {
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(value);
    }
    if ld.tangent.is_none() || rd.tangent.is_none() {
        return DualValue::opaque(value);
    }
    if ld.tangent.is_zero() && rd.tangent.is_zero() {
        return DualValue::constant(value);
    }

    let width = seeds.width();
    let (Some(lrow), Some(rrow)) = (ld.tangent.materialize(width), rd.tangent.materialize(width))
    else {
        return DualValue::opaque(value);
    };
    // Join the three numeric scalar variants (`Int`, `Real`, `Scalar`) through
    // `as_f64` — NOT by matching `Value::Scalar`, which `from_real_scalar`
    // collapses to `Real` whenever the dimension cancels (Invariant V).
    let (Some(a), Some(b)) = (ld.value.as_f64(), rd.value.as_f64()) else {
        return DualValue::opaque(value);
    };

    let row: Vec<f64> = (0..width)
        .map(|j| {
            let (ap, bp) = (lrow[j], rrow[j]);
            match op {
                BinOp::Add => ap + bp,
                BinOp::Sub => ap - bp,
                BinOp::Mul => ap * b + a * bp,
                BinOp::Div => (ap * b - a * bp) / (b * b),
                BinOp::Pow => pow_tangent(a, b, ap, bp, rd.tangent.is_zero()),
                BinOp::Mod => ap - (a / b).trunc() * bp,
                _ => unreachable!("guarded above"),
            }
        })
        .collect();

    DualValue::with_row(value, row)
}

/// The quotient cell `a % b` currently sits in.
///
/// **`trunc`, not `floor`.** Reify's `%` is Rust's *truncated* remainder
/// (`a % b == a − trunc(a/b)·b`), so the truncated quotient is the coefficient
/// that actually appears in `∂(a % b)/∂b = −trunc(a/b)`.  Labelling the cell
/// with `floor` instead would disagree with the tangent — and mislabel the
/// branch — for every negative operand.
fn mod_quotient_choice(lv: &Value, rv: &Value) -> BranchChoice {
    match (lv.as_f64(), rv.as_f64()) {
        (Some(a), Some(b)) if b != 0.0 && (a / b).trunc().is_finite() => {
            BranchChoice::ModQuotient((a / b).trunc() as i64)
        }
        _ => BranchChoice::Unresolved,
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_dual_comparison(
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let ld = descend(left, 0, ctx, seeds, env, record, path);
    let rd = descend(right, 1, ctx, seeds, env, record, path);
    let value = match op {
        BinOp::Eq => crate::eval_eq(&ld.value, &rd.value),
        BinOp::Ne => crate::eval_ne(&ld.value, &rd.value),
        BinOp::Lt => crate::eval_cmp(&ld.value, &rd.value, |a, b| a < b),
        BinOp::Le => crate::eval_cmp(&ld.value, &rd.value, |a, b| a <= b),
        BinOp::Gt => crate::eval_cmp(&ld.value, &rd.value, |a, b| a > b),
        BinOp::Ge => crate::eval_cmp(&ld.value, &rd.value, |a, b| a >= b),
        _ => unreachable!("guarded by the caller"),
    };
    let choice = match value {
        Value::Bool(true) => BranchChoice::Satisfied,
        Value::Bool(false) => BranchChoice::Unsatisfied,
        _ => BranchChoice::Unresolved,
    };
    note(record, path, KinkKind::Comparison(op), choice);
    // A Bool has no scalar derivative.  Reporting `Tangent::Zero` here would
    // let a comparison masquerade as a flat numeric residual.
    DualValue::opaque(value)
}

#[allow(clippy::too_many_arguments)]
fn eval_dual_kleene(
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let ld = descend(left, 0, ctx, seeds, env, record, path);
    let Ok(lk) = KBool::try_from(&ld.value) else {
        // Short-circuit on type error: the right operand is never evaluated.
        note(record, path, KinkKind::Kleene(op), BranchChoice::LeftTypeError);
        return DualValue::opaque(Value::Undef);
    };
    // Short-circuit on the absorbing element, matching `eval_and`/`eval_or`/
    // `eval_implies` exactly.
    let absorbed = match op {
        BinOp::And => matches!(lk, KBool::False).then_some(Value::Bool(false)),
        BinOp::Or => matches!(lk, KBool::True).then_some(Value::Bool(true)),
        // `False ⇒ anything = True`, vacuously.
        BinOp::Implies => matches!(lk, KBool::False).then_some(Value::Bool(true)),
        _ => unreachable!("guarded by the caller"),
    };
    if let Some(value) = absorbed {
        note(record, path, KinkKind::Kleene(op), BranchChoice::LeftAbsorbing);
        return DualValue::opaque(value);
    }

    // Pushed before descending into the right operand, so a flip between
    // `LeftAbsorbing` and `BothEvaluated` is visible AT this site rather than
    // as a shift of every entry the right subtree contributes.
    note(record, path, KinkKind::Kleene(op), BranchChoice::BothEvaluated);
    let rd = descend(right, 1, ctx, seeds, env, record, path);
    let Ok(rk) = KBool::try_from(&rd.value) else {
        return DualValue::opaque(Value::Undef);
    };
    let folded = match op {
        BinOp::And => kleene::kleene_and(lk, rk),
        BinOp::Or => kleene::kleene_or(lk, rk),
        BinOp::Implies => kleene::kleene_implies(lk, rk),
        _ => unreachable!("guarded by the caller"),
    };
    DualValue::opaque(folded.into())
}

/// One column of `d(a^b)`.
///
/// With a *constant* exponent the identity `b·a^(b−1)·a'` is used, which stays
/// valid for a NEGATIVE base at integral `b` — the common case in reify, where
/// `Scalar ^ Int` is the only dimensioned power the value layer accepts.  Only
/// a seed-dependent exponent needs the general form, which requires `ln a` and
/// is therefore restricted to a positive base.
fn pow_tangent(a: f64, b: f64, ap: f64, bp: f64, exponent_is_constant: bool) -> f64 {
    if exponent_is_constant {
        if b == 0.0 {
            // Exactly zero, rather than 0·a^(−1)·a' (NaN at a == 0).
            return 0.0;
        }
        return b * a.powf(b - 1.0) * ap;
    }
    a.powf(b) * (bp * a.ln() + b * ap / a)
}

// ---------------------------------------------------------------------------
// Builtin calls
// ---------------------------------------------------------------------------

/// A `FunctionCall` node.
///
/// The PRIMAL comes from `reify_stdlib::eval_builtin(name, &primal_args)` —
/// the exact call `eval_expr` makes — so every builtin's dimension handling
/// (`sqrt` → `dimension.root(2)`, `pow` → a bare `Real`, `asin`/`acos`/`atan`/
/// `atan2` → `DimensionVector::ANGLE`) is INHERITED rather than restated here.
/// Field reductions likewise come from `crate::field_reductions`, the same
/// helpers `eval_expr` intercepts with.
#[allow(clippy::too_many_arguments)]
fn eval_dual_builtin(
    expr: &CompiledExpr,
    name: &str,
    args: &[CompiledExpr],
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    // Arguments are evaluated FIRST, unconditionally, matching `eval_expr`'s
    // own order — and so that a kink inside an argument is recorded even when
    // the call itself is a name this task does not differentiate.  "The record
    // is what was traversed" has to hold for arguments too.
    let duals: Vec<DualValue> = args
        .iter()
        .enumerate()
        .map(|(i, a)| descend(a, i, ctx, seeds, env, record, path))
        .collect();
    let primal_args: Vec<Value> = duals.iter().map(|d| d.value.clone()).collect();

    // Strict `Undef` propagation, matching `eval_expr`'s short-circuit — which
    // sits BEFORE the field-shadow lookup below, so this ordering is load-bearing.
    if primal_args.iter().any(|v| v.is_undef()) {
        seeds.note_refusal(NonDifferentiable::UndefPrimal {
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(Value::Undef);
    }

    // `eval_expr` lets a `Value::Field` cell named `__field__::<name>` shadow a
    // builtin of the same name, applying its lambda to the single argument.
    // That is the reachable scalar-in scalar-out lambda-application route in a
    // residual, so it is differentiated rather than refused.
    if args.len() == 1
        && let Value::Field { lambda, .. } =
            ctx.values.get_or_undef(&ValueCellId::new(FIELD_ENTITY_PREFIX, name))
    {
        return eval_dual_lambda_apply(&lambda, &duals[0], ctx, seeds, record, path);
    }

    // Field reductions are intercepted by `eval_expr` BEFORE `eval_builtin`,
    // so they must be intercepted here too or the primal invariant breaks.
    if let Some(kind) = field_reduction_kind(name, args.len(), &primal_args[0]) {
        return eval_field_reduction(kind, &primal_args[0], &duals[0], seeds, record, path);
    }

    let smooth = is_differentiable_builtin(name, args.len());
    let kinky = is_kink_builtin(name, args.len());
    // Names in neither table (geometry, list, matrix, field ops) are not
    // differentiated.  The node's only children are its arguments, so "does it
    // move with a seed?" is exactly "does any argument tangent survive?".
    if !smooth && !kinky {
        let value = crate::eval_expr(expr, ctx);
        return if duals.iter().all(|d| d.tangent.is_zero()) {
            DualValue::constant(value)
        } else {
            seeds.note_refusal(NonDifferentiable::UnsupportedKind {
                kind: "a builtin with no derivative rule",
                site: KinkSite::new(path.to_vec()),
            });
            DualValue::opaque(value)
        };
    }

    let value = reify_stdlib::eval_builtin(name, &primal_args);
    if kinky {
        return eval_kink_builtin(name, value, &duals, &primal_args, seeds, record, path);
    }

    if value.is_undef() {
        seeds.note_refusal(NonDifferentiable::UndefPrimal {
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(value);
    }
    if duals.iter().any(|d| d.tangent.is_none()) {
        return DualValue::opaque(value);
    }
    if duals.iter().all(|d| d.tangent.is_zero()) {
        return DualValue::constant(value);
    }
    let Some(xs) = primal_args.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>() else {
        return DualValue::opaque(value);
    };
    let Some(partials) = builtin_partials(name, &xs) else {
        return DualValue::opaque(value);
    };
    // A non-finite local derivative — `sqrt(0)`, `log(0)`, `asin(±1)` — is a
    // refusal, never a NaN or Inf smuggled into a Jacobian.
    if partials.iter().any(|p| !p.is_finite()) {
        seeds.note_refusal(NonDifferentiable::UnsupportedKind {
            kind: "a builtin evaluated where its derivative does not exist",
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(value);
    }
    combine(value, &duals, &partials, seeds.width())
}

/// Chain rule over a multi-argument builtin: `Σ_i (∂f/∂arg_i)·arg_i'`.
fn combine(value: Value, duals: &[DualValue], partials: &[f64], width: usize) -> DualValue {
    let mut row = vec![0.0; width];
    for (i, d) in duals.iter().enumerate() {
        if d.tangent.is_zero() {
            continue;
        }
        let Some(t) = d.tangent.materialize(width) else {
            return DualValue::opaque(value);
        };
        for (j, slot) in row.iter_mut().enumerate() {
            *slot += partials[i] * t[j];
        }
    }
    DualValue::with_row(value, row)
}

/// The non-smooth builtins.  `min`/`max` appear at BOTH arities: binary numeric
/// selection, and the 1-argument field reduction.
fn is_kink_builtin(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("min" | "max", 2)
            | ("abs" | "sign" | "floor" | "ceil" | "round", 1)
            | ("clamp", 3)
            | ("mod", 2)
            | ("max" | "min" | "argmax" | "argmin", 1)
    )
}

fn field_reduction_kind(name: &str, arity: usize, first: &Value) -> Option<ReductionKind> {
    if arity != 1 || !matches!(first, Value::Field { .. }) {
        return None;
    }
    match name {
        "max" => Some(ReductionKind::Max),
        "min" => Some(ReductionKind::Min),
        "argmax" => Some(ReductionKind::ArgMax),
        "argmin" => Some(ReductionKind::ArgMin),
        _ => None,
    }
}

/// A field reduction: record which grid point won, and refuse a tangent when
/// the field itself moves with the seeds.
///
/// This task does not differentiate *through* a field — PRD §7.7 splits
/// derivatives three ways, and geometry/field derivatives are the ANALYTIC
/// source, not the AD one.  A seed-independent field is genuinely flat
/// (`Tangent::Zero`); a seed-dependent one gets `Tangent::None`, because a zero
/// row would tell the solver the residual is flat in a variable it really does
/// depend on.
fn eval_field_reduction(
    kind: ReductionKind,
    field: &Value,
    field_dual: &DualValue,
    seeds: &Seeds,
    record: &mut BranchRecord,
    path: &[u16],
) -> DualValue {
    let (value, arg) = match kind {
        ReductionKind::Max => {
            (crate::field_reductions::compute_max(field), crate::field_reductions::compute_argmax(field))
        }
        ReductionKind::Min => {
            (crate::field_reductions::compute_min(field), crate::field_reductions::compute_argmin(field))
        }
        ReductionKind::ArgMax => {
            let a = crate::field_reductions::compute_argmax(field);
            (a.clone(), a)
        }
        ReductionKind::ArgMin => {
            let a = crate::field_reductions::compute_argmin(field);
            (a.clone(), a)
        }
    };
    // `Unresolved` when the argextremum is not computable — e.g. an unbounded
    // Analytical field, where `compute_argmax` returns `Undef`.
    let choice = if arg.is_undef() {
        BranchChoice::Unresolved
    } else {
        BranchChoice::FieldArgExtremum(Box::new(arg))
    };
    note(record, path, KinkKind::FieldReduction(kind), choice);

    if field_dual.tangent.is_zero() {
        DualValue::constant(value)
    } else {
        seeds.note_refusal(NonDifferentiable::UnsupportedKind {
            kind: "a field reduction over a seed-dependent field",
            site: KinkSite::new(path.to_vec()),
        });
        DualValue::opaque(value)
    }
}

/// The non-smooth builtins: record the active branch, then take that branch's
/// derivative.
#[allow(clippy::too_many_arguments)]
fn eval_kink_builtin(
    name: &str,
    value: Value,
    duals: &[DualValue],
    xs_values: &[Value],
    seeds: &Seeds,
    record: &mut BranchRecord,
    path: &[u16],
) -> DualValue {
    let xs: Option<Vec<f64>> = xs_values.iter().map(|v| v.as_f64()).collect();
    let width = seeds.width();

    match (name, duals.len()) {
        ("min" | "max", 2) => {
            let kind = if name == "min" { KinkKind::Min } else { KinkKind::Max };
            let Some(xs) = xs else {
                note(record, path, kind, BranchChoice::Unresolved);
                return DualValue::opaque(value);
            };
            // A tie goes to operand 0 — deterministically, and RECORDED, so λ
            // sees the flip when the tie later breaks the other way.  An
            // unrecorded arbitrary tie-break is exactly what makes a solver
            // chatter.
            let take_left =
                if name == "min" { xs[0] <= xs[1] } else { xs[0] >= xs[1] };
            let i = if take_left { 0 } else { 1 };
            note(record, path, kind, BranchChoice::Operand(i));
            select(value, &duals[i])
        }
        ("abs", 1) => {
            let Some(xs) = xs else {
                note(record, path, KinkKind::Abs, BranchChoice::Unresolved);
                return DualValue::opaque(value);
            };
            let (choice, dfdx) = if xs[0] < 0.0 {
                (BranchChoice::Negative, Some(-1.0))
            } else if xs[0] > 0.0 {
                (BranchChoice::Positive, Some(1.0))
            } else {
                // |x| has no derivative at 0.  `signum()` would confidently
                // return +1, passing a one-sided derivative off as two-sided.
                (BranchChoice::Zero, None)
            };
            note(record, path, KinkKind::Abs, choice);
            match dfdx {
                Some(k) => combine(value, duals, &[k], width),
                None => {
                    seeds.note_refusal(NonDifferentiable::UnsupportedKind {
                        kind: "`abs` evaluated exactly at its kink (x = 0)",
                        site: KinkSite::new(path.to_vec()),
                    });
                    DualValue::opaque(value)
                }
            }
        }
        ("clamp", 3) => {
            let Some(xs) = xs else {
                note(record, path, KinkKind::Clamp, BranchChoice::Unresolved);
                return DualValue::opaque(value);
            };
            let (choice, i) = if xs[0] < xs[1] {
                (BranchChoice::BelowLo, 1)
            } else if xs[0] > xs[2] {
                (BranchChoice::AboveHi, 2)
            } else {
                (BranchChoice::Interior, 0)
            };
            note(record, path, KinkKind::Clamp, choice);
            // Outside the interior the result IS the bound, so the derivative
            // is the bound's — zero for the usual constant bounds, but a real
            // tangent when the bound is itself a function of the seeds.
            select(value, &duals[i])
        }
        ("sign" | "floor" | "ceil" | "round", 1) => {
            let kind = match name {
                "sign" => KinkKind::Sign,
                "floor" => KinkKind::Floor,
                "ceil" => KinkKind::Ceil,
                _ => KinkKind::Round,
            };
            let choice = match value.as_f64() {
                Some(v) if v.is_finite() => BranchChoice::IntegerCell(v as i64),
                _ => BranchChoice::Unresolved,
            };
            note(record, path, kind, choice);
            if value.is_undef() {
                return DualValue::opaque(value);
            }
            // Piecewise constant: the derivative is exactly zero almost
            // everywhere.  That is a genuine `Tangent::Zero`, not a refusal.
            DualValue::constant(value)
        }
        ("mod", 2) => {
            note(record, path, KinkKind::Mod, mod_quotient_choice(&xs_values[0], &xs_values[1]));
            let Some(xs) = xs else {
                return DualValue::opaque(value);
            };
            if value.is_undef() || xs[1] == 0.0 {
                return DualValue::opaque(value);
            }
            combine(value, duals, &[1.0, -(xs[0] / xs[1]).trunc()], width)
        }
        _ => DualValue::opaque(value),
    }
}

/// The result of a kink IS one of its operands: adopt that operand's tangent
/// wholesale rather than recomputing it.
fn select(value: Value, chosen: &DualValue) -> DualValue {
    if value.is_undef() {
        return DualValue::opaque(value);
    }
    DualValue { value, tangent: chosen.tangent.clone() }
}

/// True when `name` at this arity has an entry in [`builtin_partials`].
fn is_differentiable_builtin(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("sqrt" | "exp" | "log" | "log10", 1)
            | ("sin" | "cos" | "tan" | "asin" | "acos" | "atan", 1)
            | ("sinh" | "cosh" | "tanh", 1)
            | ("atan2" | "pow", 2)
            | ("lerp", 3)
            | ("remap", 5)
    )
}

/// The local partial derivatives `∂f/∂arg_i` of a smooth builtin at `xs`.
///
/// `None` when the name/arity has no entry.  A derivative that does not exist
/// at this point is returned as `NaN`, which the caller's finiteness guard
/// turns into `Tangent::None`.
fn builtin_partials(name: &str, xs: &[f64]) -> Option<Vec<f64>> {
    Some(match (name, xs.len()) {
        ("sqrt", 1) => vec![0.5 / xs[0].sqrt()],
        ("exp", 1) => vec![xs[0].exp()],
        // `log` IS the natural logarithm — there is no `ln` binding.
        ("log", 1) => vec![1.0 / xs[0]],
        ("log10", 1) => vec![1.0 / (xs[0] * std::f64::consts::LN_10)],
        ("sin", 1) => vec![xs[0].cos()],
        ("cos", 1) => vec![-xs[0].sin()],
        ("tan", 1) => {
            let c = xs[0].cos();
            vec![1.0 / (c * c)]
        }
        ("asin", 1) => vec![1.0 / (1.0 - xs[0] * xs[0]).sqrt()],
        ("acos", 1) => vec![-1.0 / (1.0 - xs[0] * xs[0]).sqrt()],
        ("atan", 1) => vec![1.0 / (1.0 + xs[0] * xs[0])],
        ("sinh", 1) => vec![xs[0].cosh()],
        ("cosh", 1) => vec![xs[0].sinh()],
        ("tanh", 1) => {
            let t = xs[0].tanh();
            vec![1.0 - t * t]
        }
        // NOTE the argument order: atan2(y, x).
        ("atan2", 2) => {
            let (y, x) = (xs[0], xs[1]);
            let d = x * x + y * y;
            vec![x / d, -y / d]
        }
        ("pow", 2) => {
            let (x, y) = (xs[0], xs[1]);
            vec![y * x.powf(y - 1.0), x.powf(y) * x.ln()]
        }
        // lerp(a, b, t) = a + t(b − a)
        ("lerp", 3) => {
            let (a, b, t) = (xs[0], xs[1], xs[2]);
            vec![1.0 - t, t, b - a]
        }
        // remap(x, flo, fhi, tlo, thi) = tlo + (x − flo)·(thi − tlo)/(fhi − flo)
        // With s = fhi − flo, d = thi − tlo, u = (x − flo)/s and k = d/s:
        //   ∂/∂x = k, ∂/∂flo = −k + u·k, ∂/∂fhi = −u·k,
        //   ∂/∂tlo = 1 − u, ∂/∂thi = u
        ("remap", 5) => {
            let (x, flo, fhi, tlo, thi) = (xs[0], xs[1], xs[2], xs[3], xs[4]);
            let s = fhi - flo;
            let k = (thi - tlo) / s;
            let u = (x - flo) / s;
            vec![k, -k + u * k, -u * k, 1.0 - u, u]
        }
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// User-defined functions and lambdas — "arbitrary user algebra"
// ---------------------------------------------------------------------------

/// A `UserFunctionCall` node, mirroring `eval_user_function_call`
/// (`reify-expr/src/lib.rs:1946`) step for step on the value path so the primal
/// invariant survives overload selection and the `@optimized` hook.
#[allow(clippy::too_many_arguments)]
fn eval_dual_user_fn(
    function_name: &str,
    args: &[CompiledExpr],
    ctx: &EvalContext,
    seeds: &Seeds,
    env: &DualEnv,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let duals: Vec<DualValue> = args
        .iter()
        .enumerate()
        .map(|(i, a)| descend(a, i, ctx, seeds, env, record, path))
        .collect();

    // Strict `Undef` propagation.
    if duals.iter().any(|d| d.value.is_undef()) {
        seeds.note_refusal(NonDifferentiable::UndefPrimal {
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(Value::Undef);
    }
    // The SAME recursion guard the evaluator uses.  Reaching it means the
    // primal is `Undef`, so there is nothing to differentiate — and stopping
    // here is what keeps the dual traversal off the stack cliff.
    if ctx.recursion_depth >= crate::MAX_RECURSION_DEPTH {
        seeds.note_refusal(NonDifferentiable::UnsupportedKind {
            kind: "user-function recursion deeper than MAX_RECURSION_DEPTH",
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(Value::Undef);
    }
    // Overload selection by name, arity and param types, exactly as the
    // evaluator resolves it — a different overload would be a different
    // function and therefore a different derivative.
    let Some(func) = crate::find_matching_compiled_function(ctx.functions, function_name, args)
    else {
        seeds.note_refusal(NonDifferentiable::UndefPrimal {
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(Value::Undef);
    };

    let primal_args: Vec<Value> = duals.iter().map(|d| d.value.clone()).collect();
    if let Some(value) = crate::try_compute_dispatch(func, &primal_args, ctx) {
        // An `@optimized` compute node is opaque to ε: its body was never
        // traversed, so there is no chain rule to apply and no honest tangent
        // to report.
        return if duals.iter().all(|d| d.tangent.is_zero()) {
            DualValue::constant(value)
        } else {
            seeds.note_refusal(NonDifferentiable::UnsupportedKind {
                kind: "an `@optimized` compute-dispatched function",
                site: KinkSite::new(path.to_vec()),
            });
            DualValue::opaque(value)
        };
    }
    eval_dual_fn_body(func, &duals, ctx, seeds, record, path)
}

/// Build the callee's scope — values AND tangents in parallel — and evaluate
/// its body.
///
/// `#[inline(never)]`, and the `ValueMap`/`DualEnv` locals live here rather
/// than in `eval_dual_user_fn`, for the same reason
/// `eval_compiled_function_with_values` and `eval_variant_bind_arm` are
/// extracted in `lib.rs`: this sits on the recursive user-function chain, whose
/// per-frame size is budgeted against a fixed stack at `MAX_RECURSION_DEPTH`
/// (256) levels.  Pinned by
/// `unbounded_user_function_recursion_yields_undef_and_no_tangent_rather_than_a_stack_overflow`.
#[inline(never)]
fn eval_dual_fn_body(
    func: &reify_ir::CompiledFunction,
    duals: &[DualValue],
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let mut scope = reify_ir::ValueMap::new();
    let mut body_env = DualEnv::new();
    for ((param_name, _), dual) in func.params.iter().zip(duals.iter()) {
        let cell = ValueCellId::new(&func.name, param_name);
        scope.insert(cell.clone(), dual.value.clone());
        body_env.bind(cell, dual.tangent.clone());
    }

    // Everything below this marker is inside the callee, so its sites cannot
    // collide with the call site's own argument indices — nor with those of a
    // second call site of the same function, which carries a different prefix.
    path.push(CALLEE_MARKER);
    for (i, (binding_name, binding_expr)) in func.body.let_bindings.iter().enumerate() {
        let bound = {
            let body_ctx = ctx.with_scope(&scope);
            path.push(i as u16);
            let d = eval_dual_at(binding_expr, &body_ctx, seeds, &body_env, record, path);
            path.pop();
            d
        };
        let cell = ValueCellId::new(&func.name, binding_name);
        scope.insert(cell.clone(), bound.value.clone());
        body_env.bind(cell, bound.tangent);
    }

    path.push(func.body.let_bindings.len() as u16);
    let result = {
        let body_ctx = ctx.with_scope(&scope);
        eval_dual_at(&func.body.result_expr, &body_ctx, seeds, &body_env, record, path)
    };
    path.pop();
    path.pop();
    result
}

/// Apply a lambda to one already-dual argument, propagating its tangent
/// through the body.
///
/// Mirrors `apply_lambda` / `apply_lambda_with_point_unpacking`: the same depth
/// guard, the same arity rule, the same captures-plus-params scope.
#[inline(never)]
fn eval_dual_lambda_apply(
    lambda: &Value,
    arg: &DualValue,
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
    path: &mut Vec<u16>,
) -> DualValue {
    let Value::Lambda { params, body, captures } = lambda else {
        seeds.note_refusal(NonDifferentiable::UndefPrimal {
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(Value::Undef);
    };
    if ctx.recursion_depth >= crate::MAX_RECURSION_DEPTH {
        seeds.note_refusal(NonDifferentiable::UnsupportedKind {
            kind: "lambda application deeper than MAX_RECURSION_DEPTH",
            site: KinkSite::new(path.to_vec()),
        });
        return DualValue::opaque(Value::Undef);
    }
    // A multi-param lambda is applied by UNPACKING a Point/Vector argument into
    // its components.  ε carries structured values primal-only (PRD §7.7 makes
    // geometry derivatives the analytic source, not the AD one), so the primal
    // still comes from the evaluator and the tangent is a loud refusal.
    if params.len() != 1 {
        let value = crate::apply_lambda_with_point_unpacking(lambda, &arg.value, ctx);
        return if arg.tangent.is_zero() {
            DualValue::constant(value)
        } else {
            seeds.note_refusal(NonDifferentiable::UnsupportedKind {
                kind: "a multi-parameter lambda applied by Point/Vector unpacking",
                site: KinkSite::new(path.to_vec()),
            });
            DualValue::opaque(value)
        };
    }

    let mut scope = captures.clone();
    let mut body_env = DualEnv::new();
    let (_, cell) = &params[0];
    scope.insert(cell.clone(), arg.value.clone());
    body_env.bind(cell.clone(), arg.tangent.clone());

    path.push(CALLEE_MARKER);
    path.push(0);
    let result = {
        let body_ctx = ctx.with_scope(&scope);
        eval_dual_at(body, &body_ctx, seeds, &body_env, record, path)
    };
    path.pop();
    path.pop();
    result
}

// ---------------------------------------------------------------------------
// The Jacobian-row boundary
// ---------------------------------------------------------------------------

/// Why a residual has no usable gradient row.
///
/// Every variant names the construct responsible, because η (#6675) turns this
/// into a tier-2 refusal the user reads WITHOUT the surrounding code.
///
/// This type exists so that "the derivative is zero" and "we could not compute
/// the derivative" can never be confused.  Returning a zero row for the second
/// case is a *claim* — that the residual does not move when the variable moves
/// — and it is false in the most damaging possible way: the solver believes it
/// is already stationary in that direction and stops pushing.  PRD §7.7
/// ("non-numeric operands stop contributing phantom gradients") and INV-SF-7
/// ("a well-typed wrong value is the worst shape") are the same rule seen from
/// two sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonDifferentiable {
    /// The residual evaluated to `Undef` — the value semantics refused to
    /// produce a number, so there is no function to differentiate.
    UndefPrimal { site: KinkSite },
    /// A seed-dependent subtree reached a construct with no derivative rule.
    UnsupportedKind { kind: &'static str, site: KinkSite },
    /// The residual is not a scalar, so it has no gradient ROW at all.
    NonScalarResult { got: &'static str },
    /// A tangent component came out NaN or ±Inf.  The primal may look perfectly
    /// ordinary, which is exactly why this is checked rather than assumed.
    NonFiniteTangent { column: usize },
}

impl std::fmt::Display for NonDifferentiable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NonDifferentiable::UndefPrimal { site } => write!(
                f,
                "residual evaluates to undef at expression path {:?}, so it has no derivative",
                site.path()
            ),
            NonDifferentiable::UnsupportedKind { kind, site } => write!(
                f,
                "no derivative rule for {kind} at expression path {:?}; \
                 the residual depends on a solved variable through it",
                site.path()
            ),
            NonDifferentiable::NonScalarResult { got } => write!(
                f,
                "residual must be a scalar to have a gradient row, but it evaluated to {got}"
            ),
            NonDifferentiable::NonFiniteTangent { column } => write!(
                f,
                "derivative with respect to variable {column} is not finite \
                 (NaN or infinite), so it cannot enter a linear solve"
            ),
        }
    }
}

impl std::error::Error for NonDifferentiable {}

/// One row of a Jacobian: the residual's SI value and `∂r/∂x_j` for every seed
/// column, or a typed refusal explaining why there is none.
///
/// This is the boundary η (#6675) and μ (#6680) call.  Column `j` corresponds
/// to `seeds.columns()[j]`; the row is always exactly `seeds.width()` wide, so
/// a caller never has to reason about a short row.
pub fn jacobian_row(
    expr: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
) -> Result<(f64, Vec<f64>), NonDifferentiable> {
    let dual = eval_dual(expr, ctx, seeds, record);

    // The primal is checked FIRST, so the message names the residual's own
    // problem rather than a downstream consequence of it.
    if dual.value.is_undef() {
        return Err(seeds
            .take_refusal()
            .unwrap_or(NonDifferentiable::UndefPrimal { site: KinkSite::root() }));
    }
    let Some(primal) = dual.value.as_f64() else {
        return Err(NonDifferentiable::NonScalarResult { got: value_kind_name(&dual.value) });
    };

    let width = seeds.width();
    let Some(row) = dual.tangent.materialize(width) else {
        // `Tangent::None` reached the root.  The traversal recorded WHY, and
        // that reason is strictly more useful than the fact itself.
        return Err(seeds.take_refusal().unwrap_or(NonDifferentiable::UnsupportedKind {
            kind: "an expression with no derivative rule",
            site: KinkSite::root(),
        }));
    };
    if let Some(column) = row.iter().position(|t| !t.is_finite()) {
        return Err(NonDifferentiable::NonFiniteTangent { column });
    }
    Ok((primal, row))
}
