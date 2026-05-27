//! Integration tests for `Engine::execute_realization_ops` kernel-attribute-hook
//! wiring (task 2875).
//!
//! Uses a synthetic `CompiledModule` (two Box primitives + one Union boolean op)
//! and a `HookKernel<RecordingHookStub>` (a wrapper around `MockGeometryKernel`
//! that advertises a `KernelAttributeHook` recording every `propagate_attributes`
//! call) to assert:
//!
//! - The engine dispatches through the hook exactly **once** per
//!   parent-having op (the Union), passing the correct
//!   `(op, parents, result, feature_id)` tuple.
//! - Primitive ops (the two Boxes) are **never** dispatched — the
//!   `parent_handles_for_op` empty-slice guard fires before the hook is
//!   reached.
//! - A `QueryError` returned by the hook surfaces as a
//!   `Diagnostic::warning` *without* regressing `geometry_output` to
//!   `None` (auxiliary-metadata invariant per task-2574).
//! - BRep-first ordering: `populate_attribute_history` (OCCT-native BRep
//!   population, which calls `kernel.extract_faces`) runs **before** the
//!   kernel attribute hook's `propagate_attributes`, as required by the
//!   design decision in the task plan.

use std::sync::{Arc, Mutex};

use reify_compiler::{BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, PrimitiveKind, SweepKind};
use reify_test_support::*;
use reify_core::{ModulePath, RealizationNodeId, Severity, Type};
use reify_ir::{AttributeHistory, CompiledExpr, ExportFormat, FeatureId, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, KernelAttributeOutcome, Mesh, QueryError, SweepOpHistoryRecords, TessError, TopologyAttributeTable, Value};

// ─── Shared call-recording type ──────────────────────────────────────────────

#[derive(Debug, Clone)]
struct RecordedCall {
    op: GeometryOp,
    parents: Vec<GeometryHandleId>,
    result: GeometryHandleId,
    feature_id: FeatureId,
}

// ─── RecordingHookStub ───────────────────────────────────────────────────────

/// `KernelAttributeHook` impl that records every `propagate_attributes` call
/// into a shared `Arc<Mutex<Vec<RecordedCall>>>` and returns
/// `Ok(Propagated)`.  Used by `HookRecordingKernel` to assert engine-level
/// wiring without depending on `ManifoldKernel`.
struct RecordingHookStub {
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl KernelAttributeHook for RecordingHookStub {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        op: &GeometryOp,
        parent_handles: &[GeometryHandleId],
        result_handle: GeometryHandleId,
        splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        self.calls.lock().unwrap().push(RecordedCall {
            op: op.clone(),
            parents: parent_handles.to_vec(),
            result: result_handle,
            feature_id: splitting_feature_id.clone(),
        });
        Ok(KernelAttributeOutcome::Propagated)
    }
}

// ─── FailingHookStub ─────────────────────────────────────────────────────────

/// `KernelAttributeHook` impl that always returns a synthetic `QueryError`.
/// Used by `FailingHookKernel` to assert that hook errors surface as
/// `Diagnostic::warning` without regressing the realization to Failed.
struct FailingHookStub;

impl KernelAttributeHook for FailingHookStub {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        _op: &GeometryOp,
        _parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        Err(QueryError::QueryFailed("synthetic hook failure".into()))
    }
}

// ─── HookKernel<H> — generic hook wrapper ────────────────────────────────────

/// Generic wrapper around [`MockGeometryKernel`] that:
/// - Overrides `attribute_hook()` to return `Some(&self.hook)`.
/// - Overrides `execute_with_history` to return `(handle, AttributeHistory::None)`
///   so the engine reaches the post-`populate_attribute_history` dispatcher slot.
/// - Forwards all other `GeometryKernel` methods to the inner `MockGeometryKernel`.
///
/// Parametrized over the hook type `H: KernelAttributeHook`, so a single
/// struct covers both the recording (ok) and failing (error) test scenarios
/// — eliminating the near-identical `HookRecordingKernel` and `FailingHookKernel`
/// boilerplate.  `OrderingKernel` is kept separate because it requires
/// distinct `extract_faces`/`extract_edges` overrides.
struct HookKernel<H: KernelAttributeHook> {
    inner: MockGeometryKernel,
    hook: H,
}

impl<H: KernelAttributeHook> HookKernel<H> {
    fn new(hook: H) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            hook,
        }
    }
}

impl<H: KernelAttributeHook> GeometryKernel for HookKernel<H> {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let handle = self.inner.execute(op)?;
        Ok((handle, AttributeHistory::None))
    }

    fn query(&self, q: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(q)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        Some(&self.hook)
    }
}

// ─── Fixture helpers ──────────────────────────────────────────────────────────

fn mm_literal(v: f64) -> CompiledExpr {
    CompiledExpr::literal(mm(v), Type::length())
}

fn box_args() -> Vec<(String, CompiledExpr)> {
    vec![
        ("width".into(), mm_literal(10.0)),
        ("height".into(), mm_literal(10.0)),
        ("depth".into(), mm_literal(10.0)),
    ]
}

/// Synthesised `CompiledModule` with three ops in a single realization:
/// `[Box, Box, Union(Step(0), Step(1))]`.
///
/// When driven through `Engine::build`, the two Box ops produce handles
/// allocated by `MockGeometryKernel.execute` in sequence (handle 1, then
/// handle 2), and the Union produces handle 3.  The hook should be called
/// exactly once (for the Union), with `parents = [1, 2]` and `result = 3`.
fn wiring_test_module() -> reify_compiler::CompiledModule {
    let box0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: box_args(),
    };
    let box1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: box_args(),
    };
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    let template = TopologyTemplateBuilder::new("WiringTest")
        .realization("WiringTest", 0, vec![box0, box1, union_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("wiring_test"))
        .template(template)
        .build()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Engine-level wiring contract: the dispatcher is invoked exactly once per
/// parent-having op (the Union); the two Box primitives must be skipped via
/// the `parent_handles_for_op.is_empty()` short-circuit.
///
/// Asserts:
/// 1. Exactly one hook call recorded.
/// 2. The op is `GeometryOp::Union { .. }`.
/// 3. Parents are `[GeometryHandleId(1), GeometryHandleId(2)]` in
///    left-then-right order.
/// 4. Result handle is `GeometryHandleId(3)` (third sequential allocation).
/// 5. `feature_id == FeatureId::from(&RealizationNodeId::new("WiringTest", 0))`.
///
/// **Red step**: fails until the dispatcher call site is wired into
/// `Engine::execute_realization_ops` (step-4).
#[test]
fn engine_build_invokes_kernel_attribute_hook_for_parent_having_ops_and_skips_primitives() {
    let module = wiring_test_module();
    let calls: Arc<Mutex<Vec<RecordedCall>>> = Arc::new(Mutex::new(Vec::new()));
    let kernel = HookKernel::new(RecordingHookStub {
        calls: Arc::clone(&calls),
    });
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let _ = engine.build(&module, ExportFormat::Step);

    let recorded = calls.lock().unwrap().clone();

    // (1) Exactly one hook call — the Union boolean op.
    assert_eq!(
        recorded.len(),
        1,
        "expected exactly 1 hook call (for the Union op); primitives must be skipped; \
         got {} calls: {:?}",
        recorded.len(),
        recorded
            .iter()
            .map(|c| format!("{:?}", c.op))
            .collect::<Vec<_>>(),
    );

    let call = &recorded[0];

    // (2) The op dispatched to the hook is the Union.
    assert!(
        matches!(call.op, GeometryOp::Union { .. }),
        "hook must be invoked with the Union op; got {:?}",
        call.op
    );

    // (3) Parents are [left_box_handle, right_box_handle] in left-then-right order.
    // MockGeometryKernel allocates sequentially: box0 → 1, box1 → 2, union → 3.
    assert_eq!(
        call.parents,
        vec![GeometryHandleId(1), GeometryHandleId(2)],
        "hook must receive [left, right] parent handles in declaration order; \
         got {:?}",
        call.parents
    );

    // (4) Result handle is the third allocation.
    assert_eq!(
        call.result,
        GeometryHandleId(3),
        "hook must receive the Union's result handle (3rd MockGeometryKernel \
         allocation); got {:?}",
        call.result
    );

    // (5) FeatureId is derived from the realization NodeId.
    let expected_feature_id = FeatureId::from(&RealizationNodeId::new("WiringTest", 0));
    assert_eq!(
        call.feature_id, expected_feature_id,
        "hook must receive feature_id == FeatureId::from(RealizationNodeId(\"WiringTest\", 0)); \
         got {:?}",
        call.feature_id
    );
}

/// Auxiliary-metadata invariant (task-2574): a `QueryError` from the hook's
/// `propagate_attributes` must surface as a `Diagnostic::warning` without
/// regressing the realization's `geometry_output` to `None`.
///
/// Uses the same `[Box, Box, Union]` module but with `FailingHookKernel`
/// (every `propagate_attributes` call returns
/// `Err(QueryError::QueryFailed("synthetic hook failure"))`).
///
/// **Red step**: fails until the `Err(e) →  Diagnostic::warning(...)` arm
/// is wired in `Engine::execute_realization_ops` (step-6).
#[test]
fn engine_build_kernel_attribute_hook_query_error_surfaces_diagnostic_warning_without_failing_realization()
 {
    let module = wiring_test_module();
    let kernel = HookKernel::new(FailingHookStub);
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&module, ExportFormat::Step);

    // (1) At least one warning containing the hook error info.
    let hook_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(d.severity, Severity::Warning)
                && d.message.contains("kernel attribute hook")
                && d.message.contains("synthetic hook failure")
        })
        .collect();
    assert!(
        !hook_warnings.is_empty(),
        "expected at least one Diagnostic::warning containing \"kernel attribute hook\" \
         and \"synthetic hook failure\"; got diagnostics: {:?}",
        result.diagnostics
    );

    // (2) No error-level diagnostics — the hook failure must not regress the
    //     realization to Failed.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "hook QueryError must not produce Error-severity diagnostics (auxiliary-metadata \
         MUST NOT regress realization to Failed per task-2574); got errors: {:?}",
        errors
    );

    // (3) geometry_output is present — the realization did NOT regress to Failed.
    assert!(
        result.geometry_output.is_some(),
        "geometry_output must be Some after a hook QueryError — the realization \
         succeeded at the kernel level; got None"
    );
}

// ─── BRep-first ordering test fixtures ────────────────────────────────────────

/// `KernelAttributeHook` impl that records a "propagate_attributes" event in
/// the shared event log when `propagate_attributes` is called.  Used by
/// `OrderingKernel` to assert that `populate_attribute_history`'s
/// `extract_faces` call is recorded **before** this hook fires.
struct OrderingHookStub {
    event_log: Arc<Mutex<Vec<&'static str>>>,
}

impl KernelAttributeHook for OrderingHookStub {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        _op: &GeometryOp,
        _parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        self.event_log.lock().unwrap().push("propagate_attributes");
        Ok(KernelAttributeOutcome::Propagated)
    }
}

/// Mock kernel for the BRep-first ordering test:
///
/// 1. Returns `AttributeHistory::Extrude(SweepOpHistoryRecords::default())`
///    for `GeometryOp::Extrude` ops — this triggers `populate_attribute_history`
///    to call `kernel.extract_faces` (the BRep population path).
/// 2. Records a "extract_faces" event in the shared log on each `extract_faces`
///    call (and returns `Ok(vec![])` so the BRep path proceeds without error).
/// 3. Returns `Ok(vec![])` from `extract_edges` (needed by
///    `populate_single_parent_sweep_op` which calls both `extract_faces` and
///    `extract_edges`).
/// 4. Advertises `OrderingHookStub` as its hook — so the hook fires for each
///    parent-having op after `populate_attribute_history` completes.
struct OrderingKernel {
    inner: MockGeometryKernel,
    event_log: Arc<Mutex<Vec<&'static str>>>,
    hook: OrderingHookStub,
}

impl OrderingKernel {
    fn new(event_log: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            hook: OrderingHookStub {
                event_log: Arc::clone(&event_log),
            },
            event_log,
        }
    }
}

impl GeometryKernel for OrderingKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let handle = self.inner.execute(op)?;
        // Return non-None history for Extrude ops so that populate_attribute_history
        // calls extract_faces (the BRep population path we're ordering against).
        let history = match op {
            GeometryOp::Extrude { .. } => {
                AttributeHistory::Extrude(SweepOpHistoryRecords::default())
            }
            _ => AttributeHistory::None,
        };
        Ok((handle, history))
    }

    fn extract_faces(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        // Record that the BRep population path has entered the kernel.
        self.event_log.lock().unwrap().push("extract_faces");
        Ok(vec![])
    }

    fn extract_edges(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        // populate_single_parent_sweep_op calls both extract_faces and extract_edges;
        // return empty vec so the call succeeds without noise in the event log.
        Ok(vec![])
    }

    fn query(&self, q: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(q)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        Some(&self.hook)
    }
}

/// Synthesised `CompiledModule` with two ops in a single realization:
/// `[LineSegment(..),  Extrude(Step(0), distance=10mm)]`.
///
/// The LineSegment is the profile stand-in (step 0 → handle 1); the Extrude
/// uses it as its profile (step 1 → handle 2).  With `OrderingKernel`:
///
/// - `execute_with_history(LineSegment)` → `(handle 1, AttributeHistory::None)`.
/// - `execute_with_history(Extrude)` → `(handle 2, AttributeHistory::Extrude(...))`.
///
/// This ensures `populate_attribute_history` enters `populate_single_parent_sweep_op`
/// and calls `extract_faces` × 2 (profile + result handle) before the hook fires.
fn ordering_test_module() -> reify_compiler::CompiledModule {
    let line_op = CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(10.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(0.0)),
        ],
    };
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![("distance".into(), mm_literal(10.0))],
    };
    let template = TopologyTemplateBuilder::new("OrderingTest")
        .realization("OrderingTest", 0, vec![line_op, extrude_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("ordering_test"))
        .template(template)
        .build()
}

/// BRep-first ordering contract: `populate_attribute_history` (which calls
/// `kernel.extract_faces`) must execute **before** the kernel attribute hook's
/// `propagate_attributes`, per the design decision in the task plan.
///
/// Uses a `[LineSegment, Extrude]` module so that the Extrude op:
/// (a) returns `AttributeHistory::Extrude(...)` from `execute_with_history`,
///     triggering `populate_attribute_history` → `populate_single_parent_sweep_op`
///     → `kernel.extract_faces` (the BRep population path), and
/// (b) has a non-empty `parent_handles_for_op` slice (profile handle only),
///     so the kernel attribute hook is also invoked.
///
/// A regression that swapped the call order — invoking the hook before
/// `populate_attribute_history`, or skipping the BRep path entirely for
/// parent-having ops — would cause "propagate_attributes" to precede
/// "extract_faces" in the event log, failing the position assertion.
#[test]
fn engine_build_kernel_attribute_hook_respects_brep_first_ordering() {
    let module = ordering_test_module();
    let event_log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let kernel = OrderingKernel::new(Arc::clone(&event_log));
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let _ = engine.build(&module, ExportFormat::Step);

    let log = event_log.lock().unwrap().clone();

    // Both paths must have fired — if either is absent the test setup is wrong.
    let extract_pos = log.iter().position(|&e| e == "extract_faces").expect(
        "BRep population must call kernel.extract_faces for Extrude ops; \
             got event log: {log:?}",
    );
    let hook_pos = log
        .iter()
        .position(|&e| e == "propagate_attributes")
        .expect(
            "kernel attribute hook must fire for the parent-having Extrude op; \
             got event log: {log:?}",
        );

    // The core ordering assertion: BRep population (extract_faces) must precede
    // the hook (propagate_attributes).
    assert!(
        extract_pos < hook_pos,
        "BRep-first ordering violated: extract_faces must precede propagate_attributes \
         in the event log; extract_pos={extract_pos}, hook_pos={hook_pos}, log={log:?}",
    );
}
