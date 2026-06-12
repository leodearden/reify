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
