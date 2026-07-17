// Recursive sub-component unfolding — unfold_recursive_sub and elaborate_child_* functions.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use reify_compiler::{TopologyTemplate, ValueCellKind, find_template};
use reify_core::{Diagnostic, ValueCellId, VersionId};
use reify_expr::ContainmentQuery;
use reify_ir::{CompiledFunction, DeterminacyState, PersistentMap, Value, ValueMap};

use crate::cache::{CacheStore, NodeId};
use crate::cell_commit::{CacheLeg, CommitLegs, DeterminacyRule, TraceSource, commit_cell_result};
use crate::cell_eval_ctx::cell_eval_ctx;
use crate::deps::{DependencyTrace, extract_dependency_trace, take_trace};
use crate::dirty::topological_sort;
use crate::eval_ctx_with_meta;
use crate::journal::EventJournal;
use crate::snapshot::Snapshot;

/// No-op [`ContainmentQuery`]: reproduces the pre-migration containment=None
/// behaviour of the bare `eval_ctx_with_meta` context that the eval-and-commit
/// sites below used before adopting `cell_eval_ctx`'s required-capability
/// constructor (task ε, #5057). Passing the live `Engine` instead would
/// require threading `&self` through these free functions, conflicting with
/// the already-split `&mut journal`/`&mut cache` borrows at the call sites in
/// engine_eval.rs.
///
/// Containment is the only one of `cell_eval_ctx`'s three required
/// capabilities that is a true no-op replica of the pre-migration context —
/// and, more precisely, of *unfold.rs's own* pre-migration context, not of
/// the main eval pass (see the second paragraph below). Determinacy is *not*
/// a stand-in: it is wired to the live `&snapshot.values`, a capability the
/// old bare `eval_ctx_with_meta` never had. This is not limited to the param
/// default_expr branch: all three `cell_eval_ctx` call sites this migration
/// introduces observe it identically — the arg branch (`args.iter().find(..)`
/// arm) and the default_expr branch in `elaborate_child_params_only`, and the
/// let-binding eval in `elaborate_child_lets_only`. A child param/let
/// expression containing a `DeterminacyPredicate` (e.g. `determined(x)`) now
/// resolves against the snapshot instead of unconditionally degrading to
/// `Value::Undef` — this intentionally matches the main eval pass, which
/// consistently uses `.with_determinacy(&snapshot.values)` (engine_eval.rs).
/// The plain arithmetic param/let expressions this migration's parity
/// fixture exercises don't observe any difference, but an expression that
/// does reference a determinacy predicate would now resolve differently at
/// any of the three sites than before this migration. (The third capability,
/// runtime_sink, is wired but its contents are intentionally discarded — see
/// the comment at its construction below.)
///
/// The "true no-op replica" claim above is narrower than it may read at a
/// glance: it holds against unfold.rs's *own* pre-migration behaviour, not
/// against the main eval pass. The main pass wires the live `Engine` as its
/// `ContainmentQuery` (engine_eval.rs), so a child param/let default or arg
/// referencing a containment/restrict predicate resolves through real
/// geometry there; on this recursive-unfold path it always sees `None` (→
/// `Value::Undef` for a restricted-field sample), both before and after this
/// migration. `cell_eval_ctx`'s required-capability constructor doesn't
/// create this divergence from the main pass — it turns unfold.rs's
/// previously-*implicit* "no containment reaches here" into an explicit,
/// permanent wire. Closing the gap (wiring the real kernel) would require
/// threading `&self`/`&Engine` through these free functions; that enrichment
/// is future work, out of ε's behaviour-preserving scope.
///
/// Confirmation (#5057 reviewer_comprehensive, suggestion #2 —
/// robustness-behavior-change): the determinacy flip above is a genuine
/// semantic change on this path, so it was checked against both existing
/// fixtures and shipped designs before landing. `cargo test -p reify-eval
/// --test recursive_unfold --test unfold_commit_parity` (the two suites that
/// exercise this file's param/let elaboration) is green; neither references a
/// `DeterminacyPredicate` expression (`recursive_unfold.rs` has no "determin"
/// hits at all, and `unfold_commit_parity.rs`'s "determin" hits are all
/// `DeterminacyState::{Determined,Undetermined}` — the committed-state enum
/// this migration's cache/snapshot legs stamp — not the expression-level
/// predicate this comment is about). A repo-wide grep for `determined(`
/// across `*.ri` files (examples + fixtures) turns up recursive
/// (`is_recursive`-eligible) templates that combine recursion with a
/// determinacy predicate —
/// `examples/integration_full_v01.ri`'s `RecursiveBeam`,
/// `examples/m9_combined.ri`'s `BracketTree`, and
/// `examples/m9_integration.ri`'s `RecursiveChain` — but in every case the
/// predicate appears as a top-level `constraint determined(x)`, never inside
/// a child `param` default or `let` binding. Constraints are checked by the
/// already-determinacy-aware constraint-checking pass (unaffected by this
/// migration, which only touches value-cell elaboration in this file), so
/// they never degraded to `Value::Undef` pre-migration and are not touched by
/// this change. `m9_integration.ri`'s own comment block even notes the
/// adjacent case that remains unsupported post-migration: `determined()` in
/// *guard* position still returns `Undef`, because the guard eval (:168,
/// `unfold_recursive_sub`) is explicitly out of this migration's scope and
/// stays on the bare `eval_ctx_with_meta` (no determinacy capability) — see
/// the "Scope boundary" section of the task's plan. So no existing fixture or
/// shipped design relied on the pre-migration Undef-degradation this
/// migration changes; the only place that behaviour is now observably
/// different is the two dedicated divergence tests in this file's `tests`
/// module below.
struct NoContainment;

impl ContainmentQuery for NoContainment {
    fn contains(&self, _region: &Value, _point: &Value) -> Option<bool> {
        None
    }
}

/// Recursively unfold a recursive sub-component until the guard evaluates to false
/// or the depth limit is reached.
///
/// The guard expression in `sub.guard_state` (when `Compiled`) uses the owning template's entity name (e.g., "A.n").
/// To correctly evaluate the guard at each recursion level, we build a "local" values context
/// by remapping the current parent entity's values to the `scope_template`'s namespace.
///
/// For self-recursion (S→S), scope_template == child_template. For mutual recursion
/// (A→B→A), scope_template is the template that owns `sub` (e.g., A for sub b=B),
/// while child_template is the target (B). This ensures guard/arg expressions match
/// local_values keys.
///
/// # Parameters
/// - `scope_template`: the template that owns `sub` (used for building local_values that
///   match the guard/arg expression references)
/// - `child_template`: the target template instantiated by `sub` (used for Phase 1 elaboration)
/// - `parent_entity`: the entity currently being processed (e.g., "A" at depth 0, "A.b" at depth 1)
/// - `depth`: current recursion depth (0 = processing the top-level template)
/// - `max_depth`: maximum allowed depth before stopping
/// - `templates`: all templates in the module, used to look up target templates for
///   child_template's recursive subs in Phase 2.
/// - `node_budget`: remaining total nodes allowed across all branches. Prevents exponential
///   blowup when B > 1 recursive subs exist (B^D total without this limit).
#[allow(clippy::too_many_arguments)]
pub(crate) fn unfold_recursive_sub<'t>(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    scope_template: &'t TopologyTemplate,
    child_template: &'t TopologyTemplate,
    sub: &reify_compiler::SubComponentDecl,
    parent_entity: &str,
    depth: usize,
    max_depth: usize,
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &'t [TopologyTemplate],
    node_budget: &mut usize,
) {
    // Check total node budget before doing any work.
    if *node_budget == 0 {
        diagnostics.push(Diagnostic::error(format!(
            "recursive unfolding of '{}' stopped: total node budget exhausted at depth {}",
            parent_entity, depth,
        )));
        return;
    }

    let Some(guard_expr) = sub.guard_state.compiled() else {
        return;
    };

    // Build a small overlay map with only the template-scoped entries needed for
    // guard/arg evaluation. Uses scope_template (the template that owns `sub`) so
    // the guard/arg expression references (e.g., A.n) match local_values keys.
    // For self-recursion scope_template == child_template; for mutual recursion they differ.
    let mut local_values = ValueMap::new();
    for cell in &scope_template.value_cells {
        let scoped_id = ValueCellId::new(parent_entity, &cell.id.member);
        if let Some(v) = values.get(&scoped_id) {
            local_values.insert(cell.id.clone(), v.clone());
        }
    }

    // Evaluate the guard in the local context.
    let guard_val = reify_expr::eval_expr(
        guard_expr,
        &eval_ctx_with_meta(&local_values, functions, meta_map),
    );

    // Differentiate guard outcomes: Bool(true) continues, Bool(false)/Undef terminate
    // normally, any other type is a guard expression bug that deserves a diagnostic.
    match &guard_val {
        Value::Bool(true) => {
            if depth >= max_depth {
                // Use Error (not Warning) so callers know the result is potentially unsound:
                // child references beyond the truncated depth resolve to Undef.
                diagnostics.push(Diagnostic::error(format!(
                    "recursive unfolding of '{}' truncated at depth limit {} (guard still true)",
                    parent_entity, max_depth,
                )));
                return;
            }
        }
        Value::Bool(false) => return, // Normal termination — guard says stop
        Value::Undef => return,       // Param not yet determined — do not unfold (per spec)
        other => {
            diagnostics.push(Diagnostic::error(format!(
                "guard for recursive sub '{}' in '{}' evaluated to {:?} (expected Bool), treating as termination",
                sub.name, parent_entity, other,
            )));
            return;
        }
    }

    // Pre-evaluate args in the local context (so child uses current level's param values, not top-level).
    // Use the arg expression's declared result_type for the literal wrapper.
    let concrete_args: Vec<(String, reify_ir::CompiledExpr)> = sub
        .args
        .iter()
        .map(|(name, arg_expr)| {
            let v = reify_expr::eval_expr(
                arg_expr,
                &eval_ctx_with_meta(&local_values, functions, meta_map),
            );
            let ty = arg_expr.result_type.clone();
            (name.clone(), reify_ir::CompiledExpr::literal(v, ty))
        })
        .collect();

    // Construct the next child's scoped entity name: parent_entity.sub_name
    let next_entity = format!("{}.{}", parent_entity, sub.name);

    // Consume one budget unit now that we know this call will actually create a node
    // (guard is true and depth is within limits). Decrementing here rather than at
    // function entry avoids wasting budget on guard-false or depth-limited returns.
    *node_budget -= 1;

    // Phase 1 (top-down): Set params for next_entity so the next recursion level
    // can evaluate its guard using the child's param values.
    let child_values = elaborate_child_params_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        &next_entity,
        &concrete_args,
        meta_map,
    );

    // Phase 2 (recurse): Unfold ALL of child_template's recursive subs at the next level
    // first (leaves-first ordering). Recomputing from child_template.sub_components (not
    // scope_template's) is critical for mutual recursion: when A→B, the next level must
    // iterate B's subs (not A's), so guard/arg expressions match B's value_cell namespace.
    let next_recursive_subs: Vec<&reify_compiler::SubComponentDecl> = child_template
        .sub_components
        .iter()
        .filter(|s| child_template.is_recursive && s.guard_state.is_compiled())
        .collect();
    let next_recursive_sub_names: Vec<&str> = next_recursive_subs
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    for next_sub in &next_recursive_subs {
        // Look up the target template for next_sub from the module's template list.
        // For self-recursion, this finds the same template. For mutual recursion (A→B→A),
        // this alternates: B's sub "a" targets A, A's sub "b" targets B.
        let next_child_template = match find_template(templates, &next_sub.structure_name) {
            Some(t) => t,
            None => {
                diagnostics.push(Diagnostic::error(format!(
                    "recursive sub \"{}\" in \"{}\" at depth {} references unknown structure \"{}\"; skipping branch",
                    next_sub.name, next_entity, depth + 1, next_sub.structure_name
                )));
                continue;
            }
        };
        // Template-role handoff across recursion levels:
        //
        // CURRENT level:  scope_template = (caller's child_template)
        //                 child_template  = this level's child_template (owns next_sub)
        //
        // NEXT level:     scope_template  ← this level's child_template
        //   because child_template owns next_sub, so its value_cells namespace is the
        //   right key-space for next_sub's guard_expr / arg expressions.
        //
        //                 child_template  ← next_child_template
        //   (the template named by next_sub.structure_name — the target instantiated
        //   by next_sub at the next depth).
        unfold_recursive_sub(
            values,
            snapshot,
            functions,
            journal,
            cache,
            version_id,
            child_template, // this level's child_template → next level's scope_template
            next_child_template, // target of next_sub → next level's child_template
            next_sub,
            &next_entity,
            depth + 1,
            max_depth,
            meta_map,
            diagnostics,
            templates,
            node_budget,
        );
    }

    // Phase 3 (bottom-up): Evaluate let-bindings for next_entity.
    // child_values is enriched inside elaborate_child_lets_only with sub-component
    // values projected from the global map — so cross-level references like
    // `S.child.total` resolve to the already-computed deeper-level value.
    // Pass next-level recursive sub names so BFS walks the correct branches.
    elaborate_child_lets_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        &next_entity,
        child_values,
        meta_map,
        &next_recursive_sub_names,
        templates,
        diagnostics,
    );
}

/// Elaborate a single child instance into the values/snapshot maps.
///
/// This handles both non-collection subs (single instance) and individual
/// collection sub instances (called in a loop for each index).
///
/// For non-recursive subs both phases run atomically (params then lets).
/// For recursive subs, use `elaborate_child_params_only` + `elaborate_child_lets_only`
/// to allow leaves-first ordering (recurse between the two phases).
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_child_instance(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &TopologyTemplate,
    scoped_entity: &str,
    args: &[(String, reify_ir::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let child_values = elaborate_child_params_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        scoped_entity,
        args,
        meta_map,
    );
    elaborate_child_lets_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        scoped_entity,
        child_values,
        meta_map,
        &[],
        &[],
        diagnostics,
    );
}

/// Builds a [`cell_eval_ctx`] from the given capabilities and evaluates
/// `expr` against it in one step, replacing the `let ctx = cell_eval_ctx(..);
/// reify_expr::eval_expr(expr, &ctx)` pair that was previously repeated at
/// each of this migration's three eval-and-commit call sites (both branches
/// of `elaborate_child_params_only`, plus `elaborate_child_lets_only`'s
/// let-eval).
///
/// Deliberately a plain function, not a closure hoisted above the caller's
/// loop: a closure capturing `determinacy`/`runtime_sink`/`containment` by
/// reference would hold its `&snapshot.values` borrow alive for as long as
/// the closure binding could still be called again — i.e. across the loop's
/// intervening `commit_cell_result(.., snapshot_values: &mut
/// snapshot.values, ..)` call, which the borrow checker rejects. A per-call
/// function borrows its arguments only for that call's duration, so it has
/// no such lifetime-extension conflict with the interleaved mutable commit.
fn eval_child_expr(
    value_map: &ValueMap,
    expr: &reify_ir::CompiledExpr,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    determinacy: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    runtime_sink: &RefCell<Vec<Diagnostic>>,
    containment: &dyn ContainmentQuery,
) -> Value {
    let ctx = cell_eval_ctx(
        value_map,
        functions,
        meta_map,
        determinacy,
        runtime_sink,
        containment,
    );
    reify_expr::eval_expr(expr, &ctx)
}

/// Phase 1: Evaluate and store only the param cells for a child instance.
///
/// Returns the template-scoped child_values map (params only) for use in phase 2.
/// All param values are also written to the global `values`, `snapshot`, journal, and cache.
#[allow(clippy::too_many_arguments)]
fn elaborate_child_params_only(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &TopologyTemplate,
    scoped_entity: &str,
    args: &[(String, reify_ir::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
) -> ValueMap {
    let mut child_values = ValueMap::new();
    // `cell_eval_ctx` requires a runtime_sink argument; diagnostics captured
    // here are intentionally discarded rather than surfaced. Pre-migration,
    // this eval ran on the bare `eval_ctx_with_meta` (no sink existed at
    // all), so nothing was ever surfaced from this path — discarding
    // preserves that behaviour (task ε, #5057 is a behaviour-preserving
    // migration; see plan.json design decisions). This function also has no
    // `diagnostics` parameter to drain into. Surfacing these diagnostics is
    // deferred to a future task.
    let runtime_sink = RefCell::new(Vec::new());
    let containment = NoContainment;

    for cell in &child_template.value_cells {
        if cell.kind != ValueCellKind::Param {
            continue;
        }

        let member = &cell.id.member;
        let scoped_id = ValueCellId::new(scoped_entity, member);

        // task 3806 (γ) precedence rule: if the parent template pushed an
        // explicit override cell for this scoped id (e.g. `sub b : Bearing {
        // bore = auto }` via entity.rs step-4), the snapshot entry carries
        // `DeterminacyState::Auto`.  Overwriting that with the child param's
        // concrete default would poison the solver's initial point and prevent
        // the M3 solver from resolving the Auto cell via the parent's
        // constraints.  Skip the child default write so the Auto state and its
        // `Value::Undef` initial value survive into `build_solver_problem`.
        if snapshot
            .values
            .get(&scoped_id)
            .map(|(_, det)| *det == DeterminacyState::Auto)
            .unwrap_or(false)
        {
            // Still populate child_values from the snapshot so that
            // downstream let-bindings that reference this member see a
            // consistent value (Undef for now; the solver will resolve it).
            if let Some((val, _)) = snapshot.values.get(&scoped_id) {
                child_values.insert(cell.id.clone(), val.clone());
            }
            continue;
        }

        let val = if let Some((_name, arg_expr)) = args.iter().find(|(name, _)| name == member) {
            eval_child_expr(
                values,
                arg_expr,
                functions,
                meta_map,
                &snapshot.values,
                &runtime_sink,
                &containment,
            )
        } else if let Some(ref default_expr) = cell.default_expr {
            eval_child_expr(
                &child_values,
                default_expr,
                functions,
                meta_map,
                &snapshot.values,
                &runtime_sink,
                &containment,
            )
        } else {
            Value::Undef
        };

        child_values.insert(cell.id.clone(), val.clone());

        // Provenance: `TraceSource::GuardedGroup` is the PRD-ratified tag for
        // this site, not a placeholder pick. `cell_commit.rs`'s enum doc
        // glosses the variant as "the GuardedParamCtx family" (engine_eval.rs's
        // guarded-group param cells, leaf γ) — but the source PRD
        // (`docs/prds/v0_6/eval-cell-commit-substrate.md` §0 and §8 leaf ε)
        // names the two paths as one bundle throughout ("...four live eval
        // paths (eval, eval_cached, edit_param, guarded-group/unfold)"; "ε ...
        // migrate unfold.rs guarded-group/unfold site"): this recursive-unfold
        // path IS the guarded-group provenance category by design, sharing the
        // slug with γ's engine_eval.rs sites rather than needing a dedicated
        // variant. A future §2.6 divergence audit reading "guarded-group"
        // should expect commits from both producers.
        commit_cell_result(
            CommitLegs {
                values,
                snapshot_values: &mut snapshot.values,
                cache,
                journal,
            },
            scoped_id.clone(),
            val,
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::GuardedGroup,
            DependencyTrace::default(),
            VersionId(version_id),
            CacheLeg::Record,
        );
    }

    child_values
}

/// Phase 2: Evaluate and store the let-binding cells for a child instance.
///
/// `child_values` should contain the template-scoped params from phase 1.
/// Before evaluating lets, this function enriches `child_values` with sub-component
/// values projected from the global `values` map — this enables cross-level let
/// expressions like `let total = if n > 0 then n + S.child.total else n` to see
/// values computed by deeper recursion levels (leaves-first ordering).
///
/// Projection rule: for each global entry whose entity starts with
/// `"{scoped_entity}."`, strip that prefix and add `"{template_name}."` to produce
/// a template-scoped key. E.g., when evaluating lets for `S.child` (template `S`):
///   global["S.child.child", "total"] → child_values["S.child", "total"]
///
/// For templates with multiple recursive subs, `recursive_sub_names` contains all
/// sub names. A BFS walks the full entity tree under `scoped_entity` (following all
/// sub name branches at each level), so cross-sub values are projected correctly.
/// E.g., for subs [left, right] at `S.left`: both `S.left.left.*` and `S.left.right.*`
/// are projected, enabling lets like `let sum = S.left.val + S.right.val`.
///
/// For heterogeneous mutual recursion (A→B→A where A and B have different members),
/// the BFS queue carries `(entity_path, &TopologyTemplate)` tuples so each depth level
/// iterates the correct template's value_cells. When enqueuing children, the entity's
/// template's sub_components determine child sub names and their target templates.
#[allow(clippy::too_many_arguments)]
fn elaborate_child_lets_only<'t>(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &'t TopologyTemplate,
    scoped_entity: &str,
    mut child_values: ValueMap,
    meta_map: &HashMap<String, HashMap<String, String>>,
    recursive_sub_names: &[&str],
    templates: &'t [TopologyTemplate],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // `cell_eval_ctx` requires a runtime_sink argument; diagnostics captured
    // here are intentionally discarded rather than appended to the
    // `diagnostics` parameter above. Pre-migration, this eval ran on the
    // bare `eval_ctx_with_meta` (no sink existed at all), so nothing was
    // ever surfaced from this path — discarding preserves that behaviour
    // (task ε, #5057 is a behaviour-preserving migration) and keeps this
    // site consistent with `elaborate_child_params_only`, which has no
    // `diagnostics` parameter to drain into at all. Surfacing these
    // diagnostics is deferred to a future task.
    let runtime_sink = RefCell::new(Vec::new());
    let containment = NoContainment;

    // Enrich child_values with sub-component values projected from the global map.
    // Only needed for recursive subs where deeper levels have already been elaborated
    // (leaves-first ordering).
    //
    // Uses BFS over the entity tree rooted at scoped_entity: starts with one immediate
    // child per sub name, then expands branches where values exist. This handles both
    // single-sub chains (O(D×C)) and multi-sub trees (O(B^D×C) where B=branching, D=depth).
    // The BFS terminates naturally when no values are found at a given entity.
    if !recursive_sub_names.is_empty() {
        let scoped_prefix = format!("{}.", scoped_entity);
        let template_prefix = format!("{}.", child_template.name);

        // BFS queue carries (entity_path, entity_template) so each depth level uses
        // the correct template's value_cells for projection (heterogeneous mutual recursion).
        let mut queue: std::collections::VecDeque<(String, &TopologyTemplate)> = recursive_sub_names
            .iter()
            .filter_map(|name| {
                // Look up the sub declaration to find its target template.
                let sub_decl = child_template.sub_components.iter().find(|s| s.name == *name)?;
                let target_tmpl = find_template(templates, &sub_decl.structure_name).or_else(|| {
                    diagnostics.push(Diagnostic::error(format!(
                        "BFS seed: sub \"{}\" in \"{}\" references unknown structure \"{}\"; skipping",
                        name, scoped_entity, sub_decl.structure_name
                    )));
                    None
                })?;
                Some((format!("{}.{}", scoped_entity, name), target_tmpl))
            })
            .collect();

        while let Some((depth_entity, entity_template)) = queue.pop_front() {
            let mut found_any = false;
            // Use entity_template.value_cells — NOT child_template.value_cells.
            // This ensures B-specific members (e.g., "height") are projected for B entities.
            for cell in &entity_template.value_cells {
                let id = ValueCellId::new(&depth_entity, &cell.id.member);
                if let Some(val) = values.get(&id)
                    && let Some(suffix) = depth_entity.strip_prefix(&scoped_prefix)
                {
                    let remapped_entity = format!("{}{}", template_prefix, suffix);
                    child_values.insert(
                        ValueCellId::new(remapped_entity, &cell.id.member),
                        val.clone(),
                    );
                    found_any = true;
                }
            }
            // For structural intermediaries (zero value_cells), found_any is always
            // false. Check whether any key in `values` has this entity as a prefix,
            // proving unfold_recursive_sub actually created descendants. Without this
            // check, two structural intermediaries forming a cycle (W1→W2→W1) would
            // cause the BFS to generate ever-growing entity paths without bound.
            let intermediary_has_descendants = entity_template.value_cells.is_empty()
                && values
                    .iter()
                    .any(|(k, _)| k.entity.starts_with(&format!("{}.", depth_entity)));
            if found_any || intermediary_has_descendants {
                // Enqueue children if:
                // 1. found_any: values were projected from this entity (entity exists), OR
                // 2. intermediary_has_descendants: structural intermediary with zero
                //    value_cells but confirmed descendants in the values map.
                // For templates WITH value_cells, found_any==false means the entity was
                // never unfolded (e.g., guard was false), so BFS terminates naturally.
                // For structural intermediaries, the prefix check serves the same purpose.
                for sub_decl in &entity_template.sub_components {
                    if sub_decl.guard_state.is_compiled() {
                        if let Some(target_tmpl) =
                            find_template(templates, &sub_decl.structure_name)
                        {
                            queue.push_back((
                                format!("{}.{}", depth_entity, sub_decl.name),
                                target_tmpl,
                            ));
                        } else {
                            diagnostics.push(Diagnostic::error(format!(
                                "BFS expand: sub \"{}\" in \"{}\" references unknown structure \"{}\"; skipping subtree",
                                sub_decl.name, depth_entity, sub_decl.structure_name
                            )));
                        }
                    }
                }
            }
        }
    }

    // Evaluate let-bindings in topological order.
    let child_let_cells: HashMap<NodeId, &reify_ir::CompiledExpr> = child_template
        .value_cells
        .iter()
        .filter(|c| c.kind == ValueCellKind::Let)
        .filter_map(|c| {
            c.default_expr
                .as_ref()
                .map(|expr| (NodeId::Value(c.id.clone()), expr))
        })
        .collect();

    let child_let_node_ids: HashSet<NodeId> = child_let_cells.keys().cloned().collect();
    let mut child_let_traces: HashMap<NodeId, DependencyTrace> = child_let_cells
        .iter()
        .map(|(nid, expr)| (nid.clone(), extract_dependency_trace(expr)))
        .collect();

    let sorted_child_lets = topological_sort(&child_let_node_ids, &child_let_traces);

    // Detect cyclic let-binding dependencies: if topological_sort dropped nodes
    // (Kahn's algorithm silently omits nodes in cycles), report them.
    if sorted_child_lets.len() < child_let_node_ids.len() {
        let sorted_set: HashSet<&NodeId> = sorted_child_lets.iter().collect();
        let mut cyclic_members: Vec<&str> = child_let_node_ids
            .iter()
            .filter(|nid| !sorted_set.contains(nid))
            .filter_map(|nid| match nid {
                NodeId::Value(vcid) => Some(vcid.member.as_str()),
                _ => None,
            })
            .collect();
        cyclic_members.sort();
        diagnostics.push(Diagnostic::error(format!(
            "circular let-binding dependency in template {} (entity {}): [{}]",
            child_template.name,
            scoped_entity,
            cyclic_members.join(", "),
        )));
    }

    for child_node_id in sorted_child_lets {
        let expr = child_let_cells[&child_node_id];
        // child_let_cells is keyed exclusively by NodeId::Value; topological_sort returns
        // only keys from that set — so this assertion holds in all correct code paths.
        // In debug/test builds it fires loud; in release the diagnostic+continue handles
        // any accidental invariant violation gracefully.
        debug_assert!(
            matches!(child_node_id, NodeId::Value(_)),
            "elaborate_child_lets_only: sorted_child_lets produced a non-Value NodeId: {:?}; construction invariant violated (entity {})",
            child_node_id,
            scoped_entity,
        );
        let child_cell_id = match &child_node_id {
            NodeId::Value(vcid) => vcid,
            _ => {
                diagnostics.push(Diagnostic::error(format!(
                    "let-binding evaluation: expected NodeId::Value, got {:?}; skipping (entity {})",
                    child_node_id, scoped_entity,
                )));
                continue;
            }
        };
        let member = &child_cell_id.member;

        let val = eval_child_expr(
            &child_values,
            expr,
            functions,
            meta_map,
            &snapshot.values,
            &runtime_sink,
            &containment,
        );
        child_values.insert(child_cell_id.clone(), val.clone());

        let scoped_id = ValueCellId::new(scoped_entity, member);

        // sorted_child_lets and child_let_traces are built from the same key set, so remove() cannot fail.
        let trace = take_trace(
            &mut child_let_traces,
            &child_node_id,
            "sorted_child_lets",
            "child_let_traces",
        );

        // Provenance: same PRD-ratified `TraceSource::GuardedGroup` slug as
        // the Site 1 commit in `elaborate_child_params_only` above — see the
        // provenance note there for why this is the intended tag, not a
        // stand-in for a missing dedicated variant.
        commit_cell_result(
            CommitLegs {
                values,
                snapshot_values: &mut snapshot.values,
                cache,
                journal,
            },
            scoped_id,
            val,
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::GuardedGroup,
            trace,
            VersionId(version_id),
            CacheLeg::Record,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion: elaborate_child_instance is accessible from this module.
    #[test]
    fn elaborate_child_instance_accessible() {
        let _: fn() -> String = || {
            // Reference the function to prove it exists in this module's namespace.
            let _ = elaborate_child_instance as fn(_, _, _, _, _, _, _, _, _, _, _);
            String::new()
        };
    }

    /// Compile-time assertion: unfold_recursive_sub is accessible from this module.
    #[test]
    fn unfold_recursive_sub_accessible() {
        let _: fn() -> String = || {
            let _ = unfold_recursive_sub as fn(_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _);
            String::new()
        };
    }

    // amend (#5057 reviewer_comprehensive, suggestion #2 — test-coverage-gap):
    // every existing parity fixture (`tests/unfold_commit_parity.rs`) supplies
    // an explicit arg for its only param, so only the `args.iter().find(..)`
    // arm (:380-389) of `elaborate_child_params_only` was ever exercised. This
    // fixture adds a second param, `m`, that `child`'s args never name — it
    // must fall back to `default_expr` (:390-399), evaluated against
    // `&child_values` rather than the parent `values`, under the newly-wired
    // `cell_eval_ctx`. Locks both the committed value and the migration's
    // journal-provenance signal for that branch.
    #[test]
    fn elaborate_child_params_only_default_expr_branch_commits_via_primitive() {
        use reify_core::{ModulePath, Type};
        use reify_ir::BinOp;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{
            CompiledModuleBuilder, TopologyTemplateBuilder, binop, gt, literal, value_ref_typed,
        };

        use crate::journal::{EventKind, EventPayload};

        let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
        let n_minus_1 = binop(
            BinOp::Sub,
            value_ref_typed("S", "n", Type::Int),
            literal(Value::Int(1)),
        );

        // `m` is never named in `child`'s args (only `n` is), so it must take
        // its default_expr at every recursion level, including S.child.
        let template = TopologyTemplateBuilder::new("S")
            .param(
                "S",
                "n",
                Type::Int,
                Some(reify_ir::CompiledExpr::literal(Value::Int(1), Type::Int)),
            )
            .param("S", "m", Type::Int, Some(literal(Value::Int(7))))
            .is_recursive(true)
            .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();
        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        let result = engine.eval(&module);

        let child_m = ValueCellId::new("S.child", "m");
        assert_eq!(
            result.values.get(&child_m),
            Some(&Value::Int(7)),
            "S.child.m should take its default_expr (7); child never receives an \
             explicit arg for m"
        );

        let events = engine
            .journal()
            .events_for_node(&NodeId::Value(child_m.clone()));
        assert!(
            events.iter().any(|e| {
                matches!(&e.kind, EventKind::Started)
                    && matches!(&e.payload, Some(EventPayload::Custom(s)) if s == "guarded-group")
            }),
            "expected a Started event for S.child.m carrying \
             EventPayload::Custom(\"guarded-group\"), got: {:?}",
            events
        );
    }

    // amend (#5057 reviewer_comprehensive, suggestion #1 — test-coverage-gap):
    // the `NoContainment` doc comment above (and the two fixtures below this
    // one) document that all three `cell_eval_ctx` call sites this migration
    // introduces observe the determinacy-map flip identically: the arg
    // branch (`args.iter().find(..)` arm), the param default_expr branch,
    // and the let-binding eval. Only the latter two had a direct lock.
    //
    // Locking the arg branch through a *recursive* sub (as the other two
    // fixtures do for their branches) would not actually exercise this
    // migration: `unfold_recursive_sub` pre-evaluates `sub.args` against the
    // old `eval_ctx_with_meta` context (:206-213 — the out-of-scope "arg
    // pre-eval" called out in plan.json's "Scope boundary" section) before
    // `elaborate_child_params_only` ever sees them, collapsing any
    // determinacy predicate in a *recursive* sub's arg to `Value::Undef`
    // before the arg branch runs, regardless of this migration. A plain
    // (non-recursive, non-guarded) sub-component instead reaches
    // `elaborate_child_params_only` via `elaborate_child_instance` with
    // `sub.args` forwarded raw (engine_eval.rs :3745-3757), so its arg
    // expression is exactly what the arg branch's newly-wired
    // `cell_eval_ctx` evaluates. This fixture uses that path: `S.child` is a
    // plain sub of `C`, supplying `determined(S.n)` as C's explicit `ready`
    // arg.
    //
    // amend (#5057 reviewer_comprehensive, suggestion #2 —
    // test-coverage-fragility): like the two divergence fixtures below, this
    // is load-bearing on an unstated ordering invariant — that root `S.n` is
    // already committed to `snapshot.values` as `DeterminacyState::Determined`
    // before `S`'s Phase 2 (sub-component elaboration) runs. That's true
    // today because Phase 1 param elaboration precedes Phase 2 within a
    // template's own root-frame evaluation (see
    // `non_recursive_top_level_guarded_sub_not_unfolded` in
    // `tests/recursive_unfold.rs`, whose assertion (a) locks the same
    // ordering). If that ordering ever changed, the `Bool(true)` assertion
    // below would silently flip to `Value::Undef` instead of failing loudly,
    // so the precondition is asserted explicitly first.
    #[test]
    fn elaborate_child_params_only_arg_branch_resolves_determinacy_predicate() {
        use reify_core::{ModulePath, Type};
        use reify_ir::DeterminacyPredicateKind;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, literal};

        let ready_arg = reify_ir::CompiledExpr::determinacy_predicate(
            DeterminacyPredicateKind::Determined,
            ValueCellId::new("S", "n"),
        );

        // Plain (non-guarded, non-recursive) sub: reaches
        // `elaborate_child_params_only` via `elaborate_child_instance` with
        // `sub.args` forwarded raw — the arg branch under test.
        let template_s = TopologyTemplateBuilder::new("S")
            .param(
                "S",
                "n",
                Type::Int,
                Some(reify_ir::CompiledExpr::literal(Value::Int(1), Type::Int)),
            )
            .sub_component("child", "C", vec![("ready".to_string(), ready_arg)])
            .build();

        let template_c = TopologyTemplateBuilder::new("C")
            .param("C", "ready", Type::Bool, Some(literal(Value::Bool(false))))
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template_s)
            .template(template_c)
            .build();
        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        let result = engine.eval(&module);

        let root_n = ValueCellId::new("S", "n");
        let snap = engine
            .snapshot()
            .expect("engine.snapshot() should be populated after eval");
        assert_eq!(
            snap.values.get(&root_n).map(|(_, det)| *det),
            Some(DeterminacyState::Determined),
            "precondition: S.n must already be committed Determined before \
             S's Phase 2 sub-component elaboration runs, or the Bool(true) \
             assertion below would silently degrade to Value::Undef instead \
             of failing loudly"
        );

        let child_ready = ValueCellId::new("S.child", "ready");
        assert_eq!(
            result.values.get(&child_ready),
            Some(&Value::Bool(true)),
            "S.child.ready, given explicit arg `ready: determined(S.n)` on a \
             plain (non-recursive) sub-component, should resolve Bool(true) \
             via the snapshot-wired determinacy cell_eval_ctx that now \
             threads through the args.iter().find(..) arm, not degrade to \
             Value::Undef as the pre-migration bare eval_ctx_with_meta \
             context did"
        );
    }

    // amend (#5057 reviewer_comprehensive, suggestion #1 —
    // robustness-untested-behavior-change): the migration wires the live
    // `&snapshot.values` determinacy map into `cell_eval_ctx`, where the
    // pre-migration bare `eval_ctx_with_meta` had none (see the `NoContainment`
    // doc comment above) — a real semantic change for any child param/let
    // expression containing a `DeterminacyPredicate`. This fixture locks the
    // intentional divergence: `S.child.ready`'s default references the
    // always-present, always-`Determined` root cell `S.n` (a stable absolute
    // reference regardless of recursion depth, unlike a recursion-local one),
    // so it must resolve `Bool(true)` post-migration instead of unconditionally
    // degrading to `Value::Undef` as it did pre-migration.
    #[test]
    fn elaborate_child_params_only_default_expr_resolves_determinacy_predicate() {
        use reify_core::{ModulePath, Type};
        use reify_ir::{BinOp, DeterminacyPredicateKind};
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{
            CompiledModuleBuilder, TopologyTemplateBuilder, binop, gt, literal, value_ref_typed,
        };

        let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
        let n_minus_1 = binop(
            BinOp::Sub,
            value_ref_typed("S", "n", Type::Int),
            literal(Value::Int(1)),
        );
        let ready_default = reify_ir::CompiledExpr::determinacy_predicate(
            DeterminacyPredicateKind::Determined,
            ValueCellId::new("S", "n"),
        );

        let template = TopologyTemplateBuilder::new("S")
            .param(
                "S",
                "n",
                Type::Int,
                Some(reify_ir::CompiledExpr::literal(Value::Int(1), Type::Int)),
            )
            .param("S", "ready", Type::Bool, Some(ready_default))
            .is_recursive(true)
            .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();
        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        let result = engine.eval(&module);

        let child_ready = ValueCellId::new("S.child", "ready");
        assert_eq!(
            result.values.get(&child_ready),
            Some(&Value::Bool(true)),
            "S.child.ready = determined(S.n) should resolve Bool(true) via the \
             snapshot-wired determinacy cell_eval_ctx now threads through the \
             default_expr branch, not degrade to Value::Undef as the \
             pre-migration bare eval_ctx_with_meta context did"
        );
    }

    // amend (#5057 reviewer_comprehensive, suggestion #1 — test-coverage-gap):
    // the test above locks the determinacy-predicate divergence for the PARAM
    // default_expr branch only. `elaborate_child_lets_only`'s let-eval (:693)
    // wires the identical live `&snapshot.values` determinacy capability, but
    // had no site-specific lock — coverage was only transitive via the shared
    // `eval_child_expr` helper. This fixture mirrors the param test above,
    // substituting a `let` binding for a param default: `S.n` (the
    // always-present, always-`Determined` root cell — a stable absolute
    // reference regardless of recursion depth) is referenced from a
    // `let ready = determined(S.n)` cell instead of a param default, proving
    // the let-binding path also resolves `Bool(true)` post-migration rather
    // than degrading to `Value::Undef` as the pre-migration bare
    // `eval_ctx_with_meta` context did on this path.
    #[test]
    fn elaborate_child_lets_only_let_expr_resolves_determinacy_predicate() {
        use reify_core::{ModulePath, Type};
        use reify_ir::{BinOp, DeterminacyPredicateKind};
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{
            CompiledModuleBuilder, TopologyTemplateBuilder, binop, gt, literal, value_ref_typed,
        };

        let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
        let n_minus_1 = binop(
            BinOp::Sub,
            value_ref_typed("S", "n", Type::Int),
            literal(Value::Int(1)),
        );
        let ready_let_expr = reify_ir::CompiledExpr::determinacy_predicate(
            DeterminacyPredicateKind::Determined,
            ValueCellId::new("S", "n"),
        );

        let template = TopologyTemplateBuilder::new("S")
            .param(
                "S",
                "n",
                Type::Int,
                Some(reify_ir::CompiledExpr::literal(Value::Int(1), Type::Int)),
            )
            .let_binding("S", "ready", Type::Bool, ready_let_expr)
            .is_recursive(true)
            .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();
        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        let result = engine.eval(&module);

        let child_ready = ValueCellId::new("S.child", "ready");
        assert_eq!(
            result.values.get(&child_ready),
            Some(&Value::Bool(true)),
            "S.child.ready = let ready = determined(S.n) should resolve Bool(true) \
             via the snapshot-wired determinacy cell_eval_ctx now threads through \
             the let-binding branch in elaborate_child_lets_only, not degrade to \
             Value::Undef as the pre-migration bare eval_ctx_with_meta context did"
        );
    }
}
