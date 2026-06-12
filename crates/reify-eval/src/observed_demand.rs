//! Observed-demand measurement types (selective-demand precondition, task 4532).
//!
//! These types back a PASSIVE, side-channel measurement: given an "observed
//! demand" cone — what the GUI is actually displaying (viewport-visible
//! realizations, property-panel cells, constraint-panel constraints) — how
//! much of the production eval-set WOULD a selective-demand scheduler prune,
//! were the observed cone enforced as the demand cone?
//!
//! The measurement is observational ONLY. The observed cone is NEVER fed into
//! `compute_eval_set`; production `demand` and evaluation semantics are left
//! byte-for-byte untouched. See `docs/prds/v0_6/selective-demand.md` §G6.

use serde::{Deserialize, Serialize};

use crate::Engine;
use crate::cache::NodeId;
use crate::demand::DemandRegistry;

/// Per-edit measurement of how much of the production eval-set the observed
/// demand cone would prune, were it enforced as the demand cone.
///
/// Invariant (held by construction at the measurement site in `edit_param`):
/// `observed_retained + would_prune.total() == eval_set_size`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemandPruneMeasurement {
    /// Total size of the production eval-set for this edit
    /// (equal to `last_eval_set().len()`).
    pub eval_set_size: usize,
    /// Count of eval-set nodes that ARE in the observed demand cone — i.e. the
    /// nodes a selective-demand scheduler would still evaluate.
    pub observed_retained: usize,
    /// Counts, by `NodeId` kind, of eval-set nodes NOT in the observed cone —
    /// i.e. the nodes a selective-demand scheduler would prune.
    pub would_prune: WouldPruneByKind,
}

/// Would-prune counts broken down by `NodeId` kind.
///
/// The per-kind split is the headline data for the coarse-per-realization vs
/// fine-per-cell question: `realization` vs `value`/`constraint` totals say
/// whether pruning whole realizations captures most of the win.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WouldPruneByKind {
    /// `NodeId::Value` nodes that would be pruned.
    pub value: usize,
    /// `NodeId::Constraint` nodes that would be pruned.
    pub constraint: usize,
    /// `NodeId::Realization` nodes that would be pruned.
    pub realization: usize,
    /// `NodeId::Resolution` nodes that would be pruned.
    pub resolution: usize,
    /// `NodeId::Compute` nodes that would be pruned.
    pub compute: usize,
}

impl WouldPruneByKind {
    /// Total count of would-prune nodes across all kinds.
    pub fn total(&self) -> usize {
        self.value + self.constraint + self.realization + self.resolution + self.compute
    }
}

/// Engine-level observed-demand side-channel (task 4532).
///
/// These methods operate EXCLUSIVELY on `self.observed_demand` — they never
/// touch the production `self.demand` registry and the observed cone is never
/// passed to `compute_eval_set`. This is the structural guarantee behind the
/// zero-behavior-change contract: registering observed demand cannot perturb
/// `EvalResult` / `last_eval_set`. See `docs/prds/v0_6/selective-demand.md` §G6.
impl Engine {
    /// Register `node` as an observed-demand root (e.g. a viewport-visible
    /// realization, a displayed property cell, a panel constraint). Call
    /// [`Engine::rebuild_observed_cone`] afterward to refresh the cone.
    pub fn add_observed_demand(&mut self, node: NodeId) {
        self.observed_demand.add_demand(node);
    }

    /// Remove `node` from the observed-demand roots. Call
    /// [`Engine::rebuild_observed_cone`] afterward to refresh the cone.
    pub fn remove_observed_demand(&mut self, node: &NodeId) {
        self.observed_demand.remove_demand(node);
    }

    /// Rebuild the observed-demand cone against the current snapshot graph.
    ///
    /// No-op when there is no eval state (no `eval()` has run yet). Mirrors the
    /// production cone-rebuild in
    /// [`Engine::rebuild_purpose_infrastructure`](crate::Engine) but rebuilds
    /// the OBSERVED registry — never `self.demand`.
    pub fn rebuild_observed_cone(&mut self) {
        if let Some(state) = self.eval_state.as_ref() {
            self.observed_demand.rebuild_cone(&state.snapshot.graph);
        }
    }

    /// Clear all observed-demand roots and the observed cone.
    pub fn reset_observed_demand(&mut self) {
        self.observed_demand = DemandRegistry::new();
    }

    /// Whether `node` is in the observed-demand cone (post-rebuild). Inspection
    /// only — has no effect on evaluation.
    pub fn observed_demand_is_demanded(&self, node: &NodeId) -> bool {
        self.observed_demand.is_demanded(node)
    }

    /// Size of the observed-demand cone (post-rebuild). Inspection only.
    pub fn observed_demand_cone_size(&self) -> usize {
        self.observed_demand.cone_size()
    }
}
