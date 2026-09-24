//! The ENGINE lane's single front door: every engine-touching request is
//! queued here and run one at a time.
//!
//! Edits coalesce, per the esc-5215-5 ruling that the newest edit supersedes
//! queued ones: a newly admitted edit makes every QUEUED older edit of the same
//! target redundant, and an edit arriving after a newer one of its target was
//! already admitted is dropped. Two rules decide "older": the newest edit wins
//! within a target, and a durable write never yields to a transient one.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::diff::{StateDelta, advance_baseline};
use crate::types::GuiState;

/// Where the frontend stamped an edit: `seq` increases with every edit of one
/// page load, and `epoch` identifies the page load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditOrder {
    pub epoch: u64,
    pub seq: u64,
}

impl EditOrder {
    /// Stamped after `other`, or in another page load — a reload restarts
    /// `seq`, so across epochs the arriving edit counts as the later one.
    fn is_after(self, other: Self) -> bool {
        self.epoch != other.epoch || self.seq > other.seq
    }

    /// Stamped before `other` in the same page load.
    fn is_before(self, other: Self) -> bool {
        self.epoch == other.epoch && self.seq < other.seq
    }
}

/// What an edit changes. Only edits of one target coalesce.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EditTarget {
    Parameter(String),
    EditorSource(PathBuf),
    DiskSource(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum EditStrength {
    Transient,
    Durable,
}

/// The identity by which an edit is coalesced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditIdentity {
    target: EditTarget,
    strength: EditStrength,
    order: Option<EditOrder>,
}

impl EditIdentity {
    /// A slider-drag frame: a transient parameter override.
    pub fn preview(cell_id: impl Into<String>, order: EditOrder) -> Self {
        Self::stamped(
            EditTarget::Parameter(cell_id.into()),
            EditStrength::Transient,
            order,
        )
    }

    /// A durable parameter write back into the `.ri` source.
    pub fn commit(cell_id: impl Into<String>, order: EditOrder) -> Self {
        Self::stamped(
            EditTarget::Parameter(cell_id.into()),
            EditStrength::Durable,
            order,
        )
    }

    /// A sync of the editor's buffer for `path`, which replaces the source
    /// whole.
    pub fn editor_source(path: impl Into<PathBuf>, order: EditOrder) -> Self {
        Self::stamped(
            EditTarget::EditorSource(path.into()),
            EditStrength::Durable,
            order,
        )
    }

    /// A watcher reload of `path`. Unstamped: the reload reads the file when it
    /// runs, so the latest arrival is the one worth running.
    pub fn disk_source(path: impl Into<PathBuf>) -> Self {
        Self {
            target: EditTarget::DiskSource(path.into()),
            strength: EditStrength::Durable,
            order: None,
        }
    }

    fn stamped(target: EditTarget, strength: EditStrength, order: EditOrder) -> Self {
        Self {
            target,
            strength,
            order: Some(order),
        }
    }

    /// Whether this arriving edit makes the queued `older` one redundant.
    pub fn supersedes(&self, older: &EditIdentity) -> bool {
        self.target == older.target
            && self.strength >= older.strength
            && match (self.order, older.order) {
                (Some(newer), Some(older)) => newer.is_after(older),
                _ => true,
            }
    }
}

/// The per-target high-water marks of admitted edits.
#[derive(Debug, Default)]
pub struct EditLedger {
    marks: HashMap<EditTarget, BTreeMap<EditStrength, EditOrder>>,
}

impl EditLedger {
    /// Admit `identity` and record its stamp, or refuse it — recording nothing —
    /// when an edit of its target that is at least as strong and stamped later
    /// in the same page load was already admitted.
    pub fn admit(&mut self, identity: &EditIdentity) -> bool {
        let Some(order) = identity.order else {
            return true;
        };
        let marks = self.marks.entry(identity.target.clone()).or_default();
        let late = marks
            .range(identity.strength..)
            .any(|(_, &mark)| order.is_before(mark));
        if !late {
            marks.insert(identity.strength, order);
        }
        !late
    }
}

/// Whether the queue has edits or evaluations in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalActivity {
    Evaluating,
    Idle,
}

/// What the frontend is told about evaluation.
pub trait EvalObserver: Send + Sync {
    fn activity(&self, activity: EvalActivity);
    fn delta(&self, delta: &StateDelta);
}

/// The one place evaluation results become frontend-visible deltas.
///
/// The baseline is the same `Arc` the debug server diffs against (INV-GUI-2):
/// otherwise a debug-driven mutation would advance the engine without the
/// deltas published here ever accounting for it.
pub struct SnapshotPublisher {
    baseline: Arc<Mutex<Option<GuiState>>>,
    observer: Arc<dyn EvalObserver>,
    last_generation: Mutex<Option<u64>>,
}

impl SnapshotPublisher {
    pub fn new(baseline: Arc<Mutex<Option<GuiState>>>, observer: Arc<dyn EvalObserver>) -> Self {
        Self {
            baseline,
            observer,
            last_generation: Mutex::new(None),
        }
    }

    /// Advance the baseline to `state` and report the delta — or, when
    /// `generation` is not newer than the last one published, touch nothing and
    /// return `false`, so deltas never go backwards.
    pub fn publish(&self, generation: u64, state: GuiState) -> bool {
        let mut last = self
            .last_generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(last) = *last
            && generation <= last
        {
            tracing::warn!(
                "refused to publish evaluation generation {generation}: generation {last} \
                 was already published"
            );
            return false;
        }
        *last = Some(generation);
        let delta = advance_baseline(&self.baseline, state);
        self.observer.delta(&delta);
        true
    }
}
