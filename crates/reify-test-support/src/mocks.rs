use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use reify_core::{ConstraintNodeId, Diagnostic, Type, ValueCellId};
use reify_ir::{AutoParam, BRepKind, ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, ConstraintSolver, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, OptimizedImpl, OptimizedImplInput, OptimizedImplOutput, QueryError, ResolutionProblem, Satisfaction, SolveResult, TessError, Value, ValueMap};

/// Create an empty `ResolutionProblem` with all fields set to empty/default values.
pub fn empty_problem() -> ResolutionProblem {
    ResolutionProblem {
        auto_params: vec![],
        constraints: vec![],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    }
}

/// Standard single-param convenience for constraint-solving tests.
///
/// Returns an `AutoParam` with `param_type = Type::length()`, `bounds = None`,
/// `free = false`, and `id = cell_id`.  Callers that need a `Vec` can wrap with
/// `vec![single_auto_param(cell_id)]`.
pub fn single_auto_param(cell_id: ValueCellId) -> AutoParam {
    AutoParam {
        id: cell_id,
        param_type: Type::length(),
        bounds: None,
        free: false,
    }
}

/// Mock constraint checker that returns predetermined results.
///
/// Three configuration channels, applied in priority order on each `check`:
/// 1. **Call queue (FIFO)** — `with_call_queue(...)` populates an ordered
///    queue. Each `check(...)` invocation pops one `Satisfaction` from the
///    head and applies it to every constraint in that call's input. Once the
///    queue is exhausted, subsequent calls fall through to the per-id map.
///    Used by v0.2 DFS backtracking tests to express
///    "leaf 1 violated, leaf 2 satisfied" without needing real
///    type-substitution mechanics (which are deferred per the PRD).
/// 2. **Per-id results** — `with_result(id, satisfaction)` overrides for a
///    specific `ConstraintNodeId`.
/// 3. **Default** — `with_default(satisfaction)` is the fallback for ids not
///    in the per-id map.
/// 4. **Call tracking** — `calls()` / `calls_handle()` expose every
///    `ConstraintNodeId` seen across all `check()` invocations, regardless of
///    which response channel produced the verdict. Mirrors
///    `MockOptimizedImpl::calls` / `calls_handle`.
pub struct MockConstraintChecker {
    results: HashMap<ConstraintNodeId, Satisfaction>,
    default: Satisfaction,
    call_queue: Arc<Mutex<VecDeque<Satisfaction>>>,
    calls: Arc<Mutex<Vec<ConstraintNodeId>>>,
}

impl MockConstraintChecker {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            default: Satisfaction::Satisfied,
            call_queue: Arc::new(Mutex::new(VecDeque::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
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

    /// Configure a per-call FIFO response queue.
    ///
    /// Each `check(...)` invocation pops one `Satisfaction` from the queue
    /// head and applies it to every constraint in that call's input. Once the
    /// queue is exhausted, subsequent calls fall through to the per-id map
    /// (`with_result`) and `default` (`with_default`) channels — i.e. the
    /// existing behavior is fully preserved when the queue is unused or
    /// drained.
    ///
    /// This is the minimal extension that lets v0.2 DFS leaf-iteration tests
    /// express a sequence of verdicts without requiring substitution
    /// mechanics. The `ConstraintChecker` trait and `ConstraintInput` shape
    /// remain unchanged.
    pub fn with_call_queue(self, queue: Vec<Satisfaction>) -> Self {
        {
            let mut guard = self
                .call_queue
                .lock()
                .expect("MockConstraintChecker::with_call_queue: mutex poisoned");
            guard.clear();
            guard.extend(queue);
        }
        self
    }

    /// Snapshot of every `ConstraintNodeId` this checker has been invoked
    /// with, in call order. A cloned `Vec` so callers can inspect it without
    /// holding the internal lock.
    pub fn calls(&self) -> Vec<ConstraintNodeId> {
        self.calls
            .lock()
            .expect("MockConstraintChecker::calls: mutex poisoned")
            .clone()
    }

    /// A clone of the shared call-tracking handle.
    ///
    /// Useful when the mock itself has been moved into a
    /// `Box<dyn ConstraintChecker>` (or passed into a resolver by reference)
    /// and is no longer reachable by the test after the call. Callers grab a
    /// handle *before* boxing, then assert against it after the run:
    ///
    /// ```ignore
    /// let mock = MockConstraintChecker::new();
    /// let calls = mock.calls_handle();
    /// resolve_auto_type_params_with_backtracking(..., &mock, ...);
    /// assert_eq!(calls.lock().unwrap().len(), 5);
    /// ```
    pub fn calls_handle(&self) -> Arc<Mutex<Vec<ConstraintNodeId>>> {
        Arc::clone(&self.calls)
    }
}

impl Default for MockConstraintChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstraintChecker for MockConstraintChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        // Priority 1: pop one verdict from the call-queue head, if any. When
        // present, apply it uniformly to every constraint in this call's
        // input — this is what makes the queue "per-call" rather than
        // "per-constraint", which is the semantic that DFS backtracking
        // tests need (one leaf check ⇒ one verdict).
        let queued = self
            .call_queue
            .lock()
            .expect("MockConstraintChecker::check: mutex poisoned")
            .pop_front();
        if let Some(satisfaction) = queued {
            let mut calls = self
                .calls
                .lock()
                .expect("MockConstraintChecker::check: mutex poisoned");
            return input
                .constraints
                .iter()
                .map(|(id, _)| {
                    calls.push(id.clone());
                    ConstraintResult {
                        id: id.clone(),
                        satisfaction,
                        diagnostics: ConstraintDiagnostics::default(),
                    }
                })
                .collect();
        }

        // Priority 2 + 3: existing per-id map → default fallback. Unchanged
        // for callers that never populate the queue.
        let mut calls = self
            .calls
            .lock()
            .expect("MockConstraintChecker::check: mutex poisoned");
        input
            .constraints
            .iter()
            .map(|(id, _)| {
                calls.push(id.clone());
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

/// Mock optimized-constraint implementation used to exercise the `@optimized`
/// dispatch path in reify-eval (Task 273).
///
/// Mirrors `MockConstraintChecker` in shape — configurable per-id results and
/// a default — and additionally records every `ConstraintNodeId` it was
/// invoked with. Tests can read `calls()` to assert that dispatch routed a
/// constraint through the optimized path instead of the language-level
/// `ConstraintChecker`.
pub struct MockOptimizedImpl {
    results: HashMap<ConstraintNodeId, Satisfaction>,
    default: Satisfaction,
    calls: Arc<Mutex<Vec<ConstraintNodeId>>>,
}

impl MockOptimizedImpl {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            default: Satisfaction::Satisfied,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set the result for a specific constraint id.
    pub fn with_result(mut self, id: ConstraintNodeId, satisfaction: Satisfaction) -> Self {
        self.results.insert(id, satisfaction);
        self
    }

    /// Set the default result for constraints not explicitly configured.
    pub fn with_default(mut self, satisfaction: Satisfaction) -> Self {
        self.default = satisfaction;
        self
    }

    /// Snapshot of every `ConstraintNodeId` this impl has been invoked with,
    /// in call order. A cloned Vec so callers can inspect it without holding
    /// the internal lock.
    pub fn calls(&self) -> Vec<ConstraintNodeId> {
        self.calls.lock().unwrap().clone()
    }

    /// A clone of the shared call-tracking handle.
    ///
    /// Useful when the mock itself has been moved into a `Box<dyn OptimizedImpl>`
    /// (e.g. registered on an `Engine`) and is no longer reachable by the test.
    /// Callers grab a handle *before* boxing, then assert against it after the
    /// engine run:
    ///
    /// ```ignore
    /// let mock = MockOptimizedImpl::new();
    /// let calls = mock.calls_handle();
    /// engine.register_optimized_impl("target_a", Box::new(mock));
    /// engine.check(&compiled);
    /// assert_eq!(calls.lock().unwrap().len(), 1);
    /// ```
    pub fn calls_handle(&self) -> Arc<Mutex<Vec<ConstraintNodeId>>> {
        Arc::clone(&self.calls)
    }
}

impl Default for MockOptimizedImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizedImpl for MockOptimizedImpl {
    fn check(&self, input: &OptimizedImplInput) -> OptimizedImplOutput {
        let mut calls = self.calls.lock().unwrap();
        let results = input
            .constraints
            .iter()
            .map(|(id, _)| {
                calls.push(id.clone());
                let satisfaction = self.results.get(id).copied().unwrap_or(self.default);
                ConstraintResult {
                    id: id.clone(),
                    satisfaction,
                    diagnostics: ConstraintDiagnostics::default(),
                }
            })
            .collect();
        OptimizedImplOutput { results }
    }
}

/// Mock optimized-constraint implementation that returns a fixed, possibly
/// wrong, number of results — used to exercise the contract-violation fallback
/// path in reify-eval (Task 1657).
///
/// Unlike [`MockOptimizedImpl`], which correctly returns one result per input
/// constraint, `BrokenCountOptimizedImpl` returns a caller-supplied result set
/// verbatim regardless of how many constraints are in the input. This triggers
/// the result-count mismatch that `dispatch_constraints` must detect and
/// recover from gracefully by emitting a `Diagnostic::error` and falling back
/// to the language-level `ConstraintChecker`.
///
/// The `calls` field records every `ConstraintNodeId` the impl was invoked
/// with (across all calls), so tests can assert the broken impl was actually
/// invoked before the fallback kicked in.
pub struct BrokenCountOptimizedImpl {
    fixed_results: Vec<ConstraintResult>,
    calls: Arc<Mutex<Vec<ConstraintNodeId>>>,
}

impl BrokenCountOptimizedImpl {
    /// Create an impl that always returns `fixed_results` verbatim, regardless
    /// of how many constraints are in the input.
    pub fn new(fixed_results: Vec<ConstraintResult>) -> Self {
        Self {
            fixed_results,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A clone of the shared call-tracking handle.
    ///
    /// Grab this before boxing so the test can inspect calls after the engine
    /// run:
    ///
    /// ```ignore
    /// let mock = BrokenCountOptimizedImpl::new(vec![]);
    /// let calls = mock.calls_handle();
    /// engine.register_optimized_impl("target_a", Box::new(mock));
    /// engine.check(&compiled);
    /// assert!(!calls.lock().unwrap().is_empty());
    /// ```
    pub fn calls_handle(&self) -> Arc<Mutex<Vec<ConstraintNodeId>>> {
        Arc::clone(&self.calls)
    }
}

impl OptimizedImpl for BrokenCountOptimizedImpl {
    fn check(&self, input: &OptimizedImplInput) -> OptimizedImplOutput {
        let mut calls = self.calls.lock().unwrap();
        for (id, _) in &input.constraints {
            calls.push(id.clone());
        }
        OptimizedImplOutput {
            results: self.fixed_results.clone(),
        }
    }
}

/// Mock constraint solver that returns predetermined results.
///
/// Each call to [`ConstraintSolver::solve`] is counted.  Use
/// [`call_count`][Self::call_count] for direct reads while the solver is still
/// owned, or [`counter_handle`][Self::counter_handle] to obtain a cloned
/// `Arc<AtomicUsize>` that remains valid after the solver is moved into a
/// `Box<dyn ConstraintSolver>` and handed to the engine.
pub struct MockConstraintSolver {
    result: SolveResult,
    invocation_count: Arc<AtomicUsize>,
}

impl MockConstraintSolver {
    /// Create a solver that returns Solved with the given values.
    pub fn new_solved(values: HashMap<ValueCellId, Value>) -> Self {
        Self {
            result: SolveResult::Solved {
                values,
                unique: true,
            },
            invocation_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a solver that returns Infeasible with the given diagnostics.
    pub fn new_infeasible(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            result: SolveResult::Infeasible { diagnostics },
            invocation_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a solver that returns NoProgress with the given reason.
    pub fn new_no_progress(reason: impl Into<String>) -> Self {
        Self {
            result: SolveResult::NoProgress {
                reason: reason.into(),
            },
            invocation_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Return the number of times [`ConstraintSolver::solve`] has been called.
    ///
    /// Use this accessor while the solver is still owned by the caller.  For
    /// reads after the solver has been moved into `Box::new(solver)`, see
    /// [`counter_handle`][Self::counter_handle].
    pub fn call_count(&self) -> usize {
        self.invocation_count.load(Ordering::Relaxed)
    }

    /// Return a shared handle to the invocation counter.
    ///
    /// Clones the internal `Arc<AtomicUsize>` so that callers can read the
    /// count after the solver has been moved into a `Box<dyn ConstraintSolver>`
    /// and ownership transferred to the engine.  The counter is the same
    /// `AtomicUsize` that `solve()` increments, so reads via the handle are
    /// always in sync with [`call_count`][Self::call_count].
    ///
    /// Mirrors the `captured_problems()` handle pattern on
    /// `MultiCallSpyConstraintSolver`.
    pub fn counter_handle(&self) -> Arc<AtomicUsize> {
        self.invocation_count.clone()
    }
}

impl ConstraintSolver for MockConstraintSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        self.invocation_count.fetch_add(1, Ordering::Relaxed);
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
    /// AdjacentFaces keys the handle + 0-based face index.
    AdjacentFaces {
        shape: GeometryHandleId,
        face_index: usize,
    },
    /// AncestorFacesOfEdge keys the handle + 0-based edge index.
    AncestorFacesOfEdge {
        shape: GeometryHandleId,
        edge_index: usize,
    },
    /// SharedEdges keys the handle + both 0-based face indices.
    SharedEdges {
        shape: GeometryHandleId,
        face_a: usize,
        face_b: usize,
    },
    IsWatertight(GeometryHandleId),
    IsManifold(GeometryHandleId),
    IsOrientable(GeometryHandleId),
    /// CenterOfMass keys the handle + density (hashed via f64::to_bits).
    CenterOfMass {
        handle: GeometryHandleId,
        density_bits: u64,
    },
    /// InertiaTensor keys the handle + density (hashed via f64::to_bits).
    InertiaTensor {
        handle: GeometryHandleId,
        density_bits: u64,
    },
    /// EdgeLength keys the (single) edge handle.
    EdgeLength(GeometryHandleId),
    /// EdgeTangent keys the (single) edge handle.
    EdgeTangent(GeometryHandleId),
    /// FaceNormal keys the (single) face handle.
    FaceNormal(GeometryHandleId),
    /// FaceSurfaceKind keys the (single) face handle.
    FaceSurfaceKind(GeometryHandleId),
    /// EdgeCurveKind keys the (single) edge handle.
    EdgeCurveKind(GeometryHandleId),
    /// OwnerBody keys the (single) child sub-handle (the `extract_*`
    /// product). The stored `Value` should be a `Value::Int` carrying the
    /// parent body's `GeometryHandleId.0`.
    OwnerBody(GeometryHandleId),
    /// ClosestPointOnShape keys the geometry handle + query point (f64 bits
    /// via `density_bits` so ±0.0 canonicalise and NaN debug-asserts).
    /// Powers the v0.1 stdlib `closest_point` helper (task 2324).
    ClosestPointOnShape {
        handle: GeometryHandleId,
        px_bits: u64,
        py_bits: u64,
        pz_bits: u64,
    },
    /// PointOnShape keys the geometry handle + query point + tolerance.
    /// Powers the v0.1 stdlib `is_on` helper (task 2324). Tolerance is bit-keyed
    /// so a future explicit-tolerance overload can stage distinct results
    /// without re-staging the same handle/point pair.
    PointOnShape {
        handle: GeometryHandleId,
        px_bits: u64,
        py_bits: u64,
        pz_bits: u64,
        tol_bits: u64,
    },
    /// Contains keys the solid handle + query point + tolerance.
    /// Powers the v0.1 stdlib `contains` helper (task 3611, KGQ-β).
    /// Tolerance is bit-keyed so a future explicit-tolerance overload can
    /// stage distinct results without re-staging the same handle/point pair.
    Contains {
        handle: GeometryHandleId,
        px_bits: u64,
        py_bits: u64,
        pz_bits: u64,
        tol_bits: u64,
    },
    /// GeoEquiv keys the two shape handles (canonicalised to canonical min/max
    /// order since geo_equiv is symmetric) and the tolerance as bit-keyed
    /// `u64` (via `density_bits`).
    /// Powers the v0.1 stdlib `geo_equiv(a, b, tol)` helper (task 3613, KGQ-δ).
    GeoEquiv {
        left: GeometryHandleId,
        right: GeometryHandleId,
        tol_bits: u64,
    },
    /// SurfaceAngle keys the two face handles. The angle is unsigned (the
    /// kernel returns `acos(|n_a · n_b|)`-style absolute-cos), so face_a
    /// and face_b are pair-canonicalised with `normalize_distance_pair` so
    /// `(a, b)` and `(b, a)` map to the same key. Powers the v0.1 stdlib
    /// `angle_between_surfaces` helper (task 2324).
    SurfaceAngle {
        face_a: GeometryHandleId,
        face_b: GeometryHandleId,
    },
    /// FaceNormalAt keys the face handle + query point (f64 bits via
    /// `density_bits` for ±0.0 canonicalisation + NaN debug-assert).
    /// Powers the v0.3 stdlib `normal(surface, point)` helper (task 3615, KGQ-ζ).
    FaceNormalAt {
        handle: GeometryHandleId,
        px_bits: u64,
        py_bits: u64,
        pz_bits: u64,
    },
    /// CurveCurvatureAt keys the edge handle + world query point (f64 bits).
    /// Powers the v0.3 stdlib `curvature(curve, point)` helper (task 3621, KGQ-μ).
    CurveCurvatureAt {
        handle: GeometryHandleId,
        px_bits: u64,
        py_bits: u64,
        pz_bits: u64,
    },
    /// SurfaceCurvatureAt keys the face handle + parametric (u, v) coordinates
    /// (f64 bits). Powers the v0.3 stdlib `curvature(surface, point)` helper
    /// (task 3621, KGQ-μ).
    SurfaceCurvatureAt {
        handle: GeometryHandleId,
        u_bits: u64,
        v_bits: u64,
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

/// Convert a density value to a stable `u64` bit pattern suitable for use as a
/// `HashMap` key.
///
/// **Canonicalization**: `-0.0` and `+0.0` are equal under IEEE 754 (`-0.0 == 0.0`)
/// but have different bit patterns. This function maps both to `0u64` so that
/// `with_*_result(handle, -0.0, …)` and a subsequent `query(…, density: 0.0)`
/// resolve to the same key.
///
/// **NaN contract**: `debug_assert!` fires in debug builds if `density` is NaN.
/// `NaN.to_bits()` would produce a bit pattern that never compares equal to itself,
/// causing HashMap lookups to silently miss. The assert is elided in release builds.
fn density_bits(density: f64) -> u64 {
    debug_assert!(
        !density.is_nan(),
        "density is NaN — to_bits would not roundtrip and HashMap lookup would silently miss"
    );
    if density == 0.0 {
        0u64
    } else {
        density.to_bits()
    }
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
            GeometryQuery::AdjacentFaces { shape, face_index } => QueryKey::AdjacentFaces {
                shape: *shape,
                face_index: *face_index,
            },
            GeometryQuery::AncestorFacesOfEdge { shape, edge_index } => {
                QueryKey::AncestorFacesOfEdge {
                    shape: *shape,
                    edge_index: *edge_index,
                }
            }
            GeometryQuery::SharedEdges {
                shape,
                face_a,
                face_b,
            } => QueryKey::SharedEdges {
                shape: *shape,
                face_a: *face_a,
                face_b: *face_b,
            },
            GeometryQuery::IsWatertight(id) => QueryKey::IsWatertight(*id),
            GeometryQuery::IsManifold(id) => QueryKey::IsManifold(*id),
            GeometryQuery::IsOrientable(id) => QueryKey::IsOrientable(*id),
            GeometryQuery::CenterOfMass { handle, density } => {
                let density_bits = density_bits(*density);
                QueryKey::CenterOfMass {
                    handle: *handle,
                    density_bits,
                }
            }
            GeometryQuery::InertiaTensor { handle, density } => {
                let density_bits = density_bits(*density);
                QueryKey::InertiaTensor {
                    handle: *handle,
                    density_bits,
                }
            }
            // Edge/face property queries from task 318: hashed by handle alone
            // (single-handle scalar/vector queries, no extra params to key on).
            GeometryQuery::EdgeLength(id) => QueryKey::EdgeLength(*id),
            GeometryQuery::EdgeTangent(id) => QueryKey::EdgeTangent(*id),
            GeometryQuery::FaceNormal(id) => QueryKey::FaceNormal(*id),
            // Geometry-kind classification queries from task 2658 (PRD line 78);
            // hashed by handle alone (no extra params).
            GeometryQuery::FaceSurfaceKind(id) => QueryKey::FaceSurfaceKind(*id),
            GeometryQuery::EdgeCurveKind(id) => QueryKey::EdgeCurveKind(*id),
            // Owner-body provenance from task 2658 (PRD line 81); hashed by
            // the child sub-handle alone.
            GeometryQuery::OwnerBody(id) => QueryKey::OwnerBody(*id),
            // Topology selectors from task 2324 (PRD §3.9). f64 fields hashed
            // via density_bits for ±0.0 canonicalisation + NaN debug-assert.
            GeometryQuery::ClosestPointOnShape { handle, px, py, pz } => {
                QueryKey::ClosestPointOnShape {
                    handle: *handle,
                    px_bits: density_bits(*px),
                    py_bits: density_bits(*py),
                    pz_bits: density_bits(*pz),
                }
            }
            GeometryQuery::PointOnShape {
                handle,
                px,
                py,
                pz,
                tolerance,
            } => QueryKey::PointOnShape {
                handle: *handle,
                px_bits: density_bits(*px),
                py_bits: density_bits(*py),
                pz_bits: density_bits(*pz),
                tol_bits: density_bits(*tolerance),
            },
            GeometryQuery::Contains {
                handle,
                px,
                py,
                pz,
                tolerance,
            } => QueryKey::Contains {
                handle: *handle,
                px_bits: density_bits(*px),
                py_bits: density_bits(*py),
                pz_bits: density_bits(*pz),
                tol_bits: density_bits(*tolerance),
            },
            GeometryQuery::GeoEquiv {
                left,
                right,
                tolerance,
            } => {
                let (lo, hi) = normalize_distance_pair(*left, *right);
                QueryKey::GeoEquiv {
                    left: lo,
                    right: hi,
                    tol_bits: density_bits(*tolerance),
                }
            }
            GeometryQuery::SurfaceAngle { face_a, face_b } => {
                let (lo, hi) = normalize_distance_pair(*face_a, *face_b);
                QueryKey::SurfaceAngle {
                    face_a: lo,
                    face_b: hi,
                }
            }
            GeometryQuery::FaceNormalAt { handle, px, py, pz } => {
                QueryKey::FaceNormalAt {
                    handle: *handle,
                    px_bits: density_bits(*px),
                    py_bits: density_bits(*py),
                    pz_bits: density_bits(*pz),
                }
            }
            GeometryQuery::CurveCurvatureAt { handle, px, py, pz } => {
                QueryKey::CurveCurvatureAt {
                    handle: *handle,
                    px_bits: density_bits(*px),
                    py_bits: density_bits(*py),
                    pz_bits: density_bits(*pz),
                }
            }
            GeometryQuery::SurfaceCurvatureAt { handle, u, v } => {
                QueryKey::SurfaceCurvatureAt {
                    handle: *handle,
                    u_bits: density_bits(*u),
                    v_bits: density_bits(*v),
                }
            }
        }
    }
}

/// Mock geometry kernel that tracks operations and returns dummy handles.
pub struct MockGeometryKernel {
    next_id: u64,
    operations: Arc<Mutex<Vec<GeometryOpRecord>>>,
    /// Tolerance values forwarded to every `tessellate(handle, tol)` call,
    /// in invocation order. Recorded behind an `Arc<Mutex<…>>` so
    /// integration tests can observe the recorded sequence across an
    /// engine ownership boundary (mirrors `operations` recorder pattern).
    /// Task 2874 step-11 adds this field so the per-realization tolerance-
    /// budget pipeline added by step-12 can be pinned at the kernel
    /// boundary: the recorder captures the post-budget tolerance the
    /// engine forwards to the kernel, not the demanded tolerance from the
    /// engine's tolerance-scope query — so the test catches a regression
    /// where the budget pipeline is bypassed and `effective_tessellation_tolerance`
    /// is forwarded instead.
    tessellate_tolerances: Arc<Mutex<Vec<f64>>>,
    /// Generic handle-only query results (fallback).
    queries: HashMap<GeometryHandleId, Value>,
    /// Per-query-type results (takes precedence over generic).
    typed_queries: HashMap<QueryKey, Value>,
    /// Per-parent edge-extraction results; `Ok(vec)` or `Err(e)`.
    extracted_edges: HashMap<GeometryHandleId, Result<Vec<GeometryHandleId>, QueryError>>,
    /// Per-parent face-extraction results; `Ok(vec)` or `Err(e)`.
    extracted_faces: HashMap<GeometryHandleId, Result<Vec<GeometryHandleId>, QueryError>>,
    /// Per-parent vertex-extraction results; `Ok(vec)` or `Err(e)`.
    extracted_vertices: HashMap<GeometryHandleId, Result<Vec<GeometryHandleId>, QueryError>>,
}

impl MockGeometryKernel {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            operations: Arc::new(Mutex::new(Vec::new())),
            tessellate_tolerances: Arc::new(Mutex::new(Vec::new())),
            queries: HashMap::new(),
            typed_queries: HashMap::new(),
            extracted_edges: HashMap::new(),
            extracted_faces: HashMap::new(),
            extracted_vertices: HashMap::new(),
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

    /// Configure a CenterOfMass query result for a specific handle and density.
    ///
    /// Matches `GeometryQuery::CenterOfMass { handle, density }` where `density`
    /// must be exactly the same value (bits-equal) as provided here.
    ///
    /// For uniform-density bodies the center of mass equals the geometric centroid
    /// so the expected `value` is typically a JSON-encoded `{"x":_,"y":_,"z":_}`
    /// string, identical to what the `Centroid` variant returns.
    ///
    /// # Panics (debug)
    /// Panics if `density` is NaN — NaN bits are not equal to themselves,
    /// which would silently break HashMap lookup.
    pub fn with_center_of_mass_result(
        mut self,
        handle: GeometryHandleId,
        density: f64,
        value: Value,
    ) -> Self {
        let density_bits = density_bits(density);
        self.typed_queries.insert(
            QueryKey::CenterOfMass {
                handle,
                density_bits,
            },
            value,
        );
        self
    }

    /// Configure an InertiaTensor query result for a specific handle and density.
    ///
    /// Matches `GeometryQuery::InertiaTensor { handle, density }` where `density`
    /// must be exactly the same value (bits-equal) as provided here.
    ///
    /// # Panics (debug)
    /// Panics if `density` is NaN — NaN bits are not equal to themselves,
    /// which would silently break HashMap lookup.
    pub fn with_inertia_tensor_result(
        mut self,
        handle: GeometryHandleId,
        density: f64,
        value: Value,
    ) -> Self {
        let density_bits = density_bits(density);
        self.typed_queries.insert(
            QueryKey::InertiaTensor {
                handle,
                density_bits,
            },
            value,
        );
        self
    }

    /// Configure a successful `extract_edges` result for `parent`.
    ///
    /// `kernel.extract_edges(parent)` will return `Ok(edges)`.
    pub fn with_extracted_edges(
        mut self,
        parent: GeometryHandleId,
        edges: Vec<GeometryHandleId>,
    ) -> Self {
        self.extracted_edges.insert(parent, Ok(edges));
        self
    }

    /// Configure a successful `extract_faces` result for `parent`.
    ///
    /// `kernel.extract_faces(parent)` will return `Ok(faces)`.
    pub fn with_extracted_faces(
        mut self,
        parent: GeometryHandleId,
        faces: Vec<GeometryHandleId>,
    ) -> Self {
        self.extracted_faces.insert(parent, Ok(faces));
        self
    }

    /// Configure `extract_edges(parent)` to return an error.
    ///
    /// Typically used to inject `QueryError::InvalidHandle(parent)` for
    /// error-propagation tests.
    pub fn with_extract_edges_error(mut self, parent: GeometryHandleId, err: QueryError) -> Self {
        self.extracted_edges.insert(parent, Err(err));
        self
    }

    /// Configure `extract_faces(parent)` to return an error.
    pub fn with_extract_faces_error(mut self, parent: GeometryHandleId, err: QueryError) -> Self {
        self.extracted_faces.insert(parent, Err(err));
        self
    }

    /// Configure a successful `extract_vertices` result for `parent`.
    ///
    /// `kernel.extract_vertices(parent)` will return `Ok(vertices)`.
    pub fn with_extracted_vertices(
        mut self,
        parent: GeometryHandleId,
        vertices: Vec<GeometryHandleId>,
    ) -> Self {
        self.extracted_vertices.insert(parent, Ok(vertices));
        self
    }

    /// Configure `extract_vertices(parent)` to return an error.
    pub fn with_extract_vertices_error(
        mut self,
        parent: GeometryHandleId,
        err: QueryError,
    ) -> Self {
        self.extracted_vertices.insert(parent, Err(err));
        self
    }

    /// Configure an EdgeLength query result for a specific edge handle.
    pub fn with_edge_length_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::EdgeLength(handle), value);
        self
    }

    /// Configure an EdgeTangent query result for a specific edge handle.
    ///
    /// The `value` should be a `Value::String` containing a JSON-encoded
    /// `{"x":..,"y":..,"z":..}` unit vector, matching the OCCT kernel's
    /// actual encoding consumed by `topology_selectors::edges_parallel_to`.
    pub fn with_edge_tangent_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::EdgeTangent(handle), value);
        self
    }

    /// Configure a FaceNormal query result for a specific face handle.
    ///
    /// The `value` should be a `Value::String` containing a JSON-encoded
    /// `{"x":..,"y":..,"z":..}` unit vector, matching the OCCT kernel's
    /// actual encoding consumed by `topology_selectors::faces_by_normal`.
    pub fn with_face_normal_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::FaceNormal(handle), value);
        self
    }

    /// Configure a `FaceNormalAt` query result for a specific
    /// (face handle, query point) pair.
    ///
    /// The `value` should be a `Value::String` containing a JSON-encoded
    /// `{"x":..,"y":..,"z":..}` unit normal vector, matching the OCCT
    /// kernel's wire format for `GeometryQuery::FaceNormalAt`. The point
    /// coordinates `[px, py, pz]` are routed through `density_bits` for
    /// stable hashing (±0.0 canonicalisation, NaN debug-assert).
    ///
    /// Powers the v0.3 stdlib `normal(surface, point)` helper (task 3615, KGQ-ζ).
    pub fn with_face_normal_at_result(
        mut self,
        handle: GeometryHandleId,
        point: [f64; 3],
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::FaceNormalAt {
                handle,
                px_bits: density_bits(point[0]),
                py_bits: density_bits(point[1]),
                pz_bits: density_bits(point[2]),
            },
            value,
        );
        self
    }

    /// Configure a `CurveCurvatureAt` query result for a specific
    /// (edge handle, world query point) pair.
    ///
    /// `value` should be a `Value::Real(κ)` where κ is the signed Frenet curvature
    /// in SI units (m⁻¹), matching the OCCT kernel's `curve_curvature_at` return.
    /// Point coordinates are hashed via `density_bits` for stable keying.
    ///
    /// Powers the v0.3 stdlib `curvature(curve, point)` helper (task 3621, KGQ-μ).
    pub fn with_curve_curvature_at_result(
        mut self,
        handle: GeometryHandleId,
        point: [f64; 3],
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::CurveCurvatureAt {
                handle,
                px_bits: density_bits(point[0]),
                py_bits: density_bits(point[1]),
                pz_bits: density_bits(point[2]),
            },
            value,
        );
        self
    }

    /// Configure a `SurfaceCurvatureAt` query result for a specific
    /// (face handle, parametric (u, v)) pair.
    ///
    /// `value` should be a nested `Value::List([[kappa_max, 0.0], [0.0, kappa_min]])`
    /// matching the OCCT kernel's diagonal principal-curvature wire format for
    /// `GeometryQuery::SurfaceCurvatureAt`. The (u, v) coordinates are hashed
    /// via `density_bits` for stable keying.
    ///
    /// Powers the v0.3 stdlib `curvature(surface, point)` helper (task 3621, KGQ-μ).
    pub fn with_surface_curvature_at_result(
        mut self,
        handle: GeometryHandleId,
        uv: [f64; 2],
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::SurfaceCurvatureAt {
                handle,
                u_bits: density_bits(uv[0]),
                v_bits: density_bits(uv[1]),
            },
            value,
        );
        self
    }

    /// Configure a `ClosestPointOnShape` query result for a specific
    /// (geometry handle, query point) pair.
    ///
    /// The `value` should be a `Value::String` containing a JSON-encoded
    /// `{"x":..,"y":..,"z":..}` Point3, matching the OCCT kernel's wire
    /// format and consumed by the eval-side dispatcher's
    /// `parse_xyz_value` round-trip. The point coordinates `[px, py, pz]`
    /// are routed through `density_bits` for stable hashing.
    ///
    /// Powers the v0.1 stdlib `closest_point` helper (task 2324).
    pub fn with_closest_point_on_shape_result(
        mut self,
        handle: GeometryHandleId,
        point: [f64; 3],
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::ClosestPointOnShape {
                handle,
                px_bits: density_bits(point[0]),
                py_bits: density_bits(point[1]),
                pz_bits: density_bits(point[2]),
            },
            value,
        );
        self
    }

    /// Configure a `PointOnShape` query result for a specific
    /// (geometry handle, query point, tolerance) triple.
    ///
    /// The `value` should be a `Value::Bool`. The `tolerance` is bit-keyed
    /// (via `density_bits`) so the stub for `is_on(p, b)` (which the
    /// dispatcher routes through `reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`)
    /// is distinguishable from a future explicit-tolerance `is_on(p, b, tol)` overload.
    ///
    /// Powers the v0.1 stdlib `is_on` helper (task 2324).
    pub fn with_point_on_shape_result(
        mut self,
        handle: GeometryHandleId,
        point: [f64; 3],
        tolerance: f64,
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::PointOnShape {
                handle,
                px_bits: density_bits(point[0]),
                py_bits: density_bits(point[1]),
                pz_bits: density_bits(point[2]),
                tol_bits: density_bits(tolerance),
            },
            value,
        );
        self
    }

    /// Configure a `contains` classifier query result for a specific solid/point/tolerance triple.
    ///
    /// The `value` should be a `Value::Bool(true)` (point inside or on the boundary)
    /// or `Value::Bool(false)` (point outside). The `tolerance` is bit-keyed
    /// (via `density_bits`) so the stub for `contains(solid, point)` (which the
    /// dispatcher routes through `reify_ir::DEFAULT_CONTAINS_TOLERANCE_M`)
    /// is distinguishable from a future explicit-tolerance `contains(solid, point, tol)` overload.
    ///
    /// Powers the v0.1 stdlib `contains` helper (task 3611, KGQ-β).
    pub fn with_contains_result(
        mut self,
        handle: GeometryHandleId,
        point: [f64; 3],
        tolerance: f64,
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::Contains {
                handle,
                px_bits: density_bits(point[0]),
                py_bits: density_bits(point[1]),
                pz_bits: density_bits(point[2]),
                tol_bits: density_bits(tolerance),
            },
            value,
        );
        self
    }

    /// Configure a `GeoEquiv` query result for a specific shape pair and tolerance.
    ///
    /// The `value` should be a `Value::Bool(true|false)`.
    /// The shape pair `(left, right)` is canonicalised via
    /// `normalize_distance_pair` so `(a, b)` and `(b, a)` map to the same
    /// key (geo_equiv is symmetric by convention).
    /// The tolerance is bit-keyed via `density_bits` so distinct tolerances
    /// produce distinct keys (consistent with `with_contains_result`).
    ///
    /// Powers the v0.1 stdlib `geo_equiv(a, b, tol)` helper (task 3613, KGQ-δ).
    pub fn with_geo_equiv_result(
        mut self,
        left: GeometryHandleId,
        right: GeometryHandleId,
        tolerance: f64,
        value: Value,
    ) -> Self {
        let (lo, hi) = normalize_distance_pair(left, right);
        self.typed_queries.insert(
            QueryKey::GeoEquiv {
                left: lo,
                right: hi,
                tol_bits: density_bits(tolerance),
            },
            value,
        );
        self
    }

    /// Configure a `SurfaceAngle` query result for a specific face pair.
    ///
    /// The `value` should be a `Value::Real(rad)` where `rad ∈ [0, π]`.
    /// The face pair `(face_a, face_b)` is canonicalised so
    /// `(a, b)` and `(b, a)` map to the same key, matching the kernel's
    /// orientation-agnostic absolute-cos convention.
    ///
    /// Powers the v0.1 stdlib `angle_between_surfaces` helper (task 2324).
    pub fn with_surface_angle_result(
        mut self,
        face_a: GeometryHandleId,
        face_b: GeometryHandleId,
        value: Value,
    ) -> Self {
        let (lo, hi) = normalize_distance_pair(face_a, face_b);
        self.typed_queries.insert(
            QueryKey::SurfaceAngle {
                face_a: lo,
                face_b: hi,
            },
            value,
        );
        self
    }

    /// Configure a `FaceSurfaceKind` query result for a specific face handle.
    ///
    /// The `value` should be a `Value::String` whose payload is one of the
    /// canonical surface-kind names (`"Plane"`, `"Cylinder"`, `"Cone"`,
    /// `"Sphere"`, `"Torus"`, `"BezierSurface"`, `"BSplineSurface"`,
    /// `"OffsetSurface"`, `"Other"`) as documented on
    /// [`GeometryQuery::FaceSurfaceKind`]. Decoded by the
    /// `selector_vocabulary_v2::faces_by_surface_kind` selector via
    /// [`reify_types::FaceSurfaceKind`]'s `TryFrom<&str>` impl.
    pub fn with_face_surface_kind_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::FaceSurfaceKind(handle), value);
        self
    }

    /// Configure an `EdgeCurveKind` query result for a specific edge handle.
    ///
    /// The `value` should be a `Value::String` whose payload is one of the
    /// canonical curve-kind names (`"Line"`, `"Circle"`, `"Ellipse"`,
    /// `"Hyperbola"`, `"Parabola"`, `"BezierCurve"`, `"BSplineCurve"`,
    /// `"OffsetCurve"`, `"Other"`) as documented on
    /// [`GeometryQuery::EdgeCurveKind`]. Decoded by the
    /// `selector_vocabulary_v2::edges_by_curve_kind` selector via
    /// [`reify_types::EdgeCurveKind`]'s `TryFrom<&str>` impl.
    pub fn with_edge_curve_kind_result(mut self, handle: GeometryHandleId, value: Value) -> Self {
        self.typed_queries
            .insert(QueryKey::EdgeCurveKind(handle), value);
        self
    }

    /// Configure an `AdjacentFaces` query result for a specific (parent
    /// shape, 0-based face index) pair.
    ///
    /// The `value` should be a `Value::List(Vec<Value::Int>)` of global
    /// face indices into the same canonical TopExp_Explorer order returned
    /// by `extract_faces(parent)`. Decoded by the
    /// `selector_vocabulary_v2::adjacent_to_face` selector, which maps
    /// each integer index back to a `GeometryHandleId` via the canonical
    /// extract_faces list.
    pub fn with_adjacent_faces_result(
        mut self,
        parent: GeometryHandleId,
        face_index: usize,
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::AdjacentFaces {
                shape: parent,
                face_index,
            },
            value,
        );
        self
    }

    /// Configure an `AncestorFacesOfEdge` query result for a specific (parent
    /// shape, 0-based edge index) pair.
    ///
    /// The `value` should be a `Value::List(Vec<Value::Int>)` of global
    /// face indices into the same canonical TopExp_Explorer order returned
    /// by `extract_faces(parent)`. Decoded by the
    /// `selector_vocabulary_v2::ancestor_faces_of_edge` selector, which
    /// maps each integer index back to a `GeometryHandleId` via the
    /// canonical extract_faces list.
    pub fn with_ancestor_faces_result(
        mut self,
        parent: GeometryHandleId,
        edge_index: usize,
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::AncestorFacesOfEdge {
                shape: parent,
                edge_index,
            },
            value,
        );
        self
    }

    /// Configure a `SharedEdges` query result for a specific (parent shape,
    /// face_a index, face_b index) triple.
    ///
    /// The `value` should be a `Value::List(Vec<Value::Int>)` of global edge
    /// indices into the same canonical TopExp_Explorer order returned by
    /// `extract_edges(parent)`. Decoded by the `shared_edges` topology-selector
    /// dispatch arm (eval-side, task 3560), which maps each integer index back
    /// to a `GeometryHandleId` via the canonical extract_edges list.
    ///
    /// Mirrors [`Self::with_adjacent_faces_result`] — needed for `shared_edges`
    /// dispatch tests because the `QueryKey::SharedEdges` variant keys on three
    /// fields (shape + face_a + face_b) and constructing it via the typed_queries
    /// map directly would expose internal API.
    pub fn with_shared_edges_result(
        mut self,
        parent: GeometryHandleId,
        face_a: usize,
        face_b: usize,
        value: Value,
    ) -> Self {
        self.typed_queries.insert(
            QueryKey::SharedEdges {
                shape: parent,
                face_a,
                face_b,
            },
            value,
        );
        self
    }

    /// Configure an `OwnerBody` query result for a specific child sub-handle,
    /// using the canonical encoding `Value::Int(parent.0 as i64)`.
    ///
    /// Pre-stages the answer that the OCCT kernel produces by recording a
    /// `parent_handle: HashMap<…>` entry inside `extract_edges` /
    /// `extract_faces`. Decoded by the
    /// `selector_vocabulary_v2::owner_body_of` selector via a `Value::Int`
    /// → `GeometryHandleId` round-trip.
    ///
    /// For tests that need to stage a non-`Value::Int` payload (e.g. the
    /// defence-in-depth "expected Value::Int, got …" branch of the
    /// selector), use [`Self::with_owner_body_value`] instead.
    pub fn with_owner_body_result(
        mut self,
        child: GeometryHandleId,
        parent: GeometryHandleId,
    ) -> Self {
        self.typed_queries
            .insert(QueryKey::OwnerBody(child), Value::Int(parent.0 as i64));
        self
    }

    /// Lower-level variant of [`Self::with_owner_body_result`] — stage a raw
    /// [`Value`] for an `OwnerBody` query keyed by the child handle.
    ///
    /// Mainly useful for defence-in-depth tests asserting the selector
    /// returns `QueryError::QueryFailed` on a non-`Value::Int` payload
    /// (e.g. a `Value::String` from a hypothetical future kernel).
    pub fn with_owner_body_value(mut self, child: GeometryHandleId, value: Value) -> Self {
        self.typed_queries.insert(QueryKey::OwnerBody(child), value);
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

    /// Get a shared reference to recorded tessellate-tolerance values, in
    /// invocation order. Each entry is the `tolerance` argument passed to a
    /// `tessellate(handle, tolerance)` call; the recorder grows by one
    /// element per `tessellate` invocation.
    ///
    /// Used by task 2874 step-11/12 to pin that the engine's per-stage
    /// tolerance-budget pipeline (`compute_realization_tolerance_budget`
    /// → `kernel.tessellate(_, budget)`) actually routes the demanded
    /// tolerance through to the kernel, rather than forwarding the
    /// module-level `effective_tessellation_tolerance` default.
    pub fn tessellate_tolerances_ref(&self) -> Arc<Mutex<Vec<f64>>> {
        self.tessellate_tolerances.clone()
    }

    /// Snapshot of recorded tessellate-tolerance values (clone of the
    /// underlying `Vec<f64>`). Equivalent to
    /// `tessellate_tolerances_ref().lock().unwrap().clone()`; provided as a
    /// convenience symmetric to `operations()`.
    pub fn tessellate_tolerances(&self) -> Vec<f64> {
        self.tessellate_tolerances.lock().unwrap().clone()
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

        let repr = match op {
            GeometryOp::LineSegment { .. }
            | GeometryOp::Arc { .. }
            | GeometryOp::Helix { .. }
            | GeometryOp::InterpCurve { .. }
            | GeometryOp::BezierCurve { .. }
            | GeometryOp::NurbsCurve { .. } => Some(BRepKind::Wire),
            _ => Some(BRepKind::Solid),
        };

        Ok(GeometryHandle { id, repr })
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        // Check per-query-type map first
        let key = QueryKey::from_query(query);
        if let Some(value) = self.typed_queries.get(&key) {
            return Ok(value.clone());
        }

        // OwnerBody is special: an unstaged child handle has no recorded
        // parent, and we mirror the OcctKernel error shape so the v2
        // selector vocabulary can match on a stable diagnostic regardless
        // of which kernel is in play (mock or OCCT). Without this short
        // circuit, the generic fallback below would return a "no mock
        // result for …" message that the test contract does not match.
        if let GeometryQuery::OwnerBody(id) = query {
            return Err(QueryError::QueryFailed(format!(
                "owner_body: handle {id:?} has no recorded parent (was extract_edges / extract_faces called?)"
            )));
        }

        // Fall back to generic handle-only map
        let handle_id = match query {
            GeometryQuery::Volume(id) => id,
            GeometryQuery::SurfaceArea(id) => id,
            GeometryQuery::Centroid(id) => id,
            GeometryQuery::BoundingBox(id) => id,
            GeometryQuery::Distance { from, .. } => from,
            GeometryQuery::MomentOfInertia { handle, .. } => handle,
            GeometryQuery::AdjacentFaces { shape, .. } => shape,
            GeometryQuery::AncestorFacesOfEdge { shape, .. } => shape,
            GeometryQuery::SharedEdges { shape, .. } => shape,
            GeometryQuery::IsWatertight(id) => id,
            GeometryQuery::IsManifold(id) => id,
            GeometryQuery::IsOrientable(id) => id,
            GeometryQuery::CenterOfMass { handle, .. } => handle,
            GeometryQuery::InertiaTensor { handle, .. } => handle,
            GeometryQuery::EdgeLength(id) => id,
            GeometryQuery::EdgeTangent(id) => id,
            GeometryQuery::FaceNormal(id) => id,
            GeometryQuery::FaceSurfaceKind(id) => id,
            GeometryQuery::EdgeCurveKind(id) => id,
            // OwnerBody is handled above the generic fallback because its
            // miss path produces a domain-specific error message. The
            // exhaustiveness guard retains this arm so a future kernel
            // change is forced to revisit the dispatch table.
            GeometryQuery::OwnerBody(id) => id,
            // Topology selectors (task 2324) — generic fallback returns the
            // canonical first handle, parallel to the Distance arm.
            GeometryQuery::ClosestPointOnShape { handle, .. } => handle,
            GeometryQuery::PointOnShape { handle, .. } => handle,
            GeometryQuery::Contains { handle, .. } => handle,
            GeometryQuery::GeoEquiv { left, .. } => left,
            GeometryQuery::SurfaceAngle { face_a, .. } => face_a,
            GeometryQuery::FaceNormalAt { handle, .. } => handle,
            GeometryQuery::CurveCurvatureAt { handle, .. } => handle,
            GeometryQuery::SurfaceCurvatureAt { handle, .. } => handle,
        };

        self.queries
            .get(handle_id)
            .cloned()
            .ok_or_else(|| QueryError::QueryFailed(format!("no mock result for {:?}", handle_id)))
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        match self.extracted_edges.get(&handle) {
            Some(result) => result.clone(),
            None => Err(QueryError::QueryFailed(format!(
                "no topology extraction fixture for {:?}",
                handle
            ))),
        }
    }

    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        match self.extracted_faces.get(&handle) {
            Some(result) => result.clone(),
            None => Err(QueryError::QueryFailed(format!(
                "no topology extraction fixture for {:?}",
                handle
            ))),
        }
    }

    fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        match self.extracted_vertices.get(&handle) {
            Some(result) => result.clone(),
            None => Err(QueryError::QueryFailed(format!(
                "no topology extraction fixture for {:?}",
                handle
            ))),
        }
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

    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        if handles.is_empty() {
            return Err(GeometryError::OperationFailed(
                "make_compound: input handle list must not be empty".into(),
            ));
        }
        let id = GeometryHandleId(self.next_id);
        self.next_id += 1;
        Ok(GeometryHandle {
            id,
            repr: Some(BRepKind::Compound),
        })
    }

    fn tessellate(&self, _handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        // Task 2874 step-11: record the tolerance forwarded to each
        // `tessellate` call so integration tests can verify the engine's
        // per-stage tolerance-budget pipeline (step-12) routes the demanded
        // tolerance through to the kernel rather than the module-level
        // `effective_tessellation_tolerance` default.
        self.tessellate_tolerances.lock().unwrap().push(tolerance);
        // Return a minimal valid mesh (one triangle)
        Ok(Mesh {
            vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            normals: Some(vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
        })
    }
}

/// A mock geometry kernel whose `execute` always returns `Err`.
///
/// Useful for testing how consumers handle geometry operation failures.
/// Because `execute` always fails, no valid `GeometryHandle` is ever
/// produced. As a defensive fail-loud guard, `tessellate`, `export`,
/// and `query` all return errors immediately — any call to them
/// indicates a regression where the engine failed to short-circuit on
/// the execute error.
pub struct FailingMockGeometryKernel;

impl GeometryKernel for FailingMockGeometryKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(
            "simulated kernel failure".into(),
        ))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(
            "should not reach: execute always fails".into(),
        ))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(
            "should not reach: execute always fails".into(),
        ))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(
            "should not reach: execute always fails".into(),
        ))
    }
}

/// Per-variant call counters for [`CountingMockKernel`].
///
/// Holds an `AtomicUsize` for each tracked `GeometryQuery` variant plus a
/// grand `total` counter that increments on every `query()` call. Counters
/// are exposed as plain `usize` via read-only accessor methods so callers
/// never need to import `Ordering`.
///
/// Held behind `Arc<QueryCounts>` so the test can clone the Arc *before*
/// moving the kernel into a `Box<dyn GeometryKernel>` and inspect the counts
/// after the move.
#[derive(Default)]
pub struct QueryCounts {
    total: AtomicUsize,
    is_watertight: AtomicUsize,
    is_manifold: AtomicUsize,
    is_orientable: AtomicUsize,
}

impl QueryCounts {
    /// Total number of `query()` calls across all variants.
    pub fn total(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }

    /// Number of `GeometryQuery::IsWatertight` calls.
    pub fn is_watertight(&self) -> usize {
        self.is_watertight.load(Ordering::SeqCst)
    }

    /// Number of `GeometryQuery::IsManifold` calls.
    pub fn is_manifold(&self) -> usize {
        self.is_manifold.load(Ordering::SeqCst)
    }

    /// Number of `GeometryQuery::IsOrientable` calls.
    pub fn is_orientable(&self) -> usize {
        self.is_orientable.load(Ordering::SeqCst)
    }
}

/// A [`MockGeometryKernel`] wrapper that counts every `query()` round-trip.
///
/// ## What is counted
///
/// Only `query()` calls are intercepted. Per the list below:
///
/// * **Counted** — `query()` (grand `total` + per-variant counter for
///   `IsWatertight`, `IsManifold`, `IsOrientable`).
/// * **Counted via default** — `query_many()` (the trait default forwards
///   per-element to `query()`, so each element is counted; see section below).
/// * **Forwarded uncounted** — `execute`, `export`, `tessellate`,
///   `extract_edges`, `extract_faces`, and `extract_vertices` are delegated
///   to the inner kernel without touching any counter.
///
/// ## Arc-sharing contract
///
/// The counters live in an `Arc<QueryCounts>`. Call [`CountingMockKernel::counts`]
/// to clone the Arc *before* moving the kernel into `Box<dyn GeometryKernel>`.
/// After the move the test can still read the counters from the saved Arc —
/// this is the pattern used by integration tests that pass the kernel to
/// `Engine::new`.
///
/// ## `query_many` is NOT overridden
///
/// The trait default for `query_many` forwards per-element to `query()`.
/// Overriding it to delegate to `self.inner.query_many` would bypass our
/// counting intercept. By leaving the default in place each element routes
/// through our override and is counted.
pub struct CountingMockKernel {
    inner: MockGeometryKernel,
    counts: Arc<QueryCounts>,
}

impl CountingMockKernel {
    /// Wrap `inner` in a counting kernel with fresh zero counters.
    pub fn new(inner: MockGeometryKernel) -> Self {
        Self {
            inner,
            counts: Arc::new(QueryCounts::default()),
        }
    }

    /// Clone the shared counter view.
    ///
    /// The returned `Arc` remains valid after `self` is moved into a
    /// `Box<dyn GeometryKernel>`, making it safe to capture before passing
    /// the kernel to `Engine::new`.
    pub fn counts(&self) -> Arc<QueryCounts> {
        Arc::clone(&self.counts)
    }

    /// Convenience accessor — equivalent to `self.counts().total()`.
    ///
    /// Preferred by unit tests that only need the grand total and don't
    /// need to hold an Arc across a kernel move.
    pub fn total_query_count(&self) -> usize {
        self.counts.total()
    }
}

impl GeometryKernel for CountingMockKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.counts.total.fetch_add(1, Ordering::SeqCst);
        match query {
            GeometryQuery::IsWatertight(_) => {
                self.counts.is_watertight.fetch_add(1, Ordering::SeqCst);
            }
            GeometryQuery::IsManifold(_) => {
                self.counts.is_manifold.fetch_add(1, Ordering::SeqCst);
            }
            GeometryQuery::IsOrientable(_) => {
                self.counts.is_orientable.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
        self.inner.query(query)
    }

    fn extract_edges(&mut self, h: GeometryHandleId) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_edges(h)
    }

    fn extract_faces(&mut self, h: GeometryHandleId) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_faces(h)
    }

    fn extract_vertices(&mut self, h: GeometryHandleId) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_vertices(h)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
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

    /// Create a spy that will return `Solved` with `unique: false` and capture
    /// the `ResolutionProblem` it receives.
    pub fn new_solved_non_unique(values: HashMap<ValueCellId, Value>) -> Self {
        Self {
            captured: Arc::new(Mutex::new(None)),
            result: SolveResult::Solved {
                values,
                unique: false,
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
    use reify_core::Type;
    use reify_ir::{CompiledExpr, Value, ValueMap};
    use std::borrow::Cow;
    use std::sync::Barrier;
    use std::sync::atomic::Ordering;

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
    fn single_auto_param_has_standard_defaults() {
        let cell_id = ValueCellId::new("X", "y");
        let param = single_auto_param(cell_id.clone());
        assert_eq!(param.id, cell_id);
        assert_eq!(param.param_type, Type::length());
        assert_eq!(param.bounds, None);
        assert!(!param.free);
    }

    #[test]
    fn mock_constraint_checker_predetermined() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let checker =
            MockConstraintChecker::new().with_result(cnid.clone(), Satisfaction::Violated);

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid.clone(), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results[0].satisfaction, Satisfaction::Violated);
    }

    /// Pins the per-call FIFO response queue contract used by v0.2 DFS
    /// backtracking tests (task 2659). Each `.check(...)` call pops one
    /// `Satisfaction` from the queue head and applies it to every constraint
    /// in that call's input. Once the queue is exhausted, subsequent calls
    /// fall back to the existing per-id map / default behavior.
    ///
    /// Without this contract DFS leaf-iteration tests can't express
    /// "leaf 1 violated, leaf 2 satisfied" because substitution mechanics
    /// (`Type::TypeParam(T)` → `Type::StructureRef(candidate)`) are deferred
    /// per the PRD's "implement v0.2 search with full re-check at each
    /// binding" decision — every leaf would otherwise see an identical
    /// `ConstraintInput` and receive the same verdict.
    #[test]
    fn mock_constraint_checker_call_queue_pops_per_call_then_falls_back_to_default() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let checker = MockConstraintChecker::new()
            .with_default(Satisfaction::Satisfied)
            .with_call_queue(vec![Satisfaction::Violated, Satisfaction::Indeterminate]);

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let make_input = || ConstraintInput {
            constraints: Cow::Owned(vec![(cnid.clone(), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        // Call 1: queue head is `Violated`.
        let r1 = checker.check(&make_input());
        assert_eq!(r1[0].satisfaction, Satisfaction::Violated);

        // Call 2: queue head is `Indeterminate`.
        let r2 = checker.check(&make_input());
        assert_eq!(r2[0].satisfaction, Satisfaction::Indeterminate);

        // Call 3: queue exhausted → fall back to `default = Satisfied`.
        let r3 = checker.check(&make_input());
        assert_eq!(r3[0].satisfaction, Satisfaction::Satisfied);
    }

    /// Pins the call-tracking observability contract for `MockConstraintChecker`.
    ///
    /// Four sub-contracts verified here:
    /// (a) `calls()` is empty before any `check()` call.
    /// (b) After a `check()` with two constraints, `calls()` contains both ids
    ///     in input order.
    /// (c) `calls_handle()` returns the same `Arc<Mutex<Vec>>` that is updated
    ///     by subsequent `check()` calls — i.e. grabbing a handle before boxing
    ///     the mock still allows inspection after the move.
    /// (d) BOTH the call-queue branch (first pop from FIFO queue) AND the
    ///     per-id/default fallback branch push ids — because the two backjumping
    ///     tests use a one-element queue and leaves 2..N flow through the
    ///     fallback.
    #[test]
    fn mock_constraint_checker_records_calls_handle_per_constraint_id() {
        let id_a = ConstraintNodeId::new("S", 0);
        let id_b = ConstraintNodeId::new("S", 1);
        let checker = MockConstraintChecker::new();

        // (a) No calls yet.
        assert!(checker.calls().is_empty(), "no calls yet");

        // (b) Multi-constraint input: two distinct ids.
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(id_a.clone(), &expr), (id_b.clone(), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let _ = checker.check(&input);
        let calls = checker.calls();
        assert_eq!(calls.len(), 2, "two constraints → two call records");
        assert_eq!(calls[0], id_a, "first id recorded first");
        assert_eq!(calls[1], id_b, "second id recorded second");

        // (c) calls_handle() returns the same Arc that updates on subsequent checks.
        let handle = checker.calls_handle();
        let _ = checker.check(&input);
        assert_eq!(
            handle.lock().unwrap().len(),
            4,
            "second check adds 2 more ids; handle must reflect live updates"
        );

        // (d) Queue branch also records ids.
        // Fresh checker with a one-element queue: first check hits queue
        // (Violated applied to both ids), second check falls through to
        // default (Satisfied). Both checks must record their ids.
        let fresh = MockConstraintChecker::new().with_call_queue(vec![Satisfaction::Violated]);
        let _ = fresh.check(&input); // queue branch
        let _ = fresh.check(&input); // fallback branch
        let fresh_calls = fresh.calls();
        assert_eq!(
            fresh_calls.len(),
            4,
            "queue branch (2 ids) + fallback branch (2 ids) = 4 total records"
        );
        assert_eq!(fresh_calls[0], id_a, "queue branch: first id");
        assert_eq!(fresh_calls[1], id_b, "queue branch: second id");
        assert_eq!(fresh_calls[2], id_a, "fallback branch: first id");
        assert_eq!(fresh_calls[3], id_b, "fallback branch: second id");
    }

    // step-7 (Task 273 — @optimized plumbing): failing tests for MockOptimizedImpl.
    //
    // MockOptimizedImpl mirrors MockConstraintChecker but also records every
    // constraint id it was invoked with, so tests can assert that dispatch
    // actually routed a constraint through the optimized path instead of the
    // language-level checker.
    #[test]
    fn mock_optimized_impl_returns_default_satisfaction() {
        let cnid = ConstraintNodeId::new("S", 0);
        let imp = MockOptimizedImpl::new().with_default(Satisfaction::Violated);

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = reify_ir::OptimizedImplInput {
            constraints: vec![(cnid.clone(), &expr)],
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let output = imp.check(&input);
        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].id, cnid);
        assert_eq!(output.results[0].satisfaction, Satisfaction::Violated);
    }

    #[test]
    fn mock_optimized_impl_returns_per_id_result() {
        let a = ConstraintNodeId::new("S", 0);
        let b = ConstraintNodeId::new("S", 1);
        let imp = MockOptimizedImpl::new()
            .with_default(Satisfaction::Satisfied)
            .with_result(a.clone(), Satisfaction::Violated);

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = reify_ir::OptimizedImplInput {
            constraints: vec![(a.clone(), &expr), (b.clone(), &expr)],
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let output = imp.check(&input);
        assert_eq!(output.results.len(), 2);
        assert_eq!(output.results[0].id, a);
        assert_eq!(output.results[0].satisfaction, Satisfaction::Violated);
        assert_eq!(output.results[1].id, b);
        assert_eq!(output.results[1].satisfaction, Satisfaction::Satisfied);
    }

    #[test]
    fn mock_optimized_impl_records_calls() {
        let a = ConstraintNodeId::new("S", 0);
        let b = ConstraintNodeId::new("S", 1);
        let imp = MockOptimizedImpl::new();

        assert!(imp.calls().is_empty(), "no calls yet");

        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let input = reify_ir::OptimizedImplInput {
            constraints: vec![(a.clone(), &expr), (b.clone(), &expr)],
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let _ = imp.check(&input);
        let calls = imp.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], a);
        assert_eq!(calls[1], b);
    }

    #[test]
    fn mock_optimized_impl_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockOptimizedImpl>();

        let _boxed: Box<dyn reify_ir::OptimizedImpl> = Box::new(MockOptimizedImpl::new());
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

    /// Verify that `MockConstraintSolver` counts invocations correctly via both
    /// `call_count()` and the `counter_handle()` Arc.  Also confirms that adding
    /// `Arc<AtomicUsize>` to the struct does not break `Send + Sync`.
    #[test]
    fn mock_constraint_solver_counts_invocations() {
        // Compile-time Send+Sync check: if the type is unsound this won't compile.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockConstraintSolver>();

        let mut values = HashMap::new();
        values.insert(ValueCellId::new("S", "thickness"), Value::length(0.005));

        let solver = MockConstraintSolver::new_solved(values);
        let problem = empty_problem();

        // Before any calls: both counter_handle and call_count must report 0.
        let counter = solver.counter_handle();
        assert_eq!(
            solver.call_count(),
            0,
            "call_count should be 0 before any solve()"
        );
        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "handle should be 0 before any solve()"
        );

        // Drive three invocations.
        solver.solve(&problem);
        solver.solve(&problem);
        solver.solve(&problem);

        // Both accessors must agree and reflect all three calls.
        assert_eq!(
            solver.call_count(),
            3,
            "call_count should be 3 after three solve() calls"
        );
        assert_eq!(
            counter.load(Ordering::Relaxed),
            3,
            "handle must stay in sync — counter_handle is a live view of the same AtomicUsize"
        );
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
    fn mock_execute_linear_pattern_2d_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.01),
                height: Value::length(0.01),
                depth: Value::length(0.01),
            })
            .unwrap();

        let handle = kernel
            .execute(&GeometryOp::LinearPattern2D {
                target: target.id,
                direction1: [1.0, 0.0, 0.0],
                count1: 3,
                spacing1: Value::length(0.02),
                direction2: [0.0, 1.0, 0.0],
                count2: 4,
                spacing2: Value::length(0.03),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::LinearPattern2D {
                target,
                direction1,
                count1,
                spacing1,
                direction2,
                count2,
                spacing2,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(*direction1, [1.0, 0.0, 0.0]);
                assert_eq!(*count1, 3);
                assert_eq!(*spacing1, Value::length(0.02));
                assert_eq!(*direction2, [0.0, 1.0, 0.0]);
                assert_eq!(*count2, 4);
                assert_eq!(*spacing2, Value::length(0.03));
            }
            other => panic!("expected LinearPattern2D, got {:?}", other),
        }
    }

    #[test]
    fn mock_execute_arbitrary_pattern_records_op() {
        let mut kernel = MockGeometryKernel::new();
        let target = kernel
            .execute(&GeometryOp::Box {
                width: Value::length(0.01),
                height: Value::length(0.01),
                depth: Value::length(0.01),
            })
            .unwrap();

        let transforms = vec![[0.02, 0.0, 0.0], [0.0, 0.02, 0.0], [0.02, 0.02, 0.0]];
        let handle = kernel
            .execute(&GeometryOp::ArbitraryPattern {
                target: target.id,
                transforms: transforms.clone(),
            })
            .unwrap();

        assert_eq!(handle.id, GeometryHandleId(2));
        match &kernel.operations()[1].op {
            GeometryOp::ArbitraryPattern {
                target,
                transforms: recorded_transforms,
            } => {
                assert_eq!(*target, GeometryHandleId(1));
                assert_eq!(recorded_transforms.len(), 3);
                assert_eq!(recorded_transforms[0], [0.02, 0.0, 0.0]);
                assert_eq!(recorded_transforms[1], [0.0, 0.02, 0.0]);
                assert_eq!(recorded_transforms[2], [0.02, 0.02, 0.0]);
            }
            other => panic!("expected ArbitraryPattern, got {:?}", other),
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
        use reify_ir::ValueMap;

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
            auto_params: vec![single_auto_param(ValueCellId::new("A", "x"))],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };
        let result1 = spy.solve(&problem1);
        assert!(
            matches!(&result1, SolveResult::Solved { values, unique: true } if values.contains_key(&ValueCellId::new("A", "x"))),
            "first call should return values_a"
        );

        // Second call
        let problem2 = ResolutionProblem {
            auto_params: vec![single_auto_param(ValueCellId::new("B", "y"))],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
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

    // step-1: failing tests for FailingMockGeometryKernel (struct not yet defined)
    #[test]
    fn failing_kernel_execute_returns_err() {
        let mut kernel = FailingMockGeometryKernel;
        let op = GeometryOp::Box {
            width: Value::length(0.08),
            height: Value::length(0.1),
            depth: Value::length(0.005),
        };
        let result = kernel.execute(&op);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, GeometryError::OperationFailed(ref msg) if msg.contains("simulated kernel failure")),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn failing_kernel_query_returns_err_defensively() {
        let kernel = FailingMockGeometryKernel;
        let id = GeometryHandleId(1);
        let result = kernel.query(&GeometryQuery::Volume(id));
        assert!(result.is_err(), "expected Err but got Ok");
        let err = result.unwrap_err();
        assert!(
            matches!(err, QueryError::QueryFailed(ref msg) if msg.contains("should not reach")),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn failing_kernel_export_returns_err_defensively() {
        let kernel = FailingMockGeometryKernel;
        let id = GeometryHandleId(1);
        let mut buf: Vec<u8> = Vec::new();
        let result = kernel.export(id, ExportFormat::Step, &mut buf);
        assert!(result.is_err(), "expected Err but got Ok");
        let err = result.unwrap_err();
        assert!(
            matches!(err, ExportError::FormatError(ref msg) if msg.contains("should not reach")),
            "unexpected error: {:?}",
            err
        );
        assert!(buf.is_empty(), "buffer should not have been written to");
    }

    #[test]
    fn failing_kernel_tessellate_returns_err_defensively() {
        let kernel = FailingMockGeometryKernel;
        let id = GeometryHandleId(1);
        let result = kernel.tessellate(id, 0.01);
        assert!(result.is_err(), "expected Err but got Ok");
        let err = result.unwrap_err();
        assert!(
            matches!(err, TessError::TessellationFailed(ref msg) if msg.contains("should not reach")),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn mock_with_center_of_mass_result_returns_for_center_of_mass_query() {
        let id = GeometryHandleId(1);
        let expected = Value::String("{\"x\":5,\"y\":0,\"z\":0}".to_string());
        let kernel =
            MockGeometryKernel::new().with_center_of_mass_result(id, 1000.0, expected.clone());
        let result = kernel
            .query(&GeometryQuery::CenterOfMass {
                handle: id,
                density: 1000.0,
            })
            .unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn mock_with_inertia_tensor_result_returns_for_inertia_tensor_query() {
        let id = GeometryHandleId(1);
        // Build a diagonal 3×3 tensor as a Value::List of lists.
        let expected = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        let kernel =
            MockGeometryKernel::new().with_inertia_tensor_result(id, 7850.0, expected.clone());
        let result = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: id,
                density: 7850.0,
            })
            .unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn mock_with_center_of_mass_result_canonicalizes_signed_zero_density() {
        let id = GeometryHandleId(1);
        let expected = Value::String("{\"x\":5,\"y\":0,\"z\":0}".to_string());

        // Insert with -0.0, query with +0.0 — must resolve to the same key.
        let kernel =
            MockGeometryKernel::new().with_center_of_mass_result(id, -0.0_f64, expected.clone());
        let result = kernel
            .query(&GeometryQuery::CenterOfMass {
                handle: id,
                density: 0.0_f64,
            })
            .unwrap();
        assert_eq!(
            result, expected,
            "insert -0.0 / query +0.0 should hit the same key"
        );

        // Insert with +0.0, query with -0.0 — symmetric case.
        let kernel =
            MockGeometryKernel::new().with_center_of_mass_result(id, 0.0_f64, expected.clone());
        let result = kernel
            .query(&GeometryQuery::CenterOfMass {
                handle: id,
                density: -0.0_f64,
            })
            .unwrap();
        assert_eq!(
            result, expected,
            "insert +0.0 / query -0.0 should hit the same key"
        );
    }

    #[test]
    fn mock_with_inertia_tensor_result_canonicalizes_signed_zero_density() {
        let id = GeometryHandleId(1);
        let expected = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);

        // Insert with -0.0, query with +0.0 — must resolve to the same key.
        let kernel =
            MockGeometryKernel::new().with_inertia_tensor_result(id, -0.0_f64, expected.clone());
        let result = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: id,
                density: 0.0_f64,
            })
            .unwrap();
        assert_eq!(
            result, expected,
            "insert -0.0 / query +0.0 should hit the same key"
        );

        // Insert with +0.0, query with -0.0 — symmetric case.
        let kernel =
            MockGeometryKernel::new().with_inertia_tensor_result(id, 0.0_f64, expected.clone());
        let result = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: id,
                density: -0.0_f64,
            })
            .unwrap();
        assert_eq!(
            result, expected,
            "insert +0.0 / query -0.0 should hit the same key"
        );
    }

    #[test]
    fn density_bits_canonicalizes_signed_zero_and_passes_through_finite_values() {
        // +0.0 → 0u64
        assert_eq!(super::density_bits(0.0_f64), 0u64);
        // -0.0 → 0u64 (canonicalization invariant: -0.0 and +0.0 must produce the same key)
        assert_eq!(super::density_bits(-0.0_f64), 0u64);
        // Finite positive: bits must round-trip exactly
        assert_eq!(super::density_bits(1.0_f64), 1.0_f64.to_bits());
        // Realistic density value
        assert_eq!(super::density_bits(7850.0_f64), 7850.0_f64.to_bits());
        // Non-zero non-special: infinity is a valid f64 bit pattern (not NaN)
        assert_eq!(super::density_bits(f64::INFINITY), f64::INFINITY.to_bits());
    }

    #[test]
    fn mock_with_extracted_edges_returns_configured_handles() {
        let parent = GeometryHandleId(1);
        let e1 = GeometryHandleId(2);
        let e2 = GeometryHandleId(3);
        let mut kernel = MockGeometryKernel::new().with_extracted_edges(parent, vec![e1, e2]);
        let result = kernel.extract_edges(parent).expect("should return Ok");
        assert_eq!(result, vec![e1, e2]);
    }

    #[test]
    fn mock_with_extracted_faces_returns_configured_handles() {
        let parent = GeometryHandleId(1);
        let f1 = GeometryHandleId(2);
        let f2 = GeometryHandleId(3);
        let mut kernel = MockGeometryKernel::new().with_extracted_faces(parent, vec![f1, f2]);
        let result = kernel.extract_faces(parent).expect("should return Ok");
        assert_eq!(result, vec![f1, f2]);
    }

    #[test]
    fn mock_with_extract_edges_error_returns_invalid_handle() {
        let parent = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new()
            .with_extract_edges_error(parent, QueryError::InvalidHandle(parent));
        let result = kernel.extract_edges(parent);
        assert!(
            matches!(result, Err(QueryError::InvalidHandle(h)) if h == parent),
            "expected Err(InvalidHandle({:?})), got {:?}",
            parent,
            result
        );
    }

    #[test]
    fn mock_with_extract_faces_error_returns_invalid_handle() {
        let parent = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new()
            .with_extract_faces_error(parent, QueryError::InvalidHandle(parent));
        let result = kernel.extract_faces(parent);
        assert!(
            matches!(result, Err(QueryError::InvalidHandle(h)) if h == parent),
            "expected Err(InvalidHandle({:?})), got {:?}",
            parent,
            result
        );
    }

    #[test]
    fn mock_extract_edges_unconfigured_returns_default_query_failed() {
        let parent = GeometryHandleId(99);
        let mut kernel = MockGeometryKernel::new();
        let result = kernel.extract_edges(parent);
        match result {
            Err(QueryError::QueryFailed(msg)) => {
                // Message should identify the handle so a misspelled handle id
                // in a test fixture is distinguishable from a genuinely-unsupported
                // kernel (which uses "topology extraction not supported by this kernel").
                assert!(
                    msg.contains("topology extraction fixture"),
                    "unexpected message: {msg:?}"
                );
                assert!(
                    msg.contains("99"),
                    "message should include the handle id (99), got: {msg:?}"
                );
            }
            other => panic!("expected Err(QueryFailed), got {:?}", other),
        }
    }

    #[test]
    fn mock_extract_faces_unconfigured_returns_default_query_failed() {
        let parent = GeometryHandleId(99);
        let mut kernel = MockGeometryKernel::new();
        let result = kernel.extract_faces(parent);
        match result {
            Err(QueryError::QueryFailed(msg)) => {
                assert!(
                    msg.contains("topology extraction fixture"),
                    "unexpected message: {msg:?}"
                );
                assert!(
                    msg.contains("99"),
                    "message should include the handle id (99), got: {msg:?}"
                );
            }
            other => panic!("expected Err(QueryFailed), got {:?}", other),
        }
    }

    #[test]
    fn mock_with_edge_length_result_returns_for_edge_length_query() {
        let handle = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new().with_edge_length_result(handle, Value::Real(1.5));
        let result = kernel.query(&GeometryQuery::EdgeLength(handle)).unwrap();
        assert_eq!(result, Value::Real(1.5));
    }

    #[test]
    fn mock_with_edge_tangent_result_returns_for_edge_tangent_query() {
        let handle = GeometryHandleId(1);
        let tangent = Value::String("{\"x\":1,\"y\":0,\"z\":0}".into());
        let kernel = MockGeometryKernel::new().with_edge_tangent_result(handle, tangent.clone());
        let result = kernel.query(&GeometryQuery::EdgeTangent(handle)).unwrap();
        assert_eq!(result, tangent);
    }

    #[test]
    fn mock_with_face_normal_result_returns_for_face_normal_query() {
        let handle = GeometryHandleId(1);
        let normal = Value::String("{\"x\":0,\"y\":0,\"z\":1}".into());
        let kernel = MockGeometryKernel::new().with_face_normal_result(handle, normal.clone());
        let result = kernel.query(&GeometryQuery::FaceNormal(handle)).unwrap();
        assert_eq!(result, normal);
    }

    #[test]
    fn mock_with_closest_point_on_shape_result_returns_for_query() {
        let handle = GeometryHandleId(1);
        let payload = Value::String("{\"x\":5,\"y\":0,\"z\":0}".into());
        let kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            handle,
            [10.0, 0.0, 0.0],
            payload.clone(),
        );
        let result = kernel
            .query(&GeometryQuery::ClosestPointOnShape {
                handle,
                px: 10.0,
                py: 0.0,
                pz: 0.0,
            })
            .unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn mock_with_point_on_shape_result_returns_for_query() {
        let handle = GeometryHandleId(1);
        let kernel = MockGeometryKernel::new().with_point_on_shape_result(
            handle,
            [5.0, 0.0, 0.0],
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            Value::Bool(true),
        );
        let result = kernel
            .query(&GeometryQuery::PointOnShape {
                handle,
                px: 5.0,
                py: 0.0,
                pz: 0.0,
                tolerance: reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            })
            .unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn mock_with_surface_angle_result_returns_for_query_with_canonical_face_pair() {
        let face_a = GeometryHandleId(7);
        let face_b = GeometryHandleId(11);
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            Value::Real(std::f64::consts::FRAC_PI_2),
        );
        // Forward order
        let result = kernel
            .query(&GeometryQuery::SurfaceAngle { face_a, face_b })
            .unwrap();
        assert_eq!(result, Value::Real(std::f64::consts::FRAC_PI_2));
        // Reverse order — must hit the same key thanks to pair canonicalisation.
        let result_rev = kernel
            .query(&GeometryQuery::SurfaceAngle {
                face_a: face_b,
                face_b: face_a,
            })
            .unwrap();
        assert_eq!(
            result_rev,
            Value::Real(std::f64::consts::FRAC_PI_2),
            "SurfaceAngle keys must be face-pair-symmetric"
        );
    }

    // ── CountingMockKernel tests ──────────────────────────────────────────────

    #[test]
    fn counting_mock_kernel_total_increments_per_query() {
        let handle = GeometryHandleId(1);
        let inner = MockGeometryKernel::new().with_query_result(handle, Value::Bool(true));
        let kernel = CountingMockKernel::new(inner);

        kernel.query(&GeometryQuery::IsWatertight(handle)).unwrap();
        kernel.query(&GeometryQuery::IsWatertight(handle)).unwrap();
        kernel.query(&GeometryQuery::IsWatertight(handle)).unwrap();

        assert_eq!(kernel.total_query_count(), 3);
        assert_eq!(kernel.counts().total(), 3);
    }

    #[test]
    fn counting_mock_kernel_per_variant_counters_track_only_their_variant() {
        let handle = GeometryHandleId(2);
        let inner = MockGeometryKernel::new().with_query_result(handle, Value::Bool(true));
        let kernel = CountingMockKernel::new(inner);

        kernel.query(&GeometryQuery::IsWatertight(handle)).unwrap();
        kernel.query(&GeometryQuery::IsManifold(handle)).unwrap();
        kernel.query(&GeometryQuery::IsOrientable(handle)).unwrap();
        kernel.query(&GeometryQuery::Volume(handle)).unwrap();

        let counts = kernel.counts();
        assert_eq!(counts.is_watertight(), 1);
        assert_eq!(counts.is_manifold(), 1);
        assert_eq!(counts.is_orientable(), 1);
        assert_eq!(
            counts.total(),
            4,
            "Volume contributes to total but not to any per-variant counter"
        );
    }

    #[test]
    fn counting_mock_kernel_query_proxies_inner_result() {
        let handle = GeometryHandleId(3);
        let inner = MockGeometryKernel::new().with_query_result(handle, Value::Bool(true));
        let kernel = CountingMockKernel::new(inner);

        let result = kernel.query(&GeometryQuery::IsWatertight(handle)).unwrap();
        assert_eq!(
            result,
            Value::Bool(true),
            "CountingMockKernel must not change the inner kernel's result"
        );
    }

    #[test]
    fn counting_mock_kernel_counts_arc_survives_kernel_move_into_box() {
        let handle = GeometryHandleId(4);
        let inner = MockGeometryKernel::new().with_query_result(handle, Value::Bool(true));
        let kernel = CountingMockKernel::new(inner);
        let counts = kernel.counts();

        // Move the kernel into a Box<dyn GeometryKernel>, simulating the
        // integration-test use case (Engine::new consumes the kernel).
        let boxed: Box<dyn GeometryKernel> = Box::new(kernel);
        boxed.query(&GeometryQuery::IsWatertight(handle)).unwrap();

        // The Arc<QueryCounts> captured before the move should still see the increment.
        assert_eq!(counts.is_watertight(), 1);
    }

    #[test]
    fn counting_mock_kernel_query_many_routes_through_query_intercept() {
        // Pins the doc-comment invariant: the trait default for `query_many`
        // forwards per-element to `query()`, so each element passes through
        // our counting intercept. This test will fail if either
        // `CountingMockKernel` or the inner `MockGeometryKernel` ever gains
        // an explicit `query_many` override that bypasses `query()`.
        let handle = GeometryHandleId(5);
        let inner = MockGeometryKernel::new().with_query_result(handle, Value::Bool(true));
        let kernel = CountingMockKernel::new(inner);

        kernel
            .query_many(&[
                GeometryQuery::IsWatertight(handle),
                GeometryQuery::IsManifold(handle),
            ])
            .unwrap();

        let counts = kernel.counts();
        assert_eq!(
            counts.is_watertight(),
            1,
            "IsWatertight element must be counted"
        );
        assert_eq!(
            counts.is_manifold(),
            1,
            "IsManifold element must be counted"
        );
        assert_eq!(counts.total(), 2, "both elements contribute to grand total");
    }

    #[test]
    fn counting_mock_kernel_extract_vertices_forwards_to_inner_uncounted() {
        // Pins the "Forwarded uncounted" contract for extract_vertices:
        // (a) the call returns the trait-default QueryFailed error (since
        //     MockGeometryKernel has no extract_vertices override), and
        // (b) no per-variant counter increments — grand total stays at zero.
        //
        // Note: this test passes both before and after the override is added
        // because both paths resolve to the same trait-default error today.
        // Its value is as a regression guard for when a sibling task adds a
        // distinguishable MockGeometryKernel::extract_vertices impl.
        let handle = GeometryHandleId(1);
        let inner = MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let result = kernel.extract_vertices(handle);

        // Pin the error variant (Err(QueryFailed)) without locking to the
        // upstream wording owned by the trait default in reify-types.  The
        // exact message may change when MockGeometryKernel gains its own
        // extract_vertices override; the forwarding contract is what matters.
        match result {
            Err(QueryError::QueryFailed(_)) => {}
            other => panic!("expected Err(QueryFailed(…)), got {other:?}"),
        }

        let counts = kernel.counts();
        assert_eq!(counts.total(), 0, "extract_vertices must not increment total");
        // Belt-and-suspenders: these per-variant counters are only incremented
        // by query()/query_many(), so they cannot be touched by extract_vertices
        // today.  The assertions guard against any future refactor that
        // accidentally routes extraction through the counting path.
        assert_eq!(
            counts.is_watertight(),
            0,
            "extract_vertices must not increment is_watertight"
        );
        assert_eq!(
            counts.is_manifold(),
            0,
            "extract_vertices must not increment is_manifold"
        );
        assert_eq!(
            counts.is_orientable(),
            0,
            "extract_vertices must not increment is_orientable"
        );
    }
}
