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
//!
//! ## Known scope gaps for migration leaves (γ/δ/ε/ι)
//!
//! Both gaps below are deliberately left as documented limitations rather
//! than closed by extending an enum now, since closing them would deviate
//! from the two-variant `CacheLeg` shape the PRD's §2.4 contract sketch
//! fixes for this task. A migration leaf that hits either gap must address
//! it explicitly (extend the enum, or document why the site is left
//! unmigrated) — not silently paper over it by routing through the existing
//! variants as-is.
//!
//! - **No freshness dimension.** [`CacheLeg::Record`] always writes
//!   `Freshness::Final` (see its doc comment) — there is no path to
//!   `CacheStore::record_evaluation_propagating_freshness` (arch §7.2). The
//!   dominant let/param commit sites in `engine_eval.rs`
//!   (`evaluate_params_and_lets_unified`, `evaluate_let_bindings`) use the
//!   propagating variant to derive freshness, so a migration leaf touching
//!   them cannot represent that commit through `commit_cell_result` as
//!   currently shaped.
//! - **`CacheLeg::Skip`'s journal `Completed.outcome` is an unspecified
//!   placeholder**, not a meaningful `EvalOutcome` — see the doc comment at
//!   the `Completed` event construction inside [`commit_cell_result`], and
//!   the `commit_skip_writes_values_snapshot_journal_but_no_cache_entry`
//!   test that pins it. The authoritative signal that nothing was cached is
//!   [`CommitOutcome::skip_reason`] (in-memory) or the paired `Started`
//!   event's `cache-skip=` payload marker (journal-only). Making this
//!   type-safe (e.g. `EventKind::Completed { outcome: Option<EvalOutcome> }`)
//!   would change `journal.rs`'s `EventKind` shape — out of scope for this
//!   module.

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

impl TraceSource {
    /// A stable, kebab-case slug identifying this provenance path. Recorded
    /// on the journal `Started` event's [`EventPayload::Custom`] payload —
    /// verbatim on `CacheLeg::Record`, or with a `|cache-skip=<reason>`
    /// suffix appended on `CacheLeg::Skip` (see [`commit_cell_result`]) — and
    /// intended as the stable key a future divergence audit attributes a
    /// mismatch to, so these strings, once shipped, should not be renamed.
    pub fn as_str(&self) -> &'static str {
        match self {
            TraceSource::ColdEval => "cold-eval",
            TraceSource::CachedServe => "cached-serve",
            TraceSource::EditReeval => "edit-reeval",
            TraceSource::GuardedGroup => "guarded-group",
            TraceSource::PostPassOverwrite => "post-pass-overwrite",
            TraceSource::ConeReeval => "cone-reeval",
        }
    }
}

/// Whether a commit writes the cache leg, and if not, why.
///
/// `Record` is a unit variant (not value-carrying) — the cache leg's
/// `DependencyTrace` rides as a separate `commit_cell_result` parameter,
/// used only on that arm.
///
/// **Freshness scope gap:** `Record` always writes `Freshness::Final` (see
/// its doc below) — there is no variant routing to
/// `CacheStore::record_evaluation_propagating_freshness` (arch §7.2). See
/// the module doc's "Known scope gaps" section before wiring a migration
/// site that needs freshness propagation.
#[allow(dead_code)] // constructed by migration call sites from leaves γ/δ/ε/ι; tests only until then
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLeg {
    /// Write the cache leg via [`CacheStore::record_evaluation`], which is a
    /// thin wrapper that hard-codes `Freshness::Final` (see its own doc
    /// comment in `cache.rs`). Does NOT call
    /// `record_evaluation_propagating_freshness` — a commit site that needs
    /// derived freshness per arch §7.2 cannot be represented by this variant
    /// as currently shaped.
    Record,
    /// Omit the cache leg. Carries the reason, surfaced on
    /// [`CommitOutcome::skip_reason`] for later divergence-audit exemptions.
    Skip(&'static str),
}

/// Outcome of a [`commit_cell_result`] call.
///
/// Carries the `(value, determinacy)` pair so callers that previously read
/// back the inserted tuple keep working, plus the cache/skip/provenance
/// metadata the four-leg commit produced. Fields are private — migration
/// call sites in other modules (leaves γ/δ/ε/ι) read them via the
/// `pub(crate)` accessor methods below, not by reaching into the struct.
#[allow(dead_code)] // fields read by tests/accessors only until migration leaves γ/δ/ε/ι land
#[derive(Debug, Clone)]
pub struct CommitOutcome {
    value: Value,
    determinacy: DeterminacyState,
    /// `Some` on `CacheLeg::Record` (forwarded from `record_evaluation`),
    /// `None` on `CacheLeg::Skip` — the authoritative in-memory signal that
    /// nothing was cached. The same fact is independently recoverable from
    /// the journal alone, without this struct, via the paired `Started`
    /// event's `cache-skip=` payload marker — see [`commit_cell_result`].
    cache_outcome: Option<EvalOutcome>,
    /// `Some(reason)` on `CacheLeg::Skip`, `None` on `CacheLeg::Record`.
    skip_reason: Option<&'static str>,
    trace_source: TraceSource,
}

/// Read-only accessors for [`CommitOutcome`] — the shape migration call
/// sites in other modules (leaves γ/δ/ε/ι) use to consume a commit's result,
/// since the struct's fields are private to this module.
#[allow(dead_code)] // called by migration call sites from leaves γ/δ/ε/ι; a test exercises them until then
impl CommitOutcome {
    /// The committed value — mirrors what was written to the values and
    /// snapshot legs.
    pub(crate) fn value(&self) -> &Value {
        &self.value
    }

    /// The resolved determinacy state — mirrors what was written to the
    /// snapshot leg's tuple and, on `CacheLeg::Record`, the cache leg's
    /// `CachedResult::Value`.
    pub(crate) fn determinacy(&self) -> DeterminacyState {
        self.determinacy
    }

    /// `Some` on `CacheLeg::Record` (forwarded from `record_evaluation`),
    /// `None` on `CacheLeg::Skip`.
    pub(crate) fn cache_outcome(&self) -> Option<EvalOutcome> {
        self.cache_outcome
    }

    /// `Some(reason)` on `CacheLeg::Skip`, `None` on `CacheLeg::Record`.
    pub(crate) fn skip_reason(&self) -> Option<&'static str> {
        self.skip_reason
    }

    /// The provenance path this commit was attributed to.
    pub(crate) fn trace_source(&self) -> TraceSource {
        self.trace_source
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
/// only `Completed`. On `CacheLeg::Skip`, the `Started` event's payload
/// additionally carries a `cache-skip=<reason>` marker (see body), so the
/// journal alone — with no access to the in-memory [`CommitOutcome`] — is
/// sufficient to tell a genuine cache write apart from a skip.
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

    // The `Started` event's payload doubles as the journal-only source of
    // truth for the cache leg's fate: the trace-source slug always, plus a
    // `|cache-skip=<reason>` suffix when `cache_leg` is `Skip`. This is what
    // lets a consumer with access to the journal alone — not the in-memory
    // `CommitOutcome` — discover that the cache leg was omitted, and why; the
    // `Completed` event constructed below is NOT such a signal on the Skip
    // path (see its `outcome` field's doc comment).
    let started_payload = match cache_leg {
        CacheLeg::Record => trace.as_str().to_string(),
        CacheLeg::Skip(reason) => format!("{}|cache-skip={reason}", trace.as_str()),
    };

    journal.record(EvalEvent {
        timestamp: start,
        node_id: node_id.clone(),
        kind: EventKind::Started,
        version,
        payload: Some(EventPayload::Custom(started_payload)),
    });

    // Every commit needs 3 independently-owned copies of `value` (values leg,
    // snapshot leg, and the returned `CommitOutcome`); `CacheLeg::Record`
    // needs a 4th (the cache leg). `Value` is not `Copy` and can hold large
    // payloads, so each extra owner costs a real clone — the two clones below
    // cover values+snapshot, the cache leg's clone (Record arm only, below)
    // is the 3rd, and the original `value` is reused via a move into
    // `CommitOutcome` at the end of this function rather than a 4th clone.
    // That is already the minimum possible for however many legs actually
    // write; reordering cannot reduce it further (some owner must always be
    // the one that "spends" the original via move, and one is all that's
    // available).
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
        CacheLeg::Skip(reason) => (None, Some(reason)),
    };

    journal.record(EvalEvent {
        timestamp: Instant::now(),
        node_id,
        kind: EventKind::Completed {
            // `EvalOutcome` has only `Changed`/`Unchanged` — both imply a
            // cache write happened, so neither is an honest value when
            // `cache_leg` was `CacheLeg::Skip(_)` (no `record_evaluation`
            // call, no cache entry written). On that path this `outcome` is
            // an UNSPECIFIED PLACEHOLDER, present only so the `Completed`
            // event can be constructed; it must NOT be read as a claim that
            // anything changed or was cached. The authoritative "nothing was
            // cached" signal is `skip_reason.is_some()` (equivalently
            // `CommitOutcome::cache_outcome == None`) for in-memory callers,
            // and the paired `Started` event's `cache-skip=` payload marker
            // (constructed above) for a journal-only consumer — a future
            // §2.6 divergence audit must branch on one of those, never on
            // this field, for any commit where the cache leg was skipped.
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
    use crate::journal::{EventJournal, EventKind, EventPayload};

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
        assert!(matches!(events[0].kind, EventKind::Started));
        assert!(matches!(events[1].kind, EventKind::Completed { .. }));

        // Returned CommitOutcome mirrors what was written to all four legs.
        // Read via the pub(crate) accessors (not the private fields
        // directly) to prove the accessors migration call sites will use
        // return the right data.
        assert_eq!(*outcome.value(), Value::Bool(true));
        assert_eq!(outcome.determinacy(), DeterminacyState::Determined);
        assert_eq!(outcome.cache_outcome(), Some(EvalOutcome::Changed));
        assert_eq!(outcome.skip_reason(), None);
        assert_eq!(outcome.trace_source(), TraceSource::ColdEval);
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

        assert_eq!(outcome.determinacy, DeterminacyState::Undetermined);
        assert_eq!(outcome.cache_outcome, Some(EvalOutcome::Changed));
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
        assert_eq!(first.cache_outcome, Some(EvalOutcome::Changed));

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
        assert_eq!(second.cache_outcome, Some(EvalOutcome::Unchanged));

        // The journal's second Completed event for this node reflects the
        // forwarded Unchanged outcome, not a stale/hardcoded Changed.
        let node_id = NodeId::Value(node.clone());
        let events = journal.events_for_node(&node_id);
        assert_eq!(
            events.len(),
            4,
            "expected Started+Completed per commit (x2), got {events:?}"
        );
        match &events[3].kind {
            EventKind::Completed { outcome } => {
                assert_eq!(*outcome, EvalOutcome::Unchanged);
            }
            other => panic!("expected Completed{{outcome: Unchanged}}, got {other:?}"),
        }
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
        assert!(matches!(events[0].kind, EventKind::Started));
        assert!(matches!(events[1].kind, EventKind::Completed { .. }));

        // The journal alone (no access to the in-memory CommitOutcome) must
        // be able to discover that the cache leg was skipped, and why: the
        // Started event's payload carries the trace slug plus a
        // `cache-skip=<reason>` suffix.
        match &events[0].payload {
            Some(EventPayload::Custom(slug)) => assert_eq!(
                slug, "cold-eval|cache-skip=cyclic let-cell (2266)",
                "Started payload must carry both the trace slug and the skip reason"
            ),
            other => panic!("expected Started payload Custom(_), got {other:?}"),
        }

        // Completed's `outcome` on the Skip path is an UNSPECIFIED PLACEHOLDER
        // (see the doc comment in commit_cell_result) — pinned here so a
        // future change to the fallback is caught by this test instead of
        // silently becoming load-bearing. It must NOT be read as "nothing
        // changed" or "the value was cached": the Started-payload assertion
        // above is the actual, journal-recoverable skip signal.
        match &events[1].kind {
            EventKind::Completed { outcome } => {
                assert_eq!(*outcome, EvalOutcome::Changed);
            }
            other => panic!("expected Completed{{outcome}}, got {other:?}"),
        }

        assert_eq!(outcome.cache_outcome, None);
        assert_eq!(outcome.skip_reason, Some("cyclic let-cell (2266)"));
        assert_eq!(outcome.determinacy, DeterminacyState::Determined);
    }

    /// Pins §2.3's provenance requirement: the commit's `TraceSource` is
    /// recorded on the journal's `Started` event — not `Completed`, which
    /// carries the cache outcome + duration instead — and every `TraceSource`
    /// variant maps to a distinct, stable kebab slug via `as_str`, the string
    /// a later divergence audit would key off of.
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

        // Started carries the provenance slug...
        assert!(matches!(events[0].kind, EventKind::Started));
        match &events[0].payload {
            Some(EventPayload::Custom(slug)) => assert_eq!(slug, "edit-reeval"),
            other => panic!("expected Started payload Custom(\"edit-reeval\"), got {other:?}"),
        }

        // ...Completed carries the duration, not provenance.
        assert!(matches!(events[1].kind, EventKind::Completed { .. }));
        match &events[1].payload {
            Some(EventPayload::Duration(_)) => {}
            other => panic!("expected Completed payload Duration(_), got {other:?}"),
        }

        assert_eq!(outcome.trace_source, TraceSource::EditReeval);

        // Every variant maps to a distinct, stable kebab slug.
        let slugs = [
            TraceSource::ColdEval.as_str(),
            TraceSource::CachedServe.as_str(),
            TraceSource::EditReeval.as_str(),
            TraceSource::GuardedGroup.as_str(),
            TraceSource::PostPassOverwrite.as_str(),
            TraceSource::ConeReeval.as_str(),
        ];
        assert_eq!(
            slugs,
            [
                "cold-eval",
                "cached-serve",
                "edit-reeval",
                "guarded-group",
                "post-pass-overwrite",
                "cone-reeval",
            ]
        );
        let unique: std::collections::HashSet<_> = slugs.iter().collect();
        assert_eq!(
            unique.len(),
            slugs.len(),
            "TraceSource slugs must be distinct: {slugs:?}"
        );
    }
}
