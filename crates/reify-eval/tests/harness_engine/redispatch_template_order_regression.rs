// SPDX-License-Identifier: AGPL-3.0-or-later

//! Regression tests for task #5951 — a premature per-template
//! `Engine::redispatch_geometry_consuming_compute_nodes` pass permanently
//! strands a geometry-consuming `@optimized` compute node.
//!
//! ## The defect these tests pin
//!
//! `redispatch_geometry_consuming_compute_nodes` is called once per template,
//! from inside the `for (t_idx, template) in module.templates.iter()` loop of
//! `Engine::build_with_geometry_output` in `engine_build.rs` — the shared
//! realization worker that `build()` and `realize_for_check()` both delegate
//! to, and whose call is mirrored in `build_snapshot`'s own template loop.
//! Each call scans **all** compute nodes in the graph — not just the current
//! template's.
//!
//! (Both of those were literal line pins until a rebase moved them 29 lines;
//! an earlier commit on this branch recorded them as verified-correct, which
//! no longer holds. They are symbol-anchored here so a rebase cannot re-stale
//! them — the same remedy that commit applied to the Phase-1 scan anchor
//! cited below.)
//!
//! Its Phase-1 candidate gate is `realization_inputs.is_empty()`: a node whose
//! `realization_inputs` are already non-empty is skipped. That gate is a
//! **one-shot latch**.
//!
//! When a template precedes the geometry-consuming one, the redispatch fires
//! during the *preceding* template's iteration, before the consumer's body has
//! realized. At that point `values` already holds a SYMBOLIC
//! `Value::GeometryHandle { kernel_handle: None, .. }` (minted by
//! `mint_symbolic_geometry_handles_into_values`), the Phase-2 gate matched
//! `Value::GeometryHandle { .. }` regardless of `kernel_handle`, and
//! `build_compute_realization_inputs` recorded a content-free handle. The latch
//! tripped, and the LATER — correct, post-hydration — pass for the consumer's
//! own template was skipped forever. The node kept the degraded first-dispatch
//! result, with **no diagnostic**: the `ReprKind::BRep` arm of
//! `project_realization_read_handle` is identity-only by design (PRD §4 D1) and
//! emits nothing.
//!
//! ## Why this file is mock-kernel, not OCCT/gmsh
//!
//! The defect is decided *before* any realization executes, on a handle whose
//! `kernel_handle` is `None`. It is therefore kernel-independent, and the whole
//! contract is observable through `MockGeometryKernel` in milliseconds. The
//! real OCCT+gmsh acceptance path lives in
//! `tests/solve_elastic_static_body_e2e.rs`.
//!
//! ## Test shape
//!
//! The two PAIRS below compile the same module — one `@optimized` probe
//! consuming a `let body = box(..)` — and register a recording `ComputeFn` for
//! it. Each pair varies exactly one thing and asserts the same property in both
//! halves, so nothing but the varied factor can explain a difference.
//!
//! 1. **Ordering pair** — varies whether a no-op `structure` is declared AHEAD
//!    of the consumer. Both halves must see at least one probe invocation
//!    carrying a KERNEL-BACKED (`kernel_handle: Some(_)`) geometry arg, i.e.
//!    the post-hydration redispatch actually ran.
//! 2. **Content pair** — varies whether the kernel can tessellate. The write
//!    that trips the Phase-1 latch must only ever happen when the rebuilt
//!    `RealizationReadHandle`s carry real content: with a non-tessellating
//!    kernel the node's `realization_inputs` must stay EMPTY (still a
//!    candidate); with a tessellating one they must be recorded as before
//!    (so the guard cannot be satisfied by suppressing the redispatch
//!    wholesale).
//! 3. **Mixed-arg case** (not a pair — a single asymmetric fixture) — a
//!    two-geometry-arg probe where one body realizes and one is refused. Both
//!    the ordering and content contracts above are satisfied by fix (B)'s
//!    content guard ALONE, so neither pair distinguishes fix (A). This case
//!    does: the write legitimately happens, and the contract is about its
//!    MEMBERSHIP — the symbolic sibling's `realization_ref` must not be in it.

use std::cell::RefCell;

use reify_ir::{ExportFormat, GeometryKernel, Value};

// ── recording probe ────────────────────────────────────────────────────────

/// One recorded `ComputeFn` invocation.
#[derive(Debug, Clone, Copy)]
struct ProbeRecord {
    /// Did any `value_inputs` entry arrive as a KERNEL-BACKED geometry handle
    /// (`Value::GeometryHandle { kernel_handle: Some(_), .. }`)?
    ///
    /// `false` covers both `Value::Undef` (the original in-`eval()` dispatch,
    /// before any geometry let has a realized handle) and the SYMBOLIC
    /// `kernel_handle: None` placeholder that the premature redispatch pass
    /// consumed.
    kernel_backed_arg: bool,
    /// `realization_inputs.len()` for this invocation.
    realization_inputs_len: usize,
    /// Did any `realization_inputs` handle carry real projected content
    /// (`RealizationReadHandle::content()` is `Some`)?
    any_realized_content: bool,
}

// Per-thread capture slot. Each cargo test runs on its own thread and
// `Engine::build` is synchronous, so two tests in this binary cannot interleave
// records even when run in parallel. Each test clears the slot at entry.
thread_local! {
    static PROBE_RECORDS: RefCell<Vec<ProbeRecord>> = const { RefCell::new(Vec::new()) };
}

/// Recording [`reify_eval::ComputeFn`] for `@optimized("test::redispatch-order-probe")`.
///
/// Purity-preserving: it only reads the handed slices and appends one
/// [`ProbeRecord`] per invocation.
fn order_probe_fn(
    value_inputs: &[Value],
    realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    let record = ProbeRecord {
        kernel_backed_arg: value_inputs.iter().any(|v| {
            matches!(
                v,
                Value::GeometryHandle {
                    kernel_handle: Some(_),
                    ..
                }
            )
        }),
        realization_inputs_len: realization_inputs.len(),
        any_realized_content: realization_inputs.iter().any(|h| h.content().is_some()),
    };
    PROBE_RECORDS.with(|slot| slot.borrow_mut().push(record));
    reify_eval::ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// Reset the capture slot; call at the top of every test.
fn reset_records() {
    PROBE_RECORDS.with(|slot| slot.borrow_mut().clear());
}

/// Drain the recorded invocations.
fn recorded() -> Vec<ProbeRecord> {
    PROBE_RECORDS.with(|slot| slot.borrow().clone())
}

// ── fixtures ───────────────────────────────────────────────────────────────

/// `@optimized` target of the single-geometry-arg probe.
const ORDER_PROBE_TARGET: &str = "test::redispatch-order-probe";

/// `@optimized` target of the two-geometry-arg (MIXED) probe.
const MIXED_PROBE_TARGET: &str = "test::redispatch-mixed-arg-probe";

/// The geometry-consuming `@optimized` probe plus the consuming structure.
///
/// `order_probe` returns a NON-geometry type (`Int`) so `let probe =
/// order_probe(body)` becomes a *value cell* carrying a
/// `CompiledExprKind::UserFunctionCall` default-expr — which is exactly the
/// shape `redispatch_geometry_consuming_compute_nodes`'s Phase-1 scan matches
/// (its `// ── Phase 1: collect candidates` block in `engine_build.rs`, whose
/// `for (c_id, node_data) in state.snapshot.graph.compute_nodes.iter()` walk
/// skips any node whose `realization_inputs` are already non-empty). A
/// Geometry-typed consumer would lower to a separate realization and be
/// invisible to that scan.
const CONSUMER_STRUCTURE: &str = r#"
@optimized("test::redispatch-order-probe")
fn order_probe(g: Geometry) -> Int {
    0
}

structure OrderConsumer {
    param width : Length = 10mm

    let body = box(width, width, width)
    let probe = order_probe(body)
}
"#;

/// A no-op structure declared AHEAD of `OrderConsumer`. It owns no geometry and
/// no compute node — its only effect is to add one iteration to `build()`'s
/// per-template loop before the consumer's body realizes.
const LEADING_STRUCTURE: &str = r#"
structure Leading {
    param nominal : Real = 1.0
}
"#;

/// A MIXED-arg `@optimized` probe: two `Geometry` params, one of which never
/// gets a kernel-backed handle.
///
/// `solid` realizes normally; `ghost` is a `sphere(..)`, which
/// [`SphereRefusingKernel`] refuses to execute, so its realization fails and its
/// value cell keeps the SYMBOLIC
/// `Value::GeometryHandle { kernel_handle: None, .. }` placeholder minted by
/// `mint_symbolic_geometry_handles_into_values`.
///
/// This is the one shape in which fix (A) — the `realization_probe_args`
/// downgrade in `redispatch_geometry_consuming_compute_nodes` — is NOT
/// subsumed by fix (B)'s content guard: the node has a hydrated arg, so
/// `.all(content().is_none())` is false and the write happens. Without the
/// downgrade the symbolic sibling's `realization_ref` rides along into
/// `realization_inputs` and into the compute cache key.
const MIXED_ARG_STRUCTURE: &str = r#"
@optimized("test::redispatch-mixed-arg-probe")
fn mixed_probe(a: Geometry, b: Geometry) -> Int {
    0
}

structure MixedConsumer {
    param width : Length = 10mm

    let solid = box(width, width, width)
    let ghost = sphere(width)
    let probe = mixed_probe(solid, ghost)
}
"#;

/// A geometry kernel that REALIZES but cannot TESSELLATE.
///
/// A **decorator** over `MockGeometryKernel`, not a re-implementation:
/// `execute`/`query`/`export` forward to `inner` verbatim, so `let body =
/// box(..)` realizes exactly as it does under the plain mock — same
/// `GeometryHandleId` minting, same Wire/Solid repr classification, same
/// op recording — and its value cell hydrates to
/// `Value::GeometryHandle { kernel_handle: Some(_), .. }`. Only `tessellate`
/// is overridden, and it always fails, so Part A's pre-tessellation in
/// `redispatch_geometry_consuming_compute_nodes` cannot populate the
/// projection store, the realization stays at its `ReprKind::BRep` default,
/// and `project_realization_read_handle` takes the identity-only BRep arm:
/// a `RealizationReadHandle` whose `content()` is `None`.
///
/// That is the second half of the #5951 defect, isolated from template
/// ordering: a handle that IS kernel-backed but carries nothing real.
///
/// Delegating rather than copying matters: an op added to the mock's Wire arm
/// would otherwise diverge here silently, surfacing as a confusing repr
/// mismatch instead of a compile error.
///
/// Defined locally rather than added to `reify-test-support` because it exists
/// only to pin this contract — neither `FailingMockGeometryKernel` (whose
/// `execute` fails, so no handle is ever minted) nor `CountingMockKernel`
/// covers realizes-but-cannot-tessellate.
#[derive(Default)]
struct NonTessellatingKernel {
    inner: reify_test_support::MockGeometryKernel,
}

impl GeometryKernel for NonTessellatingKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.inner.execute(op)
    }

    fn query(
        &self,
        query: &reify_ir::GeometryQuery,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        self.inner.query(query)
    }

    fn export(
        &self,
        handle: reify_ir::GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        Err(reify_ir::TessError::TessellationFailed(
            "NonTessellatingKernel never tessellates — the projection store \
             stays empty so the realization projects to content: None"
                .into(),
        ))
    }
}

/// [`NonTessellatingKernel`] with a shared `tessellate` call counter.
///
/// The counter is the observation point for the amendment's negative cache:
/// keeping a non-projectable node a redispatch candidate means the pass — and
/// with it Part A's pre-tessellation — is re-entered on every trailing
/// template, so without the cache the count grows with the number of templates
/// declared after the consumer.
#[derive(Clone)]
struct CountingNonTessellatingKernel {
    inner: std::sync::Arc<std::sync::Mutex<reify_test_support::MockGeometryKernel>>,
    tessellate_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl CountingNonTessellatingKernel {
    fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(
                reify_test_support::MockGeometryKernel::new(),
            )),
            tessellate_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    fn tessellate_calls(&self) -> usize {
        self.tessellate_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl GeometryKernel for CountingNonTessellatingKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.inner.lock().unwrap().execute(op)
    }

    fn query(
        &self,
        query: &reify_ir::GeometryQuery,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        self.inner.lock().unwrap().query(query)
    }

    fn export(
        &self,
        handle: reify_ir::GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.lock().unwrap().export(handle, format, writer)
    }

    fn tessellate(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        self.tessellate_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(reify_ir::TessError::TessellationFailed(
            "CountingNonTessellatingKernel never tessellates".into(),
        ))
    }
}

/// A geometry kernel that realizes everything EXCEPT `sphere(..)`.
///
/// Same decorator shape as [`NonTessellatingKernel`]: every trait method
/// forwards to `inner`, and the single overridden behaviour is one `execute`
/// arm. A refused `execute` mints no handle, so the refused body's realization
/// fails and `post_process_geometry_handle_cells` never hydrates its value
/// cell — it keeps the symbolic `kernel_handle: None` placeholder. That is the
/// only way to hold ONE arg of a multi-geometry `@optimized` node symbolic
/// while a sibling arg is fully hydrated, within a single template.
#[derive(Default)]
struct SphereRefusingKernel {
    inner: reify_test_support::MockGeometryKernel,
}

impl GeometryKernel for SphereRefusingKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        if matches!(op, reify_ir::GeometryOp::Sphere { .. }) {
            return Err(reify_ir::GeometryError::OperationFailed(
                "SphereRefusingKernel refuses sphere(): the body stays symbolic \
                 so the consuming @optimized node has one hydrated and one \
                 unhydrated geometry arg"
                    .into(),
            ));
        }
        self.inner.execute(op)
    }

    fn query(
        &self,
        query: &reify_ir::GeometryQuery,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        self.inner.query(query)
    }

    fn export(
        &self,
        handle: reify_ir::GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        handle: reify_ir::GeometryHandleId,
        tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        self.inner.tessellate(handle, tolerance)
    }
}

/// Build the module, register the probe, and run `build()` against `kernel`.
///
/// Returns the engine (so callers can inspect the post-build graph) and the
/// build diagnostics.
///
/// With `MockGeometryKernel`, `execute` accepts any op and mints a real
/// `GeometryHandleId`, so `let body = box(..)` realizes unconditionally and
/// `post_process_geometry_handle_cells` hydrates the cell with
/// `kernel_handle: Some(_)` — no OCCT required.
fn build_with_probe_using(
    source: &str,
    kernel: Box<dyn reify_ir::GeometryKernel>,
) -> (reify_eval::Engine, Vec<reify_core::Diagnostic>) {
    let compiled = reify_test_support::parse_and_compile(source);
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(kernel),
    );
    engine.register_compute_fn(ORDER_PROBE_TARGET, order_probe_fn as reify_eval::ComputeFn);
    // Same recording fn under the mixed-arg target; only one of the two
    // fixtures is ever compiled per build, so the shared capture slot stays
    // unambiguous.
    engine.register_compute_fn(MIXED_PROBE_TARGET, order_probe_fn as reify_eval::ComputeFn);
    let diagnostics = engine.build(&compiled, ExportFormat::Step).diagnostics;
    (engine, diagnostics)
}

/// [`build_with_probe_using`] against the default tessellating mock kernel,
/// keeping only the diagnostics.
fn build_with_probe(source: &str) -> Vec<reify_core::Diagnostic> {
    build_with_probe_using(
        source,
        Box::new(reify_test_support::MockGeometryKernel::new()),
    )
    .1
}

/// The `realization_inputs` recorded on the probe's compute node in the
/// post-build snapshot graph.
///
/// This is the state the Phase-1 candidate gate reads
/// (`realization_inputs.is_empty()`): EMPTY means the node is still a
/// redispatch candidate, non-empty means the one-shot latch has been tripped
/// and no later pass will ever revisit it.
fn probe_node_realization_inputs(engine: &reify_eval::Engine) -> Vec<usize> {
    probe_node_realization_input_refs(engine, ORDER_PROBE_TARGET)
        .iter()
        .map(Vec::len)
        .collect()
}

/// The `realization_inputs` REFS recorded on every compute node dispatching to
/// `target`, one inner `Vec` per node.
///
/// [`probe_node_realization_inputs`] answers "was the latch tripped?"; this
/// answers "by WHICH realizations?", which is what the mixed-arg case needs —
/// there the latch is legitimately tripped and the contract is about the
/// membership of the recorded list.
fn probe_node_realization_input_refs(
    engine: &reify_eval::Engine,
    target: &str,
) -> Vec<Vec<reify_core::RealizationNodeId>> {
    let state = engine
        .eval_state()
        .expect("build() must leave an EvaluationState behind");
    state
        .snapshot
        .graph
        .compute_nodes
        .iter()
        .filter(|(_, n)| n.target == target)
        .map(|(_, n)| n.realization_inputs.clone())
        .collect()
}

/// The `realization_ref` that the geometry let `<entity>.<name>` binds.
///
/// Read from `eval_state().snapshot.values`, which is a reliable source of the
/// REF and says nothing about hydration: `post_process_geometry_handle_cells`
/// writes the kernel-backed handle into `build()`'s LOCAL `values` map, not
/// into the snapshot, so every geometry cell in the snapshot still carries the
/// `kernel_handle: None` placeholder `mint_symbolic_geometry_handles_into_values`
/// minted — realized and unrealized bodies alike. Do not read `kernel_handle`
/// from here to decide whether a body hydrated; the probe records are the
/// observation point for that.
///
/// Panics unless the cell holds a `Value::GeometryHandle`.
fn geometry_cell_realization(
    engine: &reify_eval::Engine,
    entity: &str,
    name: &str,
) -> reify_core::RealizationNodeId {
    let state = engine
        .eval_state()
        .expect("build() must leave an EvaluationState behind");
    let cell = reify_core::ValueCellId::new(entity, name);
    match state.snapshot.values.get(&cell) {
        Some((
            Value::GeometryHandle {
                realization_ref, ..
            },
            _,
        )) => realization_ref.clone(),
        other => panic!("`{entity}.{name}` must hold a Value::GeometryHandle, got {other:?}"),
    }
}

/// Shared assertion: the post-hydration redispatch must have delivered a
/// kernel-backed geometry arg to the probe at least once.
fn assert_probe_saw_kernel_backed_body(case: &str, records: &[ProbeRecord]) {
    assert!(
        !records.is_empty(),
        "{case}: the @optimized probe must be invoked at least once; \
         it was never dispatched (records: {records:?})"
    );
    // Diagnostic aid only (NOT the assertion): count invocations that were
    // handed non-empty `realization_inputs` carrying nothing real. Each such
    // invocation corresponds to a write that tripped the Phase-1
    // `realization_inputs.is_empty()` latch while delivering no content —
    // the signature of the #5951 strand. Step-3/step-4 turn this into a
    // first-class assertion; here it just makes the failure legible.
    let empty_content_latches = records
        .iter()
        .filter(|r| r.realization_inputs_len > 0 && !r.any_realized_content)
        .count();
    assert!(
        records.iter().any(|r| r.kernel_backed_arg),
        "{case}: at least one probe invocation must receive a KERNEL-BACKED \
         `Value::GeometryHandle {{ kernel_handle: Some(_), .. }}` — i.e. the \
         post-hydration `redispatch_geometry_consuming_compute_nodes` pass must \
         actually run for this node. Every recorded invocation carried an \
         unhydrated arg instead, which is the task #5951 strand: a premature \
         per-template redispatch consumed the SYMBOLIC \
         (`kernel_handle: None`) handle and latched the Phase-1 \
         `realization_inputs.is_empty()` candidate gate, so the correct later \
         pass was skipped forever. {empty_content_latches} of \
         {} invocation(s) were handed non-empty realization_inputs carrying no \
         realized content. Records: {records:?}",
        records.len(),
    );
}

// ── tests ──────────────────────────────────────────────────────────────────

/// Task #5951 (RED before the fix): a single no-op structure declared ahead of
/// the geometry-consuming structure must NOT strand the compute node.
///
/// `Leading` owns no geometry and no compute node — it contributes nothing but
/// one extra iteration of `build()`'s per-template loop. That is enough to make
/// the redispatch fire while `OrderConsumer`'s body is still symbolic.
#[test]
fn redispatch_reaches_kernel_backed_body_when_a_template_precedes_the_consumer() {
    reset_records();
    let source = format!("{LEADING_STRUCTURE}{CONSUMER_STRUCTURE}");
    let diagnostics = build_with_probe(&source);

    // The strand is SILENT by design (the `ReprKind::BRep` projection arm emits
    // no diagnostic), so a diagnostic assertion cannot detect it. Pin the
    // silence so a future reader does not look for an error that never comes.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "leading-template case must build without Error diagnostics \
         (the defect is silent, not diagnosed): {errors:?}"
    );

    assert_probe_saw_kernel_backed_body("leading-template case", &recorded());
}

/// Control (GREEN both before and after the fix): the byte-identical module
/// WITHOUT the leading structure.
///
/// `OrderConsumer` is template index 0, so the FIRST redispatch call already
/// runs after its own `post_process_geometry_handle_cells` hydration and sees a
/// kernel-backed handle. Pairing this with the test above isolates template
/// ordering as the sole variable — same body, same probe, same kernel, same
/// assertion.
#[test]
fn redispatch_reaches_kernel_backed_body_when_the_consumer_is_the_first_template() {
    reset_records();
    let diagnostics = build_with_probe(CONSUMER_STRUCTURE);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "control case must build without Error diagnostics: {errors:?}"
    );

    assert_probe_saw_kernel_backed_body("control (no leading template)", &recorded());
}

/// Task #5951 (RED after fix (A), GREEN after fix (B)): the Phase-1 latch must
/// never be tripped by a write that carried nothing real.
///
/// Fix (A) filters SYMBOLIC handles, but a KERNEL-BACKED handle whose
/// realization projects to no content slips straight through it. The kernel
/// here realizes the body for real (`kernel_handle: Some(_)`) and then fails
/// `tessellate`, so Part A's pre-tessellation cannot populate the projection
/// store and `project_realization_read_handle` returns a handle with
/// `content(): None`.
///
/// Before fix (B), `redispatch_geometry_consuming_compute_nodes` still wrote
/// those content-free inputs onto the node and still re-dispatched, tripping
/// the one-shot `realization_inputs.is_empty()` candidate gate. The node was
/// then permanently excluded from every later pass — including one that could
/// have delivered real content — and, per the BRep arm's identity-only
/// contract (PRD §4 D1), nothing anywhere reported it.
///
/// Note this case carries NO leading structure: the consumer is template 0.
/// It isolates the content half of the defect from the ordering half.
#[test]
fn latch_is_not_tripped_when_the_rebuilt_inputs_carry_no_realized_content() {
    reset_records();
    let (engine, diagnostics) = build_with_probe_using(
        CONSUMER_STRUCTURE,
        Box::new(NonTessellatingKernel::default()),
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a kernel that cannot tessellate must not fail the build outright — \
         the degradation this test pins is silent, not diagnosed: {errors:?}"
    );

    let records = recorded();
    assert!(
        !records.is_empty(),
        "the @optimized probe must be invoked at least once; it was never \
         dispatched (records: {records:?})"
    );

    // (a) Observable at the trampoline: the same `realization_inputs` that get
    //     written onto the node are the ones handed to `run_compute_dispatch`,
    //     so a content-free write is visible here.
    let contentless: Vec<_> = records
        .iter()
        .filter(|r| r.realization_inputs_len > 0 && !r.any_realized_content)
        .collect();
    assert!(
        contentless.is_empty(),
        "no probe invocation may be handed non-empty `realization_inputs` in \
         which every `RealizationReadHandle::content()` is None. Such a write \
         delivers nothing to the trampoline yet still trips the Phase-1 \
         `realization_inputs.is_empty()` one-shot latch, permanently and \
         SILENTLY stranding the node on its degraded first-dispatch result \
         (task #5951 fix (B)). Offending invocations: {contentless:?} of all \
         records: {records:?}"
    );

    // (b) Observable in the post-build graph: the candidate gate is untripped,
    //     so a later pass — a later template's iteration, a later build tick —
    //     can still do the real work.
    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![0],
        "the probe's compute node must still have EMPTY `realization_inputs` \
         after a build that could not project any content, so it remains a \
         redispatch candidate. A non-empty value here is the latch trip: the \
         Phase-1 gate skips this node from now on, forever."
    );
}

/// Positive control for fix (B) (GREEN before and after): when the projection
/// DOES yield real content, the write must still happen.
///
/// Same module, same probe — only the kernel differs. `MockGeometryKernel`
/// tessellates successfully, so Part A populates the projection store, the
/// rebuilt handle carries `Some(RealizedContent::SurfaceMesh(..))`, and the
/// node is legitimately dispatched with real inputs and legitimately latched.
///
/// Pairing this with the test above pins fix (B) as a guard on *contentless*
/// writes specifically — not a blanket suppression that would stop the
/// redispatch from ever recording its inputs.
#[test]
fn latch_is_tripped_normally_when_the_rebuilt_inputs_carry_realized_content() {
    reset_records();
    let (engine, _diagnostics) = build_with_probe_using(
        CONSUMER_STRUCTURE,
        Box::new(reify_test_support::MockGeometryKernel::new()),
    );

    let records = recorded();
    assert!(
        records
            .iter()
            .any(|r| r.realization_inputs_len > 0 && r.any_realized_content),
        "at least one probe invocation must be handed `realization_inputs` \
         carrying real projected content — otherwise fix (B) has over-fired \
         and suppressed the legitimate redispatch write. Records: {records:?}"
    );

    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![1],
        "a redispatch that DID deliver content must record its \
         `realization_inputs` on the node as before"
    );
}

/// Task #5951 fix (A), pinned on the one shape where fix (B) does NOT subsume
/// it: a MIXED-arg node — one hydrated geometry arg, one still symbolic.
///
/// Fix (A) is two things at the same site: the Phase-2 gate narrowed to
/// `kernel_handle: Some(_)`, and the `realization_probe_args` downgrade fed to
/// `build_compute_realization_inputs`. In every SINGLE-geometry-arg case fix
/// (B)'s content guard reaches the same verdict for free — a symbolic handle
/// projects to `content: None`, so `.all(content().is_none())` is true and the
/// write is skipped anyway. That is why the four tests above stay green with
/// fix (A) reverted, and why this case exists.
///
/// Here `solid` hydrates and `ghost` does not, so:
///   * the Phase-2 gate passes on `solid` (a kernel-backed arg IS present),
///   * `solid` projects to real content, so the content guard does NOT fire,
///   * and the write happens — legitimately.
///
/// The question fix (A) answers is what that write CONTAINS.
/// `build_compute_realization_inputs` matches `Value::GeometryHandle { .. }`
/// regardless of `kernel_handle`, so on RAW `arg_values` it records `ghost`'s
/// `realization_ref` too: a content-free entry in the node's
/// `realization_inputs`, which is a dependency edge for freshness and a term in
/// the compute cache key (`compute_cache_key(node, graph)`), keyed on a
/// realization that never produced anything. The downgrade keeps it out.
#[test]
fn a_symbolic_sibling_arg_is_kept_out_of_realization_inputs() {
    reset_records();
    let (engine, diagnostics) = build_with_probe_using(
        MIXED_ARG_STRUCTURE,
        Box::new(SphereRefusingKernel::default()),
    );

    let solid_ref = geometry_cell_realization(&engine, "MixedConsumer", "solid");
    let ghost_ref = geometry_cell_realization(&engine, "MixedConsumer", "ghost");
    assert_ne!(
        solid_ref, ghost_ref,
        "the two geometry lets must bind DISTINCT realizations"
    );

    // Premise, asserted rather than assumed: `ghost` never realized, so it can
    // only be carrying the symbolic placeholder. If a future change makes
    // `sphere()` succeed under this kernel — or makes a refused realization
    // hydrate to a stub `Some(handle)` — this fixture stops exercising the
    // mixed case and the conclusion below would hold vacuously.
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == reify_core::Severity::Error
                && d.message.contains("SphereRefusingKernel refuses sphere()")),
        "`ghost`'s realization must be REFUSED by the kernel — that refusal is \
         what keeps its handle symbolic while its sibling hydrates. \
         Diagnostics: {diagnostics:?}"
    );

    let per_node = probe_node_realization_input_refs(&engine, MIXED_PROBE_TARGET);
    assert_eq!(
        per_node.len(),
        1,
        "exactly one compute node must dispatch to `{MIXED_PROBE_TARGET}`; \
         got {per_node:?}"
    );
    let inputs = &per_node[0];

    assert!(
        !inputs.contains(&ghost_ref),
        "the SYMBOLIC sibling's realization `{ghost_ref:?}` must NOT appear in \
         the node's `realization_inputs`. It projects to no content, so \
         recording it adds a freshness edge and a compute-cache-key term for a \
         realization that produced nothing — task #5951 fix (A): \
         `redispatch_geometry_consuming_compute_nodes` probes \
         `build_compute_realization_inputs` through `realization_probe_args`, \
         which downgrades `kernel_handle: None` handles to `Value::Undef`, \
         exactly as the two `@optimized` dispatch sites in `engine_eval.rs` do. \
         Recorded inputs: {inputs:?}"
    );
    assert_eq!(
        inputs,
        &vec![solid_ref.clone()],
        "the hydrated body `{solid_ref:?}` — and only it — must be recorded"
    );

    // And the write really did happen: this is not the content guard firing,
    // and `solid` really did hydrate. Pairing this with the assertion above
    // makes the case specific to fix (A) — fix (B) is inert here.
    let records = recorded();
    assert!(
        records.iter().any(|r| r.kernel_backed_arg
            && r.realization_inputs_len == 1
            && r.any_realized_content),
        "the probe must be dispatched with the one realized body's handle and \
         its projected content — otherwise either `solid` never hydrated (so \
         the Phase-2 gate never opened) or fix (B)'s content guard fired, and \
         this case is not exercising fix (A) at all. Records: {records:?}"
    );
}

/// Trailing no-op structures, `count` of them, declared AFTER the consumer.
///
/// Each adds one iteration to `build()`'s per-template loop, and therefore one
/// re-entry of `redispatch_geometry_consuming_compute_nodes` for a node the
/// content guard has left a candidate.
fn trailing_structures(count: usize) -> String {
    (0..count)
        .map(|i| format!("\nstructure Trailing{i} {{\n    param nominal : Real = 1.0\n}}\n"))
        .collect()
}

/// Amendment to #5951's content guard: keeping a node a redispatch candidate
/// must not make Part A's pre-tessellation cost scale with the number of
/// templates declared after it.
///
/// The guard's whole point is that a non-projectable node stays a candidate so
/// a later pass can still do the real work. The pass, though, is re-entered
/// once per template and scans ALL compute nodes, so the *unmitigated* shape of
/// that decision is a failing `kernel.tessellate()` re-attempted once per
/// trailing template — measured at 1 / 2 / 4 attempts for 0 / 1 / 3 trailing
/// structures. On this mock that is free; on the real OCCT/gmsh path a failing
/// tessellation of a pathological body is not, and it is paid on every build
/// tick.
///
/// `tessellate` is a pure function of `(realization, content_hash)`, so a
/// failure cannot become a success later in the SAME build. `RedispatchPassState`
/// negative-caches it. The invariant asserted here is the one that matters and
/// is robust to any constant baseline: the count does not GROW with the number
/// of trailing templates.
#[test]
fn a_failed_pre_tessellation_is_not_retried_once_per_trailing_template() {
    let mut counts = Vec::new();
    for trailing in [0usize, 1, 3] {
        reset_records();
        let kernel = CountingNonTessellatingKernel::new();
        let source = format!("{CONSUMER_STRUCTURE}{}", trailing_structures(trailing));
        let (_engine, _diagnostics) = build_with_probe_using(&source, Box::new(kernel.clone()));
        counts.push((trailing, kernel.tessellate_calls()));
    }

    let baseline = counts[0].1;
    assert!(
        baseline > 0,
        "the fixture must reach Part A's pre-tessellation at least once, \
         otherwise this test cannot observe the retry it is guarding against \
         (counts: {counts:?})"
    );
    for &(trailing, calls) in &counts {
        assert_eq!(
            calls, baseline,
            "`kernel.tessellate()` was attempted {calls} times with {trailing} \
             trailing template(s) but {baseline} time(s) with none. The content \
             guard leaves the node a redispatch candidate, so the pass is \
             re-entered once per trailing template; the negative cache in \
             `RedispatchPassState` must make the failed pre-tessellation \
             idempotent within a build. All counts: {counts:?}"
        );
    }
}

/// Amendment to #5951's content guard: a node the guard could never rescue must
/// not end the build silently.
///
/// The guard converts a permanent silent strand into a *retried* one — but for
/// a body that can never project content (this kernel's shape, or a real body
/// OCCT cannot tessellate) no later pass ever succeeds, and the end state is
/// the pre-fix one: a node stranded on its degraded first-dispatch result. The
/// original defect's defining property is that NOTHING reported it — the
/// `ReprKind::BRep` arm of `project_realization_read_handle` is identity-only
/// by design (PRD §4 D1), so `degrade_projection`'s own Warning is never
/// reached. Recoverable-in-principle is not the same as visible.
///
/// One Warning per stranded NODE per build — not one per per-template call —
/// so adding trailing templates must not multiply it.
#[test]
fn a_permanently_stranded_node_is_reported_once_at_end_of_build() {
    for trailing in [0usize, 3] {
        reset_records();
        let source = format!("{CONSUMER_STRUCTURE}{}", trailing_structures(trailing));
        let (engine, diagnostics) =
            build_with_probe_using(&source, Box::new(NonTessellatingKernel::default()));

        // Still a candidate — i.e. genuinely the stranded shape this warns about.
        assert_eq!(
            probe_node_realization_inputs(&engine),
            vec![0],
            "precondition: the content guard must have left the node a candidate"
        );

        let strand_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == reify_core::Severity::Warning
                    && d.message.contains("projected no content")
            })
            .collect();
        assert_eq!(
            strand_warnings.len(),
            1,
            "exactly ONE Warning must report the stranded node with {trailing} \
             trailing template(s) — the degradation must be visible, and it must \
             be reported per node, not per per-template redispatch call. \
             Got: {strand_warnings:?}"
        );
        assert!(
            strand_warnings[0].message.contains(ORDER_PROBE_TARGET),
            "the Warning must name the @optimized target so the user can find \
             the degraded node: {:?}",
            strand_warnings[0]
        );
    }
}
