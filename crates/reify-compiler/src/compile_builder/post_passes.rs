//! Post-compilation passes that run after entities are compiled: recursion
//! detection + hash remix (step 15) and duplicate-signature / field
//! composition / purpose compilation (step 16).
//!
//! Each function takes `&mut CompilationCtx` (and `&ParsedModule` for
//! `phase_purposes`) and mutates the relevant ctx fields in place, with
//! the exception of `phase_purposes` which returns `Vec<CompiledPurpose>`
//! since purposes are not owned by `CompilationCtx`.

use std::collections::{BTreeSet, HashMap, HashSet};

use reify_ast::ParsedModule;
use reify_core::{
    ContentHash, Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type, ValueCellId,
};
use reify_ir::{CompiledExpr, CompiledExprKind, ObjectiveSense, ObjectiveSet};

use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::{check_field_composition_types, collect_composed_field_dependencies};
use crate::scc;
use crate::termination::check_recursive_termination;
use crate::traits::compile_purpose;
use crate::types::{
    CompiledField, CompiledFieldSource, CompiledPurpose, TopologyTemplate, ValueCellDecl,
};

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

/// A proof that a template's declared objective governs nothing — the payload
/// of `E_OBJECTIVE_INERT` (DIC γ, task #5417; PRD
/// `docs/prds/v0_6/declared-intent-consumption-accounting.md` §3 decision 3).
///
/// Produced only by [`inert_objective_finding`], and only when the proof is
/// total; see that function for the bail rules.
#[derive(Debug, Clone)]
pub(crate) struct InertObjectiveFinding {
    /// The cells the objective expression itself names, each proven to be
    /// permanently non-`auto`. Sorted and deduplicated so the rendered
    /// diagnostic text is a pure function of the IR.
    ///
    /// These are the objective's *direct* references rather than the whole
    /// transitive closure: they are the identifiers the author actually wrote,
    /// which is what a "your objective reads only X" message must name.
    pub never_auto_cells: Vec<ValueCellId>,
    /// A non-empty span to anchor the diagnostic's primary label on. Neither
    /// `TopologyTemplate` nor `CompiledExpr` carries a declaration span, so the
    /// finding borrows the `ValueCellDecl.span` of the first reported cell —
    /// the declaration that *proves* the inertness, and therefore the place a
    /// reader must edit (`k` must become `auto`).
    pub anchor_span: SourceSpan,
}

/// Is this node's value-flow fully described by the sub-expressions
/// `CompiledExpr::collect_value_refs` recurses into, and confined to cells of
/// the enclosing template?
///
/// Deliberately an **allowlist**. The rule this feeds must never produce a
/// false `E_OBJECTIVE_INERT` — that would be a compile Error on legal code —
/// so an unrecognised node counts as opaque. The consequence is that a
/// `CompiledExprKind` variant added later silently *narrows* the rule
/// (a missed report) instead of silently widening it (a wrong report).
///
/// The enumerated opaque shapes this excludes, and why each is not provable
/// from compile-time refs alone:
/// - `MethodCall` — `minimize cost(self.descendants)` carries zero
///   compile-time `ValueRef`s yet genuinely couples to a child's `auto`.
/// - `Quantifier` and the other structural/reflective queries — they reach
///   cells no compile-time ref set enumerates.
/// - `Lambda` — `collect_value_refs` emits its captures and does not descend
///   into the body, so the body's reads are invisible here.
/// - `CrossSubGeometryRef` — names a cell in another scope entirely.
/// - `UserFunctionCall` — the callee's body may read cells that are not among
///   the argument expressions.
///
/// A `Type::Error` result anywhere in the term means compilation already
/// failed inside it; piling a second Error onto poisoned IR is noise, so those
/// nodes are opaque too.
fn is_data_transparent(node: &CompiledExpr) -> bool {
    if node.result_type.is_error() {
        return false;
    }
    matches!(
        node.kind,
        CompiledExprKind::Literal(_)
            | CompiledExprKind::ValueRef(_)
            | CompiledExprKind::BinOp { .. }
            | CompiledExprKind::UnOp { .. }
            | CompiledExprKind::Conditional { .. }
            | CompiledExprKind::OptionSome(_)
            | CompiledExprKind::OptionNone
    )
}

/// True when every node of `expr` is data-transparent.
fn is_wholly_transparent(expr: &CompiledExpr) -> bool {
    let mut transparent = true;
    expr.walk(&mut |node| {
        if !is_data_transparent(node) {
            transparent = false;
        }
    });
    transparent
}

/// Every value cell a template declares, including the ones a `where` clause
/// parks in a guarded group.
///
/// A guarded `auto` lands in `guarded_groups[*].members` / `.else_members` and
/// *not* in `value_cells` (`guards.rs`), so indexing `value_cells` alone would
/// read a guarded `auto` as an unknown cell and make its objective look
/// falsely inert.
fn declared_cells(template: &TopologyTemplate) -> impl Iterator<Item = &ValueCellDecl> {
    template.value_cells.iter().chain(
        template
            .guarded_groups
            .iter()
            .flat_map(|g| g.members.iter().chain(g.else_members.iter())),
    )
}

/// Does `template_name` denote the structure `structure_name`, allowing for
/// monomorphisation?
///
/// A generic structure is compiled once per instantiation under the mangled
/// name `Generic$Arg` (`types::mangle_monomorph_name`), while the sub
/// declaration that instantiated it still records the *unmangled*
/// `structure_name`. Comparing the two by equality alone would miss the link
/// and let a genuine `auto` override on a generic child slip through as a
/// false Inert.
fn names_same_structure(template_name: &str, structure_name: &str) -> bool {
    template_name == structure_name
        || template_name
            .strip_prefix(structure_name)
            .is_some_and(|rest| rest.starts_with('$'))
}

/// Does any template in the module install an `auto` override on an instance of
/// `template` that lands on one of `closure`'s members?
///
/// `sub c : Child { k = auto }` mints a `ValueCellKind::Auto` cell scoped
/// `Parent.c`/`k` into the **parent** template (`phase_sub_override_autos` /
/// `phase_connect_auto_params`); `Child`'s own `k` stays a `Param`. A check
/// with only per-template visibility is therefore structurally guaranteed to
/// misread that shape as inert, which is why this predicate takes the whole
/// module.
///
/// Returns `true` both when an override is found and when one *cannot be ruled
/// out* — a scoped `auto` whose owning sub does not resolve to a structure, or
/// resolves only through a nested path this function does not walk, is treated
/// as a possible override.
fn auto_override_possible(
    template: &TopologyTemplate,
    all_templates: &[TopologyTemplate],
    closure: &BTreeSet<ValueCellId>,
) -> bool {
    let closure_members: HashSet<&str> = closure.iter().map(|id| id.member.as_str()).collect();

    for other in all_templates {
        for decl in declared_cells(other) {
            if !decl.kind.is_auto() || !closure_members.contains(decl.id.member.as_str()) {
                continue;
            }
            let Some(sub_path) = decl
                .id
                .entity
                .strip_prefix(&format!("{}.", other.name))
                .filter(|rest| !rest.is_empty())
            else {
                // An unscoped `auto` belongs to its own template's scope and
                // cannot be an override of ours. A scoped one we cannot
                // attribute to `other` is unaccounted for — assume the worst.
                if decl.id.entity.contains('.') {
                    return true;
                }
                continue;
            };

            let (head, nested) = match sub_path.split_once('.') {
                Some((head, _)) => (head, true),
                None => (sub_path, false),
            };
            match other.sub_components.iter().find(|s| s.name == head) {
                // A nested path (`Parent.a.b`) names a sub of a sub; resolving
                // it needs a walk this function does not do.
                Some(_) if nested => return true,
                Some(sub) => {
                    if names_same_structure(&template.name, &sub.structure_name) {
                        return true;
                    }
                }
                // Scoped like a sub override, but `other` declares no such sub:
                // the target structure is unknown, so it may well be ours.
                None => return true,
            }
        }
    }
    false
}

/// Prove — or decline to prove — that `template`'s declared objective is
/// **structurally inert**: that no solver variable can ever move its cost,
/// because every cell it can reach is permanently non-`auto`.
///
/// This is the compile half of DIC γ (task #5417). It is a pure function of
/// the compiled module, wired into the pipeline by
/// `phase_inert_objective_check`, and it reports only on a positive proof:
/// every ambiguity returns `None`. A false `E_OBJECTIVE_INERT` would reject
/// legal code, whereas a missed one merely leaves today's silence in place, so
/// the asymmetry is deliberate (PRD §3 decision 5).
///
/// The proof obligations, in order:
/// 1. the template declares an objective at all;
/// 2. every node of every objective term is data-transparent
///    ([`is_data_transparent`]);
/// 3. the objective names at least one value cell — `minimize 1mm` asserts
///    nothing about autos and is not this rule's business;
/// 4. every named cell resolves to a declaration of *this* template;
/// 5. the transitive closure of those cells through their `default_expr`s
///    reaches no `auto` — a `let` may not launder one;
/// 6. no other template in the module installs an `auto` override onto an
///    instance of this one that lands in that closure
///    ([`auto_override_possible`]).
///
/// `all_templates` is the whole module, `template` included.
pub(crate) fn inert_objective_finding(
    template: &TopologyTemplate,
    all_templates: &[TopologyTemplate],
) -> Option<InertObjectiveFinding> {
    // (1) + (2) — an objective exists and is wholly readable.
    let objective = template.objective.as_ref()?;
    if !objective
        .terms
        .iter()
        .all(|term| is_wholly_transparent(&term.expr))
    {
        return None;
    }

    let cells: HashMap<&ValueCellId, &ValueCellDecl> =
        declared_cells(template).map(|d| (&d.id, d)).collect();

    // (3) — the cells the author actually named, sorted and deduplicated.
    let mut named: Vec<ValueCellId> = objective
        .terms
        .iter()
        .flat_map(|term| term.expr.collect_value_refs())
        .collect();
    named.sort();
    named.dedup();
    if named.is_empty() {
        return None;
    }

    // (4) + (5) — close over `default_expr` inside this template, refusing to
    // conclude anything the moment a cell is unknown, opaque, or `auto`.
    let mut closure: BTreeSet<ValueCellId> = BTreeSet::new();
    let mut pending: Vec<ValueCellId> = named.clone();
    while let Some(id) = pending.pop() {
        if !closure.insert(id.clone()) {
            continue;
        }
        let decl = cells.get(&id)?;
        if decl.kind.is_auto() {
            return None;
        }
        if let Some(default_expr) = &decl.default_expr {
            if !is_wholly_transparent(default_expr) {
                return None;
            }
            pending.extend(default_expr.collect_value_refs());
        }
    }

    // (6) — whole-module sub-instance `auto` overrides.
    if auto_override_possible(template, all_templates, &closure) {
        return None;
    }

    // Anchor on the declaration that proves the inertness, so the reader lands
    // on the `param` they must turn into an `auto`.
    let anchor_span = cells.get(&named[0])?.span;
    Some(InertObjectiveFinding {
        never_auto_cells: named,
        anchor_span,
    })
}

/// Render the sense shared by an objective set's terms, for the diagnostic
/// message. Falls back to the neutral word when a set mixes senses.
fn objective_sense_word(objective: &ObjectiveSet) -> &'static str {
    let mut senses = objective.terms.iter().map(|t| t.sense);
    match senses.next() {
        Some(first) if senses.all(|s| s == first) => match first {
            ObjectiveSense::Minimize => "minimize",
            ObjectiveSense::Maximize => "maximize",
        },
        _ => "objective",
    }
}

/// Post-compilation pass: report every template whose declared objective
/// provably governs nothing (`E_OBJECTIVE_INERT`, DIC γ, task #5417).
///
/// The decision itself lives in [`inert_objective_finding`]; this pass is only
/// the traversal and the wording. One diagnostic per objective *declaration*,
/// however many never-auto cells it names — the #5014 aggregation rule.
///
/// Purposes are excluded structurally rather than by a filter: their objectives
/// live on `CompiledPurpose.objective`, which a pass over `ctx.templates` never
/// touches. A purpose's `subject` is bound at application time, so there is no
/// template whose autos could even be counted.
///
/// **Ordering.** Must run after `phase_sub_override_autos` /
/// `phase_connect_auto_params`, because those are what mint the parent-scoped
/// `Parent.sub`/`member` cells that prove a child objective is governing after
/// all; running earlier would report exactly the templates the override was
/// written to rescue.
pub(crate) fn phase_inert_objective_check(ctx: &mut CompilationCtx) {
    // Collect first, extend after: the predicate borrows `ctx.templates`
    // immutably for the whole walk (the NLL idiom `phase_sub_override_autos`
    // and `phase_pending_bound_checks` already use).
    let mut findings: Vec<Diagnostic> = Vec::new();

    for template in &ctx.templates {
        let Some(objective) = template.objective.as_ref() else {
            continue;
        };
        let Some(finding) = inert_objective_finding(template, &ctx.templates) else {
            continue;
        };

        let sense = objective_sense_word(objective);
        let cells = finding
            .never_auto_cells
            .iter()
            .map(|id| format!("`{}`", id.member))
            .collect::<Vec<_>>()
            .join(", ");
        // Only the verb inflects — the subject phrase is the same either way,
        // so binding it twice just invited a reader to hunt for a difference
        // that is not there (#5417 step-15 tidy).
        let verb = if finding.never_auto_cells.len() == 1 {
            "is"
        } else {
            "are"
        };

        findings.push(
            Diagnostic::error(format!(
                "E_OBJECTIVE_INERT: the `{sense}` declared in `{}` cannot govern \
                 anything — it reads only {cells}, which {verb} never `auto`, so no solver \
                 variable can change its value. Declare one of them `auto` (e.g. \
                 `= auto` in place of the literal default) to make the objective \
                 effective, or remove the objective.",
                template.name
            ))
            .with_code(DiagnosticCode::ObjectiveInert)
            .with_label(DiagnosticLabel::new(
                finding.anchor_span,
                "this cell is never `auto`, so the objective above cannot move it",
            )),
        );
    }

    ctx.diagnostics.extend(findings);
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
