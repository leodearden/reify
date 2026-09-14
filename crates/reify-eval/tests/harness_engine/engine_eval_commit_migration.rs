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

use reify_core::DiagnosticCode;
use reify_core::ValueCellId;
use reify_core::VersionId;
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::journal::{EventKind, EventPayload};
use reify_ir::{DeterminacyState, ErrorRef, Freshness, Value};
use reify_test_support::{make_engine, mm, parse_and_compile, parse_and_compile_with_stdlib};

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
/// A sibling `let` probes `determined(..)` on it (see the doc comment on
/// `structural_query_post_pass_cache_skip_audit` for why this probe reads
/// the PRE-post-pass main-pass state, not the post-pass's own commit).
/// Kernel-free: `self.xy_plane` resolves to the intrinsic identity-frame
/// `xy` plane constant with no geometry kernel needed.
///
/// Verified (scratch run, prerequisite pre-1): compiles+evals cleanly with
/// `p` = a concrete `Value::Plane { .. }` (not `Undef`) and no diagnostics.
const SELF_DATUM_SRC: &str = r#"
    structure S {
        let p = self.xy_plane
        let p_det = determined(p)
    }
"#;

/// `@test_eval(2.0 * 1.5)` on `AnnoItem` — drives the annotation-args
/// materialization post-pass's SUCCESS arm (engine_eval.rs, ~@4926-4936): the
/// `S.it` cell (a `Value::StructureInstance`) is rebuilt with the
/// materialized `test_eval` overlay attached and re-committed. `@test_eval`
/// is a globally-registered test-only annotation schema (one
/// `AtMaterialization` arg named `value: Real`) — see
/// `crates/reify-compiler/src/annotations/schema.rs` and the existing
/// precedent in `tests/annotation_materialization_eval.rs`
/// (`eval_annotation_smoke_attaches_overlay`).
///
/// Verified (scratch run): compiles+evals cleanly; `S.it` is a
/// `Value::StructureInstance` of type `AnnoItem` whose
/// `annotation("test_eval").arg_value("value")` overlay is `Real(3.0)`.
const ANNOTATION_ARGS_SUCCESS_SRC: &str = r#"
@test_eval(2.0 * 1.5) structure def AnnoItem {
    param dummy : Real = 0
}
structure S {
    let it = AnnoItem()
}
"#;

/// `@test_eval(1.0 > 0.0)` on `BadAnnoItem` — a `Bool` result against the
/// schema's expected `Real` — drives the annotation-args materialization
/// post-pass's FAILURE arm (engine_eval.rs, ~@4937-4947): the `BadS.it` cell
/// is replaced with `Value::Undef` and an `AnnotationEvalFailed` diagnostic
/// is emitted. Mirrors
/// `eval_annotation_type_mismatch_emits_failed_diagnostic_and_undef_cell` in
/// `tests/annotation_materialization_eval.rs`.
///
/// Verified (scratch run): compiles+evals cleanly; `BadS.it` is
/// `Value::Undef` with one `AnnotationEvalFailed` diagnostic.
const ANNOTATION_ARGS_FAILURE_SRC: &str = r#"
@test_eval(1.0 > 0.0) structure def BadAnnoItem {
    param dummy : Real = 0
}
structure BadS {
    let it = BadAnnoItem()
}
"#;

/// Acceptance parity fixture for test-7: a plain defaulted `Length` param
/// (`x`) with a `DeterminacyPredicate` probe directly on it (`dp`), a plain
/// derived `let` (`s = x + 1mm`) with its own sibling probe (`s_det`), and a
/// plain (non-guarded) no-default `Int` param (`x_rejected`) with its own
/// sibling probe (`ro_det`). A test sets an incompatible-type-kind override
/// on `x_rejected` (a `Length` `Value::Scalar` against its `Int` cell_type)
/// BEFORE eval, so the rejected-override-no-default arm fires (`Value::Undef`,
/// `DeterminacyState::Undetermined`).
///
/// `x_rejected` is deliberately NOT placed inside a `where` guard here (unlike
/// `GUARDED_GROUP_SRC`'s `x_rejected`, which drives the migrated
/// `eval_guarded_group_param_cell` and is already exhaustively covered,
/// provenance + determinacy, by `guarded_group_param_provenance_and_determinacy`
/// / test-1): `template.guarded_groups` is a separate collection from
/// `template.value_cells` that ONLY `engine.eval()`'s cold "third pass"
/// iterates (engine_eval.rs ~@3392) — `eval_cached`'s unified Param+Let pass
/// (`build_combined_param_let_graph`) never visits it at all, so a `where`-
/// guarded cell referenced by a sibling `determined(..)` probe is simply
/// absent from `eval_cached`'s determinacy snapshot, panicking
/// ("not in determinacy snapshot — wiring bug or eval-order violation") — a
/// genuine, pre-existing eval_cached capability gap unrelated to γ (confirmed
/// empirically; see the escalate_info filed alongside this task). A plain,
/// non-guarded rejected-override Param exercises the SAME "rejected-override,
/// no-default" determinacy outcome (B2's third named cell kind) on both cold
/// and warm without depending on guarded-group support, which `eval_cached`
/// does not have.
///
/// Verified (scratch run): compiles+evals cleanly under both `engine.eval()`
/// and a fresh `engine.eval_cached(..., VersionId(1))`, with `dp` =
/// `Bool(true)`, `s_det` = `Bool(true)`, `ro_det` = `Bool(false)` on both.
const PARITY_FIXTURE_SRC: &str = r#"
    structure S {
        param x : Length = 5mm
        let dp = determined(x)
        let s = x + 1mm
        let s_det = determined(s)

        param x_rejected : Int
        let ro_det = determined(x_rejected)
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

// ─────────────────────────────────────────────────────────────────────────
// test-5: self-datum projection post-pass explicit CacheLeg::Skip audit
// ─────────────────────────────────────────────────────────────────────────

/// RED: self-datum projection post-pass explicit `CacheLeg::Skip` audit.
/// Mirrors `structural_query_post_pass_cache_skip_audit`'s shape exactly —
/// same reasoning applies for both `last_started_payload` (the self-datum
/// post-pass also runs after the main pass's own unmigrated evaluation of
/// the same Let cell) and `p_det` (also evaluated once, early, against `p`'s
/// PRE-projection-rewrite state).
///
/// RED today: the self-datum post-pass (engine_eval.rs, ~@3984-3993) writes
/// `values`/`snapshot.values` directly and emits NO journal event of its
/// own, so the LAST `Started` event recorded for `p` is still the main
/// pass's `None`-payload one. GREEN after impl-5 migrates the commit onto
/// `commit_cell_result` with `TraceSource::PostPassOverwrite` and
/// `CacheLeg::Skip("self-datum projection overwrite")`.
#[test]
fn self_datum_projection_post_pass_cache_skip_audit() {
    let module = parse_and_compile(SELF_DATUM_SRC);
    let mut engine = make_engine();

    let result = engine.eval(&module);

    let p_id = ValueCellId::new("S", "p");
    let p_det_id = ValueCellId::new("S", "p_det");

    eprintln!("p last_started_payload = {:?}", last_started_payload(&engine, &p_id));
    eprintln!("p = {:?}", result.values.get(&p_id));
    eprintln!("p_det = {:?}", result.values.get(&p_det_id));

    // (1) Provenance + explicit skip marker — RED today.
    assert_eq!(
        last_started_payload(&engine, &p_id),
        Some("post-pass-overwrite|cache-skip=self-datum projection overwrite".to_string()),
        "the self-datum post-pass's Started event should be the LAST one \
         recorded for the cell and should carry the 'post-pass-overwrite' \
         TraceSource slug plus its cache-skip reason once migrated onto \
         commit_cell_result with CacheLeg::Skip"
    );

    // (2) The Skip leg must not write/update the cache entry with the
    // post-pass's fresh value (see the equivalent structural-query comment
    // above for why a literal is_none() check does not hold here either).
    match engine.cache_store().get(&NodeId::Value(p_id.clone())) {
        None => {}
        Some(entry) => match &entry.result {
            CachedResult::Value(val, _) => assert!(
                !matches!(val, Value::Plane { .. }),
                "the self-datum post-pass's CacheLeg::Skip commit must not \
                 have written its fresh Plane value into the cache leg, got {val:?}"
            ),
            other => panic!("expected CachedResult::Value(_, _), got {other:?}"),
        },
    }

    // (3) Value preserved (characterization guard) — the self-datum
    // projection result is a concrete Plane, unchanged by the migration.
    assert!(
        matches!(result.values.get(&p_id), Some(Value::Plane { .. })),
        "p should evaluate to a concrete Value::Plane (the self-datum \
         projection result must be unchanged by the migration), got {:?}",
        result.values.get(&p_id)
    );
    // Regression guard on the UNRELATED main-pass short-circuit determinacy
    // (mirrors ms_det) — must stay false across this migration.
    assert_eq!(
        result.values.get(&p_det_id),
        Some(&Value::Bool(false)),
        "p_det reads p's PRE-rewrite main-pass state (Undef, Determined), \
         unrelated to and unaffected by the post-pass migration"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// test-6: annotation-args materialization post-pass explicit CacheLeg::Skip
// audit (success + failure arms)
// ─────────────────────────────────────────────────────────────────────────

/// RED: annotation-args materialization post-pass SUCCESS arm explicit
/// `CacheLeg::Skip` audit.
///
/// RED today: the post-pass's success arm (engine_eval.rs, ~@4926-4936)
/// writes `values`/`snapshot.values` directly and emits NO journal event of
/// its own, so the LAST `Started` event recorded for `it` is still the main
/// pass's (unmigrated, `None`-payload) one. GREEN after impl-6 migrates the
/// commit onto `commit_cell_result` with `TraceSource::PostPassOverwrite`
/// and `CacheLeg::Skip("annotation-args materialization overlay")`.
///
/// Also pins (characterization guards that must stay green across the
/// migration boundary): the post-pass's own commit never writes/updates the
/// cache leg with its fresh (annotation-overlaid) value — checked as "the
/// cached instance, if any, lacks the test_eval overlay" rather than
/// "absent", mirroring test-4/5's reasoning (a cache entry already exists
/// here from the main pass's own unmigrated evaluation of `it`, BEFORE the
/// overlay is attached) — and the rebuilt instance + its materialized
/// overlay are preserved (`it` is a `Value::StructureInstance` of type
/// `AnnoItem` whose `test_eval` overlay is `Real(3.0)`).
///
/// No `determined(it)` sibling probe here (see test-4/5's doc comments for
/// why that pattern can't observe a post-pass's own commit) — the rebuilt
/// instance + overlay assertions already fully characterize the value.
#[test]
fn annotation_args_materialization_success_cache_skip_audit() {
    let module = parse_and_compile(ANNOTATION_ARGS_SUCCESS_SRC);
    let mut engine = make_engine();

    let result = engine.eval(&module);

    let it_id = ValueCellId::new("S", "it");

    eprintln!("it last_started_payload = {:?}", last_started_payload(&engine, &it_id));
    eprintln!("it = {:?}", result.values.get(&it_id));

    // (1) Provenance + explicit skip marker — RED today.
    assert_eq!(
        last_started_payload(&engine, &it_id),
        Some(
            "post-pass-overwrite|cache-skip=annotation-args materialization overlay"
                .to_string()
        ),
        "the annotation-args post-pass's success-arm Started event should be \
         the LAST one recorded for the cell and should carry the \
         'post-pass-overwrite' TraceSource slug plus its cache-skip reason \
         once migrated onto commit_cell_result with CacheLeg::Skip"
    );

    // (2) The Skip leg must not write/update the cache entry with the
    // post-pass's fresh (overlay-attached) value.
    match engine.cache_store().get(&NodeId::Value(it_id.clone())) {
        None => {}
        Some(entry) => match &entry.result {
            CachedResult::Value(Value::StructureInstance(data), _) => assert!(
                data.annotation("test_eval").is_none(),
                "the annotation-args post-pass's CacheLeg::Skip commit must \
                 not have written its overlay-attached instance into the \
                 cache leg"
            ),
            other => panic!(
                "expected CachedResult::Value(StructureInstance(_), _), got {other:?}"
            ),
        },
    }

    // (3) Value preserved (characterization guard) — the rebuilt instance +
    // its materialized overlay are unchanged by the migration.
    let it_val = result.values.get(&it_id).unwrap_or_else(|| {
        panic!(
            "S.it cell not found; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    let data = match it_val {
        Value::StructureInstance(d) => d,
        other => panic!("expected S.it to be Value::StructureInstance, got {:?}", other),
    };
    assert_eq!(data.type_name, "AnnoItem");
    let overlay = data
        .annotation("test_eval")
        .and_then(|a| a.arg_value("value"))
        .cloned();
    assert_eq!(
        overlay,
        Some(Value::Real(3.0)),
        "the test_eval overlay (2.0 * 1.5) must be preserved across the migration"
    );
}

/// RED: annotation-args materialization post-pass FAILURE arm explicit
/// `CacheLeg::Skip` audit.
///
/// RED today: the post-pass's failure arm (engine_eval.rs, ~@4937-4947)
/// writes `values`/`snapshot.values` directly and emits NO journal event of
/// its own. GREEN after impl-6 migrates the commit onto `commit_cell_result`
/// with the same `TraceSource::PostPassOverwrite` +
/// `CacheLeg::Skip("annotation-args materialization overlay")` as the
/// success arm.
///
/// Also pins: an `AnnotationEvalFailed` diagnostic is emitted (unaffected by
/// this migration — mirrors the existing
/// `eval_annotation_type_mismatch_emits_failed_diagnostic_and_undef_cell`
/// precedent in `tests/annotation_materialization_eval.rs`), and the failed
/// cell's value is replaced with `Value::Undef` (characterization guard).
#[test]
fn annotation_args_materialization_failure_cache_skip_audit() {
    let module = parse_and_compile(ANNOTATION_ARGS_FAILURE_SRC);
    let mut engine = make_engine();

    let result = engine.eval(&module);

    let it_id = ValueCellId::new("BadS", "it");

    eprintln!("it last_started_payload = {:?}", last_started_payload(&engine, &it_id));
    eprintln!("it = {:?}", result.values.get(&it_id));

    // (1) Provenance + explicit skip marker — RED today.
    assert_eq!(
        last_started_payload(&engine, &it_id),
        Some(
            "post-pass-overwrite|cache-skip=annotation-args materialization overlay"
                .to_string()
        ),
        "the annotation-args post-pass's failure-arm Started event should be \
         the LAST one recorded for the cell and should carry the \
         'post-pass-overwrite' TraceSource slug plus its cache-skip reason \
         once migrated onto commit_cell_result with CacheLeg::Skip"
    );

    // (2) The failure diagnostic is emitted (unaffected by this migration).
    let has_failed_diag = result
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::AnnotationEvalFailed));
    assert!(
        has_failed_diag,
        "expected a DiagnosticCode::AnnotationEvalFailed diagnostic; got: {:?}",
        result.diagnostics
    );

    // (3) Value preserved (characterization guard) — the cell is replaced
    // with Value::Undef on failure, unchanged by the migration.
    assert_eq!(
        result.values.get(&it_id),
        Some(&Value::Undef),
        "BadS.it must be Value::Undef after the annotation-args materialization failure"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// test-7: acceptance parity fixture (B2) + consolidated CacheLeg::Skip audit (B3)
// ─────────────────────────────────────────────────────────────────────────

/// Acceptance parity fixture — the task's named user-observable signal, the
/// ratified B2/B3 split per esc-5053-2 Option A (B1, cached-path
/// warning-resurfaces, is out of γ's scope; owned downstream by μ #5062).
///
/// B2 — determinacy identical cold-vs-warm: `engine.eval(&m)` (cold — reaches
/// `dp`/`s`/`s_det` via the main-pass Param/Let evaluator, whose main LET
/// commit task #5238 has since migrated onto `commit_cell_result` while its
/// Param arm still writes directly, and `x_rejected`/`ro_det` via the migrated
/// `eval_guarded_group_param_cell`, impl-1) and a FRESH
/// `engine.eval_cached(&m, VersionId(1))` (warm — reaches
/// ALL FOUR cells via `eval_cached`'s own separately-implemented
/// unified Param+Let pass — see engine_eval.rs's "Unified single-pass
/// evaluation of Param+Let cells" doc comment; it never calls
/// `eval_guarded_group_param_cell`) must compute IDENTICAL determinacy for
/// all three named cell kinds:
///   - `dp` (a `DeterminacyPredicate` directly on a defaulted Param) -> `Bool(true)`
///   - `s_det` (probe on a plain derived Let) -> `Bool(true)`
///   - `ro_det` (probe on a rejected-override, no-default Param) -> `Bool(false)`
///
/// This is a characterization guard that must stay green through the whole
/// migration: cold and warm reach every cell through genuinely different
/// code (confirmed by reading both call paths), so parity here proves the
/// migration changed no externally observable determinacy outcome on
/// EITHER path.
///
/// B2 (task #5238) — FRESHNESS identical cold-vs-warm for a freshness-
/// PROPAGATED let: the task's headline done-criterion, asserted on `S.s`
/// (`let s = x + 1mm`), the fixture's only let that READS another cell and so
/// genuinely propagates freshness. `PLAIN_LET_SRC`'s `let y = 5mm` reads
/// nothing, which would make a "propagated" claim vacuous there.
///
/// KNOWN LIMIT — do not read this as broader coverage than it is. Parity
/// holds here because `x` is `Final`, so BOTH sides land on `Final`; but they
/// DERIVE it differently. Cold goes through the migrated
/// `evaluate_params_and_lets_unified` -> `CacheLeg::RecordPropagating {
/// still_refining: false }`, i.e. genuine arch §7.2 propagation from the
/// just-computed trace (`Engine::evaluate_params_and_lets_unified`'s main-let
/// commit). Warm goes through the `eval_cached` Let cache-MISS arm's
/// `CacheLeg::Record` -> `record_evaluation`, which hard-codes
/// `Freshness::Final`. A let with a genuinely
/// non-`Final` input WOULD therefore diverge cold-vs-warm. That divergence is
/// a PRE-EXISTING property of the warm cache-miss arm inherited from γ
/// (#5053), not a regression introduced here, and migrating that arm off
/// `CacheLeg::Record` is out of #5238's scope. The `started_payload` assertion
/// below pins that the cold side really is on the `RecordPropagating` path, so
/// the parity is measured where the task claims it.
///
/// The task's other named freshness case — a let whose freshness is NOT
/// `Final` — is covered by
/// `eval_cached_let_reserve_preserves_freshness_and_commits` (test-4), which
/// injects `Freshness::Failed` and proves the migrated preserve-freshness
/// re-serve carries it forward verbatim instead of resetting it to `Final`.
///
/// B3 — consolidated `CacheLeg::Skip` audit: re-asserts, in one place, that
/// each of the three post-pass sites migrated at impl-4/5/6 (structural-query,
/// self-datum projection, annotation-args materialization) carries its
/// `|cache-skip=<reason>` marker on the LAST Started event for its cell, and
/// that the Skip leg did not overwrite the pre-existing cache entry with its
/// fresh value. Cold-only (`engine.eval()`) — none of these three post-passes
/// run during `eval_cached`. Mirrors test-4/5/6's "stale, not absent" cache
/// check exactly: a cache entry already exists from each cell's own
/// preceding, unmigrated main-pass evaluation, so "no cache entry exists for
/// it" does not hold literally — the journal's cache-skip marker is the
/// authoritative "this commit skipped the cache leg" signal (see those
/// tests' doc comments for the full reasoning, empirically reconfirmed here).
#[test]
fn acceptance_parity_fixture_and_consolidated_cache_skip_audit() {
    // ── B2: determinacy identical cold-vs-warm ──────────────────────────
    let module = parse_and_compile(PARITY_FIXTURE_SRC);
    let x_rejected_id = ValueCellId::new("S", "x_rejected");
    let dp_id = ValueCellId::new("S", "dp");
    let s_det_id = ValueCellId::new("S", "s_det");
    let ro_det_id = ValueCellId::new("S", "ro_det");

    let mut cold_engine = make_engine();
    cold_engine.set_param_and_invalidate(&x_rejected_id, mm(5.0));
    let cold_result = cold_engine.eval(&module);

    let mut warm_engine = make_engine();
    warm_engine.set_param_and_invalidate(&x_rejected_id, mm(5.0));
    let warm_result = warm_engine.eval_cached(&module, VersionId(1));

    eprintln!(
        "cold dp/s_det/ro_det = {:?}/{:?}/{:?}",
        cold_result.values.get(&dp_id),
        cold_result.values.get(&s_det_id),
        cold_result.values.get(&ro_det_id)
    );
    eprintln!(
        "warm dp/s_det/ro_det = {:?}/{:?}/{:?}",
        warm_result.eval_result.values.get(&dp_id),
        warm_result.eval_result.values.get(&s_det_id),
        warm_result.eval_result.values.get(&ro_det_id)
    );

    for (name, id, expected) in [
        ("dp", &dp_id, Value::Bool(true)),
        ("s_det", &s_det_id, Value::Bool(true)),
        ("ro_det", &ro_det_id, Value::Bool(false)),
    ] {
        assert_eq!(
            cold_result.values.get(id),
            Some(&expected),
            "{name} should be {expected:?} on cold engine.eval()"
        );
        assert_eq!(
            warm_result.eval_result.values.get(id),
            Some(&expected),
            "{name} should be {expected:?} on a FRESH engine.eval_cached() (warm), \
             identical to the cold result"
        );
    }

    // ── B2 (task #5238): FRESHNESS identical cold-vs-warm for a
    //    freshness-propagated let ──────────────────────────────────────────
    //
    // Reuses the cold/warm engines already evaluated above — no re-eval.
    // `S.s` (`let s = x + 1mm`) is this fixture's genuinely freshness-
    // PROPAGATING let: it READS param `x`, so its freshness is derived from a
    // dependency trace rather than minted from nothing (contrast
    // `PLAIN_LET_SRC`'s `let y = 5mm`, which reads nothing — a "propagated"
    // assertion there would be vacuous).
    let s_id = ValueCellId::new("S", "s");
    let s_node = NodeId::Value(s_id.clone());

    // Existence guard FIRST: `CacheStore::freshness` returns
    // `Freshness::default()` == `Final` for an ABSENT node, so without this
    // the parity assertion below could pass by mutual absence.
    assert!(
        cold_engine.cache_store().get(&s_node).is_some(),
        "cold engine must hold a cache entry for S.s — otherwise the freshness \
         parity assertion below would pass vacuously (freshness() defaults to Final)"
    );
    assert!(
        warm_engine.cache_store().get(&s_node).is_some(),
        "warm engine must hold a cache entry for S.s — otherwise the freshness \
         parity assertion below would pass vacuously (freshness() defaults to Final)"
    );

    let cold_s_freshness = cold_engine.cache_store().freshness(&s_node);
    let warm_s_freshness = warm_engine.cache_store().freshness(&s_node);

    assert_eq!(
        cold_s_freshness, warm_s_freshness,
        "task #5238 acceptance criterion: freshness for the freshness-propagated \
         let S.s must be IDENTICAL cold (engine.eval() -> migrated \
         evaluate_params_and_lets_unified, CacheLeg::RecordPropagating) vs warm \
         (engine.eval_cached()); cold={cold_s_freshness:?} warm={warm_s_freshness:?}"
    );
    assert_eq!(
        cold_s_freshness,
        Freshness::Final,
        "both sides must be Freshness::Final specifically — pinning the value as \
         well as the parity, so a future regression making BOTH sides equally \
         wrong still fails this test"
    );

    // Proves the cold side genuinely went through the migrated
    // `evaluate_params_and_lets_unified` -> `CacheLeg::RecordPropagating`
    // commit (impl-2), rather than the parity above holding vacuously via some
    // unmigrated path.
    assert_eq!(
        started_payload(&cold_engine, &s_id).as_deref(),
        Some("cold-eval"),
        "cold S.s must carry the migrated main-pass provenance slug, proving the \
         freshness parity above is measured on the RecordPropagating path"
    );

    // ── B3: consolidated CacheLeg::Skip audit across the 3 post-pass sites ──

    // structural-query post-pass (impl-4).
    {
        let module = parse_and_compile(STRUCTURAL_QUERY_SRC);
        let mut engine = make_engine();
        engine.eval(&module);
        let ms_id = ValueCellId::new("S", "ms");
        assert_eq!(
            last_started_payload(&engine, &ms_id),
            Some(
                "post-pass-overwrite|cache-skip=structural-query post-pass overwrite"
                    .to_string()
            ),
            "structural-query post-pass: LAST Started event for ms should carry its \
             cache-skip marker"
        );
        match engine.cache_store().get(&NodeId::Value(ms_id.clone())) {
            None => {}
            Some(entry) => match &entry.result {
                CachedResult::Value(val, _) => assert_ne!(
                    *val,
                    Value::List(vec![]),
                    "structural-query post-pass's CacheLeg::Skip commit must not have \
                     written its fresh value into the cache leg"
                ),
                other => panic!("expected CachedResult::Value(_, _), got {other:?}"),
            },
        }
    }

    // self-datum projection post-pass (impl-5).
    {
        let module = parse_and_compile(SELF_DATUM_SRC);
        let mut engine = make_engine();
        engine.eval(&module);
        let p_id = ValueCellId::new("S", "p");
        assert_eq!(
            last_started_payload(&engine, &p_id),
            Some("post-pass-overwrite|cache-skip=self-datum projection overwrite".to_string()),
            "self-datum post-pass: LAST Started event for p should carry its cache-skip marker"
        );
        match engine.cache_store().get(&NodeId::Value(p_id.clone())) {
            None => {}
            Some(entry) => match &entry.result {
                CachedResult::Value(val, _) => assert!(
                    !matches!(val, Value::Plane { .. }),
                    "self-datum post-pass's CacheLeg::Skip commit must not have written \
                     its fresh Plane value into the cache leg, got {val:?}"
                ),
                other => panic!("expected CachedResult::Value(_, _), got {other:?}"),
            },
        }
    }

    // annotation-args materialization post-pass, success arm (impl-6).
    {
        let module = parse_and_compile(ANNOTATION_ARGS_SUCCESS_SRC);
        let mut engine = make_engine();
        engine.eval(&module);
        let it_id = ValueCellId::new("S", "it");
        assert_eq!(
            last_started_payload(&engine, &it_id),
            Some(
                "post-pass-overwrite|cache-skip=annotation-args materialization overlay"
                    .to_string()
            ),
            "annotation-args post-pass: LAST Started event for it should carry its \
             cache-skip marker"
        );
        match engine.cache_store().get(&NodeId::Value(it_id.clone())) {
            None => {}
            Some(entry) => match &entry.result {
                CachedResult::Value(Value::StructureInstance(data), _) => assert!(
                    data.annotation("test_eval").is_none(),
                    "annotation-args post-pass's CacheLeg::Skip commit must not have \
                     written its overlay-attached instance into the cache leg"
                ),
                other => panic!(
                    "expected CachedResult::Value(StructureInstance(_), _), got {other:?}"
                ),
            },
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// task #5238 — dominant main let/param + preserve-freshness re-serve migrations
//
// These tests EXTEND the γ (#5053) migration into the freshness dimension:
// the two main let evaluators (`evaluate_params_and_lets_unified`,
// `evaluate_let_bindings`) and the `eval_cached` preserve-freshness re-serves
// route their per-cell commit through `commit_cell_result` via the new
// freshness-carrying `CacheLeg` variants (`RecordPropagating` /
// `RecordWithFreshness`, cell_commit.rs). See
// `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.4/§7.2 and esc-5053-3.
// ─────────────────────────────────────────────────────────────────────────

/// RED: `evaluate_params_and_lets_unified`'s main let commit provenance +
/// freshness preserved.
///
/// RED today: the cold main-pass let evaluator (`evaluate_params_and_lets_unified`,
/// engine_eval.rs — reached by `engine.eval()`, NOT `eval_cached`) emits its
/// `Started` journal event with `payload: None` and writes the cache leg via a
/// direct `record_evaluation_propagating_freshness(val, Determined, false)`
/// call, so `started_payload(&engine, &y_id)` is `None`, not `Some("cold-eval")`.
/// GREEN after impl-2 migrates the commit onto `commit_cell_result` with
/// `TraceSource::ColdEval` + `CacheLeg::RecordPropagating { still_refining:
/// false }` (removing the superseded manual `Started(None)`/`Completed` pair so
/// `commit_cell_result`'s `Started` is the first — and only — one observed).
///
/// Characterization guards (must stay green BEFORE and after the migration —
/// it is behaviour-preserving in the value/determinacy/freshness dimensions):
/// `y`'s cache freshness stays `Freshness::Final` (all-`Final` inputs +
/// `still_refining: false` derive `Final`, both pre-migration via
/// `record_evaluation_propagating_freshness(val, Determined, false)` and after
/// via `CacheLeg::RecordPropagating { still_refining: false }`), and
/// `determined(y)` stays `Bool(true)` (`UnconditionalDetermined` preserved).
#[test]
fn params_and_lets_main_let_provenance_and_freshness() {
    let module = parse_and_compile(PLAIN_LET_SRC);
    let mut engine = make_engine();

    // Cold eval reaches evaluate_params_and_lets_unified (NOT eval_cached).
    let result = engine.eval(&module);

    let y_id = ValueCellId::new("S", "y");
    let y_det_id = ValueCellId::new("S", "y_det");

    eprintln!("y started_payload = {:?}", started_payload(&engine, &y_id));
    eprintln!(
        "y freshness = {:?}",
        engine.cache_store().freshness(&NodeId::Value(y_id.clone()))
    );
    eprintln!("y_det = {:?}", result.values.get(&y_det_id));

    // (1) Provenance — RED today (the main let path's Started payload is None).
    assert_eq!(
        started_payload(&engine, &y_id),
        Some("cold-eval".to_string()),
        "evaluate_params_and_lets_unified's main let commit should carry the \
         'cold-eval' TraceSource slug once migrated onto commit_cell_result"
    );

    // (2) Freshness preserved (characterization guard — green throughout).
    assert_eq!(
        engine.cache_store().freshness(&NodeId::Value(y_id.clone())),
        Freshness::Final,
        "y's cache freshness must stay Final (all-Final inputs, still_refining=false \
         derive Final) — preserved across the RecordPropagating migration"
    );

    // (3) Determinacy preserved (characterization guard — green throughout).
    assert_eq!(
        result.values.get(&y_det_id),
        Some(&Value::Bool(true)),
        "y should be Determined -> determined(y) = true (UnconditionalDetermined \
         rule, preserved across the migration boundary)"
    );
}

/// RED: `eval_cached`'s Let preserve-freshness re-serve routes through
/// `commit_cell_result` (an atomic 4-leg commit) instead of emitting a bare
/// `CacheHit`, AND preserves the cache entry's non-`Final` freshness across the
/// re-serve.
///
/// This EXTENDS the file's acceptance parity fixture (test-7,
/// `acceptance_parity_fixture_and_consolidated_cache_skip_audit`) into the
/// FRESHNESS dimension: the task's named acceptance signal is "freshness
/// identical cold-vs-warm for a freshness-propagated let". Here a `Failed`
/// freshness is injected onto a cached let after a first warm pass, and a
/// second warm pass (version bump; the cell is not dirty and basis(1) !=
/// version(2) so it is NOT the fast-path) must re-serve it through the migrated
/// commit while carrying that injected freshness forward verbatim.
///
/// RED signal (journal shape): today the un-migrated Let re-serve
/// (`Engine::eval_cached`'s Let cache-reuse block) writes `values`/`snapshot` + a direct
/// `record_evaluation_with_freshness` and records a single
/// `EventKind::CacheHit`, so the LAST journal event recorded for `y` after v2
/// is `CacheHit`. GREEN after impl-4 routes the re-serve onto
/// `commit_cell_result` (`CacheLeg::RecordWithFreshness(preserved)`), which
/// emits a `Started`/`Completed` pair — so the LAST event becomes `Completed`.
///
/// The journal KIND of the last event — `Completed`, vs today's bare
/// `CacheHit` — is the RED signal. As of the #5238 review amendment the
/// provenance slug is ALSO discriminating: the v1 Let cache-MISS (already
/// γ-migrated, `eval_cached_let_miss_provenance_and_determinacy`) stamps
/// `TraceSource::CachedServe` (`cached-serve`) while the migrated re-serve
/// stamps the distinct `TraceSource::CachedReuse` (`cached-reuse`), so
/// assertion (4) below pins the re-serve as the last writer from the journal
/// alone — without reaching for the in-memory `CacheLeg`, which
/// `commit_cell_result` consumes and never records.
///
/// Parity/characterization guards (green BEFORE and after the migration — the
/// re-serve is behaviour-preserving in the value/determinacy/freshness
/// dimensions): `y`'s cache freshness stays the injected `Freshness::Failed`
/// (the re-serve carries `entry.freshness` forward, it does not clobber it to
/// `Final`), and `y`'s committed value + determinacy are identical to the v1
/// warm pass (`determined(y)` stays `Bool(true)`).
///
/// Freshness is injected via `CacheStore::mark_failed` — NOT `set_freshness`,
/// which asserts against `Failed`/`Pending` and directs callers to
/// `mark_failed`/`mark_pending`. `mark_failed` flips only the entry's freshness
/// (leaving its cached value and dirty state untouched), so the v2 re-serve
/// still fires.
#[test]
fn eval_cached_let_reserve_preserves_freshness_and_commits() {
    let module = parse_and_compile(PLAIN_LET_SRC);
    let mut engine = make_engine();

    let y_id = ValueCellId::new("S", "y");
    let y_det_id = ValueCellId::new("S", "y_det");
    let y_node = NodeId::Value(y_id.clone());

    // (v1) Populate y's cache entry via a first warm pass.
    let v1 = engine.eval_cached(&module, VersionId(1));
    let v1_y_value = v1.eval_result.values.get(&y_id).cloned();
    assert!(
        v1_y_value.is_some(),
        "y should have a committed value after the v1 warm pass"
    );
    assert_eq!(
        v1.eval_result.values.get(&y_det_id),
        Some(&Value::Bool(true)),
        "determined(y) should be true after the v1 warm pass (baseline)"
    );

    // Inject a non-Final freshness onto y's cache entry. mark_failed (NOT
    // set_freshness, which panics on Failed/Pending) sets freshness = Failed
    // without touching the cached value or the dirty state, so the v2 re-serve
    // still fires and must carry this freshness forward.
    let err = ErrorRef::new("injected");
    let injected = Freshness::Failed { error: err.clone() };
    assert!(
        engine.cache_store_mut().mark_failed(&y_node, err),
        "mark_failed should find y's cache entry and flip its freshness"
    );
    assert_eq!(
        engine.cache_store().freshness(&y_node),
        injected,
        "sanity: the injected Failed freshness is in place before v2"
    );

    // (v2) Version bump; y is not dirty and basis(1) != version(2) so it is not
    // the fast-path → the Let preserve-freshness re-serve fires.
    let v2 = engine.eval_cached(&module, VersionId(2));

    // (1) RED signal — the re-serve's LAST journal event for y is `Completed`
    // (today it is a single `CacheHit`). Captured in a scoped block so the
    // journal borrow is released before the cache_store() reads below.
    let (last_is_completed, last_kind_dbg) = {
        let events = engine.journal().events_for_node(&y_node);
        let last = events
            .last()
            .expect("y should have at least one journal event after v2");
        (
            matches!(last.kind, EventKind::Completed { .. }),
            format!("{:?}", last.kind),
        )
    };
    eprintln!("y last journal event after v2 = {last_kind_dbg}");
    assert!(
        last_is_completed,
        "the Let preserve-freshness re-serve's LAST journal event for y should \
         be Completed once migrated onto commit_cell_result's atomic 4-leg \
         commit (today it is a single CacheHit), got {last_kind_dbg}"
    );

    // (2) Freshness preserved (characterization guard — green throughout): the
    // re-serve carries the injected Failed freshness forward, not clobbered to
    // Final. This is the freshness-dimension acceptance signal.
    assert_eq!(
        engine.cache_store().freshness(&y_node),
        injected,
        "the Let re-serve must preserve y's injected Failed freshness across v2 \
         (not reset it to Final)"
    );

    // (3) Value + determinacy identical warm-vs-warm (characterization guards —
    // the re-serve preserves the committed value and Determined determinacy).
    assert_eq!(
        v2.eval_result.values.get(&y_id),
        v1_y_value.as_ref(),
        "y's committed value must be unchanged from the v1 warm pass"
    );
    assert_eq!(
        v2.eval_result.values.get(&y_det_id),
        Some(&Value::Bool(true)),
        "determined(y) must stay true across the re-serve (determinacy preserved)"
    );

    // (4) Provenance is self-describing FROM THE JOURNAL ALONE: the re-serve's
    // Started slug is `cached-reuse`, distinct from the v1 cache-MISS arm's
    // `cached-serve`. Without the split, all three commit sites inside
    // eval_cached would stamp one slug and a §2.6 divergence audit could not
    // attribute this event to a producing path.
    assert_eq!(
        last_started_payload(&engine, &y_id),
        Some("cached-reuse".to_string()),
        "the Let preserve-freshness re-serve must stamp the distinct \
         'cached-reuse' provenance slug, not the cache-MISS arm's 'cached-serve'"
    );
}

/// A plain defaulted `Length` param (`w`) with a sibling `determined(..)` probe
/// (`w_det`) — drives the `eval_cached` Param preserve-freshness re-serve
/// (`Engine::eval_cached`'s Param cache-reuse block). The Param-dimension
/// analogue of `PLAIN_LET_SRC`:
/// `w` survives a version bump not-dirty (no override ever set, entry present),
/// so a second warm pass re-serves it through the preserve-freshness path
/// rather than re-evaluating.
const PLAIN_PARAM_SRC: &str = r#"
    structure S {
        param w : Length = 5mm
        let w_det = determined(w)
    }
"#;

/// RED: `eval_cached`'s Param preserve-freshness re-serve routes through
/// `commit_cell_result` (an atomic 4-leg commit) instead of emitting a bare
/// `CacheHit`, AND preserves the cache entry's non-`Final` freshness across the
/// re-serve.
///
/// The Param analogue of `eval_cached_let_reserve_preserves_freshness_and_commits`
/// (test-4), extending the file's parity fixture into the FRESHNESS dimension
/// on the Param path: a `Pending` freshness is injected onto a cached, defaulted
/// param after a first warm pass, and a second warm pass (version bump; the cell
/// is not overridden, not dirty, and basis(1) != version(2) so it is NOT the
/// fast-path) must re-serve it through the migrated commit while carrying that
/// injected freshness forward verbatim.
///
/// RED signal (journal shape): today the un-migrated Param re-serve
/// (`Engine::eval_cached`'s Param cache-reuse block) writes `values`/`snapshot` + a direct
/// `record_evaluation_with_freshness` and records a single `EventKind::CacheHit`,
/// so the LAST journal event recorded for `w` after v2 is `CacheHit`. GREEN after
/// impl-5 routes the re-serve onto `commit_cell_result`
/// (`CacheLeg::RecordWithFreshness(preserved)`), which emits a `Started`/
/// `Completed` pair — so the LAST event becomes `Completed`.
///
/// The journal KIND of the last event — `Completed`, vs today's bare `CacheHit`
/// — is the RED signal (mirrors test-4). As of the #5238 review amendment the
/// provenance slug also discriminates: the migrated re-serve stamps
/// `TraceSource::CachedReuse` (`cached-reuse`), distinct from the cache-MISS
/// arm's `TraceSource::CachedServe`, pinned by assertion (4) below.
///
/// Assertion (5) pins the property the whole determinacy-reproduction argument
/// exists to protect: the re-serve reproduces the stored `(value, determinacy)`
/// pair byte-for-byte, so `record_evaluation_with_freshness` stays on its
/// content-hash EARLY-CUTOFF branch — the only branch that preserves the
/// entry's `pending_cause`. Injecting the Pending via `mark_pending_with_cause`
/// (rather than the bare `mark_pending`) makes that cause observable, so a
/// future determinacy-rule drift that knocked the re-serve onto the Changed
/// branch would DROP the chain-cause and fail here instead of passing silently.
///
/// Parity/characterization guards (green BEFORE and after the migration — the
/// re-serve is behaviour-preserving in the value/determinacy/freshness
/// dimensions): `w`'s cache freshness stays the injected `Freshness::Pending`
/// (the re-serve carries `entry.freshness` forward, it does not clobber it to
/// `Final`), and `w`'s stored `(value, Determined)` determinacy is preserved so
/// `determined(w)` stays `Bool(true)`.
///
/// Freshness is injected via `CacheStore::mark_pending` — NOT `set_freshness`,
/// which asserts against `Pending`/`Failed` and directs callers to
/// `mark_pending`/`mark_failed` (the same constraint test-4 hit with `Failed`).
/// `mark_pending` flips only the entry's freshness to `Pending { last_substantive:
/// ResultRef::of_hash(result_hash) }` (leaving its cached value and dirty state
/// untouched), so the v2 re-serve still fires; the exact injected `Pending` value
/// is captured by reading it back rather than constructed, since its
/// `last_substantive` is derived internally.
#[test]
fn eval_cached_param_reserve_commits_and_preserves() {
    let module = parse_and_compile(PLAIN_PARAM_SRC);
    let mut engine = make_engine();

    let w_id = ValueCellId::new("S", "w");
    let w_det_id = ValueCellId::new("S", "w_det");
    let w_node = NodeId::Value(w_id.clone());

    // (v1) Populate w's cache entry via a first warm pass.
    let v1 = engine.eval_cached(&module, VersionId(1));
    let v1_w_value = v1.eval_result.values.get(&w_id).cloned();
    assert!(
        v1_w_value.is_some(),
        "w should have a committed value after the v1 warm pass"
    );
    assert_eq!(
        v1.eval_result.values.get(&w_det_id),
        Some(&Value::Bool(true)),
        "determined(w) should be true after the v1 warm pass (baseline)"
    );

    // Inject a non-Final (Pending) freshness onto w's cache entry.
    // mark_pending_with_cause (NOT set_freshness, which panics on
    // Pending/Failed) sets freshness = Pending { last_substantive:
    // ResultRef::of_hash(result_hash) } AND records a `pending_cause`, without
    // touching the cached value or the dirty state, so the v2 re-serve still
    // fires and must carry both forward. The cause is what makes assertion (5)
    // able to observe which `record_evaluation_with_freshness` branch ran: the
    // early-cutoff branch preserves `pending_cause`, the Changed branch resets
    // it to `None`. The exact Pending value is read back (its last_substantive
    // is derived internally), not constructed.
    let cause_node = NodeId::Value(w_det_id.clone());
    assert!(
        engine
            .cache_store_mut()
            .mark_pending_with_cause(&w_node, cause_node.clone()),
        "mark_pending_with_cause should find w's cache entry and flip its freshness"
    );
    let injected = engine.cache_store().freshness(&w_node);
    assert!(
        matches!(injected, Freshness::Pending { .. }),
        "sanity: the injected Pending freshness is in place before v2, got {injected:?}"
    );

    // (v2) Version bump; w is not overridden, not dirty, and basis(1) !=
    // version(2) so it is not the fast-path → the Param preserve-freshness
    // re-serve fires.
    let v2 = engine.eval_cached(&module, VersionId(2));

    // (1) RED signal — the re-serve's LAST journal event for w is `Completed`
    // (today it is a single `CacheHit`). Scoped so the journal borrow is
    // released before the cache_store() reads below.
    let (last_is_completed, last_kind_dbg) = {
        let events = engine.journal().events_for_node(&w_node);
        let last = events
            .last()
            .expect("w should have at least one journal event after v2");
        (
            matches!(last.kind, EventKind::Completed { .. }),
            format!("{:?}", last.kind),
        )
    };
    eprintln!("w last journal event after v2 = {last_kind_dbg}");
    assert!(
        last_is_completed,
        "the Param preserve-freshness re-serve's LAST journal event for w should \
         be Completed once migrated onto commit_cell_result's atomic 4-leg \
         commit (today it is a single CacheHit), got {last_kind_dbg}"
    );

    // (2) Freshness preserved (characterization guard — green throughout): the
    // re-serve carries the injected Pending freshness forward, not clobbered to
    // Final. This is the freshness-dimension acceptance signal.
    assert_eq!(
        engine.cache_store().freshness(&w_node),
        injected,
        "the Param re-serve must preserve w's injected Pending freshness across v2 \
         (not reset it to Final)"
    );

    // (3) Value + determinacy identical warm-vs-warm (characterization guards —
    // the re-serve preserves the committed value and Determined determinacy).
    assert_eq!(
        v2.eval_result.values.get(&w_id),
        v1_w_value.as_ref(),
        "w's committed value must be unchanged from the v1 warm pass"
    );
    assert_eq!(
        v2.eval_result.values.get(&w_det_id),
        Some(&Value::Bool(true)),
        "determined(w) must stay true across the re-serve (determinacy preserved)"
    );

    // (4) Provenance is self-describing FROM THE JOURNAL ALONE (see test-4's
    // assertion (4) for why the slug had to be split off `cached-serve`).
    assert_eq!(
        last_started_payload(&engine, &w_id),
        Some("cached-reuse".to_string()),
        "the Param preserve-freshness re-serve must stamp the distinct \
         'cached-reuse' provenance slug, not the cache-MISS arm's 'cached-serve'"
    );

    // (5) EARLY-CUTOFF branch pinned: `pending_cause` survives the re-serve.
    // Only `record_evaluation_with_freshness`' content-hash early-cutoff branch
    // leaves `pending_cause` in place; its Changed branch resets it to `None`.
    // The re-serve rides that branch ONLY because it reproduces the stored
    // `(value, determinacy)` pair exactly — so this assertion is the guard that
    // a future determinacy-rule drift (which would change the content hash)
    // cannot silently drop the Pending chain-cause.
    assert_eq!(
        engine
            .cache_store()
            .get(&w_node)
            .expect("w's cache entry must survive the re-serve")
            .pending_cause
            .as_ref(),
        Some(&cause_node),
        "the Param re-serve must stay on record_evaluation_with_freshness' \
         early-cutoff branch, which preserves pending_cause — a reset to None \
         means the reproduced (value, determinacy) pair no longer content-hash \
         matches the stored entry"
    );
}

/// Construction-only geometry source whose selector `let` (`top_face`) stays
/// `Value::Undef` on the kernel-less value-eval surface, so it is stored in the
/// cache as `(Undef, Undetermined)` rather than `(_, Determined)`.
///
/// Why it lands `Undetermined`: `b = box(..)` first evaluates to `Undef` and is
/// then resolved by the R3d (#4900) in-walk symbolic mint, which enrolls it in
/// `minted_in_walk`. The R3e (#4907) post-mint re-eval pass therefore re-commits
/// `b`'s same-pass consumer `top_face` through `reeval_cone_cell`, whose
/// `DeterminacyRule::DeriveFromValue` maps its still-`Undef` value to
/// `DeterminacyState::Undetermined`. That makes the main let evaluators'
/// `UnconditionalDetermined` NOT the only determinacy a Let cache entry can
/// carry into the `eval_cached` preserve-freshness re-serve.
const UNDETERMINED_LET_SRC: &str = r#"structure def ConstructionOnly {
    let b        = box(10mm, 20mm, 30mm)
    let zdir     = vec3(0.0, 0.0, 1.0)
    let tol      = 1deg
    let top_face = single(faces_by_normal(b, zdir, tol))
}"#;

/// Reads the `DeterminacyState` stored in `engine`'s cache entry for `node`,
/// or `None` if there is no entry or it is not a `CachedResult::Value`.
fn cached_determinacy(engine: &Engine, node: &NodeId) -> Option<DeterminacyState> {
    match &engine.cache_store().get(node)?.result {
        CachedResult::Value(_, det) => Some(*det),
        _ => None,
    }
}

/// REGRESSION (task #5238): the `eval_cached` Let preserve-freshness re-serve
/// must reproduce the cache entry's stored determinacy EXACTLY, including
/// `Undetermined`.
///
/// The impl-4 migration of that re-serve onto `commit_cell_result` originally
/// hard-coded `DeterminacyRule::UnconditionalDetermined` behind a
/// `debug_assert_eq!(det, Determined)` justified by "every Let commit path
/// stamps `UnconditionalDetermined`". That premise is false — `reeval_cone_cell`
/// commits with `DeriveFromValue` (see `UNDETERMINED_LET_SRC`'s doc) — so an
/// `Undetermined` Let entry reaching the re-serve was silently UPGRADED to
/// `Determined` (a debug-build panic, and a silent determinacy corruption in
/// release). The fix selects the rule from the stored `det`, mirroring the
/// sibling Param re-serve.
///
/// Non-vacuity: an injected `Freshness::Failed` is asserted to survive v2. Only
/// the preserve-freshness re-serve can carry it — every other path that could
/// write `top_face` on v2 (the Let cache-MISS arm, `reeval_cone_cell`) uses
/// `CacheLeg::Record`, which hard-codes `Freshness::Final`. So a still-`Failed`
/// freshness pins that the migrated re-serve really was the last writer, making
/// the determinacy assertion a genuine guard rather than a pass-by-absence.
#[test]
fn let_reserve_preserves_undetermined_determinacy() {
    let module = parse_and_compile_with_stdlib(UNDETERMINED_LET_SRC);
    let mut engine = make_engine();

    let top_face_id = ValueCellId::new("ConstructionOnly", "top_face");
    let top_face_node = NodeId::Value(top_face_id.clone());

    // (v1) Populate the cache via a first warm pass.
    engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        cached_determinacy(&engine, &top_face_node),
        Some(DeterminacyState::Undetermined),
        "fixture precondition: the unresolved selector let `top_face` must be \
         cached as Undetermined after v1 — if this fires, the fixture no longer \
         exercises a non-Determined Let re-serve and must be repaired"
    );

    // Inject a non-Final freshness so the re-serve is identifiable as the last
    // writer on v2 (mark_failed, not set_freshness — the latter panics on
    // Failed/Pending; same constraint as test-4).
    let err = ErrorRef::new("injected");
    let injected = Freshness::Failed { error: err.clone() };
    assert!(
        engine.cache_store_mut().mark_failed(&top_face_node, err),
        "mark_failed should find top_face's cache entry and flip its freshness"
    );

    // (v2) Version bump; top_face is not dirty and basis(1) != version(2), so
    // the Let preserve-freshness re-serve fires.
    engine.eval_cached(&module, VersionId(2));

    assert_eq!(
        engine.cache_store().freshness(&top_face_node),
        injected,
        "non-vacuity: the injected Failed freshness must survive v2, pinning the \
         preserve-freshness re-serve as top_face's last writer"
    );
    assert_eq!(
        cached_determinacy(&engine, &top_face_node),
        Some(DeterminacyState::Undetermined),
        "the Let re-serve must reproduce the stored Undetermined determinacy, \
         not upgrade it to Determined"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// #5238 review amendment: journal Started→terminal PAIRING regression
// ─────────────────────────────────────────────────────────────────────────

/// Three-cell chain with a panic-injectable leaf, driving every let sub-path
/// that emits a terminal journal event WITHOUT routing through
/// `commit_cell_result`: `a` takes the panic-recovery `EventKind::Failed`
/// path, and `b`/`c` take the arch §7.2/§9.2 pre-eval Pending gate
/// (`Completed { Unchanged }`).
const PAIRING_CHAIN_SRC: &str = r#"
    structure S {
        param seed : Length = 1mm
        let a = seed + 1mm
        let b = a + 1mm
        let c = b + 1mm
    }
"#;

/// Asserts that `id`'s journal events strictly ALTERNATE
/// `Started` → terminal (`Completed` | `Failed`), starting with `Started`.
///
/// This is the invariant #5238 nearly lost: both let evaluators used to emit
/// one shared `Started` at the TOP of the loop body, covering every exit.
/// Migrating the main success-path commit onto `commit_cell_result` (which
/// emits its own slug-carrying `Started`) required deleting that shared event
/// — which, on its own, would leave every OTHER exit emitting a terminal event
/// with no paired `Started`. `record_subpath_started` re-pairs them lazily.
fn assert_started_terminal_pairing(engine: &Engine, id: &ValueCellId, label: &str) {
    let node_id = NodeId::Value(id.clone());
    let events = engine.journal().events_for_node(&node_id);
    let shape: Vec<&'static str> = events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::Started => Some("Started"),
            EventKind::Completed { .. } => Some("Completed"),
            EventKind::Failed { .. } => Some("Failed"),
            _ => None,
        })
        .collect();
    assert!(
        !shape.is_empty(),
        "{label}: expected at least one Started/terminal journal event"
    );
    for (i, kind) in shape.iter().enumerate() {
        let expected_started = i % 2 == 0;
        assert_eq!(
            *kind == "Started",
            expected_started,
            "{label}: journal events must alternate Started -> terminal, \
             starting with Started; got {shape:?} (offending index {i})"
        );
    }
    assert_eq!(
        shape.len() % 2,
        0,
        "{label}: every Started must have a paired terminal event; got {shape:?}"
    );
}

/// REGRESSION (#5238 review amendment): the let sub-paths that emit a terminal
/// journal event without routing through `commit_cell_result` must still emit
/// a paired `Started` event.
///
/// Covers two of the non-migrated sub-paths end-to-end:
///   - the panic-recovery `EventKind::Failed` path (`a`, the panicking leaf);
///   - the arch §7.2/§9.2 pre-eval Pending gate's `Completed { Unchanged }`
///     (`b`/`c`, quieted downstream of the failure) — whose own in-code
///     rationale, "so the journal still records the visit", presupposes the
///     `Started` this test pins.
///
/// Pass 1 (cold, all-Final) establishes the paired baseline for every cell;
/// pass 2 (panic injected on `a`) drives the sub-paths. Asserting strict
/// alternation across BOTH passes is what makes this non-vacuous: a missing
/// pass-2 `Started` shows up as two consecutive terminal events. Verified RED
/// by neutering `record_subpath_started` — `a`'s shape degrades to
/// `["Started", "Completed", "Failed"]`.
#[test]
fn non_migrated_let_subpaths_emit_paired_started_events() {
    let module = parse_and_compile(PAIRING_CHAIN_SRC);
    let mut engine = make_engine();

    let a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");
    let c_id = ValueCellId::new("S", "c");

    // Pass 1: cold baseline — every cell commits through commit_cell_result,
    // which emits its own Started/Completed pair.
    let _ = engine.eval(&module);
    assert_started_terminal_pairing(&engine, &a_id, "pass 1: a");
    assert_started_terminal_pairing(&engine, &b_id, "pass 1: b");
    assert_started_terminal_pairing(&engine, &c_id, "pass 1: c");

    // Pass 2: panic on `a` — a takes the panic-recovery Failed path, and the
    // pre-eval Pending gate quiets b and c with Completed { Unchanged }.
    engine.set_panic_on_eval(a_id.clone());
    let _ = engine.eval(&module);

    for (id, label) in [
        (&a_id, "pass 2: a"),
        (&b_id, "pass 2: b"),
        (&c_id, "pass 2: c"),
    ] {
        assert_started_terminal_pairing(&engine, id, label);
    }

    // Non-vacuity: pass 2 really did add events for the quieted downstream
    // cell (otherwise the alternation above would hold trivially on pass 1's
    // events alone).
    let c_events = engine
        .journal()
        .events_for_node(&NodeId::Value(c_id.clone()));
    let c_started = c_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Started))
        .count();
    assert!(
        c_started >= 2,
        "non-vacuity: c must have a Started event from EACH pass (pass 1's \
         commit_cell_result commit + pass 2's lazily-paired gate Started); \
         got {c_started}"
    );
}
