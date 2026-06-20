use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use reify_core::diagnostics::Diagnostic;
use crate::expr::{CompiledExpr, CompiledFunction};
use reify_core::identity::{ConstraintNodeId, ValueCellId};
use crate::persistent::PersistentMap;
use reify_core::ty::Type;
use crate::value::{DeterminacyState, Satisfaction, Value, ValueMap};

/// Input to constraint checking: a batch of constraints with current values.
#[derive(Debug)]
pub struct ConstraintInput<'a> {
    /// The constraints to check, keyed by their node ID.
    ///
    /// Use `Cow::Borrowed(&slice)` when the caller already holds a long-lived
    /// slice (e.g., the DFS hot path in `auto_type_param`) to avoid a per-leaf
    /// clone of the `ConstraintNodeId` strings. Use `Cow::Owned(vec![...])` for
    /// ad-hoc construction in tests and one-off call sites — the `Deref` to
    /// `&[T]` means all read-only consumers (`iter()`, `len()`, `is_empty()`,
    /// `for (id, _) in &input.constraints`) are zero-touch.
    pub constraints: Cow<'a, [(ConstraintNodeId, &'a CompiledExpr)]>,
    /// Current values of all cells referenced by constraints.
    pub values: &'a ValueMap,
    /// User-defined functions available for evaluation within constraint expressions.
    pub functions: &'a [CompiledFunction],
    /// Optional determinacy snapshot for evaluating DeterminacyPredicate expressions
    /// within constraints. When `Some`, the checker passes this to `EvalContext::with_determinacy()`
    /// so that `determined()`, `undetermined()`, `constrained()`, and `partially_determined()`
    /// predicates can look up cell determinacy states.
    ///
    /// Defaults to `None` for backward compatibility — existing callers that don't need
    /// determinacy context can omit this field.
    pub determinacy: Option<&'a PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
}

/// Result of checking a single constraint.
#[derive(Debug, Clone)]
pub struct ConstraintResult {
    pub id: ConstraintNodeId,
    pub satisfaction: Satisfaction,
    pub diagnostics: ConstraintDiagnostics,
}

/// Diagnostic information from constraint checking.
#[derive(Debug, Clone, Default)]
pub struct ConstraintDiagnostics {
    /// Human-readable messages about the constraint state.
    pub messages: Vec<Diagnostic>,
}

/// The domain of a constraint, determining which solver handles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintDomain {
    /// Dimensional constraints (e.g., length ratios, unit conversions).
    Dimensional,
    /// Geometric constraints (e.g., parallelism, tangency).
    Geometric,
    /// Logical constraints (e.g., boolean conditions).
    Logical,
    /// Cross-domain constraints spanning multiple domains.
    CrossDomain,
}

/// The sense of an optimization objective term (PRD §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectiveSense {
    /// Minimize the value of the expression.
    Minimize,
    /// Maximize the value of the expression.
    Maximize,
}

/// A single term in an `ObjectiveSet` (PRD §6.1). The defaults (`weight = 1.0`,
/// `priority = 0`) are applied by `ObjectiveTerm::new`; the explicit struct-literal
/// path is reserved for compiler lowering (β) and solver folding (δ).
#[derive(Debug, Clone)]
pub struct ObjectiveTerm {
    pub sense: ObjectiveSense,
    pub expr: CompiledExpr,
    /// > 0; default 1.0 (PRD §6.1, invariant I3 — the WeightedSum cost contribution).
    pub weight: f64,
    /// default 0; higher = solved first in `Lexicographic` (PRD §6.1, invariant I4).
    pub priority: u32,
}

impl ObjectiveTerm {
    /// Build a term with default `weight = 1.0` and `priority = 0`.
    pub fn new(sense: ObjectiveSense, expr: CompiledExpr) -> Self {
        Self { sense, expr, weight: 1.0, priority: 0 }
    }
}

/// How an `ObjectiveSet`'s terms are combined into a single solver cost (PRD §6.1).
/// Closed set per invariant I6 — adding variants is a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectiveCombination {
    /// Scalar-weighted linear combination of all terms.
    WeightedSum,
    /// Solve terms in descending priority order; earlier terms dominate later ones.
    Lexicographic,
}

/// A multi-objective container (PRD §6.1).
///
/// Invariant I1 (non-empty): `terms` always holds ≥ 1 term. Enforced by the
/// `single` constructor; broader constructors land in later phases (β/γ).
#[derive(Debug, Clone)]
pub struct ObjectiveSet {
    /// INVARIANT: non-empty.
    pub terms: Vec<ObjectiveTerm>,
    pub combination: ObjectiveCombination,
}

impl ObjectiveSet {
    /// Build a 1-term `WeightedSum` set with `weight = 1.0` and `priority = 0`.
    ///
    /// This is the single-objective compat constructor (PRD §6.2 invariant I2):
    /// `ObjectiveSet::single(sense, expr)` is the multi-objective replacement
    /// for the old single-variant objective enum construction, and produces a
    /// bit-identical solver input.
    pub fn single(sense: ObjectiveSense, expr: CompiledExpr) -> Self {
        Self {
            terms: vec![ObjectiveTerm::new(sense, expr)],
            combination: ObjectiveCombination::WeightedSum,
        }
    }
}

/// The realised contribution of a single `ObjectiveTerm` after solve (PRD §3.5 item 3, task θ #4015).
///
/// Stored in `ObjectiveProvenance.term_contributions` — one entry per term in the governing
/// `ObjectiveSet`. The contribution is the signed cost added to the objective by this term:
///   `contribution = weight × σ(sense) × realized_value`
/// where σ(Minimize)=+1, σ(Maximize)=−1 (PRD §6.2 invariant I3).
#[derive(Debug, Clone)]
pub struct TermContribution {
    /// The optimisation sense (Minimize or Maximize) of the term.
    pub sense: ObjectiveSense,
    /// The term weight (> 0; 1.0 for single-objective compat via `ObjectiveTerm::new`).
    pub weight: f64,
    /// The SI-scalar value of the term expression evaluated against the post-solve value map.
    /// `f64::NAN` when the expression does not reduce to a scalar.
    pub realized_value: f64,
    /// Signed cost contribution: `weight × σ(sense) × realized_value`.
    pub contribution: f64,
}

/// Per-auto-cell provenance record produced by `Engine::eval()` (PRD §3.5, task θ #4015).
///
/// Carried in `EvalResult::objective_provenance` (keyed by `ValueCellId`) — populated by
/// `eval()` only; all other `EvalResult` construction sites set an empty map.
///
/// Four items per the PRD §3.5 enumeration:
///   (1) `objective`      — which `ObjectiveSet` governed the cell (None for synthetic centrality)
///   (2) `combination`    — the combination strategy (None for synthetic centrality)
///   (3) `term_contributions` — per-term realised contribution (empty for synthetic centrality)
///   (4) `synthetic_centrality` — whether the Chebyshev-centre default fired (I5 hook, task η)
///
/// # Sharing
///
/// `objective` and `term_contributions` are wrapped in `Arc` so that all cells in the same
/// scope share the same heap allocation — `Engine::eval()` clones these `Arc`s (O(1) refcount
/// bump) once per cell rather than deep-cloning the `ObjectiveSet` and contributions `Vec`
/// for every auto cell in the scope (was O(N × |terms|) per scope).  Consumers access the
/// contents via `Deref` as usual (`.is_some()`, `[i]`, `.iter()`, etc.).
#[derive(Debug, Clone)]
pub struct ObjectiveProvenance {
    /// The template scope name that was solved (e.g. `"WeightedObjective"`).
    pub scope: String,
    /// The `ObjectiveSet` that governed this cell, or `None` for synthetic-centrality scopes.
    ///
    /// Wrapped in `Arc` so all cells in the same scope share one allocation (see type-level doc).
    pub objective: Option<Arc<ObjectiveSet>>,
    /// Mirror of `objective.combination` for convenient access; `None` iff `objective` is `None`.
    pub combination: Option<ObjectiveCombination>,
    /// Per-term realised contributions. Empty for synthetic-centrality scopes and for cells
    /// resolved with no objective (feasibility-only); populated by `eval()` step-4.
    ///
    /// Wrapped in `Arc` so all cells in the same scope share one allocation (see type-level doc).
    pub term_contributions: Arc<Vec<TermContribution>>,
    /// `true` when the Chebyshev-centre default objective was synthesised for this scope
    /// (I5 provenance hook, task η #4013). `false` for explicit-objective and objective-less
    /// (feasibility-only) scopes.
    pub synthetic_centrality: bool,
}

/// An auto parameter to be resolved by the constraint solver.
#[derive(Debug, Clone)]
pub struct AutoParam {
    /// The value cell this auto param corresponds to.
    pub id: ValueCellId,
    /// The declared type of the parameter.
    pub param_type: Type,
    /// Optional lower and upper bounds for numeric resolution.
    pub bounds: Option<(f64, f64)>,
    /// Whether this is an `auto(free)` parameter that skips uniqueness verification.
    /// When `true`, the solver skips the perturbation-based uniqueness check and
    /// returns `SolveResult::Solved { unique: false }` directly.
    pub free: bool,
}

/// The result of a constraint solve attempt.
#[derive(Debug, Clone)]
pub enum SolveResult {
    /// Successfully resolved all auto parameters.
    ///
    /// **Note:** `Solved` indicates constraint satisfaction but does not guarantee
    /// objective optimality. When an optimization objective is present, the
    /// Nelder-Mead optimizer may have hit the iteration limit without full
    /// convergence; the returned values satisfy all constraints but the objective
    /// value may not be globally optimal.
    Solved {
        /// Resolved values for auto parameters.
        values: HashMap<ValueCellId, Value>,
        /// Whether the solution is uniquely determined. `true` for strict auto
        /// parameters that pass perturbation-based uniqueness verification;
        /// `false` for auto(free) parameters where uniqueness is skipped.
        unique: bool,
    },
    /// The constraints are infeasible — no solution exists.
    Infeasible {
        /// Diagnostics explaining why the constraints are infeasible.
        diagnostics: Vec<Diagnostic>,
    },
    /// The solver made no progress (e.g., iteration limit reached).
    NoProgress {
        /// Human-readable reason for no progress.
        reason: String,
    },
}

/// A constraint resolution problem — input to the constraint solver.
#[derive(Debug, Clone)]
pub struct ResolutionProblem {
    /// The auto parameters to resolve.
    pub auto_params: Vec<AutoParam>,
    /// Constraints to satisfy, each paired with its compiled expression.
    pub constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
    /// Current values of all cells referenced by constraints.
    pub current_values: ValueMap,
    /// Optional multi-objective optimization container (PRD §6.1).
    pub objective: Option<ObjectiveSet>,
    /// User-defined functions available for evaluating expressions.
    /// Shares the same Arc allocation as `Engine.functions` — assigned via
    /// `Arc::clone` at the solver boundary so construction is O(1) (task #2286).
    /// The inner type is `[CompiledFunction]` (slice), not `Vec<CompiledFunction>`,
    /// so the table lives in a single Arc-owned heap buffer — one pointer hop
    /// instead of Arc → Vec header → heap (task #2413).
    pub functions: Arc<[CompiledFunction]>,
}

/// Trait for constraint checking. Lives in reify-types for dependency inversion —
/// implemented in reify-constraints, consumed by reify-eval.
pub trait ConstraintChecker: Send + Sync {
    /// Check a batch of constraints against current values.
    ///
    /// # Labeled-constraint convention
    ///
    /// When a [`ConstraintInput`] entry's [`ConstraintNodeId`] corresponds to a
    /// labeled constraint (i.e. originated from a `constraint def` instantiation),
    /// implementations **SHOULD** embed `id.to_string()` somewhere in the
    /// `message` or `labels[i].message` of every [`Severity::Error`] diagnostic
    /// they emit. The engine's label-rewrite pass (`labeled_diagnostics`) uses
    /// a substring search to substitute the friendly label
    /// (e.g. `"MinWall#0[0]"`) for the opaque id, so users see the
    /// human-readable form in error output.
    ///
    /// This is a **soft recommendation**, not a hard invariant. Checkers that
    /// emit domain-specific error text without embedding the raw id (e.g.
    /// `"wall thickness below minimum"`) will have that text surface to users
    /// unmodified — which is still correct. The engine will emit a
    /// `tracing::debug!` event (target `reify_eval::engine_constraints`) when
    /// an Error-severity message is present but the raw id is absent. This
    /// signal is aimed at **first-party developers** diagnosing `Display`-impl
    /// drift; third-party `ConstraintChecker` implementations that intentionally
    /// use domain-specific text can safely ignore it — the debug level is off
    /// by default and will not appear in production logs unless explicitly
    /// enabled (e.g. `RUST_LOG=reify_eval=debug`).
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult>;

    /// Returns `true` if this checker is the compile-time indeterminate stub
    /// (`CompileTimeIndeterminateChecker` in `reify-compiler`), `false` for all
    /// real or test-injected checkers.
    ///
    /// # Purpose
    ///
    /// The Gap-C honesty diagnostic (`W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED`,
    /// task 4616) must fire on the **real-checker** resolution path (`reify check`
    /// / GUI / `compile_with_stdlib_checked`) but must NOT add noise on the pure
    /// compile-time stub path (`compile_with_stdlib`, `examples_smoke`). The
    /// checker object is the thing that knows real-vs-stub, so the discriminator
    /// belongs on it.
    ///
    /// # Default
    ///
    /// Returns `false` for all checkers that do not override this method —
    /// including `SimpleConstraintChecker`, all mock/test checkers, and any
    /// third-party implementations.  Only `CompileTimeIndeterminateChecker`
    /// (in `crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs`)
    /// overrides this to return `true`.
    ///
    /// Changing the default to `true` would invert the gate (suppress the
    /// warning everywhere) — do not change the default.
    fn is_compile_time_stub(&self) -> bool {
        false
    }
}

/// Input to an optimized implementation (Task 273 — @optimized plumbing).
///
/// Mirrors `ConstraintInput` in shape: a batch of constraints with current values,
/// available user functions, and an optional determinacy snapshot. The Engine's
/// `dispatch_constraints` helper splits constraints by registered optimization
/// target and calls into the matching `OptimizedImpl` with this input.
#[derive(Debug)]
pub struct OptimizedImplInput<'a> {
    /// The constraints assigned to this optimized implementation, keyed by node ID.
    pub constraints: Vec<(ConstraintNodeId, &'a CompiledExpr)>,
    /// Current values of all cells referenced by constraints.
    pub values: &'a ValueMap,
    /// User-defined functions available for evaluation.
    pub functions: &'a [CompiledFunction],
    /// Optional determinacy snapshot, supplied when determinacy predicates are
    /// reachable from within these constraints. Same shape as `ConstraintInput::determinacy`.
    pub determinacy: Option<&'a PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
}

/// Output from an optimized implementation.
///
/// Carries one `ConstraintResult` per input constraint. Producers are expected to
/// preserve the same id/order the caller supplied so the Engine can weave results
/// back into the original constraint sequence without extra bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct OptimizedImplOutput {
    pub results: Vec<ConstraintResult>,
}

/// Trait for an optimized constraint implementation (Task 273 — @optimized plumbing).
///
/// Registered on the Engine via `register_optimized_impl(target, imp)` and invoked
/// by `dispatch_constraints` for any constraint whose originating `constraint def`
/// carried an `@optimized("target")` annotation. Lives in reify-types so that
/// reify-eval can own a trait object without a direct dependency on any concrete
/// optimizer crate.
///
/// # Scope
///
/// This trait is currently consumed **only** on the *checker* path — the
/// Engine's `dispatch_constraints` helper routes annotated constraints to a
/// registered impl during `Engine::check` / `check_snapshot` /
/// `build_snapshot` / `edit_check`. The *solver* path (`Engine::resolve`,
/// which drives auto-param resolution via `ConstraintSolver`) still feeds
/// every constraint — including `@optimized`-annotated ones — through the
/// ordinary language-level solver, with no opportunity for an `OptimizedImpl`
/// to participate. Extending solver dispatch to route through
/// `OptimizedImpl` is a follow-up; see `CompiledConstraint::optimized_target`.
pub trait OptimizedImpl: Send + Sync {
    /// Evaluate a batch of constraints routed to this implementation.
    fn check(&self, input: &OptimizedImplInput) -> OptimizedImplOutput;
}

/// Trait for constraint solving. Lives in reify-types for dependency inversion —
/// implemented in reify-constraints, consumed by reify-eval.
pub trait ConstraintSolver: Send + Sync {
    /// Attempt to resolve auto parameters to satisfy constraints.
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraint_domain_all_variants_exist() {
        let _dimensional = ConstraintDomain::Dimensional;
        let _geometric = ConstraintDomain::Geometric;
        let _logical = ConstraintDomain::Logical;
        let _cross = ConstraintDomain::CrossDomain;
    }

    #[test]
    fn constraint_domain_is_copy_clone_eq_hash() {
        let d = ConstraintDomain::Dimensional;
        let d2 = d; // Copy
        assert_eq!(d, d2); // PartialEq + Eq

        let d3 = Clone::clone(&d); // Clone
        assert_eq!(d, d3);

        // Hash: usable as HashMap key
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(ConstraintDomain::Dimensional, "dim");
        map.insert(ConstraintDomain::Geometric, "geo");
        assert_eq!(map.get(&ConstraintDomain::Dimensional), Some(&"dim"));
    }

    #[test]
    fn constraint_domain_variants_are_distinct() {
        assert_ne!(ConstraintDomain::Dimensional, ConstraintDomain::Geometric);
        assert_ne!(ConstraintDomain::Dimensional, ConstraintDomain::Logical);
        assert_ne!(ConstraintDomain::Dimensional, ConstraintDomain::CrossDomain);
        assert_ne!(ConstraintDomain::Geometric, ConstraintDomain::Logical);
        assert_ne!(ConstraintDomain::Geometric, ConstraintDomain::CrossDomain);
        assert_ne!(ConstraintDomain::Logical, ConstraintDomain::CrossDomain);
    }

    #[test]
    fn constraint_domain_debug() {
        assert!(format!("{:?}", ConstraintDomain::Dimensional).contains("Dimensional"));
        assert!(format!("{:?}", ConstraintDomain::CrossDomain).contains("CrossDomain"));
    }

    #[test]
    fn auto_param_with_bounds() {
        use reify_core::identity::ValueCellId;
        use reify_core::ty::Type;

        let ap = AutoParam {
            id: ValueCellId::new("Bracket", "width"),
            param_type: Type::length(),
            bounds: Some((0.01, 1.0)),
            free: false,
        };
        assert_eq!(ap.id, ValueCellId::new("Bracket", "width"));
        assert_eq!(ap.param_type, Type::length());
        assert_eq!(ap.bounds, Some((0.01, 1.0)));
    }

    #[test]
    fn auto_param_with_free_flag() {
        use reify_core::identity::ValueCellId;
        use reify_core::ty::Type;

        let strict = AutoParam {
            id: ValueCellId::new("Bracket", "width"),
            param_type: Type::length(),
            bounds: Some((0.01, 1.0)),
            free: false,
        };
        assert!(!strict.free);

        let free = AutoParam {
            id: ValueCellId::new("Bracket", "height"),
            param_type: Type::length(),
            bounds: Some((0.01, 1.0)),
            free: true,
        };
        assert!(free.free);
    }

    #[test]
    fn auto_param_without_bounds() {
        use reify_core::identity::ValueCellId;
        use reify_core::ty::Type;

        let ap = AutoParam {
            id: ValueCellId::new("Bracket", "angle"),
            param_type: Type::angle(),
            bounds: None,
            free: false,
        };
        assert!(ap.bounds.is_none());

        // Debug works
        let debug = format!("{:?}", ap);
        assert!(debug.contains("AutoParam"));
    }

    fn make_literal_expr() -> CompiledExpr {
        use reify_core::hash::ContentHash;
        use crate::value::Value;
        CompiledExpr {
            kind: crate::expr::CompiledExprKind::Literal(Value::Real(1.0)),
            result_type: Type::dimensionless_scalar(),
            content_hash: ContentHash::of(b"test"),
        }
    }

    #[test]
    fn resolution_problem_empty() {
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: crate::value::ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };
        assert!(problem.auto_params.is_empty());
        assert!(problem.constraints.is_empty());
        assert!(problem.current_values.is_empty());
        assert!(problem.objective.is_none());
    }

    #[test]
    fn resolution_problem_populated() {
        use reify_core::identity::ValueCellId;
        let mut values = crate::value::ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "width"),
            crate::value::Value::length(0.08),
        );

        let problem = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: ValueCellId::new("Bracket", "width"),
                param_type: Type::length(),
                bounds: Some((0.01, 1.0)),
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Bracket", 0), make_literal_expr())],
            current_values: values,
            objective: Some(ObjectiveSet::single(ObjectiveSense::Minimize, make_literal_expr())),
            functions: vec![].into(),
        };
        assert_eq!(problem.auto_params.len(), 1);
        assert_eq!(problem.constraints.len(), 1);
        assert!(!problem.current_values.is_empty());
        assert!(problem.objective.is_some());

        // Debug works
        let debug = format!("{:?}", problem);
        assert!(debug.contains("ResolutionProblem"));
    }

    #[test]
    fn solve_result_solved() {
        use reify_core::identity::ValueCellId;
        use crate::value::Value;
        use std::collections::HashMap;

        let mut values = HashMap::new();
        values.insert(ValueCellId::new("Bracket", "width"), Value::length(0.05));

        let result = SolveResult::Solved {
            values,
            unique: true,
        };
        match &result {
            SolveResult::Solved { values, .. } => {
                assert_eq!(values.len(), 1);
                assert!(values.contains_key(&ValueCellId::new("Bracket", "width")));
            }
            _ => panic!("expected Solved"),
        }
    }

    #[test]
    fn solve_result_solved_with_unique_flag() {
        use reify_core::identity::ValueCellId;
        use crate::value::Value;
        use std::collections::HashMap;

        // unique = true (strict auto, uniquely determined)
        let mut values = HashMap::new();
        values.insert(ValueCellId::new("Bracket", "width"), Value::length(0.05));
        let result = SolveResult::Solved {
            values,
            unique: true,
        };
        match &result {
            SolveResult::Solved { unique, .. } => assert!(unique),
            _ => panic!("expected Solved"),
        }

        // unique = false (auto(free), not uniquely determined)
        let result = SolveResult::Solved {
            values: HashMap::new(),
            unique: false,
        };
        match &result {
            SolveResult::Solved { unique, .. } => assert!(!unique),
            _ => panic!("expected Solved"),
        }
    }

    #[test]
    fn solve_result_infeasible() {
        use reify_core::diagnostics::Diagnostic;

        let result = SolveResult::Infeasible {
            diagnostics: vec![Diagnostic::error("constraint unsatisfiable")],
        };
        match &result {
            SolveResult::Infeasible { diagnostics } => {
                assert_eq!(diagnostics.len(), 1);
                assert!(diagnostics[0].message.contains("unsatisfiable"));
            }
            _ => panic!("expected Infeasible"),
        }
    }

    #[test]
    fn solve_result_no_progress() {
        let result = SolveResult::NoProgress {
            reason: "iteration limit reached".to_string(),
        };
        match &result {
            SolveResult::NoProgress { reason } => {
                assert_eq!(reason, "iteration limit reached");
            }
            _ => panic!("expected NoProgress"),
        }
    }

    #[test]
    fn solve_result_clone() {
        let result = SolveResult::NoProgress {
            reason: "test".to_string(),
        };
        let result2 = result.clone();
        let d1 = format!("{:?}", result);
        let d2 = format!("{:?}", result2);
        assert_eq!(d1, d2);
    }

    struct MockSolver;

    impl ConstraintSolver for MockSolver {
        fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
            SolveResult::NoProgress {
                reason: "mock".to_string(),
            }
        }
    }

    #[test]
    fn constraint_solver_trait_call() {
        let solver = MockSolver;
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: crate::value::ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };
        let result = solver.solve(&problem);
        match result {
            SolveResult::NoProgress { reason } => assert_eq!(reason, "mock"),
            _ => panic!("expected NoProgress"),
        }
    }

    #[test]
    fn constraint_solver_is_send_sync() {
        // Verify the trait requires Send + Sync by using it as a trait object
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockSolver>();

        // Can be used as Box<dyn ConstraintSolver>
        let _boxed: Box<dyn ConstraintSolver> = Box::new(MockSolver);
    }

    /// `ConstraintInput::constraints` can be constructed with `Cow::Borrowed`,
    /// allowing the hot path to pass a slice through without cloning.
    ///
    /// Asserts:
    /// (a) the variant is `Cow::Borrowed` (no hidden `.to_owned()` call),
    /// (b) dereferencing via `Deref<Target=[T]>` works (`.len()` is transparent),
    /// (c) the borrowed-slice pointer equals the source slice (zero-copy).
    #[test]
    fn constraint_input_constraints_field_accepts_cow_borrowed() {
        use std::borrow::Cow;

        let expr = make_literal_expr();
        let v: Vec<(ConstraintNodeId, &CompiledExpr)> =
            vec![(ConstraintNodeId::new("C0", 0), &expr)];
        let empty_values = crate::value::ValueMap::new();

        let input = ConstraintInput {
            constraints: Cow::Borrowed(&v[..]),
            values: &empty_values,
            functions: &[],
            determinacy: None,
        };

        // (a) Deref to &[T] is transparent (compile-check: Cow::Borrowed(&v[..]) in
        // field position already pins the API — the matches! assertion is a tautology)
        assert_eq!(
            input.constraints.len(),
            v.len(),
            "Cow::Borrowed deref must report the correct length"
        );
        // (b) zero-copy: borrowed-slice pointer equals source — this is the genuine
        // regression guard; a hidden .to_owned() call would change the pointer.
        assert!(
            std::ptr::eq(input.constraints.as_ref(), v.as_slice()),
            "Cow::Borrowed pointer must equal source slice pointer (no hidden copy)"
        );
    }

    /// `ConstraintInput::constraints` can also be constructed with `Cow::Owned`,
    /// preserving the ergonomic inline `vec![...]` pattern for all non-hot-path callers.
    #[test]
    fn constraint_input_constraints_field_accepts_cow_owned() {
        use std::borrow::Cow;

        let expr = make_literal_expr();
        let empty_values = crate::value::ValueMap::new();

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(ConstraintNodeId::new("C0", 0), &expr)]),
            values: &empty_values,
            functions: &[],
            determinacy: None,
        };

        // Compile-check: Cow::Owned(vec![...]) in field position already pins the API.
        // The matches! variant assertion is a tautology — Deref transparency is the
        // meaningful contract here.
        assert_eq!(input.constraints.len(), 1);
    }
}
