use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reify_types::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintNodeId, ConstraintResult,
    ConstraintSolver, Diagnostic, ExportError, ExportFormat, GeometryError, GeometryHandle,
    GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, ReprKind,
    ResolutionProblem, Satisfaction, SolveResult, TessError, Value, ValueCellId,
};

/// Mock constraint checker that returns predetermined results.
pub struct MockConstraintChecker {
    results: HashMap<ConstraintNodeId, Satisfaction>,
    default: Satisfaction,
}

impl MockConstraintChecker {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            default: Satisfaction::Satisfied,
        }
    }

    /// Set the result for a specific constraint.
    pub fn with_result(mut self, id: ConstraintNodeId, satisfaction: Satisfaction) -> Self {
        self.results.insert(id, satisfaction);
        self
    }

    /// Set the default result for constraints not explicitly configured.
    pub fn with_default(mut self, satisfaction: Satisfaction) -> Self {
        self.default = satisfaction;
        self
    }
}

impl Default for MockConstraintChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstraintChecker for MockConstraintChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| {
                let satisfaction = self.results.get(id).copied().unwrap_or(self.default);
                ConstraintResult {
                    id: id.clone(),
                    satisfaction,
                    diagnostics: ConstraintDiagnostics::default(),
                }
            })
            .collect()
    }
}

/// Mock constraint solver that returns predetermined results.
pub struct MockConstraintSolver {
    result: SolveResult,
}

impl MockConstraintSolver {
    /// Create a solver that returns Solved with the given values.
    pub fn new_solved(values: HashMap<ValueCellId, Value>) -> Self {
        Self {
            result: SolveResult::Solved { values },
        }
    }

    /// Create a solver that returns Infeasible with the given diagnostics.
    pub fn new_infeasible(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            result: SolveResult::Infeasible { diagnostics },
        }
    }

    /// Create a solver that returns NoProgress with the given reason.
    pub fn new_no_progress(reason: impl Into<String>) -> Self {
        Self {
            result: SolveResult::NoProgress {
                reason: reason.into(),
            },
        }
    }
}

impl ConstraintSolver for MockConstraintSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        self.result.clone()
    }
}

/// Mock constraint solver that returns different results on each call.
/// Results are consumed in order; once exhausted, the last result is repeated.
pub struct SequencedMockConstraintSolver {
    results: Arc<Mutex<Vec<SolveResult>>>,
    last: Arc<Mutex<Option<SolveResult>>>,
}

impl SequencedMockConstraintSolver {
    /// Create a solver that returns each result in sequence.
    /// After all results are consumed, the last one is repeated.
    pub fn new(results: Vec<SolveResult>) -> Self {
        Self {
            results: Arc::new(Mutex::new(results)),
            last: Arc::new(Mutex::new(None)),
        }
    }
}

impl ConstraintSolver for SequencedMockConstraintSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        let mut results = self.results.lock().unwrap();
        if results.is_empty() {
            self.last.lock().unwrap().clone().expect("no results configured")
        } else {
            let r = results.remove(0);
            *self.last.lock().unwrap() = Some(r.clone());
            r
        }
    }
}

/// Record of operations received by MockGeometryKernel.
#[derive(Debug, Clone)]
pub struct GeometryOpRecord {
    pub op: GeometryOp,
    pub result_handle: GeometryHandleId,
}

/// Key for per-query-type result configuration in MockGeometryKernel.
///
/// Each variant matches a `GeometryQuery` discriminant plus the relevant handle IDs,
/// enabling different return values for Volume vs SurfaceArea on the same handle.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum QueryKey {
    Volume(GeometryHandleId),
    SurfaceArea(GeometryHandleId),
    Centroid(GeometryHandleId),
    BoundingBox(GeometryHandleId),
    /// Distance keys both handles. Axis floats are converted to ordered bits for hashing.
    Distance {
        from: GeometryHandleId,
        to: GeometryHandleId,
    },
    /// MomentOfInertia keys the handle + axis (f64 bits for hashing).
    MomentOfInertia {
        handle: GeometryHandleId,
        axis_bits: [u64; 3],
    },
}

impl QueryKey {
    fn from_query(query: &GeometryQuery) -> Self {
        match query {
            GeometryQuery::Volume(id) => QueryKey::Volume(*id),
            GeometryQuery::SurfaceArea(id) => QueryKey::SurfaceArea(*id),
            GeometryQuery::Centroid(id) => QueryKey::Centroid(*id),
            GeometryQuery::BoundingBox(id) => QueryKey::BoundingBox(*id),
            GeometryQuery::Distance { from, to } => QueryKey::Distance {
                from: *from,
                to: *to,
            },
            GeometryQuery::MomentOfInertia { handle, axis } => QueryKey::MomentOfInertia {
                handle: *handle,
                axis_bits: [
                    axis[0].to_bits(),
                    axis[1].to_bits(),
                    axis[2].to_bits(),
                ],
            },
        }
    }
}

/// Mock geometry kernel that tracks operations and returns dummy handles.
pub struct MockGeometryKernel {
    next_id: u64,
    operations: Arc<Mutex<Vec<GeometryOpRecord>>>,
    /// Generic handle-only query results (fallback).
    queries: HashMap<GeometryHandleId, Value>,
    /// Per-query-type results (takes precedence over generic).
    typed_queries: HashMap<QueryKey, Value>,
}

impl MockGeometryKernel {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            operations: Arc::new(Mutex::new(Vec::new())),
            queries: HashMap::new(),
            typed_queries: HashMap::new(),
        }
    }

    /// Configure a generic query response for a specific handle (fallback for all query types).
    pub fn with_query_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.queries.insert(handle, value);
        self
    }

    /// Configure a Volume query result for a specific handle.
    pub fn with_volume_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::Volume(handle), value);
        self
    }

    /// Configure a SurfaceArea query result for a specific handle.
    pub fn with_surface_area_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::SurfaceArea(handle), value);
        self
    }

    /// Configure a Centroid query result for a specific handle.
    pub fn with_centroid_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::Centroid(handle), value);
        self
    }

    /// Configure a BoundingBox query result for a specific handle.
    pub fn with_bbox_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::BoundingBox(handle), value);
        self
    }

    /// Configure a Distance query result for a specific pair of handles.
    pub fn with_distance_result(
        mut self,
        from: GeometryHandleId,
        to: GeometryHandleId,
        value: Value,
    ) -> Self {
        self.typed_queries
            .insert(QueryKey::Distance { from, to }, value);
        self
    }

    /// Configure a MomentOfInertia query result for a specific handle and axis.
    pub fn with_inertia_result(
        mut self,
        handle: GeometryHandleId,
        axis: [f64; 3],
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::MomentOfInertia {
                handle,
                axis_bits: [
                    axis[0].to_bits(),
                    axis[1].to_bits(),
                    axis[2].to_bits(),
                ],
            },
            value,
        );
        self
    }

    /// Get the operations received so far.
    pub fn operations(&self) -> Vec<GeometryOpRecord> {
        self.operations.lock().unwrap().clone()
    }

    /// Get a shared reference to operations for inspection.
    pub fn operations_ref(&self) -> Arc<Mutex<Vec<GeometryOpRecord>>> {
        self.operations.clone()
    }

    /// Return the most recently executed operation, or `None` if no ops have been recorded.
    pub fn last_op(&self) -> Option<GeometryOpRecord> {
        self.operations.lock().unwrap().last().cloned()
    }

    /// Return all operations matching a predicate on the `GeometryOp`.
    pub fn find_ops<F: Fn(&GeometryOp) -> bool>(&self, f: F) -> Vec<GeometryOpRecord> {
        self.operations
            .lock()
            .unwrap()
            .iter()
            .filter(|rec| f(&rec.op))
            .cloned()
            .collect()
    }

    /// Return the total number of operations recorded.
    pub fn op_count(&self) -> usize {
        self.operations.lock().unwrap().len()
    }

    /// Return `true` if any recorded operation matches the predicate.
    pub fn has_op<F: Fn(&GeometryOp) -> bool>(&self, f: F) -> bool {
        self.operations
            .lock()
            .unwrap()
            .iter()
            .any(|rec| f(&rec.op))
    }
}

impl Default for MockGeometryKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for MockGeometryKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        let id = GeometryHandleId(self.next_id);
        self.next_id += 1;

        self.operations.lock().unwrap().push(GeometryOpRecord {
            op: op.clone(),
            result_handle: id,
        });

        Ok(GeometryHandle {
            id,
            repr: ReprKind::Solid,
        })
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        // Check per-query-type map first
        let key = QueryKey::from_query(query);
        if let Some(value) = self.typed_queries.get(&key) {
            return Ok(value.clone());
        }

        // Fall back to generic handle-only map
        let handle_id = match query {
            GeometryQuery::Volume(id) => id,
            GeometryQuery::SurfaceArea(id) => id,
            GeometryQuery::Centroid(id) => id,
            GeometryQuery::BoundingBox(id) => id,
            GeometryQuery::Distance { from, .. } => from,
            GeometryQuery::MomentOfInertia { handle, .. } => handle,
        };

        self.queries
            .get(handle_id)
            .cloned()
            .ok_or_else(|| QueryError::QueryFailed(format!("no mock result for {:?}", handle_id)))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        // Write minimal dummy content
        writer
            .write_all(b"MOCK_EXPORT_DATA")
            .map_err(|e| ExportError::IoError(e.to_string()))
    }

    fn tessellate(
        &self,
        _handle: GeometryHandleId,
        _tolerance: f64,
    ) -> Result<Mesh, TessError> {
        // Return a minimal valid mesh (one triangle)
        Ok(Mesh {
            vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            normals: Some(vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
        })
    }
}

/// Spy constraint solver that captures the last `ResolutionProblem` passed to it.
///
/// Use this in tests where you need to assert what the engine sent to the solver,
/// not just the result of solving (e.g., to verify the `objective` field is wired).
pub struct SpyConstraintSolver {
    captured: Arc<Mutex<Option<ResolutionProblem>>>,
    result: SolveResult,
}

impl SpyConstraintSolver {
    /// Create a spy that will return `Solved` with the given values and capture
    /// the `ResolutionProblem` it receives.
    pub fn new_solved(values: HashMap<ValueCellId, Value>) -> Self {
        Self {
            captured: Arc::new(Mutex::new(None)),
            result: SolveResult::Solved { values },
        }
    }

    /// Return a shared reference to the captured problem so callers can
    /// inspect it after `solve()` has been called.
    pub fn captured_problem(&self) -> Arc<Mutex<Option<ResolutionProblem>>> {
        self.captured.clone()
    }
}

impl ConstraintSolver for SpyConstraintSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        *self.captured.lock().unwrap() = Some(problem.clone());
        self.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{meters, mm3, mm2, point3};
    use reify_types::{CompiledExpr, Type, Value, ValueMap};

    #[test]
    fn mock_constraint_checker_predetermined() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let checker = MockConstraintChecker::new()
            .with_result(cnid.clone(), Satisfaction::Violated);

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = ConstraintInput {
            constraints: vec![(cnid.clone(), &expr)],
            values: &values,
            functions: &[],
        };

        let results = checker.check(&input);
        assert_eq!(results[0].satisfaction, Satisfaction::Violated);
    }

    #[test]
    fn mock_geometry_kernel_tracks_ops() {
        let mut kernel = MockGeometryKernel::new();
        let op = GeometryOp::Box {
            width: Value::length(0.08),
            height: Value::length(0.1),
            depth: Value::length(0.005),
        };

        let handle = kernel.execute(&op).unwrap();
        assert_eq!(handle.id, GeometryHandleId(1));
        assert_eq!(kernel.operations().len(), 1);
    }

    #[test]
    fn mock_geometry_kernel_incrementing_handles() {
        let mut kernel = MockGeometryKernel::new();
        let op = GeometryOp::Sphere {
            radius: Value::length(0.01),
        };

        let h1 = kernel.execute(&op).unwrap();
        let h2 = kernel.execute(&op).unwrap();
        assert_eq!(h1.id, GeometryHandleId(1));
        assert_eq!(h2.id, GeometryHandleId(2));
    }

    #[test]
    fn mock_constraint_solver_solved() {
        let mut values = HashMap::new();
        values.insert(ValueCellId::new("S", "x"), Value::length(0.005));

        let solver = MockConstraintSolver::new_solved(values.clone());
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![],
        };

        match solver.solve(&problem) {
            SolveResult::Solved { values: v } => {
                assert_eq!(v.len(), 1);
                assert!(v.contains_key(&ValueCellId::new("S", "x")));
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn mock_constraint_solver_infeasible() {
        let solver = MockConstraintSolver::new_infeasible(vec![
            Diagnostic::error("constraints are infeasible"),
        ]);
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![],
        };

        match solver.solve(&problem) {
            SolveResult::Infeasible { diagnostics } => {
                assert_eq!(diagnostics.len(), 1);
                assert!(diagnostics[0].message.contains("infeasible"));
            }
            other => panic!("expected Infeasible, got {:?}", other),
        }
    }

    #[test]
    fn mock_constraint_solver_no_progress() {
        let solver = MockConstraintSolver::new_no_progress("iteration limit reached");
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![],
        };

        match solver.solve(&problem) {
            SolveResult::NoProgress { reason } => {
                assert_eq!(reason, "iteration limit reached");
            }
            other => panic!("expected NoProgress, got {:?}", other),
        }
    }

    #[test]
    fn mock_constraint_solver_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockConstraintSolver>();

        let _boxed: Box<dyn ConstraintSolver> = Box::new(
            MockConstraintSolver::new_no_progress("test"),
        );
    }

    // step-5: failing tests for per-query-type mock configuration
    #[test]
    fn mock_with_volume_result_returns_for_volume_query() {
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new()
            .with_volume_result(id, mm3(1000.0));

        let result = kernel.query(&GeometryQuery::Volume(id)).unwrap();
        assert_eq!(result, mm3(1000.0));
    }

    #[test]
    fn mock_with_surface_area_result_returns_for_surface_area_query() {
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new()
            .with_surface_area_result(id, mm2(600.0));

        let result = kernel.query(&GeometryQuery::SurfaceArea(id)).unwrap();
        assert_eq!(result, mm2(600.0));
    }

    #[test]
    fn mock_with_centroid_result_returns_for_centroid_query() {
        let id = GeometryHandleId(1);
        let centroid = point3(0.5, 0.5, 0.5);
        let kernel = MockGeometryKernel::new()
            .with_centroid_result(id, centroid.clone());

        let result = kernel.query(&GeometryQuery::Centroid(id)).unwrap();
        assert_eq!(result, centroid);
    }

    #[test]
    fn mock_with_bbox_result_returns_for_bounding_box_query() {
        let id = GeometryHandleId(1);
        let bbox = Value::List(vec![point3(0.0, 0.0, 0.0), point3(1.0, 1.0, 1.0)]);
        let kernel = MockGeometryKernel::new()
            .with_bbox_result(id, bbox.clone());

        let result = kernel.query(&GeometryQuery::BoundingBox(id)).unwrap();
        assert_eq!(result, bbox);
    }

    #[test]
    fn mock_with_distance_result_returns_for_distance_query() {
        let from = GeometryHandleId(1);
        let to = GeometryHandleId(2);
        let kernel = MockGeometryKernel::new()
            .with_distance_result(from, to, meters(5.0));

        let result = kernel.query(&GeometryQuery::Distance { from, to }).unwrap();
        assert_eq!(result, meters(5.0));
    }

    #[test]
    fn mock_with_inertia_result_returns_for_moment_of_inertia_query() {
        let id = GeometryHandleId(1);
        let axis = [0.0, 0.0, 1.0];
        let kernel = MockGeometryKernel::new()
            .with_inertia_result(id, axis, Value::Real(42.0));

        let result = kernel.query(&GeometryQuery::MomentOfInertia { handle: id, axis }).unwrap();
        assert_eq!(result, Value::Real(42.0));
    }

    #[test]
    fn mock_per_query_type_differentiates_same_handle() {
        // Configure different values for Volume vs SurfaceArea on the same handle
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new()
            .with_volume_result(id, mm3(1000.0))
            .with_surface_area_result(id, mm2(600.0));

        let vol = kernel.query(&GeometryQuery::Volume(id)).unwrap();
        let area = kernel.query(&GeometryQuery::SurfaceArea(id)).unwrap();
        assert_eq!(vol, mm3(1000.0));
        assert_eq!(area, mm2(600.0));
    }

    #[test]
    fn mock_per_query_type_falls_back_to_generic() {
        // with_query_result (generic) should be used when no typed config exists
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new()
            .with_query_result(id, mm3(500.0));

        let result = kernel.query(&GeometryQuery::Volume(id)).unwrap();
        assert_eq!(result, mm3(500.0));
    }

    // step-7: failing tests for operation inspection helpers
    #[test]
    fn mock_last_op_empty_returns_none() {
        let kernel = MockGeometryKernel::new();
        assert!(kernel.last_op().is_none());
    }

    #[test]
    fn mock_last_op_returns_most_recent() {
        let mut kernel = MockGeometryKernel::new();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();
        kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();

        let last = kernel.last_op().unwrap();
        assert!(matches!(last.op, GeometryOp::Box { .. }));
        assert_eq!(last.result_handle, GeometryHandleId(2));
    }

    #[test]
    fn mock_op_count_tracks_operations() {
        let mut kernel = MockGeometryKernel::new();
        assert_eq!(kernel.op_count(), 0);

        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();
        assert_eq!(kernel.op_count(), 1);

        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.02),
            })
            .unwrap();
        assert_eq!(kernel.op_count(), 2);
    }

    #[test]
    fn mock_find_ops_filters_by_predicate() {
        let mut kernel = MockGeometryKernel::new();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();
        kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.05),
            })
            .unwrap();

        let spheres = kernel.find_ops(|op| matches!(op, GeometryOp::Sphere { .. }));
        assert_eq!(spheres.len(), 2);
        assert_eq!(spheres[0].result_handle, GeometryHandleId(1));
        assert_eq!(spheres[1].result_handle, GeometryHandleId(3));
    }

    #[test]
    fn mock_has_op_returns_true_when_match_exists() {
        let mut kernel = MockGeometryKernel::new();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();

        assert!(kernel.has_op(|op| matches!(op, GeometryOp::Sphere { .. })));
        assert!(!kernel.has_op(|op| matches!(op, GeometryOp::Box { .. })));
    }

    #[test]
    fn mock_per_query_type_overrides_generic() {
        // Typed config should take precedence over generic
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new()
            .with_query_result(id, mm3(500.0))       // generic
            .with_volume_result(id, mm3(1000.0));     // typed

        let vol = kernel.query(&GeometryQuery::Volume(id)).unwrap();
        assert_eq!(vol, mm3(1000.0)); // typed wins
    }
}
