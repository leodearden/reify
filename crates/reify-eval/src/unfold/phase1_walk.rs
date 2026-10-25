//! Phase 1's walk order: a child instance's params, plus the lets their
//! defaults read, in one dependency order.

use std::collections::{HashMap, HashSet};

use reify_compiler::{TopologyTemplate, ValueCellDecl, ValueCellKind};
use reify_core::ValueCellId;
use reify_ir::CompiledExpr;

use crate::cache::NodeId;
use crate::deps::{DependencyTrace, extract_dependency_trace};
use crate::dirty::topological_sort;

/// One cell phase 1 visits.
#[derive(Clone, Copy)]
pub(super) enum Phase1Step<'t> {
    /// Evaluated and committed by phase 1.
    Param(&'t ValueCellDecl),
    /// A let some param default reads. Phase 1 evaluates it into its own scope
    /// only; phase 2 commits it.
    ScratchLet {
        key: &'t ValueCellId,
        expr: &'t CompiledExpr,
    },
}

impl<'t> Phase1Step<'t> {
    fn id(self) -> &'t ValueCellId {
        match self {
            Phase1Step::Param(cell) => &cell.id,
            Phase1Step::ScratchLet { key, .. } => key,
        }
    }
}

/// Every Param of `child_template`, plus the Lets their defaults transitively
/// read, ordered so each cell follows everything it reads.
///
/// - The node rule is template scope's (`build_combined_param_let_graph` in
///   engine_eval.rs): all Params, plus Lets with a default; Auto cells never.
///   A let no param default reads is left out. The two copies are held together
///   by `every_kind_the_node_rule_distinguishes_agrees_with_template`.
/// - A param named by `args` contributes no edge and pulls in no let: its
///   `default_expr` is never evaluated, so its reads would be invented.
/// - [`topological_sort`] omits cycle members and everything downstream of
///   them. Those cells are appended in declaration order, so every param is
///   still committed. The walk reports nothing: template scope already reports
///   the cycle once.
///
/// Pinned by tests/harness_engine/instance_scope_param_default_order.rs.
pub(super) fn phase1_walk<'t>(
    child_template: &'t TopologyTemplate,
    args: &[(String, CompiledExpr)],
) -> Vec<Phase1Step<'t>> {
    let cells: Vec<Phase1Step<'t>> = child_template
        .value_cells
        .iter()
        .filter_map(|cell| match (&cell.kind, &cell.default_expr) {
            (ValueCellKind::Param, _) => Some(Phase1Step::Param(cell)),
            (ValueCellKind::Let, Some(expr)) => Some(Phase1Step::ScratchLet {
                key: &cell.id,
                expr,
            }),
            _ => None,
        })
        .collect();

    // This runs for EVERY instance, collection elements included, so the cheap
    // exits come first: fewer than two cells admit no edge and need no
    // expression walk, and no param reading a sibling means declaration order
    // already is a dependency order. Only the remaining path pays for
    // `topological_sort`.
    if cells.len() < 2 {
        return params_only(cells);
    }

    // `Some` marks a walked cell. Params are always walked; a let is walked
    // once the closure below reaches it.
    let mut traces: Vec<Option<DependencyTrace>> = cells
        .iter()
        .map(|step| match step {
            Phase1Step::Param(cell) => Some(param_trace(cell, args)),
            Phase1Step::ScratchLet { .. } => None,
        })
        .collect();

    let index_of: HashMap<&ValueCellId, usize> = cells
        .iter()
        .enumerate()
        .map(|(i, step)| (step.id(), i))
        .collect();
    // Two cells of one template cannot share a member name. In release a
    // collision costs ordering only: placement below is keyed on the
    // declaration index, so a shadowed param is still walked and committed.
    debug_assert_eq!(
        index_of.len(),
        cells.len(),
        "phase1_walk: duplicate param/let member name in template {}",
        child_template.name,
    );

    let mut worklist: Vec<usize> = traces
        .iter()
        .flatten()
        .flat_map(|trace| &trace.reads)
        .filter_map(|read| index_of.get(read).copied())
        .collect();
    if worklist.is_empty() {
        return params_only(cells);
    }
    while let Some(i) = worklist.pop() {
        if traces[i].is_some() {
            continue;
        }
        if let Phase1Step::ScratchLet { expr, .. } = cells[i] {
            let trace = extract_dependency_trace(expr);
            worklist.extend(
                trace
                    .reads
                    .iter()
                    .filter_map(|read| index_of.get(read).copied()),
            );
            traces[i] = Some(trace);
        }
    }

    let (walked, walked_traces): (Vec<usize>, Vec<DependencyTrace>) = traces
        .into_iter()
        .enumerate()
        .filter_map(|(i, trace)| trace.map(|trace| (i, trace)))
        .unzip();
    let nodes: Vec<NodeId> = walked
        .iter()
        .map(|i| NodeId::Value(cells[*i].id().clone()))
        .collect();
    let node_ids: HashSet<NodeId> = nodes.iter().cloned().collect();
    let node_traces: HashMap<NodeId, DependencyTrace> =
        nodes.iter().cloned().zip(walked_traces).collect();

    let sorted = topological_sort(&node_ids, &node_traces);
    let mut placed = vec![false; walked.len()];
    let mut ordered: Vec<Phase1Step<'t>> = Vec::with_capacity(walked.len());
    for nid in &sorted {
        // First not-yet-placed cell carrying this node id, so one sorted node
        // consumes one declaration slot.
        let next = (0..walked.len()).find(|w| !placed[*w] && &nodes[*w] == nid);
        if let Some(w) = next {
            placed[w] = true;
            ordered.push(cells[walked[w]]);
        }
    }
    ordered.extend(
        (0..walked.len())
            .filter(|w| !placed[*w])
            .map(|w| cells[walked[w]]),
    );
    ordered
}

/// An arg-supplied param reads nothing: the arg, not its default, is evaluated.
fn param_trace(cell: &ValueCellDecl, args: &[(String, CompiledExpr)]) -> DependencyTrace {
    // The SAME predicate `elaborate_child_params_only` uses to pick its args
    // arm, so the two cannot disagree about which params are arg-supplied.
    if args.iter().any(|(name, _)| *name == cell.id.member) {
        DependencyTrace::default()
    } else {
        cell.default_expr
            .as_ref()
            .map(extract_dependency_trace)
            .unwrap_or_default()
    }
}

fn params_only(cells: Vec<Phase1Step<'_>>) -> Vec<Phase1Step<'_>> {
    cells
        .into_iter()
        .filter(|step| matches!(step, Phase1Step::Param(_)))
        .collect()
}

#[cfg(test)]
mod tests {
    use reify_compiler::{CompiledModule, find_template};
    use reify_core::Type;
    use reify_ir::{CompiledExpr, Value};
    use reify_test_support::parse_and_compile;

    use super::{Phase1Step, phase1_walk};

    /// The walk as `(kind, member)` pairs: which cells phase 1 evaluates, and in
    /// what order.
    fn walk_of(
        module: &CompiledModule,
        template: &str,
        args: &[(String, CompiledExpr)],
    ) -> Vec<(&'static str, String)> {
        let template = find_template(&module.templates, template).expect("template compiled");
        phase1_walk(template, args)
            .into_iter()
            .map(|step| match step {
                Phase1Step::Param(cell) => ("param", cell.id.member.clone()),
                Phase1Step::ScratchLet { key, .. } => ("let", key.member.clone()),
            })
            .collect()
    }

    fn steps(expected: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        expected
            .iter()
            .map(|(kind, member)| (*kind, member.to_string()))
            .collect()
    }

    #[test]
    fn a_let_no_param_reads_is_never_walked() {
        let module = parse_and_compile(
            "structure W { \
                let unused = 5 \
                param a : Int = 1 \
                let used = a * 2 \
                param b : Int = used \
            }",
        );
        assert_eq!(
            walk_of(&module, "W", &[]),
            steps(&[("param", "a"), ("let", "used"), ("param", "b")]),
        );
    }

    #[test]
    fn arg_supplied_param_pulls_in_no_let() {
        let module = parse_and_compile(
            "structure W2 { \
                let dbl = 2 \
                param p : Int = dbl \
            }",
        );
        let args = [(
            "p".to_string(),
            CompiledExpr::literal(Value::Int(9), Type::Int),
        )];
        assert_eq!(walk_of(&module, "W2", &args), steps(&[("param", "p")]));
    }

    #[test]
    fn params_reading_no_sibling_keep_declaration_order() {
        let module = parse_and_compile(
            "structure W3 { \
                param b : Int = 2 \
                let l = b \
                param a : Int = 1 \
            }",
        );
        assert_eq!(
            walk_of(&module, "W3", &[]),
            steps(&[("param", "b"), ("param", "a")]),
        );
    }
}
