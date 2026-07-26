//! Characterization pin (task ι / #5069): the load-bearing build↔tessellate
//! per-build-state reset asymmetry that `Engine::reset_per_build_state` MUST
//! preserve.
//!
//! `build()` populates `realization_handles` (the GHR-δ validity oracle) and
//! does NOT touch `achieved_repr_tol`; `tessellate_realizations()` is the
//! converse — it populates `achieved_repr_tol` and MUST leave
//! `realization_handles` INTACT. The CLI combined-constraint arm
//! (`reify-cli/src/main.rs`) production-depends on this: it runs `build(Step)`
//! then `tessellate_realizations()` then `check()`, and `check()` reads BOTH
//! maps (`measure_gdt_conformance` / `measure_dfm_rules` read
//! `realization_handles`; the `RepresentationWithin` interception reads
//! `achieved_repr_tol`). A naive full-union reset that made `tessellate` clear
//! `realization_handles` would silently degrade every `Conforms`/DFM verdict
//! to Indeterminate/skipped.
//!
//! This pin is GREEN on the pre-refactor code; it locks the behavior the
//! `reset_per_build_state` refactor must keep byte-identical. Hermetic
//! (`MockGeometryKernel`) — no OCCT needed, since the assertion is about map
//! POPULATION/PRESERVATION, not measured mesh deviation.

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_core::identity::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

/// Geometry-bearing fixture: `geometry` is a `Solid` realization reading a
/// scalar param, so `build()` populates `realization_handles` with its
/// resolved handle (modeled on `geometry_handle_freshness.rs::WIDGET_SRC`).
const WIDGET_SRC: &str = r#"structure def Widget {
    param width : Length = 10mm
    param geometry : Solid = box(width, 20mm, 30mm)
}"#;

fn dispatch_lockstep(engine: &Engine) {
    assert_eq!(
        engine.last_dispatch_count(),
        engine
            .last_dispatch_count_by_realization()
            .values()
            .sum::<usize>(),
        "dispatch tally lockstep (aggregate == sum(by_realization)) must hold: \
         both tallies are reset together at every build/tessellate surface entry"
    );
}

/// LEAF-(d) PIN: `build()` populates `realization_handles`; a following
/// `tessellate_realizations()` PRESERVES it (does not clear), while dispatch
/// tallies reset per surface. This is the exact interleaving the CLI
/// combined-constraint arm relies on; a full-union reset would break it.
#[test]
fn tessellate_preserves_realization_handles_populated_by_build() {
    let compiled = compile_source(WIDGET_SRC);
    let compile_errs: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        compile_errs.is_empty(),
        "unexpected compile errors: {compile_errs:?}"
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // ── build surface: populates realization_handles ──────────────────────
    let build_result = engine.build(&compiled, ExportFormat::Step);
    let build_errs: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        build_errs.is_empty(),
        "unexpected build errors: {build_errs:?}"
    );

    // Sanity: the geometry cell hydrated to a handle, so the realization→handle
    // oracle is genuinely populated (not vacuously empty).
    let geom_cell = ValueCellId::new("Widget", "geometry");
    assert!(
        matches!(
            build_result.values.get_or_undef(&geom_cell),
            Value::GeometryHandle { .. }
        ),
        "Widget.geometry must hydrate to a Value::GeometryHandle"
    );

    let handles_after_build = engine.realization_handles_len();
    assert!(
        handles_after_build > 0,
        "build() must populate realization_handles (GHR-δ validity oracle); \
         got len={handles_after_build}"
    );
    dispatch_lockstep(&engine);

    // ── tessellate surface: MUST preserve realization_handles ─────────────
    engine.tessellate_realizations(&compiled);

    let handles_after_tess = engine.realization_handles_len();
    assert_eq!(
        handles_after_tess, handles_after_build,
        "tessellate_realizations() MUST NOT clear realization_handles \
         (load-bearing build↔tessellate asymmetry, leaf d): \
         before={handles_after_build} after={handles_after_tess}. \
         A full-union reset would zero this and degrade every Conforms/DFM verdict."
    );
    // Per-surface dispatch reset: the tallies were reset at tessellate entry
    // and re-counted; the lockstep equality proves both reset together.
    dispatch_lockstep(&engine);
}
