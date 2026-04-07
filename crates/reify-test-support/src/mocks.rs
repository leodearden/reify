use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex};

use reify_types::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintNodeId, ConstraintResult,
    ConstraintSolver, Diagnostic, ExportError, ExportFormat, GeometryError, GeometryHandle,
    GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, ReprKind,
    ResolutionProblem, Satisfaction, SolveResult, TessError, Value, ValueCellId, ValueMap,
};

/// Create an empty `ResolutionProblem` with all fields set to empty/default values.
pub fn empty_problem() -> ResolutionProblem {
    ResolutionProblem {
        auto_params: vec![],
        constraints: vec![],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    }
}

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
            result: SolveResult::Solved {
                values,
                unique: true,
            },
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
        // Extract the next result (if any) while holding only the results lock.
        // The results lock is dropped at the end of this block, before we touch `self.last`.
        let next = {
            let mut results = self.results.lock().unwrap();
            if results.is_empty() {
                None
            } else {
                Some(results.remove(0))
            }
        };
        // results lock is released — safe to acquire last lock
        match next {
            Some(r) => {
                *self.last.lock().unwrap() = Some(r.clone());
                r
            }
            None => self
                .last
                .lock()
                .unwrap()
                .clone()
                .expect("no results configured"),
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

/// Normalize a distance pair to canonical (min, max) order so that
/// Distance(A, B) and Distance(B, A) map to the same key.
fn normalize_distance_pair(
    a: GeometryHandleId,
    b: GeometryHandleId,
) -> (GeometryHandleId, GeometryHandleId) {
    if a <= b { (a, b) } else { (b, a) }
}

impl QueryKey {
    fn from_query(query: &GeometryQuery) -> Self {
        match query {
            GeometryQuery::Volume(id) => QueryKey::Volume(*id),
            GeometryQuery::SurfaceArea(id) => QueryKey::SurfaceArea(*id),
            GeometryQuery::Centroid(id) => QueryKey::Centroid(*id),
            GeometryQuery::BoundingBox(id) => QueryKey::BoundingBox(*id),
            GeometryQuery::Distance { from, to } => {
                let (lo, hi) = normalize_distance_pair(*from, *to);
                QueryKey::Distance { from: lo, to: hi }
            }
            GeometryQuery::MomentOfInertia { handle, axis } => {
                debug_assert!(
                    !axis[0].is_nan() && !axis[1].is_nan() && !axis[2].is_nan(),
                    "MomentOfInertia axis contains NaN: {:?} — NaN bits break HashMap lookup",
                    axis
                );
                QueryKey::MomentOfInertia {
                    handle: *handle,
                    axis_bits: [axis[0].to_bits(), axis[1].to_bits(), axis[2].to_bits()],
                }
            }
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
        self.typed_queries.insert(QueryKey::Volume(handle), value);
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
        self.typed_queries.insert(QueryKey::Centroid(handle), value);
        self
    }

    /// Configure a BoundingBox query result for a specific handle.
    pub fn with_bbox_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::BoundingBox(handle), value);
        self
    }

    /// Configure a Distance query result for a specific pair of handles.
    ///
    /// The key is normalized to `(min, max)` order so lookups are symmetric —
    /// `with_distance_result(A, B, v)` matches both `Distance { from: A, to: B }`
    /// and `Distance { from: B, to: A }`.
    pub fn with_distance_result(
        mut self,
        from: GeometryHandleId,
        to: GeometryHandleId,
        value: Value,
    ) -> Self {
        let (lo, hi) = normalize_distance_pair(from, to);
        self.typed_queries
            .insert(QueryKey::Distance { from: lo, to: hi }, value);
        self
    }

    /// Configure a MomentOfInertia query result for a specific handle and axis.
    ///
    /// # Panics (debug)
    /// Panics if any axis component is NaN — NaN bits are not equal to themselves,
    /// which would silently break HashMap lookup.
    pub fn with_inertia_result(
        mut self,
        handle: GeometryHandleId,
        axis: [f64; 3],
        value: Value,
    ) -> Self {
        debug_assert!(
            !axis[0].is_nan() && !axis[1].is_nan() && !axis[2].is_nan(),
            "MomentOfInertia axis contains NaN: {:?} — NaN bits break HashMap lookup",
            axis
        );
        self.typed_queries.insert(
            QueryKey::MomentOfInertia {
                handle,
                axis_bits: [axis[0].to_bits(), axis[1].to_bits(), axis[2].to_bits()],
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
    ///
    /// The lock is released before running the predicate to avoid mutex
    /// poisoning if the closure panics (e.g. from a test assertion failure).
    pub fn find_ops<F: Fn(&GeometryOp) -> bool>(&self, f: F) -> Vec<GeometryOpRecord> {
        let ops = self.operations.lock().unwrap().clone();
        ops.iter().filter(|rec| f(&rec.op)).cloned().collect()
    }

    /// Return the total number of operations recorded.
    pub fn op_count(&self) -> usize {
        self.operations.lock().unwrap().len()
    }

    /// Return `true` if any recorded operation matches the predicate.
    ///
    /// The lock is released before running the predicate to avoid mutex
    /// poisoning if the closure panics (e.g. from a test assertion failure).
    pub fn has_op<F: Fn(&GeometryOp) -> bool>(&self, f: F) -> bool {
        let ops = self.operations.lock().unwrap().clone();
        ops.iter().any(|rec| f(&rec.op))
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

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
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
            result: SolveResult::Solved {
                values,
                unique: true,
            },
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

/// Spy constraint solver that captures ALL `ResolutionProblem`s passed to it.
///
/// Unlike `SpyConstraintSolver` which only captures the last call, this records
/// every call in order. Uses a `SequencedMockConstraintSolver` internally to
/// return per-call results.
pub struct MultiCallSpyConstraintSolver {
    captured: Arc<Mutex<Vec<ResolutionProblem>>>,
    inner: SequencedMockConstraintSolver,
}

impl MultiCallSpyConstraintSolver {
    /// Create a multi-call spy with sequenced results.
    /// Each `solve()` call returns the next result from the sequence (last is repeated).
    pub fn new(results: Vec<SolveResult>) -> Self {
        Self {
            captured: Arc::new(Mutex::new(Vec::new())),
            inner: SequencedMockConstraintSolver::new(results),
        }
    }

    /// Return a shared reference to all captured problems (in call order).
    pub fn captured_problems(&self) -> Arc<Mutex<Vec<ResolutionProblem>>> {
        self.captured.clone()
    }

    /// Return the number of times `solve()` has been called.
    pub fn call_count(&self) -> usize {
        self.captured.lock().unwrap().len()
    }
}

impl ConstraintSolver for MultiCallSpyConstraintSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        self.captured.lock().unwrap().push(problem.clone());
        self.inner.solve(problem)
    }
}

/// Run a closure on a background thread with a 10-second deadlock timeout.
///
/// Wraps `f` in [`std::panic::catch_unwind`] so that a panic inside the closure
/// is forwarded to the caller thread (via [`std::panic::resume_unwind`]) rather
/// than being swallowed or misreported as a timeout.  If the closure completes
/// normally its return value is forwarded to the caller.
///
/// # Panics
///
/// * Panics with the original payload if the closure panics.
/// * Panics with a "timed out" message if the closure does not complete within
///   10 seconds (note: the background thread cannot be cancelled and will leak).
/// * Panics with "unexpected disconnect" if the background thread terminates
///   without sending (should not happen in practice).
pub fn run_with_deadlock_timeout<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<T, Box<dyn std::any::Any + Send>>>(1);
    std::thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(f));
        let _ = tx.send(result);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) => resume_unwind(payload),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            // The background thread is still running and cannot be joined.
            // It will be leaked when the test process exits.
            panic!("test timed out after 10s — possible deadlock");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("unexpected thread termination without sending result");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_value_approx;
    use crate::values::{meters, mm2, mm3, point3};
    use reify_types::{CompiledExpr, Type, Value, ValueMap};
    use std::sync::Barrier;

    #[test]
    fn empty_problem_has_all_defaults() {
        let p = empty_problem();
        assert!(p.auto_params.is_empty());
        assert!(p.constraints.is_empty());
        assert!(p.current_values.is_empty());
        assert!(p.objective.is_none());
        assert!(p.functions.is_empty());
    }

    #[test]
    fn mock_constraint_checker_predetermined() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let checker =
            MockConstraintChecker::new().with_result(cnid.clone(), Satisfaction::Violated);

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = ConstraintInput {
            constraints: vec![(cnid.clone(), &expr)],
            values: &values,
            functions: &[],
            determinacy: None,
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
        let problem = empty_problem();

        match solver.solve(&problem) {
            SolveResult::Solved { values: v, .. } => {
                assert_eq!(v.len(), 1);
                assert!(v.contains_key(&ValueCellId::new("S", "x")));
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn mock_constraint_solver_infeasible() {
        let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
            "constraints are infeasible",
        )]);
        let problem = empty_problem();

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
        let problem = empty_problem();

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

        let _boxed: Box<dyn ConstraintSolver> =
            Box::new(MockConstraintSolver::new_no_progress("test"));
    }

    // step-5: failing tests for per-query-type mock configuration
    #[test]
    fn mock_with_volume_result_returns_for_volume_query() {
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new().with_volume_result(id, mm3(1000.0));

        let result = kernel.query(&GeometryQuery::Volume(id)).unwrap();
        assert_eq!(result, mm3(1000.0));
    }

    #[test]
    fn mock_with_surface_area_result_returns_for_surface_area_query() {
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new().with_surface_area_result(id, mm2(600.0));

        let result = kernel.query(&GeometryQuery::SurfaceArea(id)).unwrap();
        assert_eq!(result, mm2(600.0));
    }

    #[test]
    fn mock_with_centroid_result_returns_for_centroid_query() {
        let id = GeometryHandleId(1);
        let centroid = point3(0.5, 0.5, 0.5);
        let kernel = MockGeometryKernel::new().with_centroid_result(id, centroid.clone());

        let result = kernel.query(&GeometryQuery::Centroid(id)).unwrap();
        assert_eq!(result, centroid);
    }

    #[test]
    fn mock_with_bbox_result_returns_for_bounding_box_query() {
        let id = GeometryHandleId(1);
        let bbox = Value::List(vec![point3(0.0, 0.0, 0.0), point3(1.0, 1.0, 1.0)]);
        let kernel = MockGeometryKernel::new().with_bbox_result(id, bbox.clone());

        let result = kernel.query(&GeometryQuery::BoundingBox(id)).unwrap();
        assert_eq!(result, bbox);
    }

    #[test]
    fn mock_with_distance_result_returns_for_distance_query() {
        let from = GeometryHandleId(1);
        let to = GeometryHandleId(2);
        let kernel = MockGeometryKernel::new().with_distance_result(from, to, meters(5.0));

        let result = kernel.query(&GeometryQuery::Distance { from, to }).unwrap();
        assert_eq!(result, meters(5.0));
    }

    #[test]
    fn mock_with_inertia_result_returns_for_moment_of_inertia_query() {
        let id = GeometryHandleId(1);
        let axis = [0.0, 0.0, 1.0];
        let kernel = MockGeometryKernel::new().with_inertia_result(id, axis, Value::Real(42.0));

        let result = kernel
            .query(&GeometryQuery::MomentOfInertia { handle: id, axis })
            .unwrap();
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
        let kernel = MockGeometryKernel::new().with_query_result(id, mm3(500.0));

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

    // step-9: tests exercising all transform ops
    #[test]
    fn mock_execute_translate_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let base = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Translate {
                target: base.id,
                dx: 1.0,
                dy: 2.0,
                dz: 3.0,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        let ops = kernel.operations();
        assert_eq!(ops.len(), 2);
        match &ops[1].op {
            GeometryOp::Translate { target, dx, dy, dz } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert!((dx - 1.0).abs() < 1e-12);
                assert!((dy - 2.0).abs() < 1e-12);
                assert!((dz - 3.0).abs() < 1e-12);
            }
            other => panic!("expected Translate, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_rotate_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let base = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.05),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Rotate {
                target: base.id,
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Rotate {
                target,
                axis,
                angle_rad,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*axis, [0.0, 0.0, 1.0]);
                assert!((angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
            }
            other => panic!("expected Rotate, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_scale_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let base = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Scale {
                target: base.id,
                factor: 2.5,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Scale { target, factor } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert!((factor - 2.5).abs() < 1e-12);
            }
            other => panic!("expected Scale, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_rotate_around_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let base = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::length(0.02),
                height: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::RotateAround {
                target: base.id,
                point: [1.0, 0.0, 0.0],
                axis: [0.0, 1.0, 0.0],
                angle_rad: std::f64::consts::PI,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::RotateAround {
                target,
                point,
                axis,
                angle_rad,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*point, [1.0, 0.0, 0.0]);
                assert_eq!(*axis, [0.0, 1.0, 0.0]);
                assert!((angle_rad - std::f64::consts::PI).abs() < 1e-12);
            }
            other => panic!("expected RotateAround, got {:?}", other),
        }
    }

    // step-9 continued: tests exercising boolean ops
    #[test]
    fn mock_execute_union_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let left = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();
        let right = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.05),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Union {
                left: left.id,
                right: right.id,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(3));
        match &kernel.operations()[2].op {
            GeometryOp::Union { left, right } => {
                assert_eq!(*left, GeometryHandleId(1));
                assert_eq!(*right, GeometryHandleId(2));
            }
            other => panic!("expected Union, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_difference_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let left = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();
        let right = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::length(0.02),
                height: Value::length(0.2),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Difference {
                left: left.id,
                right: right.id,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(3));
        match &kernel.operations()[2].op {
            GeometryOp::Difference { left, right } => {
                assert_eq!(*left, GeometryHandleId(1));
                assert_eq!(*right, GeometryHandleId(2));
            }
            other => panic!("expected Difference, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_intersection_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let left = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();
        let right = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.08),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Intersection {
                left: left.id,
                right: right.id,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(3));
        match &kernel.operations()[2].op {
            GeometryOp::Intersection { left, right } => {
                assert_eq!(*left, GeometryHandleId(1));
                assert_eq!(*right, GeometryHandleId(2));
            }
            other => panic!("expected Intersection, got {:?}", other),
        }
    }

    // step-11: tests exercising shape and manufacturing ops
    #[test]
    fn mock_execute_extrude_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let profile = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.001),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Extrude {
                profile: profile.id,
                distance: Value::length(0.05),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Extrude { profile, distance } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert_eq!(*distance, Value::length(0.05));
            }
            other => panic!("expected Extrude, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_revolve_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let profile = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.05),
                depth: Value::length(0.001),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Revolve {
                profile: profile.id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::TAU,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert_eq!(*axis_origin, [0.0, 0.0, 0.0]);
                assert_eq!(*axis_dir, [0.0, 0.0, 1.0]);
                assert!((angle_rad - std::f64::consts::TAU).abs() < 1e-12);
            }
            other => panic!("expected Revolve, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_sweep_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let profile = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();
        let path = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::length(0.005),
                height: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Sweep {
                profile: profile.id,
                path: path.id,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(3));
        match &kernel.operations()[2].op {
            GeometryOp::Sweep { profile, path } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert_eq!(*path, GeometryHandleId(2));
            }
            other => panic!("expected Sweep, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_loft_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let p1 = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.05),
            })
            .unwrap();
        let p2 = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.03),
            })
            .unwrap();
        let p3 = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Loft {
                profiles: vec![p1.id, p2.id, p3.id],
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(4));
        match &kernel.operations()[3].op {
            GeometryOp::Loft { profiles } => {
                assert_eq!(
                    *profiles,
                    vec![
                        GeometryHandleId(1),
                        GeometryHandleId(2),
                        GeometryHandleId(3)
                    ]
                );
            }
            other => panic!("expected Loft, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_draft_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.05),
            })
            .unwrap();
        let plane = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(1.0),
                height: Value::length(1.0),
                depth: Value::length(0.001),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Draft {
                target: target.id,
                angle: Value::Real(0.05),
                plane: plane.id,
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(3));
        match &kernel.operations()[2].op {
            GeometryOp::Draft {
                target,
                angle,
                plane,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*angle, Value::Real(0.05));
                assert_eq!(*plane, GeometryHandleId(2));
            }
            other => panic!("expected Draft, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_thicken_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.05),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Thicken {
                target: target.id,
                offset: Value::length(0.002),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Thicken { target, offset } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*offset, Value::length(0.002));
            }
            other => panic!("expected Thicken, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_shell_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Shell {
                target: target.id,
                thickness: Value::length(0.003),
                faces_to_remove: vec![0, 3],
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Shell {
                target,
                thickness,
                faces_to_remove,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*thickness, Value::length(0.003));
                assert_eq!(*faces_to_remove, vec![0, 3]);
            }
            other => panic!("expected Shell, got {:?}", other),
        }
    }

    // step-13: tests exercising pattern and edge ops
    #[test]
    fn mock_execute_linear_pattern_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.01),
                height: Value::length(0.01),
                depth: Value::length(0.01),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::LinearPattern {
                target: target.id,
                direction: [1.0, 0.0, 0.0],
                count: 5,
                spacing: Value::length(0.02),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::LinearPattern {
                target,
                direction,
                count,
                spacing,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*direction, [1.0, 0.0, 0.0]);
                assert_eq!(*count, 5);
                assert_eq!(*spacing, Value::length(0.02));
            }
            other => panic!("expected LinearPattern, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_circular_pattern_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::length(0.005),
                height: Value::length(0.02),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::CircularPattern {
                target: target.id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                count: 6,
                angle: Value::Real(std::f64::consts::TAU),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::CircularPattern {
                target,
                axis_origin,
                axis_dir,
                count,
                angle,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*axis_origin, [0.0, 0.0, 0.0]);
                assert_eq!(*axis_dir, [0.0, 0.0, 1.0]);
                assert_eq!(*count, 6);
                assert_eq!(*angle, Value::Real(std::f64::consts::TAU));
            }
            other => panic!("expected CircularPattern, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_mirror_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.05),
                height: Value::length(0.05),
                depth: Value::length(0.05),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Mirror {
                target: target.id,
                plane_origin: [0.0, 0.0, 0.0],
                plane_normal: [1.0, 0.0, 0.0],
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*plane_origin, [0.0, 0.0, 0.0]);
                assert_eq!(*plane_normal, [1.0, 0.0, 0.0]);
            }
            other => panic!("expected Mirror, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_fillet_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Fillet {
                target: target.id,
                radius: Value::length(0.005),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Fillet { target, radius } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*radius, Value::length(0.005));
            }
            other => panic!("expected Fillet, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_chamfer_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.1),
                depth: Value::length(0.1),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::Chamfer {
                target: target.id,
                distance: Value::length(0.003),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::Chamfer { target, distance } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*distance, Value::length(0.003));
            }
            other => panic!("expected Chamfer, got {:?}", other),
        }
    }

    #[test]
    fn mock_per_query_type_overrides_generic() {
        // Typed config should take precedence over generic
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new()
            .with_query_result(id, mm3(500.0)) // generic
            .with_volume_result(id, mm3(1000.0)); // typed

        let vol = kernel.query(&GeometryQuery::Volume(id)).unwrap();
        assert_eq!(vol, mm3(1000.0)); // typed wins
    }

    // step-15: integration test — multi-op workflow with queries + inspection
    #[test]
    fn mock_multi_op_workflow_with_queries_and_inspection() {
        let mut kernel = MockGeometryKernel::new();

        // Create a box
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.1),
                height: Value::length(0.2),
                depth: Value::length(0.05),
            })
            .unwrap();
        assert_eq!(box_h.id, GeometryHandleId(1));

        // Scale the box
        let scaled_h = kernel
            .execute(&GeometryOp::Scale {
                target: box_h.id,
                factor: 2.0,
            })
            .unwrap();
        assert_eq!(scaled_h.id, GeometryHandleId(2));

        // Translate the scaled box
        let translated_h = kernel
            .execute(&GeometryOp::Translate {
                target: scaled_h.id,
                dx: 0.5,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        assert_eq!(translated_h.id, GeometryHandleId(3));

        // Create a linear pattern
        let pattern_h = kernel
            .execute(&GeometryOp::LinearPattern {
                target: translated_h.id,
                direction: [1.0, 0.0, 0.0],
                count: 3,
                spacing: Value::length(0.3),
            })
            .unwrap();
        assert_eq!(pattern_h.id, GeometryHandleId(4));

        // Verify operation count
        assert_eq!(kernel.op_count(), 4);

        // Verify inspection helpers
        assert!(kernel.has_op(|op| matches!(op, GeometryOp::Scale { .. })));
        assert!(kernel.has_op(|op| matches!(op, GeometryOp::LinearPattern { .. })));
        assert!(!kernel.has_op(|op| matches!(op, GeometryOp::Fillet { .. })));

        let last = kernel.last_op().unwrap();
        assert!(matches!(last.op, GeometryOp::LinearPattern { .. }));
        assert_eq!(last.result_handle, GeometryHandleId(4));

        let boxes = kernel.find_ops(|op| matches!(op, GeometryOp::Box { .. }));
        assert_eq!(boxes.len(), 1);

        // Configure per-query-type results and verify queries
        // Note: kernel needs to be rebuilt since it was consumed by execute (mut)
        // But with_*_result consumes self, so we build a new kernel for query tests.
        let query_kernel = MockGeometryKernel::new()
            .with_volume_result(pattern_h.id, mm3(8000.0))
            .with_bbox_result(
                pattern_h.id,
                Value::List(vec![point3(0.0, 0.0, 0.0), point3(1.0, 0.4, 0.1)]),
            );

        let volume = query_kernel
            .query(&GeometryQuery::Volume(pattern_h.id))
            .unwrap();
        assert_eq!(volume, mm3(8000.0));

        let bbox = query_kernel
            .query(&GeometryQuery::BoundingBox(pattern_h.id))
            .unwrap();
        match bbox {
            Value::List(items) => {
                assert_eq!(items.len(), 2);
                assert_value_approx!(items[0], point3(0.0, 0.0, 0.0));
                assert_value_approx!(items[1], point3(1.0, 0.4, 0.1));
            }
            other => panic!("expected List, got {:?}", other),
        }

        // Verify that querying an unconfigured query type falls back correctly
        let fallback_kernel = MockGeometryKernel::new()
            .with_query_result(GeometryHandleId(1), meters(42.0))
            .with_volume_result(GeometryHandleId(1), mm3(100.0));

        // Volume uses typed result
        let vol = fallback_kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        assert_eq!(vol, mm3(100.0));

        // SurfaceArea falls back to generic
        let area = fallback_kernel
            .query(&GeometryQuery::SurfaceArea(GeometryHandleId(1)))
            .unwrap();
        assert_eq!(area, meters(42.0));
    }

    #[test]
    fn mock_find_ops_does_not_poison_mutex_on_closure_panic() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut kernel = MockGeometryKernel::new();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();

        // Call find_ops with a closure that panics — catch the panic
        let result = catch_unwind(AssertUnwindSafe(|| {
            kernel.find_ops(|_op| panic!("deliberate panic inside find_ops closure"));
        }));
        assert!(result.is_err(), "closure should have panicked");

        // After the caught panic, the mutex must NOT be poisoned:
        // op_count() and last_op() should still work.
        assert_eq!(kernel.op_count(), 1);
        assert!(kernel.last_op().is_some());
    }

    // --- Distance query key symmetry tests (task 430) ---

    #[test]
    fn distance_query_key_is_symmetric() {
        let from = GeometryHandleId(1);
        let to = GeometryHandleId(2);
        // Configure with (1, 2) but query with (2, 1)
        let kernel = MockGeometryKernel::new().with_distance_result(from, to, meters(5.0));

        let result = kernel
            .query(&GeometryQuery::Distance { from: to, to: from })
            .unwrap();
        assert_eq!(result, meters(5.0));
    }

    #[test]
    fn distance_same_handle_is_identity() {
        let id = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new().with_distance_result(id, id, meters(0.0));

        let result = kernel
            .query(&GeometryQuery::Distance { from: id, to: id })
            .unwrap();
        assert_eq!(result, meters(0.0));
    }

    #[test]
    fn distance_result_symmetric_via_reversed_config() {
        // Configure with higher id first: (3, 1), query with lower id first: (1, 3)
        let kernel = MockGeometryKernel::new().with_distance_result(
            GeometryHandleId(3),
            GeometryHandleId(1),
            meters(7.0),
        );

        let result = kernel
            .query(&GeometryQuery::Distance {
                from: GeometryHandleId(1),
                to: GeometryHandleId(3),
            })
            .unwrap();
        assert_eq!(result, meters(7.0));
    }

    // --- SequencedMockConstraintSolver tests (step-1, task 430) ---

    #[test]
    fn sequenced_solver_returns_results_in_order() {
        let mut values1 = HashMap::new();
        values1.insert(ValueCellId::new("S", "x"), Value::length(0.001));
        let mut values2 = HashMap::new();
        values2.insert(ValueCellId::new("S", "x"), Value::length(0.002));
        let mut values3 = HashMap::new();
        values3.insert(ValueCellId::new("S", "x"), Value::length(0.003));

        let solver = SequencedMockConstraintSolver::new(vec![
            SolveResult::Solved {
                values: values1.clone(),
                unique: true,
            },
            SolveResult::Solved {
                values: values2.clone(),
                unique: true,
            },
            SolveResult::Solved {
                values: values3.clone(),
                unique: true,
            },
        ]);

        let problem = empty_problem();

        // Each call returns the next result in sequence
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values1),
            other => panic!("expected Solved #1, got {:?}", other),
        }
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values2),
            other => panic!("expected Solved #2, got {:?}", other),
        }
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values3),
            other => panic!("expected Solved #3, got {:?}", other),
        }
    }

    #[test]
    fn sequenced_solver_repeats_last_after_exhaustion() {
        let mut values1 = HashMap::new();
        values1.insert(ValueCellId::new("S", "a"), Value::length(0.01));
        let mut values2 = HashMap::new();
        values2.insert(ValueCellId::new("S", "b"), Value::length(0.02));

        let solver = SequencedMockConstraintSolver::new(vec![
            SolveResult::Solved {
                values: values1.clone(),
                unique: true,
            },
            SolveResult::Solved {
                values: values2.clone(),
                unique: true,
            },
        ]);

        let problem = empty_problem();

        // Consume both results
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values1),
            other => panic!("expected Solved #1, got {:?}", other),
        }
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values2),
            other => panic!("expected Solved #2, got {:?}", other),
        }

        // 3rd and 4th calls should repeat the last result
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values2),
            other => panic!("expected Solved #3 (repeated last), got {:?}", other),
        }
        match solver.solve(&problem) {
            SolveResult::Solved { values, .. } => assert_eq!(values, values2),
            other => panic!("expected Solved #4 (repeated last), got {:?}", other),
        }
    }

    #[test]
    fn mock_has_op_does_not_poison_mutex_on_closure_panic() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut kernel = MockGeometryKernel::new();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();

        // Call has_op with a closure that panics — catch the panic
        let result = catch_unwind(AssertUnwindSafe(|| {
            kernel.has_op(|_op| panic!("deliberate panic inside has_op closure"));
        }));
        assert!(result.is_err(), "closure should have panicked");

        // After the caught panic, the mutex must NOT be poisoned:
        // op_count() and last_op() should still work.
        assert_eq!(kernel.op_count(), 1);
        assert!(kernel.last_op().is_some());
    }

    #[test]
    fn mock_unconfigured_handle_query_returns_error() {
        let mut kernel = MockGeometryKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::length(0.01),
            })
            .unwrap();

        // Query without configuring any result for the handle
        let result = kernel.query(&GeometryQuery::Volume(handle.id));
        match result {
            Err(QueryError::QueryFailed(msg)) => {
                assert!(
                    msg.contains(&format!("{:?}", handle.id)),
                    "error message should contain handle id, got: {}",
                    msg
                );
            }
            other => panic!(
                "expected Err(QueryFailed) for unconfigured handle, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn distance_query_unregistered_pair_returns_error() {
        // Configure distance for pair (1, 2)
        let kernel = MockGeometryKernel::new().with_distance_result(
            GeometryHandleId(1),
            GeometryHandleId(2),
            meters(5.0),
        );

        // Query an unregistered pair (1, 3) — should return Err(QueryFailed)
        let result = kernel.query(&GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(3),
        });
        match result {
            Err(QueryError::QueryFailed(msg)) => {
                assert!(
                    msg.contains(&format!("{:?}", GeometryHandleId(1))),
                    "error message should contain handle id, got: {}",
                    msg
                );
            }
            other => panic!(
                "expected Err(QueryFailed) for unregistered distance pair, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn sequenced_solver_concurrent_no_deadlock() {
        // Pre-load 4 distinct results that threads race to consume
        // from `results` and writes to `self.last`.  Distinct values let us
        // verify every slot is consumed exactly once (no double-consumption).
        // This exercises concurrent acquisition of both locks without any
        // ordering assumption between threads, verifying the task-430 fix
        // (separate lock acquisition for `results` and `last`) doesn't
        // deadlock.
        let expected_slots: Vec<HashMap<ValueCellId, Value>> = (0..4)
            .map(|i| {
                let mut m = HashMap::new();
                m.insert(
                    ValueCellId::new("S", "x"),
                    Value::length(0.001 * (i + 1) as f64),
                );
                m
            })
            .collect();

        let solver = SequencedMockConstraintSolver::new(
            expected_slots
                .iter()
                .map(|v| SolveResult::Solved {
                    values: v.clone(),
                    unique: true,
                })
                .collect(),
        );

        let problem = empty_problem();

        // Run inside a spawned thread so we can apply a timeout — a real
        // deadlock would hang CI forever without this.
        let results = run_with_deadlock_timeout(move || {
            let collected = Mutex::new(Vec::new());
            // 4 threads each calling solve() once — threads race to pop
            // the next available result (order is non-deterministic).
            std::thread::scope(|s| {
                for _ in 0..4 {
                    s.spawn(|| {
                        let result = solver.solve(&problem);
                        collected.lock().unwrap().push(result);
                    });
                }
            });
            collected.into_inner().unwrap_or_else(|e| e.into_inner())
        });

        assert_eq!(results.len(), 4, "all 4 threads should complete");

        // Collect the returned x-values and verify each distinct slot was
        // consumed exactly once (detects double-consumption races).
        let mut seen: Vec<f64> = results
            .iter()
            .map(|r| match r {
                SolveResult::Solved { values, .. } => {
                    let v = values
                        .get(&ValueCellId::new("S", "x"))
                        .expect("missing x value");
                    match v {
                        Value::Scalar { si_value, .. } => *si_value,
                        other => panic!("expected Scalar, got {:?}", other),
                    }
                }
                other => panic!("expected Solved variant, got {:?}", other),
            })
            .collect();
        seen.sort_by(f64::total_cmp);
        let mut expected: Vec<f64> = expected_slots
            .iter()
            .map(|m| match m.get(&ValueCellId::new("S", "x")).unwrap() {
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("expected Scalar, got {:?}", other),
            })
            .collect();
        expected.sort_by(f64::total_cmp);
        // Exact f64 equality is safe here: the mock stores and returns
        // values verbatim with no arithmetic transformation, so bit-exact
        // round-trip equality holds.
        assert_eq!(
            seen, expected,
            "each distinct result slot should be consumed exactly once"
        );
    }

    #[test]
    fn sequenced_solver_concurrent_last_fallback() {
        // Deterministic test for the `self.last` fallback path under
        // concurrency.  Phase 1: a single thread consumes the only queued
        // result and writes `self.last`.  A Barrier ensures phase-1 completes
        // before phase-2 threads start, so they are guaranteed to see
        // `self.last == Some(...)` rather than racing with the writer.
        let mut expected_values = HashMap::new();
        expected_values.insert(ValueCellId::new("S", "x"), Value::length(0.001));

        let expected_clone = expected_values.clone();

        // Run inside a spawned thread so we can apply a timeout — a real
        // deadlock would hang CI forever without this.
        let (phase1_result, results) = run_with_deadlock_timeout(move || {
            let solver = SequencedMockConstraintSolver::new(vec![SolveResult::Solved {
                values: expected_clone.clone(),
                unique: true,
            }]);

            let problem = empty_problem();

            // Phase 1: consume the queued result, populating `self.last`.
            let phase1_result = solver.solve(&problem);

            // Phase 2: 3 threads concurrently hit the `last` fallback path.
            let barrier = Barrier::new(3);
            let collected = Mutex::new(Vec::new());

            std::thread::scope(|s| {
                for _ in 0..3 {
                    s.spawn(|| {
                        // Synchronize so all 3 threads contend on `self.last`
                        // simultaneously, maximizing the chance of exposing any
                        // deadlock in the two-lock pattern.
                        barrier.wait();
                        let result = solver.solve(&problem);
                        collected.lock().unwrap().push(result);
                    });
                }
            });

            (
                phase1_result,
                collected.into_inner().unwrap_or_else(|e| e.into_inner()),
            )
        });

        match &phase1_result {
            SolveResult::Solved { values, .. } => {
                assert_eq!(*values, expected_values);
            }
            other => panic!("expected Solved, got {:?}", other),
        }

        assert_eq!(results.len(), 3, "all 3 fallback threads should complete");
        for result in &results {
            match result {
                SolveResult::Solved { values, .. } => {
                    assert_eq!(
                        *values, expected_values,
                        "fallback threads should return the last result"
                    );
                }
                other => panic!("expected Solved variant from fallback, got {:?}", other),
            }
        }
    }

    #[test]
    #[should_panic(expected = "no results configured")]
    fn sequenced_solver_panics_on_empty_vec() {
        let solver = SequencedMockConstraintSolver::new(vec![]);
        let problem = empty_problem();
        // Should panic with "no results configured"
        solver.solve(&problem);
    }

    #[test]
    fn normalize_distance_pair_canonical_order() {
        let lo = GeometryHandleId(1);
        let hi = GeometryHandleId(5);

        // (high, low) → (low, high)
        assert_eq!(normalize_distance_pair(hi, lo), (lo, hi));
        // (low, high) → unchanged
        assert_eq!(normalize_distance_pair(lo, hi), (lo, hi));
        // equal IDs → (id, id)
        assert_eq!(normalize_distance_pair(lo, lo), (lo, lo));
    }

    #[test]
    fn multi_call_spy_records_all_calls_and_returns_sequenced_results() {
        use reify_types::{AutoParam, Type, ValueMap};

        let mut values_a = HashMap::new();
        values_a.insert(ValueCellId::new("A", "x"), Value::length(0.005));
        let mut values_b = HashMap::new();
        values_b.insert(ValueCellId::new("B", "y"), Value::length(0.010));

        let spy = MultiCallSpyConstraintSolver::new(vec![
            SolveResult::Solved {
                values: values_a,
                unique: true,
            },
            SolveResult::Solved {
                values: values_b,
                unique: true,
            },
        ]);
        let captured = spy.captured_problems();

        // First call
        let problem1 = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: ValueCellId::new("A", "x"),
                param_type: Type::length(),
                bounds: None,
                free: false,
            }],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![],
        };
        let result1 = spy.solve(&problem1);
        assert!(
            matches!(&result1, SolveResult::Solved { values, unique: true } if values.contains_key(&ValueCellId::new("A", "x"))),
            "first call should return values_a"
        );

        // Second call
        let problem2 = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: ValueCellId::new("B", "y"),
                param_type: Type::length(),
                bounds: None,
                free: false,
            }],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![],
        };
        let result2 = spy.solve(&problem2);
        assert!(
            matches!(&result2, SolveResult::Solved { values, unique: true } if values.contains_key(&ValueCellId::new("B", "y"))),
            "second call should return values_b"
        );

        // Verify call count and captured problems
        assert_eq!(spy.call_count(), 2);
        let problems = captured.lock().unwrap();
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0].auto_params[0].id, ValueCellId::new("A", "x"));
        assert_eq!(problems[1].auto_params[0].id, ValueCellId::new("B", "y"));
    }

    // --- run_with_deadlock_timeout helper tests ---

    #[test]
    fn run_with_deadlock_timeout_returns_value() {
        let result = run_with_deadlock_timeout(|| 42i32);
        assert_eq!(result, 42);
    }

    #[test]
    #[should_panic(expected = "deliberate test panic")]
    fn run_with_deadlock_timeout_forwards_panic() {
        run_with_deadlock_timeout(|| {
            panic!("deliberate test panic");
        });
    }

    #[test]
    fn run_with_deadlock_timeout_returns_from_scoped_threads() {
        // Validates the exact pattern used by the refactored concurrent tests:
        // thread::scope + Mutex<Vec<i32>>, recovering the Vec with
        // unwrap_or_else(|e| e.into_inner()) in case of mutex poisoning.
        let result: Vec<i32> = run_with_deadlock_timeout(|| {
            let collected = Mutex::new(Vec::new());
            std::thread::scope(|s| {
                for i in 0..4i32 {
                    let collected_ref = &collected;
                    s.spawn(move || {
                        collected_ref.lock().unwrap().push(i);
                    });
                }
            });
            collected.into_inner().unwrap_or_else(|e| e.into_inner())
        });
        assert_eq!(result.len(), 4, "all 4 scoped threads should complete");
    }
}
