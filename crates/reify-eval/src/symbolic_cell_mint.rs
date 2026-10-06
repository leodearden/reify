//! Kernel-free symbolic mint for a value cell whose plain evaluation returned
//! `Undef` (R3d, task #4900): a geometry cell gets a symbolic
//! `Value::GeometryHandle`, a selector-ctor cell a `Value::Selector`.
//!
//! The one copy shared by template scope (`engine_eval.rs`) and sub-instance
//! scope (`unfold.rs`, task #8102), so the two scopes mint identical values for
//! identical inputs.

use std::collections::HashMap;

use reify_compiler::TopologyTemplate;
use reify_core::{Diagnostic, ValueCellId};
use reify_ir::{CompiledExpr, CompiledFunction, Value, ValueMap};

/// Which mint a cell tries first. Param sites try the geometry handle first,
/// Let sites the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolicMintOrder {
    GeometryHandleFirst,
    SelectorFirst,
}

/// The scope a cell is minted in: the template the cell was COMPILED in, and
/// the map it was evaluated against (keyed in that template's scope).
pub(crate) struct SymbolicMintScope<'a> {
    pub(crate) template: &'a TopologyTemplate,
    pub(crate) values: &'a ValueMap,
    pub(crate) functions: &'a [CompiledFunction],
    pub(crate) meta_map: &'a HashMap<String, HashMap<String, String>>,
}

/// Mint `cell_id` (a cell of `scope.template`) symbolically, trying the two
/// mints in `order`; the first `Some` wins. Call only for a cell whose plain
/// evaluation returned `Undef`. A selector kind-closure violation returns
/// `Some(Value::Undef)` verbatim, with its diagnostic pushed to `diagnostics`.
///
/// Realization identity: a minted handle names `scope.template`'s
/// `RealizationDecl` id at EVERY scope and folds its `upstream_values_hash` over
/// `scope.values`. An override-free sub instance therefore mints a handle equal
/// to the template's, and an override reaching the realization's inputs mints
/// an unequal one. The build path agrees: `seed_cross_sub_named_steps` gives a
/// no-args sub the template's kernel handles and re-realizes only under
/// overrides.
pub(crate) fn mint_symbolic_cell_value(
    order: SymbolicMintOrder,
    cell_id: &ValueCellId,
    default_expr: Option<&CompiledExpr>,
    scope: &SymbolicMintScope<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    let geometry_handle = || {
        crate::Engine::mint_symbolic_geometry_handle_for_cell(
            cell_id,
            &scope.template.realizations,
            scope.values,
            scope.functions,
            scope.meta_map,
        )
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
