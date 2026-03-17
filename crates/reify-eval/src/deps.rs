// Dependency tracking for incremental re-evaluation.

use reify_types::ValueCellId;

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
}
