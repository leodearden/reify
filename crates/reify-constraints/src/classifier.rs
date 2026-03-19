//! Constraint domain classification.
//!
//! Walks a `CompiledExpr` tree and determines which `ConstraintDomain`
//! applies, based on the leaf value types and operators encountered.

use reify_types::{CompiledExpr, ConstraintDomain};

/// Classifies constraint expressions into their constraint domain.
pub struct ConstraintClassifier;

impl ConstraintClassifier {
    /// Classify a compiled expression into its constraint domain.
    pub fn classify(_expr: &CompiledExpr) -> ConstraintDomain {
        todo!("ConstraintClassifier::classify not yet implemented")
    }
}
