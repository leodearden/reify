//! Per-cell eval commit substrate (task α, #5038).
//!
//! PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.1–2.4, INV-EVAL-1.
//!
//! Defines [`commit_cell_result`], the primitive that performs the four legs
//! of a per-cell eval commit (values, snapshot, cache, journal) atomically —
//! no call path can write a subset of the legs by omission — plus the enums
//! that make today's implicit choices explicit and typed: [`DeterminacyRule`]
//! and [`CacheLeg`]. The commit records its [`TraceSource`] and the cache
//! leg's fate on the journal as a [`CommitTag`].
//!
//! Call sites in `engine_eval.rs`, `engine_edit.rs` and `unfold.rs` commit
//! through this primitive. Not every per-cell commit does yet: a hand-rolled
//! commit journals an untagged `Started { commit: None }` event.
//!
//! ## Known scope gaps for migration leaves (γ/δ/ε/ι)
//!
//! The freshness gap below is now **CLOSED** (task #5238), and so is the
//! former `Skip`-outcome placeholder (task #8859: `Completed.outcome` is
//! `None` on a Skip commit and the leg rides on [`CommitTag`]). Two gaps
//! remain OPEN — the determinacy dimension and the failure-path commit
//! shape. Each is deliberately left as a documented limitation rather than
//! closed by widening this module's enums or by changing `journal.rs`'s
//! `EventKind` shape. A migration leaf that hits one
//! of them must address it explicitly (extend the enum, or document why the
//! site is left unmigrated) — not silently paper over it by routing through
//! the existing variants as-is.
//!
//! - **Freshness dimension — CLOSED (task #5238).** [`CacheLeg::Record`]
//!   still writes `Freshness::Final`, but [`CacheLeg::RecordPropagating`] now
//!   routes to `CacheStore::record_evaluation_propagating_freshness` (deriving
//!   the output freshness from the just-computed trace, arch §7.2) and
//!   [`CacheLeg::RecordWithFreshness`] routes to
//!   `CacheStore::record_evaluation_with_freshness` (writing an explicit
//!   caller-supplied freshness). The dominant let/param commit sites in
//!   `engine_eval.rs` (`evaluate_params_and_lets_unified`,
//!   `evaluate_let_bindings`) and the `eval_cached` Param/Let
//!   preserve-freshness re-serves are migrated onto these variants; the
//!   latter two stamp [`TraceSource::CachedReuse`], distinct from the
//!   cache-MISS arm's [`TraceSource::CachedServe`], so the journal alone
//!   separates a re-serve from a miss.
//! - **Determinacy dimension — OPEN (discovered by task #5238).**
//!   [`DeterminacyRule::resolve`] yields only `DeterminacyState::Determined`
//!   or `Undetermined` — never `Auto` or `Provisional`. So a commit site that
//!   must PRESERVE a stored `Auto` cannot be expressed through this
//!   primitive at all, whichever [`CacheLeg`] it picks. That is what leaves
//!   `Engine::eval_cached`'s **Auto-cell pre-seed re-serve** (`engine_eval.rs`;
//!   the `cell.kind.is_auto()` cache-reuse block) unmigrated while its Param
//!   and Let cache-reuse re-serve siblings in the same function are migrated:
//!   the Auto pre-seed writes `(Value::Undef, Auto)`, and re-serving it
//!   through a `DeterminacyRule` would silently rewrite that `Auto` to
//!   `Determined`/`Undetermined`. [`DeterminacyRule::preserving`] names
//!   exactly this boundary — it returns `None` for `Auto`/`Provisional`, and
//!   the two migrated re-serves degrade to a direct write on `None` rather
//!   than assuming unreachability. A future leaf closing this gap needs a
//!   determinacy-PRESERVING `DeterminacyRule` VARIANT (carry the stored state
//!   through `resolve` verbatim rather than resolving from the value);
//!   deliberately out of #5238's freshness scope, since adding it would
//!   smuggle an untested determinacy change into a freshness task. Both the
//!   unmigrated Auto pre-seed re-serve and the two migrated re-serves' `None`
//!   fallback share ONE direct-write body —
//!   `engine_eval.rs`'s `reserve_preserving_determinacy_direct` — so closing
//!   this gap is a single-site change and, until then, no second hand-synced
//!   copy of the four-leg shape exists to drift.
//! - **Failure-path commit shape — OPEN (by design; discovered by task
//!   #5238).** [`commit_cell_result`] always writes the values and snapshot
//!   legs and always emits a `Started`/`Completed` pair. The four propagating
//!   failure-path writes in `engine_eval.rs` — the compute-dispatch `Failed`
//!   path and the panic-recovery path, one of each in
//!   `Engine::evaluate_params_and_lets_unified`'s Let arm and in
//!   `Engine::evaluate_let_bindings` — have the opposite shape: they journal `EventKind::Failed` (no pair), write
//!   NEITHER the values nor the snapshot leg, and follow the cache write with
//!   `mark_failed`. No freshness fidelity is lost by leaving them direct —
//!   `mark_failed` immediately overwrites the just-propagated freshness with
//!   `Freshness::Failed { error }` — so these are documented-as-unmigrated
//!   rather than blocked. A future leaf would need a failure-shaped commit
//!   entry point (or a `CommitLegs` whose values/snapshot legs are optional),
//!   which is a shape change to the primitive, not a new enum variant.

use std::time::Instant;

use reify_core::{ValueCellId, VersionId};
use reify_ir::{DeterminacyState, PersistentMap, Value, ValueMap};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::deps::DependencyTrace;
use crate::journal::{
    CacheLegKind, CommitTag, EvalEvent, EventJournal, EventKind, EventPayload, TraceSource,
};

/// The rule that determines a committed cell's [`DeterminacyState`] from its
/// evaluated [`Value`]. Encodes today's three implicit determinacy rules as
/// explicit, typed variants so a future edit cannot silently erase the
/// intentional divergence between them (INV-EVAL-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeterminacyRule {
    /// Main-pass let/param binds: stamps `Determined` unconditionally,
    /// regardless of whether the evaluated value is `Value::Undef`. See
    /// `evaluate_params_and_lets_unified` / the `group.members` arm of
    /// `Engine::eval` in `engine_eval.rs`.
    UnconditionalDetermined,
    /// The `reeval_cone_cell` rule and the `DeterminacyPredicate` family in
    /// `engine_eval.rs`: maps `Value::Undef` to `Undetermined`, everything
    /// else to `Determined`. Intentionally diverges from
    /// `UnconditionalDetermined` — `reeval_cone_cell`'s own doc comment
    /// states a future reader should NOT collapse the two rules together.
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

    /// Selects the rule that REPRODUCES an already-stored `det` exactly, for a
    /// commit site that must re-serve a cached `(value, determinacy)` pair
    /// verbatim rather than derive a fresh determinacy from the value.
    ///
    /// Both returned rules resolve value-INDEPENDENTLY
    /// ([`DeterminacyRule::UnconditionalDetermined`] always yields
    /// `Determined`, [`DeterminacyRule::Undetermined`] always yields
    /// `Undetermined`), so `preserving(det).unwrap().resolve(v) == det` for
    /// every `v`. That exactness is load-bearing at the `eval_cached`
    /// preserve-freshness re-serves: reproducing the stored pair byte-for-byte
    /// keeps `CacheStore::record_evaluation_with_freshness` on its
    /// content-hash EARLY-CUTOFF branch, which is the only branch that
    /// preserves the entry's `pending_cause` and `diagnostics` (the Changed
    /// branch resets both) — see `cache.rs`.
    ///
    /// Returns `None` for `Auto` and `Provisional`: no [`DeterminacyRule`]
    /// yields either, so those states are NOT expressible through
    /// [`commit_cell_result`] at all. This is the single site naming that
    /// caveat — see the "Determinacy dimension — OPEN" bullet in this module's
    /// doc. Callers must degrade gracefully on `None` (write the stored state
    /// through directly), never assume unreachability.
    pub fn preserving(det: DeterminacyState) -> Option<Self> {
        match det {
            DeterminacyState::Determined => Some(DeterminacyRule::UnconditionalDetermined),
            DeterminacyState::Undetermined => Some(DeterminacyRule::Undetermined),
            DeterminacyState::Auto | DeterminacyState::Provisional => None,
        }
    }
}

/// Whether a commit writes the cache leg, how, and if not, why.
///
/// The cache-writing variants (`Record`, `RecordPropagating`) do not carry
/// the `DependencyTrace` — it rides as a separate `commit_cell_result`
/// parameter, consumed by whichever cache-writing arm runs.
///
/// **Freshness routing:** `Record` writes `Freshness::Final`;
/// [`CacheLeg::RecordPropagating`] DERIVES freshness from the just-computed
/// trace (arch §7.2); [`CacheLeg::RecordWithFreshness`] writes an EXPLICIT
/// caller-supplied freshness. Because `reify_ir::Freshness` is not `Copy`,
/// this enum is `Clone` but deliberately NOT `Copy` — `commit_cell_result`
/// borrows the leg to build the `Started` event's [`CommitTag`] and consumes
/// it for the cache-write match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheLeg {
    /// Write the cache leg via [`CacheStore::record_evaluation`], which is a
    /// thin wrapper that hard-codes `Freshness::Final` (see its own doc
    /// comment in `cache.rs`). For a commit site that needs a non-Final
    /// freshness, use [`CacheLeg::RecordPropagating`] (derive from trace) or
    /// [`CacheLeg::RecordWithFreshness`] (explicit) instead.
    Record,
    /// Write the cache leg via
    /// [`CacheStore::record_evaluation_propagating_freshness`], which DERIVES
    /// the output freshness from the just-computed `dependency_trace` (arch
    /// §7.2): `still_refining: true` short-circuits to `Freshness::Intermediate`,
    /// otherwise freshness is propagated from the reads (an all-`Final` trace
    /// yields `Final`). Used by the main let/param evaluators in
    /// `engine_eval.rs` (`evaluate_params_and_lets_unified`,
    /// `evaluate_let_bindings`).
    RecordPropagating {
        /// Forwarded to `record_evaluation_propagating_freshness`: `true`
        /// marks the just-computed result as still-refining (→ `Intermediate`).
        still_refining: bool,
    },
    /// Write the cache leg via
    /// [`CacheStore::record_evaluation_with_freshness`], writing the supplied
    /// [`reify_ir::Freshness`] verbatim. Used by the `eval_cached`
    /// preserve-freshness re-serves, which carry the cached entry's own
    /// freshness forward unchanged.
    RecordWithFreshness(reify_ir::Freshness),
    /// Omit the cache leg. Carries the reason, recorded on the journal as
    /// [`CacheLegKind::Skipped`] for the divergence audit's exemption.
    Skip(&'static str),
}

impl CacheLeg {
    /// The journal's record of this leg: whether it writes the cache, and
    /// if not, why.
    fn kind(&self) -> CacheLegKind {
        match *self {
            CacheLeg::Record
            | CacheLeg::RecordPropagating { .. }
            | CacheLeg::RecordWithFreshness(_) => CacheLegKind::Recorded,
            CacheLeg::Skip(reason) => CacheLegKind::Skipped { reason },
        }
    }
}

/// The four `&mut` leg targets a commit writes to, bundled as disjoint
/// borrows — mirrors the `GuardedParamCtx` struct-bundle pattern in
/// `engine_eval.rs`. `values`/`snapshot_values` are eval-pass locals
/// threaded by migration call sites; `cache`/`journal` are Engine fields.
pub(crate) struct CommitLegs<'a> {
    pub(crate) values: &'a mut ValueMap,
    pub(crate) snapshot_values: &'a mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    pub(crate) cache: &'a mut CacheStore,
    pub(crate) journal: &'a mut EventJournal,
}

/// Performs the four legs of a per-cell eval commit — values, snapshot,
/// cache, journal — atomically: no call path can write a subset of the legs
/// by omission (INV-EVAL-1).
///
/// Emits the full journal `Started`/`Completed` `EvalEvent` pair, subsuming
/// the `record_eval_completed` helper in `engine_eval.rs`, which today emits
/// only `Completed`. The `Started` event carries a [`CommitTag`] naming the
/// trace and the cache leg's fate, so the journal alone tells a genuine cache
/// write apart from a skip.
///
/// Returns the cache outcome — the same value the `Completed` event carries:
/// `None` exactly when `cache_leg` is [`CacheLeg::Skip`].
///
/// The emitted `Started`/`Completed` pair brackets the COMMIT only: `Started`
/// is timestamped at entry and `Completed`'s `EventPayload::Duration` measures
/// from it. A caller that already captured an `Instant` before the work this
/// commit concludes should call [`commit_cell_result_at`] with that instant
/// instead, so the pair brackets the real evaluation — see its doc for why
/// that matters.
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
) -> Option<EvalOutcome> {
    commit_cell_result_at(
        Instant::now(),
        legs,
        node,
        value,
        determinacy,
        trace,
        dependency_trace,
        version,
        cache_leg,
    )
}

/// [`commit_cell_result`], but with the journal `Started` event's timestamp —
/// and therefore the `Completed` event's `EventPayload::Duration` span —
/// supplied by the caller rather than captured at commit entry.
///
/// task #5238 amendment. `engine_eval.rs`'s `record_eval_completed` documents
/// the house convention: "`start` is the `Instant` captured before the matching
/// `EventKind::Started` record so that `Duration` spans the full resolution".
/// The main let commits in `evaluate_params_and_lets_unified` and
/// `evaluate_let_bindings` honoured that before being migrated onto this
/// primitive — their `Completed` carried `start.elapsed()` measured from the
/// top of the loop body, i.e. the whole evaluation. Routing them through
/// [`commit_cell_result`], which captures its own `Instant`, silently narrowed
/// those Durations to the commit's own (sub-microsecond) cost, while every
/// non-migrated sibling sub-path in the SAME loop kept the full-resolution
/// span — so one pass's journal would have mixed two incompatible `Duration`
/// semantics per cell. No consumer reads a `Duration`'s MAGNITUDE today —
/// every one existence-matches `EventPayload::Duration(_)` (the same
/// "presence only" wording `engine_eval.rs`'s own migration notes use) —
/// which is why nothing failed; a future profiler reading the journal would
/// simply have got wrong numbers for exactly the dominant cells.
///
/// Passing `started_at` also back-dates the `Started` event's own timestamp to
/// the same instant, matching `record_subpath_started`'s behaviour, so the pair
/// really does bracket the work rather than merely reporting its length.
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_cell_result_at(
    started_at: Instant,
    legs: CommitLegs<'_>,
    node: ValueCellId,
    value: Value,
    determinacy: DeterminacyRule,
    trace: TraceSource,
    dependency_trace: DependencyTrace,
    version: VersionId,
    cache_leg: CacheLeg,
) -> Option<EvalOutcome> {
    let CommitLegs {
        values,
        snapshot_values,
        cache,
        journal,
    } = legs;

    let det = determinacy.resolve(&value);
    let node_id = NodeId::Value(node.clone());
    // Supplied by the caller (see this fn's doc): the `Started` event is
    // stamped with it and `Completed`'s Duration measures from it, so the pair
    // brackets the caller's work, not just this commit.
    let start = started_at;

    journal.record(EvalEvent {
        timestamp: start,
        node_id: node_id.clone(),
        kind: EventKind::Started {
            commit: Some(CommitTag {
                trace,
                cache: cache_leg.kind(),
            }),
        },
        version,
        payload: None,
    });

    // A cache-writing commit needs exactly 3 independently-owned copies of
    // `value` (values leg, snapshot leg, cache leg). `Value` is not `Copy` and
    // is not `Arc`-backed, so each extra owner costs a real deep clone. The two
    // clones below cover values+snapshot; the ORIGINAL `value` is then MOVED
    // into whichever cache-writing arm runs, so no third clone is made.
    // `CacheLeg::Skip` needs only 2 copies and simply drops the original. That
    // is the minimum possible for however many legs actually write; reordering
    // cannot reduce it further (some owner must always "spend" the original via
    // move, and one is all that's available).
    values.insert(node.clone(), value.clone());
    snapshot_values.insert(node, (value.clone(), det));

    let cache_outcome = match cache_leg {
        CacheLeg::Record => {
            let outcome = cache.record_evaluation(
                node_id.clone(),
                CachedResult::Value(value, det),
                version,
                dependency_trace,
            );
            Some(outcome)
        }
        CacheLeg::RecordPropagating { still_refining } => {
            let outcome = cache.record_evaluation_propagating_freshness(
                node_id.clone(),
                CachedResult::Value(value, det),
                version,
                dependency_trace,
                still_refining,
            );
            Some(outcome)
        }
        CacheLeg::RecordWithFreshness(freshness) => {
            let outcome = cache.record_evaluation_with_freshness(
                node_id.clone(),
                CachedResult::Value(value, det),
                version,
                dependency_trace,
                freshness,
            );
            Some(outcome)
        }
        CacheLeg::Skip(_) => None,
    };

    journal.record(EvalEvent {
        timestamp: Instant::now(),
        node_id,
        kind: EventKind::Completed {
            outcome: cache_outcome,
        },
        version,
        payload: Some(EventPayload::Duration(start.elapsed())),
    });

    cache_outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, ErrorRef, Freshness, PersistentMap, Value, ValueMap};

    use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
    use crate::deps::DependencyTrace;
    use crate::journal::{
        CacheLegKind, CommitTag, EvalEvent, EventJournal, EventKind, EventPayload, TraceSource,
    };

    /// The commit tag a journal `Started` event carries.
    fn started_commit(event: &EvalEvent) -> Option<CommitTag> {
        let EventKind::Started { commit } = &event.kind else {
            panic!("expected EventKind::Started, got {:?}", event.kind);
        };
        commit.clone()
    }

    /// The cache outcome a journal `Completed` event carries.
    fn completed_outcome(event: &EvalEvent) -> Option<EvalOutcome> {
        let EventKind::Completed { outcome } = &event.kind else {
            panic!("expected EventKind::Completed, got {:?}", event.kind);
        };
        *outcome
    }

    fn recorded(trace: TraceSource) -> Option<CommitTag> {
        Some(CommitTag {
            trace,
            cache: CacheLegKind::Recorded,
        })
    }

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

        // DeriveFromValue (reeval_cone_cell rule in engine_eval.rs):
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
        assert_eq!(started_commit(events[0]), recorded(TraceSource::ColdEval));
        // The tag is the only carrier of trace and cache leg.
        assert!(events[0].payload.is_none(), "got {:?}", events[0].payload);
        assert_eq!(completed_outcome(events[1]), Some(EvalOutcome::Changed));

        // The returned cache outcome is the one the journal's Completed carries.
        assert_eq!(outcome, Some(EvalOutcome::Changed));
    }

    /// Extends Record-path coverage to `DeterminacyRule::Undetermined`
    /// (rejected-override / solver-owned Auto cells awaiting a solve): the
    /// end-to-end commit must land `Undetermined` in BOTH the snapshot leg's
    /// tuple AND the cache leg's `CachedResult::Value`, not merely at the
    /// `DeterminacyRule::resolve` unit level. The value itself (`Bool(true)`)
    /// is not `Value::Undef`, proving once more that the RULE — not the
    /// value's shape — drives the recorded determinacy.
    #[test]
    fn commit_record_with_undetermined_rule_lands_undetermined_in_snapshot_and_cache() {
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
            DeterminacyRule::Undetermined,
            TraceSource::GuardedGroup,
            DependencyTrace::default(),
            VersionId(1),
            CacheLeg::Record,
        );

        // snapshot leg: Undetermined despite a non-Undef value.
        assert_eq!(
            snapshot_values.get(&node),
            Some(&(Value::Bool(true), DeterminacyState::Undetermined))
        );

        // cache leg: same Undetermined determinacy recorded in CachedResult.
        let node_id = NodeId::Value(node.clone());
        let entry = cache
            .get(&node_id)
            .expect("CacheLeg::Record must write a cache entry");
        match &entry.result {
            CachedResult::Value(v, d) => {
                assert_eq!(*v, Value::Bool(true));
                assert_eq!(*d, DeterminacyState::Undetermined);
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }

        let events = journal.events_for_node(&node_id);
        assert_eq!(started_commit(events[0]), recorded(TraceSource::GuardedGroup));
        assert_eq!(outcome, Some(EvalOutcome::Changed));
    }

    /// Pins that `CacheLeg::Record`'s `cache_outcome` forwards whatever
    /// `CacheStore::record_evaluation` returns — including the early-cutoff
    /// `Unchanged` signal — not just the cold-start `Changed` case exercised
    /// by `commit_record_writes_all_four_legs`. A regression that dropped or
    /// mislabeled the forwarded outcome (e.g. always reporting `Changed`)
    /// would pass every other test in this module but fail here.
    #[test]
    fn commit_record_forwards_unchanged_outcome_on_repeat_commit() {
        let mut values = ValueMap::new();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut cache = CacheStore::new();
        let mut journal = EventJournal::new();
        let node = ValueCellId::new("Body", "w");

        // First commit: cold start -> Changed.
        let first = commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node.clone(),
            Value::Int(7),
            DeterminacyRule::DeriveFromValue,
            TraceSource::ColdEval,
            DependencyTrace::default(),
            VersionId(1),
            CacheLeg::Record,
        );
        assert_eq!(first, Some(EvalOutcome::Changed));

        // Second commit: identical value and determinacy rule -> identical
        // content hash -> CacheStore's early-cutoff path -> Unchanged.
        let second = commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node.clone(),
            Value::Int(7),
            DeterminacyRule::DeriveFromValue,
            TraceSource::EditReeval,
            DependencyTrace::default(),
            VersionId(2),
            CacheLeg::Record,
        );
        assert_eq!(second, Some(EvalOutcome::Unchanged));

        // The journal's second Completed event for this node reflects the
        // forwarded Unchanged outcome, not a stale/hardcoded Changed.
        let node_id = NodeId::Value(node.clone());
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            4,
            "expected Started+Completed per commit (x2), got {events:?}"
        );
        assert_eq!(completed_outcome(events[3]), Some(EvalOutcome::Unchanged));
    }

    /// Pins INV-EVAL-1's core Skip assertion: `CacheLeg::Skip` must omit
    /// ONLY the cache leg — values, snapshot, and journal are still written
    /// unconditionally. Also proves the divergence in the other direction
    /// from `commit_record_writes_all_four_legs`: `UnconditionalDetermined`
    /// stamps `Determined` even for `Value::Undef` (unlike `DeriveFromValue`),
    /// so the recorded determinacy here is driven by the RULE, not the value.
    #[test]
    fn commit_skip_writes_values_snapshot_journal_but_no_cache_entry() {
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
            Value::Undef,
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::ColdEval,
            DependencyTrace::default(),
            VersionId(1),
            CacheLeg::Skip("cyclic let-cell (2266)"),
        );

        // NO cache entry.
        assert!(cache.is_empty());
        assert!(cache.get(&NodeId::Value(node.clone())).is_none());

        // values + snapshot legs still written.
        assert_eq!(values.get(&node), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&node),
            Some(&(Value::Undef, DeterminacyState::Determined))
        );

        // journal leg still written (Started + Completed).
        let node_id = NodeId::Value(node.clone());
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            2,
            "expected exactly Started + Completed, got {events:?}"
        );
        // The journal alone records that the cache leg was skipped, and why.
        assert_eq!(
            started_commit(events[0]),
            Some(CommitTag {
                trace: TraceSource::ColdEval,
                cache: CacheLegKind::Skipped {
                    reason: "cyclic let-cell (2266)",
                },
            })
        );
        assert!(events[0].payload.is_none(), "got {:?}", events[0].payload);
        // No cache write happened, so there is no cache outcome to report.
        assert_eq!(completed_outcome(events[1]), None);
        assert_eq!(outcome, None);
    }

    /// Pins §2.3's provenance requirement: the commit's `TraceSource` is
    /// recorded on the journal's `Started` event's commit tag — not on
    /// `Completed`, which carries the cache outcome + duration instead.
    #[test]
    fn trace_source_provenance_is_recorded_on_started_event() {
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
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::EditReeval,
            DependencyTrace::default(),
            VersionId(1),
            CacheLeg::Record,
        );

        let node_id = NodeId::Value(node.clone());
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            2,
            "expected exactly Started + Completed, got {events:?}"
        );

        // Started carries the provenance on its commit tag...
        let commit = started_commit(events[0]).expect("a primitive commit is tagged");
        assert_eq!(commit.trace, TraceSource::EditReeval);
        assert_eq!(commit.cache, CacheLegKind::Recorded);

        // ...Completed carries the duration, not provenance.
        assert_eq!(completed_outcome(events[1]), outcome);
        match &events[1].payload {
            Some(EventPayload::Duration(_)) => {}
            other => panic!("expected Completed payload Duration(_), got {other:?}"),
        }
    }

    /// Freshness analogue of `commit_record_writes_all_four_legs` for the new
    /// `CacheLeg::RecordPropagating` variant: it routes the cache leg through
    /// `CacheStore::record_evaluation_propagating_freshness`, which DERIVES the
    /// output freshness from the just-computed trace. With `still_refining:
    /// true` the derivation short-circuits to `Intermediate { generation }`
    /// (generation = `version.0`) regardless of the trace; with `still_refining:
    /// false` over an empty (all-Final) trace it yields `Final`. All four legs
    /// must still be written atomically (INV-EVAL-1) — this is the propagating
    /// counterpart to the four-leg Record test.
    #[test]
    fn commit_record_propagating_derives_intermediate_freshness() {
        let mut values = ValueMap::new();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut cache = CacheStore::new();
        let mut journal = EventJournal::new();
        let node = ValueCellId::new("Body", "w");

        // still_refining: true → Intermediate { generation: version.0 }.
        let outcome = commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node.clone(),
            Value::Int(42),
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::ColdEval,
            DependencyTrace::default(),
            VersionId(5),
            CacheLeg::RecordPropagating {
                still_refining: true,
            },
        );

        // values leg
        assert_eq!(values.get(&node), Some(&Value::Int(42)));

        // snapshot leg
        assert_eq!(
            snapshot_values.get(&node),
            Some(&(Value::Int(42), DeterminacyState::Determined))
        );

        // cache leg present, carrying the DERIVED Intermediate freshness.
        let node_id = NodeId::Value(node.clone());
        let entry = cache
            .get(&node_id)
            .expect("CacheLeg::RecordPropagating must write a cache entry");
        match &entry.result {
            CachedResult::Value(v, d) => {
                assert_eq!(*v, Value::Int(42));
                assert_eq!(*d, DeterminacyState::Determined);
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }
        assert_eq!(
            entry.freshness,
            Freshness::Intermediate { generation: 5 },
            "still_refining=true derives Intermediate at generation=version.0"
        );

        // journal leg: Started then Completed recorded for this node.
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            2,
            "expected exactly Started + Completed, got {events:?}"
        );
        assert_eq!(started_commit(events[0]), recorded(TraceSource::ColdEval));
        assert_eq!(completed_outcome(events[1]), Some(EvalOutcome::Changed));
        assert_eq!(outcome, Some(EvalOutcome::Changed));

        // Companion: still_refining: false over an empty (all-Final) trace
        // derives Final. A distinct node avoids the early-cutoff freshness
        // overwrite that a repeat commit on `node` would trigger.
        let node2 = ValueCellId::new("Body", "h");
        commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node2.clone(),
            Value::Int(7),
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::ColdEval,
            DependencyTrace::default(),
            VersionId(5),
            CacheLeg::RecordPropagating {
                still_refining: false,
            },
        );
        let entry2 = cache
            .get(&NodeId::Value(node2))
            .expect("CacheLeg::RecordPropagating must write a cache entry");
        assert_eq!(
            entry2.freshness,
            Freshness::Final,
            "still_refining=false over an empty (all-Final) trace derives Final"
        );
    }

    /// Freshness analogue for the new `CacheLeg::RecordWithFreshness` variant:
    /// it routes the cache leg through
    /// `CacheStore::record_evaluation_with_freshness`, which writes the
    /// EXPLICIT caller-supplied freshness verbatim (used by the `eval_cached`
    /// preserve-freshness re-serves, which carry the entry's own freshness
    /// forward). All four legs must be written; a cold write reports
    /// `EvalOutcome::Changed`.
    ///
    /// The second half pins the branch the migration actually DEPENDS on: a
    /// repeat commit of the SAME value on the SAME node takes
    /// `record_evaluation_with_freshness`' content-hash EARLY-CUTOFF branch
    /// (`EvalOutcome::Unchanged`, `cache.rs`), and the supplied freshness must
    /// be written there too. That branch is the only one preserving the
    /// entry's `pending_cause`/`diagnostics` — the Changed branch resets both —
    /// so if a future determinacy-rule drift knocked a re-serve off it, the
    /// Pending chain-cause would silently vanish. Asserting `Unchanged` here
    /// makes that drift a unit-test failure rather than a silent regression.
    #[test]
    fn commit_record_with_freshness_writes_supplied_freshness() {
        let mut values = ValueMap::new();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut cache = CacheStore::new();
        let mut journal = EventJournal::new();
        let node = ValueCellId::new("Body", "w");

        let supplied = Freshness::Failed {
            error: ErrorRef::new("x"),
        };
        let outcome = commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node.clone(),
            Value::Bool(true),
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::CachedServe,
            DependencyTrace::default(),
            VersionId(1),
            CacheLeg::RecordWithFreshness(supplied.clone()),
        );

        // values + snapshot legs.
        assert_eq!(values.get(&node), Some(&Value::Bool(true)));
        assert_eq!(
            snapshot_values.get(&node),
            Some(&(Value::Bool(true), DeterminacyState::Determined))
        );

        // cache leg: the SUPPLIED freshness is written verbatim.
        let node_id = NodeId::Value(node.clone());
        let entry = cache
            .get(&node_id)
            .expect("CacheLeg::RecordWithFreshness must write a cache entry");
        match &entry.result {
            CachedResult::Value(v, d) => {
                assert_eq!(*v, Value::Bool(true));
                assert_eq!(*d, DeterminacyState::Determined);
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }
        assert_eq!(
            entry.freshness, supplied,
            "RecordWithFreshness must write the caller-supplied freshness verbatim"
        );

        // journal leg: Started + Completed.
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            2,
            "expected exactly Started + Completed, got {events:?}"
        );
        assert_eq!(started_commit(events[0]), recorded(TraceSource::CachedServe));
        assert_eq!(completed_outcome(events[1]), Some(EvalOutcome::Changed));

        // cold write → Changed.
        assert_eq!(outcome, Some(EvalOutcome::Changed));

        // ── EARLY-CUTOFF branch (the one the eval_cached re-serves ride) ──
        // Re-commit the SAME value on the SAME node with a DIFFERENT supplied
        // freshness. The content hash is unchanged, so `record_evaluation_with_-
        // freshness` takes its early-cutoff branch and reports `Unchanged` —
        // and must still write the newly supplied freshness verbatim.
        let resupplied = Freshness::Intermediate { generation: 9 };
        let outcome2 = commit_cell_result(
            CommitLegs {
                values: &mut values,
                snapshot_values: &mut snapshot_values,
                cache: &mut cache,
                journal: &mut journal,
            },
            node.clone(),
            Value::Bool(true),
            DeterminacyRule::UnconditionalDetermined,
            TraceSource::CachedReuse,
            DependencyTrace::default(),
            VersionId(2),
            CacheLeg::RecordWithFreshness(resupplied.clone()),
        );
        assert_eq!(
            outcome2,
            Some(EvalOutcome::Unchanged),
            "re-committing an identical (value, determinacy) pair must take \
             record_evaluation_with_freshness' content-hash early-cutoff branch \
             — the branch that preserves pending_cause/diagnostics, and the one \
             the eval_cached preserve-freshness re-serves depend on"
        );
        let events = journal.events_for_node(&node_id);
        assert_eq!(started_commit(events[2]), recorded(TraceSource::CachedReuse));
        assert_eq!(completed_outcome(events[3]), Some(EvalOutcome::Unchanged));
        let entry2 = cache
            .get(&node_id)
            .expect("the early-cutoff path must leave the cache entry in place");
        assert_eq!(
            entry2.freshness, resupplied,
            "RecordWithFreshness must write the supplied freshness verbatim on \
             the early-cutoff (Unchanged) path too, not only on the cold path"
        );
        assert_eq!(entry2.basis_version, VersionId(2));
    }

    /// Pins [`DeterminacyRule::preserving`]'s round-trip contract: for every
    /// determinacy state it CAN express, the returned rule reproduces that
    /// state value-independently — including on `Value::Undef`, where
    /// `DeriveFromValue` would diverge. This is what makes the `eval_cached`
    /// re-serves reproduce `entry.result` byte-for-byte (hence stay on the
    /// early-cutoff branch pinned above).
    ///
    /// `Auto`/`Provisional` return `None` rather than panicking, so a caller
    /// can degrade gracefully instead of aborting the whole evaluation on an
    /// unexpected stored state (see the "Determinacy dimension — OPEN" bullet).
    #[test]
    fn determinacy_rule_preserving_round_trips_or_reports_inexpressible() {
        for det in [DeterminacyState::Determined, DeterminacyState::Undetermined] {
            let rule = DeterminacyRule::preserving(det)
                .unwrap_or_else(|| panic!("{det:?} must be expressible"));
            for value in [Value::Undef, Value::Bool(true), Value::Int(3)] {
                assert_eq!(
                    rule.resolve(&value),
                    det,
                    "preserving({det:?}) must reproduce {det:?} for value {value:?} \
                     (value-independently), or the eval_cached re-serves would \
                     rewrite the stored determinacy"
                );
            }
        }
        assert_eq!(
            DeterminacyRule::preserving(DeterminacyState::Auto),
            None,
            "Auto is not expressible by any DeterminacyRule — callers must degrade"
        );
        assert_eq!(
            DeterminacyRule::preserving(DeterminacyState::Provisional),
            None,
            "Provisional is not expressible by any DeterminacyRule — callers must degrade"
        );
    }
}
