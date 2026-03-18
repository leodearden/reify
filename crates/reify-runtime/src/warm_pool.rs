// WarmStatePool: memory-budgeted pool for warm-start state across nodes.
// Implementation pending.

#[cfg(test)]
mod tests {
    use reify_eval::cache::NodeId;
    use reify_types::{OpaqueState, ValueCellId};

    // Placeholder struct for compilation — will be replaced by real impl
    struct WarmStatePool;

    #[test]
    fn donate_and_retrieve_roundtrip() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let state = OpaqueState::new(42i32, 4);

        pool.donate(node.clone(), state);
        let retrieved = pool.retrieve(&node);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(42));
    }

    #[test]
    fn retrieve_removes_entry() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 4));

        let first = pool.retrieve(&node);
        assert!(first.is_some());

        let second = pool.retrieve(&node);
        assert!(second.is_none());
    }

    #[test]
    fn used_bytes_tracks_correctly() {
        let mut pool = WarmStatePool::new(1024);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));

        assert_eq!(pool.used_bytes(), 0);

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 100));
        assert_eq!(pool.used_bytes(), 100);

        pool.donate(node_b.clone(), OpaqueState::new(2i32, 200));
        assert_eq!(pool.used_bytes(), 300);

        pool.retrieve(&node_a);
        assert_eq!(pool.used_bytes(), 200);

        pool.retrieve(&node_b);
        assert_eq!(pool.used_bytes(), 0);
    }
}
