//! End-to-end dispatch pin for the explicit projection `feature(geometry) :
//! Feature` (task 4830, P3α; PRD
//! `docs/prds/naming-convergence/P3-explicit-feature-projection.md` D1).
//!
//! Mirrors the compile/build/assert harness in
//! `tests/geometry_query_kernel_dispatch.rs`: compile an inline DSL structure
//! that realizes a primitive and binds a `feature(...)` accessor `let` over
//! it, build through a real-OCCT `Engine`, and assert the resulting value
//! cell resolves to `Value::Feature(FeatureId::Realization(_))`.
//!
//! The compile-clean assertion runs unconditionally so a grammar/compile
//! regression fails on every runner; the kernel build assertion is gated on
//! `reify_kernel_occt::OCCT_AVAILABLE` and skips cleanly otherwise.

use reify_core::ValueCellId;
use reify_ir::{ExportFormat, FeatureId, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Compile `source` (asserting no error-severity diagnostics), then — if OCCT
/// is available — build it through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable, signalling the
/// caller to skip the numeric assertions.
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

// ── feature(let-bound) ───────────────────────────────────────────────────

const FEATURE_LET_BOUND_SOURCE: &str = r#"
structure def FeatureBox {
    let b = box(10mm, 10mm, 10mm)
    let f = feature(b)
}
"#;

/// `feature(b)` over a let-bound, realized whole-body handle must resolve to
/// `Value::Feature(FeatureId::Realization(_))` — non-`Undef`. The exact
/// `RealizationNodeId` entity name is deliberately NOT pinned (avoids
/// brittle coupling to internal id-generation naming).
///
/// RED until step-08 wires `Engine::post_process_feature_accessor` into
/// `run_post_processes` / `hydrate_value_cell_in_loop` — until then the cell
/// stays at its compiled default `Value::Undef`.
#[test]
fn feature_let_bound_evaluates_to_feature_value() {
    let Some(result) = compile_and_build_occt(FEATURE_LET_BOUND_SOURCE) else {
        return;
    };

    match result.values.get(&ValueCellId::new("FeatureBox", "f")) {
        Some(Value::Feature(FeatureId::Realization(_))) => {}
        other => panic!(
            "feature(b) must resolve to Value::Feature(FeatureId::Realization(_)); \
             got {other:?}"
        ),
    }
}

// ── feature(inline) — typecheck-only signal ─────────────────────────────

const FEATURE_INLINE_SOURCE: &str = r#"
structure def FeatureInline {
    let f = feature(box(10mm, 10mm, 10mm))
}
"#;

/// The inline form `feature(box(...))` must typecheck with no
/// unresolved-function error diagnostic — the `reify check` types-as-Feature
/// signal (PRD D1 LEAF form). Already green from step-02's registration;
/// included here as the end-to-end check-signal confirmation. Eval-time
/// resolution of an inline geometry-constructor arg is out of scope (see
/// task 4830 design decision on the let-bound RED-test choice).
#[test]
fn feature_inline_typechecks_as_feature() {
    let compiled = parse_and_compile_with_stdlib(FEATURE_INLINE_SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "inline feature(box(...)) must typecheck with no error-severity \
         diagnostics (feature() registers as a geometry-query helper, PRD D1); \
         got:\n{:#?}",
        errors_only(&compiled)
    );
}
