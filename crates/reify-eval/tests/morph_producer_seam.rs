//! Step-11 (task 4744 β): the `MorphProducer` hook seam.
//!
//! Pins the registration discipline + dispatch contract of the morph-producer
//! hook that `reify-eval` owns (PRD `docs/prds/v0_6/...` §4.2 / D3):
//!
//! - an engine with no producer registered → `morph_producer()` is `None`;
//! - after `register_morph_producer`, `morph_producer()` returns
//!   `Some(&dyn MorphProducer)` that dispatches `try_morph`;
//! - a second `register_morph_producer` panics (single-install discipline,
//!   mirroring `register_compute_fn`).
//!
//! RED until step-12 adds the `morph_producer` module + the Engine wiring.

use reify_eval::graph::EvaluationGraph;
use reify_eval::{BRepSnapshot, Engine, MorphProducer, MorphRequest, MorphResult};
use reify_ir::{
    BoundaryAssociation, GeometryHandle, GeometryHandleId, GeometryKernel, TopologyAttributeTable,
    Value, ValueMap, VolumeMesh,
};
use reify_test_support::mocks::MockConstraintChecker;

/// Minimal stub `GeometryKernel`. The seam test never projects, so every
/// method returns an error; the projection trait-methods are left at their
/// default-Err bodies (step-2). Exists only to give `MorphRequest` a kernel.
struct StubKernel;

impl GeometryKernel for StubKernel {
    fn execute(
        &mut self,
        _op: &reify_ir::GeometryOp,
    ) -> Result<GeometryHandle, reify_ir::GeometryError> {
        Err(reify_ir::GeometryError::OperationFailed("stub".into()))
    }
    fn query(&self, _q: &reify_ir::GeometryQuery) -> Result<Value, reify_ir::QueryError> {
        Err(reify_ir::QueryError::QueryFailed("stub".into()))
    }
    fn export(
        &self,
        _h: GeometryHandleId,
        _f: reify_ir::ExportFormat,
        _w: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        Err(reify_ir::ExportError::FormatError("stub".into()))
    }
    fn tessellate(
        &self,
        _h: GeometryHandleId,
        _t: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        Err(reify_ir::TessError::TessellationFailed("stub".into()))
    }
}

/// A mock producer that records dispatch by returning a sentinel `Ineligible`
/// payload — the seam test asserts this exact string flowed back through the
/// `&dyn MorphProducer` trait object.
struct MockProducer;

impl MorphProducer for MockProducer {
    fn try_morph(&self, _ctx: MorphRequest<'_>) -> MorphResult {
        MorphResult::Ineligible("mock-dispatched".to_string())
    }
}

fn empty_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: Vec::new(),
        tet_indices: Vec::new(),
        element_order: reify_ir::ElementOrderTag::P1,
        normals: None,
        boundary: None,
    }
}

#[test]
fn morph_producer_is_none_without_registration() {
    let engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    assert!(
        engine.morph_producer().is_none(),
        "an engine with no registered producer must return None"
    );
}

#[test]
fn register_morph_producer_then_dispatch_try_morph() {
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.register_morph_producer(Box::new(MockProducer));

    let producer = engine
        .morph_producer()
        .expect("after registration, morph_producer() must be Some");

    // Build a minimal borrowing MorphRequest and dispatch through the trait
    // object. The locals outlive `req` within this fn.
    let graph = EvaluationGraph::default();
    let values = ValueMap::new();
    let table = TopologyAttributeTable::default();
    let source = empty_mesh();
    let boundary = BoundaryAssociation::default();
    let kernel = StubKernel;
    let snap = BRepSnapshot {
        graph: &graph,
        values: &values,
        topology_attributes: &table,
        faces: &[],
        edges: &[],
        vertices: &[],
    };
    let req = MorphRequest {
        source: &source,
        boundary: &boundary,
        old_brep: snap,
        new_brep: snap,
        kernel: &kernel,
    };

    let result = producer.try_morph(req);
    assert!(
        matches!(result, MorphResult::Ineligible(ref s) if s == "mock-dispatched"),
        "morph_producer() must dispatch try_morph to the registered producer, got: {result:?}"
    );
}

#[test]
#[should_panic(expected = "already registered")]
fn second_register_morph_producer_panics() {
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.register_morph_producer(Box::new(MockProducer));
    // Single-install discipline: the second registration must panic, mirroring
    // register_compute_fn's duplicate-target guard.
    engine.register_morph_producer(Box::new(MockProducer));
}
