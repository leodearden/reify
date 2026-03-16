use crate::diagnostics::Diagnostic;
use crate::expr::CompiledExpr;
use crate::identity::ConstraintNodeId;
use crate::value::{Satisfaction, ValueMap};

/// Input to constraint checking: a batch of constraints with current values.
#[derive(Debug)]
pub struct ConstraintInput<'a> {
    /// The constraints to check, keyed by their node ID.
    pub constraints: Vec<(ConstraintNodeId, &'a CompiledExpr)>,
    /// Current values of all cells referenced by constraints.
    pub values: &'a ValueMap,
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

/// Trait for constraint checking. Lives in reify-types for dependency inversion —
/// implemented in reify-constraints, consumed by reify-eval.
pub trait ConstraintChecker: Send + Sync {
    /// Check a batch of constraints against current values.
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult>;
}
