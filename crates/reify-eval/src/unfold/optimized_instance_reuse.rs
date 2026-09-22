//! Instance-scope reuse of a template's already-dispatched `@optimized` value
//! (task #6662).
//!
//! `@optimized` → `ComputeNode` lowering exists only at TEMPLATE scope
//! (`engine_eval.rs::evaluate_params_and_lets_unified` and
//! `::evaluate_let_bindings`, both keyed on `for template in
//! &module.templates`). Instance cells are elaborated by [`super`], through
//! `cell_eval_ctx`, which carries no compute dispatch — so
//! `reify_expr::try_compute_dispatch` returns `None` and the `.ri` function
//! BODY runs. Every solver stdlib body is a bare sentinel constructor
//! (`{ ElasticResult() }`), so an instantiated `sub` silently got an empty
//! shell where the template got the real solved value.
//!
//! This module decides, per instance-scope cell, whether the template's value
//! may be COPIED verbatim ([`resolve_optimized_instance_cell`]), and reports
//! the declines that actually cost something
//! ([`report_optimized_instance_decline`]). Its three callers are the three
//! instance-scope eval sites in [`super`]: phase 1.5's scratch-let arm, phase
//! 2's authoritative let commit, and the param-default arm of phase 1.
//!
//! WHY REUSE RATHER THAN RE-DISPATCH. `OptimizedComputeDispatcher::dispatch`
//! calls `f(args, &[], &Value::Undef, None, &cancellation)` — EMPTY realization
//! inputs, `Undef` options, no warm state — whereas the template-scope lowering
//! builds `realization_read_handles` via `build_compute_realization_inputs`,
//! runs `insert_shell_extract_upstream`, and threads a persistent cache key
//! plus warm state through `run_compute_dispatch`. Re-dispatching here would
//! trade a visibly-empty sentinel for an invisibly-wrong number on every
//! realization-bearing / shell-route target, and would double every FEA solve.
//! Copying the template-scope value is bit-exact with the full lowering at zero
//! extra solve cost.

use std::cell::OnceCell;
use std::collections::HashSet;

use reify_compiler::TopologyTemplate;
use reify_core::{Diagnostic, ValueCellId};
use reify_ir::{CompiledFunction, Value, ValueMap};

use crate::snapshot::Snapshot;

/// The `@optimized`-carrying function names of one `functions` slice, built at
/// most once per instance TREE.
///
/// The index OWNS the slice it indexes, so a cache can only ever be consulted
/// against the functions it was built from — [`optimized_target_of`] takes no
/// second slice to get wrong. (A lifetime alone would not give that: two
/// distinct `&'f [CompiledFunction]`s type-check identically.)
///
/// Lazy rather than eager: an instance whose cells contain no
/// `UserFunctionCall` at all never reaches the lookup and must not pay
/// O(|functions|) for the privilege.
///
/// RESIDUAL COST, stated rather than implied: the build is still paid once per
/// top-level [`super::elaborate_child_instance`] call — i.e. once per sub, and
/// once per COLLECTION ELEMENT, since `engine_eval.rs` loops elements at its
/// own call site. Collapsing that last factor means building the index where
/// the slice is owned for the whole pass (`engine_eval.rs`) and passing it in;
/// tracked by #7267.
pub(crate) struct OptimizedNameIndex<'f> {
    functions: &'f [CompiledFunction],
    names: OnceCell<HashSet<&'f str>>,
}

impl<'f> OptimizedNameIndex<'f> {
    pub(crate) fn new(functions: &'f [CompiledFunction]) -> Self {
        Self {
            functions,
            names: OnceCell::new(),
        }
    }

    fn names(&self) -> &HashSet<&'f str> {
        self.names.get_or_init(|| {
            self.functions
                .iter()
                .filter(|f| f.optimized_target.is_some())
                .map(|f| f.name.as_str())
                .collect()
        })
    }
}

/// The ONE `@optimized`-cell probe: `Some(target)` iff `expr` is a
/// `UserFunctionCall` that overload-resolves to a function carrying an
/// `@optimized("target")` attribute.
///
/// The same shape rule is spelled a second time as
/// `engine_eval::is_optimized_userfn_cell`, which the two TEMPLATE-scope
/// lowering sites gate on. The two must agree exactly — a cell that is
/// `@optimized` at one scope and not the other either re-folds a dispatched
/// value through plain `eval_expr` (clobbering it) or leaves an instance cell
/// body-inlined forever. This function is the definition;
/// `is_optimized_userfn_cell` is `optimized_target_of(..).is_some()` over the
/// same inputs. Collapsing it into a call to this one needs an edit to
/// `engine_eval.rs`, which #6662 held no lock on, and is tracked by #7130;
/// meanwhile `optimized_probe_agrees_with_engine_eval_is_optimized_userfn_cell`
/// (below) asserts the equivalence across every shape either probe can see, so
/// a divergence reds rather than silently splitting.
///
/// The [`OptimizedNameIndex`] lookup runs BEFORE
/// `find_matching_compiled_function`, whose multi-tier overload scan walks
/// per-candidate param/arg types over the whole (stdlib-inclusive) slice, and
/// which `eval_child_expr` then runs AGAIN on the fallback path — so without
/// the pre-filter an ordinary function-call let pays full overload resolution
/// twice per instance. The pre-filter is exact, not approximate:
/// `find_matching_compiled_function` only ever returns a candidate with
/// `f.name == function_name` (every tier's `arity_match` requires it), so if no
/// candidate of that name carries an `optimized_target` the full scan cannot
/// produce one either.
pub(crate) fn optimized_target_of(
    expr: &reify_ir::CompiledExpr,
    optimized_names: &OptimizedNameIndex<'_>,
) -> Option<String> {
    let reify_ir::CompiledExprKind::UserFunctionCall {
        function_name,
        args,
    } = &expr.kind
    else {
        return None;
    };
    if !optimized_names.names().contains(function_name.as_str()) {
        return None;
    }
    reify_expr::find_matching_compiled_function(optimized_names.functions, function_name, args)
        .and_then(|f| f.optimized_target.clone())
}

/// Outcome of probing an instance-scope cell for `@optimized` reuse
/// ([`resolve_optimized_instance_cell`]).
pub(super) enum OptimizedInstanceResolution {
    /// Not an `@optimized` `UserFunctionCall` — evaluate normally.
    NotOptimized,
    /// The template-scope cell's dispatched value, safe to commit verbatim at
    /// instance scope because every input this call reads compares equal
    /// between the two scopes.
    Reuse(Value),
    /// An `@optimized` call whose template-scope value CANNOT be proven to
    /// apply to this instance. The caller falls back to body-inlining (today's
    /// behaviour); whether anything is REPORTED is
    /// [`report_optimized_instance_decline`]'s decision, and `cause` is what it
    /// decides on.
    Unreusable { target: String, cause: DeclineCause },
}

/// WHICH of the two declining conditions [`resolve_optimized_instance_cell`]
/// hit — structural, so [`report_optimized_instance_decline`] filters on the
/// condition rather than on human-readable text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeclineCause {
    /// A cell in the call's DIRECT read set has a different value at instance
    /// scope than at template scope — a constructor override, virtually always.
    /// Genuinely instance-specific and genuinely degrading (the instance falls
    /// back to the `.ri` body's sentinel), so this is the cause that is
    /// REPORTED, and `detail` names the input that differed.
    ///
    /// The detail rides on this variant rather than beside it so that the one
    /// cause with a message to print is the one cause that carries text: no
    /// string is built for an outcome that is filtered out.
    InputsDiffer { detail: String },
    /// The template-scope OUTPUT cell holds no value at all. NOT reported —
    /// under [`report_optimized_instance_decline`]'s registered-target gate
    /// this cause has no honest message to print:
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
    ///   exactly that cell, more accurately than this site could.
    ///
    /// The VALUE is body-inlined either way, unchanged.
    NoTemplateValue,
}

/// Byte budget for ONE rendered input value inside a
/// [`DeclineCause::InputsDiffer`] detail.
///
/// Chosen so the assembled warning stays a readable paragraph: the detail
/// carries TWO rendered sides plus a fixed prose frame, and
/// [`report_optimized_instance_decline`] then wraps that in a fixed body of its
/// own. The unit-test ceiling on the detail is DERIVED from this constant — two
/// budgets plus the frame — rather than tuned against a measured string, so
/// moving this number moves the ceiling by construction.
const INPUT_VALUE_RENDER_BUDGET: usize = 120;

/// A [`std::fmt::Write`] sink that REFUSES — rather than truncates — the first
/// chunk that would carry it past [`INPUT_VALUE_RENDER_BUDGET`].
///
/// WHY ABORT RATHER THAN TRUNCATE. `Value` derives `Debug`, so `{:?}` over a
/// `SampledField`, `Matrix` or mesh-bearing `List` walks the entire payload.
/// Formatting first and cutting the result afterwards would bound only the
/// string that is KEPT, while still paying the full multi-megabyte transient
/// allocation and the full traversal — which is the cost this bound exists to
/// remove — and a byte-wise cut can split a UTF-8 character.
///
/// Returning `Err(fmt::Error)` is both safe and effective because derived
/// `Debug` and std's `debug_struct` / `debug_tuple` / `debug_list` builders
/// LATCH the first error and propagate it rather than unwrapping: each later
/// entry runs through an `and_then` whose body is skipped, so no panic and no
/// further per-element rendering. MEASURED on a 20_000-element `List<Real>`: 6
/// element renders before the abort, against 20_000 renders and a 460_000-byte
/// string unbounded — and the same 6 through an `Option` wrapper and through a
/// nested list, so neither wrapping nor depth defeats the latch.
///
/// Refusing WHOLE chunks, rather than filling up to the budget, leaves the
/// buffer on both a character boundary and a boundary between formatter writes,
/// so whatever it holds is always intact text.
struct BoundedValueRender {
    rendered: String,
}

impl std::fmt::Write for BoundedValueRender {
    fn write_str(&mut self, chunk: &str) -> std::fmt::Result {
        if self.rendered.len() + chunk.len() > INPUT_VALUE_RENDER_BUDGET {
            return Err(std::fmt::Error);
        }
        self.rendered.push_str(chunk);
        Ok(())
    }
}

/// Render one input `Value` for a decline message: its `Debug` form verbatim
/// when that fits [`INPUT_VALUE_RENDER_BUDGET`], else a contents-free summary
/// naming the variant alone.
///
/// Verbatim-when-small is the point, not a concession. A decline's input
/// divergence is virtually always a constructor override of a scalar, where
/// `Int(10)` vs `Int(3)` IS the answer — summarising every side uniformly would
/// print `Int vs Int` and make the message strictly less actionable than the
/// unbounded one it replaces.
///
/// The fallback delegates to [`Value::kind_name`] rather than spelling a second
/// variant-name table: that method is the existing single source of truth, is
/// exhaustive-by-construction (it forbids a `_` arm, so a newly added `Value`
/// variant fails to compile instead of degrading silently), and its own doc
/// already states it exists because "a `SampledField` or `Matrix` payload could
/// be enormous, and this string reaches user-facing surfaces" — which is
/// precisely this surface.
fn render_input_value(value: &Value) -> String {
    use std::fmt::Write as _;

    let mut sink = BoundedValueRender {
        rendered: String::new(),
    };
    match write!(sink, "{value:?}") {
        Ok(()) => sink.rendered,
        Err(_) => format!("{}(…)", value.kind_name()),
    }
}

/// Decide whether an instance-scope cell may carry the template-scope cell's
/// already-dispatched `@optimized` value.
///
/// SOUNDNESS. The arg expressions are literally the same `CompiledExpr`s at
/// both scopes (compiled once, in the child template's scope), so the call's
/// result is a pure function of the values its reads resolve to. Comparing the
/// DIRECT read set is therefore sufficient — transitive dependencies are
/// already folded into the direct reads' values. The comparison is conservative
/// in the right direction: present-in-one-map vs absent-in-the-other compares
/// unequal, so the helper declines rather than guesses.
///
/// SHAPE-EXACTNESS is inherited from template scope, not re-invented: the probe
/// is [`optimized_target_of`]. So a cell that merely WRAPS an `@optimized` call
/// (`let m = limit - solve(..).x`) is left alone at BOTH scopes, and a future
/// change to overload resolution moves both together.
///
/// STALENESS. `Reuse` copies the template-scope value as a ONE-TIME SNAPSHOT
/// taken during sub elaboration; the copy itself is never re-run.
/// `Engine::redispatch_geometry_consuming_compute_nodes` (engine_build.rs)
/// re-dispatches a geometry-consuming compute node LATER, inside `build()`,
/// once its args have hydrated to kernel-backed `GeometryHandle`s — and it
/// writes only `node_data.output_value_cells[0]`, which is TEMPLATE-scoped
/// because every `ComputeNode` is lowered at template scope. A reused instance
/// cell would otherwise keep the degraded first-dispatch result while the
/// template cell got the corrected one.
///
/// The mechanism was a MISSING DEPENDENCY EDGE, not the snapshot as such: the
/// trace committed for a reused cell was the raw
/// `extract_dependency_trace(expr)`, which records the call's ARG reads and
/// nothing about the cell the value was copied from, so `dirty.rs`'s
/// `compute_eval_set` had no edge that could mark the instance cell dirty when
/// the template cell moved. `elaborate_child_lets_only` — the one reuse site
/// that commits a trace at all — now pushes `{child_template}.{member}` onto
/// the committed trace's `reads`, so the existing dirty machinery invalidates
/// the instance cell whenever the template cell is rewritten. The other two
/// reuse sites need no counterpart: phase 1.5's scratch arm writes only the
/// in-memory `overlay` (phase 2 owns the commit), and
/// `elaborate_child_params_only` commits `DependencyTrace::default()` for EVERY
/// param, reused or not.
///
/// What remains unpinned end to end is the narrower claim that the geometry
/// case never reaches the reuse arm at all — BELIEVED so because a
/// geometry-typed arg's instance value carries an instance-scoped realization
/// ref, so the read comparison declines first. The DISCRIMINATOR that belief
/// rests on is pinned here by
/// `resolve_optimized_instance_cell_declines_on_instance_scoped_realization_ref`;
/// what no fixture on this branch reaches is the same path through `build()`,
/// which needs a kernel-backed handle this crate's tests cannot mint. Tracked
/// by #7184.
///
/// `instance_values` is this instance's map (`child_values` in phase 2, the
/// running `overlay` in phase 1.5); `global_values` is the global map holding
/// the template-scope pass's results. Both are keyed template-scoped, so the
/// same `ValueCellId` addresses the two scopes' copies of one cell.
///
/// ## Why the two instance maps reach the SAME decision
///
/// The gate runs against two differently-built instance maps — phase 1.5's
/// `overlay` (params + scratch lets + ONE level of nested-sub member
/// projections + the collapsed `__self` aliases) and phase 2's `child_values`
/// (params + the multi-level projection BFS + collapsed values) — and a key one
/// map carries while the other does not would compare `Some(v)` vs `None` and
/// flip the gate. MEASURED on this branch, that divergence class is not
/// reachable, because the only keys the two maps differ on are ones no read set
/// can name:
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
pub(super) fn resolve_optimized_instance_cell<R>(
    expr: &reify_ir::CompiledExpr,
    optimized_names: &OptimizedNameIndex<'_>,
    child_template: &TopologyTemplate,
    member: &str,
    reads: impl FnOnce() -> R,
    instance_values: &ValueMap,
    global_values: &ValueMap,
) -> OptimizedInstanceResolution
where
    R: AsRef<[ValueCellId]>,
{
    let Some(target) = optimized_target_of(expr, optimized_names) else {
        return OptimizedInstanceResolution::NotOptimized;
    };

    // Input equality over the DIRECT read set. `reads` yields
    // `extract_dependency_trace(expr).reads` — the same function that builds
    // phase 2's topological-sort edges, so the reuse gate and the evaluation
    // order can never disagree about what a cell depends on. It is a `FnOnce`,
    // not a slice, so the trace is materialised ONLY on the `@optimized` path:
    // two of the three call sites have no pre-built trace to hand over, and
    // would otherwise walk the expression tree for every scratch let / param
    // default of every instance only to discard it on `NotOptimized`.
    let reads = reads();
    for read in reads.as_ref() {
        let instance_val = instance_values.get(read);
        let global_val = global_values.get(read);
        if instance_val != global_val {
            // Each side goes through [`render_input_value`]'s budget, so an
            // enormous input summarises to its variant instead of pouring its
            // whole payload into a user-facing warning.
            let render = |side: Option<&Value>| match side {
                Some(value) => render_input_value(value),
                None => "None".to_string(),
            };
            return OptimizedInstanceResolution::Unreusable {
                target,
                cause: DeclineCause::InputsDiffer {
                    detail: format!(
                        "input {}.{} differs from the template's ({} vs {})",
                        read.entity,
                        read.member,
                        render(instance_val),
                        render(global_val)
                    ),
                },
            };
        }
    }

    // The template-scope output cell. Absent under either of two conditions
    // this function cannot tell apart (it holds no graph), and deliberately
    // does not guess between — see [`DeclineCause::NoTemplateValue`].
    let template_cell = ValueCellId::new(&child_template.name, member);
    match global_values.get(&template_cell) {
        Some(value) => OptimizedInstanceResolution::Reuse(value.clone()),
        None => OptimizedInstanceResolution::Unreusable {
            target,
            cause: DeclineCause::NoTemplateValue,
        },
    }
}

/// Did TEMPLATE scope actually DISPATCH this cell through a compute trampoline,
/// rather than body-inline it?
///
/// `engine_eval.rs`'s lowering site inserts a `ComputeNode` carrying the cell in
/// `output_value_cells` ONLY inside its `if self.compute_dispatch(&target)
/// .is_some()` arm; the unregistered arm pushes its own `no registered compute
/// trampoline` Error and falls straight through to body-inlining without ever
/// touching the graph. The presence of that node is therefore an exact,
/// structural answer, needing neither the registry (which this module has no
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
/// Three filters:
///
/// * CAUSE GATE. Only [`DeclineCause::InputsDiffer`] is reported; see
///   [`DeclineCause::NoTemplateValue`] for why its sibling has nothing honest
///   to say. Filtering on the structural cause rather than on the presence of a
///   text-matching Error keeps this independent of any other site's wording.
/// * REGISTERED-TARGET GATE. Without it the warning fired, with actively
///   misleading text, in the plain `reify check` shape — which registers NO
///   trampolines — for every sub carrying a constructor override on a param an
///   `@optimized` call reads. MEASURED: `Outer.a.r` got `Int(11)`, the correct
///   per-instance body-inlined answer, alongside "falling back to
///   body-inlining"; there was no dispatch to fall back FROM, and template
///   scope had already emitted its own `no registered compute trampoline` Error
///   for the same condition. `reify check` is the default CLI path, so that was
///   a spurious warning on every model instantiating an `@optimized`-bearing
///   structure with an override.
/// * PER-TEMPLATE-CELL DEDUPE. The decline is a fact about `(target, child
///   template, member)`, not about one instance: three subs of one template
///   with three distinct overrides are one authoring fact, and a keyed
///   collection `sub` routes every element through this site — N warnings for
///   one fact. `diagnostics` is the one vec `engine_eval.rs` threads through
///   the whole sub-elaboration pass (collection elements included), so a scan
///   over it dedupes across sibling and collection instances alike, which a set
///   scoped to one `elaborate_child_instance` call could not.
///
///   THE KEY MUST BE DELIMITED. The key is a literal prefix of the message, and
///   `target` is delimited by `{:?}`'s closing quote and the template name by
///   the following `.` — but `{member}` sits at the end, so without a
///   terminator it is open-ended. Two members of ONE child template sharing ONE
///   target, where the shorter name is a proper prefix of the longer (`r`/`r2`,
///   `k`/`k2`, `defl`/`deflection`), then collide: the longer member's
///   already-emitted message matches the shorter member's key and the shorter
///   cell's decline is SILENTLY DROPPED — defeating the LOUD contract for
///   exactly the cells this report exists to surface. MEASURED on `let r2 =
///   dblpc(x); let r = dblpc(r2)`: two degraded instance cells, ONE warning,
///   naming only `InnerPC.r2`. The trailing `:` fixes it — `:` is not a valid
///   identifier character — and the key stays a literal prefix so the scan
///   needs no side table. Guarded by
///   `instance_scope_optimized_decline_dedupe_is_per_cell_not_per_prefix`.
///
///   The prefix scan is a stand-in for the structured form the house rule asks
///   for (match on `DiagnosticCode`, not on message substrings): keying it on a
///   `DiagnosticCode::OptimizedInstanceReuseDeclined` needs a variant added to
///   `crates/reify-core/src/diagnostics.rs`, which #6662 held no lock on. Filed
///   from #6662's amendment pass as ticket `tkt_0RTGTE246DRBM0719KQEJWWPYP` —
///   not cited as `#NNNN` because the curator assigns the task id
///   asynchronously and an unresolvable cite would be an orphan.
///
///   COST. Both scans run on the decline path only, and dedupe keeps the
///   scanned vec short — but it does NOT bound how many times that vec is
///   scanned: in the very shape the dedupe was added for (a keyed collection
///   `sub` where every element declines) this function is still entered once
///   per element. The dedupe scan is therefore ordered FIRST, so a repeat entry
///   costs one `starts_with` walk of a short vec and never touches
///   [`template_cell_was_dispatched`]'s unbounded `compute_nodes` walk. Making
///   the repeat case O(1) needs a memo owned by `engine_eval.rs` — #7267.
pub(super) fn report_optimized_instance_decline(
    diagnostics: &mut Vec<Diagnostic>,
    snapshot: &Snapshot,
    child_template: &TopologyTemplate,
    member: &str,
    scoped_entity: &str,
    target: &str,
    cause: &DeclineCause,
) {
    let DeclineCause::InputsDiffer { detail } = cause else {
        return;
    };
    let key = format!(
        "@optimized target {:?} on {}.{}:",
        target, child_template.name, member
    );
    // Dedupe BEFORE the graph scan: both are pure predicates, so the emitted
    // set is identical either way, but this ordering keeps the already-reported
    // case off the unbounded `compute_nodes` walk entirely.
    if diagnostics.iter().any(|d| d.message.starts_with(&key)) {
        return;
    }
    let template_cell = ValueCellId::new(&child_template.name, member);
    if !template_cell_was_dispatched(snapshot, &template_cell) {
        return;
    }
    diagnostics.push(Diagnostic::warning(format!(
        "{key} instance scope {scoped_entity}.{member} cannot reuse the \
         template's dispatched value ({detail}) — falling back to body-inlining. \
         Reported once per template cell: every other instance of {} whose \
         inputs differ declines the same way (per-instance dispatch under \
         constructor overrides is tracked by #6592).",
        child_template.name,
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::extract_dependency_trace;

    /// [`optimized_target_of`] and `engine_eval::is_optimized_userfn_cell` are
    /// the SAME shape rule, and a divergence between them is silent and
    /// expensive (a cell `@optimized` at one scope but not the other either has
    /// its dispatched value re-folded through plain `eval_expr` or stays
    /// body-inlined forever). Until #7130 collapses them into one call, this
    /// asserts the equivalence directly, over every shape either probe can see:
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
            // the "merely WRAPS" case left alone at BOTH scopes.
            ("d_wrapping", false),
        ] {
            let expr = reify_test_support::get_let_expr_in(&module, "ProbeShapes", cell);
            // A fresh index per probe: this test is about the VERDICT, not the
            // memoisation.
            let mine = optimized_target_of(expr, &OptimizedNameIndex::new(functions));
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

    /// The [`DeclineCause::NoTemplateValue`] branch guards the case where the
    /// child structure got no template-scope pass at all, because
    /// `engine_eval.rs` keys its `@optimized` lowering on `for template in
    /// &module.templates` and a PRELUDE/stdlib structure is deliberately not
    /// merged into that list (see [`super::find_template_in_scope`]). It is not
    /// incidental — the FEA/solver targets #6662 exists for live in the stdlib,
    /// and `find_template_in_scope` shows prelude-typed subs ARE elaborated on
    /// this path — but no end-to-end fixture can reach it: building a prelude
    /// module a user module can also name at COMPILE time needs a `&'static`
    /// prelude the test-support compile helpers do not expose. So the branch is
    /// covered where it is decided, with a `global_values` map that simply
    /// lacks the template-scope output cell.
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
            &OptimizedNameIndex::new(&module.functions),
            template,
            "r",
            || extract_dependency_trace(expr).reads,
            &instance_values,
            &global_values,
        );
        match resolution {
            OptimizedInstanceResolution::Unreusable { target, cause } => {
                assert_eq!(target, "test::prelude_target");
                // The structural cause is the whole contract: it is what
                // `report_optimized_instance_decline` filters on, and the
                // absent-output branch carries no message precisely because
                // neither of its two readings has an honest one to print.
                assert_eq!(cause, DeclineCause::NoTemplateValue);
            }
            OptimizedInstanceResolution::Reuse(v) => {
                panic!("must not reuse a value the template scope never produced (got {v:?})")
            }
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`prelude_opt` carries @optimized; the probe must see it")
            }
        }

        // Same fixture, template cell PRESENT ⇒ the branch above is genuinely
        // selected by absence and not by something else in the shape.
        global_values.insert(ValueCellId::new("PreludeLike", "r"), Value::Int(777));
        match resolve_optimized_instance_cell(
            expr,
            &OptimizedNameIndex::new(&module.functions),
            template,
            "r",
            || extract_dependency_trace(expr).reads,
            &instance_values,
            &global_values,
        ) {
            OptimizedInstanceResolution::Reuse(v) => assert_eq!(v, Value::Int(777)),
            OptimizedInstanceResolution::Unreusable { cause, .. } => panic!(
                "with the template cell present and inputs equal this must Reuse, \
                 got Unreusable({cause:?})"
            ),
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`prelude_opt` carries @optimized; the probe must see it")
            }
        }
    }

    /// The STALENESS note argues that the geometry-consuming case never reaches
    /// the `Reuse` arm, because a geometry-typed arg's INSTANCE value carries
    /// an instance-scoped realization ref and the read comparison therefore
    /// declines first. That is the load-bearing soundness claim for every
    /// geometry-consuming `@optimized` target in the stdlib (`fdm::slice`,
    /// `fdm::as_printed_material_r_fast`).
    ///
    /// The blocker recorded there ("needs a kernel-backed handle this crate's
    /// tests cannot mint") is real only for the END-TO-END path through
    /// `build()`. The DISCRIMINATOR is fully expressible here:
    /// `Value::GeometryHandle` equality is `GeometryHandleRef`'s, which compares
    /// `realization_ref` + `upstream_values_hash` and deliberately EXCLUDES
    /// `kernel_handle` (reify-ir/src/value.rs, GHR-β §DD), and
    /// `RealizationNodeId::new(entity, index)` is entity-scoped — so two
    /// symbolic (`kernel_handle: None`) handles differing only in their ref's
    /// ENTITY are exactly the instance-vs-template shape the claim depends on.
    ///
    /// This test reds if `GeometryHandleRef`'s equality ever widens to include
    /// `kernel_handle` (the `Reuse` arm would start declining), or if
    /// realization ids stop being entity-scoped (the `InputsDiffer` arms would
    /// start reusing — the actual staleness hazard).
    #[test]
    fn resolve_optimized_instance_cell_declines_on_instance_scoped_realization_ref() {
        use reify_core::identity::RealizationNodeId;

        let source = r#"
            @optimized("test::geom_target")
            fn geom_opt(g : Solid) -> Int {
                7
            }

            structure GeomLike {
                param g : Solid = box(1mm, 1mm, 1mm)
                let r = geom_opt(g)
            }
        "#;
        let module = reify_test_support::compile_source_with_stdlib(source);
        let errors = reify_test_support::collect_errors(&module.diagnostics);
        assert!(errors.is_empty(), "fixture must compile clean: {errors:?}");
        let template = module
            .templates
            .iter()
            .find(|t| t.name == "GeomLike")
            .expect("GeomLike template");
        let expr = reify_test_support::get_let_expr_in(&module, "GeomLike", "r");
        let names = OptimizedNameIndex::new(&module.functions);

        // Symbolic handles only — `kernel_handle: None` is the eval-path mint
        // (task #4652), and is precisely what makes this shape constructible
        // without a kernel.
        let handle = |entity: &str, hash: u8| Value::GeometryHandle {
            realization_ref: RealizationNodeId::new(entity, 0),
            upstream_values_hash: [hash; 32],
            kernel_handle: None,
        };

        // The template-scope map: the arg's own realization is entity-scoped to
        // the TEMPLATE, and the output cell holds a dispatched value that would
        // be reused if the gate let it through.
        let mut global_values = ValueMap::new();
        global_values.insert(ValueCellId::new("GeomLike", "g"), handle("GeomLike", 1));
        global_values.insert(ValueCellId::new("GeomLike", "r"), Value::Int(777));

        // (a) INSTANCE-SCOPED realization ref ⇒ decline. `Asm.beam` is the
        // entity an instantiated `sub beam : GeomLike` realizes under, so its
        // ref can never equal the template's.
        let mut instance_values = ValueMap::new();
        instance_values.insert(ValueCellId::new("GeomLike", "g"), handle("Asm.beam", 1));
        match resolve_optimized_instance_cell(
            expr,
            &names,
            template,
            "r",
            || extract_dependency_trace(expr).reads,
            &instance_values,
            &global_values,
        ) {
            OptimizedInstanceResolution::Unreusable { cause, .. } => assert!(
                matches!(cause, DeclineCause::InputsDiffer { .. }),
                "a geometry arg whose realization ref is instance-scoped must \
                 decline as InputsDiffer — this is the comparison the STALENESS \
                 note's 'should decline first' rests on: {cause:?}"
            ),
            OptimizedInstanceResolution::Reuse(v) => panic!(
                "REUSED a geometry-consuming @optimized value across scopes ({v:?}) — \
                 the instance cell would keep the pre-hydration result while \
                 `redispatch_geometry_consuming_compute_nodes` corrected only the \
                 template cell"
            ),
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`geom_opt` carries @optimized; the probe must see it")
            }
        }

        // (b) Same entity, DIFFERENT upstream hash ⇒ decline too. Pins the
        // second of `GeometryHandleRef`'s two equality fields, so a widening of
        // the ref's identity is caught from both sides.
        let mut same_entity_other_hash = ValueMap::new();
        same_entity_other_hash.insert(ValueCellId::new("GeomLike", "g"), handle("GeomLike", 2));
        assert!(
            matches!(
                resolve_optimized_instance_cell(
                    expr,
                    &names,
                    template,
                    "r",
                    || extract_dependency_trace(expr).reads,
                    &same_entity_other_hash,
                    &global_values,
                ),
                OptimizedInstanceResolution::Unreusable {
                    cause: DeclineCause::InputsDiffer { .. },
                    ..
                }
            ),
            "a geometry arg with the same realization ref but a different \
             upstream_values_hash must also decline"
        );

        // (c) Companion arm: identical refs ⇒ Reuse. Without this the two
        // declines above would be satisfied by a gate that declines on EVERY
        // geometry value, which would prove nothing about the discriminator.
        match resolve_optimized_instance_cell(
            expr,
            &names,
            template,
            "r",
            || extract_dependency_trace(expr).reads,
            &global_values.clone(),
            &global_values,
        ) {
            OptimizedInstanceResolution::Reuse(v) => assert_eq!(
                v,
                Value::Int(777),
                "with value-identical geometry inputs the template's dispatched \
                 value must be reused verbatim"
            ),
            OptimizedInstanceResolution::Unreusable { cause, .. } => {
                panic!("identical geometry inputs must Reuse, got Unreusable({cause:?})")
            }
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`geom_opt` carries @optimized; the probe must see it")
            }
        }
    }

    /// ITEM 1 of task #7021. The [`DeclineCause::InputsDiffer`] detail reaches a
    /// user-facing warning, and `Value` derives `Debug` — so a `SampledField`,
    /// `Matrix` or mesh-bearing `List` input renders its WHOLE payload into the
    /// message, paying a multi-megabyte transient allocation and a full `Debug`
    /// traversal to print something no reader can use.
    ///
    /// Two arms, because the bound is only half the contract:
    ///
    /// * (a) THE BOUND — an enormous value must leave the detail short, while
    ///   still naming the read cell and the elided value's VARIANT. The bound is
    ///   met by SUMMARISING, not by dropping the identification the reader needs
    ///   in order to find the input that diverged.
    /// * (b) THE COMPANION — a small scalar override, which is the dominant real
    ///   case, must still print its payload verbatim. Without this arm, (a) would
    ///   be satisfiable by printing variant names only, and `Int vs Int` is
    ///   strictly less actionable than the message this task set out to improve.
    ///
    /// The gate is type-blind (a raw `ValueMap` lookup plus `Value` equality), so
    /// the maps are hand-built here exactly as
    /// `resolve_optimized_instance_cell_declines_on_instance_scoped_realization_ref`
    /// hand-mints its `GeometryHandle`s — the declaration's type is not what the
    /// comparison reads.
    #[test]
    fn input_divergence_detail_is_bounded_for_an_enormous_value() {
        let source = r#"
            @optimized("test::bounded")
            fn bounded_opt(xs : List<Real>) -> Int {
                7
            }

            structure BoundedLike {
                param xs : List<Real> = [1.0]
                let r = bounded_opt(xs)
            }
        "#;
        let module = reify_test_support::compile_source_with_stdlib(source);
        let errors = reify_test_support::collect_errors(&module.diagnostics);
        assert!(errors.is_empty(), "fixture must compile clean: {errors:?}");
        let template = module
            .templates
            .iter()
            .find(|t| t.name == "BoundedLike")
            .expect("BoundedLike template");
        let expr = reify_test_support::get_let_expr_in(&module, "BoundedLike", "r");
        let names = OptimizedNameIndex::new(&module.functions);

        // The read cell is `BoundedLike.xs`; the template OUTPUT cell is present
        // in `global_values` so the READ LOOP is what decides, not the
        // absent-output branch.
        let declining_detail = |instance: Value, global: Value| -> String {
            let mut instance_values = ValueMap::new();
            instance_values.insert(ValueCellId::new("BoundedLike", "xs"), instance);
            let mut global_values = ValueMap::new();
            global_values.insert(ValueCellId::new("BoundedLike", "xs"), global);
            global_values.insert(ValueCellId::new("BoundedLike", "r"), Value::Int(777));
            match resolve_optimized_instance_cell(
                expr,
                &names,
                template,
                "r",
                || extract_dependency_trace(expr).reads,
                &instance_values,
                &global_values,
            ) {
                OptimizedInstanceResolution::Unreusable {
                    cause: DeclineCause::InputsDiffer { detail },
                    ..
                } => detail,
                OptimizedInstanceResolution::Unreusable { cause, .. } => {
                    panic!("two present-and-unequal inputs must decline as InputsDiffer: {cause:?}")
                }
                OptimizedInstanceResolution::Reuse(v) => {
                    panic!("unequal inputs must not reuse the template's value ({v:?})")
                }
                OptimizedInstanceResolution::NotOptimized => {
                    panic!("`bounded_opt` carries @optimized; the probe must see it")
                }
            }
        };

        // (a) THE BOUND. A 20_000-element list of long-decimal reals renders to
        // hundreds of KB under a bare `{:?}`.
        let enormous = Value::List(vec![Value::Real(1.234_567_890_123_4); 20_000]);
        let detail = declining_detail(enormous, Value::List(vec![Value::Real(1.0)]));
        assert!(
            detail.chars().count() <= 400,
            "the InputsDiffer detail must stay a readable fragment: the ceiling is \
             TWO per-side render budgets plus the fixed prose between them, so it \
             holds by construction rather than by tuning. Got {} chars: {:.200}…",
            detail.chars().count(),
            detail
        );
        assert!(
            detail.contains("BoundedLike.xs"),
            "the bound must not cost the reader the identity of the input that \
             diverged — the detail is the only place the read cell is named: {detail}"
        );
        assert!(
            detail.contains("List"),
            "an elided payload must still name its VARIANT, so the reader knows \
             what kind of value was too large to print: {detail}"
        );

        // (b) THE COMPANION. The small constructor-override case must keep
        // printing both payloads verbatim.
        let detail = declining_detail(Value::Int(10), Value::Int(3));
        assert!(
            detail.contains("Int(10)") && detail.contains("Int(3)"),
            "a small scalar override is the dominant real case and must render \
             verbatim on both sides; summarising it to `Int vs Int` would be \
             strictly less actionable than the unbounded message: {detail}"
        );
    }

    /// ITEM 2 of task #7021. The read gate compared two `Option<&Value>`s with a
    /// bare `!=`, so a cell PRESENT at one scope and ABSENT at the other was
    /// reported as a value that "differs from the template's" — an accusation of
    /// a constructor override against a read that very likely holds the identical
    /// value and is merely invisible.
    ///
    /// Both directions are reachable, not hypothetical:
    ///
    /// * INSTANCE-SIDE ABSENCE. `child_values` is seeded with the child
    ///   template's own params, the collapsed sub instances, and a BFS
    ///   projection over the subs phase 1.5 actually elaborated — so a read of a
    ///   member of a DECLINED sub (a collection, a keyed sub, a
    ///   `skip_reason_for_shape` miss, an unresolvable target, a cycle cut) is
    ///   absent at instance scope while the global map holds it from the
    ///   top-level pass.
    /// * TEMPLATE-SIDE ABSENCE. For a prelude/stdlib child structure — absent
    ///   from `module.templates`, the very population
    ///   [`DeclineCause::NoTemplateValue`]'s doc describes — the top-level pass
    ///   never seeds `{Tmpl}.{param}` globally, while
    ///   `elaborate_child_params_only` always seeds it into the instance map.
    ///
    /// Arm (c) is the DISCRIMINATOR: without it, (a) and (b) would be satisfied
    /// by reclassifying every decline, which would move the false positive
    /// rather than remove it. Arm (d) is the FROZEN INVARIANT: this task changes
    /// the message, never the value — every read that declined before declines
    /// after, and a pair that was equal (both absent) still reuses.
    #[test]
    fn presence_asymmetry_is_classified_apart_from_a_genuine_value_difference() {
        let source = r#"
            @optimized("test::asym")
            fn asym_opt(x : Int) -> Int {
                x + 1
            }

            structure AsymLike {
                param x : Int = 3
                let r = asym_opt(x)
            }
        "#;
        let module = reify_test_support::compile_source_with_stdlib(source);
        let errors = reify_test_support::collect_errors(&module.diagnostics);
        assert!(errors.is_empty(), "fixture must compile clean: {errors:?}");
        let template = module
            .templates
            .iter()
            .find(|t| t.name == "AsymLike")
            .expect("AsymLike template");
        let expr = reify_test_support::get_let_expr_in(&module, "AsymLike", "r");
        let names = OptimizedNameIndex::new(&module.functions);

        // The template OUTPUT cell is present on every arm, so the READ LOOP is
        // what decides and never the absent-output branch.
        let resolve = |instance: Option<Value>, global: Option<Value>| {
            let read = ValueCellId::new("AsymLike", "x");
            let mut instance_values = ValueMap::new();
            if let Some(value) = instance {
                instance_values.insert(read.clone(), value);
            }
            let mut global_values = ValueMap::new();
            if let Some(value) = global {
                global_values.insert(read, value);
            }
            global_values.insert(ValueCellId::new("AsymLike", "r"), Value::Int(777));
            resolve_optimized_instance_cell(
                expr,
                &names,
                template,
                "r",
                || extract_dependency_trace(expr).reads,
                &instance_values,
                &global_values,
            )
        };
        let declining_cause = |resolution| match resolution {
            OptimizedInstanceResolution::Unreusable { cause, .. } => cause,
            OptimizedInstanceResolution::Reuse(v) => panic!(
                "a read the gate cannot compare must still DECLINE — reuse here is \
                 the unsoundness #6662 exists to prevent (got {v:?})"
            ),
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`asym_opt` carries @optimized; the probe must see it")
            }
        };

        // (a) UNPROJECTED AT INSTANCE SCOPE — the ITEM 2 false positive.
        match declining_cause(resolve(None, Some(Value::Int(3)))) {
            DeclineCause::InputNotComparable { detail } => {
                assert!(
                    detail.contains("AsymLike.x"),
                    "the detail is the only place the unreadable input is named: {detail}"
                );
                assert!(
                    detail.contains("instance scope"),
                    "an input absent from the INSTANCE map must say so, so the \
                     reader looks at projection and not at overrides: {detail}"
                );
                assert!(
                    !detail.contains("differs"),
                    "the two values were never compared — one of them does not \
                     exist — so nothing may claim they differ: {detail}"
                );
            }
            other => panic!(
                "an input absent at instance scope is a VISIBILITY fact, not a \
                 value difference: {other:?}"
            ),
        }

        // (b) THE REVERSE DIRECTION — a prelude/stdlib child structure.
        match declining_cause(resolve(Some(Value::Int(3)), None)) {
            DeclineCause::InputNotComparable { detail } => {
                assert!(
                    detail.contains("AsymLike.x"),
                    "the detail is the only place the unreadable input is named: {detail}"
                );
                assert!(
                    detail.contains("template scope"),
                    "an input absent from the GLOBAL map must name TEMPLATE scope, \
                     so the two directions stay distinguishable in the message: {detail}"
                );
                assert!(
                    !detail.contains("differs"),
                    "the two values were never compared — one of them does not \
                     exist — so nothing may claim they differ: {detail}"
                );
            }
            other => panic!(
                "an input absent at template scope is a VISIBILITY fact, not a \
                 value difference: {other:?}"
            ),
        }

        // (c) THE DISCRIMINATOR. Both sides present and unequal is the one case
        // that genuinely differs, and must keep today's classification and
        // wording.
        match declining_cause(resolve(Some(Value::Int(10)), Some(Value::Int(3)))) {
            DeclineCause::InputsDiffer { detail } => assert!(
                detail.contains("differs from the template's"),
                "a genuine divergence must keep the wording it was always \
                 correct for: {detail}"
            ),
            other => panic!(
                "two present-and-unequal inputs genuinely differ and must stay \
                 InputsDiffer — reclassifying everything would move the false \
                 positive, not fix it: {other:?}"
            ),
        }

        // (d) THE FROZEN INVARIANT, other half. Two absent sides — the
        // guarded-group shape, whose member cells are in no `value_cells` at all
        // — compared EQUAL before and must still compare equal, so the
        // exhaustive match cannot have widened the declining set.
        match resolve(None, None) {
            OptimizedInstanceResolution::Reuse(v) => assert_eq!(
                v,
                Value::Int(777),
                "a read absent from BOTH maps was never a decline and must not \
                 become one: the declining set is frozen by this task"
            ),
            OptimizedInstanceResolution::Unreusable { cause, .. } => panic!(
                "a read absent at both scopes compares equal and must still \
                 reuse; declining here would widen the declining set: {cause:?}"
            ),
            OptimizedInstanceResolution::NotOptimized => {
                panic!("`asym_opt` carries @optimized; the probe must see it")
            }
        }
    }
}
