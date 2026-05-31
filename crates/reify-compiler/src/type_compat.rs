use super::*;

/// Returns `true` if `ty` is a scalar-like leaf type eligible as the `Q`
/// (quantity) side of Rules 2a/2b/2c.
///
/// The spec's "Q is a single value" framing covers the following leaf kinds:
/// plain primitives (`Bool`, `Int`, `Real`, `String`), dimensioned scalars
/// (`Scalar`), enumerations (`Enum`), type parameters (`TypeParam`),
/// user-defined structure references (`StructureRef`), trait objects
/// (`TraitObject`), and the geometry sentinel (`Geometry`).
///
/// Compound/aggregate types (`Vector`, `Tensor`, `Matrix`, `Point`, `List`,
/// `Set`, `Map`, `Option`, `Complex`, `Field`, `Range`, `Function`, `Frame`,
/// `Transform`, `Plane`, `Orientation`, `Axis`, `BoundingBox`) are NOT leaf
/// types and return `false`.
///
/// **Spec reference:** `docs/reify-language-spec.md` §3.3.1 (lines 295–329).
/// Specifically:
/// - Lines 298–301: `Scalar<Q: Dimension>` is defined as an independent rank-0
///   type without spatial dimensionality. Q must be a "Dimension" — a single
///   dimensioned value, not a compound/aggregate carrier of shape.
/// - Line 305: "**Tensor conversion:** `Scalar<Q>` converts implicitly to
///   `Tensor<0, N, Q>` for any `N`, and vice versa." This is the basis of
///   Rules 2a/2b; it only holds when Q is a leaf Dimension type.
/// - Lines 317–320: restates the alias relationship and notes
///   `Vector<N,Q> = Tensor<1,N,Q>`.
///
/// This allowlist (not a denylist) is intentional: future `Type` variants
/// default to *rejected* rather than default-admitted, forcing each new
/// variant to be explicitly evaluated against the spec's "Q is a Dimension /
/// single value" criterion before being added here.
///
/// `Type::Error` is excluded: the anti-cascade guard at the top of
/// `implicitly_converts_to` short-circuits before any leaf check is reached,
/// so Error inputs never arrive here.
fn is_scalar_like_leaf(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Bool
            | Type::Int
            | Type::Real
            | Type::String
            | Type::Scalar { .. }
            | Type::Enum(_)
            | Type::TypeParam(_)
            | Type::StructureRef(_)
            | Type::TraitObject(_)
            | Type::Geometry
    )
}

pub fn implicitly_converts_to(from: &Type, to: &Type) -> bool {
    // Anti-cascade guard — asymmetric error-wildcard contract (task-448 / task-1918).
    //
    // PRODUCER side (`from.is_error()`): legitimate anti-cascade path. When the
    // producing expression already emitted a diagnostic, its Type::Error sentinel
    // must be accepted everywhere to suppress follow-on "type mismatch" reports at
    // downstream call sites (trait conformance, function-argument checks).
    //
    // CONSUMER side (`to.is_error()`): declared annotations are resolved via
    // `resolve_type_with_aliases`, which always falls back to a concrete type
    // (e.g. Type::Real, Type::StructureRef) — Type::Error never legitimately
    // appears as the expected/declared type. The debug_assert below catches any
    // call site that accidentally passes Error as `to` (a bug, not a cascade).
    // In release builds the short-circuit preserves cascade safety as a
    // belt-and-braces fallback (task-448 rationale).
    debug_assert!(
        !to.is_error(),
        "Type::Error must not appear on the consumer/target side of implicitly_converts_to \
         — declared annotations never resolve to the poison sentinel; \
         this indicates a bug at the call site (task-1918)"
    );
    if from.is_error() || to.is_error() {
        return true;
    }

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

        // Rule 2c: Tensor<0,M,Q> -> Tensor<0,N,Q>  (same Q, any N — N irrelevant for rank-0)
        //
        // Spec rationale: rank-0 tensors are semantically scalar-like; their N dimension
        // carries no indexable information. By transitivity of Rules 2a/2b, if both
        // `Q → Tensor<0,M,Q>` and `Q → Tensor<0,N,Q>` hold, direct
        // `Tensor<0,M,Q> → Tensor<0,N,Q>` must also hold. Without this rule a trait
        // requiring `Tensor<0,5,Q>` would reject a structure providing `Tensor<0,3,Q>`
        // despite them being semantically identical.
        //
        // Guard: `is_scalar_like_leaf(q1)` mirrors the leaf-Q guard on Rules 2a/2b.
        // The transitivity argument only holds when Rules 2a/2b themselves fire (i.e.
        // when Q is a scalar-like leaf). Compound-Q pairs (e.g. Vector, Point) are
        // rejected consistently with Rules 2a/2b. Checking q1 alone is sufficient;
        // `q1 == q2` implies q2 is the same leaf.
        (
            Type::Tensor {
                rank: 0,
                quantity: q1,
                ..
            },
            Type::Tensor {
                rank: 0,
                quantity: q2,
                ..
            },
        ) if is_scalar_like_leaf(q1) => q1 == q2,

        // Rule 2a: Q -> Tensor<0,_,Q>  (N is irrelevant for rank-0)
        //
        // Guard: `from_ty` must be a scalar-like leaf type — see `is_scalar_like_leaf`.
        // Compound/aggregate types are excluded: the spec's "Q is a single value" framing
        // covers only leaf kinds (Bool, Int, Real, String, Scalar, Enum, TypeParam,
        // StructureRef, TraitObject, Geometry). Rule 2c (above) handles Tensor<0>↔Tensor<0>.
        (
            from_ty,
            Type::Tensor {
                rank: 0,
                quantity: tq,
                ..
            },
        ) if is_scalar_like_leaf(from_ty) => from_ty == tq.as_ref(),

        // Rule 2b: Tensor<0,_,Q> -> Q  (N is irrelevant for rank-0)
        //
        // Guard: `to_ty` must be a scalar-like leaf type — see `is_scalar_like_leaf`.
        (
            Type::Tensor {
                rank: 0,
                quantity: tq,
                ..
            },
            to_ty,
        ) if is_scalar_like_leaf(to_ty) => tq.as_ref() == to_ty,

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

/// Check if an argument type is compatible with a declared parameter/annotation type.
///
/// Returns `true` when `arg_ty` can be used where `param_ty` is declared, under
/// any of the following rules:
/// - **Identity**: `param_ty == arg_ty` (delegated to `implicitly_converts_to`).
/// - **Int→Real widening**: whole-number literals parse as `Int` and must be
///   accepted where `Real` is annotated (e.g. `let x : Real = 42` at
///   `conformance.rs:591`).
/// - **Bidirectional implicit conversions**: calls `implicitly_converts_to` in
///   **both** directions (`param→arg` and `arg→param`), so the explicitly
///   one-way Rule 3 (`Tensor<2,N,Q>→Matrix<N,N,Q>`) appears symmetric here.
///   This is intentional for trait-let-binding annotation checks
///   (`conformance.rs:591`), where either annotation direction must be accepted.
///
/// # When to use `implicitly_converts_to` directly
///
/// **Use `implicitly_converts_to` directly when direction matters:**
/// - Trait member conformance (`conformance.rs:384`): producer type must convert
///   *to* the trait's declared type — direction is fixed.
/// - Field composition (`functions.rs:289`): inner codomain must convert *to*
///   outer domain — direction is fixed.
///
/// Using `type_compatible` at those sites would silently accept
/// `Matrix<3,3,Q>→Tensor<2,3,Q>` even though Rule 3 is one-way.
///
/// # Error-wildcard contract (task-448 / task-1918)
///
/// `arg_ty.is_error()` is the **producer-side** anti-cascade path: when the
/// argument expression already emitted a diagnostic, its Type::Error sentinel
/// must be accepted to suppress follow-on "type mismatch" reports.
///
/// `param_ty.is_error()` **must never legitimately occur**: production call sites
/// pass types that originate from `resolve_type_with_aliases`, which always falls
/// back to a concrete type (e.g. `Type::Real`, `Type::StructureRef`) and never
/// returns `Type::Error`. The debug_assert below catches any future regression,
/// including the two recursive calls in the body below (both safe by the same
/// invariants). In release builds the short-circuit preserves cascade safety as
/// a belt-and-braces fallback (task-448 rationale).
pub fn type_compatible(param_ty: &Type, arg_ty: &Type) -> bool {
    // Producer-side anti-cascade guard (task-448 / task-1918): asymmetric contract.
    // See doc comment above for full rationale.
    debug_assert!(
        !param_ty.is_error(),
        "Type::Error must not appear on the param/expected side of type_compatible \
         — declared annotations never resolve to the poison sentinel; \
         this indicates a bug at the call site (task-1918)"
    );
    if param_ty.is_error() || arg_ty.is_error() {
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

/// Check that a function-param default expression's type is compatible with the
/// declared parameter type.
///
/// **Policy: strict equality, not bidirectional `type_compatible`.**
///
/// Call-site overload resolution (`resolve_function_overload`) and
/// `try_default_padding`'s prefix check both use exact type equality — `f(1)` is
/// already rejected today for `fn f(x: Real)` because `Type::Int != Type::Real`.
/// A default value is conceptually inserted at the padded call site, so the
/// definition-site check must be at least as strict as the call-site check;
/// otherwise a default could synthesize an argument that an explicit call would
/// refuse, creating a type-system inconsistency.
///
/// **Anti-cascade guard.** If either type is `Type::Error` (poison sentinel from
/// a failed `compile_expr`), silently accept — the root-cause diagnostic was
/// already emitted. Mirrors the same short-circuit in `implicitly_converts_to`
/// and `type_compatible` (task-448 / task-1918 cascade-safety contract).
///
/// Note: `param_ty` is always a concrete resolved type (never `Type::Error`) in
/// production — `resolve_type_expr_with_aliases` always falls back to `Type::Real`
/// on failure. The `param_ty.is_error()` branch is therefore dead code in practice
/// but is included for symmetry and belt-and-braces safety.
pub(crate) fn fn_param_default_compatible(param_ty: &Type, default_ty: &Type) -> bool {
    if param_ty.is_error() || default_ty.is_error() {
        return true;
    }
    param_ty == default_ty
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

// --- Dimension-mismatch diagnostic helpers ---

/// Build the canonical dimension-mismatch error diagnostic.
///
/// Produces `"dimension mismatch in {op_name}: {left_ty} vs {right_ty}"` with
/// `DiagnosticCode::DimensionMismatch` and the primary `"incompatible dimensions"` label.
///
/// When BOTH operands are `Type::Scalar` with a canonical name (see
/// `DimensionVector::canonical_name`) and the two names differ, attaches a
/// secondary label of the form `"<LName> and <RName> are different dimensions
/// and cannot be combined directly"` so the user sees the human-readable
/// dimension name rather than just the unit-symbol form.
pub(crate) fn format_dimension_mismatch_diagnostic(
    op_name: &str,
    left_ty: &Type,
    right_ty: &Type,
    span: SourceSpan,
) -> Diagnostic {
    // Compute the optional secondary label before building the diagnostic so
    // there is a single exit point and no early return.
    let secondary: Option<DiagnosticLabel> =
        if let (Type::Scalar { dimension: ldim }, Type::Scalar { dimension: rdim }) =
            (left_ty, right_ty)
            && let (Some(lname), Some(rname)) = (ldim.canonical_name(), rdim.canonical_name())
            && lname != rname
        {
            Some(DiagnosticLabel::new(
                span,
                format!(
                    "{lname} and {rname} are different dimensions and cannot be combined directly"
                ),
            ))
        } else {
            None
        };

    let mut d = Diagnostic::error(format!(
        "dimension mismatch in {op_name}: {left_ty} vs {right_ty}"
    ))
    .with_code(DiagnosticCode::DimensionMismatch)
    .with_label(DiagnosticLabel::new(span, "incompatible dimensions"));

    if let Some(label) = secondary {
        d = d.with_label(label);
    }

    d
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
    left: &'a reify_ast::Expr,
    right: &'a reify_ast::Expr,
) -> (Vec<&'a reify_ast::Expr>, Vec<&'a str>) {
    match &left.kind {
        reify_ast::ExprKind::BinOp {
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
        "implies" => Some(BinOp::Implies),
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
    // Anti-cascade guard (task-448): if either operand is already poisoned,
    // propagate Type::Error so downstream sites don't emit follow-on diagnostics.
    if left.is_error() || right.is_error() {
        return Type::Error;
    }
    match op {
        BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::And
        | BinOp::Or
        | BinOp::Implies => Type::Bool,
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

/// Attempt to satisfy a `NoMatch` call via default-padding.
///
/// Searches `named` for the UNIQUE same-name candidate where:
/// - the candidate has more params than `provided` args,
/// - the provided prefix `arg_types[..provided]` matches `cand.params[..provided]` exactly, and
/// - every trailing `cand.param_defaults[provided..]` is `Some`.
///
/// `provided` is `arg_types.len()` — callers no longer pass `compiled_args`
/// because only its length was used and `arg_types` is always length-aligned
/// to `compiled_args` by construction (task-3702).
///
/// If exactly one such candidate exists, returns it together with the cloned default
/// `CompiledExpr`s for the trailing params. Returns `None` when zero or multiple
/// candidates are satisfiable (caller falls through to the existing NoMatch error).
///
/// **Invariant:** every candidate in `named` must satisfy
/// `param_defaults.len() == params.len()` (task-3702 strict alignment now
/// enforced by `debug_assert!`). Violations are programming errors, not
/// recoverable call-site conditions.
pub(crate) fn try_default_padding<'a>(
    named: &[&'a CompiledFunction],
    arg_types: &[Type],
) -> Option<(&'a CompiledFunction, Vec<CompiledExpr>)> {
    let provided = arg_types.len();
    let mut satisfiable: Vec<(&CompiledFunction, Vec<CompiledExpr>)> = Vec::new();

    for &cand in named {
        // Candidate must have strictly more params than provided args.
        if cand.params.len() <= provided {
            continue;
        }
        // Strict invariant: param_defaults must be length-aligned to params.
        // Violations are bugs — surface them in debug builds. In release builds
        // the assert is compiled out, so we also `continue` on mismatch to
        // degrade gracefully instead of panicking on a future invariant-breaking
        // producer (task-3702 amendment-2).
        debug_assert!(
            cand.param_defaults.len() == cand.params.len(),
            "param_defaults.len() == params.len() invariant violated for candidate `{}` (task-3702): expected {}, got {}",
            cand.name,
            cand.params.len(),
            cand.param_defaults.len()
        );
        if cand.param_defaults.len() != cand.params.len() {
            continue;
        }
        // Provided prefix types must match candidate params exactly.
        let prefix_matches = cand.params[..provided]
            .iter()
            .zip(arg_types[..provided].iter())
            .all(|((_, param_ty), arg_ty)| param_ty == arg_ty);
        if !prefix_matches {
            continue;
        }
        // All trailing params must carry Some compiled default.
        let defaults: Option<Vec<CompiledExpr>> = cand.param_defaults[provided..]
            .iter()
            .cloned()
            .collect();
        if let Some(defaults) = defaults {
            satisfiable.push((cand, defaults));
        }
    }

    match satisfiable.len() {
        1 => Some(satisfiable.into_iter().next().unwrap()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! Anti-cascade guard tests (task-448): `Type::Error` operands must
    //! propagate `Type::Error`, not fall back to any op-specific result type.
    //!
    //! Renamed from `infer_binop_type_error_tests` per amendment-round-2 S5
    //! to match the codebase-standard `mod tests` convention.
    use super::*;

    // --- format_dimension_mismatch_diagnostic tests (step-5) ---

    fn test_span() -> SourceSpan {
        SourceSpan::new(0, 10)
    }

    fn money_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::MONEY,
        }
    }

    fn force_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::FORCE,
        }
    }

    fn length_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }
    }

    fn mass_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::MASS,
        }
    }

    /// (a) Money-vs-Force produces a secondary label naming both dimensions.
    #[test]
    fn fmt_dim_mismatch_money_vs_force_has_secondary_label() {
        let d =
            format_dimension_mismatch_diagnostic("addition", &money_ty(), &force_ty(), test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
        assert!(
            d.message.contains("dimension mismatch in addition:"),
            "message was: {}",
            d.message
        );
        assert!(
            d.labels.len() >= 2,
            "expected at least 2 labels, got {}",
            d.labels.len()
        );
        let has_canonical_hint = d
            .labels
            .iter()
            .any(|l| l.message.contains("Money") && l.message.contains("Force"));
        assert!(
            has_canonical_hint,
            "no label mentions both 'Money' and 'Force'; labels: {:?}",
            d.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// (b) Reverse polarity (Force on left, Money on right) produces the same secondary label.
    #[test]
    fn fmt_dim_mismatch_force_vs_money_has_secondary_label() {
        let d =
            format_dimension_mismatch_diagnostic("addition", &force_ty(), &money_ty(), test_span());
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
        let has_canonical_hint = d
            .labels
            .iter()
            .any(|l| l.message.contains("Money") && l.message.contains("Force"));
        assert!(
            has_canonical_hint,
            "no label mentions both 'Money' and 'Force'; labels: {:?}",
            d.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// (c) Length-vs-Mass produces secondary label naming both.
    #[test]
    fn fmt_dim_mismatch_length_vs_mass_has_secondary_label() {
        let d =
            format_dimension_mismatch_diagnostic("addition", &length_ty(), &mass_ty(), test_span());
        let has_canonical_hint = d
            .labels
            .iter()
            .any(|l| l.message.contains("Length") && l.message.contains("Mass"));
        assert!(
            has_canonical_hint,
            "no label mentions both 'Length' and 'Mass'; labels: {:?}",
            d.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// (d) Composite-vs-named produces ONLY the primary "incompatible dimensions" label (no canonical-names hint),
    /// but still attaches the code.
    #[test]
    fn fmt_dim_mismatch_composite_vs_named_no_secondary_label() {
        let composite = Type::Scalar {
            dimension: DimensionVector::MONEY.div(&DimensionVector::MASS),
        };
        let d =
            format_dimension_mismatch_diagnostic("addition", &composite, &force_ty(), test_span());
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
        // There should be exactly one label (the primary "incompatible dimensions" label).
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly 1 label for composite-vs-named, got {}",
            d.labels.len()
        );
        assert_eq!(d.labels[0].message, "incompatible dimensions");
    }

    /// (e) Non-Scalar operands do not panic and still produce a diagnostic with code.
    /// Covers the three asymmetric/symmetric non-Scalar cases the helper may receive:
    /// (Real, Scalar), (Scalar, Real), and (Real, Real).
    #[test]
    fn fmt_dim_mismatch_non_scalar_does_not_panic() {
        // Left non-Scalar, right Scalar
        let d =
            format_dimension_mismatch_diagnostic("addition", &Type::Real, &force_ty(), test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));

        // Left Scalar, right non-Scalar
        let d =
            format_dimension_mismatch_diagnostic("addition", &money_ty(), &Type::Real, test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));

        // Both non-Scalar
        let d =
            format_dimension_mismatch_diagnostic("addition", &Type::Real, &Type::Real, test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
    }

    #[test]
    fn binop_add_left_error_yields_error() {
        assert_eq!(
            infer_binop_type(BinOp::Add, &Type::Error, &Type::Int),
            Type::Error,
        );
    }

    #[test]
    fn binop_mul_right_error_yields_error() {
        assert_eq!(
            infer_binop_type(BinOp::Mul, &Type::Real, &Type::Error),
            Type::Error,
        );
    }

    #[test]
    fn binop_lt_error_operand_yields_error_not_bool() {
        // Comparison ops would normally produce Type::Bool — the error must win.
        assert_eq!(
            infer_binop_type(BinOp::Lt, &Type::Error, &Type::Int),
            Type::Error,
        );
    }

    /// Exhaustive BinOp coverage (amendment-round-2 S3): every variant of
    /// `BinOp` must propagate `Type::Error` when either operand is poisoned.
    /// This pins down the anti-cascade contract for the full enum, not just
    /// the three representatives spot-checked above. Update this list (and
    /// the inner match in `infer_binop_type`) together if a new BinOp arm
    /// is added.
    #[test]
    fn every_binop_variant_propagates_error_from_either_operand() {
        // Compile-time exhaustiveness guard: adding a new BinOp variant to
        // the enum is a build error here until the `ops` list below is also
        // updated. Keeps the test's enumeration honest as the enum grows.
        #[allow(dead_code)]
        fn _exhaustive_binop_check(op: BinOp) {
            match op {
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::Pow
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::And
                | BinOp::Or
                | BinOp::Implies => {}
            }
        }
        // (op, expected_non_error_result_for_(Real, Real))_label — the second
        // tuple element is just a documentation aid for the reviewer; we
        // never assert on it. We only assert that, with at least one operand
        // poisoned, the result is Type::Error.
        let ops: &[(BinOp, &str)] = &[
            (BinOp::Add, "arithmetic: left.clone()"),
            (BinOp::Sub, "arithmetic: left.clone()"),
            (BinOp::Mul, "arithmetic: scalar/widening rules"),
            (BinOp::Div, "arithmetic: scalar/widening rules"),
            (BinOp::Mod, "arithmetic: left.clone()"),
            (BinOp::Pow, "arithmetic: left.clone()"),
            (BinOp::Eq, "comparison: Bool"),
            (BinOp::Ne, "comparison: Bool"),
            (BinOp::Lt, "comparison: Bool"),
            (BinOp::Le, "comparison: Bool"),
            (BinOp::Gt, "comparison: Bool"),
            (BinOp::Ge, "comparison: Bool"),
            (BinOp::And, "logical: Bool"),
            (BinOp::Or, "logical: Bool"),
            (BinOp::Implies, "logical: Bool"),
        ];
        for (op, label) in ops {
            assert_eq!(
                infer_binop_type(*op, &Type::Error, &Type::Real),
                Type::Error,
                "BinOp::{:?} ({}) failed to propagate Type::Error from LEFT operand",
                op,
                label,
            );
            assert_eq!(
                infer_binop_type(*op, &Type::Real, &Type::Error),
                Type::Error,
                "BinOp::{:?} ({}) failed to propagate Type::Error from RIGHT operand",
                op,
                label,
            );
            assert_eq!(
                infer_binop_type(*op, &Type::Error, &Type::Error),
                Type::Error,
                "BinOp::{:?} ({}) failed to propagate Type::Error when BOTH operands poisoned",
                op,
                label,
            );
        }
    }

    // ── BinOp::Implies wiring (task-3921) ────────────────────────────────────

    #[test]
    fn resolve_binop_implies_keyword() {
        assert_eq!(resolve_binop("implies"), Some(BinOp::Implies));
    }

    #[test]
    fn infer_binop_implies_bool_bool_yields_bool() {
        assert_eq!(
            infer_binop_type(BinOp::Implies, &Type::Bool, &Type::Bool),
            Type::Bool,
        );
    }

    #[test]
    fn infer_binop_implies_left_error_propagates() {
        assert_eq!(
            infer_binop_type(BinOp::Implies, &Type::Error, &Type::Bool),
            Type::Error,
        );
    }

    #[test]
    fn infer_binop_implies_right_error_propagates() {
        assert_eq!(
            infer_binop_type(BinOp::Implies, &Type::Bool, &Type::Error),
            Type::Error,
        );
    }

    // ── task-3702 tests ───────────────────────────────────────────────────────

    /// Helper: build a minimal `CompiledFnBody` returning a Real(2.0) literal.
    fn stub_body_real() -> CompiledFnBody {
        CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Real(2.0), Type::Real),
        }
    }

    /// `try_default_padding` with the tightened signature (no `compiled_args`
    /// argument) returns the expected candidate and default expressions when
    /// exactly one candidate satisfies the padding contract.
    ///
    /// Candidate: `f(x: Real, y: Real)` where param 1 (`y`) has default
    /// `Real(2.0)`. Caller provides 1 arg of type `Real` — the trailing
    /// default must be filled in.
    ///
    /// Expected: `Some((&cand, vec![Real(2.0) literal]))`.
    ///
    /// RED before step-5: the current `try_default_padding` signature still
    /// requires a `compiled_args: &[CompiledExpr]` second argument, so this
    /// call (with only 2 positional args) fails to compile.
    ///
    /// task-3702 (tighten try_default_padding signature)
    #[test]
    fn try_default_padding_new_signature_returns_padded_fn() {
        let default_expr = CompiledExpr::literal(Value::Real(2.0), Type::Real);
        let cand = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("x".to_string(), Type::Real),
                ("y".to_string(), Type::Real),
            ],
            param_defaults: vec![None, Some(default_expr.clone())],
            return_type: Type::Real,
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_stub_3702"),
            annotations: vec![],
            optimized_target: None,
        };

        // New signature: no compiled_args — only arg_types.
        let result = try_default_padding(&[&cand], &[Type::Real]);

        let (matched_fn, defaults) = result.expect("should find a matching candidate");
        assert!(
            std::ptr::eq(matched_fn, &cand),
            "returned candidate must be the same object"
        );
        assert_eq!(defaults.len(), 1, "one trailing default expected");
        assert_eq!(
            defaults[0].content_hash, default_expr.content_hash,
            "default expr content hash must match the Real(2.0) literal"
        );
    }

    /// `try_default_padding` fires a `debug_assert!` (panics in debug builds)
    /// when a candidate violates the length invariant
    /// (`param_defaults.len() != params.len()`).
    ///
    /// This is the "bad shape" that was previously silently skipped by the
    /// defensive filter; after task-3702 it is a programming error surfaced in
    /// debug builds.
    ///
    /// Candidate: deliberately constructed via struct-literal with
    /// `params = vec![("x", Real)]` but `param_defaults = Vec::new()` —
    /// the legacy empty form that violates the invariant.
    ///
    /// task-3702 (tighten try_default_padding signature)
    // ── modulo_operands_are_int predicate (task-3916) ────────────────────────

    /// `(Int, Int)` is the one valid modulo shape → `true`.
    #[test]
    fn modulo_operands_int_int_is_true() {
        assert!(modulo_operands_are_int(&Type::Int, &Type::Int));
    }

    /// `(Real, Int)` is rejected (left is Real) → `false`.
    #[test]
    fn modulo_operands_real_int_is_false() {
        assert!(!modulo_operands_are_int(&Type::Real, &Type::Int));
    }

    /// `(Int, Real)` is rejected (right is Real) → `false`.
    #[test]
    fn modulo_operands_int_real_is_false() {
        assert!(!modulo_operands_are_int(&Type::Int, &Type::Real));
    }

    /// `(Real, Real)` — both wrong → `false`.
    #[test]
    fn modulo_operands_real_real_is_false() {
        assert!(!modulo_operands_are_int(&Type::Real, &Type::Real));
    }

    /// `(Scalar{LENGTH}, Scalar{LENGTH})` — dimensioned types are not Int → `false`.
    #[test]
    fn modulo_operands_scalar_scalar_is_false() {
        assert!(!modulo_operands_are_int(&length_ty(), &length_ty()));
    }

    /// `(Scalar{LENGTH}, Int)` — left is dimensioned → `false`.
    #[test]
    fn modulo_operands_scalar_int_is_false() {
        assert!(!modulo_operands_are_int(&length_ty(), &Type::Int));
    }

    /// `(Bool, Int)` — Bool is not Int → `false`.
    #[test]
    fn modulo_operands_bool_int_is_false() {
        assert!(!modulo_operands_are_int(&Type::Bool, &Type::Int));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "param_defaults.len() == params.len()")]
    fn try_default_padding_debug_assert_fires_on_misaligned_param_defaults() {
        // Deliberately bad shape: params has 1 entry, param_defaults is empty.
        let bad_cand = CompiledFunction {
            name: "bad".to_string(),
            doc: None,
            is_pub: false,
            params: vec![("x".to_string(), Type::Real)],
            param_defaults: Vec::new(), // invariant violation — intentional for this test
            return_type: Type::Real,
            body: stub_body_real(),
            content_hash: ContentHash::of_str("bad_stub_3702"),
            annotations: vec![],
            optimized_target: None,
        };

        // New signature: (named, arg_types). Providing 0 arg types so the
        // candidate has more params than provided (1 > 0) — the invariant
        // check fires before any other filtering.
        let _ = try_default_padding(&[&bad_cand], &[]);
    }
}
