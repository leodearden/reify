//! Integration tests for `Engine::execute_realization_ops` kernel-attribute-hook
//! wiring (task 2875).
//!
//! Uses a synthetic `CompiledModule` (two Box primitives + one Union boolean op)
//! and a `HookRecordingKernel` (a wrapper around `MockGeometryKernel` that
//! advertises a `KernelAttributeHook` recording every `propagate_attributes`
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

use std::sync::{Arc, Mutex};

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_test_support::*;
use reify_types::{
    AttributeHistory, CompiledExpr, ExportFormat, FeatureId, GeometryError, GeometryHandle,
    GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook,
    KernelAttributeOutcome, Mesh, ModulePath, QueryError, RealizationNodeId, Severity,
    TessError, TopologyAttributeTable, Type, Value,
};

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

// ─── HookRecordingKernel ─────────────────────────────────────────────────────

/// Wraps `MockGeometryKernel`, overrides `attribute_hook()` to return a
/// `RecordingHookStub`, and overrides `execute_with_history` to return
/// `(handle, AttributeHistory::None)` so the engine reaches the
/// post-`populate_attribute_history` dispatcher slot.
struct HookRecordingKernel {
    inner: MockGeometryKernel,
    hook: RecordingHookStub,
}

impl HookRecordingKernel {
    fn new(calls: Arc<Mutex<Vec<RecordedCall>>>) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            hook: RecordingHookStub { calls },
        }
    }
}

impl GeometryKernel for HookRecordingKernel {
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
    ) -> Result<(), reify_types::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        Some(&self.hook)
    }
}

// ─── FailingHookKernel ────────────────────────────────────────────────────────

/// Like `HookRecordingKernel` but uses `FailingHookStub` — every
/// `propagate_attributes` call returns `Err(QueryError::QueryFailed(...))`.
struct FailingHookKernel {
    inner: MockGeometryKernel,
    hook: FailingHookStub,
}

impl FailingHookKernel {
    fn new() -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            hook: FailingHookStub,
        }
    }
}

impl GeometryKernel for FailingHookKernel {
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
    ) -> Result<(), reify_types::ExportError> {
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
    let kernel = HookRecordingKernel::new(Arc::clone(&calls));
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
fn engine_build_kernel_attribute_hook_query_error_surfaces_diagnostic_warning_without_failing_realization(
) {
    let module = wiring_test_module();
    let kernel = FailingHookKernel::new();
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
