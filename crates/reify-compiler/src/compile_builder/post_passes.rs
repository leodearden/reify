//! Post-compilation passes that run after entities are compiled: recursion
//! detection + hash remix (step 15) and duplicate-signature / field
//! composition / purpose compilation (step 16).
//!
//! Each function takes `&mut CompilationCtx` (and `&ParsedModule` for
//! `phase_purposes`) and mutates the relevant ctx fields in place, with
//! the exception of `phase_purposes` which returns `Vec<CompiledPurpose>`
//! since purposes are not owned by `CompilationCtx`.

use std::collections::HashMap;

use reify_ast::ParsedModule;
use reify_core::{ContentHash, Diagnostic, Type};

use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::{check_field_composition_types, collect_composed_field_dependencies};
use crate::scc;
use crate::termination::check_recursive_termination;
use crate::traits::compile_purpose;
use crate::types::{CompiledField, CompiledFieldSource, CompiledPurpose, TopologyTemplate};

/// Phase-12 post-compilation: detect recursive sub-component cycles via
/// DFS on the template reference graph, verify recursive structures have
/// valid termination conditions, and remix `is_recursive` into each
/// recursive template's `content_hash`.
///
/// Without the hash remix, two templates with identical raw content but
/// different recursion status would hash identically, causing incorrect
/// incremental compilation cache hits. Non-recursive templates are
/// untouched so existing cache entries remain valid for them.
pub(crate) fn phase_recursion_detection(ctx: &mut CompilationCtx) {
    // Detect recursive sub-component cycles; tag participating templates
    // with is_recursive=true and emit a warning diagnostic per cycle.
    let cyclic_sccs = scc::detect_recursive_structures(&mut ctx.templates, &mut ctx.diagnostics);

    // Verify recursive structures have valid termination conditions.
    check_recursive_termination(&ctx.templates, &cyclic_sccs, &mut ctx.diagnostics);

    // Remix is_recursive into each recursive template's content_hash.
    let recursion_tag = ContentHash::of_str("is_recursive");
    for template in &mut ctx.templates {
        if template.is_recursive {
            template.content_hash = template.content_hash.combine(recursion_tag);
        }
    }
}

/// Register each LOCAL conformer's instance associated functions into the
/// module function table (`ctx.functions`) under the per-conformer mangled
/// symbol `instance_assoc_fn_symbol(conformer, trait, method)` (task 3941 ζ).
///
/// δ stores each conformer's resolved (override-or-default) instance assoc fn as
/// a `CompiledFunction` on `TopologyTemplate.assoc_fns`, but the evaluator only
/// resolves calls against the module function table via
/// `find_matching_compiled_function` (name + exact param-type match). The ζ
/// dispatch site (`expr.rs` `TraitMethodCall` arm) lowers
/// `obj.(Trait::method)(args)` to a `UserFunctionCall` of this same mangled
/// symbol with the receiver prepended as the bound `self` arg; without this pass
/// the symbol is absent from `ctx.functions` and the call evaluates to `Undef`.
///
/// The symbol is built by the shared `crate::expr::instance_assoc_fn_symbol`
/// helper — the single source of truth with the dispatch site (name-drift
/// guard). Override-beats-default is automatic: δ already placed the winning
/// `CompiledFunction` (explicit override or trait default) into `assoc_fns`, so
/// the registered clone routes to whichever body won. The clone keeps δ's
/// compiled shape, including its leading `self: StructureRef(conformer)` receiver
/// param that `find_matching_compiled_function` matches the dispatch receiver
/// against.
///
/// **Body re-keying:** δ compiles the body in
/// `CompilationScope::new(&fn_def.name)`, so its `self` / let references are baked
/// as `ValueCellId(<bare fn name>, member)`. The evaluator binds params and lets
/// at `ValueCellId(func.name, member)`, and this pass overwrites `func.name` with
/// the mangled `symbol` — so the body's cell entities must be remapped from the
/// bare name to `symbol` (via `CompiledExpr::remap_entity`) or `self` resolves
/// against a stale cell and the call evaluates to `Undef`. This mirrors the
/// static-fn path, which renames the AST `fn_def.name` *before* compiling so the
/// body bakes the final name from the start; instance assoc fns are compiled by δ
/// under the bare name and renamed here, so the equivalent re-keying happens
/// post-hoc on the compiled tree.
///
/// **Ordering:** must run AFTER `phase_fn_arg_conformance` — that pass already
/// walks each `template.assoc_fns` body, so registering the same body as a
/// `ctx.functions` entry beforehand would double-walk it and double-emit any
/// conformance diagnostic. It runs before `compute_module_hash`, so the
/// registered fns participate in the module content hash.
///
/// **Local-only:** mirrors the free-fn / static-trait-fn registration — a
/// prelude conformer's instance assoc fns were registered when the prelude
/// compiled and reach this module via the prelude function set, not
/// `ctx.templates` (which holds only locally-compiled templates).
pub(crate) fn phase_register_instance_assoc_fns(ctx: &mut CompilationCtx) {
    // Collect into a local first: the immutable borrow of `ctx.templates` must
    // end before the `ctx.functions` mutable borrow begins (NLL).
    let mut registered = Vec::new();
    for template in &ctx.templates {
        for af in &template.assoc_fns {
            let mut f = af.function.clone();
            let symbol =
                crate::expr::instance_assoc_fn_symbol(&template.name, &af.trait_name, &af.fn_name);

            // Re-key the body's param / let cell references to the mangled name.
            // δ's `compile_assoc_function` compiles the body in
            // `CompilationScope::new(&fn_def.name)`, so every `self` / let
            // reference is baked as `ValueCellId(<bare fn name>, member)`. The
            // evaluator binds params and let-bindings at
            // `ValueCellId(func.name, member)` (`eval_compiled_function_with_values`,
            // reify-expr), and we are about to set `func.name` to `symbol` — so
            // without this remap the body's `self` (and any let) would resolve
            // against the stale bare-name cell and evaluate to `Undef`, silently
            // poisoning the whole call (e.g. `self.diameter` → `Undef`). Remap the
            // bare entity to the mangled one so the baked references match the
            // names the evaluator will bind. The bare fn name only ever scopes
            // this function's own params/lets (a body never references another
            // entity under the bare fn name), so the rewrite is exact.
            let bare_name = f.name.clone();
            for (_, value_expr) in &mut f.body.let_bindings {
                value_expr.remap_entity(&bare_name, &symbol);
            }
            f.body.result_expr.remap_entity(&bare_name, &symbol);

            f.name = symbol;
            registered.push(f);
        }
    }
    ctx.functions.extend(registered);
}

/// Check for duplicate function signatures: same `name` + same param-type
/// sequence. Emits one `duplicate function signature: {name}({types})`
/// error diagnostic per colliding pair (after the first entry seen).
pub(crate) fn phase_dup_sig_check(ctx: &mut CompilationCtx) {
    let mut seen: HashMap<(String, Vec<Type>), usize> = HashMap::new();
    for (idx, f) in ctx.functions.iter().enumerate() {
        let key = (
            f.name.clone(),
            f.params.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
        );
        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
            e.insert(idx);
        } else {
            ctx.diagnostics.push(Diagnostic::error(format!(
                "duplicate function signature: {}({})",
                f.name,
                f.params
                    .iter()
                    .map(|(_, t)| format!("{}", t))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
}

/// Post-compilation pass: check field composition type compatibility for
/// composed fields. If a composed field's body references other fields,
/// verify that the codomain of the inner field matches the domain of the
/// outer field. Delegates to [`check_field_composition_types`].
pub(crate) fn phase_field_composition(ctx: &mut CompilationCtx) {
    let field_registry: HashMap<&str, &CompiledField> =
        ctx.fields.iter().map(|f| (f.name.as_str(), f)).collect();

    for field in &ctx.fields {
        if let CompiledFieldSource::Composed { expr } = &field.source {
            check_field_composition_types(expr, &field_registry, &mut ctx.diagnostics);
        }
    }
}

/// Post-compilation pass: for each composed field, inject the
/// `__field.<name>` cell IDs of every other field referenced inside its
/// compiled lambda body into the lambda's `captures` Vec. This surfaces
/// field-to-field dependencies through the existing
/// `Lambda { captures, .. }` arm of `collect_value_refs_inner`, so
/// `extract_dependency_trace` and the reverse-dependency index pick them
/// up without any new traversal mode.
///
/// Self-references are excluded by removing the outer field's name from
/// the registry passed to [`collect_composed_field_dependencies`] for
/// each iteration. Existing entries in `captures` (from lambda-time
/// scope analysis) are preserved; only missing field-cell deps are added.
///
/// Runs after `phase_field_composition` so the field registry shape is
/// identical and any future field-related post-pass can reuse the
/// pattern.
pub(crate) fn phase_augment_composed_captures(ctx: &mut CompilationCtx) {
    // Two-pass borrow split: a read-only pass walks each composed field's
    // body to compute the deps to inject, then a separate mutating pass
    // merges them into the lambda's captures. The split avoids holding
    // `&ctx.fields` (immutable, via the registry) and `&mut ctx.fields`
    // simultaneously.
    //
    // The registry is built once over all fields. For each composed field
    // we temporarily remove its own entry to suppress self-capture (a body
    // like `composed { |p| f3(p) }` inside f3 would otherwise add
    // `__field.f3` as a self-dep), then reinsert it after the helper
    // returns. This preserves `collect_composed_field_dependencies`'s
    // single-arg shape while keeping the registry build O(n) total.
    let mut registry: HashMap<&str, &CompiledField> =
        ctx.fields.iter().map(|f| (f.name.as_str(), f)).collect();
    let mut deps_to_add: Vec<(usize, Vec<reify_core::ValueCellId>)> = Vec::new();

    for (idx, field) in ctx.fields.iter().enumerate() {
        if let CompiledFieldSource::Composed { expr } = &field.source {
            // Suppress self-reference: pop self from the registry, run the
            // walk, then restore. The helper only consults `contains_key`,
            // so removing the self entry is sufficient.
            let saved = registry.remove(field.name.as_str());
            // The body lives inside a Lambda; walking the outer expr is fine
            // because `walk` recurses into Lambda bodies and will surface
            // FunctionCall nodes referencing fields. This matches the
            // traversal `check_field_composition_types` already performs.
            let deps = collect_composed_field_dependencies(expr, &registry);
            if let Some(s) = saved {
                registry.insert(field.name.as_str(), s);
            }
            deps_to_add.push((idx, deps));
        }
    }

    // Mutating pass: merge deps into each composed field's lambda captures.
    drop(registry);
    for (idx, new_caps) in deps_to_add {
        let field = &mut ctx.fields[idx];
        if let CompiledFieldSource::Composed { expr } = &mut field.source
            && let reify_ir::CompiledExprKind::Lambda { captures, .. } = &mut expr.kind
        {
            for cap in new_caps {
                if !captures.contains(&cap) {
                    captures.push(cap);
                }
            }
        }
    }
}

/// Purpose compilation pass. Compiles every `Declaration::Purpose` in
/// `parsed.declarations` against a phase-local template registry built
/// from `ctx.templates`, returning the accumulated `Vec<CompiledPurpose>`
/// to the orchestrator (purposes are not owned by `CompilationCtx` —
/// they flow straight into the assembled `CompiledModule`).
///
/// Runs after templates are fully populated so reflective schema queries
/// inside purpose bodies can resolve against `TopologyTemplate`s.
pub(crate) fn phase_purposes(
    ctx: &mut CompilationCtx,
    parsed: &ParsedModule,
) -> Vec<CompiledPurpose> {
    let purpose_template_registry: HashMap<String, &TopologyTemplate> = ctx
        .templates
        .iter()
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .collect();

    let mut purposes = Vec::new();
    for decl in &parsed.declarations {
        if let reify_ast::Declaration::Purpose(purpose_def) = decl {
            let compiled = compile_purpose(
                purpose_def,
                &ctx.resolution_enums,
                &ctx.resolution_functions,
                &purpose_template_registry,
                &ctx.unit_registry,
                &mut ctx.diagnostics,
            );
            purposes.push(compiled);
        }
    }
    purposes
}

#[cfg(test)]
mod inert_objective_tests {
    //! Unit tests for the pure `inert_objective_finding` predicate — the
    //! compile-half of DIC γ (task #5417, PRD
    //! `docs/prds/v0_6/declared-intent-consumption-accounting.md` §3 decision 3).
    //!
    //! The predicate answers one question: *does this template's declared
    //! objective provably govern nothing?* It must fire **only on a positive
    //! proof** and bail conservatively on every ambiguity — a false
    //! `E_OBJECTIVE_INERT` is a compile Error on legal code, so the whole
    //! design is skewed toward silence (PRD §3 decision 5, the
    //! never-a-false-Inert rule).
    //!
    //! Templates are hand-built literals, the in-crate idiom already used by
    //! `containment_graph.rs`'s `minimal_template` and `types.rs`'s
    //! `monomorph_collision_tests` — this keeps the predicate testable in
    //! isolation from the parser and the compile pipeline.
    //!
    //! Written RED in step-3: `inert_objective_finding` and
    //! `InertObjectiveFinding` are introduced in step-4.
    use super::{TopologyTemplate, inert_objective_finding};
    use crate::types::{
        CompiledGuardedGroup, EntityKind, GuardState, SubComponentDecl, ValueCellDecl,
        ValueCellKind, Visibility,
    };
    use reify_ast::QuantifierKind;
    use reify_core::{ContentHash, SourceSpan, Type, ValueCellId};
    use reify_ir::{
        BinOp, CompiledExpr, CompiledExprKind, ObjectiveSense, ObjectiveSet, Value,
    };
    use std::collections::{HashMap, HashSet};

    // ── builders ────────────────────────────────────────────────────────────

    fn tmpl(name: &str) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: vec![],
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// Deterministic, always **non-empty** span keyed off the member name, so
    /// `anchor_span` assertions can identify which declaration was anchored.
    fn span_for(member: &str) -> SourceSpan {
        let start = 100 + (member.as_bytes().first().copied().unwrap_or(b'?') as u32);
        SourceSpan::new(start, start + member.len() as u32)
    }

    fn cell(
        entity: &str,
        member: &str,
        kind: ValueCellKind,
        default_expr: Option<CompiledExpr>,
    ) -> ValueCellDecl {
        ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind,
            visibility: Visibility::Public,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr,
            solver_hints: vec![],
            span: span_for(member),
        }
    }

    fn param(entity: &str, member: &str, default: f64) -> ValueCellDecl {
        cell(entity, member, ValueCellKind::Param, Some(lit(default)))
    }

    fn auto(entity: &str, member: &str) -> ValueCellDecl {
        cell(entity, member, ValueCellKind::Auto { free: true }, None)
    }

    fn let_cell(entity: &str, member: &str, body: CompiledExpr) -> ValueCellDecl {
        cell(entity, member, ValueCellKind::Let, Some(body))
    }

    fn lit(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar())
    }

    fn vref(entity: &str, member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(
            ValueCellId::new(entity, member),
            Type::dimensionless_scalar(),
        )
    }

    fn mul(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(BinOp::Mul, l, r, Type::dimensionless_scalar())
    }

    /// Wrap a raw `CompiledExprKind` with a dimensionless result type. Used for
    /// the opaque-node shapes that have no public constructor.
    fn raw(kind: CompiledExprKind) -> CompiledExpr {
        CompiledExpr {
            kind,
            result_type: Type::dimensionless_scalar(),
            content_hash: ContentHash(0),
        }
    }

    fn minimize(expr: CompiledExpr) -> ObjectiveSet {
        ObjectiveSet::single(ObjectiveSense::Minimize, expr)
    }

    /// An empty guarded group whose `members` / `else_members` the caller fills.
    fn guarded_group(entity: &str) -> CompiledGuardedGroup {
        CompiledGuardedGroup {
            guard_expr: lit(1.0),
            guard_value_cell: ValueCellId::new(entity, "__guard_0"),
            members: vec![],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
            parent_guard: None,
        }
    }

    fn sub_decl(name: &str, structure_name: &str) -> SubComponentDecl {
        SubComponentDecl {
            name: name.to_string(),
            structure_name: structure_name.to_string(),
            visibility: Visibility::Public,
            args: vec![],
            type_args: vec![],
            is_collection: false,
            keyed_members: Vec::new(),
            keyed_member_overrides: Vec::new(),
            count_cell: None,
            guard_state: GuardState::None,
            pose: None,
            auto_pose: None,
            is_aux: false,
            span: SourceSpan::new(0, 1),
            content_hash: ContentHash(0),
        }
    }

    /// The `dic_min_no_autos.ri` shape: `param k : Real = 3.0` + `minimize k * k`,
    /// no autos anywhere. Returned unwired so each case can perturb one axis.
    fn no_autos_template() -> TopologyTemplate {
        let mut t = tmpl("DicMinNoAutos");
        t.value_cells = vec![param("DicMinNoAutos", "k", 3.0)];
        t.objective = Some(minimize(mul(
            vref("DicMinNoAutos", "k"),
            vref("DicMinNoAutos", "k"),
        )));
        t
    }

    // ── POSITIVE: the two target fixtures ───────────────────────────────────

    /// `docs/prds/v0_6/fixtures/dic_min_no_autos.ri` — a declared objective in a
    /// scope with NO auto params at all. Every cell it reads is a literal-backed
    /// `param`, so no solver variable can ever move the cost: structurally inert.
    #[test]
    fn objective_over_literal_param_with_no_autos_is_inert() {
        let t = no_autos_template();
        let finding = inert_objective_finding(&t, std::slice::from_ref(&t))
            .expect("minimize k*k over a never-auto param must be reported inert");

        assert_eq!(
            finding.never_auto_cells,
            vec![ValueCellId::new("DicMinNoAutos", "k")],
            "the finding must name the never-auto cell the objective reads"
        );
        assert!(
            !finding.anchor_span.is_empty(),
            "anchor_span must be non-empty so the diagnostic carries a ≥1 real label \
             (the diagnostic_coverage_checkpoint.rs convention)"
        );
        assert_eq!(
            finding.anchor_span,
            span_for("k"),
            "anchor_span must point at the declaration that proves the inertness"
        );
    }

    /// `docs/prds/v0_6/fixtures/dic_min_unread.ri` — the scope HAS an auto (`a`,
    /// bounded by constraints), but the objective reads only the concrete `k`.
    /// The presence of an unrelated auto must NOT rescue the objective: the
    /// question is reachability from the objective, not scope-level auto count.
    #[test]
    fn unrelated_auto_in_scope_does_not_rescue_the_objective() {
        let mut t = no_autos_template();
        t.name = "DicMinUnread".to_string();
        t.value_cells = vec![
            auto("DicMinUnread", "a"),
            param("DicMinUnread", "k", 3.0),
        ];
        t.objective = Some(minimize(mul(
            vref("DicMinUnread", "k"),
            vref("DicMinUnread", "k"),
        )));

        let finding = inert_objective_finding(&t, std::slice::from_ref(&t))
            .expect("an objective that reads only `k` is inert even when `a` is auto");
        assert_eq!(
            finding.never_auto_cells,
            vec![ValueCellId::new("DicMinUnread", "k")]
        );
    }

    // ── NEGATIVE: genuine reachability ──────────────────────────────────────

    /// `dic_min_unconstrained.ri`'s compile-time half: the objective reads the
    /// auto directly, so it is well-posed at compile time. (Whether the solver
    /// then consumes it is the *runtime* half, `E_OBJECTIVE_UNCONSUMED`.)
    #[test]
    fn objective_reading_an_auto_directly_is_not_inert() {
        let mut t = tmpl("DicMinUnconstrained");
        t.value_cells = vec![auto("DicMinUnconstrained", "a")];
        t.objective = Some(minimize(mul(
            vref("DicMinUnconstrained", "a"),
            vref("DicMinUnconstrained", "a"),
        )));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "an objective that reads an auto directly governs a solver variable"
        );
    }

    /// Transitive closure: `minimize v` where `let v = a * 2.0` and `a` is auto.
    /// One hop of let-indirection must not hide the auto.
    #[test]
    fn objective_reading_a_let_that_reads_an_auto_is_not_inert() {
        let mut t = tmpl("Indirect");
        t.value_cells = vec![
            auto("Indirect", "a"),
            let_cell("Indirect", "v", mul(vref("Indirect", "a"), lit(2.0))),
        ];
        t.objective = Some(minimize(vref("Indirect", "v")));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "the closure over default_expr must reach the auto through the let"
        );
    }

    /// Two hops: `minimize w`, `let w = v`, `let v = a`, `a` auto. The closure
    /// must not stop at depth 1.
    #[test]
    fn multi_hop_let_chain_to_an_auto_is_not_inert() {
        let mut t = tmpl("Chain");
        t.value_cells = vec![
            auto("Chain", "a"),
            let_cell("Chain", "v", vref("Chain", "a")),
            let_cell("Chain", "w", vref("Chain", "v")),
        ];
        t.objective = Some(minimize(vref("Chain", "w")));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "the closure must be transitive, not single-hop"
        );
    }

    // ── NEGATIVE: opaque-node conservative bails ────────────────────────────

    /// `minimize cost(self.descendants)` lowers to a `MethodCall` carrying ZERO
    /// compile-time `ValueRef`s of its own, yet genuinely couples to a child's
    /// auto at eval time. The whole subtree is opaque ⇒ bail.
    #[test]
    fn method_call_in_an_objective_term_bails() {
        let mut t = no_autos_template();
        t.objective = Some(minimize(raw(CompiledExprKind::MethodCall {
            object: Box::new(vref("DicMinNoAutos", "k")),
            method: "cost".to_string(),
            args: vec![],
        })));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a MethodCall term is not data-transparent — its coupling is invisible \
             at compile time, so the predicate must stay silent"
        );
    }

    /// A structural/reflective query (here a `Quantifier`) reaches cells that no
    /// compile-time ref set enumerates ⇒ bail.
    #[test]
    fn structural_query_in_an_objective_term_bails() {
        let mut t = no_autos_template();
        t.objective = Some(minimize(raw(CompiledExprKind::Quantifier {
            kind: QuantifierKind::ForAll,
            variable: "x".to_string(),
            variable_id: ValueCellId::new("DicMinNoAutos", "x"),
            collection: Box::new(vref("DicMinNoAutos", "k")),
            predicate: Box::new(lit(1.0)),
        })));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a structural query is not data-transparent"
        );
    }

    /// A `Lambda` body's refs are not enumerated by `collect_value_refs` (only
    /// its captures are), so the term is opaque ⇒ bail.
    #[test]
    fn lambda_in_an_objective_term_bails() {
        let mut t = no_autos_template();
        t.objective = Some(minimize(raw(CompiledExprKind::Lambda {
            params: vec![],
            param_ids: vec![],
            body: Box::new(vref("DicMinNoAutos", "k")),
            captures: vec![ValueCellId::new("DicMinNoAutos", "k")],
        })));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a Lambda term is not data-transparent"
        );
    }

    /// A `CrossSubGeometryRef` points outside this template entirely ⇒ bail.
    #[test]
    fn cross_sub_geometry_ref_in_an_objective_term_bails() {
        let mut t = no_autos_template();
        t.objective = Some(minimize(mul(
            vref("DicMinNoAutos", "k"),
            CompiledExpr::cross_sub_geometry_ref(
                ValueCellId::new("DicMinNoAutos.child", "shape"),
                Type::dimensionless_scalar(),
            ),
        )));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a CrossSubGeometryRef reaches another scope's cell — not provable here"
        );
    }

    /// An `Error`-typed subexpression means compilation already failed somewhere
    /// in the term; piling a second Error on poisoned IR is noise ⇒ bail.
    #[test]
    fn error_typed_subexpr_in_an_objective_term_bails() {
        let mut t = no_autos_template();
        let poisoned = CompiledExpr {
            kind: CompiledExprKind::Literal(Value::Real(1.0)),
            result_type: Type::Error,
            content_hash: ContentHash(0),
        };
        t.objective = Some(minimize(mul(vref("DicMinNoAutos", "k"), poisoned)));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a Type::Error subexpr marks already-poisoned IR — stay silent"
        );
    }

    // ── NEGATIVE: ref-resolution conservative bails ─────────────────────────

    /// `minimize 1mm` (purpose bodies and doc-build fixtures rely on this being
    /// diagnostic-free). With an empty resolvable-ref union there is nothing to
    /// name and nothing was claimed about autos ⇒ not reported.
    #[test]
    fn pure_literal_objective_is_not_reported() {
        let mut t = tmpl("LiteralObjective");
        t.value_cells = vec![param("LiteralObjective", "k", 3.0)];
        t.objective = Some(minimize(lit(1.0)));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a pure-literal objective has no references — the rule presupposes ≥1"
        );
    }

    /// `minimize a.b.c.d.e` over a template that declares none of those cells:
    /// the refs resolve to nothing here, so nothing is proven ⇒ bail.
    #[test]
    fn unresolvable_ref_bails() {
        let mut t = no_autos_template();
        t.objective = Some(minimize(mul(
            vref("DicMinNoAutos", "k"),
            vref("Elsewhere", "ghost"),
        )));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "a ref that resolves to no cell of this template is unproven"
        );
    }

    /// A `where`-guarded auto lands in `guarded_groups[*].members`, NOT in
    /// `value_cells` (`guards.rs`). The cell index must span both, or a guarded
    /// auto reads as a missing cell and the objective looks falsely inert.
    #[test]
    fn auto_declared_in_a_guarded_group_member_is_not_inert() {
        let mut t = tmpl("Guarded");
        let mut g = guarded_group("Guarded");
        g.members = vec![auto("Guarded", "a")];
        t.guarded_groups = vec![g];
        t.objective = Some(minimize(vref("Guarded", "a")));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "guarded_groups[*].members must participate in cell resolution"
        );
    }

    /// Same, for the `else` branch of a guarded group.
    #[test]
    fn auto_declared_in_a_guarded_group_else_member_is_not_inert() {
        let mut t = tmpl("GuardedElse");
        let mut g = guarded_group("GuardedElse");
        g.else_members = vec![auto("GuardedElse", "a")];
        t.guarded_groups = vec![g];
        t.objective = Some(minimize(vref("GuardedElse", "a")));

        assert!(
            inert_objective_finding(&t, std::slice::from_ref(&t)).is_none(),
            "guarded_groups[*].else_members must participate in cell resolution"
        );
    }

    // ── NEGATIVE: whole-module sub-instance auto override ───────────────────

    /// `sub c : Child { k = auto }` makes `Child`'s `minimize k*k` genuinely
    /// governing — but the override cell is minted into the PARENT template
    /// (`phase_sub_override_autos`), scoped `Parent.c`/`k`, while `Child`'s own
    /// `k` stays a `Param`. Without whole-module visibility this is a
    /// structurally guaranteed false Inert.
    #[test]
    fn sub_instance_auto_override_elsewhere_in_the_module_bails() {
        let child = no_autos_template();
        let mut parent = tmpl("Parent");
        parent.sub_components = vec![sub_decl("c", "DicMinNoAutos")];
        parent.value_cells = vec![auto("Parent.c", "k")];

        let all = vec![parent, child.clone()];
        assert!(
            inert_objective_finding(&child, &all).is_none(),
            "a parent-scoped `Parent.c`/`k` auto override makes Child's objective \
             governing — the predicate must see the whole module"
        );
    }

    /// The same shape, but the parent declares no `sub c` at all, so the scoped
    /// auto's structure type cannot be resolved. Unproven ⇒ conservative bail.
    #[test]
    fn unresolvable_sub_type_on_a_scoped_auto_bails_conservatively() {
        let child = no_autos_template();
        let mut parent = tmpl("Parent");
        parent.sub_components = vec![];
        parent.value_cells = vec![auto("Parent.c", "k")];

        let all = vec![parent, child.clone()];
        assert!(
            inert_objective_finding(&child, &all).is_none(),
            "when the scoped auto's sub cannot be resolved to a structure, the \
             predicate cannot prove the override is unrelated — bail"
        );
    }

    /// Control for the two cases above: a scoped auto whose sub resolves to a
    /// DIFFERENT structure is genuinely unrelated and must NOT suppress the
    /// finding — otherwise the bail would swallow every real target.
    #[test]
    fn scoped_auto_on_an_unrelated_structure_does_not_suppress() {
        let child = no_autos_template();
        let other = tmpl("Other");
        let mut parent = tmpl("Parent");
        parent.sub_components = vec![sub_decl("c", "Other")];
        parent.value_cells = vec![auto("Parent.c", "k")];

        let all = vec![parent, other, child.clone()];
        assert!(
            inert_objective_finding(&child, &all).is_some(),
            "an override on an unrelated structure must not rescue this objective"
        );
    }

    // ── determinism ─────────────────────────────────────────────────────────

    /// The reported cells are sorted and the predicate is a pure function of its
    /// inputs — diagnostic text must not vary run to run (PRD §3 decision 8).
    #[test]
    fn reported_cells_are_sorted_and_the_predicate_is_deterministic() {
        let mut t = tmpl("Deterministic");
        t.value_cells = vec![
            param("Deterministic", "z", 1.0),
            param("Deterministic", "m", 2.0),
            param("Deterministic", "b", 3.0),
        ];
        t.objective = Some(minimize(mul(
            mul(vref("Deterministic", "z"), vref("Deterministic", "m")),
            vref("Deterministic", "b"),
        )));

        let first = inert_objective_finding(&t, std::slice::from_ref(&t))
            .expect("objective over three never-auto params is inert");
        assert_eq!(
            first.never_auto_cells,
            vec![
                ValueCellId::new("Deterministic", "b"),
                ValueCellId::new("Deterministic", "m"),
                ValueCellId::new("Deterministic", "z"),
            ],
            "cells must be sorted, not in source/traversal order"
        );

        let second = inert_objective_finding(&t, std::slice::from_ref(&t))
            .expect("second call must agree with the first");
        assert_eq!(first.never_auto_cells, second.never_auto_cells);
        assert_eq!(first.anchor_span, second.anchor_span);
    }

    /// A repeated reference to the same cell is reported once.
    #[test]
    fn repeated_refs_are_deduplicated() {
        let t = no_autos_template();
        let finding = inert_objective_finding(&t, std::slice::from_ref(&t)).unwrap();
        assert_eq!(
            finding.never_auto_cells.len(),
            1,
            "`minimize k * k` names `k` once, not twice"
        );
    }

    /// A template with no declared objective is not this pass's business.
    #[test]
    fn template_without_an_objective_yields_nothing() {
        let mut t = no_autos_template();
        t.objective = None;
        assert!(inert_objective_finding(&t, std::slice::from_ref(&t)).is_none());
    }
}
