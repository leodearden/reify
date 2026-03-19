//! Constraint domain classification.
//!
//! Walks a `CompiledExpr` tree and determines which `ConstraintDomain`
//! applies, based on the leaf value types and operators encountered.

use reify_types::{CompiledExpr, CompiledExprKind, ConstraintDomain, Type, Value};

/// Internal flags collected during expression tree traversal.
#[derive(Default)]
struct DomainFlags {
    /// Saw a numeric leaf (Scalar, Real, Int) or numeric-typed ValueRef.
    has_numeric: bool,
    /// Saw a Bool leaf or Bool-typed ValueRef.
    has_logical: bool,
    /// Saw a geometry-related function call.
    has_geometric: bool,
}

impl DomainFlags {
    fn into_domain(self) -> ConstraintDomain {
        if self.has_geometric {
            if self.has_logical {
                ConstraintDomain::CrossDomain
            } else {
                ConstraintDomain::Geometric
            }
        } else if self.has_numeric && self.has_logical {
            ConstraintDomain::CrossDomain
        } else if self.has_logical {
            ConstraintDomain::Logical
        } else {
            // Default: Dimensional (numeric or empty)
            ConstraintDomain::Dimensional
        }
    }
}

/// Known geometry-related function qualified name prefixes.
fn is_geometry_qualified_name(qualified_name: &str) -> bool {
    qualified_name.starts_with("std::geo::")
        || qualified_name.starts_with("std::geometry::")
        || matches!(
            qualified_name,
            "std::distance" | "std::angle_between" | "std::parallel" | "std::tangent"
        )
}

/// Classifies constraint expressions into their constraint domain.
pub struct ConstraintClassifier;

impl ConstraintClassifier {
    /// Classify a compiled expression into its constraint domain.
    ///
    /// Walks the expression tree and collects domain flags from leaf types
    /// and function calls. Classification rules:
    /// - If any geometry-related function call → Geometric (or CrossDomain if mixed with logical)
    /// - If all leaves are numeric (Scalar, Real, Int) → Dimensional
    /// - If all leaves are Bool with only logical ops → Logical
    /// - If mixed numeric + logical → CrossDomain
    pub fn classify(expr: &CompiledExpr) -> ConstraintDomain {
        let mut flags = DomainFlags::default();
        Self::collect_flags(expr, &mut flags);
        flags.into_domain()
    }

    /// Recursively walk the expression tree, collecting domain flags.
    fn collect_flags(expr: &CompiledExpr, flags: &mut DomainFlags) {
        match &expr.kind {
            CompiledExprKind::Literal(value) => {
                match value {
                    Value::Bool(_) => flags.has_logical = true,
                    Value::Int(_) | Value::Real(_) | Value::Scalar { .. } => {
                        flags.has_numeric = true;
                    }
                    Value::String(_) | Value::Undef => {
                        // String and Undef don't contribute to domain classification
                    }
                }
            }
            CompiledExprKind::ValueRef(_) => {
                // Classify based on the result type of the reference
                match &expr.result_type {
                    Type::Bool => flags.has_logical = true,
                    Type::Int | Type::Real | Type::Scalar { .. } | Type::String => {
                        flags.has_numeric = true;
                    }
                }
            }
            CompiledExprKind::BinOp { left, right, .. } => {
                Self::collect_flags(left, flags);
                Self::collect_flags(right, flags);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                Self::collect_flags(operand, flags);
            }
            CompiledExprKind::FunctionCall { function, args } => {
                if is_geometry_qualified_name(&function.qualified_name) {
                    flags.has_geometric = true;
                }
                for arg in args {
                    Self::collect_flags(arg, flags);
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                Self::collect_flags(condition, flags);
                Self::collect_flags(then_branch, flags);
                Self::collect_flags(else_branch, flags);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{BinOp, ContentHash, DimensionVector};

    #[test]
    fn literal_int_is_numeric() {
        let expr = CompiledExpr::literal(Value::Int(42), Type::Int);
        assert_eq!(ConstraintClassifier::classify(&expr), ConstraintDomain::Dimensional);
    }

    #[test]
    fn literal_bool_is_logical() {
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        assert_eq!(ConstraintClassifier::classify(&expr), ConstraintDomain::Logical);
    }

    #[test]
    fn literal_scalar_is_dimensional() {
        let expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.01,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        assert_eq!(ConstraintClassifier::classify(&expr), ConstraintDomain::Dimensional);
    }

    #[test]
    fn empty_undef_defaults_to_dimensional() {
        let expr = CompiledExpr::literal(Value::Undef, Type::Bool);
        // Undef doesn't set any flags → default is Dimensional
        assert_eq!(ConstraintClassifier::classify(&expr), ConstraintDomain::Dimensional);
    }

    #[test]
    fn geometry_function_sets_geometric_flag() {
        use reify_types::ResolvedFunction;
        let expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "distance".to_string(),
                    qualified_name: "std::geo::distance".to_string(),
                },
                args: vec![],
            },
            result_type: Type::Real,
            content_hash: ContentHash::of(b"test"),
        };
        assert_eq!(ConstraintClassifier::classify(&expr), ConstraintDomain::Geometric);
    }

    #[test]
    fn mixed_numeric_and_bool_is_cross_domain() {
        let num = CompiledExpr::literal(Value::Int(1), Type::Int);
        let boolean = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, num, boolean, Type::Bool);
        assert_eq!(ConstraintClassifier::classify(&expr), ConstraintDomain::CrossDomain);
    }
}
