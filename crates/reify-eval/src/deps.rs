// Dependency tracking for incremental re-evaluation.

use reify_types::ValueCellId;

/// Records which value cells were read during expression evaluation.
/// Duplicate reads are preserved to reflect the actual evaluation trace.
#[derive(Clone, Debug, Default)]
pub struct DependencyTrace {
    pub reads: Vec<ValueCellId>,
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
}
