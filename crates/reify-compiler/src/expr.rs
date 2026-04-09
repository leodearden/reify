use super::*;

pub(crate) fn compile_expr(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledExpr {
    let mut lambda_counter = 0u32;
    compile_expr_guarded(
        expr,
        scope,
        enum_defs,
        functions,
        diagnostics,
        None,
        &mut lambda_counter,
    )
}

/// Compile an `Expr` from the AST into a `CompiledExpr`, with guard context.
///
/// When `current_guard` is Some, references to names guarded by a different
/// guard will produce a diagnostic error about unsafe unguarded references.
#[allow(clippy::only_used_in_recursion)]
pub(crate) fn compile_expr_guarded(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    current_guard: Option<&ValueCellId>,
    lambda_counter: &mut u32,
) -> CompiledExpr {
    match &expr.kind {
        reify_syntax::ExprKind::NumberLiteral(v) => {
            // Whole numbers become Int, fractional become Real
            if *v == (*v as i64) as f64 && v.is_finite() {
                CompiledExpr::literal(Value::Int(*v as i64), Type::Int)
            } else {
                CompiledExpr::literal(Value::Real(*v), Type::Real)
            }
        }
        reify_syntax::ExprKind::QuantityLiteral { value, unit } => {
            // Check the unit registry first (for user-declared units), then fall back to hardcoded.
            let resolved = scope
                .lookup_unit_in_registry(*value, unit)
                .or_else(|| unit_to_scalar(*value, unit));
            match resolved {
                Some((scalar_val, dimension)) => {
                    // Defense-in-depth: reject non-finite si_value from either
                    // lookup_unit_in_registry or unit_to_scalar (overflow, inf literal, etc.)
                    if let Value::Scalar { si_value, .. } = &scalar_val
                        && !si_value.is_finite()
                    {
                        diagnostics.push(
                            Diagnostic::error(
                                "overflow in quantity literal: result is not finite".to_string(),
                            )
                            .with_label(DiagnosticLabel::new(expr.span, "non-finite result")),
                        );
                        return CompiledExpr::literal(
                            Value::Undef,
                            Type::Scalar {
                                dimension: DimensionVector::DIMENSIONLESS,
                            },
                        );
                    }
                    let ty = Type::Scalar { dimension };
                    CompiledExpr::literal(scalar_val, ty)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown unit: {}", unit))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized unit")),
                    );
                    // Return an undef literal with dimensionless scalar type as a fallback.
                    // Using Scalar (not Real) keeps the type system consistent for quantity expressions.
                    CompiledExpr::literal(
                        Value::Undef,
                        Type::Scalar {
                            dimension: DimensionVector::DIMENSIONLESS,
                        },
                    )
                }
            }
        }
        reify_syntax::ExprKind::BoolLiteral(b) => {
            CompiledExpr::literal(Value::Bool(*b), Type::Bool)
        }
        reify_syntax::ExprKind::StringLiteral(s) => {
            CompiledExpr::literal(Value::String(s.clone()), Type::String)
        }
        reify_syntax::ExprKind::Ident(name) => {
            // Intercept `self` in entity scope — bare `self` resolves to StructureRef(entity_name).
            // In function scopes (is_entity_scope == false), self falls through to "unresolved name".
            if name == "self" && scope.is_entity_scope {
                let self_id = ValueCellId::new(&scope.entity_name, "__self");
                return CompiledExpr::value_ref(
                    self_id,
                    Type::StructureRef(scope.entity_name.clone()),
                );
            }
            // Intercept `none` before scope lookup — it's a language-level keyword.
            // Default inner type is Real; contextual override happens at param/let sites.
            if name == "none" {
                return CompiledExpr::option_none(Type::Option(Box::new(Type::Real)));
            }
            match scope.resolve(name) {
                Some((id, ty)) => CompiledExpr::value_ref(id.clone(), ty.clone()),
                None => {
                    // Check if this is a collection sub name — resolve to per-member __list_{name}__{member}
                    if scope.collection_sub_names.contains(name.as_str()) {
                        if let Some(members) = scope.collection_sub_member_types.get(name.as_str())
                        {
                            // Resolve to the first member's per-member list
                            if let Some((first_member, member_ty)) = members.iter().next() {
                                let list_id = ValueCellId::new(
                                    &scope.entity_name,
                                    format!("__list_{}__{}", name, first_member),
                                );
                                let list_type = Type::List(Box::new(member_ty.clone()));
                                return CompiledExpr::value_ref(list_id, list_type);
                            }
                        }
                        // Fallback: no member types available
                        let list_id =
                            ValueCellId::new(&scope.entity_name, format!("__list_{}", name));
                        let list_type = Type::List(Box::new(Type::StructureRef(name.clone())));
                        return CompiledExpr::value_ref(list_id, list_type);
                    }
                    diagnostics.push(
                        Diagnostic::error(format!("unresolved name: {}", name))
                            .with_label(DiagnosticLabel::new(expr.span, "not found in scope")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::BinOp { op, left, right } => {
            // Chained comparison desugaring: `a < b < c` → `And(Lt(a,b), Lt(b,c))`.
            // Detect when the outer op is a comparison and the left operand is also a comparison BinOp.
            if is_comparison_op(op)
                && let reify_syntax::ExprKind::BinOp { op: inner_op, .. } = &left.kind
                && is_comparison_op(inner_op)
            {
                let (operands, ops) = flatten_comparison_chain(op, left, right);
                // Compile each operand exactly once
                let compiled_operands: Vec<CompiledExpr> = operands
                    .iter()
                    .map(|e| {
                        compile_expr_guarded(
                            e,
                            scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            current_guard,
                            lambda_counter,
                        )
                    })
                    .collect();
                // Build pairwise comparison nodes
                let mut pairs: Vec<CompiledExpr> = Vec::new();
                for (i, op_str) in ops.iter().enumerate() {
                    match resolve_binop(op_str) {
                        Some(bin_op) => {
                            let lhs = compiled_operands[i].clone();
                            let rhs = compiled_operands[i + 1].clone();
                            let result_type =
                                infer_binop_type(bin_op, &lhs.result_type, &rhs.result_type);
                            pairs.push(CompiledExpr::binop(bin_op, lhs, rhs, result_type));
                        }
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!("unknown operator: {}", op_str))
                                    .with_label(DiagnosticLabel::new(
                                        expr.span,
                                        "unrecognized operator",
                                    )),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Real);
                        }
                    }
                }
                // Left-fold pairs into And-chain
                let mut acc = pairs.remove(0);
                for pair in pairs {
                    acc = CompiledExpr::binop(BinOp::And, acc, pair, Type::Bool);
                }
                return acc;
            }

            let compiled_left = compile_expr_guarded(
                left,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_right = compile_expr_guarded(
                right,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            match resolve_binop(op) {
                Some(bin_op) => {
                    let result_type = infer_binop_type(
                        bin_op,
                        &compiled_left.result_type,
                        &compiled_right.result_type,
                    );

                    // Dimension compatibility check for Add/Sub
                    if matches!(bin_op, BinOp::Add | BinOp::Sub) {
                        let op_name = if bin_op == BinOp::Add {
                            "addition"
                        } else {
                            "subtraction"
                        };
                        match (&compiled_left.result_type, &compiled_right.result_type) {
                            // Scalar + Scalar with different dimensions
                            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd })
                                if ld != rd =>
                            {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "dimension mismatch in {}: {} vs {}",
                                        op_name,
                                        compiled_left.result_type,
                                        compiled_right.result_type,
                                    ))
                                    .with_label(
                                        DiagnosticLabel::new(expr.span, "incompatible dimensions"),
                                    ),
                                );
                            }
                            // Scalar + Int/Real or Int/Real + Scalar (dimensioned + dimensionless)
                            (Type::Scalar { .. }, Type::Int | Type::Real)
                            | (Type::Int | Type::Real, Type::Scalar { .. }) => {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "incompatible types in {}: {} vs {}",
                                        op_name,
                                        compiled_left.result_type,
                                        compiled_right.result_type,
                                    ))
                                    .with_label(
                                        DiagnosticLabel::new(
                                            expr.span,
                                            "dimensioned + dimensionless",
                                        ),
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }

                    CompiledExpr::binop(bin_op, compiled_left, compiled_right, result_type)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown operator: {}", op))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::UnOp { op, operand } => {
            let compiled_operand = compile_expr_guarded(
                operand,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            match resolve_unop(op) {
                Some(un_op) => {
                    let result_type = match un_op {
                        UnOp::Not => Type::Bool,
                        UnOp::Neg => compiled_operand.result_type.clone(),
                    };
                    CompiledExpr::unop(un_op, compiled_operand, result_type)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown unary operator: {}", op))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            let compiled_lower = lower.as_ref().map(|e| {
                compile_expr_guarded(
                    e,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                )
            });
            let compiled_upper = upper.as_ref().map(|e| {
                compile_expr_guarded(
                    e,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                )
            });
            // Dimensional checking: both bounds must have the same dimension
            if let (Some(lo), Some(hi)) = (&compiled_lower, &compiled_upper) {
                match (&lo.result_type, &hi.result_type) {
                    (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd })
                        if ld != rd =>
                    {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "dimension mismatch in range: {} vs {}",
                                lo.result_type, hi.result_type,
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "incompatible dimensions")),
                        );
                    }
                    (Type::Scalar { .. }, Type::Int | Type::Real)
                    | (Type::Int | Type::Real, Type::Scalar { .. }) => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "incompatible types in range: {} vs {}",
                                lo.result_type, hi.result_type,
                            ))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "dimensioned + dimensionless",
                            )),
                        );
                    }
                    _ => {}
                }
            }
            // Infer the element type from whichever bound is present.
            // NOTE: the parser (lower_range_expr) always provides both lower
            // and upper via `?`, so both being None is an ICE path that is
            // unreachable from user code.
            let element_type = compiled_lower
                .as_ref()
                .map(|e| &e.result_type)
                .or_else(|| compiled_upper.as_ref().map(|e| &e.result_type))
                .cloned()
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::error(
                            "internal compiler error: range has no bounds; cannot infer element type",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "ICE: no lower or upper bound")),
                    );
                    Type::Real
                });
            let result_type = Type::range(element_type);
            CompiledExpr::range_constructor(
                compiled_lower,
                compiled_upper,
                *lower_inclusive,
                *upper_inclusive,
                result_type,
            )
        }
        reify_syntax::ExprKind::FunctionCall { name, args } => {
            // Intercept `some(expr)` before general function resolution.
            // some() is a language-level constructor, not a user-defined function.
            if name == "some" {
                if args.len() != 1 {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "some() requires exactly 1 argument, got {}",
                            args.len()
                        ))
                        .with_label(DiagnosticLabel::new(
                            expr.span,
                            "wrong number of arguments",
                        )),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
                let inner = compile_expr_guarded(
                    &args[0],
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                );
                let result_type = Type::Option(Box::new(inner.result_type.clone()));
                return CompiledExpr::option_some(inner, result_type);
            }

            let compiled_args: Vec<CompiledExpr> = args
                .iter()
                .map(|arg| {
                    compile_expr_guarded(
                        arg,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();

            let arg_types: Vec<Type> = compiled_args
                .iter()
                .map(|a| a.result_type.clone())
                .collect();

            match resolve_function_overload(name, &arg_types, functions) {
                OverloadResolution::Resolved(matched_fn) => {
                    // Exactly one user fn matches — emit UserFunctionCall
                    // Deprecation check: warn if the called function is @deprecated.
                    if let Some(msg) = deprecation_message(&matched_fn.annotations) {
                        emit_deprecation_warning(
                            "function",
                            name,
                            &msg,
                            expr.span,
                            diagnostics,
                        );
                    }
                    let result_type = matched_fn.return_type.clone();
                    let content_hash = {
                        let mut h = ContentHash::of(&[6]).combine(ContentHash::of_str(name));
                        for arg in &compiled_args {
                            h = h.combine(arg.content_hash);
                        }
                        h
                    };
                    CompiledExpr {
                        kind: CompiledExprKind::UserFunctionCall {
                            function_name: name.clone(),
                            args: compiled_args,
                        },
                        result_type,
                        content_hash,
                    }
                }
                OverloadResolution::Ambiguous(candidates) => {
                    // Multiple user fns match — ambiguous call
                    let candidate_sigs: Vec<String> =
                        candidates.iter().map(|f| format_fn_signature(f)).collect();
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "ambiguous function call: {} candidates match {}({}): {}",
                            candidates.len(),
                            name,
                            arg_types
                                .iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate_sigs.join(", ")
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "ambiguous call")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
                OverloadResolution::NoMatch(named_candidates) => {
                    // User functions with this name exist, but none match — error with candidates
                    let candidate_sigs: Vec<String> = named_candidates
                        .iter()
                        .map(|f| format_fn_signature(f))
                        .collect();
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "no matching overload for {}({}), candidates: {}",
                            name,
                            arg_types
                                .iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate_sigs.join(", ")
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "no matching overload")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
                OverloadResolution::NoUserFunctions => {
                    // Determinacy predicate intrinsics — compiler transforms these
                    // calls into DeterminacyPredicate nodes evaluated by the engine
                    // using the snapshot's DeterminacyState for each ValueCellId.
                    //
                    // User-facing semantic contract:
                    //   determined(x)           — true iff x is fully resolved
                    //                             (state == Determined)
                    //   undetermined(x)         — true iff x has no value
                    //                             (state == Undetermined),
                    //                             regardless of constraints
                    //   constrained(x)          — true iff x is a solver variable
                    //                             (state == Auto || Provisional);
                    //                             tests solver involvement, NOT
                    //                             constraint presence
                    //   partially_determined(x) — true iff x is in solver
                    //                             intermediate state
                    //                             (state == Provisional only);
                    //                             narrowed from original spec to
                    //                             distinguish from Auto (which is
                    //                             covered by constrained())
                    let determinacy_kind = match name.as_str() {
                        "determined" => Some(DeterminacyPredicateKind::Determined),
                        "undetermined" => Some(DeterminacyPredicateKind::Undetermined),
                        "constrained" => Some(DeterminacyPredicateKind::Constrained),
                        "partially_determined" => {
                            Some(DeterminacyPredicateKind::PartiallyDetermined)
                        }
                        _ => None,
                    };

                    if let Some(kind) = determinacy_kind {
                        if compiled_args.len() != 1 {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "{}() requires exactly 1 argument, got {}",
                                    name,
                                    compiled_args.len()
                                ))
                                .with_label(DiagnosticLabel::new(
                                    expr.span,
                                    "wrong number of arguments",
                                )),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Bool);
                        }

                        let arg = &compiled_args[0];
                        if let CompiledExprKind::ValueRef(cell_id) = &arg.kind {
                            return CompiledExpr::determinacy_predicate(kind, cell_id.clone());
                        } else {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "{}() argument must be a direct cell reference, not a computed expression",
                                    name
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "expected cell reference")),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Bool);
                        }
                    }

                    // No user fn with this name — fall through to stdlib FunctionCall
                    let resolved = ResolvedFunction {
                        name: name.clone(),
                        qualified_name: format!("std::{}", name),
                    };

                    // Infer a result type — for geometry functions, use a placeholder
                    let result_type = if is_geometry_function(name) {
                        Type::dimensionless_scalar()
                    } else {
                        compiled_args
                            .first()
                            .map(|a| a.result_type.clone())
                            .unwrap_or_else(|| {
                                diagnostics.push(
                                    Diagnostic::warning(format!(
                                        "cannot infer return type of zero-arg function '{}', defaulting to Real",
                                        name
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        expr.span,
                                        "zero-arg function: return type inferred as Real",
                                    )),
                                );
                                Type::Real
                            })
                    };

                    let content_hash = {
                        let mut h = ContentHash::of(&[4])
                            .combine(ContentHash::of_str(&resolved.qualified_name));
                        for arg in &compiled_args {
                            h = h.combine(arg.content_hash);
                        }
                        h
                    };

                    CompiledExpr {
                        kind: CompiledExprKind::FunctionCall {
                            function: resolved,
                            args: compiled_args,
                        },
                        result_type,
                        content_hash,
                    }
                }
            }
        }
        reify_syntax::ExprKind::MemberAccess { object, member } => {
            // Check if this is a `self.member` or `self.sub.member` access in entity scope.
            if scope.is_entity_scope {
                // Pattern: self.member
                if let reify_syntax::ExprKind::Ident(obj_name) = &object.kind
                    && obj_name == "self"
                {
                    // Check for self.sub.member chaining (handled at outer level): skip here
                    // if member is a sub-component name — return a StructureRef for the sub.
                    // Guard: exclude collection subs (List<T>), which must be accessed via index.
                    if scope.sub_component_types.contains_key(member.as_str())
                        && !scope.collection_sub_names.contains(member.as_str())
                    {
                        // self.sub_name — return StructureRef so that chaining works
                        // (but note: this case is handled by the outer MemberAccess pattern below)
                        let structure_name = scope.sub_component_types[member.as_str()].clone();
                        let scoped_entity = format!("{}.{}", scope.entity_name, member);
                        let sub_id = ValueCellId::new(&scoped_entity, "__self");
                        return CompiledExpr::value_ref(
                            sub_id,
                            Type::StructureRef(structure_name),
                        );
                    }
                    // Error: collection sub accessed directly through self.
                    if scope.collection_sub_names.contains(member.as_str()) {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "cannot access collection sub '{}' directly through self; \
                                 use indexed access like `{}[i].<field>` or aggregation like `{}.count`",
                                member, member, member
                            ))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "collection sub requires indexing",
                            )),
                        );
                        return CompiledExpr::literal(Value::Undef, Type::Real);
                    }
                    // Resolve member from the entity scope (same as bare identifier).
                    match scope.resolve(member) {
                        Some((id, ty)) => {
                            let id = id.clone();
                            let ty = ty.clone();
                            return CompiledExpr::value_ref(id, ty);
                        }
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unknown member '{}' on self",
                                    member
                                ))
                                .with_label(DiagnosticLabel::new(
                                    expr.span,
                                    "unknown member",
                                )),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Real);
                        }
                    }
                }

                // Pattern: self.sub.member (object is MemberAccess { Ident("self"), sub_name })
                // Guard: exclude collection subs (List<T>), which must be accessed via index.
                if let reify_syntax::ExprKind::MemberAccess {
                    object: inner_obj,
                    member: sub_name,
                } = &object.kind
                    && let reify_syntax::ExprKind::Ident(self_name) = &inner_obj.kind
                    && self_name == "self"
                    && scope.sub_component_types.contains_key(sub_name.as_str())
                    && !scope.collection_sub_names.contains(sub_name.as_str())
                {
                    // Resolve member type from sub_member_types
                    let member_type = match scope
                        .sub_member_types
                        .get(sub_name.as_str())
                        .and_then(|m| m.get(member.as_str()))
                        .cloned()
                    {
                        Some(ty) => ty,
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unknown member '{}' on sub '{}'",
                                    member, sub_name
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Real);
                        }
                    };
                    let scoped_entity =
                        format!("{}.{}", scope.entity_name, sub_name);
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    return CompiledExpr::value_ref(scoped_id, member_type);
                }
                // Error: collection sub member accessed directly through self.
                if let reify_syntax::ExprKind::MemberAccess {
                    object: inner_obj,
                    member: sub_name,
                } = &object.kind
                    && let reify_syntax::ExprKind::Ident(self_name) = &inner_obj.kind
                    && self_name == "self"
                    && scope.collection_sub_names.contains(sub_name.as_str())
                {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "cannot access member '{}' of collection sub '{}' directly through self; \
                             use `{}[i].{}` for a specific instance",
                            member, sub_name, sub_name, member
                        ))
                        .with_label(DiagnosticLabel::new(
                            expr.span,
                            "collection sub member requires indexing",
                        )),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
            }

            // Check if this is a port member access (port_name.member_name)
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && scope.port_names.contains(name.as_str())
            {
                let composite_key = format!("{}.{}", name, member);
                if let Some((id, ty)) = scope.resolve(&composite_key) {
                    let id = id.clone();
                    let ty = ty.clone();
                    return CompiledExpr::value_ref(id, ty);
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!("port '{}' has no member '{}'", name, member))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown port member")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
            }

            // Check if this is an indexed collection member access: collection[i].member
            if let reify_syntax::ExprKind::IndexAccess {
                object: idx_obj,
                index,
            } = &object.kind
                && let reify_syntax::ExprKind::Ident(name) = &idx_obj.kind
                && scope.collection_sub_names.contains(name.as_str())
            {
                // Resolve member type from pre-populated collection_sub_member_types
                let member_type = match scope
                    .collection_sub_member_types
                    .get(name.as_str())
                    .and_then(|m| m.get(member.as_str()))
                    .cloned()
                {
                    Some(ty) => ty,
                    None => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "unknown member '{}' on collection sub '{}'",
                                member, name
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                        );
                        Type::Real // fallback to allow continued compilation
                    }
                };

                // For literal integer index, resolve directly to a scoped ValueRef
                if let reify_syntax::ExprKind::NumberLiteral(n) = &index.kind {
                    if n.fract() != 0.0 || *n < 0.0 {
                        diagnostics.push(
                            Diagnostic::error(
                                "collection index must be a non-negative integer literal",
                            )
                            .with_label(DiagnosticLabel::new(expr.span, "invalid index")),
                        );
                        return CompiledExpr::literal(Value::Undef, member_type);
                    }
                    let i = *n as i64;
                    let scoped_entity = format!("{}.{}[{}]", scope.entity_name, name, i);
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    return CompiledExpr::value_ref(scoped_id, member_type);
                }
                // For non-literal index, compile as IndexAccess into a per-member synthetic list.
                // The eval engine creates __list_{name}__{member} cells that gather each
                // instance's member value into a List, so indexing gives the right value.
                let list_member = format!("__list_{}__{}", name, member);
                let list_id = ValueCellId::new(&scope.entity_name, &list_member);
                let collection_ref =
                    CompiledExpr::value_ref(list_id, Type::List(Box::new(member_type.clone())));
                diagnostics.push(
                    Diagnostic::info(format!(
                        "dynamic collection index: {}[<expr>].{} — result depends on runtime list assembly",
                        name, member
                    ))
                );
                let compiled_idx = compile_expr_guarded(
                    index,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                );
                return CompiledExpr::index_access(collection_ref, compiled_idx, member_type);
            }

            // Check if this is a collection sub member access: collection.count
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && scope.collection_sub_names.contains(name.as_str())
                && member == "count"
            {
                // Resolve to the synthetic __count_ cell
                let count_member = format!("__count_{}", name);
                let count_id = ValueCellId::new(&scope.entity_name, &count_member);
                return CompiledExpr::value_ref(count_id, Type::Int);
            }

            // Check if this is a meta block access: meta.key
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && name == "meta"
            {
                if scope.meta_entries.is_empty() {
                    diagnostics.push(
                        Diagnostic::error("entity has no meta block".to_string())
                            .with_label(DiagnosticLabel::new(expr.span, "no meta block")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::String);
                }
                if scope.meta_entries.contains_key(member.as_str()) {
                    return CompiledExpr::meta_access(scope.entity_name.clone(), member.clone());
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!("meta block has no key: {}", member))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown meta key")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::String);
                }
            }

            // For non-port member access, check if it's a known collection method
            let compiled_obj = compile_expr_guarded(
                object,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let collection_methods = ["count", "sum", "keys", "values"];
            if collection_methods.contains(&member.as_str()) {
                // Infer result type from method and object type
                let result_type = match member.as_str() {
                    "count" => Type::Int,
                    "sum" => match &compiled_obj.result_type {
                        Type::List(inner) => (**inner).clone(),
                        _ => Type::Real,
                    },
                    "keys" => match &compiled_obj.result_type {
                        Type::Map(k, _) => Type::List(k.clone()),
                        _ => Type::List(Box::new(Type::Real)),
                    },
                    "values" => match &compiled_obj.result_type {
                        Type::Map(_, v) => Type::List(v.clone()),
                        _ => Type::List(Box::new(Type::Real)),
                    },
                    _ => Type::Real,
                };
                CompiledExpr::method_call(compiled_obj, member.clone(), vec![], result_type)
            } else {
                diagnostics.push(
                    Diagnostic::error(format!("member access not yet supported: .{}", member))
                        .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
                );
                CompiledExpr::literal(Value::Undef, Type::Real)
            }
        }
        reify_syntax::ExprKind::ListLiteral(elements) => {
            let compiled_elems: Vec<CompiledExpr> = elements
                .iter()
                .map(|e| {
                    compile_expr_guarded(
                        e,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();
            // Infer element type from first element, warn and default to Real for empty lists
            let elem_type = compiled_elems
                .first()
                .map(|e| e.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "cannot infer element type of empty list literal, defaulting to Real",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "empty list")),
                    );
                    Type::Real
                });
            let result_type = Type::List(Box::new(elem_type));
            CompiledExpr::list_literal(compiled_elems, result_type)
        }
        reify_syntax::ExprKind::SetLiteral(elements) => {
            let compiled_elems: Vec<CompiledExpr> = elements
                .iter()
                .map(|e| {
                    compile_expr_guarded(
                        e,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();
            let elem_type = compiled_elems
                .first()
                .map(|e| e.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "cannot infer element type of empty set literal, defaulting to Real",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "empty set")),
                    );
                    Type::Real
                });
            let result_type = Type::Set(Box::new(elem_type));
            CompiledExpr::set_literal(compiled_elems, result_type)
        }
        reify_syntax::ExprKind::MapLiteral(entries) => {
            let compiled_entries: Vec<(CompiledExpr, CompiledExpr)> = entries
                .iter()
                .map(|(k, v)| {
                    let ck = compile_expr_guarded(
                        k,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    );
                    let cv = compile_expr_guarded(
                        v,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    );
                    (ck, cv)
                })
                .collect();
            let key_type = compiled_entries
                .first()
                .map(|(k, _)| k.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "cannot infer key type of empty map literal, defaulting to String",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "empty map")),
                    );
                    Type::String
                });
            let val_type = compiled_entries
                .first()
                .map(|(_, v)| v.result_type.clone())
                .unwrap_or_else(|| {
                    // Warning already emitted for empty map at key_type step above;
                    // no second warning needed for the value type.
                    Type::Real
                });
            let result_type = Type::Map(Box::new(key_type), Box::new(val_type));
            CompiledExpr::map_literal(compiled_entries, result_type)
        }
        reify_syntax::ExprKind::IndexAccess { object, index } => {
            let compiled_obj = compile_expr_guarded(
                object,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_idx = compile_expr_guarded(
                index,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            // Infer result type from collection's element type
            let result_type = match &compiled_obj.result_type {
                Type::List(inner) => (**inner).clone(),
                Type::Map(_, val) => (**val).clone(),
                _ => Type::Real,
            };
            CompiledExpr::index_access(compiled_obj, compiled_idx, result_type)
        }
        reify_syntax::ExprKind::EnumAccess { type_name, variant } => {
            // Look up the enum type in the registry
            if let Some(enum_def) = enum_defs.iter().find(|e| e.name == *type_name) {
                if enum_def.contains_variant(variant) {
                    CompiledExpr::literal(
                        Value::Enum {
                            type_name: type_name.clone(),
                            variant: variant.clone(),
                        },
                        Type::Enum(type_name.clone()),
                    )
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unknown variant '{}' on enum '{}'",
                            variant, type_name
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown variant")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Enum(type_name.clone()))
                }
            } else {
                diagnostics.push(
                    Diagnostic::error(format!("unknown enum type '{}'", type_name))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown enum")),
                );
                CompiledExpr::literal(Value::Undef, Type::Real)
            }
        }
        reify_syntax::ExprKind::Match { discriminant, arms } => {
            let compiled_discriminant = compile_expr_guarded(
                discriminant,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_arms: Vec<reify_types::CompiledMatchArm> = arms
                .iter()
                .map(|arm| {
                    let body = compile_expr_guarded(
                        &arm.body,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    );
                    reify_types::CompiledMatchArm {
                        patterns: arm.patterns.clone(),
                        body,
                    }
                })
                .collect();

            // Result type from the first arm's body.
            // NOTE: the grammar requires at least one arm so an empty arms
            // list is an ICE path unreachable from user code.
            let result_type = compiled_arms
                .first()
                .map(|a| a.body.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::error(
                            "internal compiler error: match expression has no arms; cannot infer result type",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "ICE: match with no arms")),
                    );
                    Type::Real
                });

            // Exhaustiveness check: if discriminant is a known enum type,
            // verify all variants are covered by arm patterns or a wildcard.
            if let Type::Enum(ref enum_name) = compiled_discriminant.result_type
                && let Some(enum_def) = enum_defs.iter().find(|e| e.name == *enum_name)
            {
                let has_wildcard = compiled_arms
                    .iter()
                    .any(|arm| arm.patterns.iter().any(|p| p == "_"));

                if !has_wildcard {
                    let covered: std::collections::HashSet<&str> = compiled_arms
                        .iter()
                        .flat_map(|arm| arm.patterns.iter().map(|p| p.as_str()))
                        .collect();

                    let missing: Vec<&str> = enum_def
                        .variants
                        .iter()
                        .filter(|v| !covered.contains(v.as_str()))
                        .map(|v| v.as_str())
                        .collect();

                    if !missing.is_empty() {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "non-exhaustive match on '{}': missing variant(s) {}",
                                enum_name,
                                missing.join(", ")
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "missing variants")),
                        );
                    }
                }
            }

            // Content hash: tag [6] + discriminant + all arms
            let mut content_hash =
                ContentHash::of(&[6]).combine(compiled_discriminant.content_hash);
            for arm in &compiled_arms {
                for pattern in &arm.patterns {
                    content_hash = content_hash.combine(ContentHash::of_str(pattern));
                }
                content_hash = content_hash.combine(arm.body.content_hash);
            }

            CompiledExpr {
                kind: CompiledExprKind::Match {
                    discriminant: Box::new(compiled_discriminant),
                    arms: compiled_arms,
                },
                result_type,
                content_hash,
            }
        }
        reify_syntax::ExprKind::Auto => {
            // Auto expressions should not appear inside compile_expr — they are
            // handled at the param compilation level. If we reach here, emit an
            // Undef literal as a safe fallback.
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            let compiled_cond = compile_expr_guarded(
                condition,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_then = compile_expr_guarded(
                then_branch,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_else = compile_expr_guarded(
                else_branch,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let result_type = compiled_then.result_type.clone();

            let content_hash = ContentHash::of(&[5])
                .combine(compiled_cond.content_hash)
                .combine(compiled_then.content_hash)
                .combine(compiled_else.content_hash);

            CompiledExpr {
                kind: CompiledExprKind::Conditional {
                    condition: Box::new(compiled_cond),
                    then_branch: Box::new(compiled_then),
                    else_branch: Box::new(compiled_else),
                },
                result_type,
                content_hash,
            }
        }
        reify_syntax::ExprKind::Lambda { params, body } => {
            let lambda_entity = format!("$lambda{}.{}", lambda_counter, scope.entity_name);
            *lambda_counter += 1;

            let mut lambda_scope = scope.clone();
            let mut compiled_params: Vec<(String, Option<Type>)> = Vec::new();
            let mut param_types: Vec<Type> = Vec::new();
            let mut param_ids: Vec<ValueCellId> = Vec::new();

            for param in params {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_name(&type_expr.name) {
                        Some(t) => t,
                        None => {
                            diagnostics.push(Diagnostic::error(format!(
                                "unresolved type in lambda param '{}': {}",
                                param.name, type_expr.name
                            )));
                            Type::Real // fallback
                        }
                    }
                } else {
                    Type::Real // default untyped params to Real
                };

                let param_id = ValueCellId::new(&lambda_entity, &param.name);
                lambda_scope
                    .names
                    .insert(param.name.clone(), (param_id.clone(), ty.clone(), None));

                param_ids.push(param_id);
                param_types.push(ty.clone());
                compiled_params.push((param.name.clone(), param.type_expr.as_ref().map(|_| ty)));
            }

            // Compile body in the nested scope
            let compiled_body = compile_expr_guarded(
                body,
                &lambda_scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            // Capture analysis: collect ValueRefs in body, filter out lambda params
            let lambda_param_set: HashSet<ValueCellId> = param_ids.iter().cloned().collect();
            let all_refs = collect_body_refs(&compiled_body);
            let mut seen = HashSet::new();
            let mut captures: Vec<ValueCellId> = Vec::new();
            for id in all_refs {
                if !lambda_param_set.contains(&id) && seen.insert(id.clone()) {
                    captures.push(id);
                }
            }

            let return_type = compiled_body.result_type.clone();
            let result_type = Type::Function {
                params: param_types,
                return_type: Box::new(return_type),
            };

            CompiledExpr::lambda(
                compiled_params,
                param_ids,
                compiled_body,
                captures,
                result_type,
            )
        }
        reify_syntax::ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
        } => {
            let quant_entity = format!("$quant{}.{}", lambda_counter, scope.entity_name);
            *lambda_counter += 1;

            // Compile collection in the outer scope
            let compiled_collection = compile_expr_guarded(
                collection,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            // Create a nested scope with the bound variable
            let mut quant_scope = scope.clone();
            let variable_id = ValueCellId::new(&quant_entity, variable);
            // Infer element type from the collection's result type
            let elem_type = match &compiled_collection.result_type {
                Type::List(elem) | Type::Set(elem) => *elem.clone(),
                _ => Type::Real, // fallback for unresolved types
            };
            quant_scope
                .names
                .insert(variable.clone(), (variable_id.clone(), elem_type, None));

            // Compile predicate in the nested scope
            let compiled_predicate = compile_expr_guarded(
                predicate,
                &quant_scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            let compiled_kind = match kind {
                reify_syntax::QuantifierKind::ForAll => reify_types::QuantifierKind::ForAll,
                reify_syntax::QuantifierKind::Exists => reify_types::QuantifierKind::Exists,
            };

            CompiledExpr::quantifier(
                compiled_kind,
                variable.clone(),
                variable_id,
                compiled_collection,
                compiled_predicate,
            )
        }
        // AdHocSelector compiler support is implemented in a separate task.
        reify_syntax::ExprKind::AdHocSelector { .. } => {
            diagnostics.push(
                Diagnostic::error("ad-hoc selector (@) is not yet supported in the compiler")
                    .with_label(DiagnosticLabel::new(expr.span, "not yet supported")),
            );
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::QualifiedAccess { qualifier, member } => {
            // Resolve `TraitName::member` to the member's ValueCellId in the current scope.
            // Only simple `Ident::member` form is supported.
            let trait_name = match &qualifier.kind {
                reify_syntax::ExprKind::Ident(name) => name.clone(),
                _ => {
                    diagnostics.push(
                        Diagnostic::error(
                            "unsupported qualified access: only 'TraitName::member' form is supported",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "unsupported form")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
            };

            // Validate trait existence.
            let members = match scope.trait_members.get(&trait_name) {
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("trait '{}' not found", trait_name))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown trait")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
                Some(m) => m,
            };

            // Validate member existence in trait.
            if !members.contains(member.as_str()) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "member '{}' not defined in trait '{}'",
                        member, trait_name
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "not in trait")),
                );
                return CompiledExpr::literal(Value::Undef, Type::Real);
            }

            // Resolve the member in the current scope (the structure should have it
            // because it conforms to the trait).
            match scope.resolve(member) {
                Some((id, ty)) => CompiledExpr::value_ref(id.clone(), ty.clone()),
                None => {
                    // Fallback: create a ValueCellId directly (trait conformance will catch
                    // missing members separately).
                    let id = ValueCellId::new(&scope.entity_name, member);
                    CompiledExpr::value_ref(id, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::InstanceQualifiedAccess { object, qualified } => {
            // Resolve `sub.(TraitName::member)` to a ValueCellId for the sub's member.

            // Extract the sub-component name.
            let sub_name = match &object.kind {
                reify_syntax::ExprKind::Ident(name) => name.clone(),
                _ => {
                    diagnostics.push(
                        Diagnostic::error(
                            "unsupported instance qualified access: object must be an identifier",
                        )
                        .with_label(DiagnosticLabel::new(object.span, "unsupported")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
            };

            // Extract trait_name and member from the qualified access part.
            let (trait_name, member) = match &qualified.kind {
                reify_syntax::ExprKind::QualifiedAccess { qualifier, member } => {
                    match &qualifier.kind {
                        reify_syntax::ExprKind::Ident(name) => (name.clone(), member.clone()),
                        _ => {
                            diagnostics.push(
                                Diagnostic::error(
                                    "unsupported qualified access in instance access",
                                )
                                .with_label(DiagnosticLabel::new(
                                    qualified.span,
                                    "unsupported form",
                                )),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Real);
                        }
                    }
                }
                _ => {
                    diagnostics.push(
                        Diagnostic::error(
                            "expected 'Trait::member' form in instance qualified access",
                        )
                        .with_label(DiagnosticLabel::new(
                            qualified.span,
                            "expected qualified access",
                        )),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
            };

            // Look up the sub-component's structure type.
            let structure_name = match scope.sub_component_types.get(&sub_name) {
                Some(s) => s.clone(),
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown sub-component '{}'", sub_name))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "unknown sub-component",
                            )),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::Real);
                }
            };

            // Check if the sub-component's structure implements the referenced trait.
            let trait_bounds = scope
                .sub_structure_traits
                .get(&structure_name)
                .cloned()
                .unwrap_or_default();
            if !trait_bounds.contains(&trait_name) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "sub-component '{}' (type '{}') does not implement trait '{}'",
                        sub_name, structure_name, trait_name
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "trait not implemented")),
                );
                return CompiledExpr::literal(Value::Undef, Type::Real);
            }

            // Optionally validate the member exists in the trait.
            if let Some(members) = scope.trait_members.get(&trait_name)
                && !members.contains(member.as_str())
            {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "member '{}' not defined in trait '{}'",
                        member, trait_name
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "not in trait")),
                );
                return CompiledExpr::literal(Value::Undef, Type::Real);
            }

            // Generate ValueCellId for the sub-component's member.
            // The eval engine scopes sub-components as "{parent}.{sub_name}".
            let scoped_entity = format!("{}.{}", scope.entity_name, sub_name);
            let id = ValueCellId::new(&scoped_entity, &member);
            // Infer member type from the sub's structure member types if available.
            let ty = scope
                .collection_sub_member_types
                .get(&sub_name)
                .and_then(|m| m.get(&member))
                .cloned()
                .unwrap_or(Type::Real);
            CompiledExpr::value_ref(id, ty)
        }
    }
}

