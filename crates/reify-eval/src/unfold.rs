// Recursive sub-component unfolding — unfold_recursive_sub and elaborate_child_* functions.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use reify_compiler::{TopologyTemplate, ValueCellKind, find_template};
use reify_types::{
    CompiledFunction, DeterminacyState, Diagnostic, Value, ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, NodeId};
use crate::deps::{DependencyTrace, extract_dependency_trace, take_trace};
use crate::dirty::topological_sort;
use crate::eval_ctx_with_meta;
use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};
use crate::snapshot::Snapshot;

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
    let concrete_args: Vec<(String, reify_types::CompiledExpr)> = sub
        .args
        .iter()
        .map(|(name, arg_expr)| {
            let v = reify_expr::eval_expr(
                arg_expr,
                &eval_ctx_with_meta(&local_values, functions, meta_map),
            );
            let ty = arg_expr.result_type.clone();
            (name.clone(), reify_types::CompiledExpr::literal(v, ty))
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
    args: &[(String, reify_types::CompiledExpr)],
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
    args: &[(String, reify_types::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
) -> ValueMap {
    let mut child_values = ValueMap::new();

    for cell in &child_template.value_cells {
        if cell.kind != ValueCellKind::Param {
            continue;
        }

        let member = &cell.id.member;

        let val = if let Some((_name, arg_expr)) = args.iter().find(|(name, _)| name == member) {
            reify_expr::eval_expr(arg_expr, &eval_ctx_with_meta(values, functions, meta_map))
        } else if let Some(ref default_expr) = cell.default_expr {
            reify_expr::eval_expr(
                default_expr,
                &eval_ctx_with_meta(&child_values, functions, meta_map),
            )
        } else {
            Value::Undef
        };

        child_values.insert(cell.id.clone(), val.clone());

        let scoped_id = ValueCellId::new(scoped_entity, member);
        let node_id = NodeId::Value(scoped_id.clone());
        let start = Instant::now();
        journal.record(EvalEvent {
            timestamp: start,
            node_id: node_id.clone(),
            kind: EventKind::Started,
            version: VersionId(version_id),
            payload: None,
        });

        values.insert(scoped_id.clone(), val.clone());
        snapshot.values.insert(
            scoped_id.clone(),
            (val.clone(), DeterminacyState::Determined),
        );

        let trace = DependencyTrace::default();
        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
        let outcome =
            cache.record_evaluation(node_id.clone(), cached_result, VersionId(version_id), trace);

        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id,
            kind: EventKind::Completed { outcome },
            version: VersionId(version_id),
            payload: Some(EventPayload::Duration(start.elapsed())),
        });
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
    let child_let_cells: HashMap<NodeId, &reify_types::CompiledExpr> = child_template
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

        let val = reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(&child_values, functions, meta_map),
        );
        child_values.insert(child_cell_id.clone(), val.clone());

        let scoped_id = ValueCellId::new(scoped_entity, member);
        let node_id = NodeId::Value(scoped_id.clone());
        let start = Instant::now();
        journal.record(EvalEvent {
            timestamp: start,
            node_id: node_id.clone(),
            kind: EventKind::Started,
            version: VersionId(version_id),
            payload: None,
        });

        values.insert(scoped_id.clone(), val.clone());
        snapshot.values.insert(
            scoped_id.clone(),
            (val.clone(), DeterminacyState::Determined),
        );

        // sorted_child_lets and child_let_traces are built from the same key set, so remove() cannot fail.
        let trace = take_trace(
            &mut child_let_traces,
            &child_node_id,
            "sorted_child_lets",
            "child_let_traces",
        );
        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
        let outcome =
            cache.record_evaluation(node_id.clone(), cached_result, VersionId(version_id), trace);

        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id,
            kind: EventKind::Completed { outcome },
            version: VersionId(version_id),
            payload: Some(EventPayload::Duration(start.elapsed())),
        });
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
}
