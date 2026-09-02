//! selective-realization-eviction β (#4729): the `changed_realizations` set
//! produced by `edit_param` / `edit_source`.
//!
//! # Why these live in `tests/` and not in `engine_edit.rs`'s `mod tests`
//!
//! They drive a REAL OCCT kernel, and an in-crate `--lib` test structurally
//! cannot: `kernel_registry.rs` registers four synthetic `#[cfg(test)]` mock
//! kernels into the process-global `OnceLock` registry, one of which
//! (`__a_kernel`) claims `(PrimitiveBox, BRep)`. Spawning `OcctKernelHandle`
//! alongside them trips the duplicate-claim `debug_assert!` at
//! `kernel_registry.rs:394`. That file says so itself: the synthetics "appear
//! in `cargo test --lib` builds for this crate but are invisible to
//! integration test binaries (which compile the lib without `cfg(test)`)".
//!
//! Nothing is lost by the move — every assertion here reads a PUBLIC surface:
//! `Engine::last_changed_realizations()` (the `test-instrumentation`-gated
//! accessor, reached through the self-dev-dep), `Engine::snapshot()`, and
//! `Engine::last_substantive_value()` (both public and ungated). The pure
//! graph-level unit tests for the compare helper itself stay in-crate, where
//! they need no kernel at all.
//!
//! # Eviction is observed via `last_substantive_value`, never `freshness`
//!
//! `Freshness::default()` is `Final` (value.rs:4444 — task #2326's deliberate
//! "default to Final on read" contract), so an ABSENT cache entry reads back
//! as `Final`. `Engine::freshness` therefore cannot distinguish "evicted"
//! from "fresh", and any eviction assertion built on it is vacuous.
//! `last_substantive_value` returns `None` for an absent entry, which is the
//! actual eviction signal γ (#4730) builds on.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_ir::{ExportFormat, Value};

/// Two independent bodies plus a display-only `let` feeding no realization.
///
/// `body_a`'s box reads `wa`; `body_b`'s reads `wb`. `label_pad` feeds only
/// `pad_display`, so editing it must move NO realization's input cone — the
/// PRD §6 "no-realization edit" boundary row.
const TWO_BODY_SRC: &str = r#"
structure TwoBody {
    param wa: Length = 10mm
    param wb: Length = 20mm
    param label_pad: Length = 3mm

    let body_a = box(wa, 5mm, 5mm)
    let body_b = box(wb, 5mm, 5mm)
    let pad_display = label_pad * 2.0
}
"#;

/// An `Engine` backed by a real OCCT kernel — β's compare-site tests need
/// realizations to actually EXECUTE, because only `execute_realization_ops`
/// writes α's `input_cone_hash`. Mirrors `tests/achieved_repr_tol.rs:60`.
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Resolve the `RealizationNodeId` backing the geometry cell named `member`,
/// via `RealizationNodeData::geometry_cell` (the GHR-δ S2 link) rather than
/// by assuming a declaration-order index — so the test does not silently
/// re-point if lowering order changes.
fn realization_for_cell(
    engine: &reify_eval::Engine,
    entity: &str,
    member: &str,
) -> RealizationNodeId {
    let graph = &engine
        .snapshot()
        .expect("engine must have an installed snapshot after eval()")
        .graph;
    graph
        .realizations
        .iter()
        .find(|(_, n)| {
            n.geometry_cell
                .as_ref()
                .is_some_and(|c| c.entity == entity && c.member == member)
        })
        .map(|(rid, _)| rid.clone())
        .unwrap_or_else(|| {
            panic!(
                "no realization is linked to the geometry cell {entity}.{member}; \
                 realizations present: {:?}",
                graph
                    .realizations
                    .iter()
                    .map(|(r, n)| (r.clone(), n.geometry_cell.clone()))
                    .collect::<Vec<_>>()
            )
        })
}

/// β `edit_param` compare site: editing `wa` must report EXACTLY the
/// realization behind `body_a` as changed — `body_b`'s input cone did not
/// move, so it must be absent.
///
/// This is the selectivity contract in its purest form. A regression that
/// reports BOTH realizations degrades γ's keyed eviction back into the
/// wholesale flush it exists to replace; a regression that reports NEITHER
/// serves stale geometry.
#[test]
fn edit_param_reports_only_the_realization_whose_input_cone_moved() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping edit_param_reports_only_the_realization_whose_input_cone_moved: \
             OCCT not available"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(TWO_BODY_SRC);
    let mut engine = make_occt_engine();
    let _ = engine.eval(&compiled);
    let _ = engine.build(&compiled, ExportFormat::Step);

    let ra = realization_for_cell(&engine, "TwoBody", "body_a");
    let rb = realization_for_cell(&engine, "TwoBody", "body_b");

    // PREMISE LOCK: α's production write must have populated BOTH stored
    // hashes. Without a kernel the realizations never execute, every hash
    // stays `None`, and the §11.2 "None → conservatively changed" arm would
    // make the real assertion below pass for entirely the wrong reason
    // (everything changed, always).
    {
        let graph = &engine.snapshot().unwrap().graph;
        for rid in [&ra, &rb] {
            assert!(
                graph
                    .realizations
                    .get(rid)
                    .unwrap()
                    .input_cone_hash
                    .is_some(),
                "premise: build() must populate {rid}'s input_cone_hash (α's write in \
                 execute_realization_ops). It is None, so every realization would be \
                 conservatively 'changed' and this test could not distinguish selective \
                 from wholesale."
            );
        }
    }

    // ── PRD §6 "no-realization edit" boundary row ──────────────────────
    //
    // Asserted FIRST, straight off the build, and the order is load-bearing.
    // The comparison is against the input cone AS OF THE LAST EXECUTION, so
    // it is cumulative across edits until a build re-executes — see the
    // third assertion below. Only here, with the stored hashes freshly
    // stamped by `build()`, does "no realization moved" mean "the set is
    // empty". `label_pad` feeds only the display-only `pad_display` cell, so
    // β pays nothing for this edit.
    engine
        .edit_param(
            ValueCellId::new("TwoBody", "label_pad"),
            Value::length(0.009),
        )
        .expect("edit_param on TwoBody.label_pad must succeed");

    assert!(
        engine.last_changed_realizations().is_empty(),
        "editing a display-only param that feeds no realization must report an EMPTY \
         changed set (PRD §6 no-realization-edit row), got: {:?}",
        engine.last_changed_realizations()
    );

    // ── The selectivity contract ───────────────────────────────────────
    engine
        .edit_param(ValueCellId::new("TwoBody", "wa"), Value::length(0.030))
        .expect("edit_param on TwoBody.wa must succeed");

    assert_eq!(
        engine.last_changed_realizations(),
        &HashSet::from([ra.clone()]),
        "editing wa must report EXACTLY body_a's realization: body_b's input cone did \
         not move, so including it would be an over-evict that defeats γ's selectivity"
    );
    // Named-subject restatement of the half of the `assert_eq!` above that is
    // easiest to lose to a future re-shaping of the expected set. Also the
    // only remaining use of `rb` outside the premise lock (amend, review
    // round 1 — `test-quality`: the previous `let _ = &rb;` claimed to
    // preserve an unused-variable signal but, being itself a use, was exactly
    // what suppressed it).
    assert!(
        !engine.last_changed_realizations().contains(&rb),
        "body_b's input cone did not move, so it must be absent from the changed set"
    );

    // ── "Changed" is relative to the last EXECUTION, not the last edit ──
    //
    // No build has run since `wa` moved, so body_a's geometry is genuinely
    // still stale. A second display-only edit must therefore CONTINUE to
    // report body_a — dropping it here would let γ skip the eviction of a
    // realization that really does hold outdated geometry, which is exactly
    // the 4317-class stale β exists to prevent. β is read-only w.r.t.
    // `input_cone_hash` precisely so this stays true: only a real execution
    // (α's write in `execute_realization_ops`) may retire the staleness.
    engine
        .edit_param(
            ValueCellId::new("TwoBody", "label_pad"),
            Value::length(0.012),
        )
        .expect("second edit_param on TwoBody.label_pad must succeed");

    assert_eq!(
        engine.last_changed_realizations(),
        &HashSet::from([ra.clone()]),
        "the changed set is measured against the last EXECUTION, not the last edit: \
         body_a was edited but never rebuilt, so it must STILL be reported changed. \
         Reporting an empty set here would let γ skip evicting genuinely stale geometry."
    );

    // And a rebuild retires it: α re-stamps both stored hashes during
    // execution, so the next display-only edit is empty again.
    let _ = engine.build(&compiled, ExportFormat::Step);
    engine
        .edit_param(
            ValueCellId::new("TwoBody", "label_pad"),
            Value::length(0.015),
        )
        .expect("third edit_param on TwoBody.label_pad must succeed");

    assert!(
        engine.last_changed_realizations().is_empty(),
        "after a rebuild re-executes the realizations, α re-stamps their input_cone_hash \
         and a display-only edit must report an EMPTY set again — otherwise the changed \
         set would grow monotonically and γ would degenerate into the wholesale flush. \
         Got: {:?}",
        engine.last_changed_realizations()
    );
}

/// V2 of [`TWO_BODY_SRC`] with ONLY `wa`'s declared default changed.
/// `body_b`'s source and BOTH bodies' geometry-op IR are byte-identical.
const TWO_BODY_SRC_V2: &str = r#"
structure TwoBody {
    param wa: Length = 40mm
    param wb: Length = 20mm
    param label_pad: Length = 3mm

    let body_a = box(wa, 5mm, 5mm)
    let body_b = box(wb, 5mm, 5mm)
    let pad_display = label_pad * 2.0
}
"#;

/// β `edit_source` compare site — and the design §5.2 gap it closes.
///
/// Recompiling with only `wa`'s default literal changed must report body_a's
/// realization as changed and body_b's as not. The load-bearing half is the
/// PREMISE LOCK: it shows that the static `RealizationNodeData::content_hash`
/// — the key `diff_realizations` compares on — is BYTE-IDENTICAL across this
/// recompile, so the pre-β machinery provably could not have seen the change.
///
/// Also pins the carry-forward: `edit_source` rebuilds the graph and
/// `EvaluationGraph::from_templates` reseeds every `input_cone_hash` to
/// `None`, so without an explicit copy every recompile would destroy the
/// "as of the last execution" record and make every realization
/// conservatively changed forever after.
#[test]
fn edit_source_reports_the_moved_input_cone_that_the_static_content_hash_cannot_see() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping edit_source_reports_the_moved_input_cone_that_the_static_content_hash_cannot_see: \
             OCCT not available"
        );
        return;
    }

    let v1 = reify_test_support::parse_and_compile_with_stdlib(TWO_BODY_SRC);
    let v2 = reify_test_support::parse_and_compile_with_stdlib(TWO_BODY_SRC_V2);

    let mut engine = make_occt_engine();
    let _ = engine.eval(&v1);
    let _ = engine.build(&v1, ExportFormat::Step);

    let ra = realization_for_cell(&engine, "TwoBody", "body_a");
    let rb = realization_for_cell(&engine, "TwoBody", "body_b");

    // Capture the OLD graph's state before edit_source replaces it.
    let (old_content_hash_a, old_input_cone_a, old_input_cone_b) = {
        let graph = &engine.snapshot().unwrap().graph;
        let a = graph.realizations.get(&ra).unwrap();
        let b = graph.realizations.get(&rb).unwrap();

        // PREMISE LOCK 1 (same as the edit_param test): α's write ran.
        assert!(
            a.input_cone_hash.is_some() && b.input_cone_hash.is_some(),
            "premise: build() must populate BOTH input_cone_hash values before the \
             recompile, else every realization is conservatively changed and neither \
             the selectivity nor the carry-forward assertion below means anything"
        );
        (a.content_hash, a.input_cone_hash, b.input_cone_hash)
    };

    engine
        .edit_source(&v2)
        .expect("edit_source onto V2 must succeed");

    let graph = &engine.snapshot().unwrap().graph;

    // ── PREMISE LOCK 2: the design §5.2 gap, made explicit ─────────────
    //
    // `diff_realizations` classifies purely by `RealizationNodeData::content_hash`
    // (engine_edit.rs:429 → `diff_nodes(.., |n| n.content_hash)`), which
    // `from_templates` builds as `of_str(id) ⊕ combine_all(of_str(format!("{:?}", op)))`
    // (graph.rs:371-396) — a Debug render of the op IR. `Primitive{Box,
    // args:[("width", ValueRef(wa))]}` renders identically no matter what
    // `wa` evaluates to, so the static hash cannot move here.
    //
    // If this assertion ever FAILS, the fixture no longer demonstrates the
    // gap and the test must be re-shaped (e.g. drive the value change through
    // an upstream `let` instead of a param default). Do NOT weaken it into a
    // tautology — it is the only thing standing between this test and a
    // vacuous pass.
    assert_eq!(
        graph.realizations.get(&ra).unwrap().content_hash,
        old_content_hash_a,
        "premise (design §5.2): body_a's STATIC content_hash must be byte-identical \
         across this recompile — that is exactly why keying eviction on it, as \
         `diff_realizations` does, provably misses a value-driven change. A moved \
         hash here means the fixture stopped demonstrating the gap."
    );

    // ── The real assertion ─────────────────────────────────────────────
    assert!(
        engine.last_changed_realizations().contains(&ra),
        "edit_source must report body_a changed: its INPUT cone moved (wa 10mm → 40mm) \
         even though its static content_hash did not. Got: {:?}",
        engine.last_changed_realizations()
    );
    assert!(
        !engine.last_changed_realizations().contains(&rb),
        "edit_source must NOT report body_b: neither its ops nor its input cone moved. \
         Got: {:?}",
        engine.last_changed_realizations()
    );

    // ── Carry-forward ──────────────────────────────────────────────────
    //
    // `from_templates` seeds every new node's input_cone_hash to `None`
    // (graph.rs:406). Without an explicit copy from the OLD graph, the
    // "as of the last EXECUTION" record dies at every recompile and the very
    // next edit_param conservatively marks EVERY realization changed —
    // defeating γ/δ's "an unaffected body's cache entry survives" boundary
    // case.
    assert_eq!(
        graph.realizations.get(&rb).unwrap().input_cone_hash,
        old_input_cone_b,
        "edit_source must carry the OLD graph's stored input_cone_hash forward onto \
         body_b's persisting node, not leave the `None` that from_templates seeds"
    );
    // The carry-forward reaches body_a too, and for a NARROW reason: body_a's
    // OPS are byte-identical across this recompile (PREMISE LOCK 2 above
    // asserts exactly that), so it is absent from `diff_realizations`'
    // `changed` set and the ops-identical carry-forward still applies. What
    // moved is a VALUE, and a recomputed fold over identical ops catches
    // precisely that. This is NOT "carrying forward is safe even when the
    // inputs moved" — that rationale is false in general, because the fold is
    // blind to an ops-only change (see the in-crate lock
    // `input_cone_fold_is_blind_to_a_boolean_kind_flip` and the sibling test
    // `edit_source_does_not_carry_input_cone_hash_forward_when_ops_changed`).
    assert_eq!(
        graph.realizations.get(&ra).unwrap().input_cone_hash,
        old_input_cone_a,
        "the carry-forward applies to body_a too: its OPS are byte-identical across the \
         recompile (its own PREMISE LOCK 2), so it is not in `changed_realizations` and \
         the ops-identical carry-forward holds. Only the VALUE moved, which the \
         recomputed fold catches."
    );

    // A subsequent display-only edit must now be empty for body_b at least:
    // the carry-forward is what makes that possible.
    engine
        .edit_param(
            ValueCellId::new("TwoBody", "label_pad"),
            Value::length(0.021),
        )
        .expect("edit_param after edit_source must succeed");
    assert!(
        !engine.last_changed_realizations().contains(&rb),
        "after the carry-forward, an edit that touches no realization must leave \
         body_b unreported — if the recompile had wiped its stored hash, the §11.2 \
         `None` arm would mark it changed forever. Got: {:?}",
        engine.last_changed_realizations()
    );
}

// ── The carry-forward hazard: an OPS-only change the fold cannot see ──────

/// Two boxes combined by a boolean, plus an independent `body_c` whose ops are
/// untouched by the V2 flip.
///
/// Sizes are chosen so BOTH boolean kinds are non-degenerate: `body_b` sits
/// strictly inside `body_a`, so `union` is `body_a` and `difference` is
/// `body_a` with an internal void — neither is an empty solid.
const BOOL_SRC: &str = r#"
structure BoolBody {
    param wa: Length = 30mm
    param wb: Length = 10mm
    param wc: Length = 8mm

    let body_a = box(wa, 20mm, 20mm)
    let body_b = box(wb, 5mm, 5mm)
    let combined = union(body_a, body_b)
    let body_c = box(wc, 5mm, 5mm)
}
"#;

/// V2 of [`BOOL_SRC`] differing in EXACTLY one token: `union` → `difference`.
///
/// No param default moves, so every scalar arg the input-cone fold can see is
/// byte-identical across the recompile. `compile_boolean_op` builds
/// `left_ops ++ right_ops ++ [Boolean{op,left,right}]`
/// (geometry_boolean.rs:165-171), so the flip moves ONLY the `op` field of the
/// trailing `Boolean` — which the fold is provably blind to (see the in-crate
/// characterization lock `input_cone_fold_is_blind_to_a_boolean_kind_flip` in
/// `engine_edit.rs`'s `mod tests`).
const BOOL_SRC_V2: &str = r#"
structure BoolBody {
    param wa: Length = 30mm
    param wb: Length = 10mm
    param wc: Length = 8mm

    let body_a = box(wa, 20mm, 20mm)
    let body_b = box(wb, 5mm, 5mm)
    let combined = difference(body_a, body_b)
    let body_c = box(wc, 5mm, 5mm)
}
"#;

/// The `edit_source` carry-forward must NOT apply to a realization whose OPS
/// changed — only to one whose ops are provably identical.
///
/// # The hazard
///
/// The two hashes are complementary and each guards the other's blind spot:
/// `RealizationNodeData::content_hash` sees the op IR but not the values (it is
/// `of_str(id) ⊕ combine_all(of_str(format!("{:?}", op)))`, graph.rs:396), while
/// the input-cone fold sees the values but not the ops (it folds only
/// `(arg_name, value)` pairs and matches `Boolean { .. } => &[]`, in
/// engine_build.rs).
///
/// A blanket carry-forward stamps the OLD execution's input-cone hash onto a
/// node whose ops just changed. The fold recomputed over the NEW ops is then
/// byte-EQUAL to it — the flip is invisible to the fold — so
/// `refresh_and_gate_demanded_realizations` reads `stored == Some(current)`,
/// marks the realization `exempt`, `demand_scoped_unified_pass` filters it out
/// of its `hash_exempt` seed, `execute_realization_ops` is never called — and
/// per the DELTA CONTRACT comment in `demand_scoped_unified_pass` the absent
/// mesh means "retain the previously rendered mesh". The GUI keeps showing the
/// union after the user typed
/// `difference`: a textbook 4317-class stale.
///
/// The fix is a targeted skip, NOT a blanket revert — hence the SELECTIVITY
/// GUARD below.
#[test]
fn edit_source_does_not_carry_input_cone_hash_forward_when_ops_changed() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping edit_source_does_not_carry_input_cone_hash_forward_when_ops_changed: \
             OCCT not available"
        );
        return;
    }

    let v1 = reify_test_support::parse_and_compile_with_stdlib(BOOL_SRC);
    let v2 = reify_test_support::parse_and_compile_with_stdlib(BOOL_SRC_V2);

    let mut engine = make_occt_engine();
    let _ = engine.eval(&v1);
    let _ = engine.build(&v1, ExportFormat::Step);

    let r_combined = realization_for_cell(&engine, "BoolBody", "combined");
    let r_c = realization_for_cell(&engine, "BoolBody", "body_c");

    // Capture the OLD graph's state before edit_source replaces it.
    let (old_content_hash_combined, old_content_hash_c, old_input_cone_c) = {
        let graph = &engine.snapshot().unwrap().graph;
        let combined = graph.realizations.get(&r_combined).unwrap();
        let c = graph.realizations.get(&r_c).unwrap();

        // PREMISE LOCK 1: α's write ran for BOTH. Without a populated stored
        // hash the RED assertion below would be vacuous — `None` is exactly
        // what it asserts, and it would already hold for the wrong reason.
        assert!(
            combined.input_cone_hash.is_some() && c.input_cone_hash.is_some(),
            "premise: build() must populate BOTH input_cone_hash values before the \
             recompile (α's write in execute_realization_ops). combined={:?} body_c={:?}",
            combined.input_cone_hash,
            c.input_cone_hash
        );
        (combined.content_hash, c.content_hash, c.input_cone_hash)
    };

    engine
        .edit_source(&v2)
        .expect("edit_source onto V2 must succeed");

    let graph = &engine.snapshot().unwrap().graph;

    // ── PREMISE LOCK 2: the ops really did change ──────────────────────
    //
    // The mirror image of the sibling test's PREMISE LOCK 2. `content_hash`
    // renders the whole op IR through `Debug`, so `Boolean{op: Union, ..}` and
    // `Boolean{op: Difference, ..}` hash differently. This is what puts
    // `combined` into `diff_realizations`' `changed` set, giving the skip
    // predicate something to key on.
    assert_ne!(
        graph.realizations.get(&r_combined).unwrap().content_hash,
        old_content_hash_combined,
        "premise: combined's STATIC content_hash must MOVE across a union→difference \
         flip — that is what makes it a member of `changed_realizations`. If it did not \
         move, the skip predicate has nothing to key on and this fixture no longer \
         exercises the hazard."
    );

    // ── PREMISE LOCK 3: body_c is genuinely untouched ──────────────────
    assert_eq!(
        graph.realizations.get(&r_c).unwrap().content_hash,
        old_content_hash_c,
        "premise: body_c's ops must be byte-identical across the recompile, so it stays \
         OUT of `changed_realizations` and the selectivity guard below is meaningful"
    );

    // ── THE RED ASSERTION ──────────────────────────────────────────────
    assert_eq!(
        graph.realizations.get(&r_combined).unwrap().input_cone_hash,
        None,
        "combined's OPS changed (union→difference), so its stored input_cone_hash must \
         NOT be carried forward — it must keep the `None` that from_templates seeds \
         (graph.rs:406). Carrying it forward is unsafe precisely because the fold is \
         BLIND to this change: see the in-crate lock \
         `input_cone_fold_is_blind_to_a_boolean_kind_flip` (engine_edit.rs), which pins \
         that a union→difference flip folds to a byte-IDENTICAL hash. With the old hash \
         carried forward, `refresh_and_gate_demanded_realizations` evaluates \
         `stored != Some(current)` as FALSE, exempts the realization, never re-executes \
         it, and the DELTA CONTRACT then retains the previously rendered mesh — stale \
         geometry served for an edit the user made by hand."
    );

    // ── SELECTIVITY GUARD (green before AND after the fix) ─────────────
    //
    // The fix must be a TARGETED skip, not a blanket revert of the
    // carry-forward. Losing body_c's carried hash would re-break γ/δ's
    // "an unaffected body's cache entry survives" boundary case: from_templates
    // reseeds it to `None`, the §11.2 `None` arm marks it conservatively
    // changed forever, and γ's keyed eviction degenerates into the wholesale
    // flush it exists to replace.
    assert_eq!(
        graph.realizations.get(&r_c).unwrap().input_cone_hash,
        old_input_cone_c,
        "body_c's ops are unchanged, so its stored input_cone_hash MUST still be carried \
         forward. A `None` here means the carry-forward was reverted wholesale rather \
         than narrowed to the ops-changed case."
    );
}

// ── The propagation contract: the FIRST production caller of
//    `compute_dirty_cone_with_realizations` ────────────────────────────────

/// Number of times [`probe_fn`] was invoked.
///
/// **Single-test ownership invariant** (the
/// `freshness_pending_compute_dispatch.rs:31` idiom): this static is touched
/// exclusively by `probe_fn`, registered only by
/// [`edit_param_evicts_only_the_compute_node_downstream_of_the_moved_realization`].
/// A second test in THIS MODULE registering a trampoline that calls it would
/// silently corrupt the count. Module privacy is what bounds that risk to this
/// file: `harness_kernel_realization` is a shared compile unit whose sibling
/// modules run in the same process, but neither this static nor `probe_fn` is
/// `pub`, so no sibling can reach either. Reset at test entry as
/// belt-and-braces against the cargo-test process being reused.
static PROBE_INVOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Trivial synthetic `ComputeFn` — counts its invocations and returns a
/// constant. Deliberately kernel-free: no gmsh/FEA adapter is needed to
/// observe the eviction contract.
fn probe_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    PROBE_INVOCATIONS.fetch_add(1, Ordering::SeqCst);
    reify_eval::ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// Two bodies, each read by its own `@optimized` probe. The probes return
/// `Int` (a NON-Geometry type) so each output stays a value cell rather than
/// lowering to a separate realization — the `tests/fixtures/morph_box.ri`
/// shape.
const TWO_BODY_PROBED_SRC: &str = r#"
@optimized("test::sel-evict-probe")
fn probe(g: Geometry) -> Int {
    0
}

structure TwoBodyProbed {
    param wa: Length = 10mm
    param wb: Length = 20mm

    let body_a = box(wa, 5mm, 5mm)
    let body_b = box(wb, 5mm, 5mm)
    let probe_a = probe(body_a)
    let probe_b = probe(body_b)
}
"#;

/// β's propagation half: seeding `compute_dirty_cone_with_realizations` with
/// `changed_realizations` must drop the freshness of the ComputeNode
/// downstream of the MOVED realization — and only that one.
///
/// This is the task's stated user-observable signal, expressed exactly as
/// "observable via its output value cell freshness".
///
/// It is also the test that catches the edge-#10 ordering hazard. If the
/// implementation seeds the walk with a reverse index built BEFORE ComputeNodes
/// were inserted into the graph, `realization_dependents_of` returns nothing,
/// the cone comes back EMPTY, and `probe_a` stays `Final` — the whole
/// propagation half would compile, run, and silently do nothing. Do NOT weaken
/// this to accept an empty cone.
#[test]
fn edit_param_evicts_only_the_compute_node_downstream_of_the_moved_realization() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping edit_param_evicts_only_the_compute_node_downstream_of_the_moved_realization: \
             OCCT not available"
        );
        return;
    }

    PROBE_INVOCATIONS.store(0, Ordering::SeqCst);

    let compiled = reify_test_support::parse_and_compile_with_stdlib(TWO_BODY_PROBED_SRC);
    let mut engine = make_occt_engine();
    engine.register_compute_fn("test::sel-evict-probe", probe_fn);

    // `build()` establishes the eval_state snapshot internally, and a separate
    // `eval()` first would finalise the probe cells — leaving each body's
    // compute node with non-empty realization_inputs so `build()`'s redispatch
    // gate skips it (morph_arm_e2e.rs:186-191).
    engine.build(&compiled, ExportFormat::Step);

    let ra = realization_for_cell(&engine, "TwoBodyProbed", "body_a");
    let rb = realization_for_cell(&engine, "TwoBodyProbed", "body_b");
    let cell_a = ValueCellId::new("TwoBodyProbed", "probe_a");
    let cell_b = ValueCellId::new("TwoBodyProbed", "probe_b");

    // ── PREMISE LOCKS ──────────────────────────────────────────────────
    //
    // Without these the assertions below could pass vacuously: an absent
    // edge #10 makes the cone empty, and a probe cell that was never `Final`
    // trivially satisfies "no longer Final".
    {
        let graph = &engine.snapshot().unwrap().graph;
        let node_for = |r: &RealizationNodeId| {
            graph
                .compute_nodes
                .iter()
                .find(|(_, cn)| cn.realization_inputs.contains(r))
                .map(|(_, cn)| cn.clone())
        };
        for (rid, label) in [(&ra, "body_a"), (&rb, "body_b")] {
            let cn = node_for(rid).unwrap_or_else(|| {
                panic!(
                    "premise: a ComputeNode consuming {label}'s realization must exist with a \
                     NON-EMPTY realization_inputs (edge #10). compute_nodes present: {:?}",
                    graph
                        .compute_nodes
                        .iter()
                        .map(|(id, cn)| (id.clone(), cn.realization_inputs.clone()))
                        .collect::<Vec<_>>()
                )
            });
            assert!(
                !cn.realization_inputs.is_empty(),
                "premise: {label}'s ComputeNode must carry a non-empty realization_inputs"
            );
        }
    }
    for (cell, label) in [(&cell_a, "probe_a"), (&cell_b, "probe_b")] {
        assert!(
            engine
                .last_substantive_value(&NodeId::Value(cell.clone()))
                .is_some(),
            "premise: {label}'s output value cell must hold a cached substantive value \
             after build(), else 'its entry was evicted' below is trivially true"
        );
    }
    assert!(
        PROBE_INVOCATIONS.load(Ordering::SeqCst) >= 2,
        "premise: both probes' trampolines must actually have been dispatched during \
         build() — otherwise the cached values asserted above came from somewhere other \
         than a real ComputeNode dispatch and the eviction assertions below would not be \
         measuring what they claim. Got {} invocation(s).",
        PROBE_INVOCATIONS.load(Ordering::SeqCst)
    );

    // ── The contract ───────────────────────────────────────────────────
    //
    // Eviction is observed through `last_substantive_value`, NOT through
    // `Engine::freshness`. `Freshness::default()` is `Final` (value.rs:4444,
    // task #2326's deliberate "default to Final on read" contract), so an
    // ABSENT cache entry reads as `Final` — freshness cannot distinguish
    // "evicted" from "fresh" and any assertion built on it would be vacuous.
    // `last_substantive_value` returns `None` for an absent entry, which is
    // exactly the eviction signal γ (#4730) builds on.
    engine
        .edit_param(
            ValueCellId::new("TwoBodyProbed", "wa"),
            Value::length(0.030),
        )
        .expect("edit_param on TwoBodyProbed.wa must succeed");

    assert!(
        engine
            .last_substantive_value(&NodeId::Value(cell_a.clone()))
            .is_none(),
        "body_a's input cone moved, so the realization dirty cone must reach the \
         ComputeNode consuming it and evict its output cell's cache entry. An EMPTY \
         cone — the edge-#10 ordering hazard, i.e. a reverse index built before \
         ComputeNodes were inserted — leaves the entry in place and lands here."
    );
    assert!(
        engine
            .last_substantive_value(&NodeId::Value(cell_b.clone()))
            .is_some(),
        "body_b's input cone did NOT move, so its consumer must RETAIN its cached \
         result. Losing it here means β over-evicted and degraded into the wholesale \
         flush it exists to replace."
    );

    // ── The S12 cross-kind cascade, now live on the param path ─────────
    //
    // (Amend, review round 1 — `architecture`.) The realization-keyed reverse
    // map carries more than edge #10: the GHR-δ S4 Realization→geometry-cell
    // edge means the moved realization's own `Value::GeometryHandle` cell is
    // dropped too. On `edit_source` that duplicates the deliberate S12 cascade
    // at `engine_edit.rs`'s step (9); on `edit_param` this is the first time
    // it runs, so it is asserted rather than left implicit.
    let geom_a = ValueCellId::new("TwoBodyProbed", "body_a");
    let geom_b = ValueCellId::new("TwoBodyProbed", "body_b");
    assert!(
        engine
            .last_substantive_value(&NodeId::Value(geom_a.clone()))
            .is_none(),
        "the S12 cascade must drop the geometry cell backed by the moved realization — \
         its handle points at geometry built from the OLD wa"
    );

    // …and the drop is RECOVERABLE, which is the whole reason S12 invalidates
    // rather than force-re-evaluating: the incremental edit path cannot re-run
    // the kernel, so a forced re-eval would strip the handle to `Undef` for
    // good. A subsequent build() re-executes the realization and the cell
    // resolves to a real handle again.
    engine.build(&compiled, ExportFormat::Step);
    for (cell, label) in [(&geom_a, "body_a"), (&geom_b, "body_b")] {
        let v = engine
            .snapshot()
            .expect("engine must have a snapshot after build()")
            .values
            .get(cell)
            .map(|(v, _)| v.clone());
        assert!(
            matches!(v, Some(Value::GeometryHandle { .. })),
            "after the rebuild, {label}'s cell must resolve to a real GeometryHandle, not \
             `Undef` — the S12 drop is recoverable by re-execution (S16 lazy revalidation \
             covers the read-before-rebuild window). Got: {v:?}"
        );
    }
}

/// V2 of [`TWO_BODY_PROBED_SRC`] with ONLY `wa`'s declared default changed —
/// the probe function, both bodies' geometry-op IR and `wb` are all
/// byte-identical.
const TWO_BODY_PROBED_SRC_V2: &str = r#"
@optimized("test::sel-evict-probe")
fn probe(g: Geometry) -> Int {
    0
}

structure TwoBodyProbed {
    param wa: Length = 40mm
    param wb: Length = 20mm

    let body_a = box(wa, 5mm, 5mm)
    let body_b = box(wb, 5mm, 5mm)
    let probe_a = probe(body_a)
    let probe_b = probe(body_b)
}
"#;

/// Invocation counter for [`probe_fn_src`].
///
/// A SECOND static, deliberately not shared with [`PROBE_INVOCATIONS`]: that
/// one documents a single-test ownership invariant, and cargo runs the two
/// tests concurrently in the same process, so sharing it would make both
/// premise locks race. Same module-privacy bound applies.
static PROBE_INVOCATIONS_SRC: AtomicUsize = AtomicUsize::new(0);

/// [`probe_fn`]'s twin for the `edit_source` propagation test, counting into
/// [`PROBE_INVOCATIONS_SRC`].
fn probe_fn_src(
    _value_inputs: &[Value],
    _realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    PROBE_INVOCATIONS_SRC.fetch_add(1, Ordering::SeqCst);
    reify_eval::ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// The `edit_source` sibling of
/// [`edit_param_evicts_only_the_compute_node_downstream_of_the_moved_realization`]
/// (amend, review round 1 — `test-coverage`).
///
/// `edit_source` is the RISKIER of the two propagation call sites and had no
/// eviction coverage at all: its `edit_source` tests asserted only
/// `last_changed_realizations` membership and the `input_cone_hash`
/// carry-forward. The specific hazard is that the site hands
/// `&new_snapshot.graph` to the index builder on the premise that per-cell
/// eval has repopulated `new_snapshot.graph.compute_nodes` by that point,
/// while `engine_edit.rs`'s step (9)/(9b) comments both state that map "is
/// always empty at this stage". If ComputeNode recreation writes to a
/// different snapshot than the one handed to the builder, the cone is
/// silently EMPTY, no test previously failed, and γ inherits a no-op.
///
/// Do NOT weaken the `probe_a` assertion to accept a surviving entry: an
/// empty cone is exactly what lands here.
#[test]
fn edit_source_evicts_only_the_compute_node_downstream_of_the_moved_realization() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping edit_source_evicts_only_the_compute_node_downstream_of_the_moved_realization: \
             OCCT not available"
        );
        return;
    }

    PROBE_INVOCATIONS_SRC.store(0, Ordering::SeqCst);

    let v1 = reify_test_support::parse_and_compile_with_stdlib(TWO_BODY_PROBED_SRC);
    let v2 = reify_test_support::parse_and_compile_with_stdlib(TWO_BODY_PROBED_SRC_V2);
    let mut engine = make_occt_engine();
    engine.register_compute_fn("test::sel-evict-probe", probe_fn_src);

    // Same build()-without-a-prior-eval() shape as the edit_param sibling —
    // see its comment for why a separate eval() would defeat the premise.
    engine.build(&v1, ExportFormat::Step);

    let ra = realization_for_cell(&engine, "TwoBodyProbed", "body_a");
    let rb = realization_for_cell(&engine, "TwoBodyProbed", "body_b");
    let cell_a = ValueCellId::new("TwoBodyProbed", "probe_a");
    let cell_b = ValueCellId::new("TwoBodyProbed", "probe_b");

    // ── PREMISE LOCKS (identical in kind to the edit_param sibling) ─────
    {
        let graph = &engine.snapshot().unwrap().graph;
        for (rid, label) in [(&ra, "body_a"), (&rb, "body_b")] {
            let cn = graph
                .compute_nodes
                .iter()
                .find(|(_, cn)| cn.realization_inputs.contains(rid))
                .map(|(_, cn)| cn.clone())
                .unwrap_or_else(|| {
                    panic!(
                        "premise: a ComputeNode consuming {label}'s realization must exist with \
                         a NON-EMPTY realization_inputs (edge #10). compute_nodes present: {:?}",
                        graph
                            .compute_nodes
                            .iter()
                            .map(|(id, cn)| (id.clone(), cn.realization_inputs.clone()))
                            .collect::<Vec<_>>()
                    )
                });
            assert!(
                !cn.realization_inputs.is_empty(),
                "premise: {label}'s ComputeNode must carry a non-empty realization_inputs"
            );
        }
    }
    for (cell, label) in [(&cell_a, "probe_a"), (&cell_b, "probe_b")] {
        assert!(
            engine
                .last_substantive_value(&NodeId::Value(cell.clone()))
                .is_some(),
            "premise: {label}'s output value cell must hold a cached substantive value \
             after build(), else 'its entry was evicted' below is trivially true"
        );
    }
    assert!(
        PROBE_INVOCATIONS_SRC.load(Ordering::SeqCst) >= 2,
        "premise: both probes must actually have been dispatched during build(). Got {} \
         invocation(s).",
        PROBE_INVOCATIONS_SRC.load(Ordering::SeqCst)
    );

    // ── The contract ───────────────────────────────────────────────────
    engine
        .edit_source(&v2)
        .expect("edit_source onto V2 must succeed");

    assert!(
        engine.last_changed_realizations().contains(&ra),
        "premise: the compare site must have classified body_a as changed — without that \
         the propagation half has no seed and the assertion below would be measuring the \
         classification bug, not the propagation one. Got: {:?}",
        engine.last_changed_realizations()
    );
    assert!(
        engine
            .last_substantive_value(&NodeId::Value(cell_a.clone()))
            .is_none(),
        "body_a's input cone moved across the recompile, so edit_source's realization \
         dirty cone must reach the ComputeNode consuming it and evict its output cell's \
         cache entry. A surviving entry here is the silently-empty-cone failure: an index \
         built while `compute_nodes` was still empty carries no edge #10."
    );
    assert!(
        engine
            .last_substantive_value(&NodeId::Value(cell_b.clone()))
            .is_some(),
        "body_b's ops and input cone are both unchanged, so its consumer must RETAIN its \
         cached result — otherwise edit_source over-evicted and β degraded into the \
         wholesale flush it exists to replace."
    );
}
