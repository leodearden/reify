use std::collections::HashMap;

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, DeterminacyPredicateKind,
    DeterminacyState, DimensionVector, FieldSourceKind, PersistentMap, QuantifierKind, Type, UnOp,
    Value, ValueCellId, ValueMap,
};

/// Maximum recursion depth for user-defined function calls.
const MAX_RECURSION_DEPTH: u32 = 256;

/// Evaluation context: provides values, user-defined functions, and recursion tracking.
pub struct EvalContext<'a> {
    /// Current values of all cells.
    pub values: &'a ValueMap,
    /// User-defined functions available for evaluation.
    pub functions: &'a [CompiledFunction],
    /// Current recursion depth (private — managed internally).
    recursion_depth: u32,
    /// Meta block entries per entity: entity name → (key → value).
    /// `None` means meta context was not provided — MetaAccess evaluation will panic.
    pub meta: Option<&'a HashMap<String, HashMap<String, String>>>,
    /// Snapshot determinacy states for DeterminacyPredicate evaluation.
    /// When `Some`, DeterminacyPredicate nodes resolve to `Bool(true/false)`.
    /// When `None`, they return `Undef` (no engine context available).
    pub determinacy: Option<&'a PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
}

impl<'a> EvalContext<'a> {
    /// Create a new evaluation context with values and user-defined functions.
    pub fn new(values: &'a ValueMap, functions: &'a [CompiledFunction]) -> Self {
        Self {
            values,
            functions,
            recursion_depth: 0,
            meta: None,
            determinacy: None,
        }
    }

    /// Create a simple evaluation context with no user-defined functions.
    pub fn simple(values: &'a ValueMap) -> Self {
        Self {
            values,
            functions: &[],
            recursion_depth: 0,
            meta: None,
            determinacy: None,
        }
    }

    /// Create a simple context with an explicit recursion depth — **test-only**.
    #[doc(hidden)]
    pub fn _test_at_depth(values: &'a ValueMap, depth: u32) -> Self {
        Self {
            values,
            functions: &[],
            recursion_depth: depth,
            meta: None,
            determinacy: None,
        }
    }

    /// Attach meta block data for MetaAccess evaluation.
    pub fn with_meta(mut self, meta: &'a HashMap<String, HashMap<String, String>>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Attach snapshot determinacy states for DeterminacyPredicate evaluation.
    pub fn with_determinacy(
        mut self,
        determinacy: &'a PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    ) -> Self {
        self.determinacy = Some(determinacy);
        self
    }

    /// Create a child context with a new scope (for function body evaluation).
    fn with_scope<'b>(&self, values: &'b ValueMap) -> EvalContext<'b>
    where
        'a: 'b,
    {
        EvalContext {
            values,
            functions: self.functions,
            recursion_depth: self.recursion_depth + 1,
            meta: self.meta,
            determinacy: self.determinacy,
        }
    }
}

/// Evaluate a compiled expression against an evaluation context.
///
/// Pure recursive evaluator implementing:
/// - Undef propagation (strict for arithmetic, Kleene for logic)
/// - Dimensional arithmetic (add/sub require same dimension, mul/div combine dimensions)
/// - Division by zero → Undef
/// - User-defined function calls with recursion depth limit
pub fn eval_expr(expr: &CompiledExpr, ctx: &EvalContext) -> Value {
    match &expr.kind {
        CompiledExprKind::Literal(v) => v.clone(),

        CompiledExprKind::ValueRef(id) => ctx.values.get_or_undef(id),

        CompiledExprKind::BinOp { op, left, right } => eval_binop(*op, left, right, ctx),

        CompiledExprKind::UnOp { op, operand } => eval_unop(*op, operand, ctx),

        CompiledExprKind::FunctionCall { function, args } => {
            let evaluated_args: Vec<Value> = args.iter().map(|a| eval_expr(a, ctx)).collect();
            // Strict Undef propagation: if any arg is Undef, short-circuit
            if evaluated_args.iter().any(|v| v.is_undef()) {
                return Value::Undef;
            }
            // Field operations: sample, gradient, divergence, curl
            // These need access to the eval context for lambda application,
            // so they're handled here rather than in stdlib.
            match function.name.as_str() {
                "sample" if evaluated_args.len() == 2 => {
                    if let Value::Field {
                        lambda,
                        source,
                        domain_type,
                        ..
                    } = &evaluated_args[0]
                    {
                        match (lambda.as_ref(), source) {
                            (Value::Lambda { .. }, _) => {
                                apply_lambda(lambda, &evaluated_args[1..], ctx)
                            }
                            // Gradient-produced field: lambda slot contains the
                            // original field (with its own lambda inside).
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    codomain_type: inner_codomain_type,
                                    ..
                                },
                                FieldSourceKind::Gradient,
                            ) => compute_numerical_gradient_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                domain_type,
                                inner_codomain_type,
                                ctx,
                            ),
                            _ => {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "[reify-expr] sample: Field lambda is not a Lambda: {:?}",
                                    lambda
                                );
                                Value::Undef
                            }
                        }
                    } else {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[reify-expr] sample: first argument is not a Field: {:?}",
                            evaluated_args[0]
                        );
                        Value::Undef
                    }
                }
                "gradient" if evaluated_args.len() == 1 => {
                    compute_gradient(&evaluated_args[0])
                }
                "divergence" | "curl" if evaluated_args.len() == 1 => {
                    if !matches!(&evaluated_args[0], Value::Field { .. }) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[reify-expr] {}: argument is not a Field: {:?}",
                            function.name, evaluated_args[0]
                        );
                    }
                    // Stub: these field operations are not yet implemented.
                    Value::Undef
                }
                _ => reify_stdlib::eval_builtin(&function.name, &evaluated_args),
            }
        }

        CompiledExprKind::Match { discriminant, arms } => {
            let disc_val = eval_expr(discriminant, ctx);
            if disc_val.is_undef() {
                return Value::Undef;
            }
            match &disc_val {
                Value::Enum { variant, .. } => {
                    for arm in arms {
                        if arm.patterns.iter().any(|p| p == variant || p == "_") {
                            return eval_expr(&arm.body, ctx);
                        }
                    }
                    // No matching arm found
                    Value::Undef
                }
                _ => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[reify-expr] match expression on non-enum value: {:?}",
                        disc_val
                    );
                    Value::Undef
                }
            }
        }

        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            let cond = eval_expr(condition, ctx);
            match cond {
                Value::Bool(true) => eval_expr(then_branch, ctx),
                Value::Bool(false) => eval_expr(else_branch, ctx),
                Value::Undef => Value::Undef,
                _ => Value::Undef, // type error: condition is not bool
            }
        }

        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => eval_user_function_call(function_name, args, ctx),

        CompiledExprKind::Lambda {
            params,
            param_ids,
            body,
            captures,
        } => {
            let mut capture_map = ValueMap::new();
            for cap_id in captures {
                capture_map.insert(cap_id.clone(), ctx.values.get_or_undef(cap_id));
            }
            Value::Lambda {
                params: params
                    .iter()
                    .zip(param_ids.iter())
                    .map(|((name, _), id)| (name.clone(), id.clone()))
                    .collect(),
                body: body.clone(),
                captures: capture_map,
            }
        }

        CompiledExprKind::ListLiteral(elements) => {
            let items: Vec<Value> = elements.iter().map(|e| eval_expr(e, ctx)).collect();
            Value::List(items)
        }

        CompiledExprKind::SetLiteral(elements) => {
            let items: std::collections::BTreeSet<Value> =
                elements.iter().map(|e| eval_expr(e, ctx)).collect();
            Value::Set(items)
        }

        CompiledExprKind::MapLiteral(entries) => {
            let map: std::collections::BTreeMap<Value, Value> = entries
                .iter()
                .map(|(k, v)| (eval_expr(k, ctx), eval_expr(v, ctx)))
                .collect();
            Value::Map(map)
        }

        CompiledExprKind::IndexAccess { object, index } => {
            let obj = eval_expr(object, ctx);
            let idx = eval_expr(index, ctx);
            if obj.is_undef() || idx.is_undef() {
                return Value::Undef;
            }
            match (&obj, &idx) {
                (Value::List(items), Value::Int(i)) => {
                    if *i < 0 {
                        return Value::Undef;
                    }
                    let i = *i as usize;
                    items.get(i).cloned().unwrap_or(Value::Undef)
                }
                (Value::Map(entries), key) => entries.get(key).cloned().unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }

        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            let obj = eval_expr(object, ctx);
            if obj.is_undef() {
                return Value::Undef;
            }
            let evaluated_args: Vec<Value> = args.iter().map(|a| eval_expr(a, ctx)).collect();
            eval_method_call(&obj, method, &evaluated_args, &expr.result_type, ctx)
        }

        CompiledExprKind::OptionNone => Value::Option(None),

        CompiledExprKind::MetaAccess { entity, key } => {
            let meta_map = ctx.meta.unwrap_or_else(|| {
                panic!("MetaAccess evaluation requires meta context in EvalContext")
            });
            let entity_meta = meta_map.get(entity.as_str()).unwrap_or_else(|| {
                panic!("MetaAccess: entity '{}' not found in meta context", entity)
            });
            let value = entity_meta.get(key.as_str()).unwrap_or_else(|| {
                panic!(
                    "MetaAccess: key '{}' not found in entity '{}' meta",
                    key, entity
                )
            });
            Value::String(value.clone())
        }

        CompiledExprKind::OptionSome(inner) => {
            let val = eval_expr(inner, ctx);
            Value::Option(Some(Box::new(val)))
        }

        CompiledExprKind::RangeConstructor {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            let lo = lower.as_ref().map(|e| eval_expr(e, ctx));
            let hi = upper.as_ref().map(|e| eval_expr(e, ctx));
            // Undef propagation: if any present bound is Undef, the range is Undef
            if lo.as_ref().is_some_and(|v| v.is_undef())
                || hi.as_ref().is_some_and(|v| v.is_undef())
            {
                return Value::Undef;
            }
            Value::range(lo, hi, *lower_inclusive, *upper_inclusive)
        }

        // DeterminacyPredicate: resolve using snapshot determinacy states if available.
        // When no determinacy context is provided (eval layer without engine), returns Undef.
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            if let Some(det_map) = ctx.determinacy {
                // Missing cell in the snapshot indicates a wiring bug (stale ID,
                // evaluation ordering violation). DeterminacyPredicate may only
                // reference cells that are guaranteed to be evaluated before the
                // current cell (topological ordering requirement).
                // Return Undef to make it visible rather than silently defaulting
                // to Undetermined.
                let Some((_, state)) = det_map.get(cell) else {
                    debug_assert!(
                        false,
                        "DeterminacyPredicate references cell {:?} not in determinacy snapshot — wiring bug or eval-order violation",
                        cell
                    );
                    return Value::Undef;
                };
                let state = *state;
                let result = match kind {
                    DeterminacyPredicateKind::Determined => state == DeterminacyState::Determined,
                    DeterminacyPredicateKind::Undetermined => {
                        state == DeterminacyState::Undetermined
                    }
                    // Semantic contract: constrained() checks solver-involvement
                    // state (Auto || Provisional), NOT constraint-presence.
                    //
                    // Rationale: in reify's architecture, the `auto` keyword
                    // explicitly marks a param for constraint-solver resolution,
                    // so constrained(auto_param) correctly returns true even
                    // without explicit constraints. A Determined param with an
                    // explicit constraint (e.g. `param a = 10mm` + `constraint
                    // a > 0mm`) returns false because the constraint is a
                    // validation check on an already-resolved value, not a solver
                    // directive — use `determined(x)` to check resolved state.
                    DeterminacyPredicateKind::Constrained => {
                        state == DeterminacyState::Auto || state == DeterminacyState::Provisional
                    }
                    // Semantic contract: partially_determined() checks for the
                    // solver's intermediate state (Provisional only).
                    //
                    // Intentionally narrowed from original spec ("has constraints
                    // AND state != Determined") to Provisional-only: Auto params
                    // are already covered by constrained(). The Provisional state
                    // uniquely represents a value being actively resolved by the
                    // solver but not yet converged — "partially determined" most
                    // precisely describes this in-flux state. This gives each
                    // predicate a distinct, non-overlapping role:
                    //   determined()           → resolved (Determined)
                    //   undetermined()         → no value (Undetermined)
                    //   constrained()          → solver variable (Auto/Provisional)
                    //   partially_determined() → solver in progress (Provisional)
                    DeterminacyPredicateKind::PartiallyDetermined => {
                        state == DeterminacyState::Provisional
                    }
                };
                Value::Bool(result)
            } else {
                Value::Undef
            }
        }

        CompiledExprKind::Quantifier {
            kind,
            variable_id,
            collection,
            predicate,
            ..
        } => {
            let coll_val = eval_expr(collection, ctx);
            if coll_val.is_undef() {
                return Value::Undef;
            }

            // Extract elements from collection (List or Set)
            let elements: Vec<&Value> = match &coll_val {
                Value::List(items) => items.iter().collect(),
                Value::Set(items) => items.iter().collect(),
                _ => return Value::Undef, // not a collection
            };

            match kind {
                QuantifierKind::ForAll => {
                    // Kleene forall: false short-circuits, undef tracked
                    let mut has_undef = false;
                    for elem in &elements {
                        let mut scope = ctx.values.clone();
                        scope.insert(variable_id.clone(), (*elem).clone());
                        let pred_val = eval_expr(predicate, &ctx.with_scope(&scope));
                        match pred_val {
                            Value::Bool(false) => return Value::Bool(false),
                            Value::Bool(true) => {}
                            Value::Undef => has_undef = true,
                            _ => return Value::Undef, // type error
                        }
                    }
                    if has_undef {
                        Value::Undef
                    } else {
                        Value::Bool(true)
                    }
                }
                QuantifierKind::Exists => {
                    // Kleene exists: true short-circuits, undef tracked
                    let mut has_undef = false;
                    for elem in &elements {
                        let mut scope = ctx.values.clone();
                        scope.insert(variable_id.clone(), (*elem).clone());
                        let pred_val = eval_expr(predicate, &ctx.with_scope(&scope));
                        match pred_val {
                            Value::Bool(true) => return Value::Bool(true),
                            Value::Bool(false) => {}
                            Value::Undef => has_undef = true,
                            _ => return Value::Undef, // type error
                        }
                    }
                    if has_undef {
                        Value::Undef
                    } else {
                        Value::Bool(false)
                    }
                }
            }
        }
    }
}

/// Evaluate a user-defined function call.
fn eval_user_function_call(function_name: &str, args: &[CompiledExpr], ctx: &EvalContext) -> Value {
    // Evaluate arguments
    let evaluated_args: Vec<Value> = args.iter().map(|a| eval_expr(a, ctx)).collect();

    // Strict Undef propagation: if any arg is Undef, short-circuit
    if evaluated_args.iter().any(|v| v.is_undef()) {
        return Value::Undef;
    }

    // Check recursion depth
    if ctx.recursion_depth >= MAX_RECURSION_DEPTH {
        return Value::Undef;
    }

    // Look up function by name, arity, and param types (to disambiguate overloads).
    // The compiler uses exact type matching during resolution, so the compiled args'
    // result_types exactly equal the selected overload's param types. Matching on
    // these result_types selects the same overload the compiler chose.
    let func = ctx.functions.iter().find(|f| {
        f.name == function_name
            && f.params.len() == args.len()
            && f.params
                .iter()
                .zip(args.iter())
                .all(|((_, param_ty), arg)| *param_ty == arg.result_type)
    });

    let func = match func {
        Some(f) => f,
        None => return Value::Undef, // no matching function
    };

    // Build fresh scope with parameter bindings
    let mut scope = ValueMap::new();
    for ((param_name, _param_type), arg_val) in func.params.iter().zip(evaluated_args) {
        scope.insert(ValueCellId::new(&func.name, param_name), arg_val);
    }

    // Evaluate let bindings in order, growing the scope
    for (binding_name, binding_expr) in &func.body.let_bindings {
        let val = {
            let body_ctx = ctx.with_scope(&scope);
            eval_expr(binding_expr, &body_ctx)
        };
        scope.insert(ValueCellId::new(&func.name, binding_name), val);
    }

    // Evaluate result expression with final scope
    let final_ctx = ctx.with_scope(&scope);
    eval_expr(&func.body.result_expr, &final_ctx)
}

/// Apply a lambda closure to a list of argument values.
///
/// Returns Undef if:
/// - The value is not a Lambda
/// - Argument count doesn't match param count
/// - Recursion depth has reached MAX_RECURSION_DEPTH
pub fn apply_lambda(lambda: &Value, args: &[Value], ctx: &EvalContext) -> Value {
    match lambda {
        Value::Lambda {
            params,
            body,
            captures,
        } => {
            // Check depth before any work (consistent with eval_user_function_call)
            if ctx.recursion_depth >= MAX_RECURSION_DEPTH {
                return Value::Undef;
            }

            if args.len() != params.len() {
                return Value::Undef;
            }

            let mut eval_map = captures.clone();
            for ((_, id), arg) in params.iter().zip(args.iter()) {
                eval_map.insert(id.clone(), arg.clone());
            }

            eval_expr(body, &ctx.with_scope(&eval_map))
        }
        _ => Value::Undef,
    }
}

/// Compute the gradient of a field value.
///
/// Returns a new Field with `FieldSourceKind::Gradient` whose lambda slot stores the
/// original field. The sample handler detects this pattern and dispatches to
/// `compute_numerical_gradient_at_point` for central-difference evaluation.
///
/// Validation:
/// - Argument must be a Field with Analytical or Composed source (Gradient fields
///   are excluded: their lambda slot contains a Value::Field, not a callable Lambda)
/// - Lambda must be a Lambda (not Undef)
/// - Domain must have scalar quantity components (Real, Int, Scalar, or Point{n, scalar})
fn compute_gradient(field_val: &Value) -> Value {
    let (domain_type, codomain_type, source, lambda) = match field_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => (domain_type, codomain_type, source, lambda),
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] gradient: argument is not a Field: {:?}",
                field_val
            );
            return Value::Undef;
        }
    };

    // Only Analytical and Composed fields support gradient.
    // Gradient fields are excluded: their lambda slot contains a Value::Field (the
    // original field), not a callable Value::Lambda, so numerical differentiation
    // via apply_lambda is not possible.
    if !matches!(source, FieldSourceKind::Analytical | FieldSourceKind::Composed) {
        return Value::Undef;
    }

    // Lambda must be a callable Lambda
    if !matches!(lambda.as_ref(), Value::Lambda { .. }) {
        return Value::Undef;
    }

    // Determine dimensionality and validate scalar domain quantity
    let n = match domain_type {
        Type::Real | Type::Int | Type::Scalar { .. } => 1,
        Type::Point { n, quantity } => {
            if !matches!(quantity.as_ref(), Type::Real | Type::Int | Type::Scalar { .. }) {
                return Value::Undef;
            }
            *n
        }
        _ => return Value::Undef,
    };

    // Compute gradient quantity type: R/Q (codomain dimension / domain dimension).
    // Mirrors the runtime logic in compute_numerical_gradient_at_point (line 756).
    let gradient_quantity = {
        // Extract domain dimension from the domain type.
        let domain_dim = match domain_type {
            Type::Scalar { dimension } => Some(*dimension),
            Type::Point { quantity, .. } => match quantity.as_ref() {
                Type::Scalar { dimension } => Some(*dimension),
                _ => None,
            },
            _ => None,
        };

        // Extract codomain dimension.
        let codomain_dim = match codomain_type {
            Type::Scalar { dimension } => Some(*dimension),
            _ => None,
        };

        // Divide codomain by domain dimension when both are non-trivial.
        match (codomain_dim, domain_dim) {
            (Some(cd), Some(dd))
                if cd != DimensionVector::DIMENSIONLESS
                    && dd != DimensionVector::DIMENSIONLESS =>
            {
                let grad_dim = cd.div(&dd);
                if grad_dim != DimensionVector::DIMENSIONLESS {
                    Type::Scalar {
                        dimension: grad_dim,
                    }
                } else {
                    Type::Real
                }
            }
            _ => match codomain_type {
                Type::Scalar { dimension }
                    if *dimension == DimensionVector::DIMENSIONLESS =>
                {
                    Type::Real
                }
                _ => codomain_type.clone(),
            },
        }
    };

    // Construct result codomain type:
    // 1D → gradient_quantity (scalar derivative with correct R/Q dimension)
    // nD → Vector{n, gradient_quantity}
    let result_codomain = if n == 1 {
        gradient_quantity
    } else {
        Type::Vector {
            n,
            quantity: Box::new(gradient_quantity),
        }
    };

    // Return a gradient field: source=Gradient, lambda slot stores the original field.
    // The sample handler detects lambda=Field + source=Gradient and dispatches to
    // compute_numerical_gradient_at_point.
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::Gradient,
        lambda: Box::new(field_val.clone()),
    }
}

/// Compute the numerical gradient of a field at a given point via central differences.
///
/// For each axis i, perturbs coordinate i by ±h where h = 1e-6 * max(|coord_i|, 1e-3),
/// evaluates the original lambda, and computes df/dx_i ≈ (f(x+h) - f(x-h)) / (2h).
///
/// Returns:
/// - Scalar (Real) for 1D fields
/// - Vector for nD fields
/// - Undef if any perturbation evaluation fails
fn compute_numerical_gradient_at_point(
    lambda: &Value,
    point: &Value,
    domain_type: &Type,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    // Decompose point into f64 coordinates.
    // Guard every extracted value with is_finite() — NaN/Inf coordinates
    // would produce NaN step sizes and silently corrupt gradient results.
    let coords: Vec<f64> = match point {
        Value::Real(r) if r.is_finite() => vec![*r],
        Value::Real(_) => return Value::Undef, // NaN or Inf
        Value::Int(i) => vec![*i as f64],      // i64 can never be NaN/Inf
        Value::Scalar { si_value, .. } if si_value.is_finite() => vec![*si_value],
        Value::Scalar { .. } => return Value::Undef, // NaN or Inf
        Value::Point(items) | Value::Vector(items) => {
            let mut v = Vec::with_capacity(items.len());
            for item in items {
                match item.as_f64() {
                    Some(f) if f.is_finite() => v.push(f),
                    _ => return Value::Undef, // None (non-numeric) or NaN/Inf
                }
            }
            v
        }
        _ => return Value::Undef,
    };

    let n = coords.len();
    if n == 0 {
        return Value::Undef;
    }

    // Determine if domain is dimensioned (for constructing perturbed args)
    let domain_dim = match domain_type {
        Type::Scalar { dimension } => Some(*dimension),
        Type::Point { quantity, .. } => match quantity.as_ref() {
            Type::Scalar { dimension } => Some(*dimension),
            _ => None,
        },
        _ => None,
    };

    // Detect calling convention: single-Point param vs decomposed params.
    // If lambda has 1 param and n > 1, wrap perturbed coords in a Value::Point.
    // If lambda has n params, pass individual scalar values (current behavior).
    let single_point_param = match lambda {
        Value::Lambda { params, .. } => params.len() == 1 && n > 1,
        _ => false,
    };

    // Warn in debug builds when arity doesn't match and calling convention is
    // decomposed. An arity mismatch silently produces Undef via apply_lambda;
    // the warning surfaces the root cause during development.
    #[cfg(debug_assertions)]
    if let Value::Lambda { params, .. } = lambda
        && !single_point_param && params.len() != n
    {
        eprintln!(
            "[reify-expr] gradient: lambda has {} params but point has {} coords",
            params.len(),
            n
        );
    }

    // Compute grad_dim once from codomain_type (avoids f_plus.dimension() per iteration).
    // The gradient is df/dx, so its dimension is [codomain] / [domain].
    let result_dim = match codomain_type {
        Type::Scalar { dimension } => *dimension,
        _ => DimensionVector::DIMENSIONLESS,
    };
    let grad_dim = match domain_dim {
        Some(dd) if result_dim != DimensionVector::DIMENSIONLESS => result_dim.div(&dd),
        _ => result_dim,
    };

    // Hoist make_arg before the loop — it only captures domain_dim (Copy),
    // so constructing it once per axis would be redundant.
    let make_arg = |val: f64| -> Value {
        match domain_dim {
            Some(dim) => Value::Scalar {
                si_value: val,
                dimension: dim,
            },
            None => Value::Real(val),
        }
    };

    // Pre-allocate work_coords (one-time clone) and work_args for perturb-in-place.
    // Each axis: perturb work_coords[i] by +h, eval, swing to -h, eval, restore.
    // This reduces per-axis allocation from O(n) clones to O(1) f64 additions.
    let mut work_coords = coords.clone();
    // Capacity: 1 for single_point_param (wraps in a Point), n for decomposed.
    let args_capacity = if single_point_param { 1 } else { n };
    let mut work_args: Vec<Value> = Vec::with_capacity(args_capacity);

    let mut gradient_components = Vec::with_capacity(n);

    for i in 0..n {
        let coord_i = coords[i];
        let h = 1e-6_f64 * coord_i.abs().max(1e-3);

        // Perturb forward (+h), evaluate
        work_coords[i] += h;
        work_args.clear();
        if single_point_param {
            work_args.push(Value::Point(work_coords.iter().map(|&v| make_arg(v)).collect()));
        } else {
            work_args.extend(work_coords.iter().map(|&v| make_arg(v)));
        }
        let f_plus = apply_lambda(lambda, &work_args, ctx);

        // Swing to backward (−h from original = −2h from current), evaluate
        work_coords[i] -= 2.0 * h;
        work_args.clear();
        if single_point_param {
            work_args.push(Value::Point(work_coords.iter().map(|&v| make_arg(v)).collect()));
        } else {
            work_args.extend(work_coords.iter().map(|&v| make_arg(v)));
        }
        let f_minus = apply_lambda(lambda, &work_args, ctx);

        // Restore coord[i] to original value
        work_coords[i] += h;

        // Extract numeric values, propagate Undef.
        // Guard with is_finite() — as_f64() returns Some(NaN) for
        // Value::Real(NaN) and Some(Inf) for Value::Real(Inf), so
        // the None check alone doesn't catch degenerate values.
        let fp = match f_plus.as_f64() {
            Some(v) if v.is_finite() => v,
            _ => return Value::Undef,
        };
        let fm = match f_minus.as_f64() {
            Some(v) if v.is_finite() => v,
            _ => return Value::Undef,
        };

        let deriv = (fp - fm) / (2.0 * h);
        // Guard the derivative: even finite fp/fm can produce
        // Inf via overflow (e.g., (MAX - (-MAX)) / small_h).
        if !deriv.is_finite() {
            return Value::Undef;
        }

        if grad_dim != DimensionVector::DIMENSIONLESS {
            gradient_components.push(Value::Scalar {
                si_value: deriv,
                dimension: grad_dim,
            });
        } else {
            gradient_components.push(Value::Real(deriv));
        }
    }

    if n == 1 {
        gradient_components.into_iter().next().unwrap_or(Value::Undef)
    } else {
        Value::Vector(gradient_components)
    }
}

/// Convert a Value that carries NaN or Inf to Undef.
///
/// All callers pass either a `Value::from_component(...)` result — which
/// returns only `Value::Real` (dimensionless) or `Value::Scalar` (dimensioned)
/// — or a directly-constructed `Value::Scalar`.  Consequently only the
/// `Value::Real` and `Value::Scalar` arms are reachable from current call
/// sites; the `Value::Orientation` arm is unreachable today but is included
/// for structural parity with `reify-stdlib::sanitize_value` (defense-in-depth:
/// if callers evolve to pass orientation values, sanitization is already in
/// place).
///
/// **Divergence from stdlib:** the `Value::Complex` arm present in
/// `reify-stdlib::sanitize_value` is intentionally absent here — it was
/// removed as unreachable by task 860.  Restoring it for full SYNC parity
/// is tracked as a separate follow-up.
///
/// This helper mirrors the private `sanitize_value` in `reify-stdlib` — the
/// duplication is intentional (making stdlib's version public would widen its
/// API surface; moving it to reify-types would add evaluation semantics to a
/// type crate).
// SYNC: mirror of reify-stdlib::sanitize_value — keep in sync
fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if x.is_nan() || x.is_infinite() => Value::Undef,
        Value::Scalar { si_value, .. } if si_value.is_nan() || si_value.is_infinite() => {
            Value::Undef
        }
        Value::Orientation { w, x, y, z }
            if w.is_nan()
                || w.is_infinite()
                || x.is_nan()
                || x.is_infinite()
                || y.is_nan()
                || y.is_infinite()
                || z.is_nan()
                || z.is_infinite() =>
        {
            Value::Undef
        }
        _ => v,
    }
}

/// Evaluate a method call on a collection value.
fn eval_method_call(
    obj: &Value,
    method: &str,
    args: &[Value],
    result_type: &Type,
    ctx: &EvalContext,
) -> Value {
    match method {
        "count" => match obj {
            Value::List(items) => {
                if items.iter().any(|v| v.is_undef()) {
                    Value::Undef
                } else {
                    Value::Int(items.len() as i64)
                }
            }
            Value::Set(items) => {
                if items.iter().any(|v| v.is_undef()) {
                    Value::Undef
                } else {
                    Value::Int(items.len() as i64)
                }
            }
            Value::Map(entries) => Value::Int(entries.len() as i64),
            _ => Value::Undef,
        },
        "contains" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            let needle = &args[0];
            match obj {
                Value::List(items) => Value::Bool(items.contains(needle)),
                Value::Set(items) => Value::Bool(items.contains(needle)),
                Value::Range {
                    lower,
                    upper,
                    lower_inclusive,
                    upper_inclusive,
                } => {
                    // Undef needle propagates immediately.
                    if needle.is_undef() {
                        return Value::Undef;
                    }
                    // Check lower bound (if present).
                    if let Some(lo) = lower {
                        let cmp_result = if *lower_inclusive {
                            eval_cmp(lo, needle, |a, b| a <= b)
                        } else {
                            eval_cmp(lo, needle, |a, b| a < b)
                        };
                        match cmp_result {
                            Value::Bool(true) => {}
                            Value::Bool(false) => return Value::Bool(false),
                            _ => return Value::Undef,
                        }
                    }
                    // Check upper bound (if present).
                    if let Some(hi) = upper {
                        let cmp_result = if *upper_inclusive {
                            eval_cmp(needle, hi, |a, b| a <= b)
                        } else {
                            eval_cmp(needle, hi, |a, b| a < b)
                        };
                        match cmp_result {
                            Value::Bool(true) => {}
                            Value::Bool(false) => return Value::Bool(false),
                            _ => return Value::Undef,
                        }
                    }
                    Value::Bool(true)
                }
                _ => Value::Undef,
            }
        }
        "lower" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Range { lower, .. } => match lower {
                    Some(lo) => Value::Option(Some(lo.clone())),
                    None => Value::Option(None),
                },
                _ => Value::Undef,
            }
        }
        "upper" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Range { upper, .. } => match upper {
                    Some(hi) => Value::Option(Some(hi.clone())),
                    None => Value::Option(None),
                },
                _ => Value::Undef,
            }
        }
        "span" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Range { lower, upper, .. } => match (lower, upper) {
                    (Some(lo), Some(hi)) => eval_sub(hi, lo),
                    _ => Value::Undef,
                },
                _ => Value::Undef,
            }
        }
        "union" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            match (obj, &args[0]) {
                (Value::Set(a), Value::Set(b)) => Value::Set(a.union(b).cloned().collect()),
                _ => Value::Undef,
            }
        }
        "intersection" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            match (obj, &args[0]) {
                (Value::Set(a), Value::Set(b)) => Value::Set(a.intersection(b).cloned().collect()),
                _ => Value::Undef,
            }
        }
        "keys" => match obj {
            Value::Map(entries) => Value::List(entries.keys().cloned().collect()),
            _ => Value::Undef,
        },
        "values" => match obj {
            Value::Map(entries) => Value::List(entries.values().cloned().collect()),
            _ => Value::Undef,
        },
        "contains_key" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            match obj {
                Value::Map(entries) => Value::Bool(entries.contains_key(&args[0])),
                _ => Value::Undef,
            }
        }
        "difference" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            match (obj, &args[0]) {
                (Value::Set(a), Value::Set(b)) => Value::Set(a.difference(b).cloned().collect()),
                _ => Value::Undef,
            }
        }
        "sum" => match obj {
            Value::List(items) => {
                if items.is_empty() {
                    return match result_type {
                        Type::Int => Value::Int(0),
                        Type::Real => Value::Real(0.0),
                        Type::Scalar { dimension } => Value::Scalar {
                            si_value: 0.0,
                            dimension: *dimension,
                        },
                        _ => Value::Undef,
                    };
                }
                let mut acc = items[0].clone();
                if acc.is_undef() {
                    return Value::Undef;
                }
                for item in &items[1..] {
                    if item.is_undef() {
                        return Value::Undef;
                    }
                    acc = eval_add(&acc, item);
                    if acc.is_undef() {
                        return Value::Undef;
                    }
                }
                acc
            }
            _ => Value::Undef,
        },
        "map" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            let lambda = &args[0];
            match obj {
                Value::List(items) => {
                    let results: Vec<Value> = items
                        .iter()
                        .map(|item| apply_lambda(lambda, std::slice::from_ref(item), ctx))
                        .collect();
                    Value::List(results)
                }
                _ => Value::Undef,
            }
        }
        "all" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            let lambda = &args[0];
            match obj {
                Value::List(items) => {
                    let mut has_undef = false;
                    for item in items {
                        match apply_lambda(lambda, std::slice::from_ref(item), ctx) {
                            Value::Bool(false) => return Value::Bool(false),
                            Value::Bool(true) => {}
                            Value::Undef => has_undef = true,
                            _ => return Value::Undef,
                        }
                    }
                    if has_undef {
                        Value::Undef
                    } else {
                        Value::Bool(true)
                    }
                }
                _ => Value::Undef,
            }
        }
        "any" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            let lambda = &args[0];
            match obj {
                Value::List(items) => {
                    let mut has_undef = false;
                    for item in items {
                        match apply_lambda(lambda, std::slice::from_ref(item), ctx) {
                            Value::Bool(true) => return Value::Bool(true),
                            Value::Bool(false) => {}
                            Value::Undef => has_undef = true,
                            _ => return Value::Undef,
                        }
                    }
                    if has_undef {
                        Value::Undef
                    } else {
                        Value::Bool(false)
                    }
                }
                _ => Value::Undef,
            }
        }
        "fold" => {
            if args.len() != 2 {
                return Value::Undef;
            }
            let init = &args[0];
            let lambda = &args[1];
            // Validate lambda arity upfront (fold requires exactly 2 params: acc, item)
            if let Value::Lambda { params, .. } = lambda
                && params.len() != 2
            {
                return Value::Undef;
            }
            match obj {
                Value::List(items) => {
                    let mut acc = init.clone();
                    for item in items {
                        acc = apply_lambda(lambda, &[acc, item.clone()], ctx);
                        if acc.is_undef() {
                            return Value::Undef;
                        }
                    }
                    acc
                }
                _ => Value::Undef,
            }
        }
        "concat" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            match (obj, &args[0]) {
                (Value::List(a), Value::List(b)) => {
                    let mut result = a.clone();
                    result.extend(b.iter().cloned());
                    Value::List(result)
                }
                _ => Value::Undef,
            }
        }
        "generate" => {
            if args.len() != 2 {
                return Value::Undef;
            }
            let count = match &args[0] {
                Value::Int(n) => *n,
                _ => return Value::Undef,
            };
            let lambda = &args[1];
            match obj {
                Value::List(_) => {
                    let results: Vec<Value> = (0..count)
                        .map(|i| apply_lambda(lambda, &[Value::Int(i)], ctx))
                        .collect();
                    Value::List(results)
                }
                _ => Value::Undef,
            }
        }
        "filter" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            let lambda = &args[0];
            match obj {
                Value::List(items) => {
                    let mut results = Vec::new();
                    for item in items {
                        let pred = apply_lambda(lambda, std::slice::from_ref(item), ctx);
                        match pred {
                            Value::Bool(true) => results.push(item.clone()),
                            Value::Bool(false) => {} // skip
                            Value::Undef => results.push(item.clone()), // conservative: retain when predicate is unknown
                            _ => return Value::Undef, // type error: non-Bool predicate
                        }
                    }
                    Value::List(results)
                }
                _ => Value::Undef,
            }
        }
        "magnitude" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Complex { re, im, dimension } => {
                    let mag = re.hypot(*im);
                    sanitize_value(Value::from_component(mag, *dimension))
                }
                _ => Value::Undef,
            }
        }
        "phase" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Complex { re, im, .. } => {
                    if !re.is_finite() || !im.is_finite() {
                        return Value::Undef;
                    }
                    if *re == 0.0 && *im == 0.0 {
                        return Value::Undef;
                    }
                    // The pre-guard above is essential: atan2(y, Inf) = 0.0 and
                    // atan2(y, -Inf) = ±π are both finite, so sanitize_value alone
                    // cannot detect Inf inputs — it would silently return a wrong result.
                    // After the guard, atan2(finite, finite) with at least one non-zero
                    // argument always returns a value in [-π, π], so no output
                    // sanitization is needed here.
                    let angle = im.atan2(*re);
                    Value::Scalar {
                        si_value: angle,
                        dimension: DimensionVector::ANGLE,
                    }
                }
                _ => Value::Undef,
            }
        }
        "conjugate" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Complex { re, im, dimension } => Value::Complex {
                    re: *re,
                    im: -im,
                    dimension: *dimension,
                },
                _ => Value::Undef,
            }
        }
        "re" | "im" => {
            if !args.is_empty() {
                return Value::Undef;
            }
            match obj {
                Value::Complex { re, im, dimension } => {
                    let component = if method == "re" { *re } else { *im };
                    sanitize_value(Value::from_component(component, *dimension))
                }
                _ => Value::Undef,
            }
        }
        "x" | "y" | "z" => {
            let index = match method {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => unreachable!(),
            };
            match obj {
                Value::Tensor(components) => components.get(index).cloned().unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

fn eval_binop(op: BinOp, left: &CompiledExpr, right: &CompiledExpr, ctx: &EvalContext) -> Value {
    // Kleene three-valued logic: short-circuit with Undef support
    match op {
        BinOp::And => return eval_and(left, right, ctx),
        BinOp::Or => return eval_or(left, right, ctx),
        _ => {}
    }

    let lv = eval_expr(left, ctx);
    let rv = eval_expr(right, ctx);

    // Strict undef propagation for arithmetic/comparison
    if lv.is_undef() || rv.is_undef() {
        return Value::Undef;
    }

    match op {
        BinOp::Add => {
            // Point + Point is undefined: spec 3.3.1 prohibits adding two points
            if matches!(&left.result_type, Type::Point { .. })
                && matches!(&right.result_type, Type::Point { .. })
            {
                return Value::Undef;
            }
            eval_add(&lv, &rv)
        }
        BinOp::Sub => eval_sub(&lv, &rv),
        BinOp::Mul => eval_mul(&lv, &rv),
        BinOp::Div => eval_div(&lv, &rv),
        BinOp::Mod => eval_mod(&lv, &rv),
        BinOp::Pow => eval_pow(&lv, &rv),
        BinOp::Eq => eval_eq(&lv, &rv),
        BinOp::Ne => eval_ne(&lv, &rv),
        BinOp::Lt => eval_cmp(&lv, &rv, |a, b| a < b),
        BinOp::Le => eval_cmp(&lv, &rv, |a, b| a <= b),
        BinOp::Gt => eval_cmp(&lv, &rv, |a, b| a > b),
        BinOp::Ge => eval_cmp(&lv, &rv, |a, b| a >= b),
        BinOp::And | BinOp::Or => unreachable!(),
    }
}

/// Kleene AND: false ∧ Undef = false
fn eval_and(left: &CompiledExpr, right: &CompiledExpr, ctx: &EvalContext) -> Value {
    let lv = eval_expr(left, ctx);
    match lv {
        Value::Bool(false) => Value::Bool(false),
        Value::Bool(true) => {
            let rv = eval_expr(right, ctx);
            match rv {
                Value::Bool(b) => Value::Bool(b),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
        Value::Undef => {
            let rv = eval_expr(right, ctx);
            match rv {
                Value::Bool(false) => Value::Bool(false),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

/// Kleene OR: true ∨ Undef = true
fn eval_or(left: &CompiledExpr, right: &CompiledExpr, ctx: &EvalContext) -> Value {
    let lv = eval_expr(left, ctx);
    match lv {
        Value::Bool(true) => Value::Bool(true),
        Value::Bool(false) => {
            let rv = eval_expr(right, ctx);
            match rv {
                Value::Bool(b) => Value::Bool(b),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
        Value::Undef => {
            let rv = eval_expr(right, ctx);
            match rv {
                Value::Bool(true) => Value::Bool(true),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

/// Apply a binary operation component-wise to two equal-length component slices,
/// wrapping the result with the given constructor. Returns `Value::Undef` if either
/// slice is empty, lengths differ, or any component operation produces `Value::Undef`.
fn componentwise_binop(
    a: &[Value],
    b: &[Value],
    op: fn(&Value, &Value) -> Value,
    wrap: fn(Vec<Value>) -> Value,
) -> Value {
    if a.is_empty() {
        return Value::Undef;
    }
    if a.len() != b.len() {
        return Value::Undef;
    }
    match a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let r = op(x, y);
            if r.is_undef() { None } else { Some(r) }
        })
        .collect::<Option<Vec<Value>>>()
    {
        Some(results) => wrap(results),
        None => Value::Undef,
    }
}

/// Scale each component of a component slice by a scalar value using the given
/// binary operation, wrapping the result with the given constructor. Returns
/// `Value::Undef` if the scalar is Undef, components are empty, or any
/// component operation produces `Value::Undef`.
fn scale_components(
    components: &[Value],
    scalar: &Value,
    op: fn(&Value, &Value) -> Value,
    wrap: fn(Vec<Value>) -> Value,
) -> Value {
    if scalar.is_undef() {
        return Value::Undef;
    }
    if components.is_empty() {
        return Value::Undef;
    }
    match components
        .iter()
        .map(|c| {
            let r = op(c, scalar);
            if r.is_undef() { None } else { Some(r) }
        })
        .collect::<Option<Vec<Value>>>()
    {
        Some(results) => wrap(results),
        None => Value::Undef,
    }
}

/// Negate a scalar (leaf) value: Int, Real, Scalar, or Complex.
/// Returns `Value::Undef` for non-negatable types or Int overflow.
fn neg_scalar(v: Value) -> Value {
    match v {
        Value::Int(i) => i.checked_neg().map(Value::Int).unwrap_or(Value::Undef),
        Value::Real(r) => Value::Real(-r),
        Value::Scalar {
            si_value,
            dimension,
        } => Value::Scalar {
            si_value: -si_value,
            dimension,
        },
        Value::Complex { re, im, dimension } => {
            if !re.is_finite() || !im.is_finite() {
                return Value::Undef;
            }
            Value::Complex { re: -re, im: -im, dimension }
        }
        _ => Value::Undef,
    }
}

/// Negate each component in a slice, wrapping the result with the given
/// constructor.  Returns `Value::Undef` if components are empty or any
/// component negation produces `Value::Undef`.  Uses the Option-collect
/// pattern for single-pass early exit.
fn negate_components(components: &[Value], wrap: fn(Vec<Value>) -> Value) -> Value {
    if components.is_empty() {
        return Value::Undef;
    }
    match components
        .iter()
        .map(|c| {
            let r = negate_value(c.clone());
            if r.is_undef() { None } else { Some(r) }
        })
        .collect::<Option<Vec<Value>>>()
    {
        Some(results) => wrap(results),
        None => Value::Undef,
    }
}

/// Recursively negate a value.  Handles all negatable variants: Int, Real,
/// Scalar, Complex, Tensor, Vector, and Matrix (canonicalized to nested Tensor).
/// Point negation is explicitly undefined (spec 3.3.1).
fn negate_value(v: Value) -> Value {
    match v {
        Value::Int(_) | Value::Real(_) | Value::Scalar { .. } | Value::Complex { .. } => {
            neg_scalar(v)
        }
        Value::Tensor(components) => negate_components(&components, Value::Tensor),
        Value::Vector(components) => negate_components(&components, Value::Vector),
        Value::Matrix(rows) => negate_value(Value::Matrix(rows).canonicalize_matrix()),
        // Affine geometry: point negation is undefined (spec 3.3.1)
        Value::Point(_) => Value::Undef,
        _ => Value::Undef,
    }
}

/// Check if a tensor slice represents rank-2 data (all elements are Tensor).
/// Returns `true` if the first element is a Tensor; callers must verify `.all()`.
fn is_rank2(slice: &[Value]) -> bool {
    slice
        .first()
        .is_some_and(|v| matches!(v, Value::Tensor(_)))
}

/// Validate rank-2 tensor operands for addition/subtraction.
/// Returns `Some(Value::Undef)` if validation fails, `None` if tensors are valid
/// for componentwise operation (or if they are rank-1 and should fall through).
fn validate_rank2_tensors(a: &[Value], b: &[Value]) -> Option<Value> {
    let a_rank2 = is_rank2(a);
    let b_rank2 = is_rank2(b);

    // If neither is rank-2, let componentwise_binop handle it (rank-1 path).
    if !a_rank2 && !b_rank2 {
        return None;
    }

    // Mixed rank (one rank-2, one rank-1) → Undef.
    if a_rank2 != b_rank2 {
        return Some(Value::Undef);
    }

    // Both claim rank-2. Verify ALL rows are Tensor (not just first).
    if !a.iter().all(|r| matches!(r, Value::Tensor(_)))
        || !b.iter().all(|r| matches!(r, Value::Tensor(_)))
    {
        return Some(Value::Undef);
    }

    // Empty inner rows (0-column matrix) → Undef.
    let a_has_empty = a
        .iter()
        .any(|r| matches!(r, Value::Tensor(row) if row.is_empty()));
    let b_has_empty = b
        .iter()
        .any(|r| matches!(r, Value::Tensor(row) if row.is_empty()));
    if a_has_empty || b_has_empty {
        return Some(Value::Undef);
    }

    // Jagged validation: all rows in each operand must have the same column count.
    let a_cols = match &a[0] {
        Value::Tensor(r) => r.len(),
        _ => 0,
    };
    if !a
        .iter()
        .all(|r| matches!(r, Value::Tensor(row) if row.len() == a_cols))
    {
        return Some(Value::Undef);
    }
    let b_cols = match &b[0] {
        Value::Tensor(r) => r.len(),
        _ => 0,
    };
    if !b
        .iter()
        .all(|r| matches!(r, Value::Tensor(row) if row.len() == b_cols))
    {
        return Some(Value::Undef);
    }

    // Valid rank-2: fall through to componentwise_binop.
    None
}

fn eval_add(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
        (Value::Real(a), Value::Real(b)) => Value::Real(a + b),
        (Value::Int(a), Value::Real(b)) | (Value::Real(b), Value::Int(a)) => {
            Value::Real(*a as f64 + b)
        }
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => {
            if ad != bd {
                Value::Undef // dimension mismatch
            } else {
                Value::Scalar {
                    si_value: a + b,
                    dimension: *ad,
                }
            }
        }
        // Complex + Complex: dimension must match
        (
            Value::Complex {
                re: ar,
                im: ai,
                dimension: ad,
            },
            Value::Complex {
                re: br,
                im: bi,
                dimension: bd,
            },
        ) => {
            if ad != bd {
                Value::Undef
            } else {
                Value::Complex {
                    re: ar + br,
                    im: ai + bi,
                    dimension: *ad,
                }
            }
        }
        (Value::String(a), Value::String(b)) => Value::String(format!("{}{}", a, b)),
        // Component-wise Tensor addition (with rank-2 validation)
        (Value::Tensor(a), Value::Tensor(b)) => {
            if let Some(undef) = validate_rank2_tensors(a, b) {
                return undef;
            }
            componentwise_binop(a, b, eval_add, Value::Tensor)
        }
        // Affine geometry: Vector + Vector → Vector
        (Value::Vector(a), Value::Vector(b)) => componentwise_binop(a, b, eval_add, Value::Vector),
        // Affine geometry: Point + Vector or Vector + Point → Point (displacement)
        (Value::Point(a), Value::Vector(b)) | (Value::Vector(b), Value::Point(a)) => {
            componentwise_binop(a, b, eval_add, Value::Point)
        }
        // Affine geometry: Point + Point is undefined
        (Value::Point(_), Value::Point(_)) => Value::Undef,
        _ => Value::Undef,
    }
}

fn eval_sub(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
        (Value::Real(a), Value::Real(b)) => Value::Real(a - b),
        (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 - b),
        (Value::Real(a), Value::Int(b)) => Value::Real(a - *b as f64),
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => {
            if ad != bd {
                Value::Undef // dimension mismatch
            } else {
                Value::Scalar {
                    si_value: a - b,
                    dimension: *ad,
                }
            }
        }
        // Complex - Complex: dimension must match
        (
            Value::Complex {
                re: ar,
                im: ai,
                dimension: ad,
            },
            Value::Complex {
                re: br,
                im: bi,
                dimension: bd,
            },
        ) => {
            if ad != bd {
                Value::Undef
            } else {
                Value::Complex {
                    re: ar - br,
                    im: ai - bi,
                    dimension: *ad,
                }
            }
        }
        // Component-wise Tensor subtraction (with rank-2 validation)
        (Value::Tensor(a), Value::Tensor(b)) => {
            if let Some(undef) = validate_rank2_tensors(a, b) {
                return undef;
            }
            componentwise_binop(a, b, eval_sub, Value::Tensor)
        }
        // Affine geometry: Point - Point → Vector (displacement)
        (Value::Point(a), Value::Point(b)) => componentwise_binop(a, b, eval_sub, Value::Vector),
        // Affine geometry: Point - Vector → Point (point displaced backwards)
        (Value::Point(a), Value::Vector(b)) => componentwise_binop(a, b, eval_sub, Value::Point),
        // Affine geometry: Vector - Vector → Vector
        (Value::Vector(a), Value::Vector(b)) => componentwise_binop(a, b, eval_sub, Value::Vector),
        // Vector - Point falls through to Undef (no geometric meaning)
        _ => Value::Undef,
    }
}

// ── Quaternion math helpers (private, for Transform evaluation) ──────────────

/// Hamilton product of two quaternions (w, x, y, z).
fn quat_mul_t(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (
        a.0 * b.0 - a.1 * b.1 - a.2 * b.2 - a.3 * b.3,
        a.0 * b.1 + a.1 * b.0 + a.2 * b.3 - a.3 * b.2,
        a.0 * b.2 - a.1 * b.3 + a.2 * b.0 + a.3 * b.1,
        a.0 * b.3 + a.1 * b.2 - a.2 * b.1 + a.3 * b.0,
    )
}

/// Quaternion conjugate (inverse for unit quaternions).
fn quat_conj(q: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (q.0, -q.1, -q.2, -q.3)
}

/// Rotate a 3D vector (vx, vy, vz) by unit quaternion q: q * (0,v) * conj(q).
fn quat_rotate(q: (f64, f64, f64, f64), vx: f64, vy: f64, vz: f64) -> (f64, f64, f64) {
    let v_quat = (0.0, vx, vy, vz);
    let result = quat_mul_t(quat_mul_t(q, v_quat), quat_conj(q));
    (result.1, result.2, result.3)
}

/// Extract (f64, f64, f64) triple and DimensionVector from a 3-element Value slice.
/// Returns None if the slice has wrong length or contains non-numeric values.
fn vec3_components(items: &[Value]) -> Option<(f64, f64, f64, DimensionVector)> {
    if items.len() != 3 {
        return None;
    }
    let x = items[0].as_f64()?;
    let y = items[1].as_f64()?;
    let z = items[2].as_f64()?;
    // Reject NaN and Infinity
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let dim = items[0].dimension();
    // All three components must share the same dimension
    if items[1].dimension() != dim || items[2].dimension() != dim {
        return None;
    }
    Some((x, y, z, dim))
}

/// Reconstruct a Vec<Value> from a (f64, f64, f64) triple and a DimensionVector.
fn make_components_3(x: f64, y: f64, z: f64, dim: DimensionVector) -> Vec<Value> {
    if dim == DimensionVector::DIMENSIONLESS {
        vec![Value::Real(x), Value::Real(y), Value::Real(z)]
    } else {
        vec![
            Value::Scalar {
                si_value: x,
                dimension: dim,
            },
            Value::Scalar {
                si_value: y,
                dimension: dim,
            },
            Value::Scalar {
                si_value: z,
                dimension: dim,
            },
        ]
    }
}

fn eval_mul(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a * b),
        (Value::Real(a), Value::Real(b)) => Value::Real(a * b),
        (Value::Int(a), Value::Real(b)) | (Value::Real(b), Value::Int(a)) => {
            Value::Real(*a as f64 * b)
        }
        // Complex * Complex: (ac-bd) + (ad+bc)i, dimensions multiply
        (
            Value::Complex {
                re: ar,
                im: ai,
                dimension: ad,
            },
            Value::Complex {
                re: br,
                im: bi,
                dimension: bd,
            },
        ) => Value::Complex {
            re: ar * br - ai * bi,
            im: ar * bi + ai * br,
            dimension: ad.mul(bd),
        },
        // Scalar * Scalar: multiply values, add dimension exponents
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => Value::Scalar {
            si_value: a * b,
            dimension: ad.mul(bd),
        },
        // Scalar * dimensionless numeric
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Int(n),
        )
        | (
            Value::Int(n),
            Value::Scalar {
                si_value,
                dimension,
            },
        ) => Value::Scalar {
            si_value: si_value * *n as f64,
            dimension: *dimension,
        },
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Real(r),
        )
        | (
            Value::Real(r),
            Value::Scalar {
                si_value,
                dimension,
            },
        ) => Value::Scalar {
            si_value: si_value * r,
            dimension: *dimension,
        },
        // Complex * Scalar | Scalar * Complex: scale re/im, combine dimensions
        (
            Value::Complex {
                re,
                im,
                dimension: cd,
            },
            Value::Scalar {
                si_value,
                dimension: sd,
            },
        )
        | (
            Value::Scalar {
                si_value,
                dimension: sd,
            },
            Value::Complex {
                re,
                im,
                dimension: cd,
            },
        ) => Value::Complex {
            re: re * si_value,
            im: im * si_value,
            dimension: cd.mul(sd),
        },
        // Complex * Int | Int * Complex: dimensionless multiplier preserves dimension
        (Value::Complex { re, im, dimension }, Value::Int(n))
        | (Value::Int(n), Value::Complex { re, im, dimension }) => Value::Complex {
            re: re * *n as f64,
            im: im * *n as f64,
            dimension: *dimension,
        },
        // Complex * Real | Real * Complex: dimensionless multiplier preserves dimension
        (Value::Complex { re, im, dimension }, Value::Real(r))
        | (Value::Real(r), Value::Complex { re, im, dimension }) => Value::Complex {
            re: re * r,
            im: im * r,
            dimension: *dimension,
        },
        // Scalar * Tensor or Tensor * Scalar: scale each component
        (Value::Tensor(components), scalar) | (scalar, Value::Tensor(components))
            if !matches!(scalar, Value::Tensor(_)) =>
        {
            scale_components(components, scalar, eval_mul, Value::Tensor)
        }
        // Scalar * Vector or Vector * Scalar: scale each component → Vector
        (Value::Vector(components), scalar) | (scalar, Value::Vector(components))
            if !matches!(
                scalar,
                Value::Vector(_) | Value::Point(_) | Value::Tensor(_) | Value::Transform { .. }
            ) =>
        {
            scale_components(components, scalar, eval_mul, Value::Vector)
        }
        // Scalar * Point or Point * Scalar: scale each component → Point
        // Pragmatic deviation from strict affine rules: needed for weighted
        // interpolation and barycentric coordinates.
        (Value::Point(components), scalar) | (scalar, Value::Point(components))
            if !matches!(
                scalar,
                Value::Vector(_) | Value::Point(_) | Value::Tensor(_) | Value::Transform { .. }
            ) =>
        {
            scale_components(components, scalar, eval_mul, Value::Point)
        }
        // Transform * Vector: apply rotation only (translation is ignored for vectors)
        (Value::Transform { rotation, .. }, Value::Vector(components)) => {
            if let Value::Orientation { w, x, y, z } = rotation.as_ref() {
                if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
                    return Value::Undef;
                }
                let norm = (w * w + x * x + y * y + z * z).sqrt();
                if norm < f64::EPSILON {
                    return Value::Undef;
                }
                let q = (w / norm, x / norm, y / norm, z / norm);
                if let Some((vx, vy, vz, dim)) = vec3_components(components) {
                    let (rx, ry, rz) = quat_rotate(q, vx, vy, vz);
                    Value::Vector(make_components_3(rx, ry, rz, dim))
                } else {
                    Value::Undef
                }
            } else {
                Value::Undef
            }
        }
        // Transform * Point: apply rotation then add translation
        (
            Value::Transform {
                rotation,
                translation,
            },
            Value::Point(components),
        ) => {
            if let Value::Orientation { w, x, y, z } = rotation.as_ref() {
                if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
                    return Value::Undef;
                }
                let norm = (w * w + x * x + y * y + z * z).sqrt();
                if norm < f64::EPSILON {
                    return Value::Undef;
                }
                let q = (w / norm, x / norm, y / norm, z / norm);
                if let Some((px, py, pz, p_dim)) = vec3_components(components) {
                    if let Value::Vector(t_items) = translation.as_ref() {
                        if let Some((tx, ty, tz, t_dim)) = vec3_components(t_items) {
                            // Dimension check: point and translation must share dimension
                            if p_dim != t_dim {
                                return Value::Undef;
                            }
                            let (rx, ry, rz) = quat_rotate(q, px, py, pz);
                            Value::Point(make_components_3(rx + tx, ry + ty, rz + tz, p_dim))
                        } else {
                            Value::Undef
                        }
                    } else {
                        Value::Undef
                    }
                } else {
                    Value::Undef
                }
            } else {
                Value::Undef
            }
        }
        // Transform * Transform: composition (R1,t1)*(R2,t2) = (R1*R2, R1*t2+t1)
        (
            Value::Transform {
                rotation: r1,
                translation: t1,
            },
            Value::Transform {
                rotation: r2,
                translation: t2,
            },
        ) => {
            if let (
                Value::Orientation {
                    w: w1,
                    x: x1,
                    y: y1,
                    z: z1,
                },
                Value::Orientation {
                    w: w2,
                    x: x2,
                    y: y2,
                    z: z2,
                },
            ) = (r1.as_ref(), r2.as_ref())
            {
                if let (Value::Vector(t1_items), Value::Vector(t2_items)) =
                    (t1.as_ref(), t2.as_ref())
                {
                    if let (Some((t1x, t1y, t1z, t1_dim)), Some((t2x, t2y, t2z, t2_dim))) =
                        (vec3_components(t1_items), vec3_components(t2_items))
                    {
                        // Dimension check: both translations must share dimension
                        if t1_dim != t2_dim {
                            return Value::Undef;
                        }
                        // Validate and normalize q1 for translation rotation
                        if !w1.is_finite() || !x1.is_finite() || !y1.is_finite() || !z1.is_finite()
                        {
                            return Value::Undef;
                        }
                        let q1_norm = (w1 * w1 + x1 * x1 + y1 * y1 + z1 * z1).sqrt();
                        if q1_norm < f64::EPSILON {
                            return Value::Undef;
                        }
                        let q1_n = (w1 / q1_norm, x1 / q1_norm, y1 / q1_norm, z1 / q1_norm);
                        // Compose rotations: R = R1 * R2
                        let (rw, rx, ry, rz) = quat_mul_t(q1_n, (*w2, *x2, *y2, *z2));
                        // Normalize result quaternion (reject NaN/Inf/zero-length)
                        if !rw.is_finite() || !rx.is_finite() || !ry.is_finite() || !rz.is_finite()
                        {
                            return Value::Undef;
                        }
                        let norm = (rw * rw + rx * rx + ry * ry + rz * rz).sqrt();
                        if norm < f64::EPSILON {
                            return Value::Undef;
                        }
                        let (rw, rx, ry, rz) = (rw / norm, rx / norm, ry / norm, rz / norm);
                        // Compose translations: t = R1 * t2 + t1
                        let (rt2x, rt2y, rt2z) = quat_rotate(q1_n, t2x, t2y, t2z);
                        Value::Transform {
                            rotation: Box::new(Value::Orientation {
                                w: rw,
                                x: rx,
                                y: ry,
                                z: rz,
                            }),
                            translation: Box::new(Value::Vector(make_components_3(
                                rt2x + t1x,
                                rt2y + t1y,
                                rt2z + t1z,
                                t1_dim,
                            ))),
                        }
                    } else {
                        Value::Undef
                    }
                } else {
                    Value::Undef
                }
            } else {
                Value::Undef
            }
        }
        _ => Value::Undef,
    }
}

fn eval_div(lv: &Value, rv: &Value) -> Value {
    // Check for division by zero
    if let Some(denom) = rv.as_f64()
        && (denom == 0.0 || denom.is_nan())
    {
        return Value::Undef;
    }

    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Value::Undef
            } else if a % b == 0 {
                Value::Int(a / b)
            } else {
                Value::Real(*a as f64 / *b as f64)
            }
        }
        (Value::Real(a), Value::Real(b)) => Value::Real(a / b),
        (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 / b),
        (Value::Real(a), Value::Int(b)) => Value::Real(a / *b as f64),
        // Scalar / Scalar: divide values, subtract dimension exponents
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => {
            let result_dim = ad.div(bd);
            Value::Scalar {
                si_value: a / b,
                dimension: result_dim,
            }
        }
        // Scalar / dimensionless
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Int(n),
        ) => Value::Scalar {
            si_value: si_value / *n as f64,
            dimension: *dimension,
        },
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Real(r),
        ) => Value::Scalar {
            si_value: si_value / r,
            dimension: *dimension,
        },
        // Complex / Scalar: divide re/im, combine dimensions
        (
            Value::Complex {
                re,
                im,
                dimension: cd,
            },
            Value::Scalar {
                si_value,
                dimension: sd,
            },
        ) => Value::Complex {
            re: re / si_value,
            im: im / si_value,
            dimension: cd.div(sd),
        },
        // Complex / Int: preserve dimension
        (Value::Complex { re, im, dimension }, Value::Int(n)) => Value::Complex {
            re: re / *n as f64,
            im: im / *n as f64,
            dimension: *dimension,
        },
        // Complex / Real: preserve dimension
        (Value::Complex { re, im, dimension }, Value::Real(r)) => Value::Complex {
            re: re / r,
            im: im / r,
            dimension: *dimension,
        },
        // Tensor / Scalar: divide each component by the scalar
        (Value::Tensor(components), scalar) if !matches!(scalar, Value::Tensor(_)) => {
            scale_components(components, scalar, eval_div, Value::Tensor)
        }
        // Vector / Scalar: divide each component by the scalar → Vector
        (Value::Vector(components), scalar)
            if !matches!(
                scalar,
                Value::Vector(_) | Value::Point(_) | Value::Tensor(_)
            ) =>
        {
            scale_components(components, scalar, eval_div, Value::Vector)
        }
        // Point / Scalar: divide each component by the scalar → Point
        // Pragmatic deviation from strict affine rules (needed for interpolation).
        (Value::Point(components), scalar)
            if !matches!(
                scalar,
                Value::Vector(_) | Value::Point(_) | Value::Tensor(_)
            ) =>
        {
            scale_components(components, scalar, eval_div, Value::Point)
        }
        _ => Value::Undef,
    }
}

fn eval_mod(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Value::Undef
            } else {
                Value::Int(a % b)
            }
        }
        (Value::Real(a), Value::Real(b)) => {
            if *b == 0.0 {
                Value::Undef
            } else {
                Value::Real(a % b)
            }
        }
        _ => Value::Undef,
    }
}

fn eval_pow(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(base), Value::Int(exp)) => {
            if *exp >= 0 && *exp <= i32::MAX as i64 {
                Value::Int(base.pow(*exp as u32))
            } else {
                Value::Real((*base as f64).powi(*exp as i32))
            }
        }
        (Value::Real(base), Value::Int(exp)) => Value::Real(base.powi(*exp as i32)),
        (Value::Real(base), Value::Real(exp)) => Value::Real(base.powf(*exp)),
        (Value::Int(base), Value::Real(exp)) => Value::Real((*base as f64).powf(*exp)),
        // Scalar ^ Int: raise value, multiply dimension exponents
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Int(n),
        ) => Value::Scalar {
            si_value: si_value.powi(*n as i32),
            dimension: dimension.pow(*n as i8),
        },
        _ => Value::Undef,
    }
}

fn eval_eq(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Bool(a), Value::Bool(b)) => Value::Bool(a == b),
        (Value::Int(a), Value::Int(b)) => Value::Bool(a == b),
        (Value::String(a), Value::String(b)) => Value::Bool(a == b),
        // Scalar-vs-Scalar: compare dimensions first
        (
            Value::Scalar {
                si_value: a,
                dimension: da,
            },
            Value::Scalar {
                si_value: b,
                dimension: db,
            },
        ) => {
            if da != db {
                Value::Bool(false)
            } else {
                Value::Bool(a == b)
            }
        }
        // Enum-vs-Enum equality: same type → compare variant, different type → false
        (
            Value::Enum {
                type_name: a,
                variant: av,
            },
            Value::Enum {
                type_name: b,
                variant: bv,
            },
        ) => {
            if a == b {
                Value::Bool(av == bv)
            } else {
                Value::Bool(false)
            }
        }
        // Enum vs non-Enum: always false
        (Value::Enum { .. }, _) | (_, Value::Enum { .. }) => Value::Bool(false),
        // Dimensioned Scalar vs non-Scalar: not equal
        (Value::Scalar { dimension, .. }, _) | (_, Value::Scalar { dimension, .. })
            if !dimension.is_dimensionless() =>
        {
            Value::Bool(false)
        }
        _ => {
            // For numeric comparisons (Int/Real/dimensionless Scalar), compare as f64
            match (lv.as_f64(), rv.as_f64()) {
                (Some(a), Some(b)) => Value::Bool(a == b),
                _ => Value::Undef,
            }
        }
    }
}

fn eval_ne(lv: &Value, rv: &Value) -> Value {
    match eval_eq(lv, rv) {
        Value::Bool(b) => Value::Bool(!b),
        other => other,
    }
}

fn eval_cmp(lv: &Value, rv: &Value, cmp: fn(f64, f64) -> bool) -> Value {
    match (lv, rv) {
        // Scalar-vs-Scalar: compare dimensions first
        (
            Value::Scalar {
                si_value: a,
                dimension: da,
            },
            Value::Scalar {
                si_value: b,
                dimension: db,
            },
        ) => {
            if da != db {
                Value::Undef
            } else {
                Value::Bool(cmp(*a, *b))
            }
        }
        // Enum comparison: no ordering on enums
        (Value::Enum { .. }, _) | (_, Value::Enum { .. }) => Value::Undef,
        // Dimensioned Scalar vs non-Scalar: incomparable
        (Value::Scalar { dimension, .. }, _) | (_, Value::Scalar { dimension, .. })
            if !dimension.is_dimensionless() =>
        {
            Value::Undef
        }
        // Fallback: Int/Real/dimensionless Scalar via as_f64
        _ => match (lv.as_f64(), rv.as_f64()) {
            (Some(a), Some(b)) => Value::Bool(cmp(a, b)),
            _ => Value::Undef,
        },
    }
}

fn eval_unop(op: UnOp, operand: &CompiledExpr, ctx: &EvalContext) -> Value {
    let v = eval_expr(operand, ctx);
    if v.is_undef() {
        return Value::Undef;
    }
    match op {
        UnOp::Neg => negate_value(v),
        UnOp::Not => match v {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Undef,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{CompiledMatchArm, DimensionVector, Type, ValueCellId};

    // Helper to build a literal expression
    fn lit(v: Value, ty: Type) -> CompiledExpr {
        CompiledExpr::literal(v, ty)
    }

    fn vref(entity: &str, member: &str, ty: Type) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new(entity, member), ty)
    }

    fn mm_val(v: f64) -> Value {
        Value::Scalar {
            si_value: v * 0.001,
            dimension: DimensionVector::LENGTH,
        }
    }

    #[test]
    fn literal_evaluation() {
        let expr = lit(Value::Int(42), Type::Int);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Int(42) => {}
            other => panic!("expected Int(42), got {:?}", other),
        }
    }

    #[test]
    fn value_ref_found() {
        let expr = vref("Bracket", "width", Type::length());
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("Bracket", "width"), mm_val(80.0));
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(!result.is_undef());
        let v = result.as_f64().unwrap();
        assert!((v - 0.08).abs() < 1e-12);
    }

    #[test]
    fn value_ref_missing_returns_undef() {
        let expr = vref("Bracket", "width", Type::length());
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn add_same_dimension() {
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(20.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::length());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        let v = result.as_f64().unwrap();
        assert!((v - 0.1).abs() < 1e-12);
    }

    #[test]
    fn add_different_dimensions_is_undef() {
        let length = lit(mm_val(80.0), Type::length());
        let mass = lit(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS,
            },
        );
        let expr = CompiledExpr::binop(BinOp::Add, length, mass, Type::length());
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn mul_dimensions_add_exponents() {
        let width = lit(mm_val(80.0), Type::length());
        let height = lit(mm_val(100.0), Type::length());
        let expr = CompiledExpr::binop(
            BinOp::Mul,
            width,
            height,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match &result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 0.008).abs() < 1e-12);
                assert_eq!(*dimension, DimensionVector::AREA);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn div_by_zero_is_undef() {
        let left = lit(Value::Int(42), Type::Int);
        let right = lit(Value::Int(0), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::Int);
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn gt_comparison() {
        let left = lit(mm_val(5.0), Type::length());
        let right = lit(mm_val(2.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn undef_propagation_arithmetic() {
        let left = lit(Value::Undef, Type::length());
        let right = lit(mm_val(2.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::length());
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn kleene_and_false_undef() {
        // false AND Undef = false
        let left = lit(Value::Bool(false), Type::Bool);
        let right = lit(Value::Undef, Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn kleene_and_undef_false() {
        // Undef AND false = false
        let left = lit(Value::Undef, Type::Bool);
        let right = lit(Value::Bool(false), Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn kleene_or_true_undef() {
        // true OR Undef = true
        let left = lit(Value::Bool(true), Type::Bool);
        let right = lit(Value::Undef, Type::Bool);
        let expr = CompiledExpr::binop(BinOp::Or, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn kleene_and_undef_undef() {
        // Undef AND true = Undef
        let left = lit(Value::Undef, Type::Bool);
        let right = lit(Value::Bool(true), Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn negation() {
        let operand = lit(mm_val(5.0), Type::length());
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::length());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        let v = result.as_f64().unwrap();
        assert!((v - (-0.005)).abs() < 1e-12);
    }

    #[test]
    fn not_bool() {
        let operand = lit(Value::Bool(true), Type::Bool);
        let expr = CompiledExpr::unop(UnOp::Not, operand, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn conditional_true() {
        let cond = lit(Value::Bool(true), Type::Bool);
        let then_branch = lit(Value::Int(1), Type::Int);
        let else_branch = lit(Value::Int(2), Type::Int);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[99]),
            result_type: Type::Int,
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Int(1) => {}
            other => panic!("expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn conditional_undef_condition() {
        let cond = lit(Value::Undef, Type::Bool);
        let then_branch = lit(Value::Int(1), Type::Int);
        let else_branch = lit(Value::Int(2), Type::Int);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[99]),
            result_type: Type::Int,
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn scalar_pow_int() {
        // (3mm)^2 = 9mm² = 9e-6 m²
        let base = lit(mm_val(3.0), Type::length());
        let exp = lit(Value::Int(2), Type::Int);
        let expr = CompiledExpr::binop(
            BinOp::Pow,
            base,
            exp,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match &result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 9e-6).abs() < 1e-15);
                assert_eq!(*dimension, DimensionVector::AREA);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn volume_computation() {
        // width * height * thickness
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "width"), mm_val(80.0));
        values.insert(ValueCellId::new("B", "height"), mm_val(100.0));
        values.insert(ValueCellId::new("B", "thickness"), mm_val(5.0));

        let w = vref("B", "width", Type::length());
        let h = vref("B", "height", Type::length());
        let t = vref("B", "thickness", Type::length());

        let wh = CompiledExpr::binop(
            BinOp::Mul,
            w,
            h,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let volume = CompiledExpr::binop(
            BinOp::Mul,
            wh,
            t,
            Type::Scalar {
                dimension: DimensionVector::VOLUME,
            },
        );

        let result = eval_expr(&volume, &EvalContext::simple(&values));
        match &result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                // 0.08 * 0.1 * 0.005 = 4e-5 m³
                assert!((si_value - 4e-5).abs() < 1e-15);
                assert_eq!(*dimension, DimensionVector::VOLUME);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn function_call_abs_dispatches_to_stdlib() {
        // FunctionCall('abs', [Literal(Real(-3.0))]) should return Real(3.0), not Undef
        let arg = lit(Value::Real(-3.0), Type::Real);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[42]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: "abs".to_string(),
                    qualified_name: "std::abs".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn function_call_sin_with_angle() {
        let arg = lit(
            Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_4,
                dimension: DimensionVector::ANGLE,
            },
            Type::Scalar {
                dimension: DimensionVector::ANGLE,
            },
        );
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[43]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: "sin".to_string(),
                    qualified_name: "std::sin".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-10),
            other => panic!("expected Real(~0.7071), got {:?}", other),
        }
    }

    #[test]
    fn function_call_unknown_returns_undef() {
        let arg = lit(Value::Real(1.0), Type::Real);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[44]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: "nonexistent".to_string(),
                    qualified_name: "std::nonexistent".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn function_call_undef_propagation() {
        // abs(Undef) should return Undef (strict propagation)
        let arg = lit(Value::Undef, Type::Real);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[45]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: "abs".to_string(),
                    qualified_name: "std::abs".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn function_call_with_value_ref_args() {
        // abs(width) where width = -80mm
        let arg = vref("B", "width", Type::length());
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[46]),
            result_type: Type::length(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: "abs".to_string(),
                    qualified_name: "std::abs".to_string(),
                },
                args: vec![arg],
            },
        };
        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("B", "width"),
            Value::Scalar {
                si_value: -0.08,
                dimension: DimensionVector::LENGTH,
            },
        );
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 0.08).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn function_call_zero_args_returns_undef() {
        // abs() with no args should return Undef
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[47]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: "abs".to_string(),
                    qualified_name: "std::abs".to_string(),
                },
                args: vec![],
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn eq_scalar_different_dimensions_is_false() {
        // 0.005 LENGTH == 0.005 MASS should be false (different dimensions)
        let left = lit(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let right = lit(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::MASS,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS,
            },
        );
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn eq_scalar_same_dimension_same_value_is_true() {
        // Two LENGTH scalars with identical si_value should be equal
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(80.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn eq_scalar_same_dimension_different_value_is_false() {
        // Two LENGTH scalars with different si_values should not be equal
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(100.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn cmp_scalar_different_dimensions_is_undef() {
        // 0.005 LENGTH < 0.005 MASS should be Undef (incomparable dimensions)
        let left = lit(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let right = lit(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::MASS,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS,
            },
        );
        let expr = CompiledExpr::binop(BinOp::Lt, left, right, Type::Bool);
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn cmp_scalar_same_dimension_works() {
        // 3mm < 5mm should be true
        let left = lit(mm_val(3.0), Type::length());
        let right = lit(mm_val(5.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Lt, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn eq_dimensioned_scalar_vs_int_is_false() {
        // Scalar{5.0, LENGTH} == Int(5) should be false
        let left = lit(
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let right = lit(Value::Int(5), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn ne_scalar_different_dimensions_is_true() {
        // 0.005 LENGTH != 0.005 MASS should be true
        let left = lit(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let right = lit(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::MASS,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS,
            },
        );
        let expr = CompiledExpr::binop(BinOp::Ne, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn eq_scalar_same_value_different_dimension_is_false() {
        // Scalar{5.0, LENGTH} == Scalar{5.0, MASS} should be false
        // Regression guard for task 38: different dimensions must never compare equal,
        // even when the numeric si_value is identical (5mm == 5kg was silently true).
        let left = lit(
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let right = lit(
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::MASS,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS,
            },
        );
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    // ── Enum eval tests ──────────────────────────────────

    fn enum_lit(type_name: &str, variant: &str) -> CompiledExpr {
        lit(
            Value::Enum {
                type_name: type_name.into(),
                variant: variant.into(),
            },
            Type::Enum(type_name.into()),
        )
    }

    #[test]
    fn eval_eq_enum_same_type_same_variant() {
        let left = enum_lit("Direction", "In");
        let right = enum_lit("Direction", "In");
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn eval_eq_enum_same_type_different_variant() {
        let left = enum_lit("Direction", "In");
        let right = enum_lit("Direction", "Out");
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn eval_eq_enum_different_types() {
        let left = enum_lit("Direction", "In");
        let right = enum_lit("ThreadSystem", "ISO");
        let expr = CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn eval_ne_enum_variants() {
        let left = enum_lit("Direction", "In");
        let right = enum_lit("Direction", "Out");
        let expr = CompiledExpr::binop(BinOp::Ne, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn eval_cmp_enum_returns_undef() {
        let left = enum_lit("Direction", "In");
        let right = enum_lit("Direction", "Out");
        let expr = CompiledExpr::binop(BinOp::Lt, left, right, Type::Bool);
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "comparison on enums should return Undef"
        );
    }

    // ── Match eval tests ──────────────────────────────────

    #[test]
    fn eval_match_basic() {
        // match Direction.In { [In] => 1, [Out] => 2, [Bidi] => 3 }
        let discriminant = enum_lit("Direction", "In");
        let arms = vec![
            reify_types::CompiledMatchArm {
                patterns: vec!["In".to_string()],
                body: lit(Value::Int(1), Type::Int),
            },
            reify_types::CompiledMatchArm {
                patterns: vec!["Out".to_string()],
                body: lit(Value::Int(2), Type::Int),
            },
            reify_types::CompiledMatchArm {
                patterns: vec!["Bidi".to_string()],
                body: lit(Value::Int(3), Type::Int),
            },
        ];
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[100]),
            result_type: Type::Int,
            kind: CompiledExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
        };
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Int(1) => {}
            other => panic!("expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn eval_match_undef_discriminant() {
        let discriminant = lit(Value::Undef, Type::Int);
        let arms = vec![reify_types::CompiledMatchArm {
            patterns: vec!["In".to_string()],
            body: lit(Value::Int(1), Type::Int),
        }];
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[101]),
            result_type: Type::Int,
            kind: CompiledExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    #[test]
    fn eval_match_wildcard() {
        // match Direction.Bidi { [In] => 1, [_] => 99 }
        let discriminant = enum_lit("Direction", "Bidi");
        let arms = vec![
            reify_types::CompiledMatchArm {
                patterns: vec!["In".to_string()],
                body: lit(Value::Int(1), Type::Int),
            },
            reify_types::CompiledMatchArm {
                patterns: vec!["_".to_string()],
                body: lit(Value::Int(99), Type::Int),
            },
        ];
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[102]),
            result_type: Type::Int,
            kind: CompiledExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
        };
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Int(99) => {}
            other => panic!("expected Int(99), got {:?}", other),
        }
    }

    #[test]
    fn eval_match_multi_variant_pattern() {
        // match Control.Button { [Socket, Button] => "recessed", [Slider] => "raised" }
        let discriminant = enum_lit("Control", "Button");
        let arms = vec![
            reify_types::CompiledMatchArm {
                patterns: vec!["Socket".to_string(), "Button".to_string()],
                body: lit(Value::String("recessed".to_string()), Type::String),
            },
            reify_types::CompiledMatchArm {
                patterns: vec!["Slider".to_string()],
                body: lit(Value::String("raised".to_string()), Type::String),
            },
        ];
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[103]),
            result_type: Type::String,
            kind: CompiledExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
        };
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::String(s) => assert_eq!(s, "recessed"),
            other => panic!("expected String(\"recessed\"), got {:?}", other),
        }
    }

    #[test]
    fn div_same_dimension_yields_dimensionless() {
        // 80mm / 20mm = 4.0 (dimensionless Scalar, consistent with eval_mul)
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(20.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match &result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 4.0).abs() < 1e-12);
                assert!(dimension.is_dimensionless());
            }
            other => panic!("expected Scalar{{dimensionless}}, got {:?}", other),
        }
    }

    // ── User function evaluation tests ──────────────────────────────────

    use reify_types::{CompiledFnBody, CompiledFunction, ContentHash};

    fn make_double_fn() -> CompiledFunction {
        // fn double(x: Real) -> Real { x + x }
        CompiledFunction {
            name: "double".to_string(),
            is_pub: false,
            params: vec![("x".to_string(), Type::Real)],
            return_type: Type::Real,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Add,
                    vref("double", "x", Type::Real),
                    vref("double", "x", Type::Real),
                    Type::Real,
                ),
            },
            content_hash: ContentHash::of(b"double"),
        }
    }

    fn make_fn_with_let() -> CompiledFunction {
        // fn f(x: Real) -> Real { let y = x + 1; y * 2 }
        CompiledFunction {
            name: "f".to_string(),
            is_pub: false,
            params: vec![("x".to_string(), Type::Real)],
            return_type: Type::Real,
            body: CompiledFnBody {
                let_bindings: vec![(
                    "y".to_string(),
                    CompiledExpr::binop(
                        BinOp::Add,
                        vref("f", "x", Type::Real),
                        lit(Value::Int(1), Type::Int),
                        Type::Real,
                    ),
                )],
                result_expr: CompiledExpr::binop(
                    BinOp::Mul,
                    vref("f", "y", Type::Real),
                    lit(Value::Int(2), Type::Int),
                    Type::Real,
                ),
            },
            content_hash: ContentHash::of(b"f_with_let"),
        }
    }

    #[test]
    fn eval_user_fn_double() {
        let double_fn = make_double_fn();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_double"),
            result_type: Type::Real,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "double".to_string(),
                args: vec![lit(Value::Real(5.0), Type::Real)],
            },
        };
        let values = ValueMap::new();
        let functions = [double_fn];
        let ctx = EvalContext::new(&values, &functions);
        let result = eval_expr(&call_expr, &ctx);
        match result {
            Value::Real(v) => assert!((v - 10.0).abs() < 1e-12, "expected 10.0, got {}", v),
            other => panic!("expected Real(10.0), got {:?}", other),
        }
    }

    fn make_factorial_fn() -> CompiledFunction {
        // fn factorial(n: Int) -> Int {
        //   if n <= 1 then 1 else n * factorial(n - 1)
        // }
        CompiledFunction {
            name: "factorial".to_string(),
            is_pub: false,
            params: vec![("n".to_string(), Type::Int)],
            return_type: Type::Int,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr {
                    content_hash: ContentHash::of(b"factorial_body"),
                    result_type: Type::Int,
                    kind: CompiledExprKind::Conditional {
                        condition: Box::new(CompiledExpr::binop(
                            BinOp::Le,
                            vref("factorial", "n", Type::Int),
                            lit(Value::Int(1), Type::Int),
                            Type::Bool,
                        )),
                        then_branch: Box::new(lit(Value::Int(1), Type::Int)),
                        else_branch: Box::new(CompiledExpr::binop(
                            BinOp::Mul,
                            vref("factorial", "n", Type::Int),
                            CompiledExpr {
                                content_hash: ContentHash::of(b"recursive_call"),
                                result_type: Type::Int,
                                kind: CompiledExprKind::UserFunctionCall {
                                    function_name: "factorial".to_string(),
                                    args: vec![CompiledExpr::binop(
                                        BinOp::Sub,
                                        vref("factorial", "n", Type::Int),
                                        lit(Value::Int(1), Type::Int),
                                        Type::Int,
                                    )],
                                },
                            },
                            Type::Int,
                        )),
                    },
                },
            },
            content_hash: ContentHash::of(b"factorial"),
        }
    }

    fn make_infinite_fn() -> CompiledFunction {
        // fn infinite(x: Int) -> Int { infinite(x) }
        CompiledFunction {
            name: "infinite".to_string(),
            is_pub: false,
            params: vec![("x".to_string(), Type::Int)],
            return_type: Type::Int,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr {
                    content_hash: ContentHash::of(b"infinite_body"),
                    result_type: Type::Int,
                    kind: CompiledExprKind::UserFunctionCall {
                        function_name: "infinite".to_string(),
                        args: vec![vref("infinite", "x", Type::Int)],
                    },
                },
            },
            content_hash: ContentHash::of(b"infinite"),
        }
    }

    #[test]
    fn eval_user_fn_with_let_bindings() {
        // fn f(x: Real) -> Real { let y = x + 1; y * 2 }
        // f(4) => y = 4 + 1 = 5; result = 5 * 2 = 10
        let f = make_fn_with_let();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_f"),
            result_type: Type::Real,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "f".to_string(),
                args: vec![lit(Value::Real(4.0), Type::Real)],
            },
        };
        let values = ValueMap::new();
        let functions = [f];
        let ctx = EvalContext::new(&values, &functions);
        let result = eval_expr(&call_expr, &ctx);
        match result {
            Value::Real(v) => assert!((v - 10.0).abs() < 1e-12, "expected 10.0, got {}", v),
            other => panic!("expected Real(10.0), got {:?}", other),
        }
    }

    #[test]
    fn eval_user_fn_recursive_factorial() {
        // factorial(5) = 5 * 4 * 3 * 2 * 1 = 120
        let factorial_fn = make_factorial_fn();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_factorial"),
            result_type: Type::Int,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "factorial".to_string(),
                args: vec![lit(Value::Int(5), Type::Int)],
            },
        };
        let values = ValueMap::new();
        let functions = [factorial_fn];
        let ctx = EvalContext::new(&values, &functions);
        let result = eval_expr(&call_expr, &ctx);
        match result {
            Value::Int(120) => {}
            other => panic!("expected Int(120), got {:?}", other),
        }
    }

    #[test]
    fn eval_user_fn_recursion_depth_exceeded() {
        // infinite(1) should return Undef (hit depth limit), not stack-overflow
        let infinite_fn = make_infinite_fn();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_infinite"),
            result_type: Type::Int,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "infinite".to_string(),
                args: vec![lit(Value::Int(1), Type::Int)],
            },
        };
        let values = ValueMap::new();
        let functions = [infinite_fn];
        let ctx = EvalContext::new(&values, &functions);
        let result = eval_expr(&call_expr, &ctx);
        assert!(
            result.is_undef(),
            "expected Undef for infinite recursion, got {:?}",
            result
        );
    }

    #[test]
    fn eval_user_fn_undef_arg_propagation() {
        // double(Undef) should return Undef
        let double_fn = make_double_fn();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_double_undef"),
            result_type: Type::Real,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "double".to_string(),
                args: vec![lit(Value::Undef, Type::Real)],
            },
        };
        let values = ValueMap::new();
        let functions = [double_fn];
        let ctx = EvalContext::new(&values, &functions);
        let result = eval_expr(&call_expr, &ctx);
        assert!(
            result.is_undef(),
            "expected Undef for undef arg, got {:?}",
            result
        );
    }

    #[test]
    fn eval_user_fn_dimension_args() {
        // fn area(w: Length, h: Length) -> Area { w * h }
        let area_fn = CompiledFunction {
            name: "area".to_string(),
            is_pub: false,
            params: vec![
                ("w".to_string(), Type::length()),
                ("h".to_string(), Type::length()),
            ],
            return_type: Type::Scalar {
                dimension: DimensionVector::AREA,
            },
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Mul,
                    vref("area", "w", Type::length()),
                    vref("area", "h", Type::length()),
                    Type::Scalar {
                        dimension: DimensionVector::AREA,
                    },
                ),
            },
            content_hash: ContentHash::of(b"area"),
        };
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_area"),
            result_type: Type::Scalar {
                dimension: DimensionVector::AREA,
            },
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "area".to_string(),
                args: vec![
                    lit(
                        Value::Scalar {
                            si_value: 0.08,
                            dimension: DimensionVector::LENGTH,
                        },
                        Type::length(),
                    ),
                    lit(
                        Value::Scalar {
                            si_value: 0.1,
                            dimension: DimensionVector::LENGTH,
                        },
                        Type::length(),
                    ),
                ],
            },
        };
        let values = ValueMap::new();
        let functions = [area_fn];
        let ctx = EvalContext::new(&values, &functions);
        let result = eval_expr(&call_expr, &ctx);
        match &result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 0.008).abs() < 1e-12,
                    "expected 0.008, got {}",
                    si_value
                );
                assert_eq!(*dimension, DimensionVector::AREA);
            }
            other => panic!("expected Scalar AREA, got {:?}", other),
        }
    }

    #[test]
    fn eval_user_fn_overload_by_arity() {
        // fn process(x: Real) -> Real { x * 2 }
        let process1 = CompiledFunction {
            name: "process".to_string(),
            is_pub: false,
            params: vec![("x".to_string(), Type::Real)],
            return_type: Type::Real,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Mul,
                    vref("process", "x", Type::Real),
                    lit(Value::Int(2), Type::Int),
                    Type::Real,
                ),
            },
            content_hash: ContentHash::of(b"process1"),
        };
        // fn process(x: Real, y: Real) -> Real { x + y }
        let process2 = CompiledFunction {
            name: "process".to_string(),
            is_pub: false,
            params: vec![("x".to_string(), Type::Real), ("y".to_string(), Type::Real)],
            return_type: Type::Real,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Add,
                    vref("process", "x", Type::Real),
                    vref("process", "y", Type::Real),
                    Type::Real,
                ),
            },
            content_hash: ContentHash::of(b"process2"),
        };

        let functions = [process1, process2];
        let values = ValueMap::new();

        // Call with 1 arg: process(3.0) → 6.0
        let call1 = CompiledExpr {
            content_hash: ContentHash::of(b"call_process1"),
            result_type: Type::Real,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "process".to_string(),
                args: vec![lit(Value::Real(3.0), Type::Real)],
            },
        };
        let ctx = EvalContext::new(&values, &functions);
        match eval_expr(&call1, &ctx) {
            Value::Real(v) => assert!((v - 6.0).abs() < 1e-12, "expected 6.0, got {}", v),
            other => panic!("expected Real(6.0), got {:?}", other),
        }

        // Call with 2 args: process(3.0, 4.0) → 7.0
        let call2 = CompiledExpr {
            content_hash: ContentHash::of(b"call_process2"),
            result_type: Type::Real,
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "process".to_string(),
                args: vec![
                    lit(Value::Real(3.0), Type::Real),
                    lit(Value::Real(4.0), Type::Real),
                ],
            },
        };
        match eval_expr(&call2, &ctx) {
            Value::Real(v) => assert!((v - 7.0).abs() < 1e-12, "expected 7.0, got {}", v),
            other => panic!("expected Real(7.0), got {:?}", other),
        }
    }

    // ── Match non-enum discriminant ──────────────────────────────

    #[test]
    fn match_non_enum_discriminant_returns_undef() {
        // match Int(42) { [In] => 1, [Out] => 2 } → Undef
        let discriminant = lit(Value::Int(42), Type::Int);
        let arms = vec![
            CompiledMatchArm {
                patterns: vec!["In".to_string()],
                body: lit(Value::Int(1), Type::Int),
            },
            CompiledMatchArm {
                patterns: vec!["Out".to_string()],
                body: lit(Value::Int(2), Type::Int),
            },
        ];
        let expr = CompiledExpr {
            content_hash: ContentHash::of(&[200]),
            result_type: Type::Int,
            kind: CompiledExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
        };
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "matching on non-enum value should return Undef"
        );
    }

    #[test]
    fn neg_int_min_returns_undef() {
        let operand = lit(Value::Int(i64::MIN), Type::Int);
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::Int);
        let values = ValueMap::new();
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&values)),
            Value::Undef,
            "negating i64::MIN should return Undef, not panic"
        );
    }

    // ── unop: Neg on Complex (NaN/Inf pre-guard) ─────────────────────────────

    #[test]
    fn neg_complex_nan_re_returns_undef() {
        // Complex{re: NaN, im: 1.0, DIMENSIONLESS} via UnOp::Neg → Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating Complex with NaN re should return Undef"
        );
    }

    #[test]
    fn neg_complex_nan_im_returns_undef() {
        // Complex{re: 1.0, im: NaN, DIMENSIONLESS} via UnOp::Neg → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating Complex with NaN im should return Undef"
        );
    }

    #[test]
    fn neg_complex_inf_re_returns_undef() {
        // Complex{re: +Inf, im: 1.0, DIMENSIONLESS} via UnOp::Neg → Undef
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating Complex with +Inf re should return Undef"
        );
    }

    #[test]
    fn neg_complex_neg_inf_re_returns_undef() {
        // Complex{re: -Inf, im: 1.0, DIMENSIONLESS} via UnOp::Neg → Undef
        let complex_val = Value::Complex {
            re: f64::NEG_INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating Complex with -Inf re should return Undef"
        );
    }

    #[test]
    fn neg_complex_inf_im_returns_undef() {
        // Complex{re: 1.0, im: +Inf, DIMENSIONLESS} via UnOp::Neg → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating Complex with +Inf im should return Undef"
        );
    }

    #[test]
    fn neg_complex_neg_inf_im_returns_undef() {
        // Complex{re: 1.0, im: -Inf, DIMENSIONLESS} via UnOp::Neg → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating Complex with -Inf im should return Undef"
        );
    }

    #[test]
    fn neg_complex_nan_dimensioned_returns_undef() {
        // Complex{re: NaN, im: 1.0, LENGTH} via UnOp::Neg → Undef (dimensioned path)
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let operand = lit(complex_val, Type::complex(Type::length()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::length()));
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "negating dimensioned Complex with NaN re should return Undef"
        );
    }

    #[test]
    fn neg_complex_finite_dimensionless_correct() {
        // Complex{re: 2.0, im: -3.0, DIMENSIONLESS} via UnOp::Neg → Complex{re: -2.0, im: 3.0, DIMENSIONLESS}
        let complex_val = Value::Complex {
            re: 2.0,
            im: -3.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let operand = lit(complex_val, Type::complex(Type::Real));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::Real));
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Complex { re, im, dimension } => {
                assert!((re - (-2.0)).abs() < 1e-12, "re should be -2.0, got {}", re);
                assert!((im - 3.0).abs() < 1e-12, "im should be 3.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS, "dimension should be DIMENSIONLESS");
            }
            other => panic!("expected Complex, got {:?}", other),
        }
    }

    #[test]
    fn neg_complex_finite_dimensioned_correct() {
        // Complex{re: 2.0, im: -3.0, LENGTH} via UnOp::Neg → Complex{re: -2.0, im: 3.0, LENGTH}
        let complex_val = Value::Complex {
            re: 2.0,
            im: -3.0,
            dimension: DimensionVector::LENGTH,
        };
        let operand = lit(complex_val, Type::complex(Type::length()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::length()));
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Complex { re, im, dimension } => {
                assert!((re - (-2.0)).abs() < 1e-12, "re should be -2.0, got {}", re);
                assert!((im - 3.0).abs() < 1e-12, "im should be 3.0, got {}", im);
                assert_eq!(dimension, DimensionVector::LENGTH, "dimension should be LENGTH");
            }
            other => panic!("expected Complex, got {:?}", other),
        }
    }

    // ── method: re ────────────────────────────────────────────────────────────

    #[test]
    fn re_nan_dimensionless_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.re → Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "re".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.re with NaN real part should return Undef"
        );
    }

    #[test]
    fn re_inf_dimensionless_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.re → Undef
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "re".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.re with Inf real part should return Undef"
        );
    }

    #[test]
    fn re_nan_dimensioned_returns_undef() {
        // Complex{re:NaN, im:1.0, LENGTH}.re → Undef (dimensioned Scalar path)
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "re".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.re with NaN real part (dimensioned) should return Undef"
        );
    }

    // ── method: im ────────────────────────────────────────────────────────────

    #[test]
    fn im_nan_dimensionless_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.im → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "im".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.im with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn im_inf_dimensionless_returns_undef() {
        // Complex{re:1.0, im:+Inf, DIMENSIONLESS}.im → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "im".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.im with Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn im_nan_dimensioned_returns_undef() {
        // Complex{re:1.0, im:NaN, LENGTH}.im → Undef (dimensioned Scalar path)
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "im".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.im with NaN imaginary part (dimensioned) should return Undef"
        );
    }

    // ── method: magnitude ─────────────────────────────────────────────────────

    #[test]
    fn magnitude_nan_dimensionless_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.magnitude → Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude with NaN should return Undef"
        );
    }

    #[test]
    fn magnitude_overflow_dimensionless_returns_undef() {
        // Complex{re:f64::MAX, im:f64::MAX, DIMENSIONLESS}.magnitude → Undef (overflow to +Inf)
        let complex_val = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude overflowing to +Inf should return Undef"
        );
    }

    #[test]
    fn magnitude_nan_dimensioned_returns_undef() {
        // Complex{re:NaN, im:1.0, LENGTH}.magnitude → Undef (dimensioned path)
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "magnitude".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude with NaN (dimensioned) should return Undef"
        );
    }

    #[test]
    fn magnitude_inf_dimensionless_returns_undef() {
        // Complex{re:+Inf, im:0.0, DIMENSIONLESS}.magnitude → Undef (direct Inf input)
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude with +Inf input should return Undef"
        );
    }

    // ── method regressions: finite values still work ──────────────────────────

    #[test]
    fn magnitude_finite_dimensionless_correct() {
        // Complex{re:3.0, im:4.0, DIMENSIONLESS}.magnitude == Real(5.0)
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12, "expected 5.0, got {}", v),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn re_finite_dimensionless_correct() {
        // Complex{re:3.0, im:4.0, DIMENSIONLESS}.re == Real(3.0)
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "re".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12, "expected 3.0, got {}", v),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn im_finite_dimensionless_correct() {
        // Complex{re:3.0, im:4.0, DIMENSIONLESS}.im == Real(4.0)
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "im".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 4.0).abs() < 1e-12, "expected 4.0, got {}", v),
            other => panic!("expected Real(4.0), got {:?}", other),
        }
    }

    #[test]
    fn re_finite_dimensioned_correct() {
        // Complex{re:0.003, im:0.004, LENGTH}.re == Scalar{0.003, LENGTH}
        let complex_val = Value::Complex {
            re: 0.003,
            im: 0.004,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "re".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 0.003).abs() < 1e-12, "expected 0.003, got {}", si_value);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{0.003, LENGTH}}, got {:?}", other),
        }
    }

    // ── method: phase (NaN/Inf sanitization) ─────────────────────────────────

    #[test]
    fn phase_nan_re_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.phase → Undef
        // atan2(1.0, NaN) = NaN; phase should return Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with NaN real part should return Undef"
        );
    }

    #[test]
    fn phase_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.phase → Undef
        // atan2(NaN, 1.0) = NaN; phase should return Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_inf_re_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.phase → Undef
        // Note: atan2(1.0, +Inf) = 0.0 which is finite — sanitize_value alone
        // would NOT catch this Inf input. The pre-guard is what correctly rejects it.
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with +Inf real part should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_im_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.phase → Undef
        // The Complex carries an Inf component, violating sanitization convention
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with -Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_re_returns_undef() {
        // Complex{re:-Inf, im:1.0, DIMENSIONLESS}.phase → Undef
        //
        // Note: atan2(1.0, -Inf) = π, which is finite — so sanitize_value alone
        // would NOT catch this -Inf input and would silently return a wrong result.
        // The pre-guard (!re.is_finite() || !im.is_finite()) is what correctly
        // rejects this case. This test locks that behaviour as a regression guard.
        let complex_val = Value::Complex {
            re: f64::NEG_INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with -Inf real part should return Undef (atan2(1.0,-Inf)=π is finite, \
             so the pre-guard, not sanitize_value, is what catches this)"
        );
    }

    // ── method regressions: finite phase values still work ────────────────────

    #[test]
    fn phase_finite_45_degrees_correct() {
        // Complex{re:1.0, im:1.0, DIMENSIONLESS}.phase == Scalar{π/4, ANGLE}
        // atan2(1.0, 1.0) = π/4 ≈ 0.7853981633974483
        let complex_val = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Scalar { si_value, dimension } => {
                let expected = std::f64::consts::FRAC_PI_4;
                assert!(
                    (si_value - expected).abs() < 1e-12,
                    "expected π/4 ≈ {}, got {}",
                    expected,
                    si_value
                );
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Scalar{{π/4, ANGLE}}, got {:?}", other),
        }
    }

    #[test]
    fn phase_finite_180_degrees_correct() {
        // Complex{re:-1.0, im:0.0, DIMENSIONLESS}.phase == Scalar{π, ANGLE}
        // atan2(0.0, -1.0) = π
        let complex_val = Value::Complex {
            re: -1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Scalar { si_value, dimension } => {
                let expected = std::f64::consts::PI;
                assert!(
                    (si_value - expected).abs() < 1e-12,
                    "expected π ≈ {}, got {}",
                    expected,
                    si_value
                );
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Scalar{{π, ANGLE}}, got {:?}", other),
        }
    }

    // ── sanitize_value direct unit tests ─────────────────────────────────────

    #[test]
    fn sanitize_real_nan_returns_undef() {
        assert!(
            sanitize_value(Value::Real(f64::NAN)).is_undef(),
            "Real(NaN) should become Undef"
        );
    }

    #[test]
    fn sanitize_real_inf_returns_undef() {
        assert!(
            sanitize_value(Value::Real(f64::INFINITY)).is_undef(),
            "Real(+Inf) should become Undef"
        );
    }

    #[test]
    fn sanitize_real_neg_inf_returns_undef() {
        assert!(
            sanitize_value(Value::Real(f64::NEG_INFINITY)).is_undef(),
            "Real(-Inf) should become Undef"
        );
    }

    #[test]
    fn sanitize_real_finite_passthrough() {
        let v = Value::Real(2.72);
        match sanitize_value(v) {
            Value::Real(x) => assert!((x - 2.72).abs() < 1e-12),
            other => panic!("expected Real(2.72), got {:?}", other),
        }
    }

    #[test]
    fn sanitize_scalar_nan_returns_undef() {
        let v = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Scalar with NaN si_value should become Undef"
        );
    }

    #[test]
    fn sanitize_scalar_inf_returns_undef() {
        let v = Value::Scalar {
            si_value: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Scalar with +Inf si_value should become Undef"
        );
    }

    #[test]
    fn sanitize_scalar_neg_inf_returns_undef() {
        let v = Value::Scalar {
            si_value: f64::NEG_INFINITY,
            dimension: DimensionVector::MASS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Scalar with -Inf si_value should become Undef"
        );
    }

    #[test]
    fn sanitize_scalar_finite_passthrough() {
        let v = Value::Scalar {
            si_value: 0.001,
            dimension: DimensionVector::LENGTH,
        };
        match sanitize_value(v) {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 0.001).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{0.001, LENGTH}}, got {:?}", other),
        }
    }

    // ── sanitize_value Orientation arm tests (task-914) ──────────────────────

    #[test]
    fn sanitize_orientation_nan_returns_undef() {
        let v = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN w should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::INFINITY,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with +Inf x should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_neg_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: f64::NEG_INFINITY,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with -Inf z should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_nan_y_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: f64::NAN,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN y should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_valid_passthrough() {
        let v = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        match sanitize_value(v) {
            Value::Orientation { w, x, y, z } => {
                assert!((w - 1.0).abs() < f64::EPSILON);
                assert!((x - 0.0).abs() < f64::EPSILON);
                assert!((y - 0.0).abs() < f64::EPSILON);
                assert!((z - 0.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Orientation{{1,0,0,0}}, got {:?}", other),
        }
    }
}
