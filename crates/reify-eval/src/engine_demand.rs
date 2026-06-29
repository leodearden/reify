//! Production-demand population API (selective-demand α, task 4737).
//!
//! These `Engine` methods mutate the PRODUCTION `self.demand` registry — the one
//! `compute_eval_set` intersects against to schedule a warm `edit_param` — so
//! that the GUI can drive evaluation SELECTIVELY from the set of viewport-visible
//! `Realization` roots. This is the enforcement counterpart to the task-4532
//! observed-demand side-channel (`observed_demand.rs`): that registry is PASSIVE
//! (measurement only, never fed into `compute_eval_set`), whereas these methods
//! INTENTIONALLY change scheduling — a hidden body's exclusive value cells drop
//! out of the demand cone and are pruned from the warm eval-set.
//!
//! The cold `eval()`/`check()`/`build()` paths keep a deterministic FULL scope via
//! the [`DemandRegistry`] `full_scope` override (α step-2): they flip
//! `is_demanded` to true for every node without destroying the selective roots
//! underneath, so a GUI selection survives a cold pass (PRD D2). See
//! [`Engine::set_demand_full_scope`].
//!
//! The plumbing deliberately mirrors `observed_demand.rs` (same
//! [`DemandRegistry::rebuild_cone`] backward-BFS, same `eval_state`-guarded
//! no-op rebuild) but targets `self.demand` instead of `self.observed_demand`.
//! The 4532 observed-demand methods are left untouched.

use crate::Engine;
use crate::cache::NodeId;
use crate::demand::DemandRegistry;

impl Engine {
    /// Populate the PRODUCTION demand registry selectively from a set of visible
    /// `Realization` roots (the viewport-visible bodies).
    ///
    /// REPLACES the current roots with `visible_realizations`, turns the
    /// `full_scope` cold-override OFF, and rebuilds the demand cone against the
    /// current snapshot graph — so the next warm `edit_param` schedules only the
    /// backward closure of the visible realizations (hidden bodies' exclusive
    /// cells are pruned). No-op cone rebuild when no `eval()` has run yet (mirrors
    /// [`Engine::rebuild_observed_cone`]); the roots are still recorded and the
    /// cone fills in on the next [`Engine::rebuild_demand_cone`].
    ///
    /// Although typed `IntoIterator<Item = NodeId>` for caller convenience, the
    /// intended roots are `NodeId::Realization`s; non-realization nodes are added
    /// verbatim and their own backward closures are pulled in by `rebuild_cone`.
    pub fn set_demand_selective(&mut self, visible_realizations: impl IntoIterator<Item = NodeId>) {
        // A fresh registry clears the prior roots AND resets `full_scope` to its
        // `false` default in one move — the idiomatic "replace" (mirrors
        // `reset_observed_demand`'s `DemandRegistry::new()`).
        let mut registry = DemandRegistry::new();
        for node in visible_realizations {
            registry.add_demand(node);
        }
        self.demand = registry;
        self.rebuild_demand_cone();
        // δ (task 4740) step-5: after the cone is rebuilt, mark every currently-Final
        // cache entry that just LEFT the demand cone as `Pending`.  This is the
        // "mark on hide" step: when body_b is hidden, its exclusive cells (e.g.
        // `sb = w*2`) become Pending so that a subsequent re-demand + tessellate can
        // detect the stale cache entries and refresh them against the current params
        // before geometry executes.  No-op under `full_scope` (every node is
        // demanded) and when nothing was pruned.
        self.mark_demand_pruned_pending();
    }

    /// Add `node` to the PRODUCTION demand roots. Call
    /// [`Engine::rebuild_demand_cone`] afterward to refresh the cone.
    pub fn add_demand(&mut self, node: NodeId) {
        self.demand.add_demand(node);
    }

    /// Remove `node` from the PRODUCTION demand roots. Call
    /// [`Engine::rebuild_demand_cone`] afterward to refresh the cone.
    pub fn remove_demand(&mut self, node: &NodeId) {
        self.demand.remove_demand(node);
    }

    /// Rebuild the PRODUCTION demand cone against the current snapshot graph.
    ///
    /// No-op when there is no eval state (no `eval()` has run yet), exactly like
    /// [`Engine::rebuild_observed_cone`] — but rebuilds `self.demand`, the
    /// registry `compute_eval_set` actually reads.
    pub fn rebuild_demand_cone(&mut self) {
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }
    }

    /// Set the cold-path FULL-scope override on the PRODUCTION demand registry.
    ///
    /// `true` makes every node demanded (the deterministic cold `eval`/`check`/
    /// `build` scope) WITHOUT clearing the selective roots underneath; `false`
    /// restores selective cone-membership. See [`DemandRegistry::set_full_scope`].
    pub fn set_demand_full_scope(&mut self, full_scope: bool) {
        self.demand.set_full_scope(full_scope);
    }

    /// Size of the PRODUCTION demand cone (post-rebuild). Inspection only.
    pub fn demand_cone_size(&self) -> usize {
        self.demand.cone_size()
    }

    /// Whether `node` is demanded by the PRODUCTION registry — `full_scope` OR
    /// membership in the rebuilt cone. Inspection only.
    pub fn demand_is_demanded(&self, node: &NodeId) -> bool {
        self.demand.is_demanded(node)
    }

    /// Whether the PRODUCTION registry's cold full-scope override is currently set.
    pub fn demand_is_full_scope(&self) -> bool {
        self.demand.is_full_scope()
    }

    /// Demand-prune **Pending producer** wired at the Engine facade (task #4739 γ).
    ///
    /// No-op (returns `0`) when the production demand registry is in its cold
    /// `full_scope` override (the `eval()`/`check()`/`build()` cold path): under
    /// full scope every node is demanded, so nothing is pruned and no cached
    /// `Final` value is stale. On the warm/selective path (`full_scope` OFF) it
    /// flips every cached `Final` node OUTSIDE the demand cone to `Pending`, so a
    /// hidden body's cached value is never served as a silently-stale `Final`
    /// number (arch §8 prune-safety scenario 3). Returns the number of nodes
    /// flipped.
    ///
    /// Delegates to [`crate::cache::CacheStore::mark_pruned_pending`] with
    /// [`DemandRegistry::is_demanded`] as the prune predicate — the SAME
    /// predicate β's warm seed (`demand_scoped_unified_pass`) uses, so the prune
    /// decision matches the schedule exactly. Must be re-run on every warm build
    /// (not once at demand-change): a cold full-scope `eval()`/`check()` can
    /// re-`Final` a still-hidden body between warm edits.
    pub fn mark_demand_pruned_pending(&mut self) -> usize {
        if self.demand.is_full_scope() {
            return 0;
        }
        // Disjoint-field borrow: bind a shared borrow of `self.demand` BEFORE the
        // `&mut self.cache` call so the prune closure can read the registry while
        // the cache is mutated (the two are distinct Engine fields).
        let demand = &self.demand;
        self.cache.mark_pruned_pending(|n| demand.is_demanded(n))
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use reify_test_support::mocks::MockConstraintChecker;

    /// `Engine::mark_demand_pruned_pending` is a no-op (returns 0) under the cold
    /// `full_scope` override: every node is demanded, so nothing is pruned. This
    /// pins the cold-path safety guarantee (existing cold eval/check/build tests
    /// stay green).
    ///
    /// Step-7 test (task #4739 γ): fails to compile until step-8 adds the
    /// `Engine::mark_demand_pruned_pending` method.
    #[test]
    fn mark_demand_pruned_pending_is_noop_under_full_scope() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.set_demand_full_scope(true);
        assert_eq!(
            engine.mark_demand_pruned_pending(),
            0,
            "mark_demand_pruned_pending must be a no-op (return 0) under full scope"
        );
    }
}
