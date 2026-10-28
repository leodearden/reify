//! Task #7989: under selective demand, a `tessellate_snapshot` whose hash gate
//! sees a `GeomRef::Sub` producer or consumer move must re-realize the
//! consumer from its current producer, including a consumer that was hidden
//! across the producer's edit, and a cross-sub consumer whose operand names a
//! member two subs export (the reverse index has no edge for it).
//!
//! Oracle: per-realization dispatch tallies, the consumer's `MeshSurface`, and
//! zero Error diagnostics. A box mesh is size-invariant, so vertex counts
//! cannot tell a stale body from a fresh one. An unedited independent body `c`
//! must stay exempt, which rules out disabling the gate whenever anything moved.

use reify_compiler::{CompiledGeometryOp, CompiledModule, GeomRef, RealizationDecl};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, Severity, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine, TessellateResult};
use reify_test_support::{MockGeometryKernel, compile_source, mm};

/// A realization, named by its structure and its `let` binding.
type Body = (&'static str, &'static str);

/// A source whose `bodies` the tests demand, with one `consumer` compiled to a
/// Transform over `GeomRef::Sub(operand)`.
struct Fixture {
    src: &'static str,
    bodies: &'static [Body],
    consumer: Body,
    operand: &'static str,
}

const SIBLING_B: Body = ("SiblingSub", "b");
const SIBLING_F: Body = ("SiblingSub", "f");
const SIBLING_C: Body = ("SiblingSub", "c");

const SIBLING_SUB: Fixture = Fixture {
    src: r#"pub structure SiblingSub {
    param w : Length = 10mm
    param d : Length = 1mm
    let b = box(w, 10mm, 10mm)
    let f = translate(b, d, 0mm, 0mm)
    let c = box(5mm, 5mm, 5mm)
}"#,
    bodies: &[SIBLING_B, SIBLING_F, SIBLING_C],
    consumer: SIBLING_F,
    operand: "b",
};

const PART_A_BODY: Body = ("PartA", "body");
const PART_B_BODY: Body = ("PartB", "body");
const ASSY_PLACED: Body = ("Assy", "placed");
const ASSY_C: Body = ("Assy", "c");

/// `PartA` and `PartB` both export `body`, so `placed`'s `a.body` operand is
/// ambiguous by member name.
const AMBIGUOUS_SUB: Fixture = Fixture {
    src: r#"pub structure PartA {
    param w : Length = 10mm
    let body = box(w, 10mm, 10mm)
}

pub structure PartB {
    let body = box(5mm, 5mm, 5mm)
}

pub structure Assy {
    param k : Length = 5mm
    param d : Length = 1mm
    sub a = PartA()
    sub b = PartB()
    let placed = translate(self.a.body, d, 0mm, 0mm)
    let c = box(k, 5mm, 5mm)
}"#,
    bodies: &[PART_A_BODY, PART_B_BODY, ASSY_PLACED, ASSY_C],
    consumer: ASSY_PLACED,
    operand: "a.body",
};

fn decl(compiled: &CompiledModule, (entity, name): Body) -> &RealizationDecl {
    compiled
        .templates
        .iter()
        .flat_map(|t| t.realizations.iter())
        .find(|r| r.id.entity == entity && r.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("fixture must compile a realization {entity}.{name}"))
}

fn rid(compiled: &CompiledModule, body: Body) -> RealizationNodeId {
    decl(compiled, body).id.clone()
}

fn tally(engine: &Engine, rid: &RealizationNodeId) -> usize {
    engine
        .last_dispatch_count_by_realization()
        .get(rid)
        .copied()
        .unwrap_or(0)
}

/// `(body, tally)` for every body, for assertion messages.
fn tallies(engine: &Engine, compiled: &CompiledModule, bodies: &[Body]) -> Vec<(Body, usize)> {
    bodies
        .iter()
        .map(|body| (*body, tally(engine, &rid(compiled, *body))))
        .collect()
}

fn has_mesh(result: &TessellateResult, rid: &RealizationNodeId) -> bool {
    result
        .meshes
        .iter()
        .any(|m| m.entity_path == rid.to_string())
}

fn errors(result: &TessellateResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

fn demand(engine: &mut Engine, compiled: &CompiledModule, bodies: &[Body]) {
    let nodes = bodies
        .iter()
        .map(|body| NodeId::Realization(rid(compiled, *body)));
    engine.set_demand_selective(nodes);
}

fn tessellate(engine: &mut Engine, compiled: &CompiledModule) -> TessellateResult {
    engine
        .tessellate_snapshot(compiled)
        .expect("tessellate_snapshot needs a prior eval")
}

fn edit(engine: &mut Engine, entity: &str, param: &str, millimetres: f64) {
    engine
        .edit_param(ValueCellId::new(entity, param), mm(millimetres))
        .unwrap_or_else(|e| panic!("edit_param({entity}.{param}) must succeed: {e:?}"));
}

/// An evaluated engine with every fixture body selectively demanded,
/// premise-locked to have realized every body once and then to reuse every body
/// on a no-edit pass.
fn warm_engine(compiled: &CompiledModule, fixture: &Fixture) -> Engine {
    let consumer_targets_operand = decl(compiled, fixture.consumer)
        .operations
        .iter()
        .any(|op| {
            matches!(op, CompiledGeometryOp::Transform { target: GeomRef::Sub(n), .. } if n == fixture.operand)
        });
    assert!(
        consumer_targets_operand,
        "premise: {:?} compiles to a Transform over GeomRef::Sub({:?}) (task #4668)",
        fixture.consumer, fixture.operand
    );

    let mut engine = Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    engine.eval(compiled);
    demand(&mut engine, compiled, fixture.bodies);

    let first = tessellate(&mut engine, compiled);
    let realized = tallies(&engine, compiled, fixture.bodies);
    assert!(
        realized.iter().all(|(_, n)| *n > 0),
        "premise: the first selective pass realizes every body: {realized:?}"
    );
    assert!(
        has_mesh(&first, &rid(compiled, fixture.consumer)),
        "premise: the consumer surfaces a mesh"
    );
    assert_eq!(errors(&first), Vec::<String>::new(), "premise: no errors");

    tessellate(&mut engine, compiled);
    let reused = tallies(&engine, compiled, fixture.bodies);
    assert!(
        reused.iter().all(|(_, n)| *n == 0),
        "premise: a no-edit pass is hash-exempt for every body: {reused:?}"
    );
    engine
}

#[test]
fn a_producer_only_edit_re_realizes_its_sibling_sub_consumer() {
    let compiled = compile_source(SIBLING_SUB.src);
    let mut engine = warm_engine(&compiled, &SIBLING_SUB);
    let (b, f, c) = (
        rid(&compiled, SIBLING_B),
        rid(&compiled, SIBLING_F),
        rid(&compiled, SIBLING_C),
    );

    edit(&mut engine, "SiblingSub", "w", 20.0);
    let result = tessellate(&mut engine, &compiled);

    let seen = tallies(&engine, &compiled, SIBLING_SUB.bodies);
    assert!(
        tally(&engine, &b) > 0,
        "premise: the edited b re-runs: {seen:?}"
    );
    assert!(
        tally(&engine, &f) > 0,
        "producer-only edit: f must re-realize over the new b: {seen:?}"
    );
    assert!(
        has_mesh(&result, &f),
        "producer-only edit: f must surface a mesh: {seen:?}"
    );
    assert_eq!(errors(&result), Vec::<String>::new(), "producer-only edit");
    assert_eq!(
        tally(&engine, &c),
        0,
        "producer-only edit: the independent c stays exempt: {seen:?}"
    );
}

#[test]
fn a_consumer_only_edit_still_resolves_its_unchanged_sibling_producer() {
    let compiled = compile_source(SIBLING_SUB.src);
    let mut engine = warm_engine(&compiled, &SIBLING_SUB);
    let (f, c) = (rid(&compiled, SIBLING_F), rid(&compiled, SIBLING_C));

    edit(&mut engine, "SiblingSub", "d", 2.0);
    let result = tessellate(&mut engine, &compiled);

    let seen = tallies(&engine, &compiled, SIBLING_SUB.bodies);
    assert!(
        tally(&engine, &f) > 0,
        "consumer-only edit: f must re-realize: {seen:?}"
    );
    assert!(
        has_mesh(&result, &f),
        "consumer-only edit: f must surface a mesh: {seen:?}"
    );
    assert_eq!(
        errors(&result),
        Vec::<String>::new(),
        "consumer-only edit: f's Sub(\"b\") must resolve: {seen:?}"
    );
    assert_eq!(
        tally(&engine, &c),
        0,
        "consumer-only edit: the independent c stays exempt: {seen:?}"
    );
}

#[test]
fn a_sub_consumer_hidden_across_a_producer_edit_re_realizes_when_shown() {
    let compiled = compile_source(SIBLING_SUB.src);
    let mut engine = warm_engine(&compiled, &SIBLING_SUB);
    let f = rid(&compiled, SIBLING_F);

    demand(&mut engine, &compiled, &[SIBLING_B, SIBLING_C]);
    edit(&mut engine, "SiblingSub", "w", 20.0);
    tessellate(&mut engine, &compiled);
    let hidden = tallies(&engine, &compiled, SIBLING_SUB.bodies);
    assert_eq!(
        tally(&engine, &f),
        0,
        "premise: hidden f does not run: {hidden:?}"
    );

    demand(&mut engine, &compiled, SIBLING_SUB.bodies);
    let result = tessellate(&mut engine, &compiled);

    let shown = tallies(&engine, &compiled, SIBLING_SUB.bodies);
    assert!(
        tally(&engine, &f) > 0,
        "hidden across a producer edit: f must re-realize when shown: {shown:?}"
    );
    assert!(
        has_mesh(&result, &f),
        "hidden across a producer edit: f must surface a mesh when shown: {shown:?}"
    );
    assert_eq!(
        errors(&result),
        Vec::<String>::new(),
        "hidden across a producer edit: {shown:?}"
    );
}

#[test]
fn an_unrelated_edit_leaves_an_ambiguous_cross_sub_consumer_resolvable() {
    let compiled = compile_source(AMBIGUOUS_SUB.src);
    let mut engine = warm_engine(&compiled, &AMBIGUOUS_SUB);
    let (placed, c) = (rid(&compiled, ASSY_PLACED), rid(&compiled, ASSY_C));

    edit(&mut engine, "Assy", "k", 10.0);
    let result = tessellate(&mut engine, &compiled);

    let seen = tallies(&engine, &compiled, AMBIGUOUS_SUB.bodies);
    assert!(
        tally(&engine, &c) > 0,
        "premise: the edited c re-runs: {seen:?}"
    );
    assert!(
        tally(&engine, &placed) > 0,
        "unrelated edit: placed, whose operand the reverse index drops, must re-realize: {seen:?}"
    );
    assert!(
        has_mesh(&result, &placed),
        "unrelated edit: placed must surface a mesh: {seen:?}"
    );
    assert_eq!(
        errors(&result),
        Vec::<String>::new(),
        "unrelated edit: placed's Sub(\"a.body\") must resolve: {seen:?}"
    );
}

#[test]
fn a_consumer_only_edit_resolves_its_ambiguous_cross_sub_producer() {
    let compiled = compile_source(AMBIGUOUS_SUB.src);
    let mut engine = warm_engine(&compiled, &AMBIGUOUS_SUB);
    let (placed, c) = (rid(&compiled, ASSY_PLACED), rid(&compiled, ASSY_C));

    edit(&mut engine, "Assy", "d", 2.0);
    let result = tessellate(&mut engine, &compiled);

    let seen = tallies(&engine, &compiled, AMBIGUOUS_SUB.bodies);
    assert!(
        tally(&engine, &placed) > 0,
        "consumer-only edit: placed must re-realize: {seen:?}"
    );
    assert!(
        has_mesh(&result, &placed),
        "consumer-only edit: placed must surface a mesh: {seen:?}"
    );
    assert_eq!(
        errors(&result),
        Vec::<String>::new(),
        "consumer-only edit: placed's Sub(\"a.body\") must resolve: {seen:?}"
    );
    assert_eq!(
        tally(&engine, &c),
        0,
        "consumer-only edit: the independent c stays exempt: {seen:?}"
    );
}
