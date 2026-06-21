//! Priority promotion for in-flight tasks.
//!
//! When a higher-priority task depends on a lower-priority in-flight task,
//! the lower-priority task is promoted. Per §8.2: 'if a P1-slow task depends
//! on a P3 task already in-flight, the P3 task is promoted to P1-slow.'

use std::collections::HashMap;
use std::sync::Mutex;

use reify_eval::cache::NodeId;

use crate::Priority;

/// Tracks the effective priority of in-flight tasks and supports promotion.
///
/// The effective priority may differ from the original Task priority when
/// a higher-priority demanded node depends on this in-flight task.
/// The original Task struct is not mutated — PriorityPromoter tracks
/// the dynamic effective priority separately.
pub struct PriorityPromoter {
    /// Maps in-flight node → current effective priority.
    effective: HashMap<NodeId, Priority>,
}

impl PriorityPromoter {
    /// Create a new empty promoter.
    pub fn new() -> Self {
        Self {
            effective: HashMap::new(),
        }
    }

    /// Register an in-flight task with its initial priority.
    pub fn register(&mut self, node_id: NodeId, priority: Priority) {
        self.effective.insert(node_id, priority);
    }

    /// Get the current effective priority for a node, if registered.
    pub fn effective_priority(&self, node_id: &NodeId) -> Option<Priority> {
        self.effective.get(node_id).copied()
    }

    /// Promote a node to a higher priority.
    ///
    /// Only raises priority (lower enum value = higher priority via Ord).
    /// Same-or-lower-priority promotions are no-ops.
    pub fn promote(&mut self, node_id: &NodeId, new_priority: Priority) {
        if let Some(current) = self.effective.get_mut(node_id)
            && new_priority < *current
        {
            *current = new_priority;
        }
    }

    /// Remove a node from the promoter (on completion or cancellation).
    pub fn remove(&mut self, node_id: &NodeId) {
        self.effective.remove(node_id);
    }

    /// Remove multiple nodes in a single call (avoids per-node locking overhead
    /// when called through [`SharedPriorityPromoter::batch_remove`]).
    pub fn batch_remove(&mut self, nodes: &[NodeId]) {
        for node in nodes {
            self.effective.remove(node);
        }
    }

    /// Return the number of tracked nodes (for test verification of cleanup).
    pub fn count(&self) -> usize {
        self.effective.len()
    }

    /// Promote all in-flight dependencies of a demanded node transitively.
    ///
    /// Walks dependency edges from `demanded_node` and promotes all in-flight
    /// lower-priority tasks to the demanded node's priority level.
    ///
    /// `dependency_map` maps each node to its direct dependencies (forward edges).
    /// Non-in-flight dependencies are silently ignored.
    pub fn promote_for_demand(
        &mut self,
        demanded_node: &NodeId,
        demand_priority: Priority,
        dependency_map: &HashMap<NodeId, Vec<NodeId>>,
    ) {
        // BFS/DFS walk from demanded_node through dependency edges
        let mut stack = Vec::new();
        if let Some(deps) = dependency_map.get(demanded_node) {
            stack.extend(deps.iter().cloned());
        }

        let mut visited = std::collections::HashSet::new();
        while let Some(node) = stack.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }

            // Promote if in-flight
            self.promote(&node, demand_priority);

            // Continue walking forward dependencies
            if let Some(deps) = dependency_map.get(&node) {
                stack.extend(deps.iter().cloned());
            }
        }
    }
}

impl Default for PriorityPromoter {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe wrapper around [`PriorityPromoter`] for concurrent access.
///
/// Uses `Mutex` (not `RwLock`) because priority operations are extremely fast
/// (HashMap lookups) and the Mutex avoids reader-writer distinction overhead.
/// This matches the `SkipState` pattern in `ConcurrentEvalAdapter`.
pub struct SharedPriorityPromoter {
    inner: Mutex<PriorityPromoter>,
}

impl SharedPriorityPromoter {
    /// Create a new shared promoter.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(PriorityPromoter::new()),
        }
    }

    /// Register an in-flight task with its initial priority.
    pub fn register(&self, node_id: NodeId, priority: Priority) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(node_id, priority);
    }

    /// Get the current effective priority for a node.
    pub fn effective_priority(&self, node_id: &NodeId) -> Option<Priority> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .effective_priority(node_id)
    }

    /// Promote a node to a higher priority (lower enum value).
    pub fn promote(&self, node_id: &NodeId, new_priority: Priority) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .promote(node_id, new_priority);
    }

    /// Remove a node from the promoter.
    pub fn remove(&self, node_id: &NodeId) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(node_id);
    }

    /// Remove multiple nodes in a single critical section (one mutex acquisition).
    pub fn batch_remove(&self, nodes: &[NodeId]) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .batch_remove(nodes);
    }

    /// Return the number of tracked nodes (for test verification of cleanup).
    pub fn count(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).count()
    }

    /// Promote all in-flight dependencies of a demanded node transitively.
    pub fn promote_for_demand(
        &self,
        demanded_node: &NodeId,
        demand_priority: Priority,
        dependency_map: &HashMap<NodeId, Vec<NodeId>>,
    ) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .promote_for_demand(demanded_node, demand_priority, dependency_map);
    }
}

impl Default for SharedPriorityPromoter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::ValueCellId;

    fn make_node(name: &str) -> NodeId {
        NodeId::Value(ValueCellId::new("T", name))
    }

    #[test]
    fn register_and_effective_priority() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P3Speculative);
        assert_eq!(
            promoter.effective_priority(&node),
            Some(Priority::P3Speculative)
        );
    }

    #[test]
    fn effective_priority_unknown_node_returns_none() {
        let promoter = PriorityPromoter::new();
        let node = make_node("unknown");
        assert_eq!(promoter.effective_priority(&node), None);
    }

    #[test]
    fn promote_raises_priority() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P3Speculative);
        promoter.promote(&node, Priority::P1Slow);
        assert_eq!(promoter.effective_priority(&node), Some(Priority::P1Slow));
    }

    #[test]
    fn promote_with_same_or_lower_priority_is_noop() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P1Slow);
        // Promote to same priority → no-op
        promoter.promote(&node, Priority::P1Slow);
        assert_eq!(promoter.effective_priority(&node), Some(Priority::P1Slow));
        // Promote to lower priority → no-op
        promoter.promote(&node, Priority::P3Speculative);
        assert_eq!(promoter.effective_priority(&node), Some(Priority::P1Slow));
    }

    #[test]
    fn remove_cleans_up() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P1Fast);
        promoter.remove(&node);
        assert_eq!(promoter.effective_priority(&node), None);
    }

    // --- promote_for_demand tests ---

    #[test]
    fn promote_for_demand_promotes_p3_dependency_to_p1_slow() {
        let mut promoter = PriorityPromoter::new();
        let demanded = make_node("demanded");
        let dep = make_node("dep");

        // dep is in-flight at P3
        promoter.register(dep.clone(), Priority::P3Speculative);

        // Build dependency map: demanded depends on dep
        let mut deps: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        deps.insert(demanded.clone(), vec![dep.clone()]);

        promoter.promote_for_demand(&demanded, Priority::P1Slow, &deps);

        assert_eq!(promoter.effective_priority(&dep), Some(Priority::P1Slow));
    }

    #[test]
    fn promote_for_demand_transitive_chain() {
        // A(P1Slow) -> B(P3) -> C(P3) should promote both B and C
        let mut promoter = PriorityPromoter::new();
        let a = make_node("a");
        let b = make_node("b");
        let c = make_node("c");

        promoter.register(b.clone(), Priority::P3Speculative);
        promoter.register(c.clone(), Priority::P3Speculative);

        let mut deps: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        deps.insert(a.clone(), vec![b.clone()]);
        deps.insert(b.clone(), vec![c.clone()]);

        promoter.promote_for_demand(&a, Priority::P1Slow, &deps);

        assert_eq!(promoter.effective_priority(&b), Some(Priority::P1Slow));
        assert_eq!(promoter.effective_priority(&c), Some(Priority::P1Slow));
    }

    #[test]
    fn promote_for_demand_does_not_demote_higher_priority() {
        let mut promoter = PriorityPromoter::new();
        let demanded = make_node("demanded");
        let dep = make_node("dep");

        // dep is already at P0Interactive (higher than P1Slow)
        promoter.register(dep.clone(), Priority::P0Interactive);

        let mut deps: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        deps.insert(demanded.clone(), vec![dep.clone()]);

        promoter.promote_for_demand(&demanded, Priority::P1Slow, &deps);

        // Should stay at P0Interactive (not demoted to P1Slow)
        assert_eq!(
            promoter.effective_priority(&dep),
            Some(Priority::P0Interactive)
        );
    }

    #[test]
    fn promote_for_demand_ignores_non_inflight_dependency() {
        let mut promoter = PriorityPromoter::new();
        let demanded = make_node("demanded");
        let dep = make_node("dep");
        // dep is NOT registered (not in-flight)

        let mut deps: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        deps.insert(demanded.clone(), vec![dep.clone()]);

        promoter.promote_for_demand(&demanded, Priority::P1Slow, &deps);

        // dep is not in-flight, so no effective priority
        assert_eq!(promoter.effective_priority(&dep), None);
    }

    // --- SharedPriorityPromoter (concurrent wrapper) tests ---

    #[test]
    fn batch_remove_removes_specified_nodes_and_preserves_others() {
        let mut promoter = PriorityPromoter::new();
        let a = make_node("a");
        let b = make_node("b");
        let c = make_node("c");

        promoter.register(a.clone(), Priority::P0Interactive);
        promoter.register(b.clone(), Priority::P1Slow);
        promoter.register(c.clone(), Priority::P3Speculative);
        assert_eq!(promoter.count(), 3);

        // Batch remove a and c, leaving b
        promoter.batch_remove(&[a.clone(), c.clone()]);

        assert_eq!(promoter.effective_priority(&a), None);
        assert_eq!(promoter.effective_priority(&c), None);
        assert_eq!(promoter.effective_priority(&b), Some(Priority::P1Slow));
        assert_eq!(promoter.count(), 1);
    }

    #[test]
    fn shared_promoter_register_and_read() {
        let shared = SharedPriorityPromoter::new();
        let node = make_node("a");
        shared.register(node.clone(), Priority::P3Speculative);
        assert_eq!(
            shared.effective_priority(&node),
            Some(Priority::P3Speculative)
        );
    }

    #[test]
    fn shared_promoter_promote_from_another_thread() {
        use std::sync::Arc;
        use std::thread;

        let shared = Arc::new(SharedPriorityPromoter::new());
        let node = make_node("a");
        shared.register(node.clone(), Priority::P3Speculative);

        let shared2 = Arc::clone(&shared);
        let node2 = node.clone();
        let handle = thread::spawn(move || {
            shared2.promote(&node2, Priority::P1Slow);
        });
        handle.join().unwrap();

        assert_eq!(shared.effective_priority(&node), Some(Priority::P1Slow));
    }

    #[test]
    fn shared_promoter_concurrent_promotes() {
        use std::sync::Arc;
        use std::thread;

        let shared = Arc::new(SharedPriorityPromoter::new());
        let node = make_node("a");
        shared.register(node.clone(), Priority::P3Speculative);

        let mut handles = Vec::new();
        // Spawn multiple threads that all try to promote
        for _ in 0..10 {
            let s = Arc::clone(&shared);
            let n = node.clone();
            handles.push(thread::spawn(move || {
                s.promote(&n, Priority::P1Fast);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        // All promotions should result in P1Fast
        assert_eq!(shared.effective_priority(&node), Some(Priority::P1Fast));
    }

    // --- Integration test: commitment + priority promotion ---

    #[test]
    fn integration_commitment_and_priority_promotion() {
        use crate::commitment::{
            CommitmentPolicy, CommitmentTracker, CommitmentTransition, NodeCommitmentOverride,
            TaskProgress,
        };
        use std::time::Duration;

        // Setup: 3 nodes at different priorities
        // node_spec (P3 speculative) → long-running
        // node_demanded (P1Slow demanded) → depends on node_spec
        // node_cancel (P3 speculative) → short-running, stale
        let node_spec = make_node("speculative");
        let node_demanded = make_node("demanded");
        let node_cancel = make_node("cancelable");

        // --- Priority promotion scenario ---
        let mut promoter = PriorityPromoter::new();
        promoter.register(node_spec.clone(), Priority::P3Speculative);
        promoter.register(node_demanded.clone(), Priority::P1Slow);
        promoter.register(node_cancel.clone(), Priority::P3Speculative);

        // node_demanded depends on node_spec
        let mut deps: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        deps.insert(node_demanded.clone(), vec![node_spec.clone()]);

        // Promote dependencies of demanded node
        promoter.promote_for_demand(&node_demanded, Priority::P1Slow, &deps);

        // node_spec should now be P1Slow (promoted from P3)
        assert_eq!(
            promoter.effective_priority(&node_spec),
            Some(Priority::P1Slow),
            "P3 speculative node should be promoted to P1Slow"
        );
        // node_cancel is unaffected (not a dependency of demanded)
        assert_eq!(
            promoter.effective_priority(&node_cancel),
            Some(Priority::P3Speculative),
            "unrelated node should remain at original priority"
        );

        // --- Commitment scenario ---
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(60),
            commit_when_proportion_done: 0.5,
        };
        let mut tracker = CommitmentTracker::new(policy);

        // Register both nodes
        tracker.register_task(node_spec.clone(), NodeCommitmentOverride::CommitIfSlow);
        tracker.register_task(node_cancel.clone(), NodeCommitmentOverride::CommitIfSlow);

        // node_spec has been running long (committed)
        let spec_progress = TaskProgress {
            elapsed: Duration::from_secs(61),
            reported_progress: None,
            previous_runtime: None,
        };
        let transition = tracker.update_status(&node_spec, &spec_progress, false);
        assert_eq!(transition, Some(CommitmentTransition::BecameCommitted));
        assert!(
            tracker.should_continue(&node_spec, true, Priority::P1Slow),
            "committed node in dirty cone should continue"
        );

        // node_cancel is not yet committed (short elapsed)
        let cancel_progress = TaskProgress {
            elapsed: Duration::from_secs(5),
            reported_progress: Some(0.1),
            previous_runtime: None,
        };
        tracker.update_status(&node_cancel, &cancel_progress, false);
        assert!(
            !tracker.should_continue(&node_cancel, true, Priority::P1Slow),
            "uncommitted node in dirty cone should NOT continue"
        );
    }

    #[test]
    fn shared_batch_remove_drops_count_and_handles_idempotent_removal() {
        let shared = SharedPriorityPromoter::new();
        let a = make_node("a");
        let b = make_node("b");
        let c = make_node("c");

        shared.register(a.clone(), Priority::P0Interactive);
        shared.register(b.clone(), Priority::P1Slow);
        shared.register(c.clone(), Priority::P3Speculative);
        assert_eq!(shared.count(), 3);

        // Batch remove a and c
        shared.batch_remove(&[a.clone(), c.clone()]);
        assert_eq!(shared.count(), 1);
        assert_eq!(shared.effective_priority(&b), Some(Priority::P1Slow));
        assert_eq!(shared.effective_priority(&a), None);
        assert_eq!(shared.effective_priority(&c), None);

        // Idempotent: removing already-removed nodes should not panic or change count
        shared.batch_remove(&[a.clone(), c.clone()]);
        assert_eq!(shared.count(), 1);
    }

    #[test]
    fn shared_promoter_recovers_after_lock_poisoning() {
        use std::sync::Arc;
        use std::thread;

        let shared = Arc::new(SharedPriorityPromoter::new());
        let node_a = make_node("a");
        let node_b = make_node("b");

        // Register node_a at P3Speculative
        shared.register(node_a.clone(), Priority::P3Speculative);

        // Poison the inner Mutex by spawning a thread that acquires
        // the lock and panics while holding it.
        let shared_clone = Arc::clone(&shared);
        thread::spawn(move || {
            let _guard = shared_clone.inner.lock().unwrap();
            panic!("intentional panic to poison inner mutex");
        })
        .join()
        .ok();

        // All operations should recover without panicking:

        // effective_priority should still return the registered value
        assert_eq!(
            shared.effective_priority(&node_a),
            Some(Priority::P3Speculative),
            "effective_priority should recover after poisoning"
        );

        // register should succeed for a new node
        shared.register(node_b.clone(), Priority::P1Fast);

        // promote should succeed
        shared.promote(&node_a, Priority::P1Slow);
        assert_eq!(
            shared.effective_priority(&node_a),
            Some(Priority::P1Slow),
            "promote should work after poisoning"
        );

        // remove should succeed
        shared.remove(&node_b);
        assert_eq!(
            shared.effective_priority(&node_b),
            None,
            "remove should work after poisoning"
        );

        // promote_for_demand should succeed
        let node_c = make_node("c");
        shared.register(node_c.clone(), Priority::P3Speculative);
        let mut deps: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        deps.insert(node_a.clone(), vec![node_c.clone()]);
        shared.promote_for_demand(&node_a, Priority::P0Interactive, &deps);
        assert_eq!(
            shared.effective_priority(&node_c),
            Some(Priority::P0Interactive),
            "promote_for_demand should work after poisoning"
        );
    }
}
