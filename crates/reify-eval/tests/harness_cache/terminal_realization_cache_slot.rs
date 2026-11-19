//! Which realization of a template owns the template's realization-cache slot
//! when a geometry `let` is declared after the `let` that consumes it (task
//! 10178). Every build surface must agree on that one owner: the consumer, not
//! the declaration-last producer, and never whichever realization a pruned walk
//! happens to run last.

use reify_compiler::CompiledModule;
use reify_core::RealizationNodeId;
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine, SolverPosture};
use reify_ir::ExportFormat;
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, errors_only, manufacturing_purpose,
    parse_and_compile,
};

/// `p` patterns `g`, but `g` is declared AFTER `p`: `p` is T3's only sink, so
/// every full walk runs `g` (T3#1) first and `p` (T3#0) last.
const FORWARD_DECLARED_GEOMETRY_SRC: &str = r#"structure T3 {
    let p = circular_pattern(g, axis_z(point3(0mm, 0mm, 0mm)), 4, 90deg)
    let g = box(2mm, 2mm, 2mm)
}"#;

fn p() -> RealizationNodeId {
    RealizationNodeId::new("T3", 0)
}

fn g() -> RealizationNodeId {
    RealizationNodeId::new("T3", 1)
}

/// A mock-kernel engine that has evaluated T3 and activated a 1e-6
/// manufacturing purpose on it, so every surface call demands a tolerance and
/// probes the realization cache.
fn engine_with_demanded_tolerance() -> (CompiledModule, Engine) {
    let mut compiled = parse_and_compile(FORWARD_DECLARED_GEOMETRY_SRC);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    compiled
        .compiled_purposes
        .push(manufacturing_purpose("mfg", 1e-6));

    let mut engine = Engine::new(
        SolverPosture::SolverFree,
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    let _eval = engine.eval(&compiled);
    engine.activate_purpose("mfg", "T3");
    (compiled, engine)
}

fn dispatched(engine: &Engine, rid: &RealizationNodeId) -> usize {
    engine
        .last_dispatch_count_by_realization()
        .get(rid)
        .copied()
        .unwrap_or(0)
}

/// Run `surface` twice. The second run must serve `p`, the walk-final
/// realization, from the cache and re-run `g`, which is not T3's terminal.
fn assert_second_call_serves_the_walk_final_realization(
    surface: impl Fn(&CompiledModule, &mut Engine),
) {
    let (compiled, mut engine) = engine_with_demanded_tolerance();

    surface(&compiled, &mut engine);
    assert!(
        !engine.realization_cache().is_empty(),
        "premise: the first call caches T3's terminal under the demanded tolerance"
    );
    for (name, rid) in [("p", p()), ("g", g())] {
        assert!(
            dispatched(&engine, &rid) > 0,
            "premise: {name} ({rid:?}) dispatches on the first call, got {:?}",
            engine.last_dispatch_count_by_realization()
        );
    }

    surface(&compiled, &mut engine);
    assert_eq!(
        dispatched(&engine, &p()),
        0,
        "p (T3#0), the walk-final realization, must be served from the cache on the \
         second call, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
    assert!(
        dispatched(&engine, &g()) > 0,
        "g (T3#1) is not T3's terminal, so it must re-run on the second call, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
}

#[test]
fn build_serves_the_walk_final_realization_from_the_cache() {
    assert_second_call_serves_the_walk_final_realization(|compiled, engine| {
        engine.build(compiled, ExportFormat::Step);
    });
}

#[test]
fn build_snapshot_serves_the_walk_final_realization_from_the_cache() {
    assert_second_call_serves_the_walk_final_realization(|compiled, engine| {
        engine
            .build_snapshot(compiled, ExportFormat::Step)
            .expect("build_snapshot must return Some after eval()");
    });
}

#[test]
fn tessellate_realizations_serves_the_walk_final_realization_from_the_cache() {
    assert_second_call_serves_the_walk_final_realization(|compiled, engine| {
        engine.tessellate_realizations(compiled);
    });
}

#[test]
fn a_pruned_walk_does_not_move_the_terminal() {
    let (compiled, mut engine) = engine_with_demanded_tolerance();

    engine.set_demand_selective([NodeId::Realization(g())]);
    engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must return Some after eval()");
    assert!(
        dispatched(&engine, &g()) > 0 && dispatched(&engine, &p()) == 0,
        "premise: the pruned walk runs g and not the hidden p, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
    assert_eq!(
        engine.realization_cache().len(),
        0,
        "g ran last in the pruned walk but is not T3's terminal, so it must not fill \
         T3's cache slot"
    );

    engine.build(&compiled, ExportFormat::Step);
    assert!(
        dispatched(&engine, &p()) > 0,
        "p (T3#0) must not be served a handle another realization cached, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
}

#[test]
fn a_distance_query_does_not_fill_the_slot_from_a_non_terminal_realization() {
    let (compiled, mut engine) = engine_with_demanded_tolerance();

    let _distance = engine.distance_between_placed(&compiled, "T3", "T3");
    assert!(
        dispatched(&engine, &g()) > 0,
        "premise: the distance query's Phase-A runs g, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
    assert_eq!(
        engine.realization_cache().len(),
        0,
        "g is not T3's terminal, so the distance query must not fill T3's cache slot"
    );

    engine.build(&compiled, ExportFormat::Step);
    assert!(
        dispatched(&engine, &p()) > 0,
        "p (T3#0) must not be served the box g cached during the distance query, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
}
