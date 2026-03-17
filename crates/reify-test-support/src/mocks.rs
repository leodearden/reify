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

/// Record of operations received by MockGeometryKernel.
#[derive(Debug, Clone)]
pub struct GeometryOpRecord {
    pub op: GeometryOp,
    pub result_handle: GeometryHandleId,
}

/// Mock geometry kernel that tracks operations and returns dummy handles.
pub struct MockGeometryKernel {
    next_id: u64,
    operations: Arc<Mutex<Vec<GeometryOpRecord>>>,
    queries: HashMap<GeometryHandleId, Value>,
}

impl MockGeometryKernel {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            operations: Arc::new(Mutex::new(Vec::new())),
            queries: HashMap::new(),
        }
    }

    /// Configure a query response for a specific handle.
    pub fn with_query_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.queries.insert(handle, value);
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
        let handle_id = match query {
            GeometryQuery::Volume(id) => id,
            GeometryQuery::SurfaceArea(id) => id,
            GeometryQuery::Centroid(id) => id,
            GeometryQuery::BoundingBox(id) => id,
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
