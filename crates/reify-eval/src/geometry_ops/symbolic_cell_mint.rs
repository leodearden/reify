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

use super::is_symbolic_eval_wired_selector_ctor;
use crate::cache::NodeId;
use crate::deps::{DependencyTrace, extract_dependency_trace, extract_realization_dependencies};
use crate::dirty::topological_sort;
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
            super::try_eval_symbolic_topology_selector(expr, scope.values, diagnostics)
        })
    };
    match order {
        SymbolicMintOrder::GeometryHandleFirst => geometry_handle().or_else(selector),
        SymbolicMintOrder::SelectorFirst => selector().or_else(geometry_handle),
    }
}

/// The graph-backed scope the edit path mints a sub-instance cell in, since
/// none of its sites has a template: the `edit_param` walk, `edit_param`'s
/// collection resize, grown-instance reseed and resolution reseed (wave2), and
/// `edit_source`'s main walk and its settle pass, collection resize and
/// resolution reseed (wave2). Each takes a cell's value from
/// [`InstanceMintScope::pass_through_or_mint`], or from
/// [`InstanceMintScope::mint_instance_cell_if_undef`], which is layered on it.
/// A caller creating an instance's cells one at a time commits them in
/// [`slot_commit_order`].
///
/// Two edit-path writers re-evaluate a sub-instance cell through
/// `Engine::reeval_cone_cell`, which does not mint:
/// - the post-walk consumer re-evals (`edit_param`'s R3e and `edit_source`'s
///   R3f, `Engine::re_eval_consumers_of_in_walk_mints_from_graph`) fill only
///   an `Undef` cell, so they never overwrite a minted one;
/// - `edit_param`'s post-wave2 guard-member reseed cone arm is the one writer
///   that can overwrite a minted sub-instance cell without minting. Cold
///   eval's guarded-cone pass runs the same function, so the two agree, and
///   minting both is #10081's (pinned by
///   `edit_param_guard_flip_leaves_instance_cells_below_a_guarded_member_as_a_cold_eval_does`).
pub(crate) struct InstanceMintScope<'a> {
    pub(crate) graph: &'a EvaluationGraph,
    pub(crate) values: &'a ValueMap,
    pub(crate) functions: &'a [CompiledFunction],
    pub(crate) meta_map: &'a HashMap<String, HashMap<String, String>>,
}

/// The outcome of [`InstanceMintScope::mint_undef_instance_cell`].
#[must_use]
#[derive(Debug, PartialEq)]
enum InstanceMint {
    /// The cell is not a sub-instance cell of the graph, so its mint is the
    /// caller's.
    NotAnInstanceCell,
    /// The value the cell takes in its instance's scope: the minted value, or
    /// `Undef` when nothing mints or the mint declines over a read the
    /// instance has not committed yet.
    InstanceCell(Value),
}

/// Which way [`InstanceMintScope::pass_through_or_mint`] decided a cell's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceMintPath {
    /// The evaluated value was not `Undef`, so it is the cell's value.
    PassedThrough,
    /// The evaluated value was `Undef` and the cell is a sub-instance cell, so
    /// the value is its instance-scope mint, `Undef` included (nothing mints,
    /// or the mint declines).
    InstanceCell,
    /// The evaluated value was `Undef` and the cell is no sub-instance cell, so
    /// the value is `Undef` and its mint is the caller's.
    NotAnInstanceCell,
}

/// A cell's value after [`InstanceMintScope::pass_through_or_mint`].
#[must_use]
#[derive(Debug, PartialEq)]
pub(crate) struct InstanceMintOutcome {
    pub(crate) value: Value,
    pub(crate) path: InstanceMintPath,
}

impl InstanceMintScope<'_> {
    /// `evaluated`, unless it is `Undef`; then the value
    /// [`Self::mint_undef_instance_cell`] gives `instance_cell`, or `Undef` when
    /// it is no sub-instance cell. The one home of the pass-through-or-mint rule
    /// every edit-path site takes; `path` says which way it went.
    pub(crate) fn pass_through_or_mint(
        &self,
        evaluated: Value,
        instance_cell: &ValueCellId,
        kind: ValueCellKind,
        expr: &CompiledExpr,
    ) -> InstanceMintOutcome {
        if !matches!(evaluated, Value::Undef) {
            return InstanceMintOutcome {
                value: evaluated,
                path: InstanceMintPath::PassedThrough,
            };
        }
        match self.mint_undef_instance_cell(instance_cell, kind, expr) {
            InstanceMint::InstanceCell(value) => InstanceMintOutcome {
                value,
                path: InstanceMintPath::InstanceCell,
            },
            InstanceMint::NotAnInstanceCell => InstanceMintOutcome {
                value: Value::Undef,
                path: InstanceMintPath::NotAnInstanceCell,
            },
        }
    }

    /// [`InstanceMint::InstanceCell`] with the value an `Undef` cell takes when
    /// `instance_cell`, an INSTANCE-scoped id such as `P.s.body`, is a
    /// sub-instance cell of `self.graph`; [`InstanceMint::NotAnInstanceCell`]
    /// when it is not one. `kind` and `expr` are the cell's.
    ///
    /// It mints as `unfold` mints it at `eval()`. An instance cell's expression
    /// is its template declaration's, so it mints as the template's cell
    /// ([`mint_template_keyed_instance_cell`]) over the instance's own values
    /// ([`project_instance_reads`]). Those values cover every cell the mint
    /// reads: the expression's reads, plus, for a geometry cell, the reads of
    /// its realization's operations, which the handle's hash folds. The two
    /// sets differ when the compiler inlines a guarded geometry let into the
    /// operations. `InstanceCell(Undef)` when nothing mints, and when the
    /// template holds a read the instance has not committed yet, which declines
    /// the mint.
    fn mint_undef_instance_cell(
        &self,
        instance_cell: &ValueCellId,
        kind: ValueCellKind,
        expr: &CompiledExpr,
    ) -> InstanceMint {
        let Some(template) = self.graph.sub_instance_template(&instance_cell.entity) else {
            return InstanceMint::NotAnInstanceCell;
        };
        let template_cell = ValueCellId::new(template, &instance_cell.member);
        let realization = self.graph.scalar_geometry_cell_realization(&template_cell);
        if realization.is_none() && !is_symbolic_eval_wired_selector_ctor(expr) {
            return InstanceMint::InstanceCell(Value::Undef);
        }
        let reads = instance_mint_reads(realization, expr);
        let Ok(instance_values) =
            project_instance_reads(self.values, &instance_cell.entity, template, &reads)
        else {
            return InstanceMint::InstanceCell(Value::Undef);
        };
        InstanceMint::InstanceCell(mint_template_keyed_instance_cell(
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

    /// The value [`Self::pass_through_or_mint`] gives a cell that must be a
    /// sub-instance cell of `self.graph`, as every caller's slot cell is
    /// stamped through `EvaluationGraph::insert_sub_instance_cell`. A debug
    /// build panics on an `Undef` one that is not; a release build leaves it
    /// `Undef`.
    pub(crate) fn mint_instance_cell_if_undef(
        &self,
        evaluated: Value,
        instance_cell: &ValueCellId,
        kind: ValueCellKind,
        expr: &CompiledExpr,
    ) -> Value {
        let InstanceMintOutcome { value, path } =
            self.pass_through_or_mint(evaluated, instance_cell, kind, expr);
        match path {
            InstanceMintPath::PassedThrough | InstanceMintPath::InstanceCell => {}
            InstanceMintPath::NotAnInstanceCell => debug_assert!(
                false,
                "{instance_cell} is not a sub-instance cell: a resized or reseeded \
                 slot cell must be stamped through \
                 EvaluationGraph::insert_sub_instance_cell"
            ),
        }
        value
    }
}

/// The template-scoped cells an instance mint of a cell with expression `expr`
/// reads: the expression's reads, plus, for a geometry cell, the reads of the
/// operations of `realization`, the one its geometry mint names, which the
/// handle's hash folds.
fn instance_mint_reads(
    realization: Option<&RealizationNodeData>,
    expr: &CompiledExpr,
) -> Vec<ValueCellId> {
    let mut reads = extract_dependency_trace(expr).reads;
    if let Some(realization) = realization {
        reads.extend(extract_realization_dependencies(&realization.operations).reads);
    }
    reads
}

/// The order each slot of `col_sub` must commit its cells in, so no instance
/// mint folds its hash over a sibling the slot has not yet evaluated. Cells in
/// a dependency cycle, and cells stranded downstream of one, come last, in
/// declaration order.
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
                .map(|expr| {
                    let realization = graph.scalar_geometry_cell_realization(&template_cell);
                    instance_mint_reads(realization, expr)
                })
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
/// `Err(I.m)` when `values` holds the template's `T.m` but not the instance's
/// `I.m`; a read absent at both scopes is left out, as template scope folds it
/// as `Undef` too.
fn project_instance_reads(
    values: &ValueMap,
    instance: &str,
    template: &str,
    reads: &[ValueCellId],
) -> Result<ValueMap, ValueCellId> {
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
        } else if values.get(read).is_some() {
            return Err(instance_cell);
        }
    }
    Ok(scope)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reify_compiler::ValueCellKind;
    use reify_core::{Type, ValueCellId};
    use reify_ir::{CompiledExpr, Value, ValueMap};
    use reify_test_support::parse_and_compile;

    use super::{
        InstanceMintOutcome, InstanceMintPath, InstanceMintScope, SymbolicMintOrder,
        instance_mint_reads, project_instance_reads, slot_commit_order,
    };
    use crate::deps::extract_dependency_trace;
    use crate::geometry_ops::try_eval_symbolic_topology_selector;
    use crate::graph::geometry_cell_link_fixture::{
        GEOMETRY_LIST_MEMBERS, SCALAR_GEOMETRY_MEMBERS, SRC, TEMPLATE,
        assert_some_realization_links,
    };
    use crate::graph::{CollectionSubInfo, EvaluationGraph, ValueCellNode};

    fn id(entity: &str, member: &str) -> ValueCellId {
        ValueCellId::new(entity, member)
    }

    type SlotCell = (String, ValueCellKind, Type, Option<CompiledExpr>);

    /// The structure the dependency-order tests stamp their hand-built slots
    /// from, and whose siblings the cells of those slots read.
    const ELEMENT: &str = "E";

    /// A slot of `Rack.items`, stamped from `structure_name`, declaring `cells`
    /// in order. Every read of a cell must name `structure_name`: a read outside
    /// it is never counted among the slot's cells, which would leave the order
    /// to the scheduling core's tie-break, and a test of dependency order
    /// passing or failing for the wrong reason.
    fn slot(structure_name: &str, cells: Vec<SlotCell>) -> CollectionSubInfo {
        for (member, .., default_expr) in &cells {
            let Some(expr) = default_expr else { continue };
            for read in extract_dependency_trace(expr).reads {
                assert_eq!(
                    read.entity, structure_name,
                    "precondition: `{member}` reads `{read}`, which is no cell of \
                     `{structure_name}`"
                );
            }
        }
        CollectionSubInfo {
            parent_entity: "Rack".to_string(),
            sub_name: "items".to_string(),
            structure_name: structure_name.to_string(),
            count_cell: id("Rack", "__count_items"),
            child_value_cells: cells,
        }
    }

    /// The let `member` of `ELEMENT`, whose expression reads its sibling `read`.
    fn let_reading(member: &str, read: &str) -> SlotCell {
        (
            member.to_string(),
            ValueCellKind::Let,
            Type::Int,
            Some(CompiledExpr::value_ref(id(ELEMENT, read), Type::Int)),
        )
    }

    /// The param `member` of `ELEMENT`, whose default reads nothing.
    fn param_with_literal_default(member: &str) -> SlotCell {
        (
            member.to_string(),
            ValueCellKind::Param,
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(1), Type::Int)),
        )
    }

    /// The members of `col_sub` in the order a slot commits them.
    fn commit_order<'c>(graph: &EvaluationGraph, col_sub: &'c CollectionSubInfo) -> Vec<&'c str> {
        slot_commit_order(graph, col_sub)
            .into_iter()
            .map(|(member, ..)| member.as_str())
            .collect()
    }

    /// The evaluation graph of `src`, which must compile.
    fn graph_of(src: &str) -> EvaluationGraph {
        EvaluationGraph::from_templates(&parse_and_compile(src).templates)
    }

    /// The graph of the shared geometry-cell fixture: template `T`, and `P`
    /// stamping it as the sub instance `P.s`.
    fn fixture_graph() -> EvaluationGraph {
        graph_of(SRC)
    }

    fn node_of<'g>(graph: &'g EvaluationGraph, cell: &ValueCellId) -> &'g ValueCellNode {
        graph
            .value_cells
            .get(cell)
            .unwrap_or_else(|| panic!("precondition: `{cell}` must be stamped"))
    }

    fn expr_of<'g>(graph: &'g EvaluationGraph, cell: &ValueCellId) -> &'g CompiledExpr {
        node_of(graph, cell)
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("precondition: `{cell}` must have an expr"))
    }

    /// The fixture's `T.member`, declared as a slot declares its cells.
    fn slot_cell_of(graph: &EvaluationGraph, member: &str) -> SlotCell {
        let node = node_of(graph, &id(TEMPLATE, member));
        (
            member.to_string(),
            node.kind,
            node.cell_type.clone(),
            node.default_expr.clone(),
        )
    }

    /// The params of the fixture's template `T` (10 mm) and of its sub instance
    /// `P.s` (20 mm).
    fn fixture_param_values() -> ValueMap {
        let mut values = ValueMap::new();
        for (entity, w) in [(TEMPLATE, 0.01), ("P.s", 0.02)] {
            values.insert(id(entity, "w"), Value::length(w));
            values.insert(id(entity, "on"), Value::Bool(true));
        }
        values
    }

    /// The cells `values` holds for the fixture's sub instance `P.s`, keyed as
    /// the template `T`'s: what the graph mint of `T.m` must be given to answer
    /// as the instance mint of `P.s.m` does.
    fn instance_keyed_as_template(values: &ValueMap) -> ValueMap {
        let mut keyed_as_template = ValueMap::new();
        for (cell, value) in values.iter().filter(|(cell, _)| cell.entity == "P.s") {
            keyed_as_template.insert(id(TEMPLATE, &cell.member), value.clone());
        }
        keyed_as_template
    }

    /// What the graph mint answers at the template cell `cell` over `values`,
    /// which is keyed in the template's scope.
    fn graph_mint(graph: &EvaluationGraph, cell: &ValueCellId, values: &ValueMap) -> Option<Value> {
        crate::Engine::mint_symbolic_geometry_handle_for_cell_from_graph(
            cell,
            graph,
            values,
            &[],
            &HashMap::new(),
        )
    }

    /// The instance arm's outcome at `cell` over `values` for the `evaluated`
    /// value, with the kind and expression of `like`. Two calls whose outcomes
    /// are compared fix `like` and vary `cell`, so the difference is the cell's
    /// and not the expression's.
    fn outcome_at(
        graph: &EvaluationGraph,
        values: &ValueMap,
        like: &ValueCellId,
        cell: &ValueCellId,
        evaluated: Value,
    ) -> InstanceMintOutcome {
        InstanceMintScope {
            graph,
            values,
            functions: &[],
            meta_map: &HashMap::new(),
        }
        .pass_through_or_mint(
            evaluated,
            cell,
            node_of(graph, like).kind,
            expr_of(graph, like),
        )
    }

    /// The outcome of an instance cell whose value in its instance's scope is
    /// `value`.
    fn instance_cell_outcome(value: Value) -> InstanceMintOutcome {
        InstanceMintOutcome {
            value,
            path: InstanceMintPath::InstanceCell,
        }
    }

    /// A selector let over a scalar geometry let, which the shared fixture
    /// lacks: `Block.side` is a `face` of `Block.body`, and `Shelf` stamps
    /// `Block` as the sub instance `Shelf.block`.
    const SELECTOR_SRC: &str = r#"structure Block {
    param w : Length = 10mm
    let body = box(w, 10mm, 10mm)
    let side = face(body, "x_max")
}
structure Shelf {
    sub block = Block()
}"#;

    /// The graph of [`SELECTOR_SRC`], and the `body`, which `side` reads, that
    /// `Block` (10 mm wide) and `Shelf.block` (20 mm wide) each commit.
    fn selector_fixture() -> (EvaluationGraph, Value, Value) {
        let graph = graph_of(SELECTOR_SRC);
        let body_of_width = |w: f64| {
            let mut params = ValueMap::new();
            params.insert(id("Block", "w"), Value::length(w));
            graph_mint(&graph, &id("Block", "body"), &params)
                .unwrap_or_else(|| panic!("precondition: `Block.body` must mint at {w} m"))
        };
        let (template_body, instance_body) = (body_of_width(0.01), body_of_width(0.02));
        assert_ne!(
            template_body, instance_body,
            "precondition: the two widths must mint unequal bodies, or a selector over \
             the instance's body is indistinguishable from one over the template's"
        );
        (graph, template_body, instance_body)
    }

    /// The selector `Block.side`'s expression mints over `body`, as template
    /// scope mints it.
    fn side_selector_over(graph: &EvaluationGraph, body: &Value) -> Option<Value> {
        let mut scope = ValueMap::new();
        scope.insert(id("Block", "body"), body.clone());
        try_eval_symbolic_topology_selector(
            expr_of(graph, &id("Block", "side")),
            &scope,
            &mut Vec::new(),
        )
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

        let projected = project_instance_reads(&values, "Asm.s", "T", &reads)
            .expect("every in-scope read the template holds has an instance entry");

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
    fn project_instance_reads_reports_a_read_the_template_holds_but_the_instance_has_not_evaluated()
    {
        let mut values = ValueMap::new();
        values.insert(id("T", "len"), Value::Int(3));
        values.insert(id("T", "w"), Value::Int(4));
        values.insert(id("Asm.s", "w"), Value::Int(5));

        let projected =
            project_instance_reads(&values, "Asm.s", "T", &[id("T", "w"), id("T", "len")]);

        assert_eq!(
            projected.err(),
            Some(id("Asm.s", "len")),
            "a read the template holds (T.len) but the instance has not evaluated \
             (no Asm.s.len) must be reported, not folded as Undef"
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

    /// An instance cell mints as its template's cell (task #9846). For each
    /// geometry let `m` of the fixture's `T`, the instance arm minting `P.s.m`
    /// must answer what the graph mint answers for `T.m` over the instance's
    /// values keyed at template scope. That pins both halves of the pairing: the
    /// early-out (a list cell mints `Undef`), and the read projection (`b`'s hash
    /// folds `P.s.w`, which only `b`'s realization reads, not its expression).
    ///
    /// Drives `InstanceMintScope` directly, as
    /// `engine_build/geometry_cell_mint_from_graph_tests.rs` drives the graph
    /// mint: the edit path never reaches it with a list cell, since a list
    /// reevaluates to `List`, not `Undef`, so a public-seam test would pass
    /// whether or not the two mints agree on list cells.
    #[test]
    fn the_instance_mint_of_a_sub_cell_pairs_with_the_graph_mint_of_its_template_cell() {
        let graph = fixture_graph();
        let values = fixture_param_values();
        let instance_values_at_template_scope = instance_keyed_as_template(&values);
        assert_eq!(graph.sub_instance_template("P.s"), Some(TEMPLATE));
        for member in SCALAR_GEOMETRY_MEMBERS {
            assert!(
                matches!(
                    graph_mint(&graph, &id(TEMPLATE, member), &values),
                    Some(Value::GeometryHandle {
                        kernel_handle: None,
                        ..
                    })
                ),
                "control: the graph mint must answer the scalar geometry let `T.{member}`"
            );
        }
        for member in GEOMETRY_LIST_MEMBERS {
            assert_some_realization_links(&graph, &id(TEMPLATE, member));
        }
        let b_expr = expr_of(&graph, &id("P.s", "b"));
        assert!(
            !extract_dependency_trace(b_expr)
                .reads
                .contains(&id(TEMPLATE, "w")),
            "precondition: `b`'s expression must not read `w`, so only the reads of \
             `b`'s realization bring `w` into the instance's projection"
        );
        assert_ne!(
            graph_mint(&graph, &id(TEMPLATE, "b"), &values),
            graph_mint(
                &graph,
                &id(TEMPLATE, "b"),
                &instance_values_at_template_scope
            ),
            "control: `T.b`'s handle must fold `w`, or pairing `P.s.b` is vacuous \
             on the read projection"
        );

        for member in SCALAR_GEOMETRY_MEMBERS
            .into_iter()
            .chain(GEOMETRY_LIST_MEMBERS)
        {
            let instance_cell = id("P.s", member);

            assert_eq!(
                outcome_at(
                    &graph,
                    &values,
                    &instance_cell,
                    &instance_cell,
                    Value::Undef
                ),
                instance_cell_outcome(
                    graph_mint(
                        &graph,
                        &id(TEMPLATE, member),
                        &instance_values_at_template_scope
                    )
                    .unwrap_or(Value::Undef)
                ),
                "the instance arm must mint `{instance_cell}` as the graph mint mints \
                 `T.{member}` over the instance's values: an early-out to Undef where \
                 the graph mint answers is the drift task #9846 retires, and a \
                 projection missing a read of the realization the one #8102 fixed"
            );
        }
    }

    /// The names are chosen so declaration order (`s`, `body`, `len`) and the
    /// scheduling core's tie-break order (`body`, `len`, `s`) both differ from
    /// the commit order: only the dependency order is `len`, `body`, `s`.
    #[test]
    fn a_slot_commits_a_cell_after_the_sibling_it_reads_whatever_its_declaration_order() {
        let col_sub = slot(
            ELEMENT,
            vec![
                let_reading("s", "body"),
                let_reading("body", "len"),
                param_with_literal_default("len"),
            ],
        );

        assert_eq!(
            commit_order(&EvaluationGraph::default(), &col_sub),
            ["len", "body", "s"],
            "each cell must commit after the sibling it reads"
        );
    }

    /// `topological_sort` drops the members of a dependency cycle (its
    /// cycle-drop contract), and with them each cell stranded behind one, which
    /// reads a cycle member and never reaches in-degree 0 either, so
    /// `slot_commit_order` appends them all itself, in declaration order. `y` is
    /// declared before `x` although the core's tie-break would order `x` first,
    /// so only the append yields `y`, `x`. `z` reads the cycle member `x`, so it
    /// is stranded with the cycle, and commits ahead of the `x` it reads because
    /// its declaration is first.
    #[test]
    fn a_slot_commits_its_cycle_and_the_cells_stranded_behind_it_last_in_declaration_order() {
        let col_sub = slot(
            ELEMENT,
            vec![
                let_reading("z", "x"),
                let_reading("y", "x"),
                let_reading("x", "y"),
                param_with_literal_default("len"),
            ],
        );

        assert_eq!(
            commit_order(&EvaluationGraph::default(), &col_sub),
            ["len", "z", "y", "x"],
            "the cycle `y` <-> `x`, and `z` stranded behind it, must commit after \
             `len`, in declaration order"
        );
    }

    /// The compiler inlines the guarded `core` into `b`'s realization, so the
    /// operations of `b`'s realization read `w` while `b`'s expression, which
    /// reads `core`, does not. The handle's hash folds `w`.
    #[test]
    fn instance_mint_reads_adds_the_reads_of_the_realization_a_guarded_let_is_inlined_into() {
        let graph = fixture_graph();
        let (b, w) = (id(TEMPLATE, "b"), id(TEMPLATE, "w"));
        let b_expr = expr_of(&graph, &b);
        let expression_reads = extract_dependency_trace(b_expr).reads;
        assert!(
            !expression_reads.contains(&w),
            "precondition: `b`'s expression must not read `w`, so only the reads of \
             `b`'s realization can bring `w` in"
        );

        assert_eq!(
            instance_mint_reads(None, b_expr),
            expression_reads,
            "with no realization, a cell's reads are its expression's alone"
        );

        let realization = graph.scalar_geometry_cell_realization(&b);
        assert!(
            realization.is_some(),
            "precondition: `T.b` must resolve to its realization"
        );
        let reads = instance_mint_reads(realization, b_expr);
        assert!(
            reads.contains(&w),
            "`b`'s realization reads `w` through the inlined `core`, so the instance \
             mint must read it too: {reads:?}"
        );
        for read in &expression_reads {
            assert!(
                reads.contains(read),
                "the realization's reads must extend the expression's, which lost \
                 `{read}`: {reads:?}"
            );
        }
    }

    /// `b` is declared first and sorts first by name, and its expression's read
    /// of `core` is outside the slot, so the slot orders `w` first only through
    /// the read of `b`'s realization. The control is the same slot over a graph
    /// with no realization, where nothing reads `w`.
    #[test]
    fn a_slot_commits_a_geometry_cell_after_a_sibling_only_its_realization_reads() {
        let graph = fixture_graph();
        let col_sub = slot(
            TEMPLATE,
            vec![slot_cell_of(&graph, "b"), slot_cell_of(&graph, "w")],
        );

        assert_eq!(
            commit_order(&EvaluationGraph::default(), &col_sub),
            ["b", "w"],
            "control: with no realization, `b` reads no sibling of its slot"
        );
        assert_eq!(
            commit_order(&graph, &col_sub),
            ["w", "b"],
            "`b`'s realization reads `w`, so `w` must commit first or `b`'s handle \
             folds a `w` the slot has not evaluated"
        );
    }

    /// The edit path asks the instance arm about every `Undef` cell, and takes
    /// its own template-scope arm only for a cell that is not a sub-instance
    /// cell. A decline is a separate outcome: an instance cell whose value is
    /// `Undef`.
    #[test]
    fn a_template_cell_is_not_an_instance_cell() {
        let graph = fixture_graph();
        let values = fixture_param_values();
        let instance_b = id("P.s", "b");

        assert_eq!(
            outcome_at(
                &graph,
                &values,
                &instance_b,
                &id(TEMPLATE, "b"),
                Value::Undef
            ),
            InstanceMintOutcome {
                value: Value::Undef,
                path: InstanceMintPath::NotAnInstanceCell,
            },
            "`T` is a template, not a sub instance: `T.b` is no sub-instance cell, \
             so the instance arm must leave its mint to the caller's template-scope \
             arm"
        );
        let minted = outcome_at(&graph, &values, &instance_b, &instance_b, Value::Undef);
        assert!(
            matches!(
                minted,
                InstanceMintOutcome {
                    value: Value::GeometryHandle { .. },
                    path: InstanceMintPath::InstanceCell,
                }
            ),
            "control: the same kind and expression at the sub instance's `P.s.b` \
             must mint, so NotAnInstanceCell comes from the cell's entity and not \
             its expression: {minted:?}"
        );
    }

    /// A defined value is the cell's value whatever the cell: the instance arm
    /// mints only an `Undef` cell, and asks whether the cell is a sub-instance
    /// cell only then.
    #[test]
    fn a_defined_value_passes_through_the_instance_mint_unchanged() {
        let graph = fixture_graph();
        let values = fixture_param_values();
        let instance_b = id("P.s", "b");
        let passed_through = InstanceMintOutcome {
            value: Value::Int(7),
            path: InstanceMintPath::PassedThrough,
        };

        assert_eq!(
            outcome_at(&graph, &values, &instance_b, &instance_b, Value::Int(7)),
            passed_through,
            "a defined value at the sub-instance cell `P.s.b` must pass through \
             unminted"
        );
        assert_eq!(
            outcome_at(
                &graph,
                &values,
                &instance_b,
                &id(TEMPLATE, "b"),
                Value::Int(7)
            ),
            passed_through,
            "a defined value at the template cell `T.b` must pass through, not be \
             reported NotAnInstanceCell: the caller's template-scope arm mints only \
             an Undef cell"
        );
        let minted = outcome_at(&graph, &values, &instance_b, &instance_b, Value::Undef);
        assert!(
            matches!(
                minted,
                InstanceMintOutcome {
                    value: Value::GeometryHandle { .. },
                    path: InstanceMintPath::InstanceCell,
                }
            ),
            "control: an Undef `P.s.b` must mint, so PassedThrough comes from the \
             value and not the cell: {minted:?}"
        );
    }

    /// Every caller of `mint_instance_cell_if_undef` passes a slot cell it
    /// stamped through `EvaluationGraph::insert_sub_instance_cell`, so a cell
    /// that is not a sub-instance cell breaches the sub-instance registry. A
    /// debug build reports the breach rather than leaving the cell `Undef`.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "T.b is not a sub-instance cell")]
    fn the_if_undef_mint_of_a_template_cell_panics_in_a_debug_build() {
        let graph = fixture_graph();
        let values = fixture_param_values();
        let instance_b = id("P.s", "b");

        InstanceMintScope {
            graph: &graph,
            values: &values,
            functions: &[],
            meta_map: &HashMap::new(),
        }
        .mint_instance_cell_if_undef(
            Value::Undef,
            &id(TEMPLATE, "b"),
            node_of(&graph, &instance_b).kind,
            expr_of(&graph, &instance_b),
        );
    }

    /// The sub-instance check guards only the mint: a defined value passes
    /// through first, so a defined template cell is no breach.
    #[test]
    fn the_if_undef_mint_passes_a_defined_value_through_at_a_template_cell() {
        let graph = fixture_graph();
        let values = fixture_param_values();
        let instance_b = id("P.s", "b");

        let value = InstanceMintScope {
            graph: &graph,
            values: &values,
            functions: &[],
            meta_map: &HashMap::new(),
        }
        .mint_instance_cell_if_undef(
            Value::Int(7),
            &id(TEMPLATE, "b"),
            node_of(&graph, &instance_b).kind,
            expr_of(&graph, &instance_b),
        );

        assert_eq!(
            value,
            Value::Int(7),
            "a defined value must pass through before the sub-instance check"
        );
    }

    /// `b`'s expression does not read `w`, only `b`'s realization does, so the
    /// decline covers a realization's reads too.
    #[test]
    fn the_instance_mint_declines_to_undef_while_the_instance_lacks_a_read_its_template_holds() {
        let graph = fixture_graph();
        let with_instance_w = fixture_param_values();
        let mut without_instance_w = with_instance_w.clone();
        without_instance_w.remove(&id("P.s", "w"));
        let instance_b = id("P.s", "b");

        assert_eq!(
            outcome_at(
                &graph,
                &without_instance_w,
                &instance_b,
                &instance_b,
                Value::Undef
            ),
            instance_cell_outcome(Value::Undef),
            "`T.w` is held but `P.s.w` is not committed yet: minting `P.s.b` over \
             the instance's values would fold a `w` it has not evaluated, so the \
             instance arm must decline to Undef. A decline is still an instance \
             cell: read as NotAnInstanceCell, it would send the edit_param walk \
             into its template-scope mint over the full value map"
        );
        let minted = outcome_at(
            &graph,
            &with_instance_w,
            &instance_b,
            &instance_b,
            Value::Undef,
        );
        assert!(
            matches!(
                minted,
                InstanceMintOutcome {
                    value: Value::GeometryHandle {
                        kernel_handle: None,
                        ..
                    },
                    path: InstanceMintPath::InstanceCell,
                }
            ),
            "control: once `P.s.w` is committed `P.s.b` must mint, so the decline \
             comes from the missing read: {minted:?}"
        );
    }

    /// A selector let has no realization, so the instance arm lets it past its
    /// early-out only as a symbolic-eval-wired selector constructor. It then
    /// selects over the instance's own `body`, which its expression reads, as
    /// template scope mints `Block.side` over that `body`.
    #[test]
    fn the_instance_mint_of_a_selector_let_mints_it_over_the_instance_geometry() {
        let (graph, template_body, instance_body) = selector_fixture();
        let side = id("Shelf.block", "side");
        let mut values = ValueMap::new();
        values.insert(id("Block", "body"), template_body.clone());
        values.insert(id("Shelf.block", "body"), instance_body.clone());

        let minted = outcome_at(&graph, &values, &side, &side, Value::Undef);

        assert!(
            matches!(
                minted,
                InstanceMintOutcome {
                    value: Value::Selector(_),
                    path: InstanceMintPath::InstanceCell,
                }
            ),
            "a selector constructor must mint through the instance arm, not \
             early-out to Undef for want of a realization: {minted:?}"
        );
        let selector_over = |body| {
            side_selector_over(&graph, body)
                .map(instance_cell_outcome)
                .unwrap_or_else(|| panic!("precondition: `Block.side` must select over {body:?}"))
        };
        assert_eq!(
            minted,
            selector_over(&instance_body),
            "`Shelf.block.side` must select over the instance's `body`"
        );
        assert_ne!(
            minted,
            selector_over(&template_body),
            "control: a selector over the template's `body` must differ, or the \
             instance's own `body` is not pinned"
        );
    }

    /// `side`'s expression reads `body`: the template holds its `body` while the
    /// instance has not committed one yet, so a mint would select over a `body`
    /// the instance has not evaluated.
    #[test]
    fn the_instance_mint_declines_a_selector_while_the_geometry_it_reads_is_uncommitted() {
        let (graph, template_body, instance_body) = selector_fixture();
        let side = id("Shelf.block", "side");
        let mut values = ValueMap::new();
        values.insert(id("Block", "body"), template_body);

        assert_eq!(
            outcome_at(&graph, &values, &side, &side, Value::Undef),
            instance_cell_outcome(Value::Undef),
            "`Block.body` is held but `Shelf.block.body` is not committed yet: \
             minting `Shelf.block.side` would select over a `body` the instance \
             has not evaluated, so the instance arm must decline to Undef"
        );
        values.insert(id("Shelf.block", "body"), instance_body);
        let minted = outcome_at(&graph, &values, &side, &side, Value::Undef);
        assert!(
            matches!(
                minted,
                InstanceMintOutcome {
                    value: Value::Selector(_),
                    path: InstanceMintPath::InstanceCell,
                }
            ),
            "control: once `Shelf.block.body` is committed `Shelf.block.side` must \
             mint, so the decline comes from the missing read: {minted:?}"
        );
    }
}
