//! Which realization of a template owns the template's realization-cache slot
//! when a geometry `let` is declared after the `let` that consumes it (task
//! 10178). Every build surface must agree on that one owner: the consumer, not
//! the declaration-last producer, and never whichever realization a pruned walk
//! happens to run last.

use reify_compiler::CompiledModule;
use reify_core::RealizationNodeId;
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine, SolverPosture};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{
    GeometryOpRecord, MockConstraintChecker, MockGeometryKernel, errors_only,
    manufacturing_purpose, parse_and_compile,
};

/// `p` patterns `g`, but `g` is declared AFTER `p`: `p` is T3's only sink, so
/// every full walk runs `g` (T3#1) first and `p` (T3#0) last.
const FORWARD_DECLARED_GEOMETRY_SRC: &str = r#"structure T3 {
    let p = circular_pattern(g, axis_z(point3(0mm, 0mm, 0mm)), 4, 90deg)
    let g = box(2mm, 2mm, 2mm)
}"#;

/// T3 with an unrelated body `x` declared first: `x` (TwoSinks#0) and `p`
/// (TwoSinks#1) are both sinks, and `p` is the higher-declared one.
const TWO_SINKS_SRC: &str = r#"structure TwoSinks {
    let x = box(1mm, 1mm, 1mm)
    let p = circular_pattern(g, axis_z(point3(0mm, 0mm, 0mm)), 4, 90deg)
    let g = box(2mm, 2mm, 2mm)
}"#;

fn p() -> RealizationNodeId {
    RealizationNodeId::new("T3", 0)
}

fn g() -> RealizationNodeId {
    RealizationNodeId::new("T3", 1)
}

/// A mock-kernel engine that has evaluated `src` and activated a 1e-6
/// manufacturing purpose on `entity`, so every surface call demands a
/// tolerance and probes the realization cache.
fn engine_with_demanded_tolerance(src: &str, entity: &str) -> (CompiledModule, Engine) {
    engine_with_demanded_tolerance_on(src, entity, MockGeometryKernel::new())
}

fn engine_with_demanded_tolerance_on(
    src: &str,
    entity: &str,
    kernel: MockGeometryKernel,
) -> (CompiledModule, Engine) {
    let mut compiled = parse_and_compile(src);
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
        Some(Box::new(kernel)),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    let _eval = engine.eval(&compiled);
    engine.activate_purpose("mfg", entity);
    (compiled, engine)
}

fn dispatched(engine: &Engine, rid: &RealizationNodeId) -> usize {
    engine
        .last_dispatch_count_by_realization()
        .get(rid)
        .copied()
        .unwrap_or(0)
}

/// Run `surface` twice on `src`. The second run must serve `terminal` from the
/// cache and re-run every realization in `others`, which share its slot.
fn assert_second_call_serves_only_the_terminal_from_the_cache(
    src: &str,
    terminal: (&str, RealizationNodeId),
    others: &[(&str, RealizationNodeId)],
    surface: impl Fn(&CompiledModule, &mut Engine),
) {
    let (terminal_name, ref terminal_rid) = terminal;
    let (compiled, mut engine) = engine_with_demanded_tolerance(src, &terminal_rid.entity);

    surface(&compiled, &mut engine);
    assert!(
        !engine.realization_cache().is_empty(),
        "premise: the first call caches the template's terminal under the demanded tolerance"
    );
    for (name, rid) in std::iter::once(&terminal).chain(others) {
        assert!(
            dispatched(&engine, rid) > 0,
            "premise: {name} ({rid:?}) dispatches on the first call, got {:?}",
            engine.last_dispatch_count_by_realization()
        );
    }

    surface(&compiled, &mut engine);
    assert_eq!(
        dispatched(&engine, terminal_rid),
        0,
        "{terminal_name} ({terminal_rid:?}), the template's terminal, must be served from \
         the cache on the second call, got {:?}",
        engine.last_dispatch_count_by_realization()
    );
    for (name, rid) in others {
        assert!(
            dispatched(&engine, rid) > 0,
            "{name} ({rid:?}) is not the template's terminal, so it must re-run on the second \
             call, got {:?}",
            engine.last_dispatch_count_by_realization()
        );
    }
}

/// T3's terminal is `p` (T3#0), its only sink, though `g` is declared last.
fn assert_second_call_serves_t3s_sink_from_the_cache(
    surface: impl Fn(&CompiledModule, &mut Engine),
) {
    assert_second_call_serves_only_the_terminal_from_the_cache(
        FORWARD_DECLARED_GEOMETRY_SRC,
        ("p", p()),
        &[("g", g())],
        surface,
    );
}

#[test]
fn build_serves_t3s_sink_from_the_cache() {
    assert_second_call_serves_t3s_sink_from_the_cache(|compiled, engine| {
        engine.build(compiled, ExportFormat::Step);
    });
}

#[test]
fn build_snapshot_serves_t3s_sink_from_the_cache() {
    assert_second_call_serves_t3s_sink_from_the_cache(|compiled, engine| {
        engine
            .build_snapshot(compiled, ExportFormat::Step)
            .expect("build_snapshot must return Some after eval()");
    });
}

#[test]
fn tessellate_realizations_serves_t3s_sink_from_the_cache() {
    assert_second_call_serves_t3s_sink_from_the_cache(|compiled, engine| {
        engine.tessellate_realizations(compiled);
    });
}

#[test]
fn the_highest_declared_of_several_sinks_owns_the_slot() {
    assert_second_call_serves_only_the_terminal_from_the_cache(
        TWO_SINKS_SRC,
        ("p", RealizationNodeId::new("TwoSinks", 1)),
        &[
            ("x", RealizationNodeId::new("TwoSinks", 0)),
            ("g", RealizationNodeId::new("TwoSinks", 2)),
        ],
        |compiled, engine| {
            engine.build(compiled, ExportFormat::Step);
        },
    );
}

#[test]
fn a_pruned_walk_does_not_move_the_terminal() {
    let (compiled, mut engine) =
        engine_with_demanded_tolerance(FORWARD_DECLARED_GEOMETRY_SRC, "T3");

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

fn op_kinds(records: &[GeometryOpRecord]) -> Vec<&'static str> {
    records.iter().map(|record| record.op.kind_name()).collect()
}

fn ran_a_box(records: &[GeometryOpRecord]) -> bool {
    records
        .iter()
        .any(|record| matches!(record.op, GeometryOp::Box { .. }))
}

fn ran_a_circular_pattern(records: &[GeometryOpRecord]) -> bool {
    records
        .iter()
        .any(|record| matches!(record.op, GeometryOp::CircularPattern { .. }))
}

#[test]
fn a_distance_query_serves_only_the_terminal_from_the_cache() {
    let kernel = MockGeometryKernel::new();
    let ops = kernel.operations_ref();
    let (compiled, mut engine) =
        engine_with_demanded_tolerance_on(FORWARD_DECLARED_GEOMETRY_SRC, "T3", kernel);

    engine.build(&compiled, ExportFormat::Step);
    assert!(
        !engine.realization_cache().is_empty(),
        "premise: the build caches T3's terminal under the demanded tolerance"
    );
    {
        let build_ops = ops.lock().unwrap();
        assert!(
            ran_a_box(&build_ops) && ran_a_circular_pattern(&build_ops),
            "premise: the build runs g's Box and p's CircularPattern, got {:?}",
            op_kinds(&build_ops)
        );
    }

    let before = ops.lock().unwrap().len();
    let _distance = engine.distance_between_placed(&compiled, "T3", "T3");
    let all_ops = ops.lock().unwrap();
    let distance_ops = &all_ops[before..];
    assert!(
        ran_a_box(distance_ops),
        "g (T3#1) is not T3's terminal, so the distance pass must re-run it rather than \
         be served the body the build cached for T3, got {:?}",
        op_kinds(distance_ops)
    );
    assert!(
        !ran_a_circular_pattern(distance_ops),
        "p (T3#0) is T3's terminal, so the distance pass must serve it from the slot the \
         build filled, got {:?}",
        op_kinds(distance_ops)
    );
}
