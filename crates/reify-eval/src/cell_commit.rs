//! Per-cell eval commit substrate (task α, #5038).
//!
//! PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.1–2.4, INV-EVAL-1.
//!
//! Defines [`commit_cell_result`], the primitive that performs the four legs
//! of a per-cell eval commit (values, snapshot, cache, journal) atomically —
//! no call path can write a subset of the legs by omission — plus the three
//! enums that make today's implicit choices explicit and typed:
//! [`DeterminacyRule`], [`TraceSource`], and [`CacheLeg`].
//!
//! This module introduces the primitive and its unit tests only. Migrating
//! existing call sites (`engine_eval.rs`, `engine_edit.rs`, ...) onto this
//! primitive is out of scope here — see PRD leaves γ/δ/ε/ι.

use std::time::Instant;

use reify_core::{ValueCellId, VersionId};
use reify_ir::{DeterminacyState, PersistentMap, Value, ValueMap};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::deps::DependencyTrace;
use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};

/// The rule that determines a committed cell's [`DeterminacyState`] from its
/// evaluated [`Value`]. Encodes today's three implicit determinacy rules as
/// explicit, typed variants so a future edit cannot silently erase the
/// intentional divergence between them (INV-EVAL-1).
#[allow(dead_code)] // constructed by migration call sites from leaves γ/δ/ε/ι; tests only until then
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeterminacyRule {
    /// Main-pass let/param binds: stamps `Determined` unconditionally,
    /// regardless of whether the evaluated value is `Value::Undef`. See
    /// `evaluate_params_and_lets_unified` / the `group.members` arm of
    /// `Engine::eval` (engine_eval.rs ~:300).
    UnconditionalDetermined,
    /// The `reeval_cone_cell` rule (engine_eval.rs:4934-4937) and the
    /// `DeterminacyPredicate` family (engine_eval.rs:4317,4356): maps
    /// `Value::Undef` to `Undetermined`, everything else to `Determined`.
    /// Intentionally diverges from `UnconditionalDetermined` — the doc
    /// comment at engine_eval.rs:4883-4897 states a future reader should NOT
    /// collapse the two rules together.
    DeriveFromValue,
    /// Rejected-override-with-no-default, or a solver-owned Auto cell still
    /// awaiting a solve: always `Undetermined`, regardless of value.
    Undetermined,
}

impl DeterminacyRule {
    /// Resolves the [`DeterminacyState`] to record for `value` under this
    /// rule. This is the single site encoding all three rules; migration
    /// call sites select a rule and never re-derive a `DeterminacyState`
    /// inline.
    pub fn resolve(self, value: &Value) -> DeterminacyState {
        match self {
            DeterminacyRule::UnconditionalDetermined => DeterminacyState::Determined,
            DeterminacyRule::DeriveFromValue => match value {
                Value::Undef => DeterminacyState::Undetermined,
                _ => DeterminacyState::Determined,
            },
            DeterminacyRule::Undetermined => DeterminacyState::Undetermined,
        }
    }
}

/// Provenance tag recorded on a commit's journal `Started` event: which
/// call-path produced this value. Makes the journal self-describing so the
/// §2.6 divergence audit (a later leaf) can attribute a mismatch to its
/// producing path.
#[allow(dead_code)] // variants other than ColdEval used from step-7 onwards
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSource {
    /// First-time (cold) evaluation of a node with no prior cache entry.
    ColdEval,
    /// Result served from an existing fresh cache entry.
    CachedServe,
    /// Re-evaluation triggered by an edit to an upstream cell.
    EditReeval,
    /// Part of a guarded-group re-evaluation (the `GuardedParamCtx` family).
    GuardedGroup,
    /// Overwrite applied after the main eval pass (post-pass correction).
    PostPassOverwrite,
    /// The `reeval_cone_cell` re-propagation pass.
    ConeReeval,
}

/// Whether a commit writes the cache leg, and if not, why.
///
/// `Record` is a unit variant (not value-carrying) — the cache leg's
/// `DependencyTrace` rides as a separate `commit_cell_result` parameter,
/// used only on that arm.
#[allow(dead_code)] // constructed by migration call sites from leaves γ/δ/ε/ι; tests only until then
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLeg {
    /// Write the cache leg via [`CacheStore::record_evaluation`].
    Record,
    /// Omit the cache leg. Carries the reason, surfaced on
    /// [`CommitOutcome::skip_reason`] for later divergence-audit exemptions.
    Skip(&'static str),
}

/// Outcome of a [`commit_cell_result`] call.
///
/// Carries the `(value, determinacy)` pair so callers that previously read
/// back the inserted tuple keep working, plus the cache/skip/provenance
/// metadata the four-leg commit produced.
#[allow(dead_code)] // fields read by tests only until migration leaves γ/δ/ε/ι land
#[derive(Debug, Clone)]
pub struct CommitOutcome {
    value: Value,
    determinacy: DeterminacyState,
    /// `Some` on `CacheLeg::Record` (forwarded from `record_evaluation`),
    /// `None` on `CacheLeg::Skip` — the authoritative signal that nothing
    /// was cached.
    cache_outcome: Option<EvalOutcome>,
    /// `Some(reason)` on `CacheLeg::Skip`, `None` on `CacheLeg::Record`.
    skip_reason: Option<&'static str>,
    trace_source: TraceSource,
}

/// The four `&mut` leg targets a commit writes to, bundled as disjoint
/// borrows — mirrors the `GuardedParamCtx` struct-bundle pattern
/// (engine_eval.rs:336). `values`/`snapshot_values` are eval-pass locals
/// threaded by migration call sites; `cache`/`journal` are Engine fields.
pub(crate) struct CommitLegs<'a> {
    values: &'a mut ValueMap,
    snapshot_values: &'a mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    cache: &'a mut CacheStore,
    journal: &'a mut EventJournal,
}

/// Performs the four legs of a per-cell eval commit — values, snapshot,
/// cache, journal — atomically: no call path can write a subset of the legs
/// by omission (INV-EVAL-1).
///
/// Emits the full journal `Started`/`Completed` `EvalEvent` pair, subsuming
/// the `record_eval_completed` helper (engine_eval.rs:369), which today emits
/// only `Completed`.
#[allow(dead_code)] // wired in by migration leaves γ/δ/ε/ι; exercised by tests until then
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_cell_result(
    legs: CommitLegs<'_>,
    node: ValueCellId,
    value: Value,
    determinacy: DeterminacyRule,
    trace: TraceSource,
    dependency_trace: DependencyTrace,
    version: VersionId,
    cache_leg: CacheLeg,
) -> CommitOutcome {
    let CommitLegs {
        values,
        snapshot_values,
        cache,
        journal,
    } = legs;

    let det = determinacy.resolve(&value);
    let node_id = NodeId::Value(node.clone());
    let start = Instant::now();

    journal.record(EvalEvent {
        timestamp: start,
        node_id: node_id.clone(),
        kind: EventKind::Started,
        version,
        payload: None,
    });

    values.insert(node.clone(), value.clone());
    snapshot_values.insert(node, (value.clone(), det));

    let (cache_outcome, skip_reason) = match cache_leg {
        CacheLeg::Record => {
            let outcome = cache.record_evaluation(
                node_id.clone(),
                CachedResult::Value(value.clone(), det),
                version,
                dependency_trace,
            );
            (Some(outcome), None)
        }
        CacheLeg::Skip(_) => unimplemented!("CacheLeg::Skip — #5038"),
    };

    journal.record(EvalEvent {
        timestamp: Instant::now(),
        node_id,
        kind: EventKind::Completed {
            outcome: cache_outcome.unwrap_or(EvalOutcome::Changed),
        },
        version,
        payload: Some(EventPayload::Duration(start.elapsed())),
    });

    CommitOutcome {
        value,
        determinacy: det,
        cache_outcome,
        skip_reason,
        trace_source: trace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, PersistentMap, Value, ValueMap};

    use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
    use crate::deps::DependencyTrace;
    use crate::journal::{EventJournal, EventKind};

    /// Pins INV-EVAL-1's "divergence encoded, not erased" invariant: the
    /// recorded `DeterminacyState` is driven by which `DeterminacyRule` was
    /// selected, not merely by the shape of `value`. In particular
    /// `DeriveFromValue` (the `reeval_cone_cell` rule) must diverge from
    /// `UnconditionalDetermined` (the main-pass let/param rule) on
    /// `Value::Undef`, while `Undetermined` (solver-owned / rejected-override)
    /// ignores the value entirely.
    #[test]
    fn determinacy_rule_resolve_encodes_all_three_rules() {
        // UnconditionalDetermined: always Determined, regardless of value.
        assert_eq!(
            DeterminacyRule::UnconditionalDetermined.resolve(&Value::Undef),
            DeterminacyState::Determined
        );
        assert_eq!(
            DeterminacyRule::UnconditionalDetermined.resolve(&Value::Bool(true)),
            DeterminacyState::Determined
        );

        // Undetermined: always Undetermined, regardless of value.
        assert_eq!(
            DeterminacyRule::Undetermined.resolve(&Value::Bool(true)),
            DeterminacyState::Undetermined
        );
        assert_eq!(
            DeterminacyRule::Undetermined.resolve(&Value::Undef),
            DeterminacyState::Undetermined
        );

        // DeriveFromValue (reeval_cone_cell rule, engine_eval.rs:4934-4937):
        // Undef -> Undetermined, else -> Determined. This is the rule that
        // must NOT be collapsed into UnconditionalDetermined's behaviour.
        assert_eq!(
            DeterminacyRule::DeriveFromValue.resolve(&Value::Undef),
            DeterminacyState::Undetermined
        );
        assert_eq!(
            DeterminacyRule::DeriveFromValue.resolve(&Value::Bool(true)),
            DeterminacyState::Determined
        );
    }

    /// Pins the atomic 4-leg commit on `CacheLeg::Record`: a single
    /// `commit_cell_result` call must write values, snapshot, cache, AND
    /// journal — no leg may be silently skipped when the caller asks for a
    /// full record (INV-EVAL-1: no path writes a subset by omission).
    #[test]
    fn commit_record_writes_all_four_legs() {
        let mut values = ValueMap::new();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut cache = CacheStore::new();
        let mut journal = EventJournal::new();
        let node = ValueCellId::new("Body", "w");

        let outcome = commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node.clone(),
            Value::Bool(true),
            DeterminacyRule::DeriveFromValue,
            TraceSource::ColdEval,
            DependencyTrace::default(),
            VersionId(1),
            CacheLeg::Record,
        );

        // values leg
        assert_eq!(values.get(&node), Some(&Value::Bool(true)));

        // snapshot leg
        assert_eq!(
            snapshot_values.get(&node),
            Some(&(Value::Bool(true), DeterminacyState::Determined))
        );

        // cache leg
        let node_id = NodeId::Value(node.clone());
        let entry = cache
            .get(&node_id)
            .expect("CacheLeg::Record must write a cache entry");
        match &entry.result {
            CachedResult::Value(v, d) => {
                assert_eq!(*v, Value::Bool(true));
                assert_eq!(*d, DeterminacyState::Determined);
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }
        assert_eq!(entry.basis_version, VersionId(1));

        // journal leg: Started then Completed recorded for this node.
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            2,
            "expected exactly Started + Completed, got {events:?}"
        );
        assert!(matches!(events[0].kind, EventKind::Started));
        assert!(matches!(events[1].kind, EventKind::Completed { .. }));

        // Returned CommitOutcome mirrors what was written to all four legs.
        assert_eq!(outcome.value, Value::Bool(true));
        assert_eq!(outcome.determinacy, DeterminacyState::Determined);
        assert_eq!(outcome.cache_outcome, Some(EvalOutcome::Changed));
        assert_eq!(outcome.skip_reason, None);
        assert_eq!(outcome.trace_source, TraceSource::ColdEval);
    }
}
