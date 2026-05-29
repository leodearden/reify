// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

mod analysis;
mod calculus;
mod complex;
mod field_reductions;
pub mod interp;
pub mod kleene;
pub mod sampled;
mod sanitize;

use std::cell::RefCell;
use std::collections::HashMap;

use reify_ast::QuantifierKind;
use reify_core::{Diagnostic, DimensionVector, FIELD_ENTITY_PREFIX, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, DeterminacyPredicateKind, DeterminacyState, FieldSourceKind, PersistentMap, SelectorKind, UnOp, Value, ValueMap, quaternion_is_finite};

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
    /// Optional sink for runtime diagnostics emitted during evaluation
    /// (e.g. `W_FIELD_OUT_OF_BOUNDS` from `sampled::sample_at_point`,
    /// `W_INTERPOLATION_DEFERRED` from `interp::resolve_method`).
    /// When `Some`, callers can push warnings into the `RefCell` and the
    /// surrounding `Engine::eval`/`edit_*` flow drains them into
    /// `EvalResult.diagnostics`. When `None`, runtime warnings are
    /// silently dropped — preserving the legacy `EvalContext::simple`
    /// semantics used by ad-hoc unit tests.
    pub diagnostics: Option<&'a RefCell<Vec<Diagnostic>>>,
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
            diagnostics: None,
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
            diagnostics: None,
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
            diagnostics: None,
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

    /// Attach a runtime diagnostics sink. Warnings emitted during
    /// `eval_expr` (e.g. `W_FIELD_OUT_OF_BOUNDS` from sampled-field OOB
    /// queries) are pushed into the `RefCell` for the caller to drain.
    pub fn with_runtime_diagnostics(mut self, sink: &'a RefCell<Vec<Diagnostic>>) -> Self {
        self.diagnostics = Some(sink);
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
            diagnostics: self.diagnostics,
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

        CompiledExprKind::CrossSubGeometryRef(_id) => {
            // This variant must be consumed by the bare-let drop site in
            // entity.rs (task-3508) before eval_expr is ever invoked. Reaching
            // this arm means a CrossSubGeometryRef escaped that drop site —
            // a routing violation, not a normal undef-propagation case.
            // `unreachable!()` fires identically in debug and release builds,
            // unlike the former `debug_assert!(false, ...) + get_or_undef` which
            // silently returned Undef in release and could mask downstream bugs.
            //
            // Why a nested CrossSubGeometryRef cannot reach eval_expr (task-3663):
            //
            // (a) `CrossSubGeometryRef` always carries `Type::Geometry`.
            //     `Type::Geometry` values are unrepresentable as value cells —
            //     entity.rs:1104 explicitly skips value-cell creation for them
            //     and the runtime invariant `assert_value_cell_types_representable`
            //     enforces this. A CrossSubGeometryRef nested inside a larger
            //     expression (e.g. a BinOp) would make the outer expression's type
            //     depend on `Type::Geometry`, which the type checker rejects with a
            //     diagnostic and replaces with a poison literal long before the
            //     compiled tree is handed to eval.
            //
            // (b) The sole producer is `try_resolve_cross_sub_geometry_value_ref`
            //     (reify-compiler/src/expr.rs:256), which fires only for bare
            //     `self.<sub>.<member>` in the MemberAccess branch — a terminal
            //     return site. The caller does `return e;` immediately, so the
            //     CrossSubGeometryRef is always the top-level kind of the compiled
            //     sub-expression, never a child of a larger operator node.
            //
            // (c) Therefore, a CrossSubGeometryRef can only appear as the top-level
            //     kind of a let binding's rhs. Entity.rs drops it there. If a future
            //     refactor ever violates premises (a) or (b), this `unreachable!()`
            //     immediately surfaces the regression in both debug and release builds.
            // A compiler-side test pinning premise (b) would live in
            // crates/reify-compiler (outside this task's scope); this comment
            // is the narrative invariant documentation until that test is added.
            unreachable!(
                "CrossSubGeometryRef should be consumed by entity.rs bare-let drop site (task-3508)"
            )
        }

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
                        codomain_type,
                    } = &evaluated_args[0]
                    {
                        match (lambda.as_ref(), source) {
                            (Value::Lambda { .. }, _) => {
                                apply_lambda_with_point_unpacking(lambda, &evaluated_args[1], ctx)
                            }
                            // Sampled-field dispatch (task 2341): runtime
                            // helper extracts query coords, detects OOB,
                            // and dispatches to interp::interpolate_Nd.
                            (Value::SampledField(sf), FieldSourceKind::Sampled) => {
                                sampled::sample_at_point(sf, &evaluated_args[1], codomain_type, ctx)
                            }
                            // Derived-field case: lambda slot contains the original field.
                            // Pass codomain_type (the derived field's already-divided codomain,
                            // stamped by compute_gradient / compute_divergence / etc.) instead
                            // of the inner field's codomain — eliminates redundant division
                            // inside the numerical compute functions.
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::Gradient,
                            ) => calculus::compute_numerical_gradient_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                domain_type,
                                codomain_type,
                                ctx,
                            ),
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::Divergence,
                            ) => calculus::compute_numerical_divergence_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                domain_type,
                                codomain_type,
                                ctx,
                            ),
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::Curl,
                            ) => calculus::compute_numerical_curl_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                domain_type,
                                codomain_type,
                                ctx,
                            ),
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::Laplacian,
                            ) => calculus::compute_numerical_laplacian_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                domain_type,
                                codomain_type,
                                ctx,
                            ),
                            // Analysis field wrappers: sample the inner field, then
                            // apply the analysis builtin pointwise.
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::VonMises,
                            ) => analysis::sample_von_mises_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                codomain_type,
                                ctx,
                            ),
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::PrincipalStresses,
                            ) => analysis::sample_principal_stresses_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                codomain_type,
                                ctx,
                            ),
                            (
                                Value::Field {
                                    lambda: inner_lambda,
                                    ..
                                },
                                FieldSourceKind::MaxShear,
                            ) => analysis::sample_max_shear_at_point(
                                inner_lambda,
                                &evaluated_args[1],
                                codomain_type,
                                ctx,
                            ),
                            // SafetyFactor: lambda slot is List[field, yield_val],
                            // not a nested Field — match on the source kind directly.
                            (_, FieldSourceKind::SafetyFactor) => {
                                analysis::sample_safety_factor_at_point(
                                    lambda,
                                    &evaluated_args[1],
                                    codomain_type,
                                    ctx,
                                )
                            }
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
                    calculus::compute_gradient(&evaluated_args[0])
                }
                "divergence" if evaluated_args.len() == 1 => {
                    calculus::compute_divergence(&evaluated_args[0])
                }
                "curl" if evaluated_args.len() == 1 => calculus::compute_curl(&evaluated_args[0]),
                "laplacian" if evaluated_args.len() == 1 => {
                    calculus::compute_laplacian(&evaluated_args[0])
                }
                // Analysis field wrappers: intercept when arg is a Field,
                // otherwise fall through to eval_builtin for concrete tensors.
                "von_mises"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    analysis::compute_von_mises(&evaluated_args[0])
                }
                "principal_stresses"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    analysis::compute_principal_stresses(&evaluated_args[0])
                }
                "max_shear"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    analysis::compute_max_shear(&evaluated_args[0])
                }
                "safety_factor"
                    if evaluated_args.len() == 2
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    analysis::compute_safety_factor(&evaluated_args[0], &evaluated_args[1])
                }
                // Field reductions (eager): collapse a Sampled field to a
                // single scalar (max/min) or a single point (argmax/argmin).
                //
                // # Dispatch gating
                //
                // The four arms below all use the gate
                // `args.len() == 1 && first arg is Value::Field`. This is
                // narrow on purpose:
                //
                // - `max(a, b)` / `min(a, b)` (2 scalar args) — falls through
                //   to `reify-stdlib::eval_builtin` → `numeric.rs:42` (`min`)
                //   / `numeric.rs:63` (`max`), which use `as_f64()` operands.
                // - `max(field, scalar)` (2 args, first is Field) — also
                //   falls through; the binary numeric form returns `Undef`
                //   because `Value::Field` has no `as_f64` mapping.
                // - `argmax(x)` / `argmin(x)` for non-Field args — fall
                //   through to `eval_builtin`, which has no binding for
                //   either name and returns `Value::Undef`.
                //
                // Pinned by `max_two_arg_scalar_form_unchanged` /
                // `min_two_arg_scalar_form_unchanged` (binary form
                // unchanged) and `argcount_gating_*_field_then_extra_arg_*`
                // (4 tests, step-19) in
                // `crates/reify-expr/tests/field_reductions_tests.rs`.
                "max"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    field_reductions::compute_max(&evaluated_args[0])
                }
                "min"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    field_reductions::compute_min(&evaluated_args[0])
                }
                "argmax"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    field_reductions::compute_argmax(&evaluated_args[0])
                }
                "argmin"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Field { .. }) =>
                {
                    field_reductions::compute_argmin(&evaluated_args[0])
                }
                // flat_map(list, lambda): apply `lambda` to each element of
                // `list`, expect each call to return a list, and concatenate
                // the per-element results into a single flat list. Intercepted
                // here (rather than in `reify_stdlib::eval_builtin`) because
                // applying the lambda requires `EvalContext` — the same reason
                // map/filter/fold are dispatched from `eval_method_call`.
                //
                // Convention: silent `Value::Undef` on type errors (non-list
                // input, non-lambda second arg, lambda result not a list,
                // wrong lambda arity). Matches the existing list-helper
                // convention; see task 2698 design decisions.
                "flat_map" if evaluated_args.len() == 2 => {
                    match (&evaluated_args[0], &evaluated_args[1]) {
                        (Value::List(items), lambda @ Value::Lambda { .. }) => {
                            let mut out: Vec<Value> = Vec::with_capacity(items.len());
                            for item in items {
                                let r = apply_lambda(lambda, std::slice::from_ref(item), ctx);
                                match r {
                                    Value::List(sub) => out.extend(sub),
                                    _ => return Value::Undef,
                                }
                            }
                            Value::List(out)
                        }
                        _ => Value::Undef,
                    }
                }
                // worst_case(mcr, lambda): dispatched here (not in
                // `reify_stdlib::eval_builtin` → `eval_fea`) because applying
                // the lambda requires `EvalContext`, mirroring `flat_map` /
                // `flat_map_pairs` above.
                //
                // Tie-break invariant: strict `>` ensures the first-seen
                // case wins on ties; combined with `BTreeMap` lexicographic
                // iteration over `Value::String` keys, this delivers
                // deterministic lex-min tie-break for free — no separate
                // sort. Mirrors the first-occurrence-wins discipline of
                // `envelope_reduce` (`crates/reify-stdlib/src/fea.rs`) and
                // `argmax_argmin_index` (`field_reductions.rs`). Pinned by
                // the `worst_case_tied_max_returns_lex_smaller_case_name`
                // smoke test.
                //
                // Body extracted into `eval_worst_case_dispatch` to keep
                // this recursive frame small in debug builds — the
                // per-iteration `String` and `Option<(String, f64)>` locals
                // would otherwise sit on every `eval_expr` frame and risk
                // overflowing the 2 MiB test-thread stack at
                // `MAX_RECURSION_DEPTH` (cf. the existing
                // `eval_user_fn_recursion_depth_exceeded` test and the
                // matching extraction of `eval_quantifier`). See
                // `eval_worst_case_dispatch` for the full dispatch contract
                // and silent-Undef discipline.
                "worst_case" if evaluated_args.len() == 2 => {
                    eval_worst_case_dispatch(&evaluated_args, ctx)
                }
                _ => {
                    // Composed-field call dispatch: a name in a composed lambda
                    // body (e.g. `base(p)` inside `composed { |p| base(p) * 30 }`)
                    // resolves to a captured `__field.<name>` cell. The compiler's
                    // `phase_augment_composed_captures` pass injects these cells
                    // into the lambda's `captures`, so by the time we reach this
                    // arm — inside `apply_lambda`, with captures already cloned
                    // into `ctx.values` — the field is in scope. We dispatch via
                    // `apply_lambda_with_point_unpacking` to mirror the `sample`
                    // path. Builtins are matched in earlier arms, so they are
                    // never shadowed; non-field names yield `Undef` from the
                    // cell lookup and fall through to `eval_builtin` unchanged.
                    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &function.name);
                    let candidate = ctx.values.get_or_undef(&field_id);
                    if let Value::Field { lambda, .. } = &candidate
                        && evaluated_args.len() == 1
                    {
                        return apply_lambda_with_point_unpacking(lambda, &evaluated_args[0], ctx);
                    }
                    let result = reify_stdlib::eval_builtin(&function.name, &evaluated_args);
                    // When a stackup builtin returns Undef, classify and emit
                    // the specific §4.4 error diagnostic into the ctx sink.
                    // Non-stackup builtins and valid stackup calls are untouched.
                    if matches!(result, Value::Undef)
                        && let Some(sink) = ctx.diagnostics
                        && let Some(diag) =
                            reify_stdlib::stackup_diagnose(&function.name, &evaluated_args)
                    {
                        sink.borrow_mut().push(diag);
                    }
                    result
                }
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

        // ReflectiveCellList: same runtime semantics as ListLiteral outside the
        // quantifier evaluator — the variant distinction only matters for
        // eval_quantifier's cell-iteration trigger (task-2458).
        CompiledExprKind::ReflectiveCellList(elements) => {
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

        CompiledExprKind::IndexAccess { object, index } => eval_index_access(object, index, ctx),

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
        } => eval_quantifier(*kind, variable_id, collection, predicate, ctx),

        // Ad-hoc selector evaluation: @point(x,y,z) is handled here in the
        // pure-expression evaluator (no kernel required). @face("name") and
        // @edge("name") require the geometry kernel and are patched by
        // Engine::post_process_ad_hoc_selectors after eval_expr completes.
        //
        // Body extracted into `eval_ad_hoc_selector` to keep this recursive
        // frame small in debug builds — the [f64; 3] coord buffer and Value
        // locals would otherwise sit on every `eval_expr` frame and risk
        // overflowing the 2 MiB test-thread stack at MAX_RECURSION_DEPTH
        // levels of recursive user-fn evaluation (cf. `eval_quantifier`).
        CompiledExprKind::AdHocSelector {
            selector_kind,
            args,
            ..
        } => eval_ad_hoc_selector(selector_kind, args, ctx),

        // Reflective-aggregation placeholder (task-2289). This variant is
        // emitted by the compiler for `subject.params` etc. and is expected
        // to be expanded by `Engine::activate_purpose` before any constraint
        // expression is evaluated. Reaching the evaluator with this variant
        // intact is a wiring bug — surface it via debug_assert; return Undef
        // in release builds to keep the eval anti-cascade-safe.
        CompiledExprKind::PurposeReflectiveAggregation { .. } => {
            debug_assert!(
                false,
                "PurposeReflectiveAggregation must be expanded by activate_purpose before evaluation"
            );
            Value::Undef
        }

        // task 3540 step-18 (SIR-α): build the structure instance with NO
        // registry lookup — `type_id`/`type_name`/`version` are baked at
        // lowering time (reify-expr stays registry-free,
        // design-decision-2; type_id is the StructureTypeId(0) placeholder,
        // identity is (name, version) per esc-3540-177 RULING 2+3).
        // Evaluate every supplied `ordered_arg` in declaration order, then
        // fill each `default` whose name is NOT covered by an ordered_arg,
        // evaluated in the SAME `EvalContext` — so a default that is itself
        // a structure ctor recurses through `eval_expr` and yields a nested
        // `Value::StructureInstance`. An `Undef` field value is kept in its
        // own slot: the structure is still constructed (no
        // `FunctionCall`-style strict whole-value short-circuit).
        CompiledExprKind::StructureInstanceCtor {
            type_id,
            type_name,
            version,
            ordered_args,
            defaults,
        } => eval_structure_instance_ctor(
            *type_id,
            type_name,
            *version,
            ordered_args,
            defaults,
            ctx,
        ),
    }
}

/// Build a `Value::StructureInstance` for a compile-lowered structure
/// constructor (task 3540 step-18; the eval half of design-decision-2).
///
/// Extracted from `eval_expr`'s match arm and marked `#[inline(never)]` on
/// purpose: the locals here (a `PersistentMap` plus loop temporaries) must
/// NOT inflate `eval_expr`'s stack frame. `eval_expr` is the hot mutually-
/// recursive evaluator, and `eval_user_fn_recursion_depth_exceeded` is a
/// safety test that drives `MAX_RECURSION_DEPTH` (256) deep and asserts the
/// guard trips *before* the native stack is exhausted. Inlining this body
/// into every recursive `eval_expr` frame regressed that test into a real
/// stack overflow; keeping it in a separate non-inlined frame (allocated
/// only when a ctor is actually evaluated, never on the deep user-fn path)
/// restores the lean frame the guard relies on.
///
/// No registry lookup — reify-expr stays registry-free (design-decision-2);
/// `type_id` is the `StructureTypeId(0)` placeholder, identity is
/// `(type_name, version)` per esc-3540-177 RULING 2+3. `ordered_args`
/// evaluate in declaration order; each `default` whose name is not covered
/// by an ordered arg fills its slot, evaluated in the SAME `EvalContext`
/// (so a default that is itself a structure ctor recurses through
/// `eval_expr` → nested `Value::StructureInstance`). An `Undef` field value
/// is kept in its own slot — the structure is still constructed (no
/// `FunctionCall`-style strict whole-value short-circuit).
#[inline(never)]
fn eval_structure_instance_ctor(
    type_id: reify_ir::StructureTypeId,
    type_name: &str,
    version: u32,
    ordered_args: &[(String, CompiledExpr)],
    defaults: &[(String, CompiledExpr)],
    ctx: &EvalContext,
) -> Value {
    let mut fields: PersistentMap<String, Value> = PersistentMap::new();
    for (name, arg) in ordered_args {
        fields.insert(name.clone(), eval_expr(arg, ctx));
    }
    for (name, def) in defaults {
        if !fields.contains_key(name) {
            fields.insert(name.clone(), eval_expr(def, ctx));
        }
    }
    Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
        type_id,
        type_name: type_name.to_string(),
        version,
        fields,
    }))
}

/// Evaluate `CompiledExprKind::IndexAccess`. Extracted from `eval_expr`'s
/// match arm and marked `#[inline(never)]` for the same reason
/// `eval_structure_instance_ctor` is hoisted out: the local `Value` slots
/// here (`obj`, `idx`) widen `eval_expr`'s debug-mode stack frame enough to
/// regress `eval_user_fn_recursion_depth_exceeded` (the safety test that
/// drives `MAX_RECURSION_DEPTH=256` and asserts the runtime guard trips
/// *before* the native stack is exhausted). Keeping this body in a separate
/// non-inlined frame — allocated only when an index access is actually
/// evaluated, not on every recursive `eval_expr` frame — restores the lean
/// frame the guard relies on. The `Value::StructureInstance` arm here is the
/// SIR-α (task 3540) field-projection path; lowering site is
/// `CompiledExpr::index_access` in `reify-compiler/src/expr.rs`.
#[inline(never)]
fn eval_index_access(object: &CompiledExpr, index: &CompiledExpr, ctx: &EvalContext) -> Value {
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
        (Value::StructureInstance(data), Value::String(k)) => {
            data.fields.get(k).cloned().unwrap_or(Value::Undef)
        }
        _ => Value::Undef,
    }
}

/// Evaluate an ad-hoc selector expression (`@point(x,y,z)`, `@face("n")`, `@edge("n")`).
///
/// Extracted from `eval_expr` to keep that recursive function's stack frame
/// small (the coord buffer and Value locals below would otherwise sit on every
/// `eval_expr` frame and risk overflowing the 2 MiB test-thread stack at
/// `MAX_RECURSION_DEPTH` levels of recursive user-fn evaluation — see the
/// `eval_user_fn_recursion_depth_exceeded` test and the matching extraction of
/// `eval_quantifier`).
///
/// - `SelectorKind::Point`: evaluates the 3 length-scalar args and builds a
///   `Value::Frame` with identity basis. Returns `Value::Undef` if any arg is
///   not a LENGTH-dimensioned scalar.
/// - `SelectorKind::Face | SelectorKind::Edge`: returns `Value::Undef` as a
///   placeholder for `Engine::post_process_ad_hoc_selectors` to overwrite.
fn eval_ad_hoc_selector(
    selector_kind: &SelectorKind,
    args: &[CompiledExpr],
    ctx: &EvalContext,
) -> Value {
    match selector_kind {
        SelectorKind::Point => {
            // @point(x, y, z): evaluate 3 coord args; if all are
            // LENGTH-dimensioned scalars, build Frame{origin, identity_basis}.
            if args.len() != 3 {
                return Value::Undef;
            }
            let mut si_coords = [0.0_f64; 3];
            for (i, arg) in args.iter().enumerate() {
                match eval_expr(arg, ctx) {
                    Value::Scalar { si_value, dimension }
                        if dimension == DimensionVector::LENGTH =>
                    {
                        si_coords[i] = si_value;
                    }
                    _ => return Value::Undef,
                }
            }
            let origin = Value::Point(
                si_coords
                    .iter()
                    .map(|&v| Value::Scalar {
                        si_value: v,
                        dimension: DimensionVector::LENGTH,
                    })
                    .collect(),
            );
            Value::Frame {
                origin: Box::new(origin),
                basis: Box::new(Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
            }
        }
        // @face("name") and @edge("name"): the engine post-process
        // (Engine::post_process_ad_hoc_selectors) patches these cells
        // via kernel.extract_faces/edges + resolve_unique_by_attribute.
        // Return Undef here as a placeholder for the engine to overwrite.
        SelectorKind::Face | SelectorKind::Edge => Value::Undef,
    }
}

/// Find the first compiled function matching `name`, arity, and per-parameter
/// [`Type`] equality against the compiled arguments' result types.
///
/// This is the canonical first-match-wins overload-resolution helper shared by:
/// - [`eval_user_function_call`] in this crate, and
/// - the `@optimized` `UserFunctionCall` → `ComputeNode` lowering site in
///   `reify-eval/src/engine_eval.rs`.
///
/// If the resolution rule ever grows (e.g. subtyping, coercion ranking,
/// operator-overloading nuance), update only this function; both call sites
/// will inherit the fix automatically.
pub fn find_matching_compiled_function<'a>(
    fns: &'a [CompiledFunction],
    name: &str,
    args: &[CompiledExpr],
) -> Option<&'a CompiledFunction> {
    fns.iter().find(|f| {
        f.name == name
            && f.params.len() == args.len()
            && f.params
                .iter()
                .zip(args.iter())
                .all(|((_, param_ty), arg)| *param_ty == arg.result_type)
    })
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
    let func = match find_matching_compiled_function(ctx.functions, function_name, args) {
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

/// Evaluate a Quantifier (`forall` / `exists`) expression.
///
/// Extracted from `eval_expr` to keep that recursive function's stack frame
/// small (the cell-iteration mode below needs to clone a `CompiledExpr` and a
/// `ValueMap` per iteration; in debug builds those locals would otherwise sit
/// on every `eval_expr` frame and blow the 2 MiB test-thread stack at
/// `MAX_RECURSION_DEPTH` levels of recursive user-fn evaluation — see the
/// `eval_user_fn_recursion_depth_exceeded` test).
///
/// Two iteration modes:
/// - **Cell-iteration mode (task-2289, narrowed in task-2458):** when
///   `collection.kind` is `ReflectiveCellList` and the list is non-empty —
///   the post-activation shape produced exclusively by
///   `expand_purpose_reflective_placeholders`. Trigger narrowed from
///   "any `ListLiteral` whose elements are all `ValueRef`s" to the dedicated
///   `ReflectiveCellList` variant (task-2458). User-written `ListLiteral`s of
///   pure `ValueRef`s now fall through to value-iteration, restoring
///   value-binding semantics for hand-authored quantifier collections.
///   Per iteration:
///     1. Clone the predicate.
///     2. Call `predicate_clone.remap_cell(variable_id, cell_id)` so any
///        `DeterminacyPredicate { cell: $loop_var }` is rewritten to point at
///        the iterated cell. `remap_cell` also rewrites nested
///        `Quantifier.variable_id`, `Lambda.captures/param_ids`, and
///        `ValueRef` ids that match.
///     3. Insert the cell's value into a per-iteration scope so
///        non-DeterminacyPredicate uses of the bound variable (e.g.
///        arithmetic) still see a value.
///     4. Evaluate the rewritten predicate.
/// - **Value-iteration fallback:** any other collection shape — evaluate the
///   collection, bind each element value under `variable_id`, evaluate the
///   predicate.
///
/// Both modes share Kleene short-circuit semantics: forall short-circuits on
/// `false`, exists short-circuits on `true`, `Undef` is tracked through the
/// loop and yields `Undef` if no short-circuit fired.
fn eval_quantifier(
    kind: QuantifierKind,
    variable_id: &ValueCellId,
    collection: &CompiledExpr,
    predicate: &CompiledExpr,
    ctx: &EvalContext,
) -> Value {
    // ── Cell-iteration mode (task-2289, trigger narrowed in task-2458) ──────
    // Fires only on `ReflectiveCellList` — the variant emitted exclusively by
    // `expand_purpose_reflective_placeholders`. User-written `ListLiteral`s of
    // `ValueRef`s now fall through to the value-iteration path below.
    if let CompiledExprKind::ReflectiveCellList(list_elements) = &collection.kind
        && !list_elements.is_empty()
    {
        let cell_ids: Vec<ValueCellId> = list_elements
            .iter()
            .map(|e| match &e.kind {
                CompiledExprKind::ValueRef(id) => id.clone(),
                _ => unreachable!("ReflectiveCellList elements must be ValueRef by construction"),
            })
            .collect();

        let mut has_undef = false;
        for cell_id in &cell_ids {
            let mut pred_clone = predicate.clone();
            pred_clone.remap_cell(variable_id, cell_id);

            // Defense-in-depth: bind the iterated cell's value under the
            // synthetic loop-var name. After `remap_cell` above, every
            // cell-bearing reference to `variable_id` (ValueRef,
            // DeterminacyPredicate.cell, nested Quantifier.variable_id,
            // Lambda.captures/param_ids) has been rewritten to `cell_id`,
            // so this insert is effectively dead for present node kinds.
            // We keep it so that any future expression variant that
            // references the loop variable by name without going through
            // `remap_cell` still sees a live binding instead of
            // `Value::Undef`. Cost is one map insert per iteration.
            let mut scope = ctx.values.clone();
            let cell_value = ctx.values.get_or_undef(cell_id);
            scope.insert(variable_id.clone(), cell_value);

            let pred_val = eval_expr(&pred_clone, &ctx.with_scope(&scope));
            match (kind, pred_val) {
                (QuantifierKind::ForAll, Value::Bool(false)) => return Value::Bool(false),
                (QuantifierKind::ForAll, Value::Bool(true)) => {}
                (QuantifierKind::Exists, Value::Bool(true)) => return Value::Bool(true),
                (QuantifierKind::Exists, Value::Bool(false)) => {}
                (_, Value::Undef) => has_undef = true,
                (_, _) => return Value::Undef, // type error
            }
        }
        return if has_undef {
            Value::Undef
        } else {
            match kind {
                QuantifierKind::ForAll => Value::Bool(true),
                QuantifierKind::Exists => Value::Bool(false),
            }
        };
    }

    // ── Value-iteration fallback ──────────────────────────────────────────
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

/// Dispatch `worst_case(mcr, lambda)` — apply `lambda` to each per-case
/// `ElasticResult` in `mcr.cases`, expect each call to return a `Field`,
/// collapse via `field_reductions::compute_max`, and return the case name
/// with the largest scalar.
///
/// Extracted from `eval_expr` to keep that recursive function's stack frame
/// small (the per-iteration `String` and running-best `Option<(String, f64)>`
/// locals would otherwise sit on every `eval_expr` frame and blow the 2 MiB
/// test-thread stack at `MAX_RECURSION_DEPTH` levels of recursive user-fn
/// evaluation — see the `eval_user_fn_recursion_depth_exceeded` test).
/// Mirrors the same extraction of `eval_quantifier`.
///
/// Intercepted in `eval_expr` (rather than in `reify_stdlib::eval_builtin`
/// → `eval_fea`) because applying the lambda requires `EvalContext` — the
/// same constraint that places `flat_map` in `eval_expr`. The `eval_fea`
/// arm for `"worst_case"` is a permanent `Value::Undef` stub that fires
/// when this dispatch declines (wrong arg shape), preserving the
/// "recognised name" contract.
///
/// Tie-break invariant: strict `>` on the running best ensures the
/// first-seen finite-max wins on ties. Combined with `BTreeMap`'s
/// lexicographic iteration over `Value::String` keys, this delivers
/// deterministic lex-min tie-break for free — no separate sort. Mirrors
/// the first-occurrence-wins discipline of `argmax_argmin_index` in
/// `field_reductions.rs` (around line 198) and `envelope_reduce` in
/// `crates/reify-stdlib/src/fea.rs`.
///
/// Convention: silent `Value::Undef` on any shape failure (non-Map first
/// arg, non-Lambda second arg, missing `"cases"` key, non-Map `cases`
/// value, non-String case key, lambda result with no finite max). Matches
/// the silent-Undef discipline of `envelope_reduce` / `case_names` /
/// `result_for`.
///
/// Pinned per guard by per-case E2E smoke tests in
/// `crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs`:
/// - `wrong_arity` — `args.len() != 2` returns `Value::Undef` immediately
///   (internal guard; pinned by `eval_worst_case_dispatch_wrong_arity_returns_undef`
///   in mod tests). At the E2E level, `arity_one` / `arity_three` fall through
///   the inline dispatch arm (which only fires for `args.len() == 2`) to
///   `eval_fea`'s permanent `worst_case` Undef stub. Pinned by
///   `worst_case_arity_one_returns_undef` and
///   `worst_case_arity_three_returns_undef`.
/// - `no_cases_key` / `cases_not_map` — outer `Map.get("cases")` match
///   below: missing key or non-Map value returns `Value::Undef`. Pinned by
///   `worst_case_missing_cases_key_returns_undef` and
///   `worst_case_cases_value_not_map_returns_undef`.
/// - `lambda_non_field` — non-Field lambda result: `compute_max` returns
///   `Value::Undef`, `as_f64()` returns `None`, the case is skipped via
///   `_ => continue`; if no case yields a finite max, the function
///   returns `Value::Undef`. Pinned by
///   `worst_case_lambda_returns_non_field_returns_undef`.
fn eval_worst_case_dispatch(args: &[Value], ctx: &EvalContext) -> Value {
    // Silent-Undef discipline: wrong arity returns Undef instead of panicking.
    // The inline `worst_case` dispatch arm in `eval_expr` already guards
    // `evaluated_args.len() == 2`, so normal call paths never reach this
    // branch — but a future second call site that forgets that guard would
    // otherwise index-out-of-bounds on the element access below. Mirrors
    // the `apply_lambda` arity-guard pattern. Pinned by
    // `eval_worst_case_dispatch_wrong_arity_returns_undef` in mod tests.
    let [first, second] = args else {
        return Value::Undef;
    };
    // Guard: first arg must be a Map (the MultiCaseResult shape). Pinned by
    // `worst_case_non_map_first_arg_returns_undef`.
    let outer = match first {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    // Guard: second arg must be a Lambda. Pinned by
    // `worst_case_non_lambda_second_arg_returns_undef`.
    //
    // `matches!` rather than a match-rebinds form: the rebinding shape has
    // bitten similar dispatch sites where the matched binding sits unused,
    // weakening the type-driven check (adding fields to `Value::Lambda` would
    // not fail-compile a discard-bind guard). The explicit `matches!`
    // precondition keeps the intent legible and the `lambda` borrow appearing
    // only after the guard succeeds.
    if !matches!(second, Value::Lambda { .. }) {
        return Value::Undef;
    }
    let lambda = second;
    // Guard: outer Map must carry a `"cases"` key bound to a Map. Pinned by
    // `worst_case_missing_cases_key_returns_undef` and
    // `worst_case_cases_value_not_map_returns_undef`.
    let cases = match outer.get(&Value::String("cases".to_string())) {
        Some(Value::Map(c)) => c,
        _ => return Value::Undef,
    };
    let mut best: Option<(String, f64)> = None;
    for (case_key, elastic_result) in cases {
        // Guard: every case key must be a String (BTreeMap iteration is then
        // lexicographic on the UTF-8 bytes, giving the tie-break invariant).
        let name = match case_key {
            Value::String(s) => s.clone(),
            _ => return Value::Undef,
        };
        let field_val = apply_lambda(lambda, std::slice::from_ref(elastic_result), ctx);
        // Guard: lambda must return a Sampled Field (or a value with a finite
        // numeric max). `compute_max` returns Undef on non-Field / non-Sampled
        // / empty-data inputs; `as_f64` then returns None and the case is
        // skipped. With no case yielding a finite max, `best` stays None and
        // the function returns Undef below. Pinned by
        // `worst_case_lambda_returns_non_field_returns_undef`.
        let max_val = field_reductions::compute_max(&field_val);
        let max_f = match max_val.as_f64() {
            Some(f) if f.is_finite() => f,
            _ => continue,
        };
        match &best {
            None => best = Some((name, max_f)),
            Some((_, b)) => {
                if max_f.total_cmp(b).is_gt() {
                    best = Some((name, max_f));
                }
            }
        }
    }
    match best {
        Some((name, _)) => Value::String(name),
        None => Value::Undef,
    }
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

/// Apply a lambda to a point or vector, handling multi-param unpacking.
///
/// Accept both `Value::Point` and `Value::Vector` — they share structural
/// representation (both wrap `Vec<Value>`).  Mirrors the calculus convention
/// established in `extract_point_coords`, `compute_numerical_divergence_at_point`,
/// and `compute_numerical_curl_at_point`.
///
/// When the lambda has `params.len() > 1` and the input is a `Point` or `Vector`
/// with matching length, unpacks the components into individual scalar arguments
/// so the arity check in `apply_lambda` passes.  A single-param lambda
/// (`params.len() == 1`) always receives the whole Point/Vector unchanged (no
/// unpacking), preserving the single-param binding contract.
///
/// See also: `calculus.rs::extract_point_coords`.
pub(crate) fn apply_lambda_with_point_unpacking(
    lambda: &Value,
    point: &Value,
    ctx: &EvalContext,
) -> Value {
    if let Value::Lambda { params, .. } = lambda {
        if params.len() > 1
            && let Value::Point(items) | Value::Vector(items) = point
            && params.len() == items.len()
        {
            return apply_lambda(lambda, items.as_slice(), ctx);
        }
        apply_lambda(lambda, std::slice::from_ref(point), ctx)
    } else {
        Value::Undef
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
        // Complex number methods are in complex.rs.
        "magnitude" | "phase" | "conjugate" | "re" | "im" => {
            match complex::eval_complex_method(obj, method, args) {
                Some(v) => v,
                None => Value::Undef,
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
        BinOp::Implies => return Value::Undef, // placeholder; eval_implies wired in step-6
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
        BinOp::And | BinOp::Or | BinOp::Implies => unreachable!(),
    }
}

/// Kleene AND: false ∧ Undef = false
///
/// Delegates truth-table folding to [`kleene::kleene_and`] while preserving:
/// - Short-circuit on type error (non-bool/non-undef left → `Value::Undef`,
///   right not evaluated).
/// - Short-circuit on absorbing element (`False` left → `Value::Bool(false)`,
///   right not evaluated).
fn eval_and(left: &CompiledExpr, right: &CompiledExpr, ctx: &EvalContext) -> Value {
    let lv = eval_expr(left, ctx);
    let lk = match kleene::KBool::try_from(&lv) {
        Ok(k) => k,
        Err(_) => return Value::Undef,
    };
    // Short-circuit on absorbing element: False ∧ anything = False.
    if matches!(lk, kleene::KBool::False) {
        return Value::Bool(false);
    }
    let rv = eval_expr(right, ctx);
    let rk = match kleene::KBool::try_from(&rv) {
        Ok(k) => k,
        Err(_) => return Value::Undef,
    };
    kleene::kleene_and(lk, rk).into()
}

/// Kleene OR: true ∨ Undef = true
///
/// Delegates truth-table folding to [`kleene::kleene_or`] while preserving:
/// - Short-circuit on type error (non-bool/non-undef left → `Value::Undef`,
///   right not evaluated).
/// - Short-circuit on absorbing element (`True` left → `Value::Bool(true)`,
///   right not evaluated).
fn eval_or(left: &CompiledExpr, right: &CompiledExpr, ctx: &EvalContext) -> Value {
    let lv = eval_expr(left, ctx);
    let lk = match kleene::KBool::try_from(&lv) {
        Ok(k) => k,
        Err(_) => return Value::Undef,
    };
    // Short-circuit on absorbing element: True ∨ anything = True.
    if matches!(lk, kleene::KBool::True) {
        return Value::Bool(true);
    }
    let rv = eval_expr(right, ctx);
    let rk = match kleene::KBool::try_from(&rv) {
        Ok(k) => k,
        Err(_) => return Value::Undef,
    };
    kleene::kleene_or(lk, rk).into()
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
            Value::Complex {
                re: -re,
                im: -im,
                dimension,
            }
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
    slice.first().is_some_and(|v| matches!(v, Value::Tensor(_)))
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
                if !quaternion_is_finite(*w, *x, *y, *z) {
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
                if !quaternion_is_finite(*w, *x, *y, *z) {
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
                        if !quaternion_is_finite(*w1, *x1, *y1, *z1) {
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
                        if !quaternion_is_finite(rw, rx, ry, rz) {
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
        // Complex / Complex: (a+bi)/(c+di) = ((ac+bd)+(bc-ad)i)/(c²+d²)
        // NOTE: No sanitize_value here — by design, matching eval_mul Complex*Complex (lib.rs:2185).
        // Overflow (e.g. MAX/0.5 → Inf) propagates as an Inf-bearing Complex in the operator path;
        // the `complex_div` builtin (reify-stdlib/src/complex.rs) adds sanitize_value for Inf→Undef.
        // The divergence is intentional and pinned by `complex_div_complex_overflow_propagates_infinity`.
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
            let denom = br * br + bi * bi;
            if denom == 0.0 {
                Value::Undef
            } else {
                Value::Complex {
                    re: (ar * br + ai * bi) / denom,
                    im: (ai * br - ar * bi) / denom,
                    dimension: ad.div(bd),
                }
            }
        }
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
        // Note: `v` cannot be `Value::Undef` here — the guard above returns
        // early for Undef, so `KBool::try_from` will never produce
        // `Ok(KBool::Undef)`.  `kleene_not` is the truth-table authority for
        // the Bool(true)/Bool(false) cases; non-bool inputs fall to `Err`.
        UnOp::Not => match kleene::KBool::try_from(&v) {
            Ok(k) => kleene::kleene_not(k).into(),
            Err(_) => Value::Undef,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DimensionVector, Type, ValueCellId};
    use reify_ir::CompiledMatchArm;

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
            content_hash: reify_core::ContentHash::of(&[99]),
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
            content_hash: reify_core::ContentHash::of(&[99]),
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
            content_hash: reify_core::ContentHash::of(&[42]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
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
            content_hash: reify_core::ContentHash::of(&[43]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
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
            content_hash: reify_core::ContentHash::of(&[44]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
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
            content_hash: reify_core::ContentHash::of(&[45]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
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
            content_hash: reify_core::ContentHash::of(&[46]),
            result_type: Type::length(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
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
            content_hash: reify_core::ContentHash::of(&[47]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "abs".to_string(),
                    qualified_name: "std::abs".to_string(),
                },
                args: vec![],
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &EvalContext::simple(&values)).is_undef());
    }

    // ── flat_map function-call interception (task 2698) ───────────────────────

    /// Construct a `Value::Lambda` from param (name, id) pairs, a body
    /// `CompiledExpr`, and captures.  Mirrors the shape used by
    /// `make_value_lambda`/`lambda_literal` in
    /// `crates/reify-expr/tests/collection_eval_tests.rs:518-548`; we
    /// replicate it here because src/-side test modules cannot import from
    /// tests/.
    fn make_lambda(
        params: Vec<(&str, ValueCellId)>,
        body: CompiledExpr,
        captures: ValueMap,
    ) -> Value {
        Value::Lambda {
            params: params
                .into_iter()
                .map(|(n, id)| (n.to_string(), id))
                .collect(),
            body: Box::new(body),
            captures,
        }
    }

    /// Wrap a `Value::Lambda` in a `CompiledExpr::literal` whose static
    /// `Type::Function { return_type, .. }` faithfully tracks the lambda
    /// body's `result_type`. Runtime evaluation does not consult this static
    /// type (it dispatches on the runtime `Value::Lambda` shape), but having
    /// the static signature line up with the body keeps these test fixtures
    /// honest under any future consumer that does inspect the lambda's
    /// declared signature (e.g. compile-time inference, IR pretty-printers).
    fn lambda_lit(
        params: Vec<(&str, ValueCellId)>,
        body: CompiledExpr,
        captures: ValueMap,
    ) -> CompiledExpr {
        let return_type = body.result_type.clone();
        let lambda = make_lambda(params, body, captures);
        CompiledExpr::literal(
            lambda,
            Type::Function {
                params: vec![],
                return_type: Box::new(return_type),
            },
        )
    }

    fn flat_map_call(list_arg: CompiledExpr, lambda_arg: CompiledExpr) -> CompiledExpr {
        CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[101]),
            result_type: Type::List(Box::new(Type::Int)),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "std::flat_map".to_string(),
                },
                args: vec![list_arg, lambda_arg],
            },
        }
    }

    fn empty_list_lit() -> CompiledExpr {
        CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)))
    }

    #[test]
    fn function_call_flat_map_empty_input() {
        // flat_map([], |x| [x]) -> []
        let x_id = ValueCellId::new("$lambda_flat_map_empty.S", "x");
        let body = CompiledExpr::list_literal(
            vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let lambda = lambda_lit(vec![("x", x_id)], body, ValueMap::new());
        let expr = flat_map_call(empty_list_lit(), lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert_eq!(result, Value::List(vec![]));
    }

    #[test]
    fn function_call_flat_map_lambda_returns_non_list_is_undef() {
        // flat_map([1, 2], |x| x) -> Undef (lambda body is Int, not List)
        let x_id = ValueCellId::new("$lambda_flat_map_nonlist.S", "x");
        let body = CompiledExpr::value_ref(x_id.clone(), Type::Int);
        let lambda = lambda_lit(vec![("x", x_id)], body, ValueMap::new());
        let list = CompiledExpr::list_literal(
            vec![lit(Value::Int(1), Type::Int), lit(Value::Int(2), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let expr = flat_map_call(list, lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            result.is_undef(),
            "flat_map with non-list lambda result should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn function_call_flat_map_wrong_lambda_arity_is_undef() {
        // flat_map([1], |x, y| [x]) -> Undef (apply_lambda enforces arity).
        // The 2-arg lambda gets called with 1 arg per element, hits the
        // arity check at apply_lambda (lib.rs:855) and returns Undef.
        // The flat_map arm sees a non-List result and propagates Undef.
        let x_id = ValueCellId::new("$lambda_flat_map_arity.S", "x");
        let y_id = ValueCellId::new("$lambda_flat_map_arity.S", "y");
        let body = CompiledExpr::list_literal(
            vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let lambda = lambda_lit(vec![("x", x_id), ("y", y_id)], body, ValueMap::new());
        let list = CompiledExpr::list_literal(
            vec![lit(Value::Int(1), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let expr = flat_map_call(list, lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            result.is_undef(),
            "flat_map with wrong-arity lambda should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn function_call_flat_map_basic() {
        // flat_map([1, 2, 3], |x| [x, x * 2]) -> [1, 2, 2, 4, 3, 6]
        let x_id = ValueCellId::new("$lambda_flat_map.S", "x");
        let body = CompiledExpr::list_literal(
            vec![
                CompiledExpr::value_ref(x_id.clone(), Type::Int),
                CompiledExpr::binop(
                    BinOp::Mul,
                    CompiledExpr::value_ref(x_id.clone(), Type::Int),
                    lit(Value::Int(2), Type::Int),
                    Type::Int,
                ),
            ],
            Type::List(Box::new(Type::Int)),
        );
        let lambda = lambda_lit(vec![("x", x_id)], body, ValueMap::new());
        let list = CompiledExpr::list_literal(
            vec![
                lit(Value::Int(1), Type::Int),
                lit(Value::Int(2), Type::Int),
                lit(Value::Int(3), Type::Int),
            ],
            Type::List(Box::new(Type::Int)),
        );
        let expr = flat_map_call(list, lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert_eq!(
            result,
            Value::List(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(2),
                Value::Int(4),
                Value::Int(3),
                Value::Int(6),
            ])
        );
    }

    // ─── flat_map runtime fallback regression guards (task 2698 amendment) ───
    //
    // These tests lock in the silent-Undef convention for the runtime
    // fallback branches in the `"flat_map"` arm of eval_expr's FunctionCall
    // match. Together they cover every leg of the `_ => Value::Undef`
    // outer fallback and the `_ => return Value::Undef` per-element
    // fallback, ensuring future refactors don't accidentally turn one of
    // these into a panic, propagation of garbage, or a different sentinel.

    /// First arg is not `Value::List` — outer match falls into `_ => Undef`.
    #[test]
    fn function_call_flat_map_non_list_first_arg_is_undef() {
        let x_id = ValueCellId::new("$lambda_flat_map_non_list_first.S", "x");
        let body = CompiledExpr::list_literal(
            vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let lambda = lambda_lit(vec![("x", x_id)], body, ValueMap::new());
        let expr = flat_map_call(lit(Value::Int(42), Type::Int), lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            result.is_undef(),
            "flat_map(non_list, lambda) should be Undef, got {:?}",
            result
        );
    }

    /// Second arg is not `Value::Lambda` — outer match falls into `_ => Undef`.
    #[test]
    fn function_call_flat_map_non_lambda_second_arg_is_undef() {
        let list = CompiledExpr::list_literal(
            vec![lit(Value::Int(1), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let expr = flat_map_call(list, lit(Value::Int(7), Type::Int));
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            result.is_undef(),
            "flat_map(list, non_lambda) should be Undef, got {:?}",
            result
        );
    }

    /// Input list contains `Value::Undef`. Because the lambda body wraps its
    /// argument (`[x]`), an Undef element produces `[Undef]` — a list — so
    /// flat_map concatenates normally and returns `[Undef]`.
    ///
    /// Note: the per-element non-list fallback (`_ => return Value::Undef`)
    /// is **not** exercised here. That leg is covered by the sibling test
    /// `function_call_flat_map_lambda_returns_undef_propagates_undef`, whose
    /// lambda body evaluates to a literal `Value::Undef` (not wrapped in a
    /// list), so the fallback fires and poisons the whole result.
    #[test]
    fn function_call_flat_map_input_with_undef_element_propagates() {
        let x_id = ValueCellId::new("$lambda_flat_map_undef_in.S", "x");
        let body = CompiledExpr::list_literal(
            vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let lambda = lambda_lit(vec![("x", x_id)], body, ValueMap::new());
        let list = CompiledExpr::list_literal(
            vec![lit(Value::Int(1), Type::Int), lit(Value::Undef, Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let expr = flat_map_call(list, lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert_eq!(
            result,
            Value::List(vec![Value::Int(1), Value::Undef]),
            "flat_map should propagate Undef elements through the lambda \
             body without short-circuiting; got {:?}",
            result
        );
    }

    /// Lambda body evaluates to `Value::Undef` for one element (e.g. by
    /// dividing by zero or calling a poisoned cell) — Undef is not
    /// `Value::List`, so the per-element fallback `_ => return Undef` fires
    /// and the whole flat_map result becomes Undef. This locks the
    /// "non-list lambda result poisons the whole call" contract.
    #[test]
    fn function_call_flat_map_lambda_returns_undef_propagates_undef() {
        let x_id = ValueCellId::new("$lambda_flat_map_undef_body.S", "x");
        // Body: literal Undef (not wrapped in a list) — exercises the
        // `_ => return Value::Undef` fallback inside the match on `r`.
        let body = lit(Value::Undef, Type::Int);
        let lambda = lambda_lit(vec![("x", x_id)], body, ValueMap::new());
        let list = CompiledExpr::list_literal(
            vec![lit(Value::Int(1), Type::Int)],
            Type::List(Box::new(Type::Int)),
        );
        let expr = flat_map_call(list, lambda);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            result.is_undef(),
            "flat_map should return Undef when the lambda body yields \
             Undef on any element, got {:?}",
            result
        );
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
            reify_ir::CompiledMatchArm {
                patterns: vec!["In".to_string()],
                body: lit(Value::Int(1), Type::Int),
            },
            reify_ir::CompiledMatchArm {
                patterns: vec!["Out".to_string()],
                body: lit(Value::Int(2), Type::Int),
            },
            reify_ir::CompiledMatchArm {
                patterns: vec!["Bidi".to_string()],
                body: lit(Value::Int(3), Type::Int),
            },
        ];
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[100]),
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
        let arms = vec![reify_ir::CompiledMatchArm {
            patterns: vec!["In".to_string()],
            body: lit(Value::Int(1), Type::Int),
        }];
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[101]),
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
            reify_ir::CompiledMatchArm {
                patterns: vec!["In".to_string()],
                body: lit(Value::Int(1), Type::Int),
            },
            reify_ir::CompiledMatchArm {
                patterns: vec!["_".to_string()],
                body: lit(Value::Int(99), Type::Int),
            },
        ];
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[102]),
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
            reify_ir::CompiledMatchArm {
                patterns: vec!["Socket".to_string(), "Button".to_string()],
                body: lit(Value::String("recessed".to_string()), Type::String),
            },
            reify_ir::CompiledMatchArm {
                patterns: vec!["Slider".to_string()],
                body: lit(Value::String("raised".to_string()), Type::String),
            },
        ];
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[103]),
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 4.0).abs() < 1e-12);
                assert!(dimension.is_dimensionless());
            }
            other => panic!("expected Scalar{{dimensionless}}, got {:?}", other),
        }
    }

    // ── task 3540 step-17: StructureInstanceCtor eval (RED) ─────────────────
    //
    // `eval_expr` on `CompiledExprKind::StructureInstanceCtor` must build a
    // `Value::StructureInstance { type_id: StructureTypeId(0), type_name,
    // version, fields }`. `fields` = each ordered_arg evaluated in order,
    // PLUS each default whose name is not covered by ordered_args, evaluated
    // in the SAME `EvalContext`. No registry lookup (reify-expr stays
    // registry-free — design-decision-2); type_id/type_name/version are baked
    // at lowering time. Undef field values stay Undef IN THEIR SLOT (the
    // ctor does NOT strict-short-circuit the whole structure to Undef the
    // way `FunctionCall` does). RED until step-18 replaces the placeholder
    // arm (currently returns `Value::Undef`).

    /// Build a `CompiledExpr::structure_instance_ctor` test fixture.
    fn sct(
        name: &str,
        version: u32,
        ordered: Vec<(&str, CompiledExpr)>,
        defaults: Vec<(&str, CompiledExpr)>,
    ) -> CompiledExpr {
        CompiledExpr::structure_instance_ctor(
            reify_ir::StructureTypeId(0),
            name.to_string(),
            version,
            ordered
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
            defaults
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
            Type::StructureRef(name.to_string()),
        )
    }

    #[test]
    fn structure_instance_ctor_all_args_supplied() {
        let expr = sct(
            "Steel_AISI_1045",
            1,
            vec![
                ("youngs_modulus", lit(Value::Int(200), Type::Int)),
                ("poisson", lit(Value::Real(0.3), Type::Real)),
            ],
            vec![],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_id, reify_ir::StructureTypeId(0));
                assert_eq!(data.type_name, "Steel_AISI_1045");
                assert_eq!(data.version, 1);
                assert_eq!(
                    data.fields.get(&"youngs_modulus".to_string()),
                    Some(&Value::Int(200))
                );
                assert_eq!(
                    data.fields.get(&"poisson".to_string()),
                    Some(&Value::Real(0.3))
                );
                assert_eq!(data.fields.len(), 2);
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    #[test]
    fn structure_instance_ctor_omitted_args_use_defaults() {
        // `target` supplied; `magnitude` omitted → its default fills the slot.
        let expr = sct(
            "PointLoad",
            1,
            vec![("target", lit(Value::Int(7), Type::Int))],
            vec![("magnitude", lit(Value::Int(0), Type::Int))],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                assert_eq!(data.fields.get(&"target".to_string()), Some(&Value::Int(7)));
                assert_eq!(
                    data.fields.get(&"magnitude".to_string()),
                    Some(&Value::Int(0)),
                    "omitted param filled from its captured default"
                );
                assert_eq!(data.fields.len(), 2);
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    #[test]
    fn structure_instance_ctor_nested_default_recurses() {
        // A default expression that is itself a structure ctor recurses
        // through the same eval path → nested Value::StructureInstance.
        let inner = sct(
            "Steel_AISI_1045",
            1,
            vec![("youngs_modulus", lit(Value::Int(200), Type::Int))],
            vec![],
        );
        let outer = sct("Beam", 2, vec![], vec![("material", inner)]);
        let values = ValueMap::new();
        match eval_expr(&outer, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "Beam");
                assert_eq!(data.version, 2);
                match data.fields.get(&"material".to_string()) {
                    Some(Value::StructureInstance(inner)) => {
                        assert_eq!(inner.type_name, "Steel_AISI_1045");
                        assert_eq!(
                            inner.fields.get(&"youngs_modulus".to_string()),
                            Some(&Value::Int(200))
                        );
                    }
                    other => panic!(
                        "expected nested StructureInstance for 'material', got {:?}",
                        other
                    ),
                }
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    #[test]
    fn structure_instance_ctor_undef_field_propagates_in_slot() {
        // An unbound ValueRef evals to Undef. The ctor keeps the structure
        // (does NOT strict-short-circuit to Undef like FunctionCall): the
        // Undef stays in that one slot.
        let expr = sct(
            "Beam",
            1,
            vec![
                ("length", lit(Value::Int(5), Type::Int)),
                ("mystery", vref("Nowhere", "missing", Type::Real)),
            ],
            vec![],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                assert_eq!(data.fields.get(&"length".to_string()), Some(&Value::Int(5)));
                assert_eq!(
                    data.fields.get(&"mystery".to_string()),
                    Some(&Value::Undef),
                    "Undef field value stays Undef in its own slot"
                );
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    // ── User function evaluation tests ──────────────────────────────────

    use reify_core::ContentHash;
    use reify_ir::{CompiledFnBody, CompiledFunction};

    fn make_double_fn() -> CompiledFunction {
        // fn double(x: Real) -> Real { x + x }
        let params = vec![("x".to_string(), Type::Real)];
        CompiledFunction {
            name: "double".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
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
            annotations: vec![],
            optimized_target: None,
        }
    }

    fn make_fn_with_let() -> CompiledFunction {
        // fn f(x: Real) -> Real { let y = x + 1; y * 2 }
        let params = vec![("x".to_string(), Type::Real)];
        CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
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
            annotations: vec![],
            optimized_target: None,
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
        let params = vec![("n".to_string(), Type::Int)];
        CompiledFunction {
            name: "factorial".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
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
            annotations: vec![],
            optimized_target: None,
        }
    }

    fn make_infinite_fn() -> CompiledFunction {
        // fn infinite(x: Int) -> Int { infinite(x) }
        let params = vec![("x".to_string(), Type::Int)];
        CompiledFunction {
            name: "infinite".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
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
            annotations: vec![],
            optimized_target: None,
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
        let params = vec![
            ("w".to_string(), Type::length()),
            ("h".to_string(), Type::length()),
        ];
        let area_fn = CompiledFunction {
            name: "area".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
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
            annotations: vec![],
            optimized_target: None,
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
        let params1 = vec![("x".to_string(), Type::Real)];
        let process1 = CompiledFunction {
            name: "process".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params1),
            params: params1,
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
            annotations: vec![],
            optimized_target: None,
        };
        // fn process(x: Real, y: Real) -> Real { x + y }
        let params2 = vec![("x".to_string(), Type::Real), ("y".to_string(), Type::Real)];
        let process2 = CompiledFunction {
            name: "process".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params2),
            params: params2,
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
            annotations: vec![],
            optimized_target: None,
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
                assert_eq!(
                    dimension,
                    DimensionVector::DIMENSIONLESS,
                    "dimension should be DIMENSIONLESS"
                );
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
                assert_eq!(
                    dimension,
                    DimensionVector::LENGTH,
                    "dimension should be LENGTH"
                );
            }
            other => panic!("expected Complex, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Full truth-table characterization tests for eval-level Kleene operators.
    // These pin end-to-end behavior through eval_expr so that the step-4
    // refactor (delegating to kleene::*) cannot silently drift.
    // -----------------------------------------------------------------------

    /// Assert that evaluating `expr` (a BinOp or UnOp literal) yields `expected`.
    fn check(expr: CompiledExpr, expected: Value) {
        let values = ValueMap::new();
        let got = eval_expr(&expr, &EvalContext::simple(&values));
        assert_eq!(got, expected, "eval result mismatch");
    }

    #[test]
    fn eval_binop_and_truth_table_full() {
        // Row: L  ∧  R  =  expected
        let cases: &[(Value, Value, Value)] = &[
            (Value::Bool(true), Value::Bool(true), Value::Bool(true)),
            (Value::Bool(true), Value::Bool(false), Value::Bool(false)),
            (Value::Bool(true), Value::Undef, Value::Undef),
            (Value::Bool(false), Value::Bool(true), Value::Bool(false)),
            (Value::Bool(false), Value::Bool(false), Value::Bool(false)),
            (Value::Bool(false), Value::Undef, Value::Bool(false)), // absorbing
            (Value::Undef, Value::Bool(true), Value::Undef),
            (Value::Undef, Value::Bool(false), Value::Bool(false)), // absorbing
            (Value::Undef, Value::Undef, Value::Undef),
        ];
        for (l, r, expected) in cases {
            let expr = CompiledExpr::binop(
                BinOp::And,
                lit(l.clone(), Type::Bool),
                lit(r.clone(), Type::Bool),
                Type::Bool,
            );
            check(expr, expected.clone());
        }
    }

    #[test]
    fn eval_binop_or_truth_table_full() {
        // Row: L  ∨  R  =  expected
        let cases: &[(Value, Value, Value)] = &[
            (Value::Bool(true), Value::Bool(true), Value::Bool(true)),
            (Value::Bool(true), Value::Bool(false), Value::Bool(true)),
            (Value::Bool(true), Value::Undef, Value::Bool(true)), // absorbing
            (Value::Bool(false), Value::Bool(true), Value::Bool(true)),
            (Value::Bool(false), Value::Bool(false), Value::Bool(false)),
            (Value::Bool(false), Value::Undef, Value::Undef),
            (Value::Undef, Value::Bool(true), Value::Bool(true)), // absorbing
            (Value::Undef, Value::Bool(false), Value::Undef),
            (Value::Undef, Value::Undef, Value::Undef),
        ];
        for (l, r, expected) in cases {
            let expr = CompiledExpr::binop(
                BinOp::Or,
                lit(l.clone(), Type::Bool),
                lit(r.clone(), Type::Bool),
                Type::Bool,
            );
            check(expr, expected.clone());
        }
    }

    #[test]
    fn eval_unop_not_truth_table_full() {
        // ¬T=F, ¬F=T, ¬U=U
        let cases: &[(Value, Value)] = &[
            (Value::Bool(true), Value::Bool(false)),
            (Value::Bool(false), Value::Bool(true)),
            (Value::Undef, Value::Undef),
        ];
        for (operand, expected) in cases {
            let expr = CompiledExpr::unop(UnOp::Not, lit(operand.clone(), Type::Bool), Type::Bool);
            check(expr, expected.clone());
        }
    }

    #[test]
    fn eval_binop_and_non_bool_left_is_undef() {
        // Non-bool left operand → Value::Undef (type-error catch-all)
        let expr = CompiledExpr::binop(
            BinOp::And,
            lit(Value::Int(3), Type::Int),
            lit(Value::Bool(true), Type::Bool),
            Type::Bool,
        );
        assert!(eval_expr(&expr, &EvalContext::simple(&ValueMap::new())).is_undef());
    }

    #[test]
    fn eval_binop_or_non_bool_left_is_undef() {
        // Non-bool left operand → Value::Undef (type-error catch-all)
        let expr = CompiledExpr::binop(
            BinOp::Or,
            lit(Value::Int(3), Type::Int),
            lit(Value::Bool(false), Type::Bool),
            Type::Bool,
        );
        assert!(eval_expr(&expr, &EvalContext::simple(&ValueMap::new())).is_undef());
    }

    #[test]
    fn eval_unop_not_non_bool_is_undef() {
        // Non-bool operand → Value::Undef (type-error catch-all)
        let expr = CompiledExpr::unop(UnOp::Not, lit(Value::Int(3), Type::Int), Type::Bool);
        assert!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())).is_undef(),
            "UnOp::Not on non-bool operand must return Value::Undef (type-error contract)"
        );
    }

    /// Returns a `CompiledExpr` that panics if it is ever evaluated.
    ///
    /// Uses `CompiledExprKind::MetaAccess` as the panic mechanism: when
    /// `EvalContext.meta` is `None` (which is the case for
    /// `EvalContext::simple(&values)`), evaluation panics with
    /// "MetaAccess evaluation requires meta context in EvalContext".
    ///
    /// Place this expression on the right operand of AND/OR short-circuit
    /// tests. If the implementation correctly short-circuits, the sentinel is
    /// never evaluated → no panic → test passes. If a future refactor
    /// silently evaluates the right operand, the test panics loudly.
    fn panic_on_eval_sentinel() -> CompiledExpr {
        CompiledExpr::meta_access("__sentinel".into(), "should_not_evaluate".into())
    }

    /// Verifies the assumption that `panic_on_eval_sentinel()` actually panics
    /// when evaluated under `EvalContext::simple` (i.e., `ctx.meta` is `None`).
    ///
    /// If `MetaAccess` evaluation is ever changed to return `Value::Undef` instead
    /// of panicking on missing context, this test fails — alerting maintainers that
    /// the short-circuit sentinel mechanism is broken and the AND/OR short-circuit
    /// tests no longer provide reliable coverage.
    #[test]
    #[should_panic(expected = "MetaAccess evaluation requires meta context")]
    fn panic_on_eval_sentinel_panics_when_evaluated() {
        let sentinel = panic_on_eval_sentinel();
        let _ = eval_expr(&sentinel, &EvalContext::simple(&ValueMap::new()));
    }

    /// Pins that `eval_and` short-circuits on a non-bool left operand:
    /// the right operand is **never evaluated** when the left is not bool/undef.
    ///
    /// Contract: non-bool left → `Value::Undef`, right NOT evaluated
    /// (see `eval_and`, type-error branch).
    ///
    /// The right operand is a `panic_on_eval_sentinel()`: if the implementation
    /// silently starts evaluating the right operand on this path, the test panics
    /// with "MetaAccess evaluation requires meta context in EvalContext".
    #[test]
    fn eval_and_short_circuit_on_non_bool_left_does_not_evaluate_right() {
        let expr = CompiledExpr::binop(
            BinOp::And,
            lit(Value::Int(3), Type::Int),
            panic_on_eval_sentinel(), // panics if evaluated
            Type::Bool,
        );
        // No panic → sentinel was not evaluated → short-circuit is preserved.
        assert!(eval_expr(&expr, &EvalContext::simple(&ValueMap::new())).is_undef());
    }

    /// Pins that `eval_and` short-circuits on the absorbing element `False`:
    /// the right operand is **never evaluated** when the left is `Bool(false)`.
    ///
    /// Contract: `False` left → `Value::Bool(false)`, right NOT evaluated
    /// (see `eval_and`, absorbing-element branch).
    ///
    /// The right operand is a `panic_on_eval_sentinel()`: if the implementation
    /// silently starts evaluating the right operand on this path, the test panics
    /// with "MetaAccess evaluation requires meta context in EvalContext".
    #[test]
    fn eval_and_short_circuit_on_false_absorbing_left_does_not_evaluate_right() {
        let expr = CompiledExpr::binop(
            BinOp::And,
            lit(Value::Bool(false), Type::Bool),
            panic_on_eval_sentinel(), // panics if evaluated
            Type::Bool,
        );
        // No panic → sentinel was not evaluated → short-circuit is preserved.
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())),
            Value::Bool(false)
        );
    }

    /// Pins that `eval_or` short-circuits on a non-bool left operand:
    /// the right operand is **never evaluated** when the left is not bool/undef.
    ///
    /// Contract: non-bool left → `Value::Undef`, right NOT evaluated
    /// (see `eval_or`, type-error branch).
    ///
    /// The right operand is a `panic_on_eval_sentinel()`: if the implementation
    /// silently starts evaluating the right operand on this path, the test panics
    /// with "MetaAccess evaluation requires meta context in EvalContext".
    #[test]
    fn eval_or_short_circuit_on_non_bool_left_does_not_evaluate_right() {
        let expr = CompiledExpr::binop(
            BinOp::Or,
            lit(Value::Int(3), Type::Int),
            panic_on_eval_sentinel(), // panics if evaluated
            Type::Bool,
        );
        // No panic → sentinel was not evaluated → short-circuit is preserved.
        assert!(eval_expr(&expr, &EvalContext::simple(&ValueMap::new())).is_undef());
    }

    /// Pins that `eval_or` short-circuits on the absorbing element `True`:
    /// the right operand is **never evaluated** when the left is `Bool(true)`.
    ///
    /// Contract: `True` left → `Value::Bool(true)`, right NOT evaluated
    /// (see `eval_or`, absorbing-element branch).
    ///
    /// The right operand is a `panic_on_eval_sentinel()`: if the implementation
    /// silently starts evaluating the right operand on this path, the test panics
    /// with "MetaAccess evaluation requires meta context in EvalContext".
    #[test]
    fn eval_or_short_circuit_on_true_absorbing_left_does_not_evaluate_right() {
        let expr = CompiledExpr::binop(
            BinOp::Or,
            lit(Value::Bool(true), Type::Bool),
            panic_on_eval_sentinel(), // panics if evaluated
            Type::Bool,
        );
        // No panic → sentinel was not evaluated → short-circuit is preserved.
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())),
            Value::Bool(true)
        );
    }

    // ── Task 2343 step-7b: composed-field call dispatch ──────────────────
    //
    // Pin the runtime fallthrough that turns a `field_name(p)` call inside
    // a composed lambda body into a dispatch to the captured
    // `__field.<field_name>` cell. The `_ =>` arm of the FunctionCall match
    // (eval_expr) checks ctx.values for a `Value::Field` keyed by the
    // resolved `__field.<name>` cell ID; if present, it dispatches via
    // `apply_lambda_with_point_unpacking`. Otherwise it falls through to
    // `reify_stdlib::eval_builtin` (preserving the prior behavior for
    // unknown / non-field names — e.g. `function_call_unknown_returns_undef`
    // above is not affected because no `__field.nonexistent` cell exists).

    /// A captured `__field.base` cell containing a `Value::Field { lambda }`
    /// dispatches `base(p)` via `apply_lambda_with_point_unpacking`. The
    /// lambda body is `p * 2.0`, so `base(3.0) == 6.0`.
    #[test]
    fn function_call_dispatches_to_captured_field_cell() {
        use std::sync::Arc;

        // Lambda body: ValueRef of param `p`.
        let p_id = ValueCellId::new("__field.base", "p");
        let p_ref = CompiledExpr::value_ref(p_id.clone(), Type::Real);
        let body = CompiledExpr::binop(
            BinOp::Mul,
            p_ref,
            lit(Value::Real(2.0), Type::Real),
            Type::Real,
        );

        // The lambda value (as it would appear inside Value::Field.lambda).
        let lambda_value = Value::Lambda {
            params: vec![("p".to_string(), p_id)],
            body: Box::new(body),
            captures: ValueMap::new(),
        };

        // Build the field cell and seed the values map under __field.base.
        let field_value = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Composed,
            lambda: Arc::new(lambda_value),
        };
        let field_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "base");
        let mut values = ValueMap::new();
        values.insert(field_cell, field_value);

        // Synthesize a FunctionCall: `base(3.0)`.
        let call = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[100]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "base".to_string(),
                    qualified_name: "field::base".to_string(),
                },
                args: vec![lit(Value::Real(3.0), Type::Real)],
            },
        };

        // Dispatch must apply the lambda: 3.0 * 2.0 = 6.0.
        let result = eval_expr(&call, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!(
                (v - 6.0).abs() < 1e-12,
                "expected base(3.0) == 6.0 via field dispatch, got {}",
                v
            ),
            other => panic!("expected Real(6.0) via field dispatch, got {:?}", other),
        }
    }

    /// A name that does not resolve to a `Value::Field` falls through to
    /// `eval_builtin` exactly as before. Pins the no-regression contract:
    /// the new dispatch is a no-op for non-field names.
    #[test]
    fn function_call_falls_through_when_field_cell_absent() {
        // No `__field.abs` cell present → dispatch fall-through →
        // `reify_stdlib::eval_builtin("abs", &[Real(-3.0)])` runs and
        // returns Real(3.0).
        let arg = lit(Value::Real(-3.0), Type::Real);
        let call = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[101]),
            result_type: Type::Real,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "abs".to_string(),
                    qualified_name: "std::abs".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        let result = eval_expr(&call, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12),
            other => panic!(
                "expected Real(3.0) (eval_builtin fallthrough), got {:?}",
                other
            ),
        }
    }

    /// `eval_worst_case_dispatch` must return `Value::Undef` for any call with
    /// fewer or more than 2 arguments — the silent-Undef discipline.
    ///
    /// The inline dispatch arm (lib.rs line 444) already guards
    /// `evaluated_args.len() == 2`, so normal call paths never hit this
    /// branch. The test calls the private function directly to pin the
    /// internal guard that protects against a future second call site that
    /// forgets the arity check.
    #[test]
    fn eval_worst_case_dispatch_wrong_arity_returns_undef() {
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        let v = Value::Undef;

        for (n, args) in [
            (0usize, vec![]),
            (1, vec![v.clone()]),
            (3, vec![v.clone(), v.clone(), v.clone()]),
        ] {
            let result = eval_worst_case_dispatch(&args, &ctx);
            assert!(
                result.is_undef(),
                "wrong-arity len {} must return Undef, got {:?}",
                n,
                result
            );
        }
    }

    // ── AdHocSelector (@point) unit tests ────────────────────────────────────

    /// Build a length-dimensioned scalar for a given mm value.
    fn mm_lit(v_mm: f64) -> CompiledExpr {
        lit(
            Value::Scalar {
                si_value: v_mm * 1e-3,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        )
    }

    /// `@point(1mm, 2mm, 3mm)` should evaluate to a `Value::Frame` whose origin
    /// is a `Value::Point` of three length-dimensioned scalars (in SI metres) and
    /// whose basis is the identity orientation `Orientation { w: 1, x: 0, y: 0, z: 0 }`.
    ///
    /// RED on HEAD: current arm returns `Value::Undef` unconditionally.
    #[test]
    fn ad_hoc_selector_point_constructs_frame_at_world_coords() {
        use reify_ir::SelectorKind;
        let base = lit(Value::String("ignored".into()), Type::String);
        let args = vec![mm_lit(1.0), mm_lit(2.0), mm_lit(3.0)];
        let expr = CompiledExpr::ad_hoc_selector(base, SelectorKind::Point, args);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Frame { origin, basis } => {
                // Check origin is a Point of three length scalars (SI metres)
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3, "origin Point should have 3 components");
                        let expected = [1e-3_f64, 2e-3_f64, 3e-3_f64];
                        for (i, (comp, &exp)) in comps.iter().zip(&expected).enumerate() {
                            match comp {
                                Value::Scalar { si_value, dimension } => {
                                    assert_eq!(
                                        *dimension, DimensionVector::LENGTH,
                                        "origin[{i}] dimension should be LENGTH"
                                    );
                                    assert!(
                                        (si_value - exp).abs() < 1e-15,
                                        "origin[{i}] si_value: expected {exp}, got {si_value}"
                                    );
                                }
                                other => panic!(
                                    "origin[{i}] should be Value::Scalar, got {:?}", other
                                ),
                            }
                        }
                    }
                    other => panic!("origin should be Value::Point, got {:?}", other),
                }
                // Check basis is identity orientation
                match *basis {
                    Value::Orientation { w, x, y, z } => {
                        assert!(
                            (w - 1.0).abs() < 1e-15 && x.abs() < 1e-15
                                && y.abs() < 1e-15 && z.abs() < 1e-15,
                            "basis should be identity orientation (w=1,x=0,y=0,z=0), got w={w},x={x},y={y},z={z}"
                        );
                    }
                    other => panic!("basis should be Value::Orientation, got {:?}", other),
                }
            }
            other => panic!(
                "ad_hoc_selector @point(1mm,2mm,3mm) should return Value::Frame, got {:?}",
                other
            ),
        }
    }

    /// When one of the coordinate args is `Value::Undef`, `@point` should return
    /// `Value::Undef` as a defensive degraded path.
    ///
    /// RED on HEAD: current arm already returns `Value::Undef`, but for the wrong
    /// reason (blanket arm). After step-2 this test stays GREEN via the explicit
    /// Undef-propagation logic.
    #[test]
    fn ad_hoc_selector_point_with_undef_arg_returns_undef() {
        use reify_ir::SelectorKind;
        let base = lit(Value::String("ignored".into()), Type::String);
        let args = vec![
            mm_lit(1.0),
            lit(Value::Undef, Type::length()), // <-- Undef arg
            mm_lit(3.0),
        ];
        let expr = CompiledExpr::ad_hoc_selector(base, SelectorKind::Point, args);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            matches!(result, Value::Undef),
            "@point with Undef coord arg should return Value::Undef, got {:?}",
            result
        );
    }

    // ── task-3663 tests ───────────────────────────────────────────────────────

    /// Pins that `eval_expr` panics with the routing-violation message in every
    /// build profile when a `CrossSubGeometryRef` reaches the eval arm.  The
    /// entity name contains `'.'` so the constructor's `debug_assert` does not
    /// pre-empt the eval-side `unreachable!()`.  See the invariant comment at
    /// `eval_expr` (lib.rs:145) for the full routing-violation rationale.
    #[test]
    #[should_panic(expected = "CrossSubGeometryRef should be consumed by entity.rs")]
    fn cross_sub_geometry_ref_panics_in_eval_when_not_consumed() {
        // Entity contains '.' so the step-4 constructor debug_assert (which fires
        // only in debug builds) does not pre-empt the eval-side unreachable!().
        let expr = CompiledExpr::cross_sub_geometry_ref(
            ValueCellId::new("Parent.sub", "member"),
            Type::Geometry,
        );
        let values = ValueMap::new();
        // Should always panic with the routing-violation message, in every profile.
        eval_expr(&expr, &EvalContext::simple(&values));
    }

    /// Companion to `cross_sub_geometry_ref_panics_in_eval_when_not_consumed`:
    /// a `ValueRef` with the same `ValueCellId` shape does NOT panic — it
    /// returns `Value::Undef` when the cell is absent.  This pins that the
    /// panic above is keyed on the `CrossSubGeometryRef` *variant*, not on
    /// the id shape or the missing-value path.
    #[test]
    fn value_ref_with_identical_id_returns_undef_not_panics() {
        let expr = CompiledExpr::value_ref(ValueCellId::new("Parent.sub", "member"), Type::Geometry);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(
            result.is_undef(),
            "ValueRef with absent cell should return Value::Undef, got {:?}",
            result
        );
    }
}
