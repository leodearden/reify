//! Task #7989: under selective demand, a `tessellate_snapshot` whose hash gate
//! sees a `GeomRef::Sub` producer or consumer move must re-realize the
//! consumer from its current producer, including a consumer that was hidden
//! across the producer's edit.
//!
//! Oracle: per-realization dispatch tallies, the consumer's `MeshSurface`, and
//! zero Error diagnostics. A box mesh is size-invariant, so vertex counts
//! cannot tell a stale body from a fresh one. The independent body `c` must stay
//! exempt, which rules out disabling the gate whenever anything moved.

use reify_compiler::{CompiledGeometryOp, CompiledModule, GeomRef, RealizationDecl};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, Severity, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine, TessellateResult};
use reify_test_support::{MockGeometryKernel, compile_source, mm};

const SIBLING_SUB_SRC: &str = r#"pub structure SiblingSub {
    param w : Length = 10mm
    param d : Length = 1mm
    let b = box(w, 10mm, 10mm)
    let f = translate(b, d, 0mm, 0mm)
    let c = box(5mm, 5mm, 5mm)
}"#;

const ALL_BODIES: [&str; 3] = ["b", "f", "c"];

fn decl<'m>(compiled: &'m CompiledModule, name: &str) -> &'m RealizationDecl {
    compiled
        .templates
        .iter()
        .flat_map(|t| t.realizations.iter())
        .find(|r| r.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("fixture must compile a realization named {name:?}"))
}

fn rid(compiled: &CompiledModule, name: &str) -> RealizationNodeId {
    decl(compiled, name).id.clone()
}

fn tally(engine: &Engine, rid: &RealizationNodeId) -> usize {
    engine
        .last_dispatch_count_by_realization()
        .get(rid)
        .copied()
        .unwrap_or(0)
}

/// `(name, tally)` for every body, for assertion messages.
fn tallies(engine: &Engine, compiled: &CompiledModule) -> Vec<(&'static str, usize)> {
    ALL_BODIES
        .iter()
        .map(|name| (*name, tally(engine, &rid(compiled, name))))
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

fn demand(engine: &mut Engine, compiled: &CompiledModule, names: &[&str]) {
    let nodes = names
        .iter()
        .map(|name| NodeId::Realization(rid(compiled, name)));
    engine.set_demand_selective(nodes);
}

fn tessellate(engine: &mut Engine, compiled: &CompiledModule) -> TessellateResult {
    engine
        .tessellate_snapshot(compiled)
        .expect("tessellate_snapshot needs a prior eval")
}

fn edit(engine: &mut Engine, param: &str, millimetres: f64) {
    engine
        .edit_param(ValueCellId::new("SiblingSub", param), mm(millimetres))
        .unwrap_or_else(|e| panic!("edit_param(SiblingSub.{param}) must succeed: {e:?}"));
}

/// An evaluated engine with all three bodies selectively demanded, premise-locked
/// to have realized every body once and then to reuse every body on a no-edit pass.
fn warm_engine(compiled: &CompiledModule) -> Engine {
    let f_targets_b = decl(compiled, "f").operations.iter().any(|op| {
        matches!(op, CompiledGeometryOp::Transform { target: GeomRef::Sub(n), .. } if n == "b")
    });
    assert!(
        f_targets_b,
        "premise: f compiles to a Transform over GeomRef::Sub(\"b\") (task #4668)"
    );

    let mut engine = Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    engine.eval(compiled);
    demand(&mut engine, compiled, &ALL_BODIES);

    let first = tessellate(&mut engine, compiled);
    let realized = tallies(&engine, compiled);
    assert!(
        realized.iter().all(|(_, n)| *n > 0),
        "premise: the first selective pass realizes every body: {realized:?}"
    );
    assert!(
        has_mesh(&first, &rid(compiled, "f")),
        "premise: f surfaces a mesh"
    );
    assert_eq!(errors(&first), Vec::<String>::new(), "premise: no errors");

    tessellate(&mut engine, compiled);
    let reused = tallies(&engine, compiled);
    assert!(
        reused.iter().all(|(_, n)| *n == 0),
        "premise: a no-edit pass is hash-exempt for every body: {reused:?}"
    );
    engine
}

#[test]
fn a_producer_only_edit_re_realizes_its_sibling_sub_consumer() {
    let compiled = compile_source(SIBLING_SUB_SRC);
    let mut engine = warm_engine(&compiled);
    let (b, f, c) = (
        rid(&compiled, "b"),
        rid(&compiled, "f"),
        rid(&compiled, "c"),
    );

    edit(&mut engine, "w", 20.0);
    let result = tessellate(&mut engine, &compiled);

    let seen = tallies(&engine, &compiled);
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
    let compiled = compile_source(SIBLING_SUB_SRC);
    let mut engine = warm_engine(&compiled);
    let (f, c) = (rid(&compiled, "f"), rid(&compiled, "c"));

    edit(&mut engine, "d", 2.0);
    let result = tessellate(&mut engine, &compiled);

    let seen = tallies(&engine, &compiled);
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
    let compiled = compile_source(SIBLING_SUB_SRC);
    let mut engine = warm_engine(&compiled);
    let f = rid(&compiled, "f");

    demand(&mut engine, &compiled, &["b", "c"]);
    edit(&mut engine, "w", 20.0);
    tessellate(&mut engine, &compiled);
    let hidden = tallies(&engine, &compiled);
    assert_eq!(
        tally(&engine, &f),
        0,
        "premise: hidden f does not run: {hidden:?}"
    );

    demand(&mut engine, &compiled, &ALL_BODIES);
    let result = tessellate(&mut engine, &compiled);

    let shown = tallies(&engine, &compiled);
    assert!(
        tally(&engine, &f) > 0,
        "hidden across a producer edit: f must re-realize when shown: {shown:?}"
    );
    assert!(
        has_mesh(&result, &f),
        "hidden across a producer edit: f must surface a mesh when shown: {shown:?}"
    );
}
