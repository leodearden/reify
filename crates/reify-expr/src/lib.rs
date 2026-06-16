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
mod sampled_fd;
mod sanitize;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use reify_ast::QuantifierKind;
use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, FIELD_ENTITY_PREFIX, SourceSpan, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, DeterminacyPredicateKind, DeterminacyState, FieldSourceKind, InterpolationKind, PersistentMap, SampledField, SampledGridKind, SelectorKind, StructureInstanceData, StructureTypeId, UnOp, UndefCause, Value, ValueMap, quaternion_is_finite};

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
    /// Optional sink for op/builtin contract-failure `UndefCause` entries
    /// (task 4323 γ, PRD undef-self-describing §4.3).
    ///
    /// When `Some`, `push_op_contract_failure` pushes an
    /// `UndefCause::OpContractFailed { code: OpContractViolation, span: empty }`
    /// into the sink at each op/builtin push site that returns `Value::Undef`
    /// with ALL inputs determined (genuine contract failure, not propagated undef).
    ///
    /// When `None`, all pushes are no-ops — preserving the legacy semantics
    /// for every call site that does not attach the sink (A1/G3 transparency:
    /// main-eval values are byte-identical with and without a sink attached).
    ///
    /// The engine's `record_op_contract_failures` helper attaches this sink during
    /// the post-eval re-evaluation pass; callers that want to test the sink
    /// directly can use `with_undef_cause_sink`.
    pub undef_causes: Option<&'a RefCell<Vec<UndefCause>>>,
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
            undef_causes: None,
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
            undef_causes: None,
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
            undef_causes: None,
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

    /// Attach an op/builtin contract-failure undef-cause sink (task 4323 γ).
    ///
    /// When attached, `push_op_contract_failure` pushes an
    /// `UndefCause::OpContractFailed { code: OpContractViolation, span: empty }`
    /// into the sink at each op/builtin site that returns `Value::Undef` with
    /// ALL inputs determined (genuine domain/contract failure, not propagated undef).
    ///
    /// The cell-level drain boundary in `record_op_contract_failures` (reify-eval)
    /// re-stamps the span with the owning cell's `decl.span`; the push site itself
    /// uses an empty placeholder span because `CompiledExpr` carries no span
    /// (spans are lost at compile).
    pub fn with_undef_cause_sink(mut self, sink: &'a RefCell<Vec<UndefCause>>) -> Self {
        self.undef_causes = Some(sink);
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
            undef_causes: self.undef_causes,
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
            // __interp_render intercept — MUST sit before the strict-Undef
            // short-circuit below. This placement is load-bearing: the
            // determinacy decision (PRD §6.3, task 3964) requires that an
            // Undef interpolation hole renders as the literal string "undef"
            // rather than poisoning the result with Value::Undef. A stdlib
            // binding would be reached only AFTER the short-circuit and could
            // never observe an Undef argument.
            //
            // Match on the fully qualified name ("std::__interp_render") rather
            // than the bare name so that a user-defined symbol that happens to
            // carry the unqualified name "__interp_render" is never silently
            // intercepted and mis-rendered.
            if function.qualified_name == "std::__interp_render" && evaluated_args.len() == 1 {
                return Value::String(interp_render(&evaluated_args[0]));
            }
            // Strict Undef propagation: if any arg is Undef, short-circuit
            if evaluated_args.iter().any(|v| v.is_undef()) {
                return Value::Undef;
            }
            // Field operations: sample, gradient, divergence, curl
            // These need access to the eval context for lambda application,
            // so they're handled here rather than in stdlib.
            match function.name.as_str() {
                "sample" if evaluated_args.len() == 2 => {
                    sample_field_at(&evaluated_args[0], &evaluated_args[1], ctx)
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
                // fn_field(lambda): wrap a user lambda as FieldSourceKind::Analytical.
                //
                // This is the β-phase intercepting builtin (task 4220,
                // PRD docs/prds/v0_6/std-fields-api.md §5.2). It constructs a
                // `Value::Field { source: Analytical, lambda: Arc(lambda) }` from
                // the evaluated first argument (which must be a `Value::Lambda`).
                //
                // Extracted into `eval_fn_field` (`#[inline(never)]`) to keep this
                // recursive frame small in debug builds — the two `Type` locals
                // (`domain_type`, `codomain_type`) would otherwise sit on every
                // `eval_expr` frame and risk overflowing the 2 MiB test-thread
                // stack at `MAX_RECURSION_DEPTH` levels of recursive user-fn
                // evaluation (same rationale as `eval_structure_instance_ctor`,
                // `eval_quantifier`, etc.; pinned by
                // `eval_user_fn_recursion_depth_exceeded`).
                "fn_field"
                    if evaluated_args.len() == 1
                        && matches!(&evaluated_args[0], Value::Lambda { .. }) =>
                {
                    eval_fn_field(&evaluated_args[0], &expr.result_type)
                }
                // from_samples(points, values, method): construct a Regular1D
                // gridded SampledField from explicit sample points.
                //
                // This is the γ-phase intercepting builtin (task 4221,
                // PRD docs/prds/v0_6/std-fields-api.md §D3/D5). It builds a
                // `Value::Field { source: Sampled, lambda: Arc(Value::SampledField(sf)) }`
                // from a uniform 1-D grid of points + values and an
                // InterpolationMethod variant.
                //
                // Gate: exactly 3 args (two Lists + one Enum). Mis-shaped args
                // fall through to eval_builtin → Undef (graceful degradation).
                // The strict-Undef short-circuit above already handles any
                // Undef arg before we get here.
                //
                // Extracted into `eval_from_samples` (`#[inline(never)]`) for
                // the same stack-frame-shrinking rationale as `eval_fn_field`.
                "from_samples" if evaluated_args.len() == 3 => eval_from_samples(
                    &evaluated_args[0],
                    &evaluated_args[1],
                    &evaluated_args[2],
                    &expr.result_type,
                    ctx,
                ),
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
                // Bounded reductions: max/min/argmax/argmin(field, bounds: BoundingBox).
                // Placed after the 1-arg arms; disjoint arg-count means no shadowing
                // of the 1-arg form or the binary numeric max/min(a, b).
                // A non-BoundingBox 2nd arg does NOT match this guard and falls
                // through to eval_builtin (→ Undef for Field operands).
                "max"
                    if evaluated_args.len() == 2
                        && matches!(&evaluated_args[0], Value::Field { .. })
                        && matches!(&evaluated_args[1], Value::BoundingBox { .. }) =>
                {
                    field_reductions::compute_max_bounded(
                        &evaluated_args[0],
                        &evaluated_args[1],
                        ctx,
                    )
                }
                "min"
                    if evaluated_args.len() == 2
                        && matches!(&evaluated_args[0], Value::Field { .. })
                        && matches!(&evaluated_args[1], Value::BoundingBox { .. }) =>
                {
                    field_reductions::compute_min_bounded(
                        &evaluated_args[0],
                        &evaluated_args[1],
                        ctx,
                    )
                }
                "argmax"
                    if evaluated_args.len() == 2
                        && matches!(&evaluated_args[0], Value::Field { .. })
                        && matches!(&evaluated_args[1], Value::BoundingBox { .. }) =>
                {
                    field_reductions::compute_argmax_bounded(
                        &evaluated_args[0],
                        &evaluated_args[1],
                        ctx,
                    )
                }
                "argmin"
                    if evaluated_args.len() == 2
                        && matches!(&evaluated_args[0], Value::Field { .. })
                        && matches!(&evaluated_args[1], Value::BoundingBox { .. }) =>
                {
                    field_reductions::compute_argmin_bounded(
                        &evaluated_args[0],
                        &evaluated_args[1],
                        ctx,
                    )
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
                    // Post-Undef builtin diagnostics: when a stackup / multi-load-
                    // case (`linear_combine`) / AffineMap-constructor / inverse-
                    // dynamics / iso_it_tolerance builtin returns `Value::Undef`,
                    // classify and emit its specific diagnostic into the ctx sink.
                    // The five name families are disjoint, so at most one diagnose
                    // helper fires for a single Undef. Consolidated into one
                    // `#[inline(never)]` helper so the owned `Diagnostic` locals
                    // live in that helper's frame, NOT on every recursive `eval_expr`
                    // frame — keeping the 2 MiB test-thread stack under
                    // `MAX_RECURSION_DEPTH` (pinned by
                    // `eval_user_fn_recursion_depth_exceeded`), the same stack-
                    // shrinking rationale as `emit_flexure_diagnostics`.
                    emit_undef_builtin_diagnostics(&function.name, &evaluated_args, &result, ctx);
                    // Flexure PRB constructors (task 3871) surface their §5.3 / §1
                    // diagnostics on BOTH the success and Undef paths — unlike the
                    // post-Undef-only stackup/fea hooks above, W_FlexureYielding /
                    // W_FlexurePrbOutOfRange fire on a SUCCESSFULLY constructed joint.
                    // Extracted into `emit_flexure_diagnostics` so the per-`diag`
                    // owned-`Diagnostic` loop local does not inflate every recursive
                    // `eval_expr` frame (the same stack-shrinking rationale as
                    // `eval_worst_case_dispatch`; pinned by
                    // `eval_user_fn_recursion_depth_exceeded`).
                    emit_flexure_diagnostics(&function.name, &evaluated_args, &result, ctx);
                    // DFM build-volume rules (task 4272) surface their severity
                    // diagnostic on BOTH the success and Undef paths, like the
                    // flexure hook above (not the post-Undef-only stackup/fea/geometry
                    // hooks): a `fits_build_volume` evaluating to Bool(false) is a
                    // build-volume VIOLATION (success path), while a Value::Undef is a
                    // usage error. Extracted into `emit_dfm_diagnostics` for the same
                    // stack-shrinking rationale as `emit_flexure_diagnostics`.
                    emit_dfm_diagnostics(&function.name, &evaluated_args, &result, ctx);
                    // Snapshot center_of_mass fallback Warning (task 4471): fires on
                    // BOTH paths like the flexure/DFM hooks — the fallback Warning is
                    // emitted even when center_of_mass returns a valid Point (success
                    // path), so the Undef-only `emit_undef_builtin_diagnostics` gate
                    // cannot surface it. A no-op for every non-snapshot name.
                    emit_snapshot_diagnostics(&function.name, &evaluated_args, &result, ctx);
                    // γ (task 4323): genuine op/builtin contract failure — all args are
                    // determined (strict undef-arg short-circuit above), so an Undef
                    // result here is a real domain/contract violation, NOT propagated
                    // undef. Push into the undef-cause sink if one is attached. When
                    // no sink is attached this is a no-op (A1/G3 transparency).
                    if result.is_undef() {
                        push_op_contract_failure(ctx, DiagnosticCode::OpContractViolation);
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
        } => {
            // Intercept `solve_load_cases` before the body-evaluation path so
            // `EvalContext` is available for per-case `solve_elastic_static`
            // dispatch.  `solve_load_cases` is a `pub fn` in `fea_multi_case.ri`
            // (compiled to `UserFunctionCall`), so the FunctionCall match block
            // above never sees it — the intercept lives here instead, mirroring
            // how `engine_eval.rs` intercepts @optimized `UserFunctionCall` nodes
            // before handing off to the body-eval path.
            //
            // Arity guard: 5 or 6 args (6th = `options`, may be default-padded).
            // Undef propagation: consistent with `eval_user_function_call` — if any
            // arg is Undef, skip the intercept and let the body return Undef.
            if function_name == "solve_load_cases" && (args.len() == 5 || args.len() == 6) {
                // Cheap arity precheck on the compiled arg count (no evaluation
                // yet). Wrong-arity calls fall straight through to
                // eval_user_function_call without paying arg-evaluation cost here
                // and without triggering double-evaluation on the decline path.
                let evaluated_args: Vec<Value> =
                    args.iter().map(|a| eval_expr(a, ctx)).collect();
                // Strict Undef propagation: return Undef directly (consistent with
                // eval_user_function_call's own Undef check) so the decline path
                // does not re-evaluate the args a second time inside
                // eval_user_function_call.
                if evaluated_args.iter().any(|v| v.is_undef()) {
                    return Value::Undef;
                }
                return eval_solve_load_cases(&evaluated_args, ctx);
            }
            eval_user_function_call(function_name, args, ctx)
        }

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
                let Some((value, state)) = det_map.get(cell) else {
                    debug_assert!(
                        false,
                        "DeterminacyPredicate references cell {:?} not in determinacy snapshot — wiring bug or eval-order violation",
                        cell
                    );
                    return Value::Undef;
                };
                let state = *state;
                let result = match kind {
                    // A cell is "determined" only when BOTH its state is
                    // Determined AND its resolved Value is non-Undef.
                    //
                    // Rationale: the eval pipeline legitimately stores
                    // (Value::Undef, DeterminacyState::Determined) for cells
                    // that have a binding expression but whose value depends on
                    // geometry not yet resolved (geometry-undef params). Such a
                    // cell is NOT determined from the caller's perspective —
                    // e.g. simulation_ready (task 4016) must not treat a
                    // geometry-undef input as a resolved one. Value::is_undef()
                    // is true only for Value::Undef; concrete values like
                    // Bool(false), Int(0), Option(None), and empty collections
                    // are genuine resolved values and remain "determined".
                    DeterminacyPredicateKind::Determined => {
                        state == DeterminacyState::Determined && !value.is_undef()
                    }
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
                    //   determined()           → resolved (Determined + non-Undef value)
                    //   undetermined()         → no value (Undetermined state)
                    //   constrained()          → solver variable (Auto/Provisional)
                    //   partially_determined() → solver in progress (Provisional)
                    //
                    // Note: a geometry-undef cell stored as (Undef, Determined) —
                    // e.g. a param whose default depends on geometry not yet resolved
                    // — falls into NEITHER the determined() nor the undetermined()
                    // bucket. This is intentional: determined() requires a non-Undef
                    // value (see its arm above), while undetermined() checks only for
                    // Undetermined state. Callers written as
                    //   `if undetermined(x) { … } else { /* treat as determined */ }`
                    // are therefore unsound for geometry-undef params; the correct
                    // pattern is `if determined(x) { … } else { /* not yet ready */ }`.
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
            lets,
        } => eval_structure_instance_ctor(
            *type_id,
            type_name,
            *version,
            ordered_args,
            defaults,
            lets,
            ctx,
        ),

        // task 4118 (γ): Selector→List<Geometry> coercion node. In the
        // registry-free evaluator this is a PASSTHROUGH — evaluate the inner
        // selector (yielding a `Value::Selector`) and return it unchanged.
        // Resolving the selector into a concrete `Value::List` of geometry
        // handles requires a `GeometryKernel`, so the real coercion happens in
        // the kernel-bearing post-process (reify-eval), not here.
        CompiledExprKind::ResolveSelector { selector } => eval_expr(selector, ctx),
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
    lets: &[(String, CompiledExpr)],
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
    if !lets.is_empty() {
        materialize_template_lets(type_name, lets, &mut fields, ctx);
    }
    Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
        type_id,
        type_name: type_name.to_string(),
        version,
        fields,
    }))
}

/// Eagerly materialize template `Let` cells into a just-built structure
/// instance's `fields` map (task-4342, step-6).
///
/// Marked `#[inline(never)]` for the same reason `eval_structure_instance_ctor`
/// is hoisted out: the locals here (a `ValueMap` plus loop temporaries) must
/// NOT inflate `eval_expr`'s stack frame or widen the frame the
/// `eval_user_fn_recursion_depth_exceeded` safety test relies on.  This
/// function is called only when `!lets.is_empty()`, so the deep user-fn path
/// (no struct lets) never pays the extra frame.
///
/// Algorithm: build a child `reify_ir::ValueMap` keyed by
/// `ValueCellId::new(type_name, member)` from the already-populated `fields`,
/// then iterate `lets` in declaration order.  For each let evaluate its
/// compiled expr against `ctx.with_scope(&child)` (so earlier lets are visible
/// to later ones), and write the result into BOTH the child map and `fields`.
#[inline(never)]
fn materialize_template_lets(
    type_name: &str,
    lets: &[(String, CompiledExpr)],
    fields: &mut PersistentMap<String, Value>,
    ctx: &EvalContext,
) {
    // Build a child ValueMap keyed by ValueCellId::new(type_name, member)
    // from the already-populated `fields` (params + defaults filled in by
    // eval_structure_instance_ctor).  The child scope lets the let exprs
    // reference sibling params by their ValueCellId (entity == type_name).
    //
    // PERFORMANCE NOTE: this loop performs an O(fields) deep-clone — one
    // `value.clone()` per param/default in the struct.  For structures with
    // many fields instantiated in hot loops this adds up.  In practice,
    // tolerancing-style derives are cheap arithmetic and this path is gated on
    // `!lets.is_empty()` in the caller, so structs with no derived lets pay
    // nothing.  If profiling ever shows this on a hot path, consider an
    // overlay/COW scope that borrows from `fields` directly rather than copying
    // every entry (analogous to how `ctx.with_scope` borrows without cloning the
    // full value map).
    let mut child = ValueMap::new();
    for (member, value) in fields.iter() {
        child.insert(ValueCellId::new(type_name, member.as_str()), value.clone());
    }

    // Evaluate lets in declaration order.  For each let:
    //   1. evaluate its compiled expr against ctx.with_scope(&child) — the
    //      temporary borrow is released after eval_expr returns, so
    //      child.insert on the next line is safe;
    //   2. insert the result into `child` so later lets see earlier ones;
    //   3. insert into `fields` for the final StructureInstance.
    //
    // Declaration order is a valid eval order: the compiler enforces left-to-right
    // scoping (a let can only reference earlier params/lets), so no topological
    // sort or cycle guard is needed here (unlike elaborate_child_lets_only).
    //
    // SCOPE INVARIANT (suggestion 2): `ctx.with_scope(&child)` REPLACES the value
    // map entirely.  This is intentional and correct for structure-def lets because:
    //
    //   (a) The compiler compiles each let expr in a `CompilationScope` that only
    //       registers the structure's own param names and earlier lets
    //       (entity.rs:1107 / entity.rs:4107,4136).  Any identifier not in that
    //       scope produces a compile error or an unresolved-ICE Undef expr — so a
    //       well-compiled let expr can only carry `ValueRef(ValueCellId{entity ==
    //       type_name, ...})` refs, all of which are present in `child`.
    //
    //   (b) The `sub`-path reference implementation (`elaborate_child_lets_only`,
    //       reify-eval/src/unfold.rs:570-574) ALSO evaluates let exprs against a
    //       child-scoped context (`eval_ctx_with_meta(&child_values, ...)`), not
    //       the full module ValueMap.  Our behavior is therefore identical to the
    //       sub baseline, guaranteeing sub-consistency by construction.
    //
    // If a future language feature adds module-level constants accessible inside
    // structure-def lets (analogous to C++ `static constexpr`), the child scope
    // would need to be layered over `ctx.values` for unresolved ids.  For now no
    // such feature exists; the invariant is enforced by the compiler.
    for (name, let_expr) in lets {
        let value = eval_expr(let_expr, &ctx.with_scope(&child));
        child.insert(ValueCellId::new(type_name, name.as_str()), value.clone());
        fields.insert(name.clone(), value);
    }
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

/// Returns `true` when `t` is, or recursively wraps, a [`Type::TraitObject`].
///
/// Local mirror of `reify_compiler::type_compat::type_carries_trait_object`,
/// kept VERBATIM with it. reify-expr's library deps are only reify-core +
/// reify-ir (reify-compiler is a dev-dep), so the compiler helper cannot be
/// imported here — the two MUST be kept in sync. If they drift, a call whose
/// param is a trait object (e.g. `loads: List<Load>`) resolves at compile time
/// (compile-side `resolve_function_overload` treats trait-carrying params as
/// wildcards for ALL fns) but the eval-side resolver rejects it → the
/// `@optimized` `ComputeNode` dispatch never fires and the call evals to
/// `Value::Undef` / no targets (the esc-4093-152 divergence class — the FEA
/// `solve_elastic_static(loads: List<Load>, supports: List<Support>)` signature
/// tightening).
///
/// Covers bare `TraitObject(name)` and the `Option`/`List`/`Set`/`Map` wrappers,
/// matching the compiler-side copy in `type_compat.rs`. Used by
/// [`find_matching_compiled_function`] to make trait-carrying params act as
/// eval-time resolution wildcards for non-generic fns too — unlike
/// [`type_carries_type_param`], this is NOT gated on `!type_params.is_empty()`,
/// because the compile-side `resolve_function_overload` applies the trait-object
/// wildcard to every candidate regardless of genericity.
fn type_carries_trait_object(t: &Type) -> bool {
    match t {
        Type::TraitObject(_) => true,
        Type::Option(inner) => type_carries_trait_object(inner),
        Type::List(inner) => type_carries_trait_object(inner),
        Type::Set(inner) => type_carries_trait_object(inner),
        Type::Map(key, val) => type_carries_trait_object(key) || type_carries_trait_object(val),
        _ => false,
    }
}

/// Returns `true` when `t` is, or recursively wraps, a [`Type::TypeParam`].
///
/// Local mirror of `reify_compiler::type_compat::type_carries_type_param`,
/// kept VERBATIM with it. reify-expr's library deps are only reify-core +
/// reify-ir (reify-compiler is a dev-dep), so the compiler helper cannot be
/// imported here — the two MUST be kept in sync. If they drift, a generic call
/// whose param embeds a type-param in a constructor covered by only one copy
/// resolves at compile time but the eval-side resolver rejects it → an
/// `id(..)`-style call falls back to `Value::Undef` (the esc-4231-120 /
/// esc-4231-126 divergence class).
///
/// Recurses through the same inner-`Type`-bearing constructor set as the
/// compiler-side `unify` / `substitute_type_params` walks —
/// `List`/`Set`/`Keyed`/`Option`/`Complex`/`Range`,
/// `Point`/`Vector`/`Tensor`/`Matrix` (quantity slot), `Map`, `Field`,
/// `Function` (params + return), and `Union`. Used by
/// [`find_matching_compiled_function`] to make a *generic* candidate's
/// type-param-carrying params act as eval-time resolution wildcards, gated on
/// `!f.type_params.is_empty()` so non-generic fns are bit-for-bit unchanged
/// (INV-6, task 4231 β-eval).
///
/// The `match` is intentionally exhaustive (no `_` wildcard) so a future `Type`
/// variant forces a compile-time decision here, in lock-step with the canonical
/// compiler-side copy.
fn type_carries_type_param(t: &Type) -> bool {
    match t {
        // The type-parameter leaf itself.
        Type::TypeParam(_) => true,

        // Single-inner-Type wrappers: recurse on the child.
        Type::List(inner)
        | Type::Set(inner)
        | Type::Keyed(inner)
        | Type::Option(inner)
        | Type::Complex(inner)
        | Type::Range(inner) => type_carries_type_param(inner),

        // Quantity-bearing aggregates: recurse into the quantity slot.
        Type::Point { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Tensor { quantity, .. }
        | Type::Matrix { quantity, .. } => type_carries_type_param(quantity),

        // Two-inner-Type wrappers.
        Type::Map(key, val) => type_carries_type_param(key) || type_carries_type_param(val),
        Type::Field { domain, codomain } => {
            type_carries_type_param(domain) || type_carries_type_param(codomain)
        }

        // Function: any param, or the return type.
        Type::Function {
            params,
            return_type,
        } => params.iter().any(type_carries_type_param) || type_carries_type_param(return_type),

        // Union: any arm.
        Type::Union(arms) => arms.iter().any(type_carries_type_param),

        // All remaining leaves carry no inner `Type`.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::Geometry
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::Direction
        // Relation directive (γ): an inner-Type-free leaf, carries no type param.
        | Type::Relation
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
        // Dimension-param scalar: opaque leaf — carries no *type* param.
        // MUST remain verbatim-synced with the canonical copy in
        // reify-compiler/src/type_compat.rs (drift reproduces esc-4231-120/126).
        // `type_carries_dim_param` (below) handles the ScalarParam wildcard case.
        | Type::ScalarParam(_)
        | Type::Error => false,
    }
}

/// Returns `true` when `t` is, or recursively wraps, a [`Type::ScalarParam`].
///
/// Local mirror of `reify_compiler::type_compat::type_carries_dim_param`,
/// kept VERBATIM with it. reify-expr's library deps are only reify-core +
/// reify-ir (reify-compiler is a dev-dep), so the compiler helper cannot be
/// imported here — the two MUST be kept in sync. If they drift, a generic call
/// whose param is a dimension-kinded `Scalar<Q>` resolves at compile time but
/// the eval-side resolver rejects it → the call evals to `Value::Undef`
/// (the ζ/D8 divergence class — the same failure mode as esc-4231-120/126 for
/// type-params).
///
/// Recurses through the same inner-`Type`-bearing constructor set as
/// [`type_carries_type_param`]. Returns `true` at the `ScalarParam(_)` leaf,
/// `false` at all other leaves. Used by [`find_matching_compiled_function`] to
/// make a *generic* candidate's dimension-param-carrying params act as
/// eval-time resolution wildcards, gated on `!f.type_params.is_empty()` (task ζ
/// / D8).
///
/// The `match` is intentionally exhaustive (no `_` wildcard) so a future `Type`
/// variant forces a compile-time decision here, in lock-step with the canonical
/// compiler-side copy.
fn type_carries_dim_param(t: &Type) -> bool {
    match t {
        // The dimension-parameter leaf itself.
        Type::ScalarParam(_) => true,

        // Single-inner-Type wrappers: recurse on the child.
        Type::List(inner)
        | Type::Set(inner)
        | Type::Keyed(inner)
        | Type::Option(inner)
        | Type::Complex(inner)
        | Type::Range(inner) => type_carries_dim_param(inner),

        // Quantity-bearing aggregates: recurse into the quantity slot.
        Type::Point { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Tensor { quantity, .. }
        | Type::Matrix { quantity, .. } => type_carries_dim_param(quantity),

        // Two-inner-Type wrappers.
        Type::Map(key, val) => type_carries_dim_param(key) || type_carries_dim_param(val),
        Type::Field { domain, codomain } => {
            type_carries_dim_param(domain) || type_carries_dim_param(codomain)
        }

        // Function: any param, or the return type.
        Type::Function {
            params,
            return_type,
        } => params.iter().any(type_carries_dim_param) || type_carries_dim_param(return_type),

        // Union: any arm.
        Type::Union(arms) => arms.iter().any(type_carries_dim_param),

        // All remaining leaves carry no inner ScalarParam.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::TypeParam(_)
        | Type::Geometry
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::Direction
        // Relation directive (γ): an inner-Type-free leaf, carries no dim param.
        | Type::Relation
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
        | Type::Error => false,
    }
}

/// Find the compiled function matching `name`, arity, and per-parameter
/// [`Type`] compatibility against the compiled arguments' result types.
///
/// This is the canonical overload-resolution helper shared by:
/// - [`eval_user_function_call`] in this crate, and
/// - the `@optimized` `UserFunctionCall` → `ComputeNode` lowering site in
///   `reify-eval/src/engine_eval.rs`.
///
/// Mirrors the compile-time `reify_compiler::type_compat::resolve_function_overload`
/// so eval re-selects the SAME overload the compiler chose (task 4231 β-eval):
/// - For a non-generic candidate (`type_params` empty) every *concrete* param keeps
///   **exact** type equality, but a **trait-object**-carrying param (e.g.
///   `List<Load>`) acts as a wildcard — mirroring compile-side
///   `resolve_function_overload`, which applies the trait-object wildcard to ALL
///   candidates regardless of genericity (esc-4093-152). Non-generic fns with no
///   trait-object params are bit-for-bit unchanged (INV-6).
/// - For a *generic* candidate a type-param-carrying param acts as a **wildcard**
///   (matches any arg type) — eval is type-erased (INV-2), so the concrete arg
///   binds the param positionally with no runtime type check.
/// - **Exact-match-wins tie-break:** if any candidate matches ALL params by exact
///   equality, generic wildcard matches are discarded first, so a concrete
///   overload still beats a generic one (mirrors resolve_function_overload).
///
/// If the resolution rule ever grows (e.g. subtyping, coercion ranking,
/// operator-overloading nuance), update only this function; both call sites
/// will inherit the fix automatically.
pub fn find_matching_compiled_function<'a>(
    fns: &'a [CompiledFunction],
    name: &str,
    args: &[CompiledExpr],
) -> Option<&'a CompiledFunction> {
    let arity_match = |f: &&CompiledFunction| f.name == name && f.params.len() == args.len();
    let exact = |((_, param_ty), arg): (&(String, Type), &CompiledExpr)| *param_ty == arg.result_type;

    // First-match-wins among candidates whose params ALL match by exact equality.
    // (Includes generic candidates only when their args happen to be exact —
    // e.g. a TypeParam param vs a concrete arg is NOT exact, so generics fall
    // through to the wildcard pass below.)
    if let Some(f) = fns
        .iter()
        .filter(arity_match)
        .find(|f| f.params.iter().zip(args.iter()).all(exact))
    {
        return Some(f);
    }

    // No exact overload — allow wildcard params to match:
    //   * a *generic* candidate's type-param-carrying params (gated on
    //     `!type_params.is_empty()` so this pass can never relax a non-generic
    //     fn via type-params), and
    //   * a *generic* candidate's dimension-param-carrying params (ScalarParam,
    //     task ζ / D8 — gated on genericity, same as type-param wildcards), and
    //   * ANY candidate's trait-object-carrying params (NOT gated on genericity),
    //     mirroring compile-side `resolve_function_overload` which treats
    //     trait-carrying params as wildcards for every candidate. Without this,
    //     a non-generic fn like
    //     `solve_elastic_static(loads: List<Load>, supports: List<Support>)`
    //     resolves at compile time but the eval-side resolver returns None →
    //     the `@optimized` ComputeNode dispatch never fires (esc-4093-152).
    fns.iter().filter(arity_match).find(|f| {
        let is_generic = !f.type_params.is_empty();
        f.params.iter().zip(args.iter()).all(|((_, param_ty), arg)| {
            (is_generic && (type_carries_type_param(param_ty) || type_carries_dim_param(param_ty)))
                || type_carries_trait_object(param_ty)
                || *param_ty == arg.result_type
        })
    })
}

/// Evaluate a compiled function's body with pre-evaluated `Value` arguments.
///
/// Builds a fresh scope by binding each param name to the corresponding arg,
/// evaluates any `let`-bindings in order, then evaluates the result expression.
/// Recursion-depth checking is the caller's responsibility.
///
/// Extracted to eliminate duplication between `eval_user_function_call`
/// (which evaluates `CompiledExpr` args and then calls this) and
/// `invoke_solve_elastic_static` (which already holds `Value` args).
fn eval_compiled_function_with_values(
    func: &CompiledFunction,
    args: &[Value],
    ctx: &EvalContext,
) -> Value {
    let mut scope = ValueMap::new();
    for ((param_name, _), arg_val) in func.params.iter().zip(args.iter()) {
        scope.insert(ValueCellId::new(&func.name, param_name), arg_val.clone());
    }
    for (binding_name, binding_expr) in &func.body.let_bindings {
        let val = {
            let body_ctx = ctx.with_scope(&scope);
            eval_expr(binding_expr, &body_ctx)
        };
        scope.insert(ValueCellId::new(&func.name, binding_name), val);
    }
    eval_expr(&func.body.result_expr, &ctx.with_scope(&scope))
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

    // Delegate scope-building and body evaluation to the shared helper.
    eval_compiled_function_with_values(func, &evaluated_args, ctx)
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

/// Render a [`Value`] to its human-display [`String`] for string-interpolation
/// holes (task 3964, PRD §3).
///
/// Uses the `format_display` family — **never** the [`std::fmt::Display`] impl —
/// to produce bare strings (not quoted), engineering units (5 mm not 0.005 m),
/// and composite forms.
///
/// Render rules:
/// - `Undef` → `"undef"` (the surface keyword; NOT `format_display`'s `"undefined"`)
/// - `Scalar | Complex | Option(Some(_))` → `format_display_pair` joined
///   `"{value} {unit}"` when the unit is non-empty (e.g. `5 mm`); plain value
///   when the unit is empty (dimensionless scalars)
/// - Everything else → `format_display` verbatim
///
/// # Nested Undef in composites
///
/// When `value` is a composite (e.g. `List`, `Map`, `Point`) whose *elements*
/// contain `Undef`, those inner undefs are rendered by `format_display`, which
/// emits `"undefined"` — **not** `"undef"`.  This divergence from the top-level
/// case is intentional: PRD §3 specifies the `Undef → "undef"` rule only for
/// the interpolation hole value itself, and `format_display` is the
/// authoritative renderer for composite interiors.  A future revision could
/// normalise the two spellings inside `format_display` if consistency becomes a
/// requirement.
fn interp_render(value: &Value) -> String {
    match value {
        Value::Undef => "undef".to_string(),
        Value::Scalar { .. } | Value::Complex { .. } | Value::Option(Some(_)) => {
            let (v, u) = value.format_display_pair();
            if u.is_empty() { v } else { format!("{v} {u}") }
        }
        _ => value.format_display(),
    }
}

/// Emit the post-`Undef` builtin diagnostics — stackup (§4.4), multi-load-case
/// FEA (`linear_combine`, task #10), AffineMap constructors (PRD §4.2, task β),
/// inverse-dynamics body mass, and ISO tolerancing — for a builtin call whose
/// `result` is `Value::Undef`.
///
/// Extracted from `eval_expr`'s `FunctionCall` arm — and marked
/// `#[inline(never)]` — for the same stack-frame-shrinking reason as
/// `emit_flexure_diagnostics` / `eval_worst_case_dispatch`: each `let Some(diag)`
/// binds an owned `Diagnostic`, and in unoptimized builds those by-value locals
/// would otherwise sit on every recursive `eval_expr` frame (regardless of which
/// match arm runs) and blow the 2 MiB test-thread stack at `MAX_RECURSION_DEPTH`
/// levels of recursive user-fn evaluation (pinned by
/// `eval_user_fn_recursion_depth_exceeded`).
///
/// The five name families (stackup math builtins / `"linear_combine"` /
/// `affine_*` constructors / inverse-dynamics / `"iso_it_tolerance"`) are
/// disjoint, so at most one of the five classifiers returns `Some` for any
/// single `Undef`; each returns `None` for every other name or for valid input,
/// making this a cheap no-op for ordinary builtins.
#[inline(never)]
fn emit_undef_builtin_diagnostics(name: &str, args: &[Value], result: &Value, ctx: &EvalContext) {
    if !matches!(result, Value::Undef) {
        return;
    }
    let Some(sink) = ctx.diagnostics else {
        return;
    };
    // §4.4 stackup error diagnostics (empty/invalid chain, bad samples).
    if let Some(diag) = reify_stdlib::stackup_diagnose(name, args) {
        sink.borrow_mut().push(diag);
    }
    // Multi-load-case FEA failures (empty/unknown-case weights, incompatible meshes).
    if let Some(diag) = reify_stdlib::fea_diagnose(name, args) {
        sink.borrow_mut().push(diag);
    }
    // AffineMap-constructor warnings: `affine_scale` zero (degenerate, det=0) or
    // dimensioned scale factor (the linear part of an affine map is dimensionless).
    if let Some(diag) = reify_stdlib::geometry_diagnose(name, args) {
        sink.borrow_mut().push(diag);
    }
    // Inverse-dynamics Undef: body has no resolvable mass.
    if let Some(diag) = reify_stdlib::dynamics_diagnose(name, args) {
        sink.borrow_mut().push(diag);
    }
    // ISO 286-1 tolerancing: out-of-envelope iso_it_tolerance (grade outside
    // IT5–IT18 / nominal > 500 mm) — well-typed but unsupported, surfaced as
    // Severity::Error instead of a silent Undef. Post-Undef-only, same
    // (name,&[Value])->Option<Diagnostic> shape as stackup/fea/geometry/dynamics.
    if let Some(diag) = reify_stdlib::tolerancing_diagnose(name, args) {
        sink.borrow_mut().push(diag);
    }
}

/// Emit the task-3871 PRB-flexure diagnostics for a builtin call `result`.
///
/// Extracted from `eval_expr`'s `FunctionCall` arm — and marked
/// `#[inline(never)]` — to keep that recursive function's stack frame small:
/// the `for diag in …` loop binds an owned `Diagnostic` per iteration, and in
/// unoptimized builds that by-value local would sit on every `eval_expr` frame
/// (regardless of which match arm runs) and blow the 2 MiB test-thread stack at
/// `MAX_RECURSION_DEPTH` levels of recursive user-fn evaluation. Same rationale
/// and pinning test (`eval_user_fn_recursion_depth_exceeded`) as the
/// `eval_worst_case_dispatch` / `eval_quantifier` extractions.
///
/// Unlike the post-`Undef`-only `stackup_diagnose` / `fea_diagnose` hooks, this
/// runs on BOTH the success and `Undef` paths — `W_FlexureYielding` /
/// `W_FlexurePrbOutOfRange` fire on a SUCCESSFULLY constructed joint.
/// `flexure_diagnose` returns an empty `Vec` for any non-flexure name, so this
/// is a cheap no-op for every other builtin. The standing
/// `W_FlexureFatigueCheckMissing` Info advisory is deduped to once per eval
/// session (per sink): pushed only if the sink does not already carry that code.
#[inline(never)]
fn emit_flexure_diagnostics(name: &str, args: &[Value], result: &Value, ctx: &EvalContext) {
    let Some(sink) = ctx.diagnostics else {
        return;
    };
    for diag in reify_stdlib::flexure_diagnose(name, args, result) {
        if diag.code == Some(DiagnosticCode::FlexureFatigueCheckMissing) {
            // NOTE(perf, deferred): the once-per-session fatigue dedup re-scans
            // the whole sink for the code on every emitting flexure-ctor call —
            // O(M·N) for M ctors and an N-entry sink. Impact is negligible in
            // practice (M and N are both tiny) and `any` short-circuits on the
            // first hit. The clean O(1) fix — a session-scoped `Cell<bool>` /
            // `HashSet<DiagnosticCode>` of already-emitted once-per-session codes
            // — must outlive the per-cell `EvalContext` (a fresh one is built per
            // cell; only this `&RefCell` sink is shared across the eval session),
            // so that companion state has to be allocated and threaded by the
            // sink's owner in `crates/reify-eval/src` (outside task 3871's lock
            // scope). Left as a tracked follow-up rather than reached around.
            let already = sink
                .borrow()
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::FlexureFatigueCheckMissing));
            if already {
                continue;
            }
        }
        sink.borrow_mut().push(diag);
    }
}

/// Emit DFM (design-for-manufacturing) diagnostics for a builtin call into the
/// runtime sink (PRD v0_6 process-dfm-completion, task α).
///
/// Mirrors [`emit_flexure_diagnostics`]: a no-op when no sink is attached, else it
/// pushes every `Diagnostic` returned by [`reify_stdlib::dfm_diagnose`] into the
/// sink. Like the flexure hook — and unlike the post-`Undef`-only
/// stackup/fea/geometry hooks consolidated in `emit_undef_builtin_diagnostics` —
/// `dfm_diagnose` fires on BOTH paths: a `fits_build_volume` returning
/// `Bool(false)` is a build-volume VIOLATION surfaced on the SUCCESS path, while a
/// `Value::Undef` is a usage error. `dfm_diagnose` returns an empty `Vec` for every
/// non-DFM name, so this is a cheap no-op for other builtins. Extracted
/// `#[inline(never)]` for the same stack-shrinking reason as
/// `emit_flexure_diagnostics` — keeping the per-`diag` owned-`Diagnostic` loop
/// local off every recursive `eval_expr` frame (pinned by
/// `eval_user_fn_recursion_depth_exceeded`).
#[inline(never)]
fn emit_dfm_diagnostics(name: &str, args: &[Value], result: &Value, ctx: &EvalContext) {
    let Some(sink) = ctx.diagnostics else {
        return;
    };
    for diag in reify_stdlib::dfm_diagnose(name, args, result) {
        sink.borrow_mut().push(diag);
    }
}

/// Emit snapshot `center_of_mass` fallback diagnostics for a builtin call into
/// the runtime sink (task 4471).
///
/// Mirrors [`emit_dfm_diagnostics`]: a no-op when no sink is attached, else it
/// pushes every `Diagnostic` returned by [`reify_stdlib::snapshot_diagnose`]
/// into the sink. Like the flexure/DFM hooks — and unlike the post-`Undef`-only
/// stackup/fea/geometry hooks consolidated in `emit_undef_builtin_diagnostics`
/// — `snapshot_diagnose` fires on BOTH paths: the `center_of_mass` fallback
/// Warning is emitted when the result is a valid `Point` (success path, the
/// legacy density-weighted centroid). `snapshot_diagnose` returns an empty
/// `Vec` for every non-`center_of_mass` name, so this is a cheap no-op for
/// other builtins. Extracted `#[inline(never)]` for the same stack-shrinking
/// reason as `emit_flexure_diagnostics` and `emit_dfm_diagnostics`.
#[inline(never)]
fn emit_snapshot_diagnostics(name: &str, args: &[Value], result: &Value, ctx: &EvalContext) {
    let Some(sink) = ctx.diagnostics else {
        return;
    };
    for diag in reify_stdlib::snapshot_diagnose(name, args, result) {
        sink.borrow_mut().push(diag);
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

/// Resolve the effective `ElasticOptions` for a single `LoadCase`.
///
/// `LoadCase.options` is declared `Option<ElasticOptions> = none` in
/// `fea_multi_case.ri`, so at runtime:
///   - `Some(Value::Option(Some(X)))` → `X` (per-case override)
///   - `Some(Value::Option(None))` → `shared_options` (inherited default)
///   - absent / unexpected shape → `shared_options` (silent-Undef discipline;
///     per-field diagnostics are PRD task #10 scope)
///
/// Extracted from `eval_solve_load_cases` to enable direct unit testing of the
/// branching logic — the E2E smoke test with `make_simple_engine()` cannot
/// distinguish between branches because the stub solver returns the same result
/// regardless of which options are passed.
fn resolve_load_case_options(fields: &PersistentMap<String, Value>, shared_options: &Value) -> Value {
    match fields.get(&"options".to_string()) {
        Some(Value::Option(Some(per_case_opts))) => (**per_case_opts).clone(),
        _ => shared_options.clone(),
    }
}

/// Dispatch `solve_load_cases(material, length, width, height, cases, options)` —
/// iterate `cases : List<LoadCase>`, call `solve_elastic_static` per case
/// (using shared `options` or per-case override), and collect per-case
/// `ElasticResult` into a `MultiCaseResult`-shaped
/// `Value::Map { "cases" -> Map<String, ElasticResult> }`.
///
/// Extracted from `eval_expr` to keep that recursive frame small (mirrors the
/// extraction of `eval_worst_case_dispatch` and `eval_quantifier`).
///
/// Dispatch site: the `"solve_load_cases"` inline arm in `eval_expr` — requires
/// `EvalContext` to re-invoke `solve_elastic_static` per case via
/// `invoke_solve_elastic_static`.  The `eval_fea` arm for `"solve_load_cases"`
/// is a permanent `Value::Undef` stub (recognised-name contract for wrong-arity
/// callers; mirrors the `"worst_case"` dual-arm pattern).
///
/// # Cache-key invariant (step-6 / PRD task 3005)
///
/// The geometry/mesh-affecting arguments (`material`, `length`, `width`, `height`)
/// are bound ONCE from `args[]` and passed by CLONE to every per-case
/// `invoke_solve_elastic_static` call.  They are never re-evaluated or derived
/// from per-case data — ensuring that all cases sharing the same outer arguments
/// pass an identical mesh-cache key to the `@optimized` trampoline (geometry +
/// mesh params).  Only `loads` and `supports` vary per case.
///
/// The effective `ElasticOptions` per case is resolved as:
///   - `LoadCase.options == Value::Option(Some(X))` → `X` (per-case override)
///   - `Value::Option(None)` / absent → `shared_options` arg (inherited default)
///
/// This means cases that share the same body/material/options AND override their
/// ElasticOptions to the SAME effective value will produce the SAME mesh-cache key,
/// while cases that override to a DIFFERENT value (different element_order /
/// mesh_size) will produce a DIFFERENT mesh-cache key (intentional — they want a
/// different mesh).
///
/// With the current `invoke_solve_elastic_static` implementation (evaluates the
/// contract body directly, bypassing the `@optimized` ComputeNode trampoline),
/// no actual ComputeNode realization occurs — mesh-reuse verification requires
/// routing through the engine's @optimized dispatch (future work; see step-5 test).
///
/// Failure modes (PRD task #10 now diagnoses empty-cases and duplicate-names;
/// the remaining shape mismatches stay silent-Undef):
/// - `args.len() < 5`: wrong arity (guarded by dispatch arm; defensive here) → Undef
/// - `cases` arg is not `Value::List`: Undef (silent)
/// - cases list is empty: Undef + `MultiLoadEmptyCases` diagnostic (task #10)
/// - duplicate case names: Undef + `MultiLoadDuplicateCaseName` diagnostic
///   naming the offending case (task #10). An up-front uniqueness pre-pass
///   rejects the second occurrence — no longer the silent last-wins overwrite
///   it was while task #10 was deferred.
/// - any case is not `Value::StructureInstance`: Undef (silent)
/// - any `LoadCase.name` is not `Value::String`: Undef (silent; such cases are
///   skipped by the duplicate pre-pass and rejected by the main solve loop)
fn eval_solve_load_cases(args: &[Value], ctx: &EvalContext) -> Value {
    if args.len() < 5 {
        return Value::Undef;
    }
    let material = &args[0];
    let length = &args[1];
    let width = &args[2];
    let height = &args[3];
    let cases_val = &args[4];
    // Shared options: arg[5] when present (fn-param-default padding supplies it);
    // Undef for 5-arg calls (step-4 resolves effective options per-case).
    let shared_options = args.get(5).cloned().unwrap_or(Value::Undef);

    let cases = match cases_val {
        Value::List(v) => v,
        _ => return Value::Undef,
    };

    if cases.is_empty() {
        // task #10 (multi-load-case FEA): empty cases is a user error, not a
        // silent Undef. Emit MultiLoadEmptyCases into the runtime sink (when
        // present) and still return Undef.
        if let Some(sink) = ctx.diagnostics {
            sink.borrow_mut().push(
                Diagnostic::error(
                    "Multi-load case analysis requires at least one LoadCase. Use solve_elastic_static for single-case analysis.",
                )
                .with_code(DiagnosticCode::MultiLoadEmptyCases),
            );
        }
        return Value::Undef;
    }

    // task #10 (multi-load-case FEA): reject duplicate case names up front.
    // Each LoadCase in a single solve_load_cases call must have a unique name
    // so downstream linear_combine weight maps can reference cases
    // unambiguously. This pre-pass runs before any solve invocation, so the
    // diagnostic fires with zero solver work. Cases lacking a `Value::String`
    // name are skipped here and handled as before by the main loop below.
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for case_val in cases.iter() {
        if let Value::StructureInstance(data) = case_val
            && let Some(Value::String(name)) = data.fields.get(&"name".to_string())
            && !seen_names.insert(name.clone())
        {
            if let Some(sink) = ctx.diagnostics {
                sink.borrow_mut().push(
                    Diagnostic::error(format!(
                        "Duplicate load case name: '{name}'. Each LoadCase in a single solve_load_cases call must have a unique name."
                    ))
                    .with_code(DiagnosticCode::MultiLoadDuplicateCaseName),
                );
            }
            return Value::Undef;
        }
    }

    let mut out: std::collections::BTreeMap<Value, Value> = std::collections::BTreeMap::new();

    for case_val in cases.iter() {
        let data = match case_val {
            Value::StructureInstance(d) => d,
            _ => return Value::Undef,
        };

        let name = match data.fields.get(&"name".to_string()) {
            Some(Value::String(s)) => s.clone(),
            _ => return Value::Undef,
        };

        let loads = match data.fields.get(&"loads".to_string()) {
            Some(v) => v.clone(),
            None => return Value::Undef,
        };

        let supports = match data.fields.get(&"supports".to_string()) {
            Some(v) => v.clone(),
            None => return Value::Undef,
        };

        // Resolve effective options per case via the extracted helper
        // (enables direct unit testing of the branching logic).
        let effective_options = resolve_load_case_options(&data.fields, &shared_options);

        let per_case = invoke_solve_elastic_static(
            &[
                material.clone(),
                length.clone(),
                width.clone(),
                height.clone(),
                loads,
                supports,
                effective_options,
            ],
            ctx,
        );

        out.insert(Value::String(name), per_case);
    }

    let mut outer: std::collections::BTreeMap<Value, Value> = std::collections::BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(out));
    Value::Map(outer)
}

/// Invoke the `solve_elastic_static` compiled function directly with runtime
/// `Value` args (no `CompiledExpr` wrappers needed).
///
/// Finds the compiled function by name + arity in `ctx.functions`, builds a
/// fresh scope binding params to args, and evaluates the function body.
/// When the FEA compute trampoline is not registered (e.g. in unit tests using
/// `make_simple_engine()`), this evaluates the contract body `ElasticResult()`
/// and returns a `Value::StructureInstance { type_name: "ElasticResult", .. }`.
fn invoke_solve_elastic_static(args: &[Value], ctx: &EvalContext) -> Value {
    // Lookup by name + arity only (not param types), because `solve_elastic_static`
    // has a single definition at any given arity and no overloads.
    //
    // ASSUMPTION: exactly one `solve_elastic_static` function exists at arity
    // `args.len()` in `ctx.functions`. If a future overload were added at the
    // same arity, `.find()` would return whichever definition appears first —
    // a latent footgun. The type-aware `find_matching_compiled_function` can be
    // used here if/when a second arity-7 overload lands. For now, document the
    // single-definition invariant rather than pay the type-matching overhead.
    let func = match ctx
        .functions
        .iter()
        .find(|f| f.name == "solve_elastic_static" && f.params.len() == args.len())
    {
        Some(f) => f,
        None => return Value::Undef,
    };

    if ctx.recursion_depth >= MAX_RECURSION_DEPTH {
        return Value::Undef;
    }

    // Delegate scope-building and body evaluation to the shared helper (shared
    // with eval_user_function_call to keep the scope-bind/let-bind/result-eval
    // loop in a single place).
    let result = eval_compiled_function_with_values(func, args, ctx);
    if result.is_undef() {
        // The contract body `ElasticResult()` compiles as
        // `CompiledExprKind::FunctionCall` (not `StructureInstanceCtor`) because
        // `ElasticResult` is declared in the same module as `solve_elastic_static`,
        // and `phase_functions` only builds the template registry from the prelude
        // (modules *before* `solver_elastic.ri`) when compiling function bodies.
        // `ElasticResult` is absent from that prelude registry, so the
        // ctor-lowering path never fires and the call is emitted as `FunctionCall`,
        // which `eval_builtin` does not recognise → Undef.
        //
        // Synthesise the intended `Value::StructureInstance` fallback so callers in
        // non-trampoline contexts (e.g. `make_simple_engine()`) receive a non-Undef
        // placeholder — consistent with the docstring's contract-body-fallback
        // description.  The real ElasticResult (with displacement, stress, etc.) is
        // produced by the `@optimized` trampoline in engine_eval.rs; the simple
        // StructureInstance here is only the stub path.
        //
        // KNOWN LIMITATION — this fallback is broader than the contract-body case:
        // it activates on ANY Undef returned by the function body, including genuine
        // solve failures (e.g. invalid geometry causing the @optimized trampoline to
        // return Undef). Under `make_simple_engine()` that path never fires, so the
        // masking is invisible in unit tests — but with the real engine, a failed
        // solve would appear as a stub `ElasticResult` rather than `Value::Undef`.
        // Narrowing to the exact contract-body case (detect that the function body
        // is the stub) is deferred; the docstring above acknowledges the limitation.
        return Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "ElasticResult".to_string(),
            version: 0,
            fields: PersistentMap::new(),
        }));
    }
    result
}

/// Wrap a user lambda as a `Value::Field { source: Analytical, .. }`.
///
/// Implements the `fn_field` intercepting builtin (task 4220 β,
/// PRD docs/prds/v0_6/std-fields-api.md §5.2).
///
/// Marked `#[inline(never)]` to keep `eval_expr`'s stack frame small in
/// debug builds — the `domain_type` and `codomain_type` locals (each a
/// `Type`) would otherwise sit on every recursive `eval_expr` frame and
/// overflow the 2 MiB test-thread stack at `MAX_RECURSION_DEPTH` (256)
/// levels of user-fn recursion (same rationale as
/// `eval_structure_instance_ctor`; pinned by
/// `eval_user_fn_recursion_depth_exceeded`).
#[inline(never)]
fn eval_fn_field(lambda: &Value, result_type: &Type) -> Value {
    debug_assert!(
        matches!(result_type, Type::Field { .. }),
        "fn_field result_type should be Field<D,C>, stamped by \
         field_op_result_type (task 4219 α); got {:?}",
        result_type
    );
    let (domain_type, codomain_type) = if let Type::Field { domain, codomain } = result_type {
        ((**domain).clone(), (**codomain).clone())
    } else {
        (Type::dimensionless_scalar(), Type::dimensionless_scalar())
    };
    Value::Field {
        domain_type,
        codomain_type,
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda.clone()),
    }
}

/// Construct a Regular1D gridded `SampledField` from explicit sample points.
///
/// Implements the `from_samples` intercepting builtin (task 4221 γ,
/// PRD docs/prds/v0_6/std-fields-api.md §D3/D5).
///
/// # Contract (full — diagnostics added in steps 6 and 8)
///
/// - `points` and `values` must be `Value::List` of scalar elements
///   (Real/Int/Scalar) of equal length >= 2. Scalar (dimensioned) values are
///   accepted via `Value::as_f64()` (SI-unwrapped), consistent with how
///   `sampled::sample_at_point` accepts Scalar coordinates.
/// - Points must be finite (no NaN/Inf) and form a uniformly-spaced 1-D grid.
///   Non-finite or non-uniform spacing pushes
///   `DiagnosticCode::FieldSamplesNotGrid` (step-6) and returns Undef.
/// - `method` must be a `Value::Enum { type_name: "InterpolationMethod", .. }`.
///   Linear/NearestNeighbor/Cubic → `InterpolationKind`;
///   RBF/Kriging → `DiagnosticCode::InterpMethodUnsupported` (step-8), returns Undef.
///   An enum with the wrong `type_name` returns Undef silently (guarded upstream).
/// - Returns `Value::Field { source: Sampled, lambda: Arc(Value::SampledField(sf)) }`.
///
/// Marked `#[inline(never)]` for the same stack-frame rationale as `eval_fn_field`.
#[inline(never)]
fn eval_from_samples(
    points: &Value,
    values: &Value,
    method: &Value,
    result_type: &Type,
    ctx: &EvalContext,
) -> Value {
    // ── 1. Extract lists ─────────────────────────────────────────────────────
    let pts = match points {
        Value::List(v) => v,
        _ => {
            push_eval_error(
                ctx,
                "from_samples: points must form a uniformly-spaced 1-D regular grid \
                 (points argument is not a List)",
                DiagnosticCode::FieldSamplesNotGrid,
            );
            return Value::Undef;
        }
    };
    let vals = match values {
        Value::List(v) => v,
        _ => {
            push_eval_error(
                ctx,
                "from_samples: values argument is invalid (not a List)",
                DiagnosticCode::FieldSamplesNotGrid,
            );
            return Value::Undef;
        }
    };

    // ── 2. Length checks ─────────────────────────────────────────────────────
    if pts.len() != vals.len() {
        push_eval_error(
            ctx,
            "from_samples: points must form a uniformly-spaced 1-D regular grid \
             (points and values have different lengths)",
            DiagnosticCode::FieldSamplesNotGrid,
        );
        return Value::Undef;
    }
    if pts.len() < 2 {
        push_eval_error(
            ctx,
            "from_samples: points must form a uniformly-spaced 1-D regular grid \
             (at least 2 sample points are required)",
            DiagnosticCode::FieldSamplesNotGrid,
        );
        return Value::Undef;
    }

    // ── 3. Convert to f64 — Real/Int/Scalar all accepted via Value::as_f64() ─
    // Value::as_f64() is the canonical numeric extractor (reify-ir/value.rs:1141)
    // and handles Value::Scalar { si_value, .. } consistently with how
    // sampled::sample_at_point extracts coordinates (scalar_si in sampled.rs:272).
    let pt_f64: Vec<f64> = match pts.iter().map(|v| v.as_f64()).collect::<Option<Vec<_>>>() {
        Some(v) => v,
        None => {
            push_eval_error(
                ctx,
                "from_samples: points must form a uniformly-spaced 1-D regular grid \
                 (only scalar (Real/Int/Scalar) point elements are supported; \
                  N-D point types are deferred to a follow-up)",
                DiagnosticCode::FieldSamplesNotGrid,
            );
            return Value::Undef;
        }
    };
    let val_f64: Vec<f64> = match vals.iter().map(|v| v.as_f64()).collect::<Option<Vec<_>>>() {
        Some(v) => v,
        None => {
            push_eval_error(
                ctx,
                "from_samples: values argument is invalid \
                 (non-scalar value elements are not supported)",
                DiagnosticCode::FieldSamplesNotGrid,
            );
            return Value::Undef;
        }
    };

    // ── 4. Uniform spacing check ─────────────────────────────────────────────
    // Guard against NaN/Inf point values first: if any point is non-finite,
    // the spacing arithmetic below silently produces NaN (which passes both
    // `step <= 0.0` and `rel_err > 1e-6` comparisons due to NaN semantics),
    // causing a SampledField with NaN bounds/spacing rather than a clean error.
    if pt_f64.iter().any(|x| !x.is_finite()) {
        push_eval_error(
            ctx,
            "from_samples: points must form a uniformly-spaced 1-D regular grid \
             (non-finite point values (NaN/Inf) are not supported)",
            DiagnosticCode::FieldSamplesNotGrid,
        );
        return Value::Undef;
    }
    let step = pt_f64[1] - pt_f64[0];
    if step <= 0.0 {
        push_eval_error(
            ctx,
            "from_samples: points must form a uniformly-spaced 1-D regular grid \
             (points must be strictly increasing)",
            DiagnosticCode::FieldSamplesNotGrid,
        );
        return Value::Undef;
    }
    for i in 1..pt_f64.len() {
        let delta = pt_f64[i] - pt_f64[i - 1];
        let rel_err = (delta - step).abs() / step;
        if rel_err > 1e-6 {
            push_eval_error(
                ctx,
                "from_samples: points must form a uniformly-spaced 1-D regular grid \
                 (spacing between consecutive points is not uniform)",
                DiagnosticCode::FieldSamplesNotGrid,
            );
            return Value::Undef;
        }
    }

    // ── 5. Map InterpolationMethod variant → InterpolationKind ──────────────
    // The type_name guard enforces the stated contract: only
    // `Value::Enum { type_name: "InterpolationMethod", .. }` is accepted.
    // An enum with a different type_name (wrong-type argument) falls through to
    // the wildcard and returns Undef silently — upstream type-checking has
    // already been violated, so a silent Undef is appropriate (no misleading
    // InterpMethodUnsupported message for a mistyped argument).
    let interp = match method {
        Value::Enum { type_name, variant } if type_name == "InterpolationMethod" => {
            match variant.as_str() {
                "Linear" => InterpolationKind::Linear,
                "NearestNeighbor" => InterpolationKind::NearestNeighbor,
                "Cubic" => InterpolationKind::Cubic,
                other => {
                    // RBF/Kriging/unknown: E_INTERP_METHOD_UNSUPPORTED.
                    // This is a HARD error in from_samples — unlike interp::resolve_method
                    // which falls back to Linear + W_INTERPOLATION_DEFERRED for sampled{}
                    // fields. from_samples is a new surface with no back-compat obligation.
                    push_eval_error(
                        ctx,
                        &format!(
                            "from_samples: interpolation method '{}' is not supported by \
                             from_samples (supported: Linear, NearestNeighbor, Cubic)",
                            other
                        ),
                        DiagnosticCode::InterpMethodUnsupported,
                    );
                    return Value::Undef;
                }
            }
        }
        _ => return Value::Undef,
    };

    // ── 6. Build Regular1D SampledField ──────────────────────────────────────
    let p0 = pt_f64[0];
    let pn = *pt_f64.last().unwrap();
    let sf = SampledField {
        name: "from_samples".to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![p0],
        bounds_max: vec![pn],
        spacing: vec![step],
        axis_grids: vec![pt_f64],
        interpolation: interp,
        data: val_f64,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    };

    // ── 7. Read domain/codomain from result_type (Field<D,C> stamped by α) ──
    let (domain_type, codomain_type) = if let Type::Field { domain, codomain } = result_type {
        ((**domain).clone(), (**codomain).clone())
    } else {
        (Type::dimensionless_scalar(), Type::dimensionless_scalar())
    };

    Value::Field {
        domain_type,
        codomain_type,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Push a `Severity::Error` diagnostic into the eval context's diagnostics sink
/// (if a sink is attached). Used by `eval_from_samples` for B3/B4 error codes.
#[inline]
fn push_eval_error(ctx: &EvalContext, msg: &str, code: DiagnosticCode) {
    if let Some(sink) = ctx.diagnostics {
        sink.borrow_mut()
            .push(Diagnostic::error(msg).with_code(code));
    }
}

/// Push an `UndefCause::OpContractFailed` into the undef-cause sink (task 4323 γ).
///
/// Called at the two op/builtin push sites that return `Value::Undef` with ALL
/// inputs determined (genuine contract failure, not propagated undef):
///
/// 1. The `FunctionCall` arm in `eval_expr`, after `reify_stdlib::eval_builtin`
///    returns `Value::Undef` (reachable only because the strict undef-arg
///    short-circuit at lib.rs:206 already filtered out Undef args).
/// 2. `eval_binop`, after the strict undef-propagation check (both operands
///    are determined at that point; an Undef result is a genuine contract failure).
///
/// Uses an **empty placeholder span** (`SourceSpan::default()`) because
/// `CompiledExpr` carries no span (spans are lost at compile). The engine's
/// drain boundary (`record_op_contract_failures`) re-stamps the span with the
/// owning cell's `decl.span` before writing to the side-map.
///
/// When `ctx.undef_causes` is `None`, this function is a complete no-op —
/// preserving main-eval byte-identity (A1/G3 transparency).
#[inline]
fn push_op_contract_failure(ctx: &EvalContext, code: DiagnosticCode) {
    if let Some(sink) = ctx.undef_causes {
        sink.borrow_mut().push(UndefCause::OpContractFailed {
            code,
            // Placeholder span: CompiledExpr carries no span (spans are lost at
            // compile).  The engine's drain boundary (record_op_contract_failures)
            // re-stamps this with the owning cell's decl.span before writing to
            // the side-map.
            span: SourceSpan::empty(0),
        });
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
/// Sample `field` at `at`, dispatching over the stored lambda form.
///
/// This is the shared core of the `"sample"` builtin arm extracted from the
/// inline match (std.fields α, task 4219).  Calling it recursively enables
/// the Composed-list-form dispatch (`sample_field_at(f, sample_field_at(g, p))`).
///
/// | `source` + lambda                          | behaviour                                           |
/// |--------------------------------------------|-----------------------------------------------------|
/// | any + `Value::Lambda`                      | apply lambda directly (point unpacking if needed)   |
/// | `Sampled`/`Imported` + `Value::SampledField` | grid interpolation via `sampled::sample_at_point`  |
/// | `Gradient`/`Divergence`/`Curl`/`Laplacian` + inner `Value::Field` | numerical calculus helpers |
/// | `VonMises`/`PrincipalStresses`/`MaxShear` + inner `Value::Field`  | analysis wrappers          |
/// | `SafetyFactor` (any lambda)                | `analysis::sample_safety_factor_at_point`           |
/// | `Composed` + `Value::List[f, g]`           | `sample_field_at(f, sample_field_at(g, at))`        |
/// | `Restricted` + `Value::List[inner, region]`| stub → `Value::Undef` (task δ: OCCT containment)   |
fn sample_field_at(field: &Value, at: &Value, ctx: &EvalContext) -> Value {
    if let Value::Field {
        lambda,
        source,
        domain_type,
        codomain_type,
    } = field
    {
        match (lambda.as_ref(), source) {
            (Value::Lambda { .. }, _) => {
                apply_lambda_with_point_unpacking(lambda, at, ctx)
            }
            // Sampled-field dispatch (task 2341): runtime helper extracts query
            // coords, detects OOB, and dispatches to interp::interpolate_Nd.
            (Value::SampledField(sf), FieldSourceKind::Sampled) => {
                sampled::sample_at_point(sf, at, codomain_type, ctx)
            }
            // Imported-field dispatch (task 3576 PRD §80): imported fields
            // lower to a SampledField via read_vdb_file in elaborate_field
            // and are sampled identically to Sampled fields — "indistinguishable
            // from sampled at the field-machinery level".
            (Value::SampledField(sf), FieldSourceKind::Imported) => {
                sampled::sample_at_point(sf, at, codomain_type, ctx)
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
                at,
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
                at,
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
                at,
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
                at,
                domain_type,
                codomain_type,
                ctx,
            ),
            // Analysis field wrappers: sample the inner field, then apply the
            // analysis builtin pointwise.
            (
                Value::Field {
                    lambda: inner_lambda,
                    ..
                },
                FieldSourceKind::VonMises,
            ) => analysis::sample_von_mises_at_point(inner_lambda, at, codomain_type, ctx),
            (
                Value::Field {
                    lambda: inner_lambda,
                    ..
                },
                FieldSourceKind::PrincipalStresses,
            ) => analysis::sample_principal_stresses_at_point(
                inner_lambda,
                at,
                codomain_type,
                ctx,
            ),
            (
                Value::Field {
                    lambda: inner_lambda,
                    ..
                },
                FieldSourceKind::MaxShear,
            ) => analysis::sample_max_shear_at_point(inner_lambda, at, codomain_type, ctx),
            // SafetyFactor: lambda slot is List[field, yield_val],
            // not a nested Field — match on the source kind directly.
            (_, FieldSourceKind::SafetyFactor) => {
                analysis::sample_safety_factor_at_point(lambda, at, codomain_type, ctx)
            }
            // Composed list-form (std.fields α, task 4219, PRD §5.2):
            // lambda slot is Value::List[f, g] where f is the outer field and
            // g is the inner field.  sample(composed, p) == f(g(p)).
            // Convention: items[0] = f (outer), items[1] = g (inner).
            (Value::List(items), FieldSourceKind::Composed) if items.len() == 2 => {
                let intermediate = sample_field_at(&items[1], at, ctx);
                sample_field_at(&items[0], &intermediate, ctx)
            }
            // Restricted scaffold (std.fields α, task 4219, PRD §5.3 option (b)):
            // lambda slot is Value::List[inner_field, region].  Returns Undef
            // unconditionally pending the OCCT point-in-region containment hook.
            // Task δ implements contains(region, point) and changes this to:
            //   inside  → sample_field_at(inner_field, at)
            //   outside → Value::Undef
            (Value::List(items), FieldSourceKind::Restricted) if items.len() == 2 => {
                let _ = (&items[0], &items[1]); // inner_field, region — reserved for task δ
                Value::Undef
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
            field
        );
        Value::Undef
    }
}

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

/// Extract three finite numeric components from a 3-component `Point` or
/// `Vector` value. Returns `None` for any other shape or a non-finite
/// component. Mirrors `reify_eval::geometry_ops::point3_components` (which is
/// `pub(crate)` to reify-eval and so unreachable across the crate boundary).
fn datum_vec3(v: &Value) -> Option<[f64; 3]> {
    let comps = match v {
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => c,
        _ => return None,
    };
    let a = comps[0].as_f64().filter(|f| f.is_finite())?;
    let b = comps[1].as_f64().filter(|f| f.is_finite())?;
    let c = comps[2].as_f64().filter(|f| f.is_finite())?;
    Some([a, b, c])
}

/// Normalize a 3-vector to unit length. `None` if the magnitude is non-finite
/// or below the degeneracy floor (a zero/near-zero direction is not a valid
/// unit `Direction`).
fn unit3(v: [f64; 3]) -> Option<[f64; 3]> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if mag.is_finite() && mag > 1e-12 {
        Some([v[0] / mag, v[1] / mag, v[2] / mag])
    } else {
        None
    }
}

/// Project a stored direction/normal `Point`/`Vector` to a unit `Direction`,
/// or `Undef` if the vector is degenerate or malformed.
fn vec3_to_direction(v: &Value) -> Value {
    match datum_vec3(v).and_then(unit3) {
        Some([x, y, z]) => Value::Direction { x, y, z },
        None => Value::Undef,
    }
}

/// Column `axis` (0=x, 1=y, 2=z) of the rotation matrix for the unit quaternion
/// `(w, x, y, z)` — i.e. the rotated world basis vector, a unit `Direction`.
/// The quaternion is normalized first; `None` for a non-finite or zero
/// quaternion. The column formulas match `reify_stdlib::orientation`'s
/// row-major `R` (and `quat_rotate`'s active `q·v·q*` convention).
fn quat_basis_axis(w: f64, x: f64, y: f64, z: f64, axis: usize) -> Option<[f64; 3]> {
    let n = (w * w + x * x + y * y + z * z).sqrt();
    if !n.is_finite() || n < 1e-12 {
        return None;
    }
    let (w, x, y, z) = (w / n, x / n, y / n, z / n);
    let col = match axis {
        0 => [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
        ],
        1 => [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
        ],
        2 => [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
        _ => return None,
    };
    Some(col)
}

/// Project a `Frame` basis axis (`.x`/`.y`/`.z`) to a unit `Direction`.
fn frame_axis_direction(basis: &Value, axis: usize) -> Value {
    match basis {
        Value::Orientation { w, x, y, z } => match quat_basis_axis(*w, *x, *y, *z, axis) {
            Some([dx, dy, dz]) => Value::Direction {
                x: dx,
                y: dy,
                z: dz,
            },
            None => Value::Undef,
        },
        _ => Value::Undef,
    }
}

/// Project a `Frame` to its `.xy_plane`: a `Plane` at the frame origin whose
/// normal is the frame's z-axis.
fn frame_xy_plane(origin: &Value, basis: &Value) -> Value {
    match basis {
        Value::Orientation { w, x, y, z } => match quat_basis_axis(*w, *x, *y, *z, 2) {
            Some([nx, ny, nz]) => Value::Plane {
                origin: Box::new(origin.clone()),
                normal: Box::new(Value::Vector(vec![
                    Value::Real(nx),
                    Value::Real(ny),
                    Value::Real(nz),
                ])),
            },
            None => Value::Undef,
        },
        _ => Value::Undef,
    }
}

/// Evaluate a datum-projection member access (task 4382 β) on a datum receiver
/// (`Axis`/`Plane`/`Frame`/`Direction`). Returns `Some(value)` when `obj` is a
/// datum and `method` is one of that datum's projection members (the projection
/// is `Undef` if the datum's internals are degenerate/malformed); returns
/// `None` for any non-datum receiver or unrecognized member, so the regular
/// collection/tensor method dispatch proceeds unchanged.
///
/// The compiler (expr.rs `MemberAccess` datum-projection branch) only lowers
/// *valid* projections to a `MethodCall`, so the disallowed cases (e.g. the
/// ambiguous `frame.dir`, or `axis.x`) never reach eval; the `_ => None` arms
/// are defensive.
fn eval_datum_projection(obj: &Value, method: &str) -> Option<Value> {
    match obj {
        Value::Axis { origin, direction } => match method {
            "dir" => Some(vec3_to_direction(direction)),
            "origin" => Some((**origin).clone()),
            _ => None,
        },
        Value::Plane { origin, normal } => match method {
            "normal" => Some(vec3_to_direction(normal)),
            "origin" => Some((**origin).clone()),
            _ => None,
        },
        Value::Frame { origin, basis } => match method {
            "x" => Some(frame_axis_direction(basis, 0)),
            "y" => Some(frame_axis_direction(basis, 1)),
            "z" => Some(frame_axis_direction(basis, 2)),
            "origin" => Some((**origin).clone()),
            "xy_plane" => Some(frame_xy_plane(origin, basis)),
            _ => None,
        },
        Value::Direction { x, y, z } => match method {
            "x" => Some(Value::Real(*x)),
            "y" => Some(Value::Real(*y)),
            "z" => Some(Value::Real(*z)),
            _ => None,
        },
        _ => None,
    }
}

/// Evaluate a method call on a collection value, or a datum-projection member
/// access on a datum receiver (Axis/Plane/Frame/Direction → see
/// [`eval_datum_projection`]).
fn eval_method_call(
    obj: &Value,
    method: &str,
    args: &[Value],
    result_type: &Type,
    ctx: &EvalContext,
) -> Value {
    // Datum-projection member access (task 4382 β): `axis.dir`, `plane.normal`,
    // `frame.x/.y/.z`, `frame.origin`, `frame.xy_plane`, `direction.x/.y/.z`.
    // Dispatched ONLY for datum receivers, so the collection/tensor arms below
    // (including the `"x"|"y"|"z"` Tensor arm) are unaffected for everything
    // else: a non-datum receiver yields `None` here and falls through.
    if let Some(projected) = eval_datum_projection(obj, method) {
        return projected;
    }
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
                        Type::Scalar { dimension } if dimension.is_dimensionless() => Value::Real(0.0),
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
        BinOp::Implies => return eval_implies(left, right, ctx),
        _ => {}
    }

    let lv = eval_expr(left, ctx);
    let rv = eval_expr(right, ctx);

    // Strict undef propagation for arithmetic/comparison
    if lv.is_undef() || rv.is_undef() {
        return Value::Undef;
    }

    // γ (task 4323): bind the result so we can inspect it for OpContractFailed
    // before returning. Both operands are determined at this point (the strict
    // undef-propagation check above already returned early for Undef operands),
    // so any Undef result here is a genuine contract failure, NOT propagated undef.
    let result = match op {
        BinOp::Add => {
            // Point + Point is undefined: spec 3.3.1 prohibits adding two points
            if matches!(&left.result_type, Type::Point { .. })
                && matches!(&right.result_type, Type::Point { .. })
            {
                Value::Undef
            } else {
                eval_add(&lv, &rv)
            }
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
    };
    // Push OpContractFailed when the result is Undef AND a sink is attached.
    // When no sink is attached this is a no-op (A1/G3 transparency).
    if result.is_undef() {
        push_op_contract_failure(ctx, DiagnosticCode::OpContractViolation);
    }
    result
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

/// Kleene IMPLIES: `False ⇒ anything = True` (vacuous)
///
/// Mirrors the structure of [`eval_or`]:
/// - Short-circuit on type error (non-bool/non-undef left → `Value::Undef`,
///   right not evaluated).
/// - Short-circuit on vacuous absorbing element (`False` left → `Value::Bool(true)`,
///   right not evaluated; because `¬False = True` is absorbing for OR).
/// - Otherwise delegates to [`kleene::kleene_implies`].
///
/// See `docs/reify-language-spec.md` §9.2.3.
fn eval_implies(left: &CompiledExpr, right: &CompiledExpr, ctx: &EvalContext) -> Value {
    let lv = eval_expr(left, ctx);
    let lk = match kleene::KBool::try_from(&lv) {
        Ok(k) => k,
        Err(_) => return Value::Undef,
    };
    // Short-circuit on vacuous element: False ⇒ anything = True.
    if matches!(lk, kleene::KBool::False) {
        return Value::Bool(true);
    }
    let rv = eval_expr(right, ctx);
    let rk = match kleene::KBool::try_from(&rv) {
        Ok(k) => k,
        Err(_) => return Value::Undef,
    };
    kleene::kleene_implies(lk, rk).into()
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

/// Build `Complex { re, im, DIMENSIONLESS }` if `dimension` is DIMENSIONLESS, else `Undef`.
///
/// Centralises the dimensionless-guard + construction pattern shared by the six
/// Real/Int ± Complex arms in `eval_add` / `eval_sub`.  Each arm computes the
/// correct `re`/`im` formulas and delegates the guard to this helper, keeping the
/// dimension-invariant impossible to get wrong in only one arm.
#[inline]
fn guard_dimensionless_complex(re: f64, im: f64, dimension: DimensionVector) -> Value {
    if dimension != DimensionVector::DIMENSIONLESS {
        Value::Undef
    } else {
        Value::Complex {
            re,
            im,
            dimension: DimensionVector::DIMENSIONLESS,
        }
    }
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
                // Route through the value-layer chokepoint: a dimensionless sum
                // (DL + DL) collapses to Value::Real (Invariant V, task 4374/β).
                // Dimensioned sums stay Value::Scalar, byte-identical to before.
                Value::from_real_scalar(a + b, *ad)
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
        // Real/Int + Complex{DIMENSIONLESS} or Complex{DIMENSIONLESS} + Real/Int:
        // Promote the dimensionless scalar to a Complex and sum. Addition is commutative,
        // so both orderings are handled by the combined `|` pattern.
        // A dimensioned Complex (e.g. Complex{LENGTH}) → Undef (dimensionless-only, D3).
        (Value::Real(a), Value::Complex { re, im, dimension })
        | (Value::Complex { re, im, dimension }, Value::Real(a)) => {
            guard_dimensionless_complex(a + re, *im, *dimension)
        }
        (Value::Int(a), Value::Complex { re, im, dimension })
        | (Value::Complex { re, im, dimension }, Value::Int(a)) => {
            guard_dimensionless_complex(*a as f64 + re, *im, *dimension)
        }
        // Dimensionless Scalar + Real or Int: additive dimension-safety requires
        // both operands to be dimensionless — you cannot add a bare number to a
        // Length. The `is_dimensionless()` guard enforces this; DIMENSIONED scalars
        // fall through to Undef. Result is Value::Real (not Scalar{DIMENSIONLESS})
        // because the operand is already a bare number.
        // Note: eval_mul/eval_div have no such guard — scaling a dimensioned quantity
        // by a pure number is always legal and preserves the dimension.
        // Defensive post-β (task 4374): arithmetic ops no longer PRODUCE a
        // Scalar{DIMENSIONLESS} (the value layer collapses those to Value::Real
        // via from_real_scalar), so these mixed arms now fire only for
        // hand-constructed dimensionless Scalars (e.g. in tests).
        (Value::Scalar { si_value, dimension }, Value::Real(r))
        | (Value::Real(r), Value::Scalar { si_value, dimension })
            if dimension.is_dimensionless() =>
        {
            Value::Real(si_value + r)
        }
        (Value::Scalar { si_value, dimension }, Value::Int(n))
        | (Value::Int(n), Value::Scalar { si_value, dimension })
            if dimension.is_dimensionless() =>
        {
            Value::Real(si_value + *n as f64)
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
                // Route through the value-layer chokepoint: a dimensionless
                // difference (DL - DL) collapses to Value::Real (Invariant V,
                // task 4374/β). Dimensioned differences stay Value::Scalar.
                Value::from_real_scalar(a - b, *ad)
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
        // Subtraction is non-commutative, so Real/Int ± Complex needs four distinct arms.
        // Guard: Complex must be DIMENSIONLESS; otherwise → Undef (D3 policy).
        //
        // Real(a) - Complex{re,im,DIMENSIONLESS} → Complex{ re: a-re, im: -im }
        (Value::Real(a), Value::Complex { re, im, dimension }) => {
            guard_dimensionless_complex(a - re, -im, *dimension)
        }
        // Complex{re,im,DIMENSIONLESS} - Real(a) → Complex{ re: re-a, im }
        (Value::Complex { re, im, dimension }, Value::Real(a)) => {
            guard_dimensionless_complex(re - a, *im, *dimension)
        }
        // Int(a) - Complex{re,im,DIMENSIONLESS} → Complex{ re: a-re, im: -im }
        (Value::Int(a), Value::Complex { re, im, dimension }) => {
            guard_dimensionless_complex(*a as f64 - re, -im, *dimension)
        }
        // Complex{re,im,DIMENSIONLESS} - Int(a) → Complex{ re: re-a, im }
        (Value::Complex { re, im, dimension }, Value::Int(a)) => {
            guard_dimensionless_complex(re - *a as f64, *im, *dimension)
        }
        // Dimensionless Scalar - Real/Int and Real/Int - dimensionless Scalar.
        // Additive dimension-safety: both operands must be dimensionless (you
        // cannot subtract a bare number from a Length). DIMENSIONED scalars fall
        // through to Undef. Subtraction is non-commutative, so each ordering is
        // a separate arm. Note: eval_mul/eval_div scale any-dimension scalars
        // without this guard — scaling preserves dimension; addition/subtraction
        // do not.
        // Defensive post-β (task 4374): arithmetic ops no longer PRODUCE a
        // Scalar{DIMENSIONLESS}, so these mixed arms now fire only for
        // hand-constructed dimensionless Scalars (e.g. in tests).
        (Value::Scalar { si_value, dimension }, Value::Real(r))
            if dimension.is_dimensionless() =>
        {
            Value::Real(si_value - r)
        }
        (Value::Real(r), Value::Scalar { si_value, dimension })
            if dimension.is_dimensionless() =>
        {
            Value::Real(r - si_value)
        }
        (Value::Scalar { si_value, dimension }, Value::Int(n))
            if dimension.is_dimensionless() =>
        {
            Value::Real(si_value - *n as f64)
        }
        (Value::Int(n), Value::Scalar { si_value, dimension })
            if dimension.is_dimensionless() =>
        {
            Value::Real(*n as f64 - si_value)
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
        ) => Value::from_real_scalar(a * b, ad.mul(bd)),
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
        ) => Value::from_real_scalar(si_value * *n as f64, *dimension),
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
        ) => Value::from_real_scalar(si_value * r, *dimension),
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
            // Route through the value-layer chokepoint: a dimension-cancelling
            // quotient (e.g. L/L) collapses to Value::Real (Invariant V).
            Value::from_real_scalar(a / b, result_dim)
        }
        // Scalar / dimensionless
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Int(n),
        ) => Value::from_real_scalar(si_value / *n as f64, *dimension),
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Real(r),
        ) => Value::from_real_scalar(si_value / r, *dimension),
        // Bare number / Scalar: a dimensionless numerator ÷ a dimensioned scalar
        // yields the reciprocal dimension (e.g. `1.0 / 5s → Frequency`). Division
        // is non-commutative, so these reciprocal arms are distinct from the
        // (Scalar, Real/Int) scaling arms above. Post-β (task 4374) a
        // dimension-cancelling product/quotient collapses to Value::Real, so a
        // chain like AVOGADRO_CONSTANT's `6.022e23 * 1mol / 1mol / 1mol` now
        // reaches this arm at the final `/ 1mol` — its intermediate, formerly a
        // Scalar{DIMENSIONLESS}, is now Real. Routed through the chokepoint so a
        // fully-cancelling reciprocal still collapses to Real (Invariant V).
        (Value::Real(a), Value::Scalar { si_value, dimension }) => {
            Value::from_real_scalar(a / si_value, DimensionVector::DIMENSIONLESS.div(dimension))
        }
        (Value::Int(a), Value::Scalar { si_value, dimension }) => {
            let recip_dim = DimensionVector::DIMENSIONLESS.div(dimension);
            Value::from_real_scalar(*a as f64 / si_value, recip_dim)
        }
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
    // Compute the raw result, then sanitize NaN/Inf → Undef.
    //
    // Rationale: the value-level `^` operator must satisfy the same
    // "arithmetic produces Undef, not NaN/Inf" contract as the stdlib
    // pow()/sqrt() helpers (which apply sanitize_value at the function-call
    // boundary) and as eval_div/eval_mod (which return Undef on divide-by-zero).
    // Non-finite results are a footgun: Real(NaN) and Real(+Inf) can propagate
    // silently through downstream cells.
    //
    // The one existing no-sanitize exception is the Complex/Complex arm of
    // eval_div (lib.rs:2868), which is separately pinned by the test
    // `complex_div_complex_overflow_propagates_infinity`.  That exception does
    // NOT generalise to scalar/real `^`.
    //
    // Int and other finite-only arms pass through sanitize_value bit-identically,
    // so the wrapping is zero-cost for the common case.
    // (task-4106 step-6)
    let result = match (lv, rv) {
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
        // Scalar ^ Int: raise value, multiply dimension exponents.
        // Guard with i8::try_from: out-of-range exponents return Undef (defense-in-depth;
        // the compile guard from task-4106 step-2 normally rejects these first).
        // Mirrors units.rs:680-681 (`i8::try_from` pattern).
        (
            Value::Scalar {
                si_value,
                dimension,
            },
            Value::Int(n),
        ) => match i8::try_from(*n) {
            // Route through the value-layer chokepoint: a zero exponent cancels
            // the dimension (dimension.pow(0) = DIMENSIONLESS) and collapses to
            // Value::Real (Invariant V). The outer sanitize_value wrap stays.
            Ok(n_i8) => Value::from_real_scalar(si_value.powi(n_i8 as i32), dimension.pow(n_i8)),
            Err(_) => Value::Undef,
        },
        _ => Value::Undef,
    };
    sanitize::sanitize_value(result)
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
        // Scalar vs non-Scalar. A *dimensioned* Scalar (e.g. a Length) is never
        // equal to a bare number, so this is `false`. A *dimensionless* Scalar
        // is just a plain quantity, though: post-Invariant V (task 4374/β) no
        // arithmetic op produces one (producers route through from_real_scalar),
        // but eq operands also come from literals, struct/field defaults, map
        // values, and deserialized state — non-arithmetic sources that can still
        // carry a Scalar{DIMENSIONLESS}. So keep the `!dimension.is_dimensionless()`
        // guard and let such a value fall through to the as_f64 numeric
        // comparison below rather than silently flipping it to `false`.
        // (Scalar-vs-Scalar is handled by the earlier arm.)
        (Value::Scalar { dimension, .. }, _) | (_, Value::Scalar { dimension, .. })
            if !dimension.is_dimensionless() =>
        {
            Value::Bool(false)
        }
        _ => {
            // Int / Real / dimensionless Scalar numeric comparison via as_f64.
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
        // Scalar vs non-Scalar. A *dimensioned* Scalar is incomparable to a bare
        // number → Undef. A *dimensionless* Scalar is a plain quantity, though:
        // post-Invariant V (task 4374/β) no arithmetic op produces one, but cmp
        // operands also come from literals, struct/field defaults, map values,
        // and deserialized state — non-arithmetic sources that can still carry a
        // Scalar{DIMENSIONLESS}. So keep the `!dimension.is_dimensionless()`
        // guard and let such a value fall through to the as_f64 numeric
        // comparison below. (Scalar-vs-Scalar is handled by the earlier arm.)
        (Value::Scalar { dimension, .. }, _) | (_, Value::Scalar { dimension, .. })
            if !dimension.is_dimensionless() =>
        {
            Value::Undef
        }
        // Fallback: Int / Real / dimensionless Scalar numeric comparison via as_f64.
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
        let arg = lit(Value::Real(-3.0), Type::dimensionless_scalar());
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[42]),
            result_type: Type::dimensionless_scalar(),
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

    // ── task 4272 step-11: fits_build_volume DFM diagnostic via eval_expr ─────
    //
    // A `fits_build_volume(part, envelope, DFMSeverity.Warning)` builtin call
    // whose design VIOLATES the rule (part bbox extent past the envelope →
    // Bool(false)) must push a W_DFM Warning into the runtime diagnostics sink
    // when evaluated through eval_expr's FunctionCall arm. Drives the
    // emit_dfm_diagnostics wiring (step-12); mirrors the flexure diagnostic-sink
    // emission, which likewise fires on the SUCCESS (non-Undef) path.
    #[test]
    fn fits_build_volume_violation_emits_dfm_warning_into_sink() {
        // LENGTH scalar of `si` metres.
        fn len(si: f64) -> Value {
            Value::Scalar {
                si_value: si,
                dimension: DimensionVector::LENGTH,
            }
        }
        // BoundingBox from two LENGTH Point3 corners (metres).
        fn bbox(min: [f64; 3], max: [f64; 3]) -> Value {
            Value::BoundingBox {
                min: Box::new(Value::Point(vec![len(min[0]), len(min[1]), len(min[2])])),
                max: Box::new(Value::Point(vec![len(max[0]), len(max[1]), len(max[2])])),
            }
        }

        // Part X-extent 30 mm exceeds the 20 mm envelope → does not fit.
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let sev = Value::Enum {
            type_name: "DFMSeverity".into(),
            variant: "Warning".into(),
        };

        // The literal args' static Type is not consulted at runtime (eval_expr's
        // Literal arm clones the value), so Type::dimensionless_scalar() is a neutral placeholder.
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[0x4f, 0x44, 0x46, 0x4d]),
            result_type: Type::Bool,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "fits_build_volume".to_string(),
                    qualified_name: "std::fits_build_volume".to_string(),
                },
                args: vec![
                    lit(part, Type::dimensionless_scalar()),
                    lit(env, Type::dimensionless_scalar()),
                    lit(sev, Type::dimensionless_scalar()),
                ],
            },
        };

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let result = eval_expr(&expr, &ctx);
        assert_eq!(result, Value::Bool(false), "part does not fit the envelope");

        let diags = sink.borrow();
        assert!(
            diags
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("W_DFM")),
            "expected a W_DFM Warning in the runtime sink, got {diags:?}"
        );
    }

    // ── amend: fits_build_volume DFM diagnostics — fitting + usage-error paths ─
    //
    // The step-11 test above covers only the Bool(false) VIOLATION path through
    // eval_expr. These two mirror it for the other two outcomes so the
    // emit_dfm_diagnostics wiring itself (not just dfm.rs's unit-level diagnose) is
    // exercised end-to-end: a regression dropping the Undef branch or always pushing
    // a diagnostic would be caught here.

    /// LENGTH scalar of `si` metres (shared shape with the step-11 test helpers).
    fn dfm_len(si: f64) -> Value {
        Value::Scalar {
            si_value: si,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// BoundingBox from two LENGTH Point3 corners (metres).
    fn dfm_bbox(min: [f64; 3], max: [f64; 3]) -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                dfm_len(min[0]),
                dfm_len(min[1]),
                dfm_len(min[2]),
            ])),
            max: Box::new(Value::Point(vec![
                dfm_len(max[0]),
                dfm_len(max[1]),
                dfm_len(max[2]),
            ])),
        }
    }

    /// Build a `fits_build_volume(...)` FunctionCall expr over the given args.
    fn dfm_call_expr(args: Vec<Value>) -> CompiledExpr {
        CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[0x44, 0x46, 0x4d, 0x32]),
            result_type: Type::Bool,
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "fits_build_volume".to_string(),
                    qualified_name: "std::fits_build_volume".to_string(),
                },
                // Literal args' static Type is not consulted at runtime; Type::dimensionless_scalar()
                // is a neutral placeholder (matches the step-11 test).
                args: args.into_iter().map(|v| lit(v, Type::dimensionless_scalar())).collect(),
            },
        }
    }

    #[test]
    fn fits_build_volume_fitting_emits_no_dfm_diagnostic_into_sink() {
        // A fitting design (Bool(true)) is NOT a violation → the sink stays empty.
        let part = dfm_bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = dfm_bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let sev = Value::Enum {
            type_name: "DFMSeverity".into(),
            variant: "Warning".into(),
        };
        let expr = dfm_call_expr(vec![part, env, sev]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let result = eval_expr(&expr, &ctx);
        assert_eq!(result, Value::Bool(true), "part fits the envelope");
        assert!(
            sink.borrow().is_empty(),
            "a fitting design emits no diagnostic, got {:?}",
            sink.borrow()
        );
    }

    #[test]
    fn fits_build_volume_usage_error_emits_dfm_error_into_sink() {
        // A non-BoundingBox part (a raw Real) makes fits_build_volume return Undef;
        // emit_dfm_diagnostics must push exactly one Error E_DFM usage diagnostic.
        let env = dfm_bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let expr = dfm_call_expr(vec![Value::Real(1.0), env]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let result = eval_expr(&expr, &ctx);
        assert_eq!(result, Value::Undef, "a non-bbox arg yields Undef");

        let diags = sink.borrow();
        assert_eq!(diags.len(), 1, "exactly one usage-error diagnostic, got {diags:?}");
        assert_eq!(diags[0].severity, reify_core::Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM"),
            "usage error carries the E_DFM prefix: {}",
            diags[0].message
        );
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
            result_type: Type::dimensionless_scalar(),
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
        let arg = lit(Value::Real(1.0), Type::dimensionless_scalar());
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[44]),
            result_type: Type::dimensionless_scalar(),
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
        let arg = lit(Value::Undef, Type::dimensionless_scalar());
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[45]),
            result_type: Type::dimensionless_scalar(),
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
            result_type: Type::dimensionless_scalar(),
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
        // 80mm / 20mm = 4.0. LENGTH/LENGTH cancels to DIMENSIONLESS, so per
        // Invariant V (task 4374/β) eval_div routes through the value-layer
        // chokepoint and the result is the canonical Value::Real(4.0), NOT
        // Value::Scalar{DIMENSIONLESS}. (Was pinned to Scalar{dimensionless}
        // before β closed the leak.)
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(20.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match &result {
            Value::Real(v) => assert!((v - 4.0).abs() < 1e-12, "expected ~4.0, got {v}"),
            other => panic!("expected Value::Real(4.0), got {:?}", other),
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
        sct_with_lets(name, version, ordered, defaults, vec![])
    }

    /// Like `sct` but also accepts template `Let` cells for step-5 / step-6.
    fn sct_with_lets(
        name: &str,
        version: u32,
        ordered: Vec<(&str, CompiledExpr)>,
        defaults: Vec<(&str, CompiledExpr)>,
        lets: Vec<(&str, CompiledExpr)>,
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
            lets.into_iter()
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
                ("poisson", lit(Value::Real(0.3), Type::dimensionless_scalar())),
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
                ("mystery", vref("Nowhere", "missing", Type::dimensionless_scalar())),
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

    // ── task-4342 step-5: StructureInstanceCtor let materialization (RED) ──────
    //
    // `eval_expr` on a `StructureInstanceCtor` carrying `lets` must eagerly
    // materialize derived members into `fields`.  RED until step-6 implements
    // `materialize_template_lets` (currently a stub that does nothing).

    /// (a) single `let sum = a + b` over two Real params; (e) fields.len()
    /// counts params + materialized lets.
    ///
    /// RED: stub does nothing — fields["sum"] is absent (None) and
    /// fields.len() == 2 (params only), not 3.
    #[test]
    fn ctor_let_single_sum_materializes() {
        // let sum = a + b
        let sum_let = CompiledExpr::binop(
            BinOp::Add,
            vref("S", "a", Type::dimensionless_scalar()),
            vref("S", "b", Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );
        let expr = sct_with_lets(
            "S",
            1,
            vec![
                ("a", lit(Value::Real(3.0), Type::dimensionless_scalar())),
                ("b", lit(Value::Real(5.0), Type::dimensionless_scalar())),
            ],
            vec![],
            vec![("sum", sum_let)],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                let sum = data.fields.get(&"sum".to_string());
                assert_eq!(
                    sum,
                    Some(&Value::Real(8.0)),
                    "let sum = a + b must materialize to Real(8.0); got {:?}",
                    sum
                );
                // (e) fields.len() must count params + materialized lets
                assert_eq!(
                    data.fields.len(),
                    3,
                    "fields must include both params and materialized lets (len 3); got {}",
                    data.fields.len()
                );
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    /// (b) a let referencing an EARLIER let resolves (declaration-order dependency).
    ///
    /// RED: stub; neither derived let is materialized → fields["double"] and
    /// fields["quad"] are absent.
    #[test]
    fn ctor_let_chain_declaration_order() {
        // param a = 2.0
        // let double = a + a      → 4.0
        // let quad   = double + double → 8.0
        let double_let = CompiledExpr::binop(
            BinOp::Add,
            vref("S", "a", Type::dimensionless_scalar()),
            vref("S", "a", Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );
        let quad_let = CompiledExpr::binop(
            BinOp::Add,
            vref("S", "double", Type::dimensionless_scalar()),
            vref("S", "double", Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );
        let expr = sct_with_lets(
            "S",
            1,
            vec![("a", lit(Value::Real(2.0), Type::dimensionless_scalar()))],
            vec![],
            vec![("double", double_let), ("quad", quad_let)],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.fields.get(&"double".to_string()),
                    Some(&Value::Real(4.0)),
                    "let double = a + a must be Real(4.0)"
                );
                assert_eq!(
                    data.fields.get(&"quad".to_string()),
                    Some(&Value::Real(8.0)),
                    "let quad = double + double must be Real(8.0) (reads earlier let)"
                );
                assert_eq!(data.fields.len(), 3, "1 param + 2 lets = 3 fields total");
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    /// (c) a let reading a NESTED struct member resolves — nesting composition.
    ///
    /// Inner ctor S_inner { param x = 3.0; let derived = x + x } → derived = 6.0.
    /// Outer ctor reads inner_s.derived via IndexAccess.
    ///
    /// RED: inner lets not materialized → inner.derived absent → outer let reads Undef.
    #[test]
    fn ctor_let_reads_nested_struct_member() {
        // inner struct
        let inner_derived_let = CompiledExpr::binop(
            BinOp::Add,
            vref("S_inner", "x", Type::dimensionless_scalar()),
            vref("S_inner", "x", Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );
        let inner_ctor = sct_with_lets(
            "S_inner",
            1,
            vec![("x", lit(Value::Real(3.0), Type::dimensionless_scalar()))],
            vec![],
            vec![("derived", inner_derived_let)],
        );

        // outer struct: let outer_d = inner_s.derived
        let inner_s_ref = vref("S_outer", "inner_s", Type::StructureRef("S_inner".to_string()));
        let derived_key = CompiledExpr::literal(
            Value::String("derived".to_string()),
            Type::String,
        );
        let outer_let = CompiledExpr::index_access(inner_s_ref, derived_key, Type::dimensionless_scalar());
        let expr = sct_with_lets(
            "S_outer",
            1,
            vec![("inner_s", inner_ctor)],
            vec![],
            vec![("outer_d", outer_let)],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.fields.get(&"outer_d".to_string()),
                    Some(&Value::Real(6.0)),
                    "let outer_d = inner_s.derived must be Real(6.0) (inner derived = x+x = 6.0); \
                     got {:?}",
                    data.fields.get(&"outer_d".to_string())
                );
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    /// (d) an Undef param yields an Undef derived member kept IN ITS SLOT —
    /// no short-circuit of the whole structure.
    ///
    /// RED: stub; the let slot is absent entirely (not Undef-in-slot).
    #[test]
    fn ctor_let_undef_param_yields_undef_in_slot() {
        // param a = Undef (unbound vref), param b = 5.0
        // let sum = a + b → Undef (Undef propagation)
        let sum_let = CompiledExpr::binop(
            BinOp::Add,
            vref("S", "a", Type::dimensionless_scalar()),
            vref("S", "b", Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );
        let expr = sct_with_lets(
            "S",
            1,
            vec![
                ("a", vref("nowhere", "missing", Type::dimensionless_scalar())), // unbound → Undef
                ("b", lit(Value::Real(5.0), Type::dimensionless_scalar())),
            ],
            vec![],
            vec![("sum", sum_let)],
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::StructureInstance(data) => {
                // The whole struct must still be constructed (no Undef short-circuit).
                assert!(
                    data.fields.contains_key(&"sum".to_string()),
                    "let sum slot must exist even when derived value is Undef (no short-circuit)"
                );
                assert_eq!(
                    data.fields.get(&"sum".to_string()),
                    Some(&Value::Undef),
                    "let sum = a + b must be Undef when a is Undef"
                );
            }
            other => panic!("expected StructureInstance (not Undef short-circuit), got {:?}", other),
        }
    }

    // ── User function evaluation tests ──────────────────────────────────

    use reify_core::ContentHash;
    use reify_ir::{CompiledFnBody, CompiledFunction};

    fn make_double_fn() -> CompiledFunction {
        // fn double(x: Real) -> Real { x + x }
        let params = vec![("x".to_string(), Type::dimensionless_scalar())];
        CompiledFunction {
            name: "double".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Add,
                    vref("double", "x", Type::dimensionless_scalar()),
                    vref("double", "x", Type::dimensionless_scalar()),
                    Type::dimensionless_scalar(),
                ),
            },
            content_hash: ContentHash::of(b"double"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        }
    }

    fn make_fn_with_let() -> CompiledFunction {
        // fn f(x: Real) -> Real { let y = x + 1; y * 2 }
        let params = vec![("x".to_string(), Type::dimensionless_scalar())];
        CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![(
                    "y".to_string(),
                    CompiledExpr::binop(
                        BinOp::Add,
                        vref("f", "x", Type::dimensionless_scalar()),
                        lit(Value::Int(1), Type::Int),
                        Type::dimensionless_scalar(),
                    ),
                )],
                result_expr: CompiledExpr::binop(
                    BinOp::Mul,
                    vref("f", "y", Type::dimensionless_scalar()),
                    lit(Value::Int(2), Type::Int),
                    Type::dimensionless_scalar(),
                ),
            },
            content_hash: ContentHash::of(b"f_with_let"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        }
    }

    #[test]
    fn eval_user_fn_double() {
        let double_fn = make_double_fn();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_double"),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "double".to_string(),
                args: vec![lit(Value::Real(5.0), Type::dimensionless_scalar())],
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

    #[test]
    fn find_matching_resolves_generic_field_param_call() {
        // Parity guard (esc-4231-126): a generic candidate whose param embeds a
        // type-param inside a NON-collection constructor (`Field<T, Real>`) must
        // be selected by the eval-side resolver for a concrete `Field<Real, Real>`
        // arg, mirroring compile-time `resolve_function_overload`. Before the
        // local `type_carries_type_param` mirror was widened it only covered the
        // `Option`/`List`/`Set`/`Map` wrappers, so `Field<T, _>` fell through to
        // the old `_ => false` arm → the generic param was NOT a wildcard → the
        // call resolved at compile time but evaled to `Undef`. This locks the two
        // copies (compiler-side + eval-side) in parity for the `Field` walk.
        let params = vec![(
            "x".to_string(),
            Type::Field {
                domain: Box::new(Type::TypeParam("T".to_string())),
                codomain: Box::new(Type::dimensionless_scalar()),
            },
        )];
        let generic_fn = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: lit(Value::Real(1.0), Type::dimensionless_scalar()),
            },
            content_hash: ContentHash::of(b"generic_field_f"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![reify_ir::TypeParam {
                name: "T".to_string(),
                bounds: vec![],
                default: None,
            }],
        };
        // Arg's result_type is the concrete Field<Real, Real>; the Value payload
        // is irrelevant to overload resolution (which keys on result_type).
        let concrete_field = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        let args = vec![lit(Value::Undef, concrete_field)];
        let fns = [generic_fn];
        assert!(
            find_matching_compiled_function(&fns, "f", &args).is_some(),
            "generic fn with a Field<T, Real> param should resolve for a Field<Real, Real> arg"
        );
    }

    #[test]
    fn find_matching_resolves_trait_object_param_for_non_generic_fn() {
        // Parity guard (esc-4093-152): a NON-generic candidate whose param is a
        // trait object inside a `List` (`List<Load>`) must be selected by the
        // eval-side resolver for a concrete `List<StructureRef("PointLoad")>` arg,
        // mirroring compile-time `resolve_function_overload` which treats
        // trait-carrying params as wildcards for EVERY candidate (not just generic
        // ones). Before the local `type_carries_trait_object` mirror was added, the
        // wildcard pass was gated on `!type_params.is_empty()`, so the FEA
        // `solve_elastic_static(loads: List<Load>, supports: List<Support>)`
        // signature resolved at compile time but the eval-side resolver returned
        // None → the `@optimized` ComputeNode dispatch never fired ("found
        // targets: []").
        let params = vec![(
            "loads".to_string(),
            Type::List(Box::new(Type::TraitObject("Load".to_string()))),
        )];
        let non_generic_fn = CompiledFunction {
            name: "solve".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: lit(Value::Real(1.0), Type::dimensionless_scalar()),
            },
            content_hash: ContentHash::of(b"non_generic_trait_obj_solve"),
            annotations: vec![],
            optimized_target: None,
            // NON-generic: empty type_params is the crux of this regression.
            type_params: vec![],
        };
        // Arg's result_type is the concrete List<StructureRef("PointLoad")>; the
        // Value payload is irrelevant to overload resolution (keys on result_type).
        let concrete_loads = Type::List(Box::new(Type::StructureRef("PointLoad".to_string())));
        let args = vec![lit(Value::Undef, concrete_loads)];
        let fns = [non_generic_fn];
        assert!(
            find_matching_compiled_function(&fns, "solve", &args).is_some(),
            "non-generic fn with a List<Load> trait-object param should resolve \
             for a List<StructureRef(\"PointLoad\")> arg"
        );
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
            type_params: vec![],
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
            type_params: vec![],
        }
    }

    #[test]
    fn eval_user_fn_with_let_bindings() {
        // fn f(x: Real) -> Real { let y = x + 1; y * 2 }
        // f(4) => y = 4 + 1 = 5; result = 5 * 2 = 10
        let f = make_fn_with_let();
        let call_expr = CompiledExpr {
            content_hash: ContentHash::of(b"call_f"),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "f".to_string(),
                args: vec![lit(Value::Real(4.0), Type::dimensionless_scalar())],
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
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "double".to_string(),
                args: vec![lit(Value::Undef, Type::dimensionless_scalar())],
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
            type_params: vec![],
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
        let params1 = vec![("x".to_string(), Type::dimensionless_scalar())];
        let process1 = CompiledFunction {
            name: "process".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params1),
            params: params1,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Mul,
                    vref("process", "x", Type::dimensionless_scalar()),
                    lit(Value::Int(2), Type::Int),
                    Type::dimensionless_scalar(),
                ),
            },
            content_hash: ContentHash::of(b"process1"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        // fn process(x: Real, y: Real) -> Real { x + y }
        let params2 = vec![("x".to_string(), Type::dimensionless_scalar()), ("y".to_string(), Type::dimensionless_scalar())];
        let process2 = CompiledFunction {
            name: "process".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params2),
            params: params2,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::binop(
                    BinOp::Add,
                    vref("process", "x", Type::dimensionless_scalar()),
                    vref("process", "y", Type::dimensionless_scalar()),
                    Type::dimensionless_scalar(),
                ),
            },
            content_hash: ContentHash::of(b"process2"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        let functions = [process1, process2];
        let values = ValueMap::new();

        // Call with 1 arg: process(3.0) → 6.0
        let call1 = CompiledExpr {
            content_hash: ContentHash::of(b"call_process1"),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "process".to_string(),
                args: vec![lit(Value::Real(3.0), Type::dimensionless_scalar())],
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
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "process".to_string(),
                args: vec![
                    lit(Value::Real(3.0), Type::dimensionless_scalar()),
                    lit(Value::Real(4.0), Type::dimensionless_scalar()),
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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
        let operand = lit(complex_val, Type::complex(Type::dimensionless_scalar()));
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::complex(Type::dimensionless_scalar()));
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

    // ── BinOp::Implies eval (task-3921) ──────────────────────────────────

    /// Pins eval_implies truth table row T⇒F = Bool(false).
    #[test]
    fn eval_implies_true_implies_false_is_false() {
        let expr = CompiledExpr::binop(
            BinOp::Implies,
            lit(Value::Bool(true), Type::Bool),
            lit(Value::Bool(false), Type::Bool),
            Type::Bool,
        );
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())),
            Value::Bool(false),
        );
    }

    /// Pins eval_implies truth table row T⇒T = Bool(true).
    #[test]
    fn eval_implies_true_implies_true_is_true() {
        let expr = CompiledExpr::binop(
            BinOp::Implies,
            lit(Value::Bool(true), Type::Bool),
            lit(Value::Bool(true), Type::Bool),
            Type::Bool,
        );
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())),
            Value::Bool(true),
        );
    }

    /// Pins eval_implies truth table row F⇒U = Bool(true) (vacuous).
    #[test]
    fn eval_implies_false_implies_undef_is_true() {
        let expr = CompiledExpr::binop(
            BinOp::Implies,
            lit(Value::Bool(false), Type::Bool),
            lit(Value::Undef, Type::Bool),
            Type::Bool,
        );
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())),
            Value::Bool(true),
        );
    }

    /// Pins eval_implies truth table row U⇒F = Undef.
    #[test]
    fn eval_implies_undef_implies_false_is_undef() {
        let expr = CompiledExpr::binop(
            BinOp::Implies,
            lit(Value::Undef, Type::Bool),
            lit(Value::Bool(false), Type::Bool),
            Type::Bool,
        );
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&ValueMap::new())),
            Value::Undef,
        );
    }

    /// Non-Bool left operand (Int literal) → Value::Undef; right NOT evaluated.
    ///
    /// Mirrors `eval_and_short_circuit_on_non_bool_left_does_not_evaluate_right`.
    #[test]
    fn eval_implies_non_bool_left_does_not_evaluate_right() {
        let expr = CompiledExpr::binop(
            BinOp::Implies,
            lit(Value::Int(5), Type::Int),
            panic_on_eval_sentinel(), // panics if evaluated
            Type::Bool,
        );
        assert!(eval_expr(&expr, &EvalContext::simple(&ValueMap::new())).is_undef());
    }

    /// False left operand short-circuits to Bool(true) without evaluating right.
    ///
    /// Vacuous truth: `¬False = True` is the absorbing element for OR; right
    /// operand must NOT be evaluated.
    #[test]
    fn eval_implies_false_left_short_circuits_does_not_evaluate_right() {
        let expr = CompiledExpr::binop(
            BinOp::Implies,
            lit(Value::Bool(false), Type::Bool),
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
        let p_ref = CompiledExpr::value_ref(p_id.clone(), Type::dimensionless_scalar());
        let body = CompiledExpr::binop(
            BinOp::Mul,
            p_ref,
            lit(Value::Real(2.0), Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );

        // The lambda value (as it would appear inside Value::Field.lambda).
        let lambda_value = Value::Lambda {
            params: vec![("p".to_string(), p_id)],
            body: Box::new(body),
            captures: ValueMap::new(),
        };

        // Build the field cell and seed the values map under __field.base.
        let field_value = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Composed,
            lambda: Arc::new(lambda_value),
        };
        let field_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "base");
        let mut values = ValueMap::new();
        values.insert(field_cell, field_value);

        // Synthesize a FunctionCall: `base(3.0)`.
        let call = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[100]),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "base".to_string(),
                    qualified_name: "field::base".to_string(),
                },
                args: vec![lit(Value::Real(3.0), Type::dimensionless_scalar())],
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
        let arg = lit(Value::Real(-3.0), Type::dimensionless_scalar());
        let call = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[101]),
            result_type: Type::dimensionless_scalar(),
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

    // ── resolve_load_case_options unit tests (task 3005 / amendment pass) ────
    //
    // These tests directly pin the options-resolution branching in
    // `resolve_load_case_options`, which cannot be observed through the E2E
    // smoke tests: with `make_simple_engine()`, `invoke_solve_elastic_static`
    // evaluates the contract body and always returns the same stub
    // `Value::StructureInstance{ElasticResult}` regardless of which
    // `ElasticOptions` were passed — so the branch `Some(Value::Option(Some(X)))`
    // is executed but its correctness is not assertable from the outside.
    //
    // Addresses: amendment review suggestion 1 (test_coverage_gap).

    /// Pins the `Some(...)` branch: when the LoadCase `options` field is
    /// `Value::Option(Some(X))`, the per-case value `X` must be returned
    /// (not `shared_options`).
    #[test]
    fn resolve_load_case_options_returns_per_case_when_some() {
        let per_case = Value::Int(42);
        let shared = Value::Int(99);
        let mut fields: PersistentMap<String, Value> = PersistentMap::new();
        fields.insert(
            "options".to_string(),
            Value::Option(Some(Box::new(per_case.clone()))),
        );
        let result = resolve_load_case_options(&fields, &shared);
        assert_eq!(
            result, per_case,
            "Value::Option(Some(X)) should return the per-case value X, not shared_options"
        );
    }

    /// Pins the `None` branch: when `options` is `Value::Option(None)`, the
    /// shared options must be returned (inherited default).
    #[test]
    fn resolve_load_case_options_returns_shared_when_none() {
        let shared = Value::Int(99);
        let mut fields: PersistentMap<String, Value> = PersistentMap::new();
        fields.insert("options".to_string(), Value::Option(None));
        let result = resolve_load_case_options(&fields, &shared);
        assert_eq!(
            result, shared,
            "Value::Option(None) should fall back to shared_options"
        );
    }

    /// Pins the absent-field branch: when the LoadCase has no `options` field,
    /// the shared options must be returned (malformed LoadCase → silent-Undef
    /// discipline: fall back to shared rather than Undef-ing the entire solve).
    #[test]
    fn resolve_load_case_options_returns_shared_when_absent() {
        let shared = Value::Int(99);
        let fields: PersistentMap<String, Value> = PersistentMap::new();
        let result = resolve_load_case_options(&fields, &shared);
        assert_eq!(
            result, shared,
            "absent options field should fall back to shared_options"
        );
    }

    /// Pins the unexpected-shape branch: when `options` holds a value that is
    /// not `Value::Option(...)`, the shared options must be returned.
    /// (silent-Undef discipline — unexpected shapes are not diagnosticated here)
    #[test]
    fn resolve_load_case_options_returns_shared_for_unexpected_shape() {
        let shared = Value::Int(99);
        let mut fields: PersistentMap<String, Value> = PersistentMap::new();
        fields.insert("options".to_string(), Value::Int(777)); // not a Value::Option
        let result = resolve_load_case_options(&fields, &shared);
        assert_eq!(
            result, shared,
            "unexpected options shape should fall back to shared_options"
        );
    }

    // ── task 3029 step-1: solve_load_cases empty-cases diagnostic (RED) ──────
    //
    // `eval_solve_load_cases` with an empty `cases` list (args[4]) must emit a
    // `MultiLoadEmptyCases` error diagnostic into the runtime sink and still
    // return `Value::Undef`. Before v0.3.x task #10 this path returned Undef
    // silently (lib.rs empty-cases guard). White-box test: calls the private
    // fn directly with a sink-bearing EvalContext — the FEA solve trampoline is
    // irrelevant because the empty-cases guard fires before the solve loop.
    #[test]
    fn solve_load_cases_empty_cases_emits_diagnostic() {
        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        // 5-element args slice: material/length/width/height are irrelevant
        // here because the empty-cases guard fires before they are used.
        let args = [
            Value::Undef,
            Value::Undef,
            Value::Undef,
            Value::Undef,
            Value::List(vec![]),
        ];

        let result = eval_solve_load_cases(&args, &ctx);

        assert_eq!(result, Value::Undef, "empty cases must return Undef");

        let diags = sink.borrow();
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one diagnostic, got {diags:?}"
        );
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::MultiLoadEmptyCases)
        );
        assert_eq!(
            diags[0].message,
            "Multi-load case analysis requires at least one LoadCase. Use solve_elastic_static for single-case analysis."
        );
    }

    /// Build a minimal `LoadCase` `Value::StructureInstance` with the given
    /// `name` and empty `loads`/`supports` lists (task 3029 multi-load tests).
    fn load_case(name: &str) -> Value {
        let mut fields: PersistentMap<String, Value> = PersistentMap::new();
        fields.insert("name".to_string(), Value::String(name.to_string()));
        fields.insert("loads".to_string(), Value::List(vec![]));
        fields.insert("supports".to_string(), Value::List(vec![]));
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "LoadCase".to_string(),
            version: 0,
            fields,
        }))
    }

    // ── task 3029 step-3: solve_load_cases duplicate-names diagnostic (RED) ──
    //
    // Two LoadCases sharing the same `name` must emit a
    // MultiLoadDuplicateCaseName error diagnostic (naming the offending case)
    // and return Value::Undef. Before task #10 duplicates were silently
    // last-wins in the output BTreeMap with no diagnostic emitted.
    #[test]
    fn solve_load_cases_duplicate_names_emits_diagnostic() {
        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let args = [
            Value::Undef,
            Value::Undef,
            Value::Undef,
            Value::Undef,
            Value::List(vec![load_case("operating"), load_case("operating")]),
        ];

        let result = eval_solve_load_cases(&args, &ctx);

        assert_eq!(result, Value::Undef, "duplicate names must return Undef");

        let diags = sink.borrow();
        assert!(
            diags.iter().any(|d| d.code
                == Some(reify_core::DiagnosticCode::MultiLoadDuplicateCaseName)
                && d.message
                    == "Duplicate load case name: 'operating'. Each LoadCase in a single solve_load_cases call must have a unique name."),
            "expected a MultiLoadDuplicateCaseName diagnostic naming 'operating', got {diags:?}"
        );
    }

    // ── BinOp::Mod eval regression guards (task-3916 / spec §9.2.1) ──────────
    //
    // These are GREEN on arrival — the eval path is already correct end-to-end.
    // They pin `7 % 3 -> Int(1)`, `7 % 0 -> Undef`, `undef % 5 -> Undef` so
    // that a future change to eval_mod regresses visibly rather than silently.
    // (Mirrors the already-green `pow_int_int_result_type_is_int` pattern in
    // value_pow_compile_tests.rs.)

    /// `7 % 3` must evaluate to `Int(1)`.
    ///
    /// GREEN on arrival: `eval_mod(Int(7), Int(3)) = Int(7 % 3) = Int(1)`.
    #[test]
    fn eval_mod_int_int_returns_remainder() {
        let expr = CompiledExpr::binop(
            BinOp::Mod,
            lit(Value::Int(7), Type::Int),
            lit(Value::Int(3), Type::Int),
            Type::Int,
        );
        let values = ValueMap::new();
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&values)),
            Value::Int(1),
            "7 % 3 should evaluate to Int(1)"
        );
    }

    /// `7 % 0` must evaluate to `Undef` (division by zero).
    ///
    /// GREEN on arrival: `eval_mod(Int(7), Int(0))` hits the `b == 0` branch
    /// in `eval_mod` and returns `Value::Undef`.
    #[test]
    fn eval_mod_int_zero_returns_undef() {
        let expr = CompiledExpr::binop(
            BinOp::Mod,
            lit(Value::Int(7), Type::Int),
            lit(Value::Int(0), Type::Int),
            Type::Int,
        );
        let values = ValueMap::new();
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&values)),
            Value::Undef,
            "7 % 0 should evaluate to Undef (divide-by-zero)"
        );
    }

    /// `undef % 5` must evaluate to `Undef` (undef propagation).
    ///
    /// GREEN on arrival: the dispatcher's leading undef short-circuit
    /// (`lv.is_undef() || rv.is_undef() → Undef`, lib.rs:2022) fires before
    /// `eval_mod` is called.
    #[test]
    fn eval_mod_undef_left_propagates_undef() {
        let expr = CompiledExpr::binop(
            BinOp::Mod,
            lit(Value::Undef, Type::Int),
            lit(Value::Int(5), Type::Int),
            Type::Int,
        );
        let values = ValueMap::new();
        assert_eq!(
            eval_expr(&expr, &EvalContext::simple(&values)),
            Value::Undef,
            "undef % 5 should propagate Undef"
        );
    }

    // ── eval_pow overflow guard (task 4106 / step-3 RED / step-4 GREEN) ─────────
    //
    // The compile guard (step-2) already rejects out-of-i8-range integer literal
    // exponents on dimensioned bases, so this path is not normally reachable via
    // `eval_source`/`parse_and_compile`.  It is a defense-in-depth guard tested
    // here by calling `eval_pow` directly from the in-crate `mod tests`
    // (`use super::*`).

    /// `eval_pow(5mm, Int(256))` must return `Value::Undef`.
    ///
    /// Without the `i8::try_from` guard the current code does
    /// `dimension.pow(256 as i8)` = `pow(0)` = DIMENSIONLESS and
    /// `0.005.powi(256)` ≈ 0.0, returning `Scalar{0.0, DIMENSIONLESS}` — a
    /// silently truncated dimension, not Undef.
    ///
    /// RED (step-3): returns Scalar with i8-truncated dimension.
    /// GREEN (step-4): `i8::try_from(256)` Err → `Value::Undef`.
    #[test]
    fn eval_pow_scalar_int_overflow_returns_undef() {
        let result = eval_pow(&mm_val(5.0), &Value::Int(256));
        assert!(
            result.is_undef(),
            "eval_pow(5mm, Int(256)) should return Undef (256 overflows i8), got {:?}",
            result
        );
    }

    /// `eval_pow(5mm, Int(-200))` must return `Value::Undef`.
    ///
    /// -200 underflows i8::MIN (-128).
    ///
    /// RED (step-3): returns Scalar with i8-truncated dimension (-200 as i8 = 56).
    /// GREEN (step-4): `i8::try_from(-200)` Err → `Value::Undef`.
    #[test]
    fn eval_pow_scalar_int_underflow_returns_undef() {
        let result = eval_pow(&mm_val(5.0), &Value::Int(-200));
        assert!(
            result.is_undef(),
            "eval_pow(5mm, Int(-200)) should return Undef (-200 underflows i8), got {:?}",
            result
        );
    }

    /// `determined(cell)` must return `false` when the cell is present in the
    /// snapshot but holds `Value::Undef` (geometry-undef param case: the eval
    /// pipeline stores `(Undef, DeterminacyState::Determined)` for cells that
    /// have a binding expression but whose value depends on geometry not yet
    /// resolved).
    ///
    /// RED: current code returns `Bool(true)` — the `Determined` arm checks only
    /// the state field and ignores the value field of the snapshot tuple.
    ///
    /// Also guards the happy path: a concrete `(Value::Real(2.5), Determined)`
    /// cell must still return `Bool(true)` both before and after the fix (guards
    /// against over-correction).
    #[test]
    fn determined_false_for_present_undef_value_cell() {
        let undef_cell = ValueCellId::new("S", "geom_param");
        let concrete_cell = ValueCellId::new("S", "concrete_param");

        // Build the determinacy snapshot:
        //   undef_cell:    (Undef, Determined)    ← geometry-undef param — NOT determined
        //   concrete_cell: (Real(2.5), Determined) ← resolved param — IS determined
        let mut det_map: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        det_map.insert(
            undef_cell.clone(),
            (Value::Undef, DeterminacyState::Determined),
        );
        det_map.insert(
            concrete_cell.clone(),
            (Value::Real(2.5), DeterminacyState::Determined),
        );

        let values = ValueMap::new();
        let ctx = EvalContext::new(&values, &[]).with_determinacy(&det_map);

        // RED: present-but-Undef cell must NOT count as determined.
        let undef_pred = CompiledExpr::determinacy_predicate(
            DeterminacyPredicateKind::Determined,
            undef_cell.clone(),
        );
        let undef_result = eval_expr(&undef_pred, &ctx);
        assert_eq!(
            undef_result,
            Value::Bool(false),
            "determined(geom_param) must be false for a present-but-Undef cell (got {:?})",
            undef_result,
        );

        // Happy-path guard: concrete resolved cell must still be determined.
        let concrete_pred = CompiledExpr::determinacy_predicate(
            DeterminacyPredicateKind::Determined,
            concrete_cell.clone(),
        );
        let concrete_result = eval_expr(&concrete_pred, &ctx);
        assert_eq!(
            concrete_result,
            Value::Bool(true),
            "determined(concrete_param) must be true for a concrete resolved cell (got {:?})",
            concrete_result,
        );

        // Table-driven falsy-but-concrete guards: Bool(false) and Option(None) are
        // genuine resolved values — they must remain "determined" even though they
        // are falsy. This locks in the over-correction boundary documented in the
        // impl: Value::is_undef() is true ONLY for Value::Undef, not for any other
        // variant. A future change to is_undef() (or a new Value variant gaining
        // undef-like semantics) that breaks this would be caught here.
        let falsy_cases: &[(&str, Value)] = &[
            ("Bool(false)", Value::Bool(false)),
            ("Option(None)", Value::Option(None)),
        ];
        for (label, falsy_value) in falsy_cases {
            let cell = ValueCellId::new("S", *label);
            det_map.insert(cell.clone(), (falsy_value.clone(), DeterminacyState::Determined));
            let ctx2 = EvalContext::new(&values, &[]).with_determinacy(&det_map);
            let pred =
                CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell);
            let result = eval_expr(&pred, &ctx2);
            assert_eq!(
                result,
                Value::Bool(true),
                "determined() must be true for falsy-but-concrete value {} (got {:?})",
                label,
                result,
            );
        }
    }

    fn dimensionless_val(v: f64) -> Value {
        Value::Scalar {
            si_value: v,
            dimension: DimensionVector::DIMENSIONLESS,
        }
    }

    // ── eval_add: dimensionless Scalar mixed-arm tests (task 4319) ────────────

    #[test]
    fn dscalar_add_real_is_real() {
        let left = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let right = lit(Value::Real(4.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 29.0).abs() < 1e-12, "expected 29.0 got {v}"),
            other => panic!("expected Real(29.0), got {:?}", other),
        }
    }

    #[test]
    fn real_add_dscalar_is_real() {
        let left = lit(Value::Real(4.0), Type::dimensionless_scalar());
        let right = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 29.0).abs() < 1e-12, "expected 29.0 got {v}"),
            other => panic!("expected Real(29.0), got {:?}", other),
        }
    }

    #[test]
    fn dscalar_add_int_is_real() {
        let left = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let right = lit(Value::Int(4), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 29.0).abs() < 1e-12, "expected 29.0 got {v}"),
            other => panic!("expected Real(29.0), got {:?}", other),
        }
    }

    #[test]
    fn int_add_dscalar_is_real() {
        let left = lit(Value::Int(4), Type::Int);
        let right = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 29.0).abs() < 1e-12, "expected 29.0 got {v}"),
            other => panic!("expected Real(29.0), got {:?}", other),
        }
    }

    #[test]
    fn dimensioned_scalar_add_real_is_undef() {
        // Scalar{LENGTH} + Real must NOT match the new dimensionless arm → Undef
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(Value::Real(4.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "expected Undef for dimensioned Scalar + Real"
        );
    }

    // ── eval_sub: dimensionless Scalar mixed-arm tests (task 4319) ────────────

    #[test]
    fn dscalar_sub_real_is_real() {
        let left = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let right = lit(Value::Real(4.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 21.0).abs() < 1e-12, "expected 21.0 got {v}"),
            other => panic!("expected Real(21.0), got {:?}", other),
        }
    }

    #[test]
    fn real_sub_dscalar_is_real() {
        let left = lit(Value::Real(4.0), Type::dimensionless_scalar());
        let right = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - (-21.0)).abs() < 1e-12, "expected -21.0 got {v}"),
            other => panic!("expected Real(-21.0), got {:?}", other),
        }
    }

    #[test]
    fn dscalar_sub_int_is_real() {
        let left = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let right = lit(Value::Int(4), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - 21.0).abs() < 1e-12, "expected 21.0 got {v}"),
            other => panic!("expected Real(21.0), got {:?}", other),
        }
    }

    #[test]
    fn int_sub_dscalar_is_real() {
        let left = lit(Value::Int(4), Type::Int);
        let right = lit(dimensionless_val(25.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        match result {
            Value::Real(v) => assert!((v - (-21.0)).abs() < 1e-12, "expected -21.0 got {v}"),
            other => panic!("expected Real(-21.0), got {:?}", other),
        }
    }

    #[test]
    fn dimensioned_scalar_sub_real_is_undef() {
        // Scalar{LENGTH} - Real must NOT match the new dimensionless arm → Undef
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(Value::Real(4.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::dimensionless_scalar());
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "expected Undef for dimensioned Scalar - Real"
        );
    }

    // ─── β (task 4374): arithmetic producers route through the value-layer ───
    // chokepoint `Value::from_real_scalar`, collapsing dimension-cancelling
    // results to Value::Real (Invariant V): no arithmetic op may construct a
    // `Value::Scalar { dimension }` with `dimension.is_dimensionless()`.

    #[test]
    fn eval_div_cancelling_dims_collapse_to_real() {
        // 30mm / 10mm: LENGTH/LENGTH = DIMENSIONLESS → must be Value::Real, not
        // Value::Scalar{DIMENSIONLESS}. The VARIANT check is load-bearing; value
        // tolerance is required because 0.03/0.01 is not exactly 3.0 in f64.
        match eval_div(&mm_val(30.0), &mm_val(10.0)) {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12, "expected ~3.0, got {v}"),
            other => panic!("expected Value::Real(~3.0), got {:?}", other),
        }

        // Headline regression via compiled-expr eval of `30mm / 10mm`.
        let expr = CompiledExpr::binop(
            BinOp::Div,
            lit(mm_val(30.0), Type::length()),
            lit(mm_val(10.0), Type::length()),
            Type::dimensionless_scalar(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12, "expected ~3.0, got {v}"),
            other => panic!("expected Value::Real(~3.0) from 30mm/10mm, got {:?}", other),
        }
    }

    #[test]
    fn eval_div_noncancelling_dims_stay_scalar() {
        // AREA / LENGTH = LENGTH: a dimensioned quotient must stay Value::Scalar
        // (byte-identical to the pre-β behaviour). Guards against over-collapse.
        let area = Value::Scalar {
            si_value: 6.0,
            dimension: DimensionVector::AREA,
        };
        match eval_div(&area, &mm_val(2.0)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(
                    dimension,
                    DimensionVector::LENGTH,
                    "AREA/LENGTH should be LENGTH"
                );
                // 6.0 / 0.002 = 3000.0 (mm_val(2.0) is 0.002 m).
                assert!(
                    (si_value - 3000.0).abs() < 1e-9,
                    "expected ~3000.0, got {si_value}"
                );
            }
            other => panic!("expected Value::Scalar{{LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn eval_mul_cancelling_dims_collapse_to_real() {
        // (1/L) · L = DIMENSIONLESS → must be Value::Real, not Scalar{DL}.
        let inv_len = Value::Scalar {
            si_value: 4.0,
            dimension: DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH),
        };
        // 4.0 · 0.25 m = 1.0 (dimension cancels). VARIANT check is load-bearing.
        match eval_mul(&inv_len, &mm_val(250.0)) {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12, "expected ~1.0, got {v}"),
            other => panic!("expected Value::Real(~1.0), got {:?}", other),
        }
    }

    #[test]
    fn eval_mul_noncancelling_dims_stay_scalar() {
        // L · L = AREA: a dimensioned product must stay Value::Scalar.
        match eval_mul(&mm_val(2.0), &mm_val(3.0)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(dimension, DimensionVector::AREA, "L·L should be AREA");
                // 0.002 · 0.003 = 6e-6.
                assert!(
                    (si_value - 6e-6).abs() < 1e-12,
                    "expected ~6e-6, got {si_value}"
                );
            }
            other => panic!("expected Value::Scalar{{AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn eval_pow_zero_exponent_collapses_to_real() {
        // L^0 = DIMENSIONLESS → must be Value::Real, not Scalar{DL}.
        // (0.005^0 = 1.0 exactly; the VARIANT check is the load-bearing point.)
        match eval_pow(&mm_val(5.0), &Value::Int(0)) {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12, "expected ~1.0, got {v}"),
            other => panic!("expected Value::Real(~1.0), got {:?}", other),
        }
    }

    #[test]
    fn eval_pow_nonzero_exponent_stays_scalar() {
        // L^2 = AREA: a dimensioned power must stay Value::Scalar.
        match eval_pow(&mm_val(5.0), &Value::Int(2)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(dimension, DimensionVector::AREA, "L^2 should be AREA");
                // 0.005^2 = 2.5e-5.
                assert!(
                    (si_value - 2.5e-5).abs() < 1e-12,
                    "expected ~2.5e-5, got {si_value}"
                );
            }
            other => panic!("expected Value::Scalar{{AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn eval_add_dimensionless_scalars_collapse_to_real() {
        // DL + DL = DL → must be Value::Real, not Scalar{DL} (Invariant V).
        match eval_add(&dimensionless_val(2.0), &dimensionless_val(3.0)) {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12, "expected ~5.0, got {v}"),
            other => panic!("expected Value::Real(~5.0), got {:?}", other),
        }
    }

    #[test]
    fn eval_sub_dimensionless_scalars_collapse_to_real() {
        // DL - DL = DL → must be Value::Real, not Scalar{DL} (Invariant V).
        match eval_sub(&dimensionless_val(5.0), &dimensionless_val(2.0)) {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12, "expected ~3.0, got {v}"),
            other => panic!("expected Value::Real(~3.0), got {:?}", other),
        }
    }

    #[test]
    fn eval_add_same_dimension_scalars_stay_scalar() {
        // L + L = L: a dimensioned sum must stay Value::Scalar.
        match eval_add(&mm_val(2.0), &mm_val(3.0)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(dimension, DimensionVector::LENGTH, "L+L should be LENGTH");
                assert!(
                    (si_value - 0.005).abs() < 1e-12,
                    "expected ~0.005, got {si_value}"
                );
            }
            other => panic!("expected Value::Scalar{{LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn eval_add_length_plus_bare_real_is_undef() {
        // Additive dimension-safety: cannot add a bare number to a Length.
        assert!(
            eval_add(&mm_val(2.0), &Value::Real(3.0)).is_undef(),
            "Length + Real must be Undef"
        );
    }

    /// Invariant V (task 4374/β): no ARITHMETIC op may construct a
    /// `Value::Scalar { dimension }` with `dimension.is_dimensionless()`. This
    /// is the consolidated regression lock for the per-op routing (steps
    /// 2/4/6/8). It asserts only runtime VARIANTS — never docstrings or symbol
    /// names. Scoped to arithmetic eval_* ops; the geometry_ops producer has
    /// its own guard in reify-eval.
    #[test]
    fn arithmetic_never_produces_dimensionless_scalar() {
        fn assert_no_dimensionless_scalar(v: &Value) {
            if let Value::Scalar { dimension, .. } = v {
                assert!(
                    !dimension.is_dimensionless(),
                    "Invariant V violated: arithmetic produced a dimensionless Scalar: {:?}",
                    v
                );
            }
        }

        // 1/L operand for the dimension-cancelling product case.
        let inv_len = Value::Scalar {
            si_value: 4.0,
            dimension: DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH),
        };

        // Direct private-fn calls across the dimension-cancelling matrix.
        assert_no_dimensionless_scalar(&eval_div(&mm_val(30.0), &mm_val(10.0))); // L / L
        assert_no_dimensionless_scalar(&eval_mul(&inv_len, &mm_val(250.0))); // (1/L) · L
        assert_no_dimensionless_scalar(&eval_pow(&mm_val(5.0), &Value::Int(0))); // L ^ 0
        assert_no_dimensionless_scalar(&eval_add(
            &dimensionless_val(2.0),
            &dimensionless_val(3.0),
        )); // DL + DL
        assert_no_dimensionless_scalar(&eval_sub(
            &dimensionless_val(5.0),
            &dimensionless_val(2.0),
        )); // DL - DL

        // Headline via compiled-expr eval of `30mm / 10mm`.
        let expr = CompiledExpr::binop(
            BinOp::Div,
            lit(mm_val(30.0), Type::length()),
            lit(mm_val(10.0), Type::length()),
            Type::dimensionless_scalar(),
        );
        let values = ValueMap::new();
        assert_no_dimensionless_scalar(&eval_expr(&expr, &EvalContext::simple(&values)));
    }

    /// Characterization (task 4374/β step-10): locks comparison behaviour of
    /// dimensionless arithmetic RESULTS before the eval_eq/eval_cmp dead-guard
    /// removal (step-11). `30mm/10mm` now evaluates to Value::Real(3.0)
    /// (0.03/0.01 == 3.0 exactly in f64); the as_f64 fallback must compare it
    /// numerically. This stays GREEN across step-11 — it is the regression
    /// guard for that refactor.
    #[test]
    fn dimensionless_division_result_compares_numerically() {
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        // `30mm / 10mm` (yields Real(3.0)) compared against an Int rhs.
        let make_cmp = |op: BinOp, rhs: i64| {
            let div = CompiledExpr::binop(
                BinOp::Div,
                lit(mm_val(30.0), Type::length()),
                lit(mm_val(10.0), Type::length()),
                Type::dimensionless_scalar(),
            );
            CompiledExpr::binop(op, div, lit(Value::Int(rhs), Type::Int), Type::Bool)
        };

        assert_eq!(
            eval_expr(&make_cmp(BinOp::Eq, 3), &ctx),
            Value::Bool(true),
            "30mm/10mm == 3"
        );
        assert_eq!(
            eval_expr(&make_cmp(BinOp::Lt, 4), &ctx),
            Value::Bool(true),
            "30mm/10mm < 4"
        );
        assert_eq!(
            eval_expr(&make_cmp(BinOp::Gt, 2), &ctx),
            Value::Bool(true),
            "30mm/10mm > 2"
        );
        assert_eq!(
            eval_expr(&make_cmp(BinOp::Ne, 5), &ctx),
            Value::Bool(true),
            "30mm/10mm != 5"
        );

        // Dimensioned-incompatibility guards — must remain stable across the
        // step-11 dead-guard removal: a Length is neither equal to nor
        // comparable with a bare Real.
        assert_eq!(
            eval_eq(&mm_val(3.0), &Value::Real(3.0)),
            Value::Bool(false),
            "Length == Real must be false"
        );
        assert_eq!(
            eval_cmp(&mm_val(3.0), &Value::Real(3.0), |a, b| a < b),
            Value::Undef,
            "Length < Real must be Undef"
        );
    }

    /// Amendment (task 4374/β, reviewer suggestion 1): a *dimensionless* Scalar
    /// reaching eval_eq/eval_cmp must still compare numerically via the as_f64
    /// fallback — NOT silently become `false`/`Undef`. Invariant V keeps
    /// arithmetic producers from emitting Scalar{DIMENSIONLESS}, but
    /// non-arithmetic sources (literals, struct/field defaults, deserialized
    /// state) can, so the `!dimension.is_dimensionless()` guard on the
    /// Scalar-vs-non-Scalar arm is retained defensively. This locks that
    /// behaviour against a future re-removal of the guard.
    #[test]
    fn dimensionless_scalar_compares_numerically_in_eq_cmp() {
        // eq: a hand-built Scalar{DIMENSIONLESS} compares numerically with a
        // bare Real/Int of the same magnitude.
        assert_eq!(
            eval_eq(&dimensionless_val(3.0), &Value::Real(3.0)),
            Value::Bool(true),
            "Scalar{{DIMENSIONLESS}} == Real of equal magnitude must be true"
        );
        assert_eq!(
            eval_eq(&dimensionless_val(3.0), &Value::Int(3)),
            Value::Bool(true),
            "Scalar{{DIMENSIONLESS}} == Int of equal magnitude must be true"
        );
        assert_eq!(
            eval_eq(&dimensionless_val(3.0), &Value::Real(4.0)),
            Value::Bool(false),
            "Scalar{{DIMENSIONLESS}} == Real of differing magnitude must be false"
        );

        // cmp: ordering against a bare Real/Int flows through as_f64.
        assert_eq!(
            eval_cmp(&dimensionless_val(3.0), &Value::Real(4.0), |a, b| a < b),
            Value::Bool(true),
            "Scalar{{DIMENSIONLESS}} < Real must compare numerically"
        );
        assert_eq!(
            eval_cmp(&dimensionless_val(5.0), &Value::Int(2), |a, b| a > b),
            Value::Bool(true),
            "Scalar{{DIMENSIONLESS}} > Int must compare numerically"
        );
    }

    // ─── tolerancing Undef-diagnosis sink tests (task 4461, step-1) ──────────

    /// Build an `iso_it_tolerance(...)` FunctionCall expr over the given args.
    fn iso_it_tolerance_call_expr(args: Vec<Value>) -> CompiledExpr {
        CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[0x49, 0x49, 0x54, 0x31]),
            result_type: Type::length(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "iso_it_tolerance".to_string(),
                    qualified_name: "std::iso_it_tolerance".to_string(),
                },
                // Literal args' static Type is not consulted at runtime.
                args: args.into_iter().map(|v| lit(v, Type::dimensionless_scalar())).collect(),
            },
        }
    }

    #[test]
    fn iso_it_tolerance_in_envelope_emits_no_diagnostic_into_sink() {
        // Grade 6 with 30–50mm nominal is in-envelope → iso_it_tolerance returns
        // a finite LENGTH scalar; the sink must stay empty (pins the None-path and
        // the matches!(result, Value::Undef) gate — an unconditional emit or
        // mis-gated success path would be caught here).
        let expr = iso_it_tolerance_call_expr(vec![
            Value::Int(6),
            mm_val(30.0),
            mm_val(50.0),
        ]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let result = eval_expr(&expr, &ctx);
        match &result {
            Value::Scalar { dimension, si_value } => {
                assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "iso_it_tolerance(6,30mm,50mm) should be a LENGTH scalar"
                );
                assert!(
                    *si_value > 0.0,
                    "iso_it_tolerance(6,30mm,50mm) should be positive, got {si_value}"
                );
            }
            other => panic!(
                "iso_it_tolerance(6,30mm,50mm) should be a LENGTH scalar, got {:?}",
                other
            ),
        }
        assert!(
            sink.borrow().is_empty(),
            "in-envelope iso_it_tolerance must emit no diagnostic, got {:?}",
            sink.borrow()
        );
    }

    #[test]
    fn iso_it_tolerance_out_of_envelope_emits_tolerancing_error_into_sink() {
        // Grade 25 is outside IT5–IT18 → iso_it_tolerance returns Value::Undef.
        // tolerancing_diagnose is wired into emit_undef_builtin_diagnostics as
        // the fifth classifier arm (after dynamics_diagnose), so the sink now
        // receives exactly one Severity::Error whose message contains
        // "E_TolerancingOutOfEnvelope". GREEN: wiring is live.
        let expr = iso_it_tolerance_call_expr(vec![
            Value::Int(25),
            mm_val(30.0),  // 30mm nominal_min
            mm_val(50.0),  // 50mm nominal_max
        ]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let result = eval_expr(&expr, &ctx);
        assert_eq!(result, Value::Undef, "grade 25 is out of envelope → Undef");

        let diags = sink.borrow();
        assert_eq!(
            diags.len(),
            1,
            "exactly one E_TolerancingOutOfEnvelope diagnostic, got {diags:?}"
        );
        assert_eq!(
            diags[0].severity,
            reify_core::Severity::Error,
            "out-of-envelope iso_it_tolerance must emit Severity::Error"
        );
        assert!(
            diags[0].message.contains("E_TolerancingOutOfEnvelope"),
            "message must contain E_TolerancingOutOfEnvelope prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn iso_it_tolerance_oversize_nominal_emits_tolerancing_error_into_sink() {
        // Grade 6 is within IT5–IT18 (valid), but nmax = 700mm > 500mm is outside
        // the ISO 286-1 size envelope → iso_it_tolerance returns Value::Undef and
        // tolerancing_diagnose fires the iso_size_in_envelope branch (not the
        // grade branch).  This pins the other half of the envelope predicate at
        // the wiring layer, independently of the grade-out-of-range path exercised
        // by iso_it_tolerance_out_of_envelope_emits_tolerancing_error_into_sink.
        let expr = iso_it_tolerance_call_expr(vec![
            Value::Int(6),
            mm_val(600.0), // 600mm nominal_min — grade valid, size oversize
            mm_val(700.0), // 700mm nominal_max > 500mm → out-of-size-envelope
        ]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);

        let result = eval_expr(&expr, &ctx);
        assert_eq!(
            result,
            Value::Undef,
            "oversize nominal (700mm > 500mm) with valid grade 6 should yield Undef"
        );

        let diags = sink.borrow();
        assert_eq!(
            diags.len(),
            1,
            "exactly one E_TolerancingOutOfEnvelope diagnostic for oversize nominal, \
             got {diags:?}"
        );
        assert_eq!(
            diags[0].severity,
            reify_core::Severity::Error,
            "oversize nominal must emit Severity::Error"
        );
        assert!(
            diags[0].message.contains("E_TolerancingOutOfEnvelope"),
            "message must contain E_TolerancingOutOfEnvelope prefix: {}",
            diags[0].message
        );
    }

    // ── center_of_mass emit_snapshot_diagnostics wiring (task 4471 step-7) ──
    //
    // End-to-end tests that emit_snapshot_diagnostics pushes a
    // SnapshotCenterOfMassDensityFallback Warning into the runtime sink when
    // center_of_mass falls back to the legacy density path on a mixed snapshot.
    // RED until step-8 wires emit_snapshot_diagnostics immediately after
    // emit_dfm_diagnostics in the FunctionCall post-process.

    /// Build an axis_x unit vector (1,0,0) for test snapshots.
    fn snap_axis_x() -> Value {
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
    }

    /// Build a two-body snapshot where body 0 has `solid0` placed at world
    /// x=`x0` (metres) and body 1 has `solid1` at world x=`x1`, via
    /// prismatic-X joints.  Mirrors `make_two_body_explicit_mass_snapshot`
    /// in reify-stdlib's snapshot.rs tests but is self-contained here to
    /// avoid a test-only cross-crate dependency on private helpers.
    fn snap_two_body(solid0: Value, x0: f64, solid1: Value, x1: f64) -> Value {
        let j0 = reify_stdlib::eval_builtin(
            "prismatic",
            &[
                snap_axis_x(),
                Value::Range {
                    lower: Some(Box::new(Value::length(x0 - 1.0))),
                    upper: Some(Box::new(Value::length(x0 + 1.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );
        let j1 = reify_stdlib::eval_builtin(
            "prismatic",
            &[
                snap_axis_x(),
                Value::Range {
                    lower: Some(Box::new(Value::length(x1 - 1.0))),
                    upper: Some(Box::new(Value::length(x1 + 1.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );
        let m0 = reify_stdlib::eval_builtin("mechanism", &[]);
        let m1 = reify_stdlib::eval_builtin("body", &[m0, solid0, j0.clone()]);
        let m2 = reify_stdlib::eval_builtin("body", &[m1, solid1, j1.clone()]);
        let bind0 = reify_stdlib::eval_builtin("bind", &[j0, Value::length(x0)]);
        let bind1 = reify_stdlib::eval_builtin("bind", &[j1, Value::length(x1)]);
        reify_stdlib::eval_builtin("snapshot", &[m2, Value::List(vec![bind0, bind1])])
    }

    /// Build a `center_of_mass(snapshot)` FunctionCall CompiledExpr.
    fn com_call_expr(snapshot: Value, hash_bytes: [u8; 4]) -> CompiledExpr {
        CompiledExpr {
            content_hash: reify_core::ContentHash::of(&hash_bytes),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "center_of_mass".to_string(),
                    qualified_name: "std::center_of_mass".to_string(),
                },
                args: vec![lit(snapshot, Type::dimensionless_scalar())],
            },
        }
    }

    /// `center_of_mass` on a mixed snapshot (one explicit-mass body, one plain
    /// body) evaluated through `eval_expr` must push a
    /// `SnapshotCenterOfMassDensityFallback` Warning into the runtime sink.
    ///
    /// RED until step-8 wires `emit_snapshot_diagnostics` after
    /// `emit_dfm_diagnostics` — without that call the sink stays empty even
    /// though `reify_stdlib::snapshot_diagnose` returns the Warning.
    #[test]
    fn center_of_mass_mixed_snapshot_emits_density_fallback_warning_into_sink() {
        let pm_1 = reify_stdlib::eval_builtin(
            "point_mass",
            &[Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            }],
        );
        let plain = Value::String("plain".to_string());
        let snapshot = snap_two_body(pm_1, 0.0, plain, 4.0);
        let expr = com_call_expr(snapshot, [0xC0, 0x4A, 0x71, 0x10]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);
        let result = eval_expr(&expr, &ctx);
        assert!(
            !result.is_undef(),
            "mixed snapshot center_of_mass must return a valid Point (legacy fallback), got {:?}",
            result
        );

        let diags = sink.borrow();
        assert!(
            diags.iter().any(|d| {
                d.severity == reify_core::Severity::Warning
                    && d.code == Some(DiagnosticCode::SnapshotCenterOfMassDensityFallback)
            }),
            "expected a SnapshotCenterOfMassDensityFallback Warning in the runtime sink, \
             got {diags:?}"
        );
    }

    /// `center_of_mass` on a pure-legacy snapshot (no explicit-mass bodies)
    /// evaluated through `eval_expr` must NOT push a
    /// `SnapshotCenterOfMassDensityFallback` Warning — pure-legacy mechanisms
    /// stay silent (the mixed-case Warning fires only when >= 1 body resolves
    /// AND >= 1 body does not).
    #[test]
    fn center_of_mass_pure_legacy_snapshot_emits_no_density_fallback_warning() {
        let solid_a = Value::String("a".to_string());
        let solid_b = Value::String("b".to_string());
        let snapshot = snap_two_body(solid_a, 0.0, solid_b, 4.0);
        let expr = com_call_expr(snapshot, [0xC0, 0x4A, 0x71, 0x11]);

        let values = ValueMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);
        eval_expr(&expr, &ctx);

        let diags = sink.borrow();
        assert!(
            !diags
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::SnapshotCenterOfMassDensityFallback)),
            "pure-legacy snapshot must NOT emit SnapshotCenterOfMassDensityFallback, \
             got {diags:?}"
        );
    }

    // --- task 4323 γ: undef-cause sink tests ---
    //
    // Drives the `with_undef_cause_sink` builder, `push_op_contract_failure` helper,
    // and the two push sites (FunctionCall arm, eval_binop).
    //
    // RED: `with_undef_cause_sink` does not exist → compile fail.
    // GREEN after step-4 adds it to EvalContext.

    /// BinOp::Div with a zero Int divisor produces Undef AND, when a sink is
    /// attached via `with_undef_cause_sink`, records exactly one
    /// `UndefCause::OpContractFailed { code: OpContractViolation, .. }`.
    ///
    /// Mirrors the `div_by_zero_is_undef` test above but adds the sink assertion.
    /// Drives the eval_binop push site (after the strict undef-propagation check).
    #[test]
    fn div_by_zero_with_sink_records_op_contract_failed() {
        use reify_ir::UndefCause;

        let left = lit(Value::Int(42), Type::Int);
        let right = lit(Value::Int(0), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::Int);
        let values = ValueMap::new();
        let sink: RefCell<Vec<UndefCause>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_undef_cause_sink(&sink);
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_undef(), "div-by-zero must produce Undef");
        let causes = sink.borrow();
        assert_eq!(causes.len(), 1, "sink must contain exactly one cause, got {causes:?}");
        assert!(
            matches!(
                &causes[0],
                UndefCause::OpContractFailed {
                    code: DiagnosticCode::OpContractViolation,
                    ..
                }
            ),
            "cause must be OpContractFailed {{ OpContractViolation }}, got {:?}",
            causes[0]
        );
    }

    /// sqrt of a determined negative Real produces Undef AND the sink receives
    /// an `OpContractFailed { code: OpContractViolation, .. }`.
    ///
    /// Drives the FunctionCall arm push site (after `eval_builtin` returns Undef
    /// with a fully-determined arg list — the arg list has no Undef entries,
    /// so the strict undef-arg short-circuit did NOT fire).
    #[test]
    fn sqrt_negative_with_sink_records_op_contract_failed() {
        use reify_ir::UndefCause;

        let arg = lit(Value::Real(-1.0), Type::dimensionless_scalar());
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[0x4c, 0x21]),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "sqrt".to_string(),
                    qualified_name: "std::sqrt".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        let sink: RefCell<Vec<UndefCause>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_undef_cause_sink(&sink);
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_undef(), "sqrt(-1.0) must produce Undef");
        let causes = sink.borrow();
        assert!(
            !causes.is_empty(),
            "sink must contain at least one OpContractFailed cause, got {causes:?}"
        );
        assert!(
            causes.iter().any(|c| matches!(
                c,
                UndefCause::OpContractFailed {
                    code: DiagnosticCode::OpContractViolation,
                    ..
                }
            )),
            "causes must include OpContractFailed {{ OpContractViolation }}, got {causes:?}"
        );
    }

    /// sqrt(Undef) with a sink leaves the sink EMPTY (expr-layer BT6).
    ///
    /// The strict undef-arg short-circuit at lib.rs:206 fires BEFORE the builtin
    /// is ever called, so `reify_stdlib::eval_builtin` is never reached and
    /// no OpContractFailed can be pushed — the no-false-attribution guarantee
    /// falls out of the existing short-circuit structure.
    #[test]
    fn sqrt_undef_arg_with_sink_leaves_sink_empty() {
        use reify_ir::UndefCause;

        let arg = lit(Value::Undef, Type::dimensionless_scalar());
        let expr = CompiledExpr {
            content_hash: reify_core::ContentHash::of(&[0x4c, 0x22]),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "sqrt".to_string(),
                    qualified_name: "std::sqrt".to_string(),
                },
                args: vec![arg],
            },
        };
        let values = ValueMap::new();
        let sink: RefCell<Vec<UndefCause>> = RefCell::new(Vec::new());
        let ctx = EvalContext::simple(&values).with_undef_cause_sink(&sink);
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_undef(), "sqrt(Undef) must produce Undef");
        let causes = sink.borrow();
        assert!(
            causes.is_empty(),
            "sink must be EMPTY for Undef arg (BT6 — undef-arg short-circuit fires first), \
             got {causes:?}"
        );
    }

    /// div-by-zero with NO sink attached does not panic; result is still Undef.
    ///
    /// Pins the transparency invariant G3: absence of a sink changes nothing in
    /// the eval result — all pushes are no-ops when `undef_causes` is None.
    #[test]
    fn div_by_zero_without_sink_is_transparent() {
        let left = lit(Value::Int(10), Type::Int);
        let right = lit(Value::Int(0), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::Int);
        let values = ValueMap::new();
        // Plain EvalContext::simple — no sink.
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert!(result.is_undef(), "div-by-zero without sink must still produce Undef");
    }
}
