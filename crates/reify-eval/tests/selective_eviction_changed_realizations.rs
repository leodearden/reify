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
//! accessor, reached through the self-dev-dep) and `Engine::freshness()`
//! (public and ungated). The pure graph-level unit tests for the compare
//! helper itself stay in-crate, where they need no kernel at all.

use std::collections::HashSet;

use reify_core::{RealizationNodeId, ValueCellId};
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
                graph.realizations.get(rid).unwrap().input_cone_hash.is_some(),
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

    // `rb` is referenced by the premise lock above; bind it here so an
    // accidental future removal of that lock surfaces as an unused warning
    // rather than silently weakening the test.
    let _ = &rb;
}
