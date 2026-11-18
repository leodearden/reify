//! How `edit_source`'s main walk evaluates a cell through the instance-scope
//! mint, and settles the cells that took it, after the walk (#8102).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use reify_compiler::{CompiledFunction, ValueCellKind};
use reify_core::{Diagnostic, ValueCellId, VersionId};
use reify_ir::{CompiledExpr, DeterminacyState, PersistentMap, Value, ValueMap};

use crate::cache::NodeId;
use crate::cell_commit::{CacheLeg, CommitLegs, DeterminacyRule, commit_cell_result_at};
use crate::deps::{ReverseDependencyIndex, extract_dependency_trace};
use crate::eval_ctx_with_meta;
use crate::geometry_ops::symbolic_cell_mint::{InstanceMint, InstanceMintScope};
use crate::graph::EvaluationGraph;
use crate::journal::TraceSource;

/// A value `edit_source`'s main walk commits for a cell, and whether it took
/// the instance-scope mint path (#8102): a sub-instance cell whose plain
/// evaluation was `Undef`. That holds whatever the mint returned, `Undef`
/// included (a cell neither geometry nor selector, or a decline over a read
/// the instance has not committed yet), so it is NOT "a mint replaced the
/// value".
pub(super) struct WalkCellValue {
    pub(super) value: Value,
    pub(super) took_instance_mint_path: bool,
}

/// The read-only scope `edit_source`'s main walk evaluates a cell in.
pub(super) struct EditSourceWalkCtx<'a> {
    pub(super) graph: &'a EvaluationGraph,
    pub(super) functions: &'a [CompiledFunction],
    pub(super) meta_map: &'a HashMap<String, HashMap<String, String>>,
    pub(super) runtime_sink: &'a RefCell<Vec<Diagnostic>>,
}

impl EditSourceWalkCtx<'_> {
    /// `expr` evaluated over `values`; when that is `Undef` and `vcid` is a
    /// sub-instance cell, the cell minted in its instance's scope, as `eval()`
    /// mints it.
    pub(super) fn eval_cell(
        &self,
        values: &ValueMap,
        determinacy: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        vcid: &ValueCellId,
        kind: ValueCellKind,
        expr: &CompiledExpr,
    ) -> WalkCellValue {
        let evaluated = reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(values, self.functions, self.meta_map)
                .with_determinacy(determinacy)
                .with_runtime_diagnostics(self.runtime_sink),
        );
        if !matches!(evaluated, Value::Undef) {
            return WalkCellValue {
                value: evaluated,
                took_instance_mint_path: false,
            };
        }
        let instance_scope = InstanceMintScope {
            graph: self.graph,
            values,
            functions: self.functions,
            meta_map: self.meta_map,
        };
        match instance_scope.mint_undef_instance_cell(vcid, kind, expr) {
            InstanceMint::InstanceCell(minted) => WalkCellValue {
                value: minted,
                took_instance_mint_path: true,
            },
            InstanceMint::NotAnInstanceCell => WalkCellValue {
                value: Value::Undef,
                took_instance_mint_path: false,
            },
        }
    }

    /// Re-evaluates, over the values the walk committed, every cell that took
    /// the instance-mint path in the walk ([`WalkCellValue`]) and every
    /// template-scoped dependent (per `reverse_index`) of a cell this pass
    /// changes, and returns the cells it re-committed.
    ///
    /// The walk is ordered by template-scoped traces (`I.body` reads `T.len`,
    /// not `I.len`), so it can mint an instance cell over a sibling it has not
    /// re-committed yet, or decline it as `Undef` over one it has not
    /// committed at all, then cut that cell's consumers off as unchanged.
    ///
    /// The seed must hold EVERY mint-path cell, the `Undef` and declined ones
    /// too: an instance cell reads its instance siblings only through the
    /// mint's projection, and those reads are absent from the template-scoped
    /// `reverse_index`, so a sibling re-minted here never pulls them in. Each
    /// round visits `walk_order` and commits only a changed value; a round
    /// that changes nothing ends the pass. A changing round settles at least
    /// one more link of an acyclic read chain, which bounds the rounds.
    pub(super) fn settle_instance_mints(
        &self,
        instance_mint_path_cells: &[ValueCellId],
        walk_order: &[NodeId],
        reverse_index: &ReverseDependencyIndex,
        legs: CommitLegs<'_>,
        version: VersionId,
    ) -> Vec<NodeId> {
        let CommitLegs {
            values,
            snapshot_values,
            cache,
            journal,
        } = legs;
        let mut settling: HashSet<ValueCellId> = instance_mint_path_cells.iter().cloned().collect();
        let mut recommitted: Vec<NodeId> = Vec::new();
        if settling.is_empty() {
            return recommitted;
        }
        for _ in 0..=walk_order.len() {
            let mut changed = false;
            for node_id in walk_order {
                let NodeId::Value(vcid) = node_id else {
                    continue;
                };
                if !settling.contains(vcid) {
                    continue;
                }
                let Some(node) = self.graph.value_cells.get(vcid) else {
                    continue;
                };
                let Some(expr) = node.default_expr.as_ref() else {
                    continue;
                };
                let WalkCellValue { value, .. } =
                    self.eval_cell(values, snapshot_values, vcid, node.kind, expr);
                if values.get(vcid) == Some(&value) {
                    continue;
                }
                commit_cell_result_at(
                    Instant::now(),
                    CommitLegs {
                        values: &mut *values,
                        snapshot_values: &mut *snapshot_values,
                        cache: &mut *cache,
                        journal: &mut *journal,
                    },
                    vcid.clone(),
                    value,
                    DeterminacyRule::UnconditionalDetermined,
                    TraceSource::PostPassOverwrite,
                    extract_dependency_trace(expr),
                    version,
                    CacheLeg::Record,
                );
                for dependent in reverse_index.dependents_of(vcid) {
                    if let NodeId::Value(dependent) = dependent {
                        settling.insert(dependent.clone());
                    }
                }
                if !recommitted.contains(node_id) {
                    recommitted.push(node_id.clone());
                }
                changed = true;
            }
            if !changed {
                break;
            }
        }
        recommitted
    }
}
