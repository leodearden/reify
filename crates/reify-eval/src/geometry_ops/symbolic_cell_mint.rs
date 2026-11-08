//! Kernel-free symbolic mint for a value cell whose plain evaluation returned
//! `Undef` (R3d, task #4900): a geometry cell gets a symbolic
//! `Value::GeometryHandle`, a selector-ctor cell a `Value::Selector`.
//!
//! The one copy shared by template scope (`engine_eval.rs`) and sub-instance
//! scope (`unfold.rs` at `eval()`, `engine_edit.rs`'s edit path, task #8102),
//! so the scopes mint identical values for identical inputs.

use std::collections::{HashMap, HashSet};

use reify_compiler::{TopologyTemplate, ValueCellKind};
use reify_core::{Diagnostic, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledFunction, Value, ValueMap};

use crate::cache::NodeId;
use crate::deps::{DependencyTrace, extract_dependency_trace, extract_realization_dependencies};
use crate::dirty::topological_sort;
use super::is_symbolic_eval_wired_selector_ctor;
use crate::graph::{CollectionSubInfo, EvaluationGraph, RealizationNodeData};

/// Which mint a cell tries first; the first `Some` wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolicMintOrder {
    GeometryHandleFirst,
    SelectorFirst,
}

impl SymbolicMintOrder {
    /// The order a cell of `kind` takes: a param (an `auto` one included) tries
    /// the geometry handle first, a let the selector. Every mint in this module
    /// derives its order here.
    fn for_kind(kind: ValueCellKind) -> Self {
        match kind {
            ValueCellKind::Let => Self::SelectorFirst,
            ValueCellKind::Param | ValueCellKind::Auto { .. } => Self::GeometryHandleFirst,
        }
    }
}

/// Where the realization backing a geometry cell is looked up.
#[derive(Clone, Copy)]
pub(crate) enum RealizationSource<'a> {
    /// The compiled template's declarations, which `eval()` has.
    Template(&'a TopologyTemplate),
    /// The graph's realization nodes, which carry the same operations, for the
    /// edit path, which has no template. This is the hook #8209 folds the
    /// `edit_param` walk's template-scope mint onto.
    Graph(&'a EvaluationGraph),
}

/// The scope a cell is minted in: the realizations of the template the cell
/// was COMPILED in, and the map it was evaluated against (keyed in that
/// template's scope).
pub(crate) struct SymbolicMintScope<'a> {
    pub(crate) realizations: RealizationSource<'a>,
    pub(crate) values: &'a ValueMap,
    pub(crate) functions: &'a [CompiledFunction],
    pub(crate) meta_map: &'a HashMap<String, HashMap<String, String>>,
}

/// A cell's value after [`mint_symbolic_if_undef`].
pub(crate) struct SymbolicMintOutcome {
    pub(crate) value: Value,
    /// `true` iff the evaluated value was `Undef` and the mint replaced it.
    pub(crate) minted: bool,
}

/// `evaluated` unchanged unless it is `Undef`; then `cell_id`, a cell of `kind`
/// in the template `scope.realizations` belongs to, is minted symbolically,
/// trying the two mints in the order [`SymbolicMintOrder::for_kind`] gives
/// `kind`. A selector kind-closure violation leaves the cell `Undef`, with its
/// diagnostic pushed to `diagnostics`.
///
/// Realization identity: a minted handle names the template's
/// `RealizationDecl` id at EVERY scope and folds its `upstream_values_hash` over
/// `scope.values`. An override-free sub instance therefore mints a handle equal
/// to the template's, and an override reaching the realization's inputs mints
/// an unequal one. The build path agrees: `seed_cross_sub_named_steps` gives a
/// no-args sub the template's kernel handles and re-realizes only under
/// overrides.
pub(crate) fn mint_symbolic_if_undef(
    evaluated: Value,
    kind: ValueCellKind,
    cell_id: &ValueCellId,
    default_expr: Option<&CompiledExpr>,
    scope: &SymbolicMintScope<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> SymbolicMintOutcome {
    if !matches!(evaluated, Value::Undef) {
        return SymbolicMintOutcome {
            value: evaluated,
            minted: false,
        };
    }
    let order = SymbolicMintOrder::for_kind(kind);
    match mint_symbolic(order, cell_id, default_expr, scope, diagnostics) {
        Some(value) if !matches!(value, Value::Undef) => SymbolicMintOutcome {
            value,
            minted: true,
        },
        _ => SymbolicMintOutcome {
            value: Value::Undef,
            minted: false,
        },
    }
}

/// [`mint_symbolic_if_undef`] for a sub-instance cell, addressed by its
/// TEMPLATE-scoped id `template_cell`; `scope.values` holds the instance's
/// values keyed in the template's scope.
///
/// The one home of the instance-scope diagnostics policy: the mint's
/// diagnostics are discarded. They are type-level, a property of the expression
/// every instance shares with its template, so template scope reports each one
/// once. Reporting them here as well would repeat it per instance, and per slot
/// of a collection.
pub(crate) fn mint_template_keyed_instance_cell(
    evaluated: Value,
    kind: ValueCellKind,
    template_cell: &ValueCellId,
    expr: &CompiledExpr,
    scope: &SymbolicMintScope<'_>,
) -> Value {
    mint_symbolic_if_undef(
        evaluated,
        kind,
        template_cell,
        Some(expr),
        scope,
        &mut Vec::new(),
    )
    .value
}

fn mint_symbolic(
    order: SymbolicMintOrder,
    cell_id: &ValueCellId,
    default_expr: Option<&CompiledExpr>,
    scope: &SymbolicMintScope<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    let geometry_handle = || match scope.realizations {
        RealizationSource::Template(template) => {
            crate::Engine::mint_symbolic_geometry_handle_for_cell(
                cell_id,
                &template.realizations,
                scope.values,
                scope.functions,
                scope.meta_map,
            )
        }
        RealizationSource::Graph(graph) => {
            crate::Engine::mint_symbolic_geometry_handle_for_cell_from_graph(
                cell_id,
                graph,
                scope.values,
                scope.functions,
                scope.meta_map,
            )
        }
    };
    let mut selector = || {
        default_expr.and_then(|expr| {
            super::try_eval_symbolic_topology_selector(
                expr,
                scope.values,
                diagnostics,
            )
        })
    };
    match order {
        SymbolicMintOrder::GeometryHandleFirst => geometry_handle().or_else(selector),
        SymbolicMintOrder::SelectorFirst => selector().or_else(geometry_handle),
    }
}

/// The graph-backed scope the edit path mints a sub-instance cell in, since
/// none of its sites has a template: the `edit_param` walk, `edit_param`'s
/// collection resize and grown-instance reseed, and `edit_source`'s
/// collection resize. A caller creating an instance's cells one at a time
/// commits them in [`slot_commit_order`].
pub(crate) struct InstanceMintScope<'a> {
    pub(crate) graph: &'a EvaluationGraph,
    pub(crate) values: &'a ValueMap,
    pub(crate) functions: &'a [CompiledFunction],
    pub(crate) meta_map: &'a HashMap<String, HashMap<String, String>>,
}

impl InstanceMintScope<'_> {
    /// The value an `Undef` cell takes when `instance_cell`, an INSTANCE-scoped
    /// id such as `P.s.body`, is a sub-instance cell of `self.graph`; `None`
    /// when it is not one. `kind` and `expr` are the cell's.
    ///
    /// It mints as `unfold` mints it at `eval()`. An instance cell's expression
    /// is its template declaration's, so it mints as the template's cell
    /// ([`mint_template_keyed_instance_cell`]) over the instance's own values
    /// ([`project_instance_reads`]). Those values cover every cell the mint
    /// reads: the expression's reads, plus, for a geometry cell, the reads of
    /// its realization's operations, which the handle's hash folds. The two
    /// sets differ when the compiler inlines a guarded geometry let into the
    /// operations. `Undef` when nothing mints.
    pub(crate) fn mint_undef_instance_cell(
        &self,
        instance_cell: &ValueCellId,
        kind: ValueCellKind,
        expr: &CompiledExpr,
    ) -> Option<Value> {
        let template = self.graph.sub_instance_template(&instance_cell.entity)?;
        let template_cell = ValueCellId::new(template, &instance_cell.member);
        if geometry_cell_realization(self.graph, &template_cell).is_none()
            && !is_symbolic_eval_wired_selector_ctor(expr)
        {
            return Some(Value::Undef);
        }
        let reads = instance_mint_reads(self.graph, &template_cell, expr);
        let instance_values =
            project_instance_reads(self.values, &instance_cell.entity, template, &reads);
        Some(mint_template_keyed_instance_cell(
            Value::Undef,
            kind,
            &template_cell,
            expr,
            &SymbolicMintScope {
                realizations: RealizationSource::Graph(self.graph),
                values: &instance_values,
                functions: self.functions,
                meta_map: self.meta_map,
            },
        ))
    }

    /// `evaluated`, unless it is `Undef` and `instance_cell` is a sub-instance
    /// cell of `self.graph`; then the cell minted by
    /// [`Self::mint_undef_instance_cell`].
    pub(crate) fn mint_instance_cell_if_undef(
        &self,
        evaluated: Value,
        instance_cell: &ValueCellId,
        kind: ValueCellKind,
        expr: &CompiledExpr,
    ) -> Value {
        match evaluated {
            Value::Undef => self
                .mint_undef_instance_cell(instance_cell, kind, expr)
                .unwrap_or(Value::Undef),
            evaluated => evaluated,
        }
    }
}

/// The template-scoped cells an instance mint of `template_cell` reads: the
/// expression's reads, plus, for a geometry cell, the reads of its
/// realization's operations, which the handle's hash folds.
fn instance_mint_reads(
    graph: &EvaluationGraph,
    template_cell: &ValueCellId,
    expr: &CompiledExpr,
) -> Vec<ValueCellId> {
    let mut reads = extract_dependency_trace(expr).reads;
    if let Some(realization) = geometry_cell_realization(graph, template_cell) {
        reads.extend(extract_realization_dependencies(&realization.operations).reads);
    }
    reads
}

/// The realization a scalar geometry cell's handle names, found as
/// [`crate::Engine::mint_symbolic_geometry_handle_for_cell_from_graph`] finds
/// it. `None` for any other cell, which only the selector mint can resolve.
fn geometry_cell_realization<'g>(
    graph: &'g EvaluationGraph,
    template_cell: &ValueCellId,
) -> Option<&'g RealizationNodeData> {
    let is_geometry = graph
        .value_cells
        .get(template_cell)
        .is_some_and(|cell| cell.cell_type == Type::Geometry);
    if !is_geometry {
        return None;
    }
    graph
        .realizations
        .values()
        .find(|realization| realization.geometry_cell.as_ref() == Some(template_cell))
}

/// The order each slot of `col_sub` must commit its cells in, so no instance
/// mint folds its hash over a sibling the slot has not yet evaluated. Cells in
/// a dependency cycle come last, in declaration order.
pub(crate) fn slot_commit_order<'c>(
    graph: &EvaluationGraph,
    col_sub: &'c CollectionSubInfo,
) -> Vec<&'c (String, ValueCellKind, Type, Option<CompiledExpr>)> {
    let traces: HashMap<NodeId, DependencyTrace> = col_sub
        .child_value_cells
        .iter()
        .map(|(member, _, _, default_expr)| {
            let template_cell = ValueCellId::new(&col_sub.structure_name, member);
            let reads = default_expr
                .as_ref()
                .map(|expr| instance_mint_reads(graph, &template_cell, expr))
                .unwrap_or_default();
            let trace = DependencyTrace {
                reads,
                ..Default::default()
            };
            (NodeId::Value(template_cell), trace)
        })
        .collect();
    let nodes: HashSet<NodeId> = traces.keys().cloned().collect();
    let mut uncommitted: HashMap<&str, _> = col_sub
        .child_value_cells
        .iter()
        .map(|cell| (cell.0.as_str(), cell))
        .collect();
    let mut order: Vec<_> = topological_sort(&nodes, &traces)
        .into_iter()
        .filter_map(|node| match node {
            NodeId::Value(cell) => uncommitted.remove(cell.member.as_str()),
            _ => None,
        })
        .collect();
    order.extend(
        col_sub
            .child_value_cells
            .iter()
            .filter(|cell| uncommitted.contains_key(cell.0.as_str())),
    );
    order
}

/// The `values` entries a template-scoped expression of `template` `reads`,
/// taken from sub instance `instance`: `T.m` from `I.m`, and a nested sub's
/// `T.x.m` from `I.x.m`, as `unfold`'s phase-2 projection keys them. A read
/// outside `template`'s scope is left out, since an instance scope holds none.
fn project_instance_reads(
    values: &ValueMap,
    instance: &str,
    template: &str,
    reads: &[ValueCellId],
) -> ValueMap {
    let mut scope = ValueMap::new();
    for read in reads {
        let Some(suffix) = read.entity.strip_prefix(template) else {
            continue;
        };
        if !(suffix.is_empty() || suffix.starts_with('.')) {
            continue;
        }
        let instance_cell = ValueCellId::new(format!("{instance}{suffix}"), &read.member);
        if let Some(value) = values.get(&instance_cell) {
            scope.insert(read.clone(), value.clone());
        }
    }
    scope
}

#[cfg(test)]
mod tests {
    use reify_compiler::ValueCellKind;
    use reify_core::ValueCellId;
    use reify_ir::{Value, ValueMap};

    use super::{SymbolicMintOrder, project_instance_reads};

    fn id(entity: &str, member: &str) -> ValueCellId {
        ValueCellId::new(entity, member)
    }

    #[test]
    fn project_instance_reads_rekeys_the_instance_and_its_nested_subs_onto_the_template() {
        let mut values = ValueMap::new();
        values.insert(id("Asm.s", "len"), Value::Int(1));
        values.insert(id("Asm.s.x", "m"), Value::Int(2));
        values.insert(id("T", "len"), Value::Int(3));
        values.insert(id("Other", "k"), Value::Int(4));
        values.insert(id("Asm.s", "k"), Value::Int(5));
        let reads = [
            id("T", "len"),
            id("T.x", "m"),
            id("T", "absent"),
            id("Other", "k"),
            id("Tx", "k"),
        ];

        let projected = project_instance_reads(&values, "Asm.s", "T", &reads);

        assert_eq!(
            projected.get(&id("T", "len")),
            Some(&Value::Int(1)),
            "T.len must come from the instance's Asm.s.len, not the template's T.len"
        );
        assert_eq!(
            projected.get(&id("T.x", "m")),
            Some(&Value::Int(2)),
            "a nested sub's T.x.m must come from Asm.s.x.m"
        );
        assert_eq!(
            projected.len(),
            2,
            "a read with no instance entry (T.absent), one outside the template \
             (Other.k), and one whose entity merely starts with the template's \
             name (Tx.k, not T or T.*) must all be left out"
        );
    }

    #[test]
    fn a_param_tries_the_geometry_handle_first_and_a_let_the_selector() {
        assert_eq!(
            SymbolicMintOrder::for_kind(ValueCellKind::Param),
            SymbolicMintOrder::GeometryHandleFirst
        );
        assert_eq!(
            SymbolicMintOrder::for_kind(ValueCellKind::Auto { free: false }),
            SymbolicMintOrder::GeometryHandleFirst
        );
        assert_eq!(
            SymbolicMintOrder::for_kind(ValueCellKind::Let),
            SymbolicMintOrder::SelectorFirst
        );
    }
}
