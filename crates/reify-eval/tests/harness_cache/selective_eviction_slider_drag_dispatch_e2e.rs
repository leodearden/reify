//! ε (#4732) selective-realization-eviction headline e2e: a slider drag on
//! one body re-executes ONLY that body (PRD
//! `docs/prds/v0_6/selective-realization-eviction.md` §2).
//!
//! Two templates, `PartA` (2 kernel ops) and `PartB` (3 kernel ops), each
//! cached at a demanded tolerance through its own manufacturing purpose. The
//! distinct op counts let the global `last_dispatch_count()` say WHICH body
//! ran, and the per-realization tally says it again per body. Every
//! assertion is an op-count equality, never a tolerance.
//!
//! The scheduler is pinned to `UnifiedDag` (PRD D6): the edit/flush seam is
//! scheduler-agnostic, so there is no feature gate here — the `unified-dag`
//! cargo feature is vestigial and gating on it would make this never run.

use reify_compiler::{CompiledGeometryOp, CompiledModule, GeomRef, PrimitiveKind, TransformKind};
use reify_core::{ModulePath, RealizationNodeId, Type, ValueCellId};
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{CompiledExpr, ExportFormat, Value};
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, manufacturing_purpose, mm, step_output_template,
};

const PART_A_OPS: usize = 2;
const PART_B_OPS: usize = 3;
const ALL_OPS: usize = PART_A_OPS + PART_B_OPS;

fn mm_lit(v: f64) -> CompiledExpr {
    CompiledExpr::literal(mm(v), Type::length())
}

/// A template `name` whose named realization is a Box reading `<name>.w`,
/// followed by `translates` translations — `1 + translates` kernel ops. The
/// param `<name>.label` is read by nothing. The realization entity equals
/// the template name, which `build()`'s schedule filter requires.
fn body_template(name: &str, width_mm: f64, translates: usize) -> reify_compiler::TopologyTemplate {
    let mut ops = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            (
                "width".into(),
                CompiledExpr::value_ref(ValueCellId::new(name, "w"), Type::length()),
            ),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    }];
    for step in 0..translates {
        ops.push(CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(step),
            args: vec![
                ("dx".into(), mm_lit(1.0)),
                ("dy".into(), mm_lit(0.0)),
                ("dz".into(), mm_lit(0.0)),
            ],
        });
    }
    TopologyTemplateBuilder::new(name)
        .param(name, "w", Type::length(), Some(mm_lit(width_mm)))
        .param(name, "label", Type::dimensionless_scalar(), None)
        .realization_named(name, 0, "body", ops)
        .build()
}

/// `PartA` and `PartB` with one manufacturing purpose PER entity —
/// `activate_purpose` is keyed by purpose name, so a shared purpose would
/// bind only the first entity and leave the other uncached.
fn two_body_module() -> CompiledModule {
    CompiledModuleBuilder::new(ModulePath::new(vec!["slider_drag_dispatch".to_string()]))
        .template(step_output_template(1e-6))
        .template(body_template("PartA", 10.0, PART_A_OPS - 1))
        .template(body_template("PartB", 20.0, PART_B_OPS - 1))
        .compiled_purpose(manufacturing_purpose("mfg_a", 1e-6))
        .compiled_purpose(manufacturing_purpose("mfg_b", 1e-6))
        .build()
}

fn part(entity: &str) -> RealizationNodeId {
    RealizationNodeId::new(entity, 0)
}

fn tally(engine: &Engine, entity: &str) -> usize {
    engine
        .last_dispatch_count_by_realization()
        .get(&part(entity))
        .copied()
        .unwrap_or(0)
}

/// A cold-built engine, premise-locked to have dispatched every op of both
/// bodies and then to be fully warm (a no-edit rebuild dispatches nothing).
fn warm_two_body_engine(module: &CompiledModule) -> Engine {
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    let _eval = engine.eval(module);
    engine.activate_purpose("mfg_a", "PartA");
    engine.activate_purpose("mfg_b", "PartB");

    engine.build_snapshot(module, ExportFormat::Step);
    assert_eq!(
        (tally(&engine, "PartA"), tally(&engine, "PartB")),
        (PART_A_OPS, PART_B_OPS),
        "premise: the cold build dispatches every op of each body"
    );
    assert_eq!(engine.last_dispatch_count(), ALL_OPS, "premise: cold total");

    engine.build_snapshot(module, ExportFormat::Step);
    assert_eq!(
        engine.last_dispatch_count(),
        0,
        "premise: a no-edit rebuild is served entirely from the warm cache"
    );
    engine
}

fn drag_part_a_width(engine: &mut Engine) {
    engine
        .edit_param(ValueCellId::new("PartA", "w"), mm(30.0))
        .expect("edit_param must succeed against the PartA.w Length param");
}

#[test]
fn slider_drag_reexecutes_only_affected_body_e2e() {
    let module = two_body_module();
    let mut engine = warm_two_body_engine(&module);

    drag_part_a_width(&mut engine);
    engine.build_snapshot(&module, ExportFormat::Step);

    assert_eq!(
        engine.last_dispatch_count(),
        PART_A_OPS,
        "a drag of PartA's width must re-execute exactly PartA's ops"
    );
    assert!(engine.last_dispatch_count() < ALL_OPS);
    assert_eq!(tally(&engine, "PartA"), PART_A_OPS);
    assert_eq!(
        tally(&engine, "PartB"),
        0,
        "PartB's input cone did not move, so it must be served from the cache"
    );
}

#[test]
fn no_realization_edit_dispatches_zero_e2e() {
    let module = two_body_module();
    let mut engine = warm_two_body_engine(&module);

    engine
        .edit_param(ValueCellId::new("PartA", "label"), Value::Real(0.5))
        .expect("edit_param must succeed against the display-only PartA.label param");
    engine.build_snapshot(&module, ExportFormat::Step);

    assert_eq!(
        engine.last_dispatch_count(),
        0,
        "an edit read by no realization must dispatch no kernel op"
    );
}

/// The pre-γ regime on the same fixture: the identical PartA-only edit
/// followed by the public whole-cache flush re-executes every body. This
/// is the ceiling the selective count above sits strictly below.
#[test]
fn wholesale_flush_baseline_redispatches_every_body() {
    let module = two_body_module();
    let mut engine = warm_two_body_engine(&module);

    drag_part_a_width(&mut engine);
    engine.clear_realization_cache();
    engine.build_snapshot(&module, ExportFormat::Step);

    assert_eq!(engine.last_dispatch_count(), ALL_OPS);
}
