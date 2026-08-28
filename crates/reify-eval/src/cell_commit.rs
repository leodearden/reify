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
//! The freshness gap below is now **CLOSED** (task #5238); three gaps remain
//! OPEN — the `Skip`-outcome placeholder, the determinacy dimension, and the
//! failure-path commit shape. Each is deliberately left as a documented
//! limitation rather than closed by widening this module's enums or by
//! changing `journal.rs`'s `EventKind` shape. A migration leaf that hits one
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
//! - **`CacheLeg::Skip`'s journal `Completed.outcome` is an unspecified
//!   placeholder — OPEN**, not a meaningful `EvalOutcome` — see the doc comment at
//!   the `Completed` event construction inside [`commit_cell_result`], and
//!   the `commit_skip_writes_values_snapshot_journal_but_no_cache_entry`
//!   test that pins it. The authoritative signal that nothing was cached is
//!   [`CommitOutcome::skip_reason`] (in-memory) or the paired `Started`
//!   event's `cache-skip=` payload marker (journal-only). Making this
//!   type-safe (e.g. `EventKind::Completed { outcome: Option<EvalOutcome> }`)
//!   would change `journal.rs`'s `EventKind` shape — out of scope for this
//!   module.
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
use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};

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

/// Provenance tag recorded on a commit's journal `Started` event: which
/// call-path produced this value. Makes the journal self-describing so the
/// §2.6 divergence audit (a later leaf) can attribute a mismatch to its
/// producing path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSource {
    /// First-time (cold) evaluation of a node with no prior cache entry.
    ColdEval,
    /// Produced by the `eval_cached` warm pass's cache-MISS arm — a cold eval
    /// inside the cached-serve pass. Deliberately distinct from
    /// [`TraceSource::CachedReuse`]: this arm recomputes a value and writes
    /// `Freshness::Final` via `CacheLeg::Record`.
    CachedServe,
    /// Re-served verbatim from an existing non-dirty cache entry by one of
    /// `eval_cached`'s preserve-freshness re-serves (Param and Let), which
    /// reproduce the stored `(value, determinacy)` pair and carry the entry's
    /// own freshness forward via `CacheLeg::RecordWithFreshness`. Split out
    /// from [`TraceSource::CachedServe`] (task #5238) so the journal alone
    /// separates a re-serve from a miss — a §2.6 divergence audit reading the
    /// slug must not have to reach for the in-memory `CacheLeg`, which
    /// [`commit_cell_result`] consumes and never records.
    ///
    /// Named `CachedReuse`/`"cached-reuse"`, NOT `CachedReserve`/
    /// `"cached-reserve"`: sitting directly beside [`TraceSource::CachedServe`]
    /// / `"cached-serve"`, a one-letter difference would have carried the whole
    /// semantic load, and "reserve" (to set aside) is a different English word
    /// from "re-serve" (to serve again) — the intended sense. "Reuse" is also
    /// what the producing blocks in `engine_eval.rs` already call themselves
    /// (`// Cache-reuse: not dirty + entry exists`). Renamed before the slug
    /// shipped; [`TraceSource::as_str`]'s freeze rule now applies to it.
    CachedReuse,
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
    /// verbatim on every cache-WRITING [`CacheLeg`] (`Record`,
    /// `RecordPropagating`, `RecordWithFreshness`), or with a
    /// `|cache-skip=<reason>` suffix appended on `CacheLeg::Skip` (see
    /// [`commit_cell_result`]) — and
    /// intended as the stable key a future divergence audit attributes a
    /// mismatch to, so these strings, once shipped, should not be renamed.
    ///
    /// Every variant's slug is pinned — for uniqueness AND for its exact
    /// frozen string — by `trace_source_provenance_is_recorded_on_started_event`
    /// in this module's tests, over the `ALL_TRACE_SOURCES` array that
    /// `trace_source_enumeration_is_exhaustive` forces to stay exhaustive.
    pub fn as_str(&self) -> &'static str {
        match self {
            TraceSource::ColdEval => "cold-eval",
            TraceSource::CachedServe => "cached-serve",
            TraceSource::CachedReuse => "cached-reuse",
            TraceSource::EditReeval => "edit-reeval",
            TraceSource::GuardedGroup => "guarded-group",
            TraceSource::PostPassOverwrite => "post-pass-overwrite",
            TraceSource::ConeReeval => "cone-reeval",
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
/// borrows the leg for the `Started`-payload match and consumes it for the
/// cache-write match.
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
    /// Omit the cache leg. Carries the reason, surfaced on
    /// [`CommitOutcome::skip_reason`] for later divergence-audit exemptions.
    Skip(&'static str),
}

/// Outcome of a [`commit_cell_result`] call.
///
/// Carries the resolved `determinacy` plus the cache/skip/provenance metadata
/// the four-leg commit produced. Fields are private — migration call sites in
/// other modules (leaves γ/δ/ε/ι) read them via the `pub(crate)` accessor
/// methods below, not by reaching into the struct.
///
/// Deliberately does NOT carry the committed `Value` (#5238 amendment). It
/// once did, purely so a caller could read the inserted tuple back — but no
/// production call site ever did, and holding a 4th owner of the value forced
/// an extra deep `Value` clone on EVERY commit. `Value` is not `Arc`-backed
/// (`Value::List(Vec<Value>)`, `Value::Enum { payload: Vec<_> }`,
/// `Value::String`), and the `eval_cached` preserve-freshness re-serves run
/// this path for every clean cell on every warm pass, so that clone was a real
/// per-cell-per-pass cost. A caller that needs the value back already owns it
/// at the call site (it passed it in); the committed copies are readable from
/// the `values`/`snapshot` legs.
// #5238: migration call sites discard the returned CommitOutcome, so
// `determinacy`, `skip_reason` and `trace_source` are read only by the accessors
// below + tests until the §2.6 divergence-audit consumer (a future leaf) reads
// them from non-test code. (`cache_outcome` is already read by non-test code.)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CommitOutcome {
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
// #5238: migration call sites discard the returned CommitOutcome, so these
// accessors are exercised only by tests until the §2.6 divergence-audit consumer
// (a future leaf) consumes a commit's result from non-test code.
#[allow(dead_code)]
impl CommitOutcome {
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
) -> CommitOutcome {
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
) -> CommitOutcome {
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

    // The `Started` event's payload doubles as the journal-only source of
    // truth for the cache leg's fate: the trace-source slug always, plus a
    // `|cache-skip=<reason>` suffix when `cache_leg` is `Skip`. This is what
    // lets a consumer with access to the journal alone — not the in-memory
    // `CommitOutcome` — discover that the cache leg was omitted, and why; the
    // `Completed` event constructed below is NOT such a signal on the Skip
    // path (see its `outcome` field's doc comment).
    let started_payload = match &cache_leg {
        CacheLeg::Record
        | CacheLeg::RecordPropagating { .. }
        | CacheLeg::RecordWithFreshness(_) => trace.as_str().to_string(),
        CacheLeg::Skip(reason) => format!("{}|cache-skip={reason}", trace.as_str()),
    };

    journal.record(EvalEvent {
        timestamp: start,
        node_id: node_id.clone(),
        kind: EventKind::Started,
        version,
        payload: Some(EventPayload::Custom(started_payload)),
    });

    // A cache-writing commit needs exactly 3 independently-owned copies of
    // `value` (values leg, snapshot leg, cache leg). `Value` is not `Copy` and
    // is not `Arc`-backed, so each extra owner costs a real deep clone. The two
    // clones below cover values+snapshot; the ORIGINAL `value` is then MOVED
    // into whichever cache-writing arm runs, so no third clone is made and
    // `CommitOutcome` holds no value at all (#5238 amendment — see its doc).
    // `CacheLeg::Skip` needs only 2 copies and simply drops the original. That
    // is the minimum possible for however many legs actually write; reordering
    // cannot reduce it further (some owner must always "spend" the original via
    // move, and one is all that's available).
    values.insert(node.clone(), value.clone());
    snapshot_values.insert(node, (value.clone(), det));

    let (cache_outcome, skip_reason) = match cache_leg {
        CacheLeg::Record => {
            let outcome = cache.record_evaluation(
                node_id.clone(),
                CachedResult::Value(value, det),
                version,
                dependency_trace,
            );
            (Some(outcome), None)
        }
        CacheLeg::RecordPropagating { still_refining } => {
            let outcome = cache.record_evaluation_propagating_freshness(
                node_id.clone(),
                CachedResult::Value(value, det),
                version,
                dependency_trace,
                still_refining,
            );
            (Some(outcome), None)
        }
        CacheLeg::RecordWithFreshness(freshness) => {
            let outcome = cache.record_evaluation_with_freshness(
                node_id.clone(),
                CachedResult::Value(value, det),
                version,
                dependency_trace,
                freshness,
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
    use reify_ir::{DeterminacyState, ErrorRef, Freshness, PersistentMap, Value, ValueMap};

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

        // Returned CommitOutcome mirrors the commit's metadata. Read via the
        // pub(crate) accessors (not the private fields directly) to prove the
        // accessors migration call sites will use return the right data. The
        // committed VALUE is deliberately not carried on the outcome (#5238
        // amendment); it is asserted above, on the legs that actually hold it.
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

        // Every variant maps to a distinct, stable kebab slug. `ALL_TRACE_SOURCES`
        // is kept exhaustive by `trace_source_enumeration_is_exhaustive` below —
        // adding a variant without listing it here is a compile error, not a
        // silently-unchecked slug (#5238 amendment: `CachedReuse` was shipped
        // while this list still had six entries, so neither its uniqueness nor
        // its frozen string was covered).
        let slugs: Vec<&'static str> = ALL_TRACE_SOURCES.iter().map(|t| t.as_str()).collect();
        assert_eq!(
            slugs,
            [
                "cold-eval",
                "cached-serve",
                "cached-reuse",
                "edit-reeval",
                "guarded-group",
                "post-pass-overwrite",
                "cone-reeval",
            ],
            "TraceSource slugs are frozen once shipped (see TraceSource::as_str)"
        );
        let unique: std::collections::HashSet<_> = slugs.iter().collect();
        assert_eq!(
            unique.len(),
            slugs.len(),
            "TraceSource slugs must be distinct: {slugs:?}"
        );
    }

    /// Every [`TraceSource`] variant, in declaration order — the list
    /// `trace_source_provenance_is_recorded_on_started_event` pins slugs over.
    ///
    /// Kept exhaustive by `trace_source_enumeration_is_exhaustive`: because that
    /// sentinel matches on `TraceSource` with no wildcard arm, adding a variant
    /// fails to compile until it is named there, and the `ALL_TRACE_SOURCES.len()`
    /// assertion then fails until it is added here too.
    const ALL_TRACE_SOURCES: [TraceSource; 7] = [
        TraceSource::ColdEval,
        TraceSource::CachedServe,
        TraceSource::CachedReuse,
        TraceSource::EditReeval,
        TraceSource::GuardedGroup,
        TraceSource::PostPassOverwrite,
        TraceSource::ConeReeval,
    ];

    /// Compile-time + run-time guard that `ALL_TRACE_SOURCES` really is every
    /// variant, so the slug uniqueness/stability assertions above are checked
    /// over the WHOLE enum rather than over a hand-maintained subset that
    /// silently drifts (which is exactly how `CachedReuse` shipped uncovered).
    #[test]
    fn trace_source_enumeration_is_exhaustive() {
        // Wildcard-free match: a new `TraceSource` variant fails to compile
        // here until it is named, which is the whole point of the sentinel.
        // Mapping each variant to its expected INDEX (rather than just
        // counting) also catches a repeated or omitted entry in the array.
        fn declaration_index(t: TraceSource) -> usize {
            match t {
                TraceSource::ColdEval => 0,
                TraceSource::CachedServe => 1,
                TraceSource::CachedReuse => 2,
                TraceSource::EditReeval => 3,
                TraceSource::GuardedGroup => 4,
                TraceSource::PostPassOverwrite => 5,
                TraceSource::ConeReeval => 6,
            }
        }
        assert_eq!(
            ALL_TRACE_SOURCES.len(),
            7,
            "ALL_TRACE_SOURCES must list every TraceSource variant exactly once"
        );
        for (i, t) in ALL_TRACE_SOURCES.iter().enumerate() {
            assert_eq!(
                declaration_index(*t),
                i,
                "ALL_TRACE_SOURCES[{i}] = {t:?} is out of order, repeated, or \
                 omits a variant: {ALL_TRACE_SOURCES:?}"
            );
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
        assert!(matches!(events[0].kind, EventKind::Started));
        assert!(matches!(events[1].kind, EventKind::Completed { .. }));

        assert_eq!(outcome.cache_outcome(), Some(EvalOutcome::Changed));
        assert_eq!(outcome.skip_reason(), None);

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
        assert!(matches!(events[0].kind, EventKind::Started));
        assert!(matches!(events[1].kind, EventKind::Completed { .. }));

        // cold write → Changed.
        assert_eq!(outcome.cache_outcome(), Some(EvalOutcome::Changed));
        assert_eq!(outcome.skip_reason(), None);

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
            outcome2.cache_outcome(),
            Some(EvalOutcome::Unchanged),
            "re-committing an identical (value, determinacy) pair must take \
             record_evaluation_with_freshness' content-hash early-cutoff branch \
             — the branch that preserves pending_cause/diagnostics, and the one \
             the eval_cached preserve-freshness re-serves depend on"
        );
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
