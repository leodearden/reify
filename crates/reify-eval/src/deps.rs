// Dependency tracking for incremental re-evaluation.

use std::collections::{HashMap, HashSet};

use reify_types::{NodeId, ValueCellId};

/// Records which value cells were read during expression evaluation.
/// Duplicate reads are preserved to reflect the actual evaluation trace.
#[derive(Clone, Debug, Default)]
pub struct DependencyTrace {
    pub reads: Vec<ValueCellId>,
}

/// Accumulates value cell reads during expression evaluation.
/// Use with `eval_expr_traced` by calling `record_read` from the callback.
pub struct TraceRecorder {
    trace: DependencyTrace,
}

impl TraceRecorder {
    pub fn new() -> Self {
        Self {
            trace: DependencyTrace::default(),
        }
    }

    /// Record a read of the given value cell.
    pub fn record_read(&mut self, cell: ValueCellId) {
        self.trace.reads.push(cell);
    }

    /// Consume the recorder and return the completed trace.
    pub fn finish(self) -> DependencyTrace {
        self.trace
    }
}

/// Inverted index mapping each value cell to the set of nodes that read it.
/// Enables O(1) lookup of which nodes need re-evaluation when a cell changes.
#[derive(Debug, Clone, Default)]
pub struct ReverseDependencyIndex {
    /// cell → set of nodes that depend on it
    index: HashMap<ValueCellId, HashSet<NodeId>>,
}

impl ReverseDependencyIndex {
    /// Build a reverse index from a complete set of node traces.
    pub fn build(traces: &HashMap<NodeId, DependencyTrace>) -> Self {
        let mut index: HashMap<ValueCellId, HashSet<NodeId>> = HashMap::new();
        for (node, trace) in traces {
            for cell in &trace.reads {
                index
                    .entry(cell.clone())
                    .or_default()
                    .insert(node.clone());
            }
        }
        Self { index }
    }

    /// Return the set of nodes that depend on the given cell.
    /// Returns an empty set if no nodes depend on it.
    pub fn dependents_of(&self, cell: &ValueCellId) -> &HashSet<NodeId> {
        static EMPTY: std::sync::LazyLock<HashSet<NodeId>> =
            std::sync::LazyLock::new(HashSet::new);
        self.index.get(cell).unwrap_or(&EMPTY)
    }
}

#[cfg(test)]
mod tests {
    use reify_types::ValueCellId;

    #[test]
    fn dependency_trace_default_is_empty() {
        let trace = super::DependencyTrace::default();
        assert!(trace.reads.is_empty());
    }

    #[test]
    fn dependency_trace_push_reads() {
        let mut trace = super::DependencyTrace::default();
        trace.reads.push(ValueCellId::new("B", "width"));
        trace.reads.push(ValueCellId::new("B", "height"));
        assert_eq!(trace.reads.len(), 2);
        assert_eq!(trace.reads[0], ValueCellId::new("B", "width"));
        assert_eq!(trace.reads[1], ValueCellId::new("B", "height"));
    }

    #[test]
    fn dependency_trace_clone_is_independent() {
        let mut trace = super::DependencyTrace::default();
        trace.reads.push(ValueCellId::new("B", "width"));
        let mut cloned = trace.clone();
        cloned.reads.push(ValueCellId::new("B", "height"));
        assert_eq!(trace.reads.len(), 1);
        assert_eq!(cloned.reads.len(), 2);
    }

    #[test]
    fn dependency_trace_debug() {
        let trace = super::DependencyTrace::default();
        let debug = format!("{:?}", trace);
        assert!(debug.contains("DependencyTrace"));
    }

    #[test]
    fn dependency_trace_preserves_duplicates() {
        let mut trace = super::DependencyTrace::default();
        let id = ValueCellId::new("B", "width");
        trace.reads.push(id.clone());
        trace.reads.push(id.clone());
        assert_eq!(trace.reads.len(), 2);
    }

    // --- TraceRecorder tests ---

    #[test]
    fn trace_recorder_new_is_empty() {
        let recorder = super::TraceRecorder::new();
        let trace = recorder.finish();
        assert!(trace.reads.is_empty());
    }

    #[test]
    fn trace_recorder_record_read_captures_id() {
        let mut recorder = super::TraceRecorder::new();
        let id = ValueCellId::new("B", "width");
        recorder.record_read(id.clone());
        let trace = recorder.finish();
        assert_eq!(trace.reads, vec![id]);
    }

    #[test]
    fn trace_recorder_finish_returns_reads_in_order() {
        let mut recorder = super::TraceRecorder::new();
        recorder.record_read(ValueCellId::new("B", "width"));
        recorder.record_read(ValueCellId::new("B", "height"));
        recorder.record_read(ValueCellId::new("B", "thickness"));
        let trace = recorder.finish();
        assert_eq!(trace.reads[0], ValueCellId::new("B", "width"));
        assert_eq!(trace.reads[1], ValueCellId::new("B", "height"));
        assert_eq!(trace.reads[2], ValueCellId::new("B", "thickness"));
    }

    #[test]
    fn trace_recorder_records_duplicates() {
        let mut recorder = super::TraceRecorder::new();
        let id = ValueCellId::new("B", "width");
        recorder.record_read(id.clone());
        recorder.record_read(id.clone());
        let trace = recorder.finish();
        assert_eq!(trace.reads.len(), 2);
    }

    // --- TraceRecorder + eval_expr_traced integration ---

    #[test]
    fn trace_recorder_with_eval_expr_traced_volume() {
        use reify_types::{BinOp, CompiledExpr, DimensionVector, Type, Value, ValueMap};

        let mut values = ValueMap::new();
        let width_id = ValueCellId::new("B", "width");
        let height_id = ValueCellId::new("B", "height");
        let thickness_id = ValueCellId::new("B", "thickness");

        let mm = |v: f64| Value::Scalar {
            si_value: v * 0.001,
            dimension: DimensionVector::LENGTH,
        };

        values.insert(width_id.clone(), mm(80.0));
        values.insert(height_id.clone(), mm(100.0));
        values.insert(thickness_id.clone(), mm(5.0));

        let w = CompiledExpr::value_ref(width_id.clone(), Type::length());
        let h = CompiledExpr::value_ref(height_id.clone(), Type::length());
        let t = CompiledExpr::value_ref(thickness_id.clone(), Type::length());

        let wh = CompiledExpr::binop(
            BinOp::Mul,
            w,
            h,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let volume = CompiledExpr::binop(
            BinOp::Mul,
            wh,
            t,
            Type::Scalar {
                dimension: DimensionVector::VOLUME,
            },
        );

        let mut recorder = super::TraceRecorder::new();
        let _result = reify_expr::eval_expr_traced(&volume, &values, &mut |id| {
            recorder.record_read(id.clone());
        });

        let trace = recorder.finish();
        assert_eq!(trace.reads, vec![width_id, height_id, thickness_id]);
    }

    // --- ReverseDependencyIndex::build tests ---

    #[test]
    fn reverse_index_build_from_traces() {
        use std::collections::{HashMap, HashSet};
        use reify_types::{ConstraintNodeId, NodeId};

        let width = ValueCellId::new("B", "width");
        let height = ValueCellId::new("B", "height");
        let thickness = ValueCellId::new("B", "thickness");

        let volume_node = NodeId::ValueCell(ValueCellId::new("B", "volume"));
        let constraint_node = NodeId::Constraint(ConstraintNodeId::new("B", 0));

        let mut traces = HashMap::new();

        // volume reads width, height, thickness
        let mut vol_trace = super::DependencyTrace::default();
        vol_trace.reads.push(width.clone());
        vol_trace.reads.push(height.clone());
        vol_trace.reads.push(thickness.clone());
        traces.insert(volume_node.clone(), vol_trace);

        // constraint reads thickness
        let mut con_trace = super::DependencyTrace::default();
        con_trace.reads.push(thickness.clone());
        traces.insert(constraint_node.clone(), con_trace);

        let index = super::ReverseDependencyIndex::build(&traces);

        // dependents_of(width) = {volume_node}
        let width_deps = index.dependents_of(&width);
        assert_eq!(width_deps, &HashSet::from([volume_node.clone()]));

        // dependents_of(height) = {volume_node}
        let height_deps = index.dependents_of(&height);
        assert_eq!(height_deps, &HashSet::from([volume_node.clone()]));

        // dependents_of(thickness) = {volume_node, constraint_node}
        let thickness_deps = index.dependents_of(&thickness);
        assert_eq!(
            thickness_deps,
            &HashSet::from([volume_node.clone(), constraint_node.clone()])
        );
    }

    #[test]
    fn reverse_index_empty_traces() {
        use std::collections::HashMap;

        let traces = HashMap::new();
        let index = super::ReverseDependencyIndex::build(&traces);

        let unknown = ValueCellId::new("X", "y");
        assert!(index.dependents_of(&unknown).is_empty());
    }

    // --- ReverseDependencyIndex::add_trace and remove_node tests ---

    #[test]
    fn reverse_index_add_trace() {
        use std::collections::HashSet;
        use reify_types::NodeId;

        let width = ValueCellId::new("B", "width");
        let volume_node = NodeId::ValueCell(ValueCellId::new("B", "volume"));

        let mut index = super::ReverseDependencyIndex::default();

        let mut trace = super::DependencyTrace::default();
        trace.reads.push(width.clone());
        index.add_trace(volume_node.clone(), &trace);

        assert_eq!(
            index.dependents_of(&width),
            &HashSet::from([volume_node])
        );
    }

    #[test]
    fn reverse_index_remove_node() {
        use std::collections::HashSet;
        use reify_types::{ConstraintNodeId, NodeId};

        let width = ValueCellId::new("B", "width");
        let thickness = ValueCellId::new("B", "thickness");
        let volume_node = NodeId::ValueCell(ValueCellId::new("B", "volume"));
        let constraint_node = NodeId::Constraint(ConstraintNodeId::new("B", 0));

        let mut index = super::ReverseDependencyIndex::default();

        // volume reads width, thickness
        let mut vol_trace = super::DependencyTrace::default();
        vol_trace.reads.push(width.clone());
        vol_trace.reads.push(thickness.clone());
        index.add_trace(volume_node.clone(), &vol_trace);

        // constraint reads thickness
        let mut con_trace = super::DependencyTrace::default();
        con_trace.reads.push(thickness.clone());
        index.add_trace(constraint_node.clone(), &con_trace);

        // Before removal: thickness has both dependents
        assert_eq!(
            index.dependents_of(&thickness),
            &HashSet::from([volume_node.clone(), constraint_node.clone()])
        );

        // Remove constraint node
        index.remove_node(&constraint_node);

        // After removal: thickness only has volume_node
        assert_eq!(
            index.dependents_of(&thickness),
            &HashSet::from([volume_node.clone()])
        );

        // width still has volume_node (unaffected)
        assert_eq!(
            index.dependents_of(&width),
            &HashSet::from([volume_node])
        );
    }

    #[test]
    fn reverse_index_remove_node_no_cross_contamination() {
        use std::collections::HashSet;
        use reify_types::NodeId;

        let a = ValueCellId::new("X", "a");
        let b = ValueCellId::new("X", "b");
        let node1 = NodeId::ValueCell(ValueCellId::new("X", "n1"));
        let node2 = NodeId::ValueCell(ValueCellId::new("X", "n2"));

        let mut index = super::ReverseDependencyIndex::default();

        // node1 reads a
        let mut t1 = super::DependencyTrace::default();
        t1.reads.push(a.clone());
        index.add_trace(node1.clone(), &t1);

        // node2 reads b
        let mut t2 = super::DependencyTrace::default();
        t2.reads.push(b.clone());
        index.add_trace(node2.clone(), &t2);

        // Remove node1
        index.remove_node(&node1);

        // a should be empty, b should still have node2
        assert!(index.dependents_of(&a).is_empty());
        assert_eq!(
            index.dependents_of(&b),
            &HashSet::from([node2])
        );
    }
}
