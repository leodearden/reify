//! Branch signatures: which non-smooth (kink) branch each dual evaluation took.
//!
//! Task #6672 (solver-unification ε).  Design reference:
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7.
//!
//! Forward-mode AD over an expression containing `if`, `match`, a comparison, a
//! Kleene connective, `min`/`max`/`abs`/`clamp`/`sign`/`floor`/`ceil`/`round`/
//! `mod`, or a field reduction returns the derivative of the *branch that was
//! actually taken* — the active-branch (Clarke) Jacobian.  That is the right
//! answer locally, but it is only meaningful together with a record of *which*
//! branch was taken: two evaluations that took different branches are two
//! different smooth functions, and a solver that treats their Jacobians as
//! samples of one function will chatter across the kink forever.
//!
//! [`BranchRecord`] is that record.  λ (#6679) consumes it to detect chatter,
//! through exactly two primitives: [`BranchRecord::differs_from`] (did the
//! active branch set change, and *where*) and [`BranchRecord::signature_key`]
//! (a hashable identity for a branch set, so alternation between two
//! signatures more than K times can be counted without retaining every
//! record).
//!
//! # Two properties λ depends on
//!
//! **A [`KinkSite`] is a STRUCTURAL PATH, deliberately not a `SourceSpan`.**
//! `CompiledExpr` carries no general span field — only `StructureInstanceCtor`
//! has one (`reify-ir/src/expr.rs:209`) — so there is no user-facing position
//! to record here even in principle.  λ must resolve the span for
//! `W_SOLVER_NONSMOOTH_STALL` from the owning constraint's
//! `ConstraintNodeId`, which `reify-constraints` already carries alongside
//! every `CompiledExpr`.
//!
//! **A site is stable under branch flips BY CONSTRUCTION.**  It is the
//! child-index path from the residual root, so it is a property of the tree
//! rather than of the traversal.  The alternative — a pre-order visit counter
//! — would fail precisely where it matters: short-circuiting
//! `Conditional`/`And`/`Or` skips whole subtrees, so every kink after a flip
//! would renumber by however many nodes the skipped branch contains, at
//! exactly the moment λ is trying to identify which kink moved.

use std::hash::{Hash, Hasher};

use reify_ir::{BinOp, Value};

/// Where a kink sits in the expression tree: the structural child-index path
/// from the residual root.
///
/// This is deliberately **not** a pre-order visit counter.  Short-circuiting
/// `Conditional`/`And`/`Or`/`Implies` means the number of nodes visited before
/// a given kink changes when a branch flips — so a counter would renumber every
/// downstream kink at exactly the moment λ is trying to identify which one
/// moved.  A structural path is invariant under branch flips, which is the
/// property chatter detection is built on.
///
/// **A note λ needs:** `CompiledExpr` carries no general `span` field (only
/// `StructureInstanceCtor` has one), so this cannot be a `SourceSpan`.  λ
/// resolves the user-facing span from the owning constraint's
/// `ConstraintNodeId`, which reify-constraints already carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct KinkSite(Vec<u16>);

impl KinkSite {
    /// The residual root itself.
    pub fn root() -> Self {
        KinkSite(Vec::new())
    }

    /// A site from an explicit child-index path.
    pub fn new(path: Vec<u16>) -> Self {
        KinkSite(path)
    }

    /// The child-index path from the residual root.
    pub fn path(&self) -> &[u16] {
        &self.0
    }
}

/// Which field reduction a [`KinkKind::FieldReduction`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReductionKind {
    Max,
    Min,
    ArgMax,
    ArgMin,
}

/// The kind of non-smooth node — the full PRD §7.7 kink vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KinkKind {
    /// `if cond then a else b`
    Conditional,
    /// `match disc { ... }`
    Match,
    /// One of `Eq Ne Lt Le Gt Ge`.
    Comparison(BinOp),
    /// One of `And Or Implies` — the short-circuit is the branch.
    Kleene(BinOp),
    Min,
    Max,
    Abs,
    Clamp,
    Sign,
    Floor,
    Ceil,
    Round,
    Mod,
    FieldReduction(ReductionKind),
}

/// Which branch of a kink was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchChoice {
    // Conditional
    Then,
    Else,
    // Match — the index of the selected arm.
    Arm(usize),
    // Comparison
    Satisfied,
    Unsatisfied,
    // Kleene: the short-circuit outcome, mirroring `eval_and`/`eval_or`/
    // `eval_implies`.
    /// The left operand was absorbing, so the right was never evaluated.
    LeftAbsorbing,
    /// Neither short-circuit fired; both operands were evaluated.
    BothEvaluated,
    /// The left operand was neither Bool nor Undef; the right was never
    /// evaluated and the result is `Undef`.
    LeftTypeError,
    /// `min`/`max`/`clamp`-style selection of argument `i`.
    Operand(usize),
    // abs
    Negative,
    Zero,
    Positive,
    // clamp
    BelowLo,
    Interior,
    AboveHi,
    /// The integer (or sign) cell `sign`/`floor`/`ceil`/`round` landed in.
    IntegerCell(i64),
    /// The quotient cell `mod` landed in.
    ModQuotient(i64),
    /// The domain coordinate at which a field reduction attained its extremum.
    FieldArgExtremum(Box<Value>),
    /// The branch could not be determined (e.g. an `Undef` comparison, or a
    /// field whose `argmax`/`argmin` is not computable).
    Unresolved,
}

impl Hash for BranchChoice {
    /// Hand-written because `Value` implements `PartialEq`/`Eq`/`Ord` (all via
    /// `total_cmp` for floats) but **not** `Hash`.
    ///
    /// The float-bearing [`BranchChoice::FieldArgExtremum`] arm hashes the
    /// value's `content_hash()`, which is reify's canonical content-addressed
    /// identity.  KNOWN CAVEAT, inherited from that function: `content_hash`
    /// canonicalises every NaN bit pattern, so two `FieldArgExtremum` values
    /// differing only in NaN payload are unequal under `PartialEq` yet hash
    /// alike.  That is a deliberate documented exception in `Value::content_hash`
    /// itself, and it only ever costs a hash collision, never a wrong `==`.
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            BranchChoice::Arm(i) | BranchChoice::Operand(i) => i.hash(state),
            BranchChoice::IntegerCell(k) | BranchChoice::ModQuotient(k) => k.hash(state),
            BranchChoice::FieldArgExtremum(v) => v.content_hash().0.hash(state),
            _ => {}
        }
    }
}

/// One non-smooth node traversed by one dual evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchEntry {
    /// Where the kink sits, as a structural child-index path from the root.
    pub site: KinkSite,
    /// What kind of kink it is.
    pub kind: KinkKind,
    /// Which branch this evaluation took.
    pub choice: BranchChoice,
}

/// The branches taken by one dual evaluation of one expression, in traversal
/// order.
///
/// An empty record means the traversal encountered no non-smooth node at all —
/// the expression is smooth at this point, and its Jacobian row is an ordinary
/// derivative rather than a Clarke selection.  That statement has to be
/// trustworthy, so a kink is recorded even when its tangent turns out to be
/// zero or unavailable: "no entries" must mean "no kink", never "we did not
/// look".
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BranchRecord {
    entries: Vec<BranchEntry>,
}

impl BranchRecord {
    /// An empty record — no kink has been traversed yet.
    pub fn new() -> Self {
        BranchRecord::default()
    }

    /// Append one traversed kink.
    pub fn push(&mut self, entry: BranchEntry) {
        self.entries.push(entry);
    }

    /// The traversed kinks, in traversal order.
    pub fn entries(&self) -> &[BranchEntry] {
        &self.entries
    }

    /// Number of non-smooth nodes traversed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the traversal encountered no non-smooth node.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The first site, in traversal order, at which these two records
    /// disagree — or `None` when they took exactly the same branches.
    ///
    /// `Some(site)` means the two evaluations sampled two *different* smooth
    /// functions, so their Jacobians are not two samples of one function.
    /// That is the "branch-change signature difference contracts the trust
    /// region" primitive η (#6675) and λ (#6679) call.
    ///
    /// Records are compared entry-by-entry rather than as sets, because the
    /// order is itself meaningful: it is evaluation order, and a kink that
    /// stops being traversed at all is as much a change as one that flips.
    /// A disagreement is therefore reported when the site, the kind or the
    /// choice differs, and also when one record simply has an entry the other
    /// lacks — in which case the site named is the extra entry's own.
    pub fn differs_from(&self, other: &BranchRecord) -> Option<KinkSite> {
        for (a, b) in self.entries.iter().zip(other.entries.iter()) {
            if a != b {
                return Some(a.site.clone());
            }
        }
        // One ran out first: the next entry of the longer record is the
        // divergence.
        match self.entries.len().cmp(&other.entries.len()) {
            std::cmp::Ordering::Greater => Some(self.entries[other.entries.len()].site.clone()),
            std::cmp::Ordering::Less => Some(other.entries[self.entries.len()].site.clone()),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// A stable, order-sensitive key for this branch set.
    ///
    /// Equal records hash equal; a change to any single `site`, `kind` or
    /// `choice` changes the key.  The key is a pure function of the entries,
    /// so re-evaluating at the same point reproduces it exactly — λ counts
    /// alternations between keys, and a key that wobbled would manufacture
    /// phantom chatter.
    ///
    /// The length is folded in first so that one record being a prefix of
    /// another cannot collide.
    pub fn signature_key(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.entries.len().hash(&mut hasher);
        for entry in &self.entries {
            entry.hash(&mut hasher);
        }
        hasher.finish()
    }
}
