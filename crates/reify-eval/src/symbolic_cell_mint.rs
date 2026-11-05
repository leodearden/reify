//! Kernel-free symbolic mint for a value cell whose plain evaluation returned
//! `Undef` (R3d, task #4900): a geometry cell gets a symbolic
//! `Value::GeometryHandle`, a selector-ctor cell a `Value::Selector`.
//!
//! The one copy shared by template scope (`engine_eval.rs`) and sub-instance
//! scope (`unfold.rs` at `eval()`, `engine_edit.rs`'s `edit_param` walk, task
//! #8102), so the scopes mint identical values for identical inputs.

use std::collections::HashMap;

use reify_compiler::{TopologyTemplate, ValueCellKind};
use reify_core::{Diagnostic, ValueCellId};
use reify_ir::{CompiledExpr, CompiledFunction, Value, ValueMap};

use crate::graph::EvaluationGraph;

/// Which mint a cell tries first. Param sites try the geometry handle first,
/// Let sites the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolicMintOrder {
    GeometryHandleFirst,
    SelectorFirst,
}

impl SymbolicMintOrder {
    /// The order a cell of `kind` takes: a param (an `auto` one included) tries
    /// the geometry handle first, a let the selector.
    pub(crate) fn for_kind(kind: ValueCellKind) -> Self {
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
    /// `edit_param` walk, which has no template. This is the hook #8209 folds
    /// that walk's template-scope mint onto.
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

/// `evaluated` unchanged unless it is `Undef`; then `cell_id` (a cell of the
/// template `scope.realizations` belongs to) is minted symbolically, trying the two mints in `order`
/// (the first `Some` wins). A selector kind-closure violation leaves the cell
/// `Undef`, with its diagnostic pushed to `diagnostics`.
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
    order: SymbolicMintOrder,
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
            crate::geometry_ops::try_eval_symbolic_topology_selector(
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

/// The `values` entries a template-scoped expression of `template` `reads`,
/// taken from sub instance `instance`: `T.m` from `I.m`, and a nested sub's
/// `T.x.m` from `I.x.m`, as `unfold`'s phase-2 projection keys them. A read
/// outside `template`'s scope is left out, since an instance scope holds none.
///
/// This is the instance scope the `edit_param` walk mints in. An instance cell's
/// expression is its template declaration's, so evaluated against the global
/// map it would read the TEMPLATE's cells.
pub(crate) fn project_instance_reads(
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
