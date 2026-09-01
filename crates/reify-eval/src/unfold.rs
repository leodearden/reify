// Recursive sub-component unfolding — unfold_recursive_sub and elaborate_child_* functions.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};

use reify_compiler::{CompiledModule, TopologyTemplate, ValueCellKind, find_template};
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

/// Single source of truth for sub-component template resolution: the user
/// module's templates first (module definitions shadow the prelude), then the
/// engine's compiled stdlib prelude modules.
///
/// Stdlib occurrence/structure templates (e.g. `STLOutput` and `DisplayStyle`
/// from `stdlib/io.ri`) live in `Engine::prelude` and are deliberately NOT
/// merged into `CompiledModule::templates`, so a module-only lookup reports
/// them as unknown (io-export δ / esc-4287-15).
///
/// EVERY resolver on this file's recursion paths must go through here, not bare
/// [`find_template`]. esc-5360-9: the instance-scope nesting recursion
/// originally resolved module-only while the top-level sub-elaboration loop in
/// `engine_eval.rs` resolved with the prelude fallback. That asymmetry both
/// (a) silently declined to materialise `Parent.m.o` for a prelude-typed
/// `sub o` that DOES elaborate at template scope — reintroducing, for every
/// prelude type, the exact instance/template gap task 5360 exists to close —
/// and (b) recorded the skip as "target structure not found in this module",
/// so `report_unresolvable_nested_reads` raised a NEW error naming a structure
/// that is in fact perfectly resolvable.
///
/// `engine_eval::find_template_with_prelude` is the `&CompiledModule` adapter
/// over this function; it delegates here rather than duplicating the rule.
pub(crate) fn find_template_in_scope<'a>(
    templates: &'a [TopologyTemplate],
    prelude: &'a [CompiledModule],
    name: &str,
) -> Option<&'a TopologyTemplate> {
    find_template(templates, name).or_else(|| {
        prelude
            .iter()
            .find_map(|pm| find_template(&pm.templates, name))
    })
}

/// No-op [`ContainmentQuery`]: containment stays `None` on this
/// recursive-unfold path, matching its pre-migration behaviour (restricted-
/// field samples resolve to `Value::Undef`, same as before `cell_eval_ctx`
/// existed here). Wiring the live `Engine` instead would require threading
/// `&self` through these free functions, conflicting with the `&mut
/// journal`/`&mut cache` borrows already split out at the engine_eval.rs
/// call sites.
///
/// Determinacy is *not* a no-op: `cell_eval_ctx` requires it, so all three
/// call sites below now wire the live `&snapshot.values`. This is an
/// intentional semantic change, not pure behaviour-preservation — a child
/// param/let expression containing a `DeterminacyPredicate` (e.g.
/// `determined(x)`) now resolves against the snapshot instead of
/// unconditionally degrading to `Value::Undef`. No existing fixture or
/// shipped design relies on the old degradation (checked repo-wide; see the
/// divergence tests in this file's `tests` module, which lock the new
/// behaviour); guard-position `determined()` is unaffected, since the guard
/// eval (`unfold_recursive_sub`) stays out of scope on `eval_ctx_with_meta`.
/// Required-capability rationale: `docs/prds/v0_6/eval-cell-commit-substrate.md`
/// §2.5.
///
/// Scope caveat (param `default_expr`, let-binding call sites — see
/// `eval_child_expr`): there, `value_map` is the recursion-local
/// `child_values` while `determinacy` stays the globally-scoped
/// `&snapshot.values`, so a compound expression mixing a plain reference and
/// a `determined()` predicate on the same recursion-local name (e.g. `n`)
/// resolves them against different cells — only absolute-root references
/// (e.g. `S.n`) are guaranteed consistent. The arg branch is unaffected
/// (`value_map` is already the global `values` there). Fixing this would
/// mean projecting a template-scoped determinacy view alongside
/// `child_values` instead of the global snapshot — future work, out of
/// scope here.
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
    prelude: &'t [CompiledModule],
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
    //
    // task #6662 deliberately does NOT apply `resolve_optimized_instance_cell`
    // here or to the guard eval above: both are ctor-ARG / guard pre-evaluation
    // in the PARENT's scope, not cell evaluation in the child's, so the
    // helper's read comparison (which assumes both maps are keyed in the child
    // template's scope) would compare the wrong maps. The cells these args feed
    // reach the authoritative `@optimized` resolution downstream, via
    // `elaborate_child_lets_only`.
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
        diagnostics,
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
        let next_child_template = match find_template_in_scope(
            templates,
            prelude,
            &next_sub.structure_name,
        ) {
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
            prelude,
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
        prelude,
        diagnostics,
    );
}

/// Elaborate a child instance — and its plain nested subtree — into the
/// values/snapshot maps.
///
/// This handles both non-collection subs (single instance) and individual
/// collection sub instances (called in a loop for each index).
///
/// For non-recursive subs both phases run atomically (params then lets).
/// For recursive subs, use `elaborate_child_params_only` + `elaborate_child_lets_only`
/// to allow leaves-first ordering (recurse between the two phases).
///
/// # Instance-nested sub-components (task 5360)
///
/// Between the two phases this also elaborates the child template's *own*
/// PLAIN sub-components at the nested scope `{scoped_entity}.{sub.name}`,
/// leaves-first and interleaved with the child's own lets in dependency order
/// (see `elaborate_child_instance_nested`), then hands their names to
/// `elaborate_child_lets_only` so its
/// projection BFS remaps the freshly-committed global cells
/// `{scoped_entity}.{sub}.{member}` onto the template-scoped key
/// `{child_template.name}.{sub}.{member}` — exactly what a cross-sub read
/// `self.<sub>.<member>` compiles to. Without this, an instance-scope let such
/// as `Parent.m.relay = self.k.off` had no `Mid.k.off` entry to read and
/// silently resolved to `Value::Undef`.
///
/// Only PLAIN subs recurse. Collection subs (`is_collection`) and keyed subs
/// (non-empty `keyed_members`) need `__list_`/indexed/per-key scoping that is
/// not modelled at instance nesting; guarded subs belong to recursive
/// templates, which `unfold_recursive_sub` handles at template scope. Anything
/// skipped is surfaced by [`report_unresolvable_nested_reads`], never dropped
/// quietly: a child let left `Undef` *because* of a skip gets an error
/// diagnostic naming both it and the nested cell it could not read.
///
/// Termination does NOT rest on structural acyclicity — plain sub nesting is
/// *not* guaranteed to be a DAG. The compiler admits both self-recursive
/// (`structure Node { sub child : Node }`) and mutually-recursive
/// (`A { sub b : B }` / `B { sub a : A }`) UNGUARDED plain subs; it reports
/// "recursive sub has no termination condition" as a COMPILE-time diagnostic
/// but still emits the templates, so eval sees a cyclic sub graph. Termination
/// therefore comes from the ancestor-chain cycle guard in
/// `elaborate_child_instance_nested`: a nested sub whose target template is
/// already being elaborated on the current chain is skipped. (The surfacing
/// walk guards the same cycles with its own `depth > templates.len()` bound —
/// see `tests/sub_placement_surfacing.rs`.)
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_child_instance<'t>(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &'t TopologyTemplate,
    scoped_entity: &str,
    args: &[(String, reify_ir::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &'t [TopologyTemplate],
    prelude: &'t [CompiledModule],
) {
    elaborate_child_instance_nested(
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
        diagnostics,
        templates,
        prelude,
        &mut Vec::new(),
    );
}

/// Recursion body of [`elaborate_child_instance`], carrying the extra
/// `ancestors` chain the public entry point seeds empty.
///
/// `ancestors` holds the template names currently being elaborated on this
/// nesting chain, innermost last. A nested sub whose target template already
/// appears there would re-enter a cycle, so it is skipped — that check is what
/// makes the recursion terminate on the compiler's admitted cyclic sub graphs.
/// Cutting at the exact cycle (rather than at a fixed depth bound) keeps the
/// materialised entity set minimal: a self-recursive `Node` still elaborates
/// exactly `Node.child` and stops, matching pre-task-5360 behaviour.
///
/// Depth is bounded by the number of distinct templates, since each chain is a
/// simple path in the template graph. Total work is that times the per-level
/// sub fan-out; a wide *acyclic* nest could in principle be exponential in
/// depth, but no such shape exists in-tree and bounding it would need a node
/// budget like `unfold_recursive_sub`'s.
///
/// # Phase 1.5 ordering (task 5360 round 2)
///
/// Phase 1.5 is NOT a declaration-order loop over `sub_components`: the child's
/// own `let` cells and its plain nested subs form ONE dependency-ordered walk
/// ([`build_phase15_nodes`] + [`phase15_node_traces`] + [`topological_sort`]).
/// A nested sub's constructor arg is compiled in `child_template`'s scope, so it
/// may read anything that scope offers — a param (already in phase 1's
/// `child_values`), a `let` of the child, or a sibling sub's member via
/// `self.<sub>.<member>`. Only the param case needs no ordering; the other two
/// must be produced *before* the consuming sub's args are evaluated, or the read
/// falls through to the TEMPLATE-scope entry in the global map and silently
/// substitutes the template default for this instance's value.
///
/// The walk therefore threads a running `overlay` (template-scoped keys, seeded
/// with phase 1's params): a LET node evaluates into it as a SCRATCH value, and
/// a SUB node pre-evaluates its args against it, recurses, then projects its
/// freshly-committed cells back into it for later nodes. Ordering by dependency
/// rather than by declaration also makes the result order-insensitive: a sub
/// declared before the sibling it reads resolves identically.
///
/// A SUB node also commits the COLLAPSED instance value — the SIR-α
/// `Value::StructureInstance` at `{scoped_entity}`/`{sub}` and its `__self`
/// alias at `{scoped_entity}.{sub}`/`__self` — mirroring what `engine_eval.rs`
/// commits for the same sub at template scope. The per-member cells serve
/// `self.<sub>.<member>`; the collapsed pair serves every use of the sub as a
/// WHOLE value, which is a distinct compiler lowering
/// (`resolve_non_collection_sub_to_structure_ref`). Both are also handed to
/// phase 2, whose projection BFS structurally cannot derive them: it walks
/// declared `value_cells`, and neither a sub name nor `__self` is one.
///
/// When the walk CANNOT be ordered — a dependency cycle among phase-1.5 nodes,
/// which [`topological_sort`] (Kahn) reports by silently omitting the members —
/// the dropped nodes are appended back in declaration order so evaluation still
/// terminates and produces values. That fallback is unconditional, but it is not
/// silent: a cycle whose MEMBERS include a nested SUB is also diagnosed here,
/// because it is the class phase 1.5 uniquely owns (phase 2's graph holds only
/// the child's own lets, so a cycle routed through a sub boundary is invisible
/// to it). A cycle confined to LET nodes is deliberately left to
/// [`elaborate_child_lets_only`] at instance scope and to `engine_eval.rs` at
/// template scope, which already report it — phase 1.5 stays quiet there rather
/// than triple-reporting one defect. So no phase-1.5 cycle member is silently
/// tolerated: it is either diagnosed here or diagnosed by the phase that owns
/// it.
///
/// Membership is computed by [`phase15_cycle_members`] rather than read off the
/// nodes Kahn omitted, because the omitted set also holds everything
/// transitively downstream of the cycle. Both the ownership gate above and the
/// participant list in the diagnostic use the computed members, so a bystander
/// neither triggers a duplicate report nor gets named as a participant.
///
/// Phase 2 ([`elaborate_child_lets_only`]) is deliberately NOT restructured — it
/// still owns the authoritative let commits, and the recursive path via
/// `unfold_recursive_sub` reaches it unchanged.
#[allow(clippy::too_many_arguments)]
fn elaborate_child_instance_nested<'t>(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &'t TopologyTemplate,
    scoped_entity: &str,
    args: &[(String, reify_ir::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &'t [TopologyTemplate],
    prelude: &'t [CompiledModule],
    ancestors: &mut Vec<&'t str>,
) {
    let mut child_values = elaborate_child_params_only(
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
        diagnostics,
    );

    // Phase 1.5 (leaves-first, dependency-ordered): elaborate the child
    // template's own plain subs at nested instance scope, so their cells exist
    // before this instance's lets are evaluated and can be projected into the
    // let context below.
    //
    // `runtime_sink`/`containment` mirror the other two phases: the sink's
    // contents are discarded (this path never surfaced eval-time diagnostics)
    // and containment stays a no-op, matching `NoContainment`'s rationale.
    let arg_runtime_sink = RefCell::new(Vec::new());
    let arg_containment = NoContainment;
    // One `@optimized`-name index for this phase's whole walk (amend #6662
    // suggestion #3). Lazily built on the first `UserFunctionCall` probe, so an
    // instance with no function-call lets pays nothing.
    let optimized_names: OptimizedNameIndex = OptimizedNameIndex::new();
    // Every sub this phase declines to descend into, with the reason. Handed to
    // `report_unresolvable_nested_reads` below so a child let left `Undef` by a
    // skip is named rather than silently dropped. All four skip arms feed it,
    // the cycle cut included. Classified in DECLARATION order so the diagnostic
    // prose is order-stable.
    let mut skipped_subs: Vec<(&str, &'static str)> = Vec::new();
    ancestors.push(child_template.name.as_str());
    let mut nestable: Vec<(&'t reify_compiler::SubComponentDecl, &'t TopologyTemplate)> = Vec::new();
    for sub in &child_template.sub_components {
        if !is_plain_nestable_sub(sub) {
            skipped_subs.push((sub.name.as_str(), skip_reason_for_shape(sub)));
            continue;
        }
        // Module-first, then the stdlib prelude — the SAME rule the top-level
        // sub-elaboration loop in `engine_eval.rs` applies (esc-5360-9). A bare
        // `find_template(templates, ..)` here would miss every prelude type,
        // leaving `Parent.m.<prelude-typed sub>` unelaborated while `Mid.<same
        // sub>` elaborates at template scope, and would then mis-report the
        // resolvable name as "not found in this module" below.
        let Some(nested_template) = find_template_in_scope(templates, prelude, &sub.structure_name)
        else {
            skipped_subs.push((
                sub.name.as_str(),
                "target structure not found in this module or the stdlib prelude",
            ));
            continue;
        };
        // Cycle guard — see this function's doc comment. Re-entering a
        // template already on the chain would recurse forever on the
        // compiler-admitted unguarded cyclic sub graphs (`Node { sub child :
        // Node }`, `A { sub b : B }` / `B { sub a : A }`), which is a stack
        // overflow, not a hang.
        if ancestors.contains(&nested_template.name.as_str()) {
            skipped_subs.push((sub.name.as_str(), "cyclic sub nesting cut"));
            continue;
        }
        nestable.push((sub, nested_template));
    }

    let (nodes, declaration_order) = build_phase15_nodes(child_template, &nestable);
    let node_ids: HashSet<NodeId> = nodes.keys().cloned().collect();
    let node_traces = phase15_node_traces(child_template, &nodes, &nestable);
    let sorted = topological_sort(&node_ids, &node_traces);
    // `topological_sort` (Kahn) reports a cycle by silently omitting its
    // members. Append them back in declaration order so evaluation still
    // terminates and produces values, and make sure the cycle is reported by
    // SOMEBODY: a cycle here leaves the chain `Undef`, and an unreported `Undef`
    // is exactly the silent-default class this path exists to eliminate. Which
    // phase does the reporting depends on the cycle's shape — see the ownership
    // note on the gate below.
    let mut walk_order = sorted;
    if walk_order.len() < node_ids.len() {
        let placed: HashSet<&NodeId> = walk_order.iter().collect();
        let dropped: Vec<NodeId> = declaration_order
            .iter()
            .filter(|nid| !placed.contains(nid))
            .cloned()
            .collect();

        // `dropped` is NOT the cycle — it is the cycle PLUS everything
        // transitively downstream of it, because Kahn omits a node whose
        // dependency it could never place. Both the gate and the diagnostic
        // below ask "which nodes are on the cycle?", so both must key on
        // computed membership; keying on `dropped` fires the gate for a merely
        // downstream sub and names bystanders. Pinned by
        // `nested_pure_let_cycle_with_downstream_sub_is_reported_once_per_scope`
        // and `nested_sub_cycle_diagnostic_names_only_real_participants`
        // (tests/harness_engine/nested_sub_derived_let_e2e.rs);
        // see [`phase15_cycle_members`] for the measured example.
        let cycle_members = phase15_cycle_members(&dropped, &node_traces);

        // ONE OWNER PER CYCLE CLASS. Pinned by
        // `nested_pure_let_cycle_is_reported_once_per_scope`.
        //
        // A cycle confined entirely to LET nodes belongs to phase 2: it is
        // already reported at instance scope by `elaborate_child_lets_only` and
        // at template scope by `engine_eval.rs`. Phase 1.5's node set includes
        // every let with a `default_expr`, so such a cycle is dropped by BOTH
        // topological sorts and reporting it here would be a third diagnostic
        // for one defect — spam that degrades the signal this path exists to
        // sharpen. That holds however far the fallout spreads: a sub merely
        // DOWNSTREAM of a pure-let cycle leaves this phase silent. Its arg is
        // still `Undef`, but that `Undef` is not unexplained — its root cause
        // is named at both scopes by the phase that owns it.
        //
        // A cycle whose MEMBERS include a SUB node is the class phase 1.5
        // uniquely owns: phase 2's graph holds only the child's own lets, so a
        // cycle routed through a sub boundary is structurally invisible to it
        // (in the repro, `relay` reads `Mid.k.off`, which is not a let of
        // `Mid`). Nobody else can see it, so if this gate stops firing that
        // class goes back to silent `Undef`.
        let touches_sub = cycle_members
            .iter()
            .any(|nid| matches!(nodes.get(nid), Some(Phase15Node::Sub { .. })));

        if touches_sub {
            // Render each participant so it names a cell a reader can go find.
            // A LET node renders as its member, matching the let-only detector's
            // prose. A SUB node CANNOT: `PHASE15_SUB_NODE_MEMBER` is the EMPTY
            // string, so rendering it that way would print a dangling dot naming
            // nothing. Render it from `sub.name` — the same string
            // `phase15_sub_node_key` puts in the entity suffix, taken from the
            // node directly rather than by stripping the `{child_template}.`
            // prefix back off the key.
            let mut participants: Vec<String> = cycle_members
                .iter()
                .filter_map(|nid| match nodes.get(nid) {
                    Some(Phase15Node::Let { key, .. }) => Some(key.member.clone()),
                    Some(Phase15Node::Sub { sub, .. }) => Some(format!("sub {}", sub.name)),
                    None => None,
                })
                .collect();
            participants.sort();
            diagnostics.push(Diagnostic::error(format!(
                "circular dependency among nested-sub arguments and let bindings in \
                 template {} (entity {}): [{}]",
                child_template.name,
                scoped_entity,
                participants.join(", "),
            )));
        }

        // The fallback stays UNCONDITIONAL — only the diagnostic is gated. A
        // pure let cycle must still terminate and produce values exactly as it
        // did before this step.
        walk_order.extend(dropped);
    }

    // The running overlay: template-scoped keys visible to this instance's
    // nested-arg evaluation, seeded with phase 1's params and extended as the
    // walk goes. None of these SCRATCH LET values reach `child_values` — phase 2
    // must recompute its lets against its own BFS-enriched map, and seeding it
    // with them would mask a phase-2 regression. (The collapsed instance values
    // gathered into `collapsed` are the one deliberate exception; the hand-off
    // below says why they are not the same thing.)
    let mut overlay = child_values.clone();
    let mut elaborated: HashSet<&str> = HashSet::new();
    // Collapsed nested-sub instance values (and their `__self` aliases), keyed
    // template-scoped, held aside for the phase-2 hand-off below.
    let mut collapsed: Vec<(ValueCellId, Value)> = Vec::new();

    for node_id in &walk_order {
        // `walk_order` only ever contains keys of `nodes` (topological_sort
        // returns a subset of its input set, and the fallback is drawn from
        // `declaration_order`), so a miss is an unreachable construction bug:
        // loud in debug/test builds, a skip rather than a panic in release.
        debug_assert!(
            nodes.contains_key(node_id),
            "phase 1.5 walk order contains a non-node {:?} (entity {})",
            node_id,
            scoped_entity,
        );
        let Some(node) = nodes.get(node_id) else {
            continue;
        };
        match node {
            // A LET node computes a SCRATCH value only: it is inserted into the
            // overlay so later nodes' args can read it, and deliberately NOT
            // committed to `values`/snapshot/journal/cache. Phase 2 still owns
            // the authoritative write; double-committing would duplicate
            // journal entries. Evaluating against the overlay alone (rather
            // than the overlay over the global map) matches phase 2's own
            // let-eval scope, so the scratch value equals the value phase 2
            // will commit.
            Phase15Node::Let { key, expr } => {
                // task #6662: the invariant above covers `@optimized` cells
                // too, and ONLY because this arm resolves them the same way
                // phase 2 does. Phase 2 commits the template's dispatched value
                // for a reusable `@optimized` cell; if this arm body-inlined
                // instead, the overlay would hold the `.ri` sentinel while the
                // committed cell held the real value — and since the overlay is
                // exactly what nested subs' ctor args are pre-evaluated
                // against, a `sub sink = Leaf(seed: computed)` would silently
                // receive the stale sentinel. That is a quieter instance of the
                // defect #6662 exists to fix, which is why it routes through
                // the SAME helper rather than a second copy of the rule.
                //
                // No diagnostic is emitted here: phase 2 is the authoritative
                // site and sees the same cell, so reporting from both would
                // double every warning. That justification holds ONLY because
                // the two sites reach the same decision on the same cell — see
                // `resolve_optimized_instance_cell`'s "Why the two instance maps
                // reach the SAME decision" for the measured argument, and the
                // parity tests it names.
                //
                // `node_traces` cannot supply `reads` here: it NORMALISES each
                // read onto a phase-1.5 node key (dropping param reads, remapping
                // cross-sub reads onto their sub node) so `topological_sort` can
                // see the edges, which is the wrong set for a value comparison.
                // The raw trace is what the gate needs — and it is passed as a
                // CLOSURE so the tree walk happens only on the `@optimized` path
                // (amend suggestion #2); every other scratch let now pays one
                // hash lookup instead of a full `extract_dependency_trace`.
                let v = match resolve_optimized_instance_cell(
                    expr,
                    functions,
                    &optimized_names,
                    child_template,
                    &key.member,
                    || extract_dependency_trace(expr).reads,
                    &overlay,
                    values,
                ) {
                    OptimizedInstanceResolution::Reuse(v) => v,
                    OptimizedInstanceResolution::NotOptimized
                    | OptimizedInstanceResolution::Unreusable { .. } => eval_child_expr(
                        &overlay,
                        expr,
                        functions,
                        meta_map,
                        &snapshot.values,
                        &arg_runtime_sink,
                        &arg_containment,
                    ),
                };
                overlay.insert(key.clone(), v);
            }
            // Pre-evaluate the nested sub's constructor args HERE, in this
            // instance's own scope, and pass them down as literals — mirroring
            // `unfold_recursive_sub`'s `concrete_args` above. Without this the
            // nested `elaborate_child_params_only` would evaluate the raw arg
            // expressions against the global `values` map instead.
            //
            // That matters because arg expressions are compiled in
            // `child_template`'s scope, so they reference
            // `{child_template}.{member}` keys. The global map holds the
            // TEMPLATE DEFAULT for those keys (`Mid.scale = 1mm`), whereas the
            // overlay holds THIS INSTANCE's value (`Parent.m.scale = 30mm`,
            // keyed `Mid.scale`). Reading the global map would silently drop
            // the parent's override and yield a wrong value rather than an
            // `Undef` — nothing downstream would catch it.
            //
            // The context overlays the running overlay ON TOP OF the global map
            // rather than replacing it, so the instance wins wherever both hold
            // a key while every reference the global map alone could already
            // resolve still resolves. `ValueMap` is an `im::HashMap`, so the
            // clone is an O(1) structural-sharing clone, not a deep copy.
            Phase15Node::Sub { sub, template } => {
                let mut arg_scope = values.clone();
                for (id, v) in overlay.iter() {
                    arg_scope.insert(id.clone(), v.clone());
                }
                let concrete_args: Vec<(String, reify_ir::CompiledExpr)> = sub
                    .args
                    .iter()
                    .map(|(name, arg_expr)| {
                        let v = eval_child_expr(
                            &arg_scope,
                            arg_expr,
                            functions,
                            meta_map,
                            &snapshot.values,
                            &arg_runtime_sink,
                            &arg_containment,
                        );
                        (
                            name.clone(),
                            reify_ir::CompiledExpr::literal(v, arg_expr.result_type.clone()),
                        )
                    })
                    .collect();

                let nested_entity = format!("{}.{}", scoped_entity, sub.name);
                elaborate_child_instance_nested(
                    values,
                    snapshot,
                    functions,
                    journal,
                    cache,
                    version_id,
                    // `template` is `&&'t StructureTemplate` here (bound by
                    // ref-pattern off the `nodes` lookup); auto-deref reborrows
                    // it to the `&StructureTemplate` the callee takes. Writing
                    // `*template` is a `clippy::explicit_auto_deref` failure
                    // under `-D warnings`.
                    template,
                    &nested_entity,
                    &concrete_args,
                    meta_map,
                    diagnostics,
                    templates,
                    prelude,
                    ancestors,
                );

                // Project the freshly-committed nested cells into the overlay
                // under the template-scoped key a cross-sub read
                // `self.<sub>.<member>` compiles to — the same remap phase 2's
                // BFS performs, applied incrementally so LATER nodes in this
                // walk see it.
                let projected_entity = format!("{}.{}", child_template.name, sub.name);
                for cell in &template.value_cells {
                    let committed = ValueCellId::new(&nested_entity, &cell.id.member);
                    if let Some(v) = values.get(&committed) {
                        overlay.insert(
                            ValueCellId::new(&projected_entity, &cell.id.member),
                            v.clone(),
                        );
                    }
                }

                // The COLLAPSED instance value, mirroring the SIR-α block
                // `engine_eval.rs` runs for a TEMPLATE-scope plain sub (task
                // 3540 + the task 3941 ζ `__self` alias). The per-member cells
                // projected above serve `self.<sub>.<member>`; they do NOT serve
                // a use of the sub as a WHOLE value — a bare `k`, a `self.k`
                // member-access chain, or the receiver of `k.(Trait::method)()`
                // — which `resolve_non_collection_sub_to_structure_ref`
                // (expr.rs) lowers to `ValueRef(ValueCellId("{tmpl}.{sub}",
                // "__self"))`. Without this block those resolved at template
                // scope and were silently `Undef` at instance scope: the same
                // instance/template asymmetry this path exists to close, left
                // open for every non-scalar read. Fields are gathered from the
                // just-committed nested cells in declaration order, exactly as
                // the template-scope block gathers them.
                let mut fields: PersistentMap<String, Value> = PersistentMap::new();
                for cell in &template.value_cells {
                    if let Some(v) = values.get(&ValueCellId::new(&nested_entity, &cell.id.member)) {
                        fields.insert(cell.id.member.clone(), v.clone());
                    }
                }
                let si = Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
                    type_id: reify_ir::StructureTypeId(0),
                    type_name: sub.structure_name.clone(),
                    version: template.version(),
                    fields,
                }));
                // Direct inserts, not `commit_cell_result`: the template-scope
                // block does the same. This is a derived VIEW of cells already
                // committed with their own provenance, so giving it a journal
                // entry and a cache record of its own would double-count the
                // nested elaboration.
                let sub_id = ValueCellId::new(scoped_entity, &sub.name);
                values.insert(sub_id.clone(), si.clone());
                snapshot
                    .values
                    .insert(sub_id, (si.clone(), DeterminacyState::Determined));
                let self_alias_id = ValueCellId::new(&nested_entity, "__self");
                values.insert(self_alias_id.clone(), si.clone());
                snapshot
                    .values
                    .insert(self_alias_id, (si.clone(), DeterminacyState::Determined));

                // Both keys, template-scoped, for the two consumers that cannot
                // reach the global map: the overlay (later phase-1.5 nodes'
                // ARGS) and `collapsed` (phase 2's LET scope — see the note at
                // its application site below).
                let collapsed_key = ValueCellId::new(&child_template.name, &sub.name);
                let collapsed_alias = ValueCellId::new(&projected_entity, "__self");
                overlay.insert(collapsed_key.clone(), si.clone());
                overlay.insert(collapsed_alias.clone(), si.clone());
                collapsed.push((collapsed_key, si.clone()));
                collapsed.push((collapsed_alias, si));

                elaborated.insert(sub.name.as_str());
            }
        }
    }
    ancestors.pop();

    // Declaration order, not walk order: phase 2 only uses these as BFS seeds,
    // and keeping the original order keeps its (unchanged) behaviour and any
    // seed diagnostics byte-identical.
    let nested_sub_names: Vec<&str> = child_template
        .sub_components
        .iter()
        .map(|sub| sub.name.as_str())
        .filter(|name| elaborated.contains(name))
        .collect();

    // Hand phase 2 the collapsed instance values. This is the ONE thing the
    // "child_values stays untouched" rule above does not cover, and the
    // exception is structural rather than a convenience: phase 2's projection
    // BFS walks `entity_template.value_cells`, and NEITHER key here is a
    // declared value cell — `{tmpl}.{sub}` is a sub, and `__self` is a compiler
    // alias — so the BFS can never produce them however far it walks. Without
    // this a let reading the sub as a whole value (`let whole = self.k`) sees no
    // `Mid.k.__self` in its scope and lands `Undef` at instance scope while
    // resolving fine at template scope.
    //
    // These are NOT let scratch values: each is a derived view of cells phase 2
    // itself has no part in producing, so seeding them cannot mask a phase-2
    // let regression the way seeding the overlay's let values would.
    for (key, value) in collapsed {
        child_values.insert(key, value);
    }

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
        &nested_sub_names,
        templates,
        prelude,
        diagnostics,
    );

    // Never-silent-undef: any child let the loop above knowingly starved must
    // say so. Gated on a non-empty skip list so the common all-plain case pays
    // nothing.
    if !skipped_subs.is_empty() {
        report_unresolvable_nested_reads(
            values,
            child_template,
            scoped_entity,
            &skipped_subs,
            diagnostics,
        );
    }
}

/// One orderable unit of phase 1.5: either a `let` of the child template or one
/// of its plain nested subs. Both kinds go into a single dependency order, which
/// is what lets an arg read a let (and a let read an earlier sub) at instance
/// scope — see [`elaborate_child_instance_nested`].
enum Phase15Node<'t> {
    /// A `let` cell of the child template. `key` is the cell's own
    /// template-scoped id — the key a same-template read compiles to, and the
    /// key phase 1 already uses for params, so the overlay stays one namespace.
    Let {
        key: ValueCellId,
        expr: &'t reify_ir::CompiledExpr,
    },
    /// A plain nested sub of the child template, with its already-resolved
    /// target template.
    Sub {
        sub: &'t reify_compiler::SubComponentDecl,
        template: &'t TopologyTemplate,
    },
}

/// Member name reserved for phase-1.5 SUB nodes.
///
/// [`topological_sort`] orders `NodeId`s and derives its edges from
/// `DependencyTrace::reads`, which are `ValueCellId`s — so a sub has to be
/// addressable as one. A cross-sub read carries a real member
/// (`{tmpl}.{sub}`/`off`), and every member of a real cell is an identifier, so
/// the empty member cannot collide with any read or with any let node key: it
/// names the sub as a whole. Reads are normalised onto it by
/// [`phase15_node_traces`].
const PHASE15_SUB_NODE_MEMBER: &str = "";

/// The phase-1.5 node key for `sub` — see [`PHASE15_SUB_NODE_MEMBER`].
fn phase15_sub_node_key(
    child_template: &TopologyTemplate,
    sub: &reify_compiler::SubComponentDecl,
) -> ValueCellId {
    ValueCellId::new(
        format!("{}.{}", child_template.name, sub.name),
        PHASE15_SUB_NODE_MEMBER,
    )
}

/// Build phase 1.5's node set: one node per `let` cell of `child_template` that
/// has a `default_expr`, plus one per already-classified nestable sub.
///
/// Returns the nodes by id and a declaration-order listing of the same ids
/// (lets in `value_cells` order, then subs in `sub_components` order), used as
/// the deterministic fallback order for nodes a dependency cycle keeps out of
/// the topological sort.
fn build_phase15_nodes<'t>(
    child_template: &'t TopologyTemplate,
    nestable: &[(&'t reify_compiler::SubComponentDecl, &'t TopologyTemplate)],
) -> (HashMap<NodeId, Phase15Node<'t>>, Vec<NodeId>) {
    let mut nodes: HashMap<NodeId, Phase15Node<'t>> = HashMap::new();
    let mut declaration_order: Vec<NodeId> = Vec::new();

    for cell in &child_template.value_cells {
        if cell.kind != ValueCellKind::Let {
            continue;
        }
        let Some(expr) = cell.default_expr.as_ref() else {
            continue;
        };
        let node_id = NodeId::Value(cell.id.clone());
        nodes.insert(
            node_id.clone(),
            Phase15Node::Let {
                key: cell.id.clone(),
                expr,
            },
        );
        declaration_order.push(node_id);
    }

    for &(sub, template) in nestable {
        let node_id = NodeId::Value(phase15_sub_node_key(child_template, sub));
        nodes.insert(node_id.clone(), Phase15Node::Sub { sub, template });
        declaration_order.push(node_id);
    }

    (nodes, declaration_order)
}

/// Dependency traces for phase 1.5's node set, with every read normalised onto
/// a node key so [`topological_sort`] can see the edges.
///
/// A read resolves to a node in exactly two ways:
/// - it *is* a let node's cell id (a same-template read of one of this
///   template's own lets), or
/// - its entity is `{child_template}.{sub}` for a node sub — the key a cross-sub
///   read `self.<sub>.<member>` compiles to, whichever member it names.
///
/// Everything else contributes no edge: a read of a PARAM is already resolved in
/// phase 1's `child_values`, a read under a SKIPPED sub has no node to wait for
/// (it is reported by [`report_unresolvable_nested_reads`] / the phase-1.5
/// stale-read guard instead), and reads of other templates or globals are
/// outside this instance entirely.
fn phase15_node_traces<'t>(
    child_template: &'t TopologyTemplate,
    nodes: &HashMap<NodeId, Phase15Node<'t>>,
    nestable: &[(&'t reify_compiler::SubComponentDecl, &'t TopologyTemplate)],
) -> HashMap<NodeId, DependencyTrace> {
    // `{child_template}.{sub}` -> that sub's node cell id.
    let sub_node_keys: HashMap<String, ValueCellId> = nestable
        .iter()
        .map(|&(sub, _)| {
            (
                format!("{}.{}", child_template.name, sub.name),
                phase15_sub_node_key(child_template, sub),
            )
        })
        .collect();

    let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
    for (node_id, node) in nodes {
        let raw: Vec<ValueCellId> = match node {
            Phase15Node::Let { expr, .. } => extract_dependency_trace(expr).reads,
            Phase15Node::Sub { sub, .. } => sub
                .args
                .iter()
                .flat_map(|(_, arg_expr)| extract_dependency_trace(arg_expr).reads)
                .collect(),
        };
        let reads: Vec<ValueCellId> = raw
            .into_iter()
            .filter_map(|read| {
                if nodes.contains_key(&NodeId::Value(read.clone())) {
                    return Some(read);
                }
                sub_node_keys.get(read.entity.as_str()).cloned()
            })
            .collect();
        traces.insert(
            node_id.clone(),
            DependencyTrace {
                reads,
                ..DependencyTrace::default()
            },
        );
    }
    traces
}

/// The subset of `dropped` that actually lies on a dependency cycle.
///
/// [`topological_sort`] (Kahn) reports a cycle by OMISSION, but the set it omits
/// is a strict SUPERSET of the cycle: a node whose dependency sits on a cycle
/// never reaches in-degree 0 either, so everything TRANSITIVELY DOWNSTREAM of
/// the cycle is omitted with it, recursively. Measured on
///
/// ```text
/// structure def Mid {
///     let a = b * 2.0
///     let b = a * 2.0
///     sub k = Kid(w: a)
///     let tail = self.k.off * 3.0
/// }
/// ```
///
/// the omitted set is `[a, b, sub k, tail]` while the only cycle is `{a, b}` —
/// `sub k` and `tail` are bystanders. Reusing the omitted set as if it were the
/// cycle is therefore wrong twice over: it makes the caller's `touches_sub` gate
/// fire on a merely-downstream sub (double-reporting a pure-let cycle that
/// [`elaborate_child_lets_only`] already owns), and it names non-participants in
/// the diagnostic prose. Hence membership is COMPUTED here, not assumed.
///
/// `nid` is on a cycle iff `nid` is reachable from itself along dependency edges
/// that stay inside `dropped`; a self-edge (`n` reads `n`) satisfies that for
/// free. Edges are read from the SAME `traces` map [`topological_sort`] consumed
/// — reusing it, rather than rebuilding or re-normalising the graph, is what
/// guarantees this answers a question about the graph Kahn actually saw.
///
/// `dropped` arrives in declaration order and is filtered in place, so the
/// caller's participant prose stays order-stable.
fn phase15_cycle_members(
    dropped: &[NodeId],
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<NodeId> {
    let inside: HashSet<NodeId> = dropped.iter().cloned().collect();
    dropped
        .iter()
        .filter(|start| {
            let mut seen: HashSet<NodeId> = HashSet::new();
            let mut stack: Vec<NodeId> = vec![(*start).clone()];
            while let Some(node) = stack.pop() {
                let Some(trace) = traces.get(&node) else {
                    continue;
                };
                for read in &trace.reads {
                    let next = NodeId::Value(read.clone());
                    if !inside.contains(&next) {
                        continue;
                    }
                    if next == **start {
                        return true;
                    }
                    if seen.insert(next.clone()) {
                        stack.push(next);
                    }
                }
            }
            false
        })
        .cloned()
        .collect()
}

/// Why [`is_plain_nestable_sub`] rejected `sub` — the shape half of the skip
/// reasons carried in `elaborate_child_instance_nested`'s `skipped_subs`.
///
/// Only called on subs that already failed `is_plain_nestable_sub`, so the
/// final arm is unreachable in practice; it degrades to a generic phrase rather
/// than panicking, since this only ever feeds diagnostic prose.
fn skip_reason_for_shape(sub: &reify_compiler::SubComponentDecl) -> &'static str {
    if sub.is_collection {
        "collection sub"
    } else if !sub.keyed_members.is_empty() {
        "keyed sub"
    } else if sub.guard_state.is_compiled() {
        "guarded sub (recursive-context only)"
    } else {
        "unsupported sub shape"
    }
}

/// Emit an error diagnostic for every child `let` that is `Undef` *because* the
/// nesting recursion declined to elaborate a sub it reads across.
///
/// Without this, an unsupported nesting shape degrades exactly like the task
/// 5360 bug it fixes: the let resolves to `Value::Undef` and nothing anywhere
/// says why. The diagnostic names both ends — the undefined instance cell
/// `{scoped_entity}.{member}` and the nested cell
/// `{scoped_entity}.{skipped}.{target}` it could not read — plus the reason the
/// sub was skipped.
///
/// Deliberately NARROW: a let qualifies only when its read set contains a
/// `ValueCellId` whose entity is exactly `{child_template.name}.{skipped}` —
/// the key a cross-sub read `self.<skipped>.<member>` compiles to (expr.rs
/// sub-member lowering). A blanket "child let is Undef → diagnostic" would fire
/// on legitimately-`Undef` lets (auto params the solver resolves later, genuine
/// upstream undefs), regressing shipped designs. Same-template reads are
/// excluded by construction: their entity is `{child_template.name}` with no
/// nested segment, so it can never equal a `{name}.{sub}` key.
///
/// Residual: reads *deeper* under a skipped sub (entity
/// `{name}.{skipped}.{deeper}`) are not matched. Only one level of
/// `self.<sub>.<member>` lowers to a scoped cell, so no such read reaches here
/// today; broadening to a prefix match would widen the false-positive surface
/// for no in-tree gain.
fn report_unresolvable_nested_reads(
    values: &ValueMap,
    child_template: &TopologyTemplate,
    scoped_entity: &str,
    skipped_subs: &[(&str, &'static str)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // The read-set entity key each skipped sub would have been reached through.
    let skipped_keys: Vec<(String, &str, &'static str)> = skipped_subs
        .iter()
        .map(|(name, reason)| (format!("{}.{}", child_template.name, name), *name, *reason))
        .collect();

    for cell in &child_template.value_cells {
        if cell.kind != ValueCellKind::Let {
            continue;
        }
        let Some(expr) = cell.default_expr.as_ref() else {
            continue;
        };
        // Only lets that actually landed `Undef` at this instance scope. A let
        // that resolved (e.g. a conditional that never took the starved branch)
        // is not a defect.
        let scoped_id = ValueCellId::new(scoped_entity, &cell.id.member);
        if !matches!(values.get(&scoped_id), Some(Value::Undef)) {
            continue;
        }

        let reads = extract_dependency_trace(expr).reads;
        let mut starved: Vec<String> = Vec::new();
        for read in &reads {
            if let Some((_, sub, reason)) =
                skipped_keys.iter().find(|(key, _, _)| *key == read.entity)
            {
                let named = format!("\"{}.{}.{}\" ({})", scoped_entity, sub, read.member, reason);
                if !starved.contains(&named) {
                    starved.push(named);
                }
            }
        }
        if starved.is_empty() {
            continue;
        }

        diagnostics.push(Diagnostic::error(format!(
            "unresolvable nested sub-component read: let \"{}.{}\" is undefined because it reads {}, \
             which structure \"{}\" does not elaborate at instance scope",
            scoped_entity,
            cell.id.member,
            starved.join(" and "),
            child_template.name,
        )));
    }
}

/// True iff `sub` is a PLAIN sub-component — the only shape
/// `elaborate_child_instance` recurses into at nested instance scope.
///
/// Excluded shapes and why (see the `elaborate_child_instance` doc comment):
/// - `is_collection`: needs `__list_`/indexed scoping, unmodelled when nested.
/// - non-empty `keyed_members`: needs per-key `sub["key"]` scoping, ditto.
/// - `guard_state.is_compiled()`: a guarded sub is only meaningful in a
///   recursive context, which `unfold_recursive_sub` owns at template scope.
fn is_plain_nestable_sub(sub: &reify_compiler::SubComponentDecl) -> bool {
    !sub.is_collection && sub.keyed_members.is_empty() && !sub.guard_state.is_compiled()
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
///
/// `value_map` and `determinacy` are not always same-scoped across callers —
/// see the scope caveat in the `NoContainment` doc comment above.
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

/// The set of function NAMES in a `functions` slice that carry an
/// `@optimized` attribute, built at most once per elaboration phase.
///
/// amend (#6662 reviewer_comprehensive, suggestion #3 — efficiency). The
/// pre-filter inside [`optimized_target_of`] was `functions.iter().any(|f|
/// f.name == *name && f.optimized_target.is_some())` — O(|functions|) string
/// comparisons against the STDLIB-INCLUSIVE set, run once per
/// `UserFunctionCall` cell per instance, so its cost scaled as
/// instances × cells × |stdlib functions|. Hoisting the name set turns the
/// per-cell half into one hash lookup.
///
/// A `OnceCell`, not an eagerly-built set, because the build is only worth
/// paying for when the phase actually probes something: a template whose cells
/// contain no `UserFunctionCall` at all never reaches the lookup, and must not
/// pay |functions| for the privilege. Each of the three instance-scope
/// elaboration phases owns one, declared beside the loop it serves — the
/// lifetime `'f` ties the borrowed names to the `functions` slice they were
/// read from, so a cache can never outlive or be paired with a different slice.
pub(crate) type OptimizedNameIndex<'f> = OnceCell<HashSet<&'f str>>;

/// The ONE `@optimized`-cell probe: `Some(target)` iff `expr` is a
/// `UserFunctionCall` that overload-resolves to a function carrying an
/// `@optimized("target")` attribute.
///
/// amend (#6662 reviewer_comprehensive, suggestion #4 — code-reuse). This shape
/// was written twice: here and as `engine_eval.rs::is_optimized_userfn_cell`,
/// which the two TEMPLATE-scope lowering sites gate on. The two must agree
/// exactly — a cell that is `@optimized` at one scope and not the other either
/// re-folds a dispatched value through plain `eval_expr` (clobbering it) or
/// leaves an instance cell body-inlined forever — and a doc comment asserting
/// "a future change to overload resolution moves both together" enforced
/// nothing. This function is the definition; `is_optimized_userfn_cell` is
/// `optimized_target_of(..).is_some()` over the same inputs, and
/// `optimized_probe_agrees_with_engine_eval_is_optimized_userfn_cell` (below)
/// asserts the equivalence across every shape either one can see, so a
/// divergence reds rather than silently splitting.
///
/// `is_optimized_userfn_cell` is not YET re-expressed as a call to this
/// function — `engine_eval.rs` was outside #6662's lock set, a process
/// constraint rather than a technical one (same crate; this fn is already
/// `pub(crate)`). The test above is the binding half meanwhile: it fails if the
/// two ever disagree on any of the shapes it enumerates. The collapse is
/// tracked as a follow-up, filed from #6662's amendment pass as ticket
/// `tkt_0RT53256ABFRYETNAXEJP3EN1T` — not cited as `#NNNN` because the curator
/// assigns the task id asynchronously and an unresolvable cite would be an
/// orphan under the repo's citation convention.
///
/// PERF (suggestion #3). The [`OptimizedNameIndex`] lookup runs BEFORE
/// `find_matching_compiled_function`, whose multi-tier overload scan walks
/// per-candidate param/arg types over the whole `functions` slice (stdlib
/// included). Instance elaboration probes EVERY `UserFunctionCall` cell of
/// every instance, and `eval_child_expr` then resolves the same call again on
/// the fallback path — without the pre-filter an ordinary, non-`@optimized`
/// function-call let pays full overload resolution twice per instance. The
/// pre-filter is exact, not approximate: `find_matching_compiled_function` only
/// ever returns a candidate with `f.name == function_name` (every tier's
/// `arity_match` requires it), so if no candidate of that name carries an
/// `optimized_target` the full scan cannot produce one either.
pub(crate) fn optimized_target_of<'f>(
    expr: &reify_ir::CompiledExpr,
    functions: &'f [CompiledFunction],
    optimized_names: &OptimizedNameIndex<'f>,
) -> Option<String> {
    let reify_ir::CompiledExprKind::UserFunctionCall {
        function_name,
        args,
    } = &expr.kind
    else {
        return None;
    };
    let names = optimized_names.get_or_init(|| {
        functions
            .iter()
            .filter(|f| f.optimized_target.is_some())
            .map(|f| f.name.as_str())
            .collect()
    });
    if !names.contains(function_name.as_str()) {
        return None;
    }
    reify_expr::find_matching_compiled_function(functions, function_name, args)
        .and_then(|f| f.optimized_target.clone())
}

/// Outcome of probing an instance-scope cell for `@optimized` reuse
/// ([`resolve_optimized_instance_cell`]).
enum OptimizedInstanceResolution {
    /// Not an `@optimized` `UserFunctionCall` — evaluate normally.
    NotOptimized,
    /// The template-scope cell's dispatched value, safe to commit verbatim at
    /// instance scope because every input this call reads compares equal
    /// between the two scopes.
    Reuse(Value),
    /// An `@optimized` call whose template-scope value CANNOT be proven to
    /// apply to this instance. The caller falls back to body-inlining (today's
    /// behaviour) and reports `reason` — subject to
    /// [`report_optimized_instance_decline`]'s filters, of which `cause` is one.
    Unreusable {
        target: String,
        cause: DeclineCause,
        reason: String,
    },
}

/// WHICH of the two declining conditions [`resolve_optimized_instance_cell`]
/// hit. Structural, so [`report_optimized_instance_decline`] can filter on the
/// condition rather than on the human-readable `reason` text.
///
/// amend (#6662 reviewer_comprehensive, suggestion #1 — robustness). Before
/// this split there was only a `reason` string, and the missing-template-value
/// branch hard-coded ONE explanation ("structure not in module.templates — e.g.
/// a prelude/stdlib structure"). That explanation is wrong in every case the
/// warning could actually be PRINTED: see [`NoTemplateValue`]'s doc for why.
///
/// [`NoTemplateValue`]: DeclineCause::NoTemplateValue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclineCause {
    /// A cell in the call's DIRECT read set has a different value at instance
    /// scope than at template scope — a constructor override, virtually always.
    /// Genuinely instance-specific and genuinely degrading (the instance falls
    /// back to the `.ri` body's sentinel), so this is the cause that is
    /// REPORTED.
    InputsDiffer,
    /// The template-scope OUTPUT cell holds no value at all. NOT reported —
    /// under `report_optimized_instance_decline`'s registered-target gate this
    /// cause has no honest message to print:
    ///
    /// * If the owning structure got NO template-scope pass (a prelude/stdlib
    ///   structure — `engine_eval.rs` keys its lowering on `for template in
    ///   &module.templates`), then no `ComputeNode` names the cell either, so
    ///   the registered-target gate is already false and nothing is emitted.
    /// * If a `ComputeNode` DOES name the cell, template scope registered a
    ///   trampoline and DISPATCHED — and the value is missing because that
    ///   dispatch Failed or was Cancelled (the Failed handler deliberately does
    ///   not write to `values`; pinned by
    ///   `e2e_registered_failed_trampoline_does_not_silently_body_inline`).
    ///   Template scope has already surfaced the trampoline's own Error for
    ///   exactly that cell. A second, instance-scope report would be the same
    ///   class of misleading duplicate the registered-target gate was added to
    ///   eliminate — and worse, it would attribute a failed solve to "a
    ///   prelude/stdlib structure".
    ///
    /// So the two reachable readings are "nothing happened" and "template scope
    /// already told you, more accurately". Neither wants a warning here; the
    /// VALUE is body-inlined either way, unchanged.
    NoTemplateValue,
}

/// Decide whether an instance-scope cell may carry the template-scope cell's
/// already-dispatched `@optimized` value (task #6662).
///
/// WHY REUSE RATHER THAN RE-DISPATCH. `@optimized` → ComputeNode lowering
/// exists only at template scope (`engine_eval.rs::evaluate_params_and_lets_unified`
/// and `::evaluate_let_bindings`, both keyed on `for template in
/// &module.templates`). Instance cells are elaborated here, through
/// `cell_eval_ctx`, which carries no compute dispatch — so
/// `reify_expr::try_compute_dispatch` returns `None` and the `.ri` function
/// BODY runs. Every solver stdlib body is a bare sentinel constructor
/// (`{ ElasticResult() }`), so an instantiated `sub` silently gets an empty
/// shell where the template got the real solved value.
///
/// Re-dispatching here through the `ComputeDispatch` trait hook would be
/// WORSE, not better: `OptimizedComputeDispatcher::dispatch` calls
/// `f(args, &[], &Value::Undef, None, &cancellation)` — EMPTY realization
/// inputs, `Undef` options, no warm state — whereas the template-scope
/// lowering builds `realization_read_handles` via
/// `build_compute_realization_inputs`, runs `insert_shell_extract_upstream`,
/// and threads a persistent cache key plus warm state through
/// `run_compute_dispatch`. That would trade a visibly-empty sentinel for an
/// invisibly-wrong number on every realization-bearing / shell-route target,
/// and would double every FEA solve. Reusing the template-scope value is
/// bit-exact with the full lowering at zero extra solve cost.
///
/// SOUNDNESS. The arg expressions are literally the same `CompiledExpr`s at
/// both scopes (compiled once, in the child template's scope), so the call's
/// result is a pure function of the values its reads resolve to. Comparing the
/// DIRECT read set is therefore sufficient — transitive dependencies are
/// already folded into the direct reads' values. The comparison is conservative
/// in the right direction: present-in-one-map vs absent-in-the-other compares
/// unequal, so the helper declines rather than guesses.
///
/// KNOWN LIMITATION — STALENESS (amend #6662 reviewer_comprehensive,
/// suggestion #5). `Reuse` copies the template-scope value as a ONE-TIME
/// SNAPSHOT taken during sub elaboration; nothing refreshes it afterwards.
/// `Engine::redispatch_geometry_consuming_compute_nodes` (engine_build.rs)
/// re-dispatches a geometry-consuming compute node LATER, inside `build()`,
/// once its args have hydrated to kernel-backed `GeometryHandle`s — and it
/// writes only `node_data.output_value_cells[0]`, which is TEMPLATE-scoped
/// because every `ComputeNode` is lowered at template scope. A reused instance
/// cell would therefore keep the degraded first-dispatch result while the
/// template cell got the corrected one: the "invisibly-wrong number" hazard
/// this doc cites as the reason not to re-dispatch, reached by staleness
/// instead. It is BELIEVED unreachable — a geometry-typed arg's instance value
/// carries an instance-scoped realization ref, so the read comparison above
/// should decline first — but that belief is NOT pinned by any fixture on this
/// branch, and pinning it needs a kernel-backed handle this crate's tests
/// cannot mint. Tracked as ticket `tkt_0RT532KR05N0FNK05FJT69R2CA` (filed from
/// #6662's amendment pass; not cited as `#NNNN` because the curator assigns the
/// task id asynchronously and an unresolvable cite would be an orphan).
///
/// SHAPE-EXACTNESS is inherited from template scope, not re-invented: the probe
/// is [`optimized_target_of`], the ONE definition `engine_eval.rs`'s
/// `is_optimized_userfn_cell` is also expressed over (see that helper's doc for
/// why there is exactly one). So a cell that merely WRAPS an `@optimized` call
/// (`let m = limit - solve(..).x`) is left alone at BOTH scopes, and a future
/// change to overload resolution moves both together.
///
/// `instance_values` is this instance's map (`child_values` in phase 2, the
/// running `overlay` in phase 1.5); `global_values` is the global map holding
/// the template-scope pass's results. Both are keyed template-scoped, so the
/// same `ValueCellId` addresses the two scopes' copies of one cell.
///
/// ## Why the two instance maps reach the SAME decision
///
/// amend (#6662 reviewer_comprehensive, suggestion #7). The gate runs against
/// two different instance maps — phase 1.5's `overlay` (params + scratch lets +
/// ONE level of nested-sub member projections + the collapsed `__self` aliases)
/// and phase 2's `child_values` (params + the multi-level projection BFS +
/// collapsed values) — and a key one map carries while the other does not would
/// compare `Some(v)` vs `None` and flip the gate. MEASURED on this branch, that
/// divergence class is not reachable, because the only keys the two maps differ
/// on are ones no read set can name:
///
/// * a read TWO levels down (`self.leaf.inner.x`) — the shape that would key on
///   `Mid.leaf.inner.x`, which phase 2's BFS projects and phase 1.5's
///   single-level loop does not — is REJECTED BY THE COMPILER:
///   `unknown member 'inner' on sub 'leaf'`, followed by `no matching overload`.
///   The deepest expressible cross-sub read is one hop (`self.leaf.ix`, keyed
///   `Mid.leaf.ix`), which BOTH maps carry. Pinned by
///   `phase15_phase2_parity_deepest_expressible_cross_sub_read` and
///   `two_level_cross_sub_read_is_rejected_by_the_compiler` in
///   `tests/compute_dispatch_registry.rs` — if the language ever gains the flat
///   two-level form, the second of those reds and this note must be revisited.
/// * params, same-template lets, one-level `{tmpl}.{sub}.{member}` projections
///   and the collapsed `{tmpl}.{sub}` / `{tmpl}.{sub}.__self` aliases are in
///   BOTH maps, and in both cases the producing node is a topological
///   PREDECESSOR of the reading cell (`phase15_node_traces` normalises a
///   cross-sub read onto its sub node), so ordering cannot make one map hold a
///   key the other is still missing.
/// * a SKIPPED sub (non-nestable / cycle cut / unresolvable) contributes to
///   NEITHER map: phase 2's BFS is seeded from `elaborated_sub_names`, which
///   excludes exactly the subs phase 1.5 skipped.
// 8 args: the file's established shape for these elaboration helpers (five
// siblings above carry the same allow). Each argument is an independent
// capability — two value maps that must NOT be conflated, the probe index, the
// cell's identity, and the lazy read trace — so bundling them into a struct
// would only rename the coupling.
#[allow(clippy::too_many_arguments)]
fn resolve_optimized_instance_cell<'f, R>(
    expr: &reify_ir::CompiledExpr,
    functions: &'f [CompiledFunction],
    optimized_names: &OptimizedNameIndex<'f>,
    child_template: &TopologyTemplate,
    member: &str,
    reads: impl FnOnce() -> R,
    instance_values: &ValueMap,
    global_values: &ValueMap,
) -> OptimizedInstanceResolution
where
    R: AsRef<[ValueCellId]>,
{
    let Some(target) = optimized_target_of(expr, functions, optimized_names) else {
        return OptimizedInstanceResolution::NotOptimized;
    };

    // Input equality over the DIRECT read set. `reads` yields
    // `extract_dependency_trace(expr).reads` — the same function that builds
    // phase 2's topological-sort edges, so the reuse gate and the evaluation
    // order can never disagree about what a cell depends on.
    //
    // amend (#6662 reviewer_comprehensive, suggestion #2 — efficiency). It is a
    // `FnOnce`, not a slice, so the trace is materialised ONLY on the
    // `@optimized` path. Two of the three call sites have no pre-built trace to
    // hand over and were calling `extract_dependency_trace(expr)` eagerly for
    // EVERY scratch let / param default of every instance (a full
    // expression-tree walk plus a `Vec<ValueCellId>` allocation), discarding the
    // result on the overwhelmingly common `NotOptimized` return. The third site
    // (phase-2 lets) still passes its pre-built `child_let_traces` entry by
    // reference — `&[ValueCellId]: AsRef<[ValueCellId]>` — so it keeps costing
    // nothing.
    let reads = reads();
    for read in reads.as_ref() {
        let instance_val = instance_values.get(read);
        let global_val = global_values.get(read);
        if instance_val != global_val {
            return OptimizedInstanceResolution::Unreusable {
                target,
                cause: DeclineCause::InputsDiffer,
                reason: format!(
                    "input {}.{} differs from the template's ({:?} vs {:?})",
                    read.entity, read.member, instance_val, global_val
                ),
            };
        }
    }

    // The template-scope output cell. Absent under either of two conditions —
    // the owning structure got no template-scope pass at all (a prelude/stdlib
    // structure, not in `module.templates`), or template scope DID dispatch and
    // the dispatch produced no value (Failed / Cancelled). This function cannot
    // tell them apart (it holds no graph), and deliberately does not guess: the
    // reason names both, and `DeclineCause::NoTemplateValue` carries the
    // distinction structurally to the one place that CAN discriminate — see
    // that variant's doc, and `report_optimized_instance_decline`.
    let template_cell = ValueCellId::new(&child_template.name, member);
    match global_values.get(&template_cell) {
        Some(value) => OptimizedInstanceResolution::Reuse(value.clone()),
        None => OptimizedInstanceResolution::Unreusable {
            target,
            cause: DeclineCause::NoTemplateValue,
            reason: format!(
                "template-scope cell {}.{} holds no value — either the structure \
                 got no template-scope pass (not in module.templates, e.g. a \
                 prelude/stdlib structure) or its template-scope dispatch \
                 produced none (Failed/Cancelled)",
                child_template.name, member
            ),
        },
    }
}

/// Did TEMPLATE scope actually DISPATCH this cell through a compute trampoline,
/// rather than body-inline it?
///
/// amend (#6662 reviewer_comprehensive, suggestion #1). `engine_eval.rs`'s
/// lowering site inserts a `ComputeNode` carrying the cell in
/// `output_value_cells` ONLY inside its `if self.compute_dispatch(&target)
/// .is_some()` arm; the unregistered arm pushes its own
/// `no registered compute trampoline` Error and falls straight through to
/// body-inlining without ever touching the graph. The presence of that node is
/// therefore an exact, structural answer to "is the template-scope value a
/// dispatched one", needing neither the registry (which `unfold.rs` has no
/// handle on) nor a re-evaluation to probe.
fn template_cell_was_dispatched(snapshot: &Snapshot, template_cell: &ValueCellId) -> bool {
    snapshot
        .graph
        .compute_nodes
        .values()
        .any(|n| n.output_value_cells.contains(template_cell))
}

/// Report an instance-scope `@optimized` reuse DECLINE — once per authoring
/// fact, and only when declining actually costs something.
///
/// The decline is LOUD but not fatal. The VALUE is byte-identical to pre-#6662
/// behaviour, so no fixture changes outcome and no exit code moves — the
/// warning removes only the silence, which is the actual defect class here.
/// Escalating it to a hard failure is #6608's declared scope, and genuine
/// per-instance dispatch under constructor overrides is #6592's.
///
/// Three filters, all from the #6662 amendment review:
///
/// * CAUSE GATE (round-2 suggestion #1). Only
///   [`DeclineCause::InputsDiffer`] is reported.
///   [`DeclineCause::NoTemplateValue`] is not: under the registered-target gate
///   below, the only condition that reaches it is a template-scope dispatch
///   that Failed or was Cancelled — which template scope has ALREADY reported
///   as an Error naming the same cell — and the pre-fix message attributed that
///   failure to "a prelude/stdlib structure", the one explanation that is
///   provably NOT the cause whenever the gate lets the message through. See
///   [`DeclineCause::NoTemplateValue`] for the full case split. Filtering on the
///   structural cause rather than on the presence of a text-matching Error
///   keeps this independent of any other site's diagnostic wording.
///
/// * REGISTERED-TARGET GATE (suggestion #1). Without it the warning fired, with
///   actively misleading text, in the plain `reify check` shape — which
///   registers NO trampolines — for every sub carrying a constructor override on
///   a param an `@optimized` call reads. MEASURED: `Outer.a.r` got `Int(11)`,
///   the correct per-instance body-inlined answer, alongside "falling back to
///   body-inlining"; there was no dispatch to fall back FROM, and template scope
///   had already emitted its own `no registered compute trampoline` Error for
///   the same condition. `reify check` is the default CLI path, so that was a
///   spurious warning on every model instantiating an `@optimized`-bearing
///   structure with an override. The unregistered condition belongs to template
///   scope's diagnostic and is not re-reported here.
/// * PER-TEMPLATE-CELL DEDUPE (suggestion #2). The decline is a fact about
///   `(target, child template, member)`, not about one instance: three subs of
///   one template with three distinct overrides are one authoring fact, and a
///   keyed/collection `sub` routes every element through the same site — N
///   warnings for one fact, scaling with element count. `diagnostics` is the
///   one vec `engine_eval.rs` threads through the whole sub-elaboration pass
///   (collection elements included), so a prefix scan over it dedupes across
///   sibling and collection instances alike, which a set scoped to one
///   `elaborate_child_instance` call could not. The scan runs on the decline
///   path only, and dedupe keeps the scanned vec short.
#[allow(clippy::too_many_arguments)]
fn report_optimized_instance_decline(
    diagnostics: &mut Vec<Diagnostic>,
    snapshot: &Snapshot,
    child_template: &TopologyTemplate,
    member: &str,
    scoped_entity: &str,
    target: &str,
    cause: DeclineCause,
    reason: &str,
) {
    if cause != DeclineCause::InputsDiffer {
        return;
    }
    let template_cell = ValueCellId::new(&child_template.name, member);
    if !template_cell_was_dispatched(snapshot, &template_cell) {
        return;
    }
    // The dedupe key is a literal PREFIX of the message, so the scan needs no
    // side table and cannot drift from the text it keys.
    let key = format!(
        "@optimized target {:?} on {}.{}",
        target, child_template.name, member
    );
    if diagnostics.iter().any(|d| d.message.starts_with(&key)) {
        return;
    }
    diagnostics.push(Diagnostic::warning(format!(
        "{key}: instance scope {scoped_entity}.{member} cannot reuse the \
         template's dispatched value ({reason}) — falling back to body-inlining. \
         Reported once per template cell: every other instance of {} whose \
         inputs differ declines the same way (per-instance dispatch under \
         constructor overrides is tracked by #6592).",
        child_template.name,
    )));
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
    diagnostics: &mut Vec<Diagnostic>,
) -> ValueMap {
    let mut child_values = ValueMap::new();
    // runtime_sink is required by `cell_eval_ctx`; its contents are
    // discarded rather than surfaced — pre-migration this path never surfaced
    // eval-time diagnostics, and the `diagnostics` param added by the #6662
    // amendment carries ONE specific report (the `@optimized` param-default
    // decline below), deliberately not a general drain for this sink.
    let runtime_sink = RefCell::new(Vec::new());
    let containment = NoContainment;
    // One `@optimized`-name index for this whole param loop (amend #6662
    // suggestion #3); lazily built, so a template with no function-call param
    // defaults pays nothing.
    let optimized_names: OptimizedNameIndex = OptimizedNameIndex::new();

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
            // task #6662: same `@optimized` resolution as the two let sites,
            // applied to a param whose DEFAULT is the call. Deliberately NOT
            // applied to the explicit-arg arm above: an explicit ctor arg is by
            // construction an instance-specific input, and it is evaluated in
            // the PARENT's scope (`values`), so the helper's read comparison —
            // which assumes both maps are keyed in the CHILD template's scope —
            // would be comparing the wrong maps there.
            //
            // Measured on this branch: template scope does not lower an
            // `@optimized` param default to a ComputeNode either (only
            // let-cells are lowered), so the template cell holds the same
            // body-inlined sentinel and the helper returns `Reuse(sentinel)` —
            // instance and template agree, silently and correctly. Wiring it
            // here is what makes instance scope inherit a future template-scope
            // param fix with no further change.
            // The read trace is materialised lazily (amend suggestion #2): a
            // param default that is not an `@optimized` call — the common case,
            // and one that runs for EVERY param of EVERY instance, collection
            // elements included — never walks the expression tree here.
            let resolution = resolve_optimized_instance_cell(
                default_expr,
                functions,
                &optimized_names,
                child_template,
                member,
                || extract_dependency_trace(default_expr).reads,
                &child_values,
                values,
            );

            // amend (#6662 reviewer_comprehensive, suggestion #5). This site
            // reports its OWN declines. The lets site's "phase 2 is the
            // authoritative site and sees the same cell" justification for
            // staying silent does NOT cover a param: `elaborate_child_lets_only`
            // filters on `c.kind == ValueCellKind::Let`, so a Param cell is
            // never seen there and a decline here would otherwise be reported by
            // nobody — contradicting #6662's "the decline is LOUD" contract.
            //
            // Today this is provably silent rather than merely quiet: template
            // scope lowers only LET cells to ComputeNodes, so
            // `report_optimized_instance_decline`'s registered-target gate —
            // which asks whether a ComputeNode names this template cell — is
            // false for every param default, and both scopes body-inline in
            // agreement. When the template-scope param-default gap (#6750)
            // closes, the gate flips to true and this site starts reporting with
            // no further change here, which is the point of wiring it now.
            if let OptimizedInstanceResolution::Unreusable {
                target,
                cause,
                reason,
            } = &resolution
            {
                report_optimized_instance_decline(
                    diagnostics,
                    snapshot,
                    child_template,
                    member,
                    scoped_entity,
                    target,
                    *cause,
                    reason,
                );
            }

            match resolution {
                OptimizedInstanceResolution::Reuse(v) => v,
                OptimizedInstanceResolution::NotOptimized
                | OptimizedInstanceResolution::Unreusable { .. } => eval_child_expr(
                    &child_values,
                    default_expr,
                    functions,
                    meta_map,
                    &snapshot.values,
                    &runtime_sink,
                    &containment,
                ),
            }
        } else {
            Value::Undef
        };

        child_values.insert(cell.id.clone(), val.clone());

        // TraceSource::GuardedGroup: the PRD (§0, §8) treats this
        // recursive-unfold path and engine_eval.rs's guarded-group param
        // cells as one shared provenance category, not two — this is the
        // intended tag, not a placeholder.
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
/// `elaborated_sub_names` names every sub of `child_template` whose entity was
/// ALREADY materialised before this call — recursive subs unfolded by
/// [`unfold_recursive_sub`], and (since task 5360) the plain instance-nested subs
/// phase 1.5 elaborates. It is deliberately NOT "the recursive subs": naming it
/// that was a leftover from when this function had only the one caller, and it
/// contradicted the projection rule directly above it. A BFS walks the full
/// entity tree under `scoped_entity` (following all sub name branches at each
/// level), so cross-sub values are projected correctly.
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
    elaborated_sub_names: &[&str],
    templates: &'t [TopologyTemplate],
    prelude: &'t [CompiledModule],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // runtime_sink is required by `cell_eval_ctx`; its contents are
    // discarded rather than appended to `diagnostics` above — pre-migration
    // this path never surfaced eval-time diagnostics either, same as
    // `elaborate_child_params_only`.
    let runtime_sink = RefCell::new(Vec::new());
    let containment = NoContainment;

    // Enrich child_values with sub-component values projected from the global map.
    // Only needed where the named subs' entities have already been elaborated
    // (leaves-first ordering): recursive subs unfolded by `unfold_recursive_sub`,
    // and — since task 5360 — plain instance-nested subs elaborated by
    // `elaborate_child_instance`'s phase 1.5. Both feed the same key remap.
    //
    // Uses BFS over the entity tree rooted at scoped_entity: starts with one immediate
    // child per sub name, then expands branches where values exist. This handles both
    // single-sub chains (O(D×C)) and multi-sub trees (O(B^D×C) where B=branching, D=depth).
    // The BFS terminates naturally when no values are found at a given entity.
    if !elaborated_sub_names.is_empty() {
        let scoped_prefix = format!("{}.", scoped_entity);
        let template_prefix = format!("{}.", child_template.name);

        // BFS queue carries (entity_path, entity_template) so each depth level uses
        // the correct template's value_cells for projection (heterogeneous mutual recursion).
        let mut queue: std::collections::VecDeque<(String, &TopologyTemplate)> = elaborated_sub_names
            .iter()
            .filter_map(|name| {
                // Look up the sub declaration to find its target template.
                let sub_decl = child_template.sub_components.iter().find(|s| s.name == *name)?;
                // Prelude fallback is REQUIRED here, not cosmetic: phase 1.5
                // now materialises prelude-typed nested subs (esc-5360-9), and
                // it hands their names to this seed. A module-only lookup would
                // fail on exactly those names and raise a spurious "unknown
                // structure" error for a sub that WAS just elaborated.
                let target_tmpl =
                    find_template_in_scope(templates, prelude, &sub_decl.structure_name).or_else(
                        || {
                            diagnostics.push(Diagnostic::error(format!(
                                "BFS seed: sub \"{}\" in \"{}\" references unknown structure \"{}\"; skipping",
                                name, scoped_entity, sub_decl.structure_name
                            )));
                            None
                        },
                    )?;
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
            // The prefix is built ONCE per BFS node, not once per `values`
            // entry: this scan is O(|values|) and the `format!` used to sit
            // inside the closure, allocating a `String` on every probe.
            let intermediary_has_descendants = entity_template.value_cells.is_empty() && {
                let depth_prefix = format!("{}.", depth_entity);
                values.iter().any(|(k, _)| k.entity.starts_with(&depth_prefix))
            };
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
                            find_template_in_scope(templates, prelude, &sub_decl.structure_name)
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

    // One `@optimized`-name index for this whole let loop (amend #6662
    // suggestion #3); lazily built on the first `UserFunctionCall` probe.
    let optimized_names: OptimizedNameIndex = OptimizedNameIndex::new();

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

        // task #6662: an `@optimized` cell's value comes from the compute
        // dispatch registry, which only template scope has. When this
        // instance's inputs are value-identical to the template's, reuse the
        // template's already-dispatched value instead of body-inlining the
        // `.ri` function's sentinel. This is the AUTHORITATIVE instance-scope
        // let commit, so it is the site that decides the committed value.
        // `child_let_traces` already holds `extract_dependency_trace(expr)` for
        // exactly this `NodeId` (built above, consumed by `take_trace` below),
        // so the gate borrows it rather than walking the expression again —
        // amend suggestion #3. The `unwrap_or_default` is unreachable (both maps
        // are built from `child_let_cells`) and only keeps the borrow total.
        let no_reads: Vec<ValueCellId> = Vec::new();
        let reads: &[ValueCellId] = child_let_traces
            .get(&child_node_id)
            .map(|t| t.reads.as_slice())
            .unwrap_or(&no_reads);
        let resolution = resolve_optimized_instance_cell(
            expr,
            functions,
            &optimized_names,
            child_template,
            member,
            || reads,
            &child_values,
            values,
        );

        // Emitted from THIS site only. Phase 1.5's scratch arm sees the same
        // cell, so reporting from both would double every warning; phase 2 is
        // the authoritative committing site, so it owns the report.
        // `report_optimized_instance_decline` owns both the registered-target
        // gate and the per-template-cell dedupe — see its doc comment; in
        // particular it is what keeps the plain `reify check` shape (no
        // trampolines registered at all) silent.
        if let OptimizedInstanceResolution::Unreusable {
            target,
            cause,
            reason,
        } = &resolution
        {
            report_optimized_instance_decline(
                diagnostics,
                snapshot,
                child_template,
                member,
                scoped_entity,
                target,
                *cause,
                reason,
            );
        }

        let val = match resolution {
            OptimizedInstanceResolution::Reuse(v) => v,
            // Unreusable falls through to body-inlining, unchanged from
            // pre-#6662 behaviour — only the silence is removed.
            OptimizedInstanceResolution::NotOptimized
            | OptimizedInstanceResolution::Unreusable { .. } => eval_child_expr(
                &child_values,
                expr,
                functions,
                meta_map,
                &snapshot.values,
                &runtime_sink,
                &containment,
            ),
        };
        child_values.insert(child_cell_id.clone(), val.clone());

        let scoped_id = ValueCellId::new(scoped_entity, member);

        // sorted_child_lets and child_let_traces are built from the same key set, so remove() cannot fail.
        let trace = take_trace(
            &mut child_let_traces,
            &child_node_id,
            "sorted_child_lets",
            "child_let_traces",
        );

        // Same TraceSource::GuardedGroup provenance as the Site 1 commit
        // above.
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
    //
    // amend (#5057 reviewer_comprehensive, suggestion #2 —
    // test-coverage-fragility): the `Bool(true)` assertion below is
    // load-bearing on an unstated ordering invariant — that root `S.n` is
    // already committed to `snapshot.values` as `DeterminacyState::Determined`
    // before `elaborate_child_params_only` runs for `S.child` (true today:
    // root params are elaborated by the main eval pass before any unfold
    // recursion begins). If that ordering ever changed, the assertion would
    // silently flip to `Value::Undef` rather than failing loudly, so the
    // precondition is asserted explicitly below.
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

        let root_n = ValueCellId::new("S", "n");
        let snap = engine
            .snapshot()
            .expect("engine.snapshot() should be populated after eval");
        assert_eq!(
            snap.values.get(&root_n).map(|(_, det)| *det),
            Some(DeterminacyState::Determined),
            "precondition: S.n must already be committed Determined before \
             child elaboration runs, or the Bool(true) assertion below would \
             silently degrade to Value::Undef instead of failing loudly"
        );

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
    //
    // amend (#5057 reviewer_comprehensive, suggestion #2 —
    // test-coverage-fragility): as with the param default_expr fixture above,
    // the `Bool(true)` assertion below is load-bearing on an unstated
    // ordering invariant — that root `S.n` is already committed to
    // `snapshot.values` as `DeterminacyState::Determined` before
    // `elaborate_child_lets_only` runs for `S.child` (true today: root params
    // are elaborated by the main eval pass before any unfold recursion
    // begins). If that ordering ever changed, the assertion would silently
    // flip to `Value::Undef` rather than failing loudly, so the precondition
    // is asserted explicitly below.
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

        let root_n = ValueCellId::new("S", "n");
        let snap = engine
            .snapshot()
            .expect("engine.snapshot() should be populated after eval");
        assert_eq!(
            snap.values.get(&root_n).map(|(_, det)| *det),
            Some(DeterminacyState::Determined),
            "precondition: S.n must already be committed Determined before \
             child elaboration runs, or the Bool(true) assertion below would \
             silently degrade to Value::Undef instead of failing loudly"
        );

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

    // ── #6662 amendment tests ────────────────────────────────────────────────

    /// amend (#6662 reviewer_comprehensive, suggestion #4 — code-reuse).
    ///
    /// [`optimized_target_of`] and `engine_eval::is_optimized_userfn_cell` are
    /// the SAME shape rule, and a divergence between them is silent and
    /// expensive (a cell `@optimized` at one scope but not the other either has
    /// its dispatched value re-folded through plain `eval_expr` or stays
    /// body-inlined forever). `engine_eval.rs` is outside this task's lock set,
    /// so the two cannot yet be collapsed into one call; this asserts the
    /// equivalence directly instead, over every shape either probe can see:
    /// an `@optimized` call, a plain call, a non-call expression, and a call
    /// that merely WRAPS an `@optimized` call.
    #[test]
    fn optimized_probe_agrees_with_engine_eval_is_optimized_userfn_cell() {
        let source = r#"
            @optimized("test::probe")
            fn opt_call(x : Int) -> Int {
                x + 1
            }

            fn plain_call(x : Int) -> Int {
                x + 2
            }

            structure ProbeShapes {
                param x : Int = 3
                let a_optimized = opt_call(x)
                let b_plain = plain_call(x)
                let c_not_a_call = x + 1
                let d_wrapping = plain_call(opt_call(x))
            }
        "#;
        let module = reify_test_support::compile_source_with_stdlib(source);
        let errors = reify_test_support::collect_errors(&module.diagnostics);
        assert!(errors.is_empty(), "fixture must compile clean: {errors:?}");

        // The probe must see the stdlib-inclusive function set the production
        // sites see, not just the module's own — that is what makes the
        // overload-resolution tiers actually run.
        let functions = &module.functions;

        for (cell, expect_optimized) in [
            ("a_optimized", true),
            ("b_plain", false),
            ("c_not_a_call", false),
            // `let d = plain_call(opt_call(x))` is a call to a NON-optimized fn;
            // the `@optimized` call is an ARGUMENT. Both probes look at the
            // top-level expression only, so both must say "not optimized" —
            // this is the "merely WRAPS" case the doc comments promise is left
            // alone at BOTH scopes.
            ("d_wrapping", false),
        ] {
            let expr = reify_test_support::get_let_expr_in(&module, "ProbeShapes", cell);
            // A fresh index per probe: this test is about the VERDICT, not the
            // memoisation, and a per-call cache keeps the two probes' inputs as
            // close to identical as the signatures allow.
            let mine = optimized_target_of(expr, functions, &OptimizedNameIndex::new());
            let theirs = crate::engine_eval::is_optimized_userfn_cell(expr, functions);
            assert_eq!(
                mine.is_some(),
                theirs,
                "optimized_target_of and engine_eval::is_optimized_userfn_cell \
                 disagree on `{cell}` ({mine:?} vs {theirs}); the two probes have \
                 split and one scope now treats this cell differently from the other"
            );
            assert_eq!(
                mine.is_some(),
                expect_optimized,
                "probe verdict for `{cell}` is not the documented one (got {mine:?})"
            );
        }
    }

    /// amend (#6662 reviewer_comprehensive, suggestion #6 — test-coverage).
    ///
    /// The `Unreusable { reason: "template-scope cell … was never evaluated" }`
    /// branch guards the case where the child structure got no template-scope
    /// pass at all, because `engine_eval.rs` keys its `@optimized` lowering on
    /// `for template in &module.templates` and a PRELUDE/stdlib structure is
    /// deliberately not merged into that list (see
    /// [`find_template_in_scope`]). It is not incidental — the FEA/solver
    /// targets #6662 exists for live in the stdlib, and `find_template_in_scope`
    /// shows prelude-typed subs ARE elaborated on this path — but no
    /// end-to-end fixture could reach it, because building a prelude module
    /// that a user module can also name at COMPILE time needs a `&'static`
    /// prelude the test-support compile helpers do not expose. So the branch is
    /// covered where it is decided: at the helper, with a `global_values` map
    /// that simply lacks the template-scope output cell.
    #[test]
    fn resolve_optimized_instance_cell_reports_unevaluated_template_cell() {
        let source = r#"
            @optimized("test::prelude_target")
            fn prelude_opt(x : Int) -> Int {
                x + 1
            }

            structure PreludeLike {
                param x : Int = 3
                let r = prelude_opt(x)
            }
        "#;
        let module = reify_test_support::compile_source_with_stdlib(source);
        let errors = reify_test_support::collect_errors(&module.diagnostics);
        assert!(errors.is_empty(), "fixture must compile clean: {errors:?}");
        let template = module
            .templates
            .iter()
            .find(|t| t.name == "PreludeLike")
            .expect("PreludeLike template");
        let expr = reify_test_support::get_let_expr_in(&module, "PreludeLike", "r");

        // Both maps agree on the INPUT (`PreludeLike.x`), so the read loop
        // passes and the outcome is decided purely by the output cell's
        // absence — which is exactly what a never-template-evaluated structure
        // looks like.
        let mut instance_values = ValueMap::new();
        instance_values.insert(ValueCellId::new("PreludeLike", "x"), Value::Int(3));
        let mut global_values = ValueMap::new();
        global_values.insert(ValueCellId::new("PreludeLike", "x"), Value::Int(3));
        assert!(
            global_values
                .get(&ValueCellId::new("PreludeLike", "r"))
                .is_none(),
            "the fixture's premise is that the template-scope OUTPUT cell is absent"
        );

        let resolution = resolve_optimized_instance_cell(
            expr,
            &module.functions,
            &OptimizedNameIndex::new(),
            template,
            "r",
            || extract_dependency_trace(expr).reads,
            &instance_values,
            &global_values,
        );
        match resolution {
            OptimizedInstanceResolution::Unreusable {
                target,
                cause,
                reason,
            } => {
                assert_eq!(target, "test::prelude_target");
                assert_eq!(
                    cause,
                    DeclineCause::NoTemplateValue,
                    "the missing-output-cell branch must carry the structural cause, \
                     not just prose — `report_optimized_instance_decline` filters on it \
                     (amend round-2 suggestion #1): {reason}"
                );
                // amend (#6662 reviewer_comprehensive round 2, suggestion #1).
                // The reason must NOT hard-code the prelude/stdlib explanation:
                // a template-scope dispatch that Failed lands in this same
                // branch, and is in fact the ONLY reading under which the text
                // could ever be printed. Both causes must be named.
                assert!(
                    reason.contains("template-scope cell PreludeLike.r holds no value"),
                    "wrong decline reason for the no-template-value branch: {reason}"
                );
                assert!(
                    reason.contains("prelude/stdlib structure")
                        && reason.contains("Failed/Cancelled"),
                    "the no-template-value reason must name BOTH causes — a missing \
                     template-scope pass AND a template-scope dispatch that produced \
                     no value — since this branch cannot tell them apart: {reason}"
                );
            }
            OptimizedInstanceResolution::Reuse(v) => panic!(
                "must not reuse a value the template scope never produced (got {v:?})"
            ),
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`prelude_opt` carries @optimized; the probe must see it")
            }
        }

        // Same fixture, template cell PRESENT ⇒ the branch above is genuinely
        // selected by absence and not by something else in the shape.
        global_values.insert(ValueCellId::new("PreludeLike", "r"), Value::Int(777));
        match resolve_optimized_instance_cell(
            expr,
            &module.functions,
            &OptimizedNameIndex::new(),
            template,
            "r",
            || extract_dependency_trace(expr).reads,
            &instance_values,
            &global_values,
        ) {
            OptimizedInstanceResolution::Reuse(v) => assert_eq!(v, Value::Int(777)),
            other => panic!(
                "with the template cell present and inputs equal this must Reuse, got {:?}",
                match other {
                    OptimizedInstanceResolution::Unreusable { reason, .. } => reason,
                    _ => "NotOptimized".to_string(),
                }
            ),
        }
    }
}
