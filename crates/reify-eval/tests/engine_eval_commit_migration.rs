//! Integration tests for task γ (#5053): migrating engine_eval.rs's per-cell
//! eval-and-commit transaction/ctx sites onto `commit_cell_result` (task α,
//! `cell_commit.rs`) built with the free-function `cell_eval_ctx` (task β,
//! `cell_eval_ctx.rs`).
//!
//! CHARACTERIZATION-FIRST / behaviour-preserving: each migrated site keeps
//! its current determinacy rule and cache-recording decision. Every test
//! below is written RED-first against TODAY's baseline behaviour (no
//! `commit_cell_result` call at its target site yet), pins the preserved
//! value/determinacy, and turns GREEN only once the matching `impl-N` step
//! wires the site onto the primitive. See `docs/prds/v0_6/eval-cell-commit-substrate.md`
//! §2, §7 (B2/B3 — B1 is out of γ's scope per esc-5053-2 Option A).
//!
//! This file grows incrementally: the prerequisite `pre-1` commit added
//! shared scaffolding only (the `started_payload` provenance-reading helper
//! and the trigger `.ri` sources); `test-1` onward each add one `#[test]` fn.

use reify_core::ValueCellId;
use reify_core::VersionId;
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::journal::{EventKind, EventPayload};
use reify_ir::Value;
use reify_test_support::{make_engine, mm, parse_and_compile};

/// Returns the payload of the FIRST `EventKind::Started` event recorded for
/// `id` in `engine`'s journal, IF that payload is `EventPayload::Custom` —
/// `None` if there is no `Started` event for `id`, or if its payload isn't
/// `Custom` (e.g. today's baseline `payload: None` at the not-yet-migrated
/// sites).
///
/// This is the observability hook every RED provenance test in this file
/// asserts against: `commit_cell_result` (`cell_commit.rs`) records the
/// migration's additive `TraceSource` slug on the `Started` event's
/// `EventPayload::Custom` — verbatim on `CacheLeg::Record`, or suffixed
/// `|cache-skip=<reason>` on `CacheLeg::Skip` — so a test asserting
/// `started_payload(&engine, &cell) == Some("<slug>")` is RED before the
/// matching site is migrated (today's payload is `None` or absent) and GREEN
/// after.
fn started_payload(engine: &Engine, id: &ValueCellId) -> Option<String> {
    let node_id = NodeId::Value(id.clone());
    let events = engine.journal().events_for_node(&node_id);
    let started = events
        .iter()
        .find(|event| matches!(event.kind, EventKind::Started))?;
    match &started.payload {
        Some(EventPayload::Custom(slug)) => Some(slug.clone()),
        _ => None,
    }
}

/// Guarded group with an accepted-default Param (`x_ok`) and a
/// rejected-override-no-default Param (`x_rejected`) in the SAME `where`
/// block — drives `eval_guarded_group_param_cell` (engine_eval.rs, ~@2132).
/// Two structure-scope `let`s sit alongside the guard, each a
/// `determined(..)` probe on one of the guarded cells — a `let` outside a
/// `where` block may reference a cell declared inside it (verified: this
/// source compiles).
///
/// `x_rejected` has no default_expr; a test sets an incompatible-type-kind
/// override on it BEFORE eval (a `Value::Scalar` against its `Int`
/// cell_type) via `Engine::set_param_and_invalidate`, so
/// `validate_param_override` rejects it with `TypeKindMismatch` and the
/// helper's rejected-override-no-default arm fires — this is the parity
/// fixture's rejected-override cell (writes `(Undef, Undetermined)`).
/// `x_ok` has no override ever set, so its default_expr (`5mm`) evaluates
/// and the helper's override/default-accepted arm fires (writes
/// `(val, Determined)`).
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `x_ok` = `Scalar(0.005m, LENGTH)`, `x_rejected` = `Undef`, and exactly one
/// `Warning` diagnostic ("type-kind mismatch") — reaches the target site as
/// intended.
const GUARDED_GROUP_SRC: &str = r#"
    structure S {
        param active : Bool = true
        where active {
            param x_ok : Length = 5mm
            param x_rejected : Int
        }
        let ok_a = determined(x_ok)
        let ok_r = determined(x_rejected)
    }
"#;

/// A plain `Let` with no guard/structural-query/self-datum wrapper — drives
/// the `eval_cached` Let cache-miss path (engine_eval.rs, ~@6317). A sibling
/// `let` probes `determined(..)` on it.
const PLAIN_LET_SRC: &str = r#"
    structure S {
        let y = 5mm
        let y_det = determined(y)
    }
"#;

/// A `Let` whose default_expr is a structural query (`self.members`) —
/// drives the structural-query post-pass (engine_eval.rs, ~@3894-3903). A
/// sibling `let` probes `determined(..)` on it. Reaches the post-pass
/// regardless of whether `S` declares any `sub`s: 0 subs evaluates
/// `self.members` to `Value::List(vec![])` — an empty-but-concrete list, so
/// `determined(ms)` is `true` (empty collections are genuine resolved values,
/// not `Undef`).
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `ms` = `List([])` and no diagnostics.
const STRUCTURAL_QUERY_SRC: &str = r#"
    structure S {
        let ms = self.members
        let ms_det = determined(ms)
    }
"#;

/// A `Let` whose default_expr is a self-datum projection (`self.xy_plane`) —
/// drives the self-datum projection post-pass (engine_eval.rs, ~@3984-3993).
/// Kernel-free: `self.xy_plane` resolves to the intrinsic identity-frame
/// `xy` plane constant with no geometry kernel needed.
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `p` = a concrete `Value::Plane { .. }` (not `Undef`) and no diagnostics.
#[allow(dead_code)] // first consumer lands at test-5
const SELF_DATUM_SRC: &str = r#"
    structure S {
        let p = self.xy_plane
    }
"#;

// ─────────────────────────────────────────────────────────────────────────
// test-1: guarded-group Param provenance + determinacy
// ─────────────────────────────────────────────────────────────────────────

/// RED: guarded-group Param provenance + determinacy preserved.
///
/// RED today: `eval_guarded_group_param_cell` (engine_eval.rs, ~@2132) emits
/// its Started journal event with `payload: None` (@~2148) — none of its
/// four value-write arms calls `commit_cell_result` yet — so
/// `started_payload` is `None` for both `x_ok` and `x_rejected`, not
/// `Some("guarded-group")`. GREEN after impl-1 migrates the helper's commit
/// onto `commit_cell_result` with `TraceSource::GuardedGroup`.
///
/// Also pins determinacy PRESERVED across the migration boundary — the
/// characterization half of this test, which must stay green through
/// impl-1, not just after it: `x_ok` (override/default-accepted arm) is
/// Determined -> `ok_a` = `Bool(true)`; `x_rejected`
/// (rejected-override-no-default arm) is Undetermined -> `ok_r` =
/// `Bool(false)`, and `x_rejected`'s raw value is `Value::Undef` in
/// `EvalResult.values` — this is the parity fixture's rejected-override cell
/// (test-7 reuses this exact scenario).
#[test]
fn guarded_group_param_provenance_and_determinacy() {
    let module = parse_and_compile(GUARDED_GROUP_SRC);
    let mut engine = make_engine();

    let x_rejected_id = ValueCellId::new("S", "x_rejected");
    // Store an incompatible-type-kind override BEFORE eval so the
    // rejected-override-no-default arm fires (a Length Scalar override
    // against an Int cell_type -> ParamOverrideRejection::TypeKindMismatch).
    engine.set_param_and_invalidate(&x_rejected_id, mm(5.0));

    let result = engine.eval(&module);

    let x_ok_id = ValueCellId::new("S", "x_ok");
    let ok_a_id = ValueCellId::new("S", "ok_a");
    let ok_r_id = ValueCellId::new("S", "ok_r");

    eprintln!("x_ok started_payload = {:?}", started_payload(&engine, &x_ok_id));
    eprintln!("x_rejected started_payload = {:?}", started_payload(&engine, &x_rejected_id));
    eprintln!("ok_a = {:?}", result.values.get(&ok_a_id));
    eprintln!("ok_r = {:?}", result.values.get(&ok_r_id));
    eprintln!("x_rejected value = {:?}", result.values.get(&x_rejected_id));
    eprintln!("diagnostics = {:?}", result.diagnostics);

    // (1) Provenance — RED today (payload is None, not Some("guarded-group")).
    assert_eq!(
        started_payload(&engine, &x_ok_id),
        Some("guarded-group".to_string()),
        "x_ok's (override/default-accepted arm) Started event should carry the \
         'guarded-group' TraceSource slug once migrated onto commit_cell_result"
    );
    assert_eq!(
        started_payload(&engine, &x_rejected_id),
        Some("guarded-group".to_string()),
        "x_rejected's (rejected-override-no-default arm) Started event should carry \
         the 'guarded-group' TraceSource slug once migrated onto commit_cell_result"
    );

    // (2) Determinacy preserved (characterization guard).
    assert_eq!(
        result.values.get(&ok_a_id),
        Some(&Value::Bool(true)),
        "x_ok (override/default-accepted) should be Determined -> determined(x_ok) = true"
    );
    assert_eq!(
        result.values.get(&ok_r_id),
        Some(&Value::Bool(false)),
        "x_rejected (rejected-override-no-default) should be Undetermined -> \
         determined(x_rejected) = false"
    );
    assert_eq!(
        result.values.get(&x_rejected_id),
        Some(&Value::Undef),
        "x_rejected's raw value must be Value::Undef in EvalResult.values \
         (parity fixture's rejected-override cell)"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// test-3: eval_cached Let-miss provenance + determinacy
// ─────────────────────────────────────────────────────────────────────────

/// RED: `eval_cached` Let cache-miss provenance + determinacy preserved.
///
/// RED today: the Let cache-miss arm (engine_eval.rs, ~@6317) records its
/// Started journal event with `payload: None` (@~6322-6328), so
/// `started_payload` is `None`, not `Some("cached-serve")`. GREEN after
/// impl-3 migrates the commit onto `commit_cell_result` with
/// `TraceSource::CachedServe`.
///
/// Also pins `UnconditionalDetermined` determinacy PRESERVED across the
/// migration boundary: `y`'s literal default_expr evaluates to a non-Undef
/// value, so `determined(y)` stays `Bool(true)`.
///
/// Covers only the always-reachable Let-miss path. The impl-3 plan step
/// also names a secondary "wave-2" cone-reeval site (post-solver downstream
/// let re-eval), guarded by reachability — omitted here because reaching it
/// needs a solver-backed engine (`with_solver(..)`), a fixture shape this
/// file's `make_engine()`-only scaffolding doesn't build, and wave-2 shares
/// the exact `commit_cell_result`/`cell_eval_ctx` call shape already
/// exercised by this test and by test-2's `reeval_cone_cell` coverage.
#[test]
fn eval_cached_let_miss_provenance_and_determinacy() {
    let module = parse_and_compile(PLAIN_LET_SRC);
    let mut engine = make_engine();

    let result = engine.eval_cached(&module, VersionId(1));

    let y_id = ValueCellId::new("S", "y");
    let y_det_id = ValueCellId::new("S", "y_det");

    eprintln!("y started_payload = {:?}", started_payload(&engine, &y_id));
    eprintln!("y_det = {:?}", result.eval_result.values.get(&y_det_id));

    assert_eq!(
        started_payload(&engine, &y_id),
        Some("cached-serve".to_string()),
        "eval_cached's Let cache-miss path should carry the 'cached-serve' \
         TraceSource slug once migrated onto commit_cell_result"
    );
    assert_eq!(
        result.eval_result.values.get(&y_det_id),
        Some(&Value::Bool(true)),
        "y should be Determined -> determined(y) = true (UnconditionalDetermined \
         rule, preserved across the migration boundary)"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// test-4: structural-query post-pass explicit CacheLeg::Skip audit
// ─────────────────────────────────────────────────────────────────────────

/// Returns the payload of the LAST `EventKind::Started` event recorded for
/// `id` in `engine`'s journal, IF that payload is `EventPayload::Custom` —
/// mirrors `started_payload` but selects the LAST match rather than the
/// first.
///
/// Needed specifically for post-pass provenance assertions (structural-query
/// / self-datum / annotation-args): a post-pass site's whole purpose is to
/// OVERWRITE a cell the main eval pass already wrote via
/// `evaluate_params_and_lets_unified` / `evaluate_let_bindings` — the
/// dominant, freshness-propagating let/param evaluators that are themselves
/// left unmigrated (documented at impl-7; see `cell_commit.rs`'s "Known
/// scope gaps") and which emit their OWN Started(`payload: None`) event for
/// the same node first. So a post-pass cell always carries (at least) two
/// Started events — the main pass's, then the post-pass's — and
/// `started_payload`'s first-match semantics would only ever observe the
/// (unmigrated, `None`-payload) main-pass event, never the post-pass's own.
/// Confirmed empirically via a scratch probe on `ms` (`STRUCTURAL_QUERY_SRC`):
/// exactly 2 `Started` events, matching this reasoning.
fn last_started_payload(engine: &Engine, id: &ValueCellId) -> Option<String> {
    let node_id = NodeId::Value(id.clone());
    let events = engine.journal().events_for_node(&node_id);
    let started = events
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::Started))?;
    match &started.payload {
        Some(EventPayload::Custom(slug)) => Some(slug.clone()),
        _ => None,
    }
}

/// RED: structural-query post-pass explicit `CacheLeg::Skip` audit.
///
/// RED today: the structural-query post-pass (engine_eval.rs, ~@3894-3903)
/// writes `values`/`snapshot.values` directly and emits NO journal event of
/// its own, so the LAST `Started` event recorded for `ms` is still the main
/// pass's (unmigrated, `None`-payload) one — `None`, not
/// `Some("post-pass-overwrite|cache-skip=structural-query post-pass
/// overwrite")`. GREEN after impl-4 migrates the commit onto
/// `commit_cell_result` with `TraceSource::PostPassOverwrite` and
/// `CacheLeg::Skip("structural-query post-pass overwrite")`, which adds a
/// SECOND, later `Started` event carrying that slug.
///
/// Also pins (characterization guards that must stay green across the
/// migration boundary): the post-pass's own commit never writes/updates the
/// cache leg with its fresh value (a cache entry DOES already exist for
/// `ms` — see the `(2)` comment below for why — so this is checked as
/// "stale, not fresh" rather than "absent"), and the structural-query result
/// is preserved (`ms` = `List([])`).
///
/// `ms_det` (`determined(ms)`) is NOT usable to black-box-observe the
/// post-pass's own committed determinacy: `ms_det`'s default_expr
/// (`determined(ms)`) does not itself contain a structural query, so it is
/// evaluated exactly ONCE, by the main pass, BEFORE the structural-query
/// post-pass runs — reading `ms`'s PRE-expansion state, which is
/// `(Value::Undef, Determined)` (the same main-pass short-circuit
/// `self.xy_plane`-style behaviour documented at the self-datum post-pass
/// below), so `ms_det` is `Bool(false)` — `Determined && !is_undef()` is
/// false because the value is `Undef` — and stays `false` regardless of
/// what the post-pass later does to `ms`. This is verified empirically
/// (unaffected by this migration either way) and pinned below as its own
/// regression guard, distinct from the post-pass's determinacy. The
/// post-pass's own `Determined` stamp is instead verified by construction:
/// `DeterminacyRule::UnconditionalDetermined::resolve` always returns
/// `Determined` regardless of value (exhaustively unit-pinned in
/// `cell_commit.rs`), and impl-4 passes exactly that rule at this site,
/// matching the pre-migration code's hardcoded `DeterminacyState::Determined`.
#[test]
fn structural_query_post_pass_cache_skip_audit() {
    let module = parse_and_compile(STRUCTURAL_QUERY_SRC);
    let mut engine = make_engine();

    let result = engine.eval(&module);

    let ms_id = ValueCellId::new("S", "ms");
    let ms_det_id = ValueCellId::new("S", "ms_det");

    eprintln!("ms last_started_payload = {:?}", last_started_payload(&engine, &ms_id));
    eprintln!("ms = {:?}", result.values.get(&ms_id));
    eprintln!("ms_det = {:?}", result.values.get(&ms_det_id));

    // (1) Provenance + explicit skip marker — RED today (the last Started
    // event is still the main pass's None-payload one).
    assert_eq!(
        last_started_payload(&engine, &ms_id),
        Some("post-pass-overwrite|cache-skip=structural-query post-pass overwrite".to_string()),
        "the structural-query post-pass's Started event should be the LAST \
         one recorded for the cell and should carry the 'post-pass-overwrite' \
         TraceSource slug plus its cache-skip reason once migrated onto \
         commit_cell_result with CacheLeg::Skip"
    );

    // (2) The Skip leg must not write/update the cache entry with the
    // post-pass's fresh value. A cache entry already exists here — the main
    // pass's own (unmigrated, deferred to impl-7) evaluation of this same
    // Let cell writes one first, against the PRE-expansion `Undef` value —
    // so a literal "no cache entry" check does not hold; CacheLeg::Skip's
    // contract is that THIS commit omits the cache leg, leaving whatever
    // was there before untouched. Checking "not the fresh value" is the
    // form of that proof available here; the journal's `cache-skip=` marker
    // asserted in (1) is the authoritative "this commit skipped the cache
    // leg" signal (the plan's journal-recoverable alternative).
    match engine.cache_store().get(&NodeId::Value(ms_id.clone())) {
        None => {}
        Some(entry) => match &entry.result {
            CachedResult::Value(val, _) => assert_ne!(
                *val,
                Value::List(vec![]),
                "the structural-query post-pass's CacheLeg::Skip commit must \
                 not have written its fresh value into the cache leg"
            ),
            other => panic!("expected CachedResult::Value(_, _), got {other:?}"),
        },
    }

    // (3) Value preserved (characterization guard) — the structural-query
    // result is unchanged by the migration.
    assert_eq!(
        result.values.get(&ms_id),
        Some(&Value::List(vec![])),
        "ms should evaluate to an empty list (0 subs) — the structural-query \
         result must be unchanged by the migration"
    );
    // Regression guard on the UNRELATED main-pass short-circuit determinacy
    // (see doc comment above) — must stay false across this migration, since
    // γ does not touch the main-pass evaluator.
    assert_eq!(
        result.values.get(&ms_det_id),
        Some(&Value::Bool(false)),
        "ms_det reads ms's PRE-expansion main-pass state (Undef, Determined), \
         unrelated to and unaffected by the post-pass migration"
    );
}
