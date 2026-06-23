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
}
