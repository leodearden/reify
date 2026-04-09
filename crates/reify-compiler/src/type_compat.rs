use super::*;

pub fn implicitly_converts_to(from: &Type, to: &Type) -> bool {
    // Identity: same type always converts to itself.
    if from == to {
        return true;
    }

    match (from, to) {
        // Rule 1a: Vector<N,Q> -> Tensor<1,N,Q>
        (
            Type::Vector {
                n: vn,
                quantity: vq,
            },
            Type::Tensor {
                rank: 1,
                n: tn,
                quantity: tq,
            },
        ) => vn == tn && vq == tq,

        // Rule 1b: Tensor<1,N,Q> -> Vector<N,Q>
        (
            Type::Tensor {
                rank: 1,
                n: tn,
                quantity: tq,
            },
            Type::Vector {
                n: vn,
                quantity: vq,
            },
        ) => tn == vn && tq == vq,

        // Rule 2a: Q -> Tensor<0,_,Q>  (N is irrelevant for rank-0)
        (
            from_ty,
            Type::Tensor {
                rank: 0,
                quantity: tq,
                ..
            },
        ) => from_ty == tq.as_ref(),

        // Rule 2b: Tensor<0,_,Q> -> Q  (N is irrelevant for rank-0)
        (
            Type::Tensor {
                rank: 0,
                quantity: tq,
                ..
            },
            to_ty,
        ) => tq.as_ref() == to_ty,

        // Rule 3: Tensor<2,N,Q> -> Matrix<N,N,Q>  (one-way, square matrices only)
        // Note: Matrix->Tensor is NOT allowed; the default `false` arm handles that.
        (
            Type::Tensor {
                rank: 2,
                n: tn,
                quantity: tq,
            },
            Type::Matrix {
                m,
                n: mn,
                quantity: mq,
            },
        ) => tn == m && tn == mn && tq == mq,

        _ => false,
    }
}

/// Check if an argument type is compatible with a parameter type.
/// Exact match always works. Int→Real widening is allowed.
/// Implicit tensor/vector/matrix conversions are also checked (bidirectional).
///
/// Not used in overload resolution (which uses exact matching), but used
/// in trait conformance and field composition checks.
pub fn type_compatible(param_ty: &Type, arg_ty: &Type) -> bool {
    if param_ty == arg_ty {
        return true;
    }
    // Allow Int→Real widening coercion
    if matches!((param_ty, arg_ty), (Type::Real, Type::Int)) {
        return true;
    }
    // Bidirectional implicit tensor/vector/matrix conversions
    if implicitly_converts_to(param_ty, arg_ty) || implicitly_converts_to(arg_ty, param_ty) {
        return true;
    }
    false
}

/// Result of attempting to resolve a function call against user-defined functions.
pub(crate) enum OverloadResolution<'a> {
    /// Exactly one user-defined function matches by name, arity, and exact param types.
    Resolved(&'a CompiledFunction),
    /// No user-defined function has this name at all — fall through to stdlib.
    NoUserFunctions,
    /// User-defined functions with this name exist, but none match the given arg types.
    /// Carries all same-name candidates for error reporting.
    NoMatch(Vec<&'a CompiledFunction>),
    /// Multiple user-defined functions match — ambiguous call.
    /// Carries all matching candidates for error reporting.
    Ambiguous(Vec<&'a CompiledFunction>),
}

/// Resolve a function call against the list of compiled user functions.
///
/// Uses **exact** type matching (param_ty == arg_ty). Int→Real widening is
/// NOT applied during overload resolution so that `f(Int)` and `f(Real)` are
/// treated as distinct overloads.
pub(crate) fn resolve_function_overload<'a>(
    name: &str,
    arg_types: &[Type],
    functions: &'a [CompiledFunction],
) -> OverloadResolution<'a> {
    // All user functions with the given name (for error reporting).
    let named: Vec<&CompiledFunction> = functions.iter().filter(|f| f.name == name).collect();

    if named.is_empty() {
        return OverloadResolution::NoUserFunctions;
    }

    // Among named functions, filter by arity and exact param types.
    let matches: Vec<&CompiledFunction> = named
        .iter()
        .copied()
        .filter(|f| {
            f.params.len() == arg_types.len()
                && f.params
                    .iter()
                    .zip(arg_types.iter())
                    .all(|((_, param_ty), arg_ty)| param_ty == arg_ty)
        })
        .collect();

    match matches.len() {
        1 => OverloadResolution::Resolved(matches[0]),
        0 => OverloadResolution::NoMatch(named),
        _ => OverloadResolution::Ambiguous(matches),
    }
}

/// Format a function signature for error messages: `name(T1, T2) -> Ret`.
pub(crate) fn format_fn_signature(f: &CompiledFunction) -> String {
    format!(
        "{}({}) -> {}",
        f.name,
        f.params
            .iter()
            .map(|(_, t)| format!("{}", t))
            .collect::<Vec<_>>()
            .join(", "),
        f.return_type
    )
}

// --- Chained comparison helpers ---

/// Returns true if `op` is a comparison operator that participates in chaining.
pub(crate) fn is_comparison_op(op: &str) -> bool {
    matches!(op, "<" | "<=" | ">" | ">=" | "==" | "!=")
}

/// Flatten a left-nested comparison chain into (operands, operators).
///
/// Given `BinOp(op2, BinOp(op1, a, b), c)` where both op1 and op2 are comparison
/// operators, returns `([a, b, c], [op1, op2])`.
///
/// `outer_op`, `left`, and `right` are the components of the outermost BinOp.
/// Precondition: `outer_op` is a comparison op and `left` is a comparison BinOp.
pub(crate) fn flatten_comparison_chain<'a>(
    outer_op: &'a str,
    left: &'a reify_syntax::Expr,
    right: &'a reify_syntax::Expr,
) -> (Vec<&'a reify_syntax::Expr>, Vec<&'a str>) {
    match &left.kind {
        reify_syntax::ExprKind::BinOp {
            op: inner_op,
            left: ll,
            right: lr,
        } if is_comparison_op(inner_op) => {
            // Recurse: flatten the left subtree first, then append current right and op
            let (mut operands, mut ops) = flatten_comparison_chain(inner_op, ll, lr);
            operands.push(right);
            ops.push(outer_op);
            (operands, ops)
        }
        _ => {
            // Base case: left is not a comparison chain; operands = [left, right], ops = [outer_op]
            (vec![left, right], vec![outer_op])
        }
    }
}

// --- BinOp resolution ---

/// Parse a string operator into a `BinOp`.
pub(crate) fn resolve_binop(op: &str) -> Option<BinOp> {
    match op {
        "+" => Some(BinOp::Add),
        "-" => Some(BinOp::Sub),
        "*" => Some(BinOp::Mul),
        "/" => Some(BinOp::Div),
        "%" => Some(BinOp::Mod),
        "**" | "^" => Some(BinOp::Pow),
        "==" => Some(BinOp::Eq),
        "!=" => Some(BinOp::Ne),
        "<" => Some(BinOp::Lt),
        "<=" => Some(BinOp::Le),
        ">" => Some(BinOp::Gt),
        ">=" => Some(BinOp::Ge),
        "&&" | "and" => Some(BinOp::And),
        "||" | "or" => Some(BinOp::Or),
        _ => None,
    }
}

/// Parse a string unary operator into a `UnOp`.
pub(crate) fn resolve_unop(op: &str) -> Option<UnOp> {
    match op {
        "-" => Some(UnOp::Neg),
        "!" | "not" => Some(UnOp::Not),
        _ => None,
    }
}

// --- Type inference for binary operations ---

/// Infer the result type of a binary operation given operand types.
pub(crate) fn infer_binop_type(op: BinOp, left: &Type, right: &Type) -> Type {
    match op {
        BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::And
        | BinOp::Or => Type::Bool,
        BinOp::Add | BinOp::Sub => left.clone(), // same dimension required
        BinOp::Mul => match (left, right) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => Type::Scalar {
                dimension: ld.mul(rd),
            },
            (Type::Scalar { .. }, _) | (_, Type::Scalar { .. }) => {
                // Scalar * non-scalar preserves the scalar type
                if let Type::Scalar { .. } = left {
                    left.clone()
                } else {
                    right.clone()
                }
            }
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Div => match (left, right) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => {
                let result = ld.div(rd);
                if result.is_dimensionless() {
                    Type::Real
                } else {
                    Type::Scalar { dimension: result }
                }
            }
            (Type::Scalar { .. }, _) => left.clone(),
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Mod => left.clone(),
        BinOp::Pow => left.clone(), // simplified for M1
    }
}
