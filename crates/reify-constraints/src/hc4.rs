//! HC4 box consistency for compiled constraint leaves (task #6655).
//!
//! A [`Relation`] is one comparison leaf compiled into a flattened expression
//! tree over a box of variable intervals. [`Relation::revise`] runs HC4-revise:
//! a forward pass encloses every node, the comparison narrows the two sides,
//! and a backward pass projects that narrowing onto every operand through the
//! relational inverses in [`crate::interval`], down to the variables' box.
//!
//! Every step is sound (C8): it removes only points at which the relation
//! cannot hold, so an EMPTY result proves the relation unsatisfiable on the
//! box, while a non-empty box is no evidence that it is satisfiable.
//! An unsupported sub-expression is opaque: the whole real line, with no
//! backward step.

use reify_core::scalar_tolerance::equality_band;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, UnOp, dimension_of};

use crate::interval::Interval;

/// What a value-cell reference is to the box being contracted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefClass {
    /// A box variable, by its index in the box.
    Variable(usize),
    /// Moves with the solve but is not a box variable, so it bounds nothing.
    Varying,
    /// Constant over the box; evaluable.
    Fixed,
}

/// The caller's knowledge of the operands a [`Relation`] is compiled from.
pub trait OperandClassifier {
    fn classify(&self, id: &ValueCellId) -> RefClass;

    /// The finite SI value of an expression whose refs are all
    /// [`RefClass::Fixed`], or `None`.
    fn evaluate(&self, expr: &CompiledExpr) -> Option<f64>;
}

/// How far a comparison is relaxed before it narrows the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slack {
    /// The comparison as written (strictness aside: `>`/`<` narrow as their
    /// non-strict closures).
    Exact,
    /// The solver's own acceptance: an order relation relaxed by the absolute
    /// `inequality` threshold, and `==` by
    /// [`reify_core::scalar_tolerance::equality_band`] at the relation's
    /// dimension.
    Acceptance { inequality: f64 },
}

/// The outcome of one [`Relation::revise`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Revision {
    Consistent,
    /// No point of the box satisfies the relation.
    Empty,
}

/// The outcome of [`contract`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contraction {
    /// The box is narrowed as far as the sweep budget allowed. This is NOT
    /// evidence that any point of it satisfies the relations.
    Consistent,
    /// No point of the input box satisfies every relation. `relation` indexes
    /// the input slice: the first relation, in input order, whose revise
    /// emptied the box.
    Empty { relation: usize },
}

/// A sweep that moves no endpoint by more than this fraction of its
/// interval's previous width (of its magnitude, for a half-unbounded
/// interval) ends the fixpoint. An infinite endpoint becoming finite always
/// counts.
const HC4_MIN_RELATIVE_SHRINK: f64 = 1e-4;

/// The sweep budget. A count, never a clock (solution-set-completeness C7);
/// exhausting it returns the still-sound box as [`Contraction::Consistent`].
const HC4_MAX_SWEEPS: usize = 64;

/// Contract `bx` against every relation to an approximate HC4 fixpoint:
/// AC3-style sweeps over `relations` in input order until a sweep stops
/// shrinking the box or the budget runs out. Pure and deterministic.
///
/// It only ever narrows the box it is given, so it runs unchanged on any
/// caller-supplied sub-box.
pub fn contract(relations: &[Relation], bx: &mut [Interval], slack: Slack) -> Contraction {
    for _ in 0..HC4_MAX_SWEEPS {
        let before = bx.to_vec();
        for (index, relation) in relations.iter().enumerate() {
            if relation.revise(bx, slack) == Revision::Empty {
                return Contraction::Empty { relation: index };
            }
        }
        if !shrank_materially(&before, bx) {
            break;
        }
    }
    Contraction::Consistent
}

fn shrank_materially(before: &[Interval], after: &[Interval]) -> bool {
    before.iter().zip(after).any(|(old, new)| {
        let width = old.hi() - old.lo();
        let scale = |endpoint: f64| {
            if width.is_finite() {
                width
            } else {
                endpoint.abs()
            }
        };
        endpoint_moved(old.lo(), new.lo(), scale(old.lo()))
            || endpoint_moved(old.hi(), new.hi(), scale(old.hi()))
    })
}

fn endpoint_moved(old: f64, new: f64, scale: f64) -> bool {
    if old.is_infinite() {
        return new.is_finite();
    }
    (new - old).abs() > HC4_MIN_RELATIVE_SHRINK * scale
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comparison {
    AtLeast,
    AtMost,
    Equal,
}

/// One node of a flattened expression tree; operands are indices of earlier
/// nodes, so the vector is in post-order.
#[derive(Clone, Copy, Debug)]
enum Node {
    Const(Interval),
    Var(usize),
    Opaque,
    Add(usize, usize),
    Sub(usize, usize),
    Mul(usize, usize),
    Div(usize, usize),
    Neg(usize),
    Powi(usize, u32),
    Sqrt(usize),
    Abs(usize),
    Min(usize, usize),
    Max(usize, usize),
    /// A conditional's branches, enclosed forward only: the untaken branch
    /// must never be constrained.
    Hull(usize, usize),
}

/// Integer exponents above this are [`Node::Opaque`]: the directed power is a
/// chain of that many products.
const MAX_INTEGER_EXPONENT: u32 = 64;

/// One compiled comparison leaf. Owns its tree; borrows nothing.
#[derive(Clone, Debug)]
pub struct Relation {
    nodes: Vec<Node>,
    lhs: usize,
    rhs: usize,
    comparison: Comparison,
    strict: bool,
    dimension: DimensionVector,
}

impl Relation {
    /// Compile a `>=`/`>`/`<=`/`<`/`==` leaf with numeric operands; `None` for
    /// any other leaf, and for an `==` whose operand dimensions differ.
    pub fn compile(leaf: &CompiledExpr, classifier: &impl OperandClassifier) -> Option<Relation> {
        let CompiledExprKind::BinOp { op, left, right } = &leaf.kind else {
            return None;
        };
        let (comparison, strict) = match op {
            BinOp::Ge => (Comparison::AtLeast, false),
            BinOp::Gt => (Comparison::AtLeast, true),
            BinOp::Le => (Comparison::AtMost, false),
            BinOp::Lt => (Comparison::AtMost, true),
            BinOp::Eq => (Comparison::Equal, false),
            _ => return None,
        };
        if !(is_numeric(left) && is_numeric(right)) {
            return None;
        }
        let dimension = dimension_of(&left.result_type);
        if comparison == Comparison::Equal && dimension != dimension_of(&right.result_type) {
            return None;
        }
        let mut builder = TreeBuilder {
            classifier,
            nodes: Vec::new(),
        };
        let lhs = builder.node(left);
        let rhs = builder.node(right);
        Some(Relation {
            nodes: builder.nodes,
            lhs,
            rhs,
            comparison,
            strict,
            dimension,
        })
    }

    /// The comparison is `>` or `<`.
    pub fn is_strict(&self) -> bool {
        self.strict
    }

    /// The comparison bounds rather than determines: `>=`/`>`/`<=`/`<`.
    pub fn is_order(&self) -> bool {
        self.comparison != Comparison::Equal
    }

    /// Every sub-expression was compiled; nothing is opaque.
    pub fn is_fully_representable(&self) -> bool {
        !self.nodes.iter().any(|node| matches!(node, Node::Opaque))
    }

    /// Narrow `bx` to the points where this relation can hold, relaxed by
    /// `slack`. On [`Revision::Empty`] the box's contents are unspecified.
    pub fn revise(&self, bx: &mut [Interval], slack: Slack) -> Revision {
        match self.narrow(bx, slack) {
            Some(()) => Revision::Consistent,
            None => Revision::Empty,
        }
    }

    fn narrow(&self, bx: &mut [Interval], slack: Slack) -> Option<()> {
        let mut values = self.forward(bx)?;
        self.compare(&mut values, slack)?;
        self.backward(&mut values, bx)
    }

    /// Enclose every node over `bx`; `None` when a node's domain is missed
    /// entirely (`√` of a wholly negative operand).
    fn forward(&self, bx: &[Interval]) -> Option<Vec<Interval>> {
        let mut values: Vec<Interval> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let value = match *node {
                Node::Const(c) => c,
                Node::Var(i) => bx.get(i).copied().unwrap_or(Interval::ENTIRE),
                Node::Opaque => Interval::ENTIRE,
                Node::Add(a, b) => values[a] + values[b],
                Node::Sub(a, b) => values[a] - values[b],
                Node::Mul(a, b) => values[a] * values[b],
                Node::Div(a, b) => values[a] / values[b],
                Node::Neg(a) => -values[a],
                Node::Powi(a, n) => values[a].powi(n),
                Node::Sqrt(a) => values[a].sqrt()?,
                Node::Abs(a) => values[a].abs(),
                Node::Min(a, b) => values[a].min(values[b]),
                Node::Max(a, b) => values[a].max(values[b]),
                Node::Hull(a, b) => values[a].hull(values[b]),
            };
            values.push(value);
        }
        Some(values)
    }

    /// Narrow both sides by the relaxed comparison, left first.
    fn compare(&self, values: &mut [Interval], slack: Slack) -> Option<()> {
        let relax = |side: Interval| self.relaxed(side, slack);
        let lhs = values[self.lhs];
        let narrowed_lhs = match self.comparison {
            Comparison::AtLeast => lhs.intersect(at_least(relax(values[self.rhs]).lo()))?,
            Comparison::AtMost => lhs.intersect(at_most(relax(values[self.rhs]).hi()))?,
            Comparison::Equal => lhs.intersect(relax(values[self.rhs]))?,
        };
        let rhs = values[self.rhs];
        let narrowed_rhs = match self.comparison {
            Comparison::AtLeast => rhs.intersect(at_most(relax(narrowed_lhs).hi()))?,
            Comparison::AtMost => rhs.intersect(at_least(relax(narrowed_lhs).lo()))?,
            Comparison::Equal => rhs.intersect(relax(narrowed_lhs))?,
        };
        values[self.lhs] = narrowed_lhs;
        values[self.rhs] = narrowed_rhs;
        Some(())
    }

    /// Every value `slack` lets the other side of the comparison take.
    fn relaxed(&self, side: Interval, slack: Slack) -> Interval {
        match (slack, self.comparison) {
            (Slack::Exact, _) => side,
            (Slack::Acceptance { .. }, Comparison::Equal) => {
                let (lo, hi) = equality_band(side.lo(), side.hi(), self.dimension);
                Interval::new(lo, hi).unwrap_or(Interval::ENTIRE)
            }
            (Slack::Acceptance { inequality }, _) => {
                debug_assert!(inequality >= 0.0, "negative inequality slack {inequality}");
                side + Interval::new(-inequality, inequality).unwrap_or(Interval::ENTIRE)
            }
        }
    }

    /// Project each node's narrowed value onto its operands, root to leaves.
    fn backward(&self, values: &mut [Interval], bx: &mut [Interval]) -> Option<()> {
        for index in (0..self.nodes.len()).rev() {
            let z = values[index];
            match self.nodes[index] {
                Node::Const(_) | Node::Opaque | Node::Hull(..) => {}
                Node::Var(i) => {
                    if let Some(slot) = bx.get_mut(i) {
                        *slot = slot.intersect(z)?;
                    }
                }
                Node::Add(a, b) => {
                    values[a] = values[a].intersect(z - values[b])?;
                    values[b] = values[b].intersect(z - values[a])?;
                }
                Node::Sub(a, b) => {
                    values[a] = values[a].intersect(z + values[b])?;
                    values[b] = values[b].intersect(values[a] - z)?;
                }
                Node::Mul(a, b) => {
                    values[a] = Interval::mul_preimage(z, values[a], values[b])?;
                    values[b] = Interval::mul_preimage(z, values[b], values[a])?;
                }
                Node::Div(a, b) => {
                    values[a] = Interval::div_numerator_preimage(z, values[a], values[b])?;
                    values[b] = Interval::div_denominator_preimage(z, values[b], values[a])?;
                }
                Node::Neg(a) => values[a] = values[a].intersect(-z)?,
                Node::Powi(a, n) => values[a] = Interval::powi_preimage(z, values[a], n)?,
                Node::Sqrt(a) => values[a] = Interval::sqrt_preimage(z, values[a])?,
                Node::Abs(a) => values[a] = Interval::abs_preimage(z, values[a])?,
                Node::Min(a, b) => {
                    values[a] = Interval::min_preimage(z, values[a], values[b])?;
                    values[b] = Interval::min_preimage(z, values[b], values[a])?;
                }
                Node::Max(a, b) => {
                    values[a] = Interval::max_preimage(z, values[a], values[b])?;
                    values[b] = Interval::max_preimage(z, values[b], values[a])?;
                }
            }
        }
        Some(())
    }
}

fn is_numeric(expr: &CompiledExpr) -> bool {
    expr.result_type.is_numeric()
}

fn at_least(lo: f64) -> Interval {
    Interval::new(lo, f64::INFINITY).unwrap_or(Interval::ENTIRE)
}

fn at_most(hi: f64) -> Interval {
    Interval::new(f64::NEG_INFINITY, hi).unwrap_or(Interval::ENTIRE)
}

/// A sub-expression after lowering: still constant over the box, left
/// unevaluated so that only a MAXIMAL constant subtree is evaluated; or the
/// node computing it, with its operands already placed.
#[derive(Clone, Copy, Debug)]
enum Lowered {
    Fixed,
    Computed(Node),
}

/// Lowers a [`CompiledExpr`] into [`Node`]s, children before parents.
///
/// Fixedness is decided bottom-up, once per node: a node the builder descends
/// into is Fixed when all its operands are, and a node it does not descend
/// into is asked once, by its refs. Lowering is therefore linear in the tree.
struct TreeBuilder<'c, C> {
    classifier: &'c C,
    nodes: Vec<Node>,
}

impl<C: OperandClassifier> TreeBuilder<'_, C> {
    /// Lower `expr` and return its node's index.
    fn node(&mut self, expr: &CompiledExpr) -> usize {
        let lowered = self.lower(expr);
        self.place(expr, lowered)
    }

    /// Push `expr`'s node, evaluating it if it is still Fixed.
    fn place(&mut self, expr: &CompiledExpr, lowered: Lowered) -> usize {
        let node = match lowered {
            Lowered::Fixed => self
                .classifier
                .evaluate(expr)
                .and_then(Interval::point)
                .map_or(Node::Opaque, Node::Const),
            Lowered::Computed(node) => node,
        };
        self.push(node)
    }

    fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn lower(&mut self, expr: &CompiledExpr) -> Lowered {
        match &expr.kind {
            CompiledExprKind::ValueRef(id) => match self.classifier.classify(id) {
                RefClass::Variable(i) => Lowered::Computed(Node::Var(i)),
                RefClass::Varying => Lowered::Computed(Node::Opaque),
                RefClass::Fixed => Lowered::Fixed,
            },
            CompiledExprKind::BinOp { op, left, right }
                if is_numeric(left) && is_numeric(right) =>
            {
                self.arithmetic(expr, *op, left, right)
            }
            CompiledExprKind::UnOp {
                op: UnOp::Neg,
                operand,
            } if is_numeric(operand) => self.compose([operand], |_, [a]| Node::Neg(a)),
            CompiledExprKind::FunctionCall { function, args } if args.iter().all(is_numeric) => {
                self.builtin(expr, &function.name, args)
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } if is_numeric(then_branch) && is_numeric(else_branch) => {
                self.conditional(condition, then_branch, else_branch)
            }
            _ => self.by_refs(expr),
        }
    }

    /// A node over `operands`: Fixed when every operand is, otherwise `make`
    /// over the operands' placed indices.
    fn compose<const N: usize>(
        &mut self,
        operands: [&CompiledExpr; N],
        make: impl FnOnce(&mut Self, [usize; N]) -> Node,
    ) -> Lowered {
        let lowered = operands.map(|operand| (operand, self.lower(operand)));
        if lowered
            .iter()
            .all(|(_, state)| matches!(state, Lowered::Fixed))
        {
            return Lowered::Fixed;
        }
        let placed = lowered.map(|(operand, lowered)| self.place(operand, lowered));
        Lowered::Computed(make(self, placed))
    }

    /// An expression the builder does not descend into: Fixed when every ref
    /// it holds is, otherwise opaque.
    fn by_refs(&self, expr: &CompiledExpr) -> Lowered {
        if self.is_fixed(expr) {
            Lowered::Fixed
        } else {
            Lowered::Computed(Node::Opaque)
        }
    }

    fn is_fixed(&self, expr: &CompiledExpr) -> bool {
        !expr.contains_value_ref_matching(|id| self.classifier.classify(id) != RefClass::Fixed)
    }

    fn arithmetic(
        &mut self,
        expr: &CompiledExpr,
        op: BinOp,
        left: &CompiledExpr,
        right: &CompiledExpr,
    ) -> Lowered {
        match op {
            BinOp::Add => self.compose([left, right], |_, [a, b]| Node::Add(a, b)),
            BinOp::Sub => self.compose([left, right], |_, [a, b]| Node::Sub(a, b)),
            BinOp::Mul if left.content_hash == right.content_hash => {
                self.compose([left], |_, [a]| Node::Powi(a, 2))
            }
            BinOp::Mul => self.compose([left, right], |_, [a, b]| Node::Mul(a, b)),
            BinOp::Div => self.compose([left, right], |_, [a, b]| Node::Div(a, b)),
            BinOp::Pow => self.power(left, right),
            _ => self.by_refs(expr),
        }
    }

    /// `base ^ exponent` is a [`Node::Powi`] when the exponent is Fixed and
    /// evaluates to an integer in `0..=MAX_INTEGER_EXPONENT`; any other power
    /// is opaque unless the whole of it is Fixed.
    fn power(&mut self, base: &CompiledExpr, exponent: &CompiledExpr) -> Lowered {
        if !self.is_fixed(exponent) {
            return Lowered::Computed(Node::Opaque);
        }
        match self.integer_exponent(exponent) {
            Some(n) => self.compose([base], |_, [a]| Node::Powi(a, n)),
            None => self.by_refs(base),
        }
    }

    /// The value of a Fixed exponent, when it is an integer in
    /// `0..=MAX_INTEGER_EXPONENT`.
    fn integer_exponent(&self, exponent: &CompiledExpr) -> Option<u32> {
        let value = self.classifier.evaluate(exponent)?;
        let in_range =
            value.fract() == 0.0 && (0.0..=f64::from(MAX_INTEGER_EXPONENT)).contains(&value);
        in_range.then_some(value as u32)
    }

    /// A conditional is the hull of its branches; it is Fixed only when its
    /// condition is too.
    fn conditional(
        &mut self,
        condition: &CompiledExpr,
        then_branch: &CompiledExpr,
        else_branch: &CompiledExpr,
    ) -> Lowered {
        if self.is_fixed(condition) {
            return self.compose([then_branch, else_branch], |_, [a, b]| Node::Hull(a, b));
        }
        let [a, b] = [then_branch, else_branch].map(|branch| self.node(branch));
        Lowered::Computed(Node::Hull(a, b))
    }

    /// The stdlib builtins recognised by `(name, arity)`, the keys reify-expr's
    /// `dual_builtin_rules` uses.
    fn builtin(&mut self, expr: &CompiledExpr, name: &str, args: &[CompiledExpr]) -> Lowered {
        match (name, args) {
            ("sqrt", [x]) => self.compose([x], |_, [a]| Node::Sqrt(a)),
            ("abs", [x]) => self.compose([x], |_, [a]| Node::Abs(a)),
            ("min", [a, b]) => self.compose([a, b], |_, [a, b]| Node::Min(a, b)),
            ("max", [a, b]) => self.compose([a, b], |_, [a, b]| Node::Max(a, b)),
            ("clamp", [x, lo, hi]) => self.compose([x, lo, hi], |builder, [x, lo, hi]| {
                let floored = builder.push(Node::Max(x, lo));
                Node::Min(floored, hi)
            }),
            _ => self.by_refs(expr),
        }
    }
}
