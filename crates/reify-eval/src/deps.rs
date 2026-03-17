use reify_types::ValueCellId;

/// Tracks which value cells a node read during evaluation.
///
/// This is a minimal stub for task 12 (content-hash caching).
/// Task 11 will replace this with a full dependency tracing implementation.
#[derive(Debug, Clone, Default)]
pub struct DependencyTrace {
    pub reads: Vec<ValueCellId>,
}
