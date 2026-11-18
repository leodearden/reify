//! What a realization-cache hit restores, twice-built on real OCCT under a
//! demanded tolerance (PRD `docs/prds/v0_6/realized-world-evaluation.md`
//! §9 ξ / D15): the second build is served from the cache and must give every
//! displayed history-only datum the value the first build gave it.

use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, manufacturing_purpose, parse_and_compile_with_stdlib};

/// Compile `source` cleanly and return a real-OCCT engine that has evaluated
/// it and activated a 1e-6 manufacturing purpose on `entity`, so every build
/// demands a tolerance and probes the realization cache. `None` when OCCT is
/// unavailable.
fn occt_engine_with_demanded_tolerance(
    source: &str,
    entity: &str,
) -> Option<(CompiledModule, Engine)> {
    let mut compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    compiled
        .compiled_purposes
        .push(manufacturing_purpose("mfg", 1e-6));
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(planner)),
    );
    let _eval = engine.eval(&compiled);
    engine.activate_purpose("mfg", entity);
    Some((compiled, engine))
}

fn assert_build_was_cache_hit(engine: &Engine) {
    assert_eq!(
        engine.last_dispatch_count(),
        0,
        "premise: build 2 is served from the realization cache"
    );
    assert!(
        engine
            .last_dispatch_count_by_realization()
            .values()
            .all(|&n| n == 0),
        "premise: no realization dispatched on build 2, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
}

const SWEPT_HIT_SOURCE: &str = r#"
structure def SweptHit {
    let e = extrude(rectangle(20mm, 10mm), 5mm)
    let d : Direction = e.dir
}
"#;

#[test]
fn cache_hit_restores_swept_kind() {
    let Some((compiled, mut engine)) =
        occt_engine_with_demanded_tolerance(SWEPT_HIT_SOURCE, "SweptHit")
    else {
        return;
    };
    let d_cell = ValueCellId::new("SweptHit", "d");

    let first = engine.build(&compiled, ExportFormat::Step);
    assert!(
        engine.last_dispatch_count() > 0,
        "premise: build 1 dispatches kernel ops"
    );
    let first_d = first.values.get(&d_cell).cloned();
    assert!(
        matches!(first_d, Some(Value::Direction(_))),
        "premise: build 1 resolves e.dir to a Direction, got {first_d:?}"
    );

    let second = engine.build(&compiled, ExportFormat::Step);
    assert_build_was_cache_hit(&engine);
    assert_eq!(
        second.values.get(&d_cell).cloned(),
        first_d,
        "a cache-served extrude keeps its e.dir"
    );
    let ambiguous: Vec<_> = second
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FeatureDatumAmbiguous))
        .collect();
    assert!(
        ambiguous.is_empty(),
        "build 2 must not report FeatureDatumAmbiguous, got {ambiguous:#?}"
    );
}
