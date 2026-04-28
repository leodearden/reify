//! End-to-end runtime tests for the conformance-query stdlib helpers
//! `is_watertight`, `is_manifold`, `is_orientable` (task 2320).
//!
//! These tests exercise the full pipeline: parse → `compile_with_stdlib` →
//! `Engine::build` (with `MockGeometryKernel`) → assert the resulting
//! `BuildResult.values` contain `Value::Bool(_)` for the conformance-query
//! `let` bindings.
//!
//! Architecture: the kernel-aware dispatch lives in
//! `crates/reify-eval/src/geometry_ops.rs::try_eval_conformance_query` and is
//! invoked as a post-process from `engine_build.rs`'s build / build_snapshot
//! after `execute_realization_ops` populates `named_steps`. These tests pin
//! that the post-process correctly patches the resulting `Value::Bool(_)`
//! into the `ValueMap` (overwriting the `Value::Undef` left by the pure
//! `eval_expr` path).
//!
//! The mock kernel allocates `GeometryHandleId(1)` for the first `execute`
//! call, so each fixture's `box(10mm, 10mm, 10mm)` resolves to handle id 1
//! and the kernel is pre-configured with `with_query_result(GeometryHandleId(1), …)`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use reify_compiler::compile_with_stdlib;
use reify_eval::Engine;
use reify_test_support::MockGeometryKernel;
use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, ModulePath, QueryError, Severity, TessError, Value,
    ValueCellId,
};

/// Parse and compile a source string with the stdlib prelude.
/// Asserts the parse and compile pipelines produce no errors.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("conformance_runtime"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

/// Build an `Engine` with the constraint checker and a mock kernel
/// that returns `Value::Bool(reply)` for any handle-id query.
fn engine_with_mock_kernel(reply: bool) -> Engine {
    let kernel = MockGeometryKernel::new()
        .with_query_result(GeometryHandleId(1), Value::Bool(reply));
    let checker = reify_constraints::SimpleConstraintChecker;
    Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Step-11 (RED): a `let watertight = is_watertight(body)` cell on a
/// structure containing `let body = box(10mm, 10mm, 10mm)` must resolve to
/// `Value::Bool(true)` when the kernel reports `IsWatertight(handle=1) → true`.
///
/// Fails until step-12 wires `try_eval_conformance_query` into
/// `engine_build.rs`'s post-process; without that wire-up the cell stays at
/// its compiled default (`Value::Undef`) because pure-value `eval_expr` has
/// no kernel access.
#[test]
fn is_watertight_let_resolves_to_bool_true_via_kernel_reply() {
    let source =
        "structure def Bracket {\n    let body = box(10mm, 10mm, 10mm)\n    let watertight = is_watertight(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(true);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "watertight");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Bool(true)),
        "Bracket.watertight must resolve to Bool(true) via kernel IsWatertight reply, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// Step-11 (RED): parallel test for `is_manifold`. Same structure
/// shape with `let manifold = is_manifold(body)` instead.
#[test]
fn is_manifold_let_resolves_to_bool_true_via_kernel_reply() {
    let source =
        "structure def Bracket {\n    let body = box(10mm, 10mm, 10mm)\n    let manifold = is_manifold(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(true);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "manifold");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Bool(true)),
        "Bracket.manifold must resolve to Bool(true) via kernel IsManifold reply, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// Step-11 (RED): parallel test for `is_orientable`. Same structure
/// shape with `let orientable = is_orientable(body)` instead.
#[test]
fn is_orientable_let_resolves_to_bool_true_via_kernel_reply() {
    let source =
        "structure def Bracket {\n    let body = box(10mm, 10mm, 10mm)\n    let orientable = is_orientable(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(true);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "orientable");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Bool(true)),
        "Bracket.orientable must resolve to Bool(true) via kernel IsOrientable reply, \
         got {:?}",
        result.values.get(&cell),
    );
}

// ── Step-13: negative-path / defensive integration tests ────────────────────

/// Step-13: the kernel's `Value::Bool(false)` reply must propagate through
/// the post-process unchanged when no matching marker trait is declared.
/// This exercises the full `kernel.query(...)` round-trip in
/// `try_eval_conformance_query` (no escape-hatch short-circuit) end-to-end
/// through `engine_build.rs::post_process_conformance_queries`.
#[test]
fn is_watertight_let_honours_kernel_bool_false_reply() {
    let source =
        "structure def Bracket {\n    let body = box(10mm, 10mm, 10mm)\n    let watertight = is_watertight(body)\n}";
    let compiled = compile_no_errors(source);
    // No `: Watertight` bound on the structure, so the escape hatch is
    // skipped and the kernel's Bool(false) reply is honoured.
    let mut engine = engine_with_mock_kernel(false);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "watertight");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Bool(false)),
        "Bracket.watertight must resolve to Bool(false) when the kernel reports the \
         body is not watertight (no escape-hatch short-circuit), got {:?}",
        result.values.get(&cell),
    );
}

// ── Step-15: end-to-end user-assertion escape-hatch integration test ─────────

/// Test-local wrapper around `MockGeometryKernel` that increments a counter
/// whenever a `GeometryQuery::IsWatertight(_)` query is dispatched against
/// the kernel. The counter is shared via `Arc<AtomicUsize>` so the test can
/// inspect it after the kernel is moved into the engine's `Box<dyn _>`.
struct RecordingMockKernel {
    inner: MockGeometryKernel,
    is_watertight_query_count: Arc<AtomicUsize>,
}

impl GeometryKernel for RecordingMockKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        if matches!(query, GeometryQuery::IsWatertight(_)) {
            self.is_watertight_query_count.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.query(query)
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

/// Step-15: end-to-end escape-hatch integration test.
///
/// A structure with a `: Watertight` declaration must short-circuit
/// `is_watertight(body)` to `Value::Bool(true)` *without* invoking the
/// kernel — even when the kernel is pre-configured to reply `Bool(false)`.
/// Pins that the user-assertion override (`try_eval_conformance_query`'s
/// step-3 escape hatch) composes correctly end-to-end with
/// `engine_build.rs::post_process_conformance_queries` and the structure's
/// `trait_bounds` plumbing carrying the `"Watertight"` marker.
///
/// Asserts both:
///   (a) the cell value is `Bool(true)` (user assertion wins over the
///       kernel's would-fail reply), AND
///   (b) the recording kernel observes **zero** `GeometryQuery::IsWatertight`
///       round-trips (the kernel was never invoked for this conformance check).
#[test]
fn watertight_user_assertion_short_circuits_kernel_query() {
    let source = "structure def TrustedShell : Watertight {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let watertight = is_watertight(body)\n}";
    let compiled = compile_no_errors(source);

    let count = Arc::new(AtomicUsize::new(0));
    // Configure the inner kernel to return `Bool(false)` if it were ever
    // consulted — so a non-zero count would also surface as `Bool(false)`
    // in the cell value, double-pinning the escape-hatch contract.
    let inner =
        MockGeometryKernel::new().with_query_result(GeometryHandleId(1), Value::Bool(false));
    let kernel = RecordingMockKernel {
        inner,
        is_watertight_query_count: count.clone(),
    };
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("TrustedShell", "watertight");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Bool(true)),
        "TrustedShell.watertight must short-circuit to Bool(true) via the \
         `: Watertight` user assertion (kernel would have replied Bool(false)), \
         got {:?}",
        result.values.get(&cell),
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "RecordingMockKernel must observe zero IsWatertight queries when the \
         enclosing structure declares `: Watertight` (escape-hatch short-circuit \
         is checked before the kernel.query round-trip)",
    );
}

/// Step-13: defensive non-`ValueRef` arg test.
///
/// `is_watertight(42)` compiles (per step-4: `result_type = Bool` is forced
/// regardless of arg shape) and at build-time must fall through to
/// `Value::Undef` rather than panicking.  Pinned guard inside
/// `try_eval_conformance_query` rejects non-`ValueRef` args before any
/// `named_steps` lookup or `kernel.query(...)` round-trip, so the cell
/// stays at the compiled default left by `eval_expr` (`Value::Undef`).
///
/// This pins the v0.1 contract: ill-formed conformance-query call sites
/// degrade gracefully rather than crashing the build.
#[test]
fn is_watertight_with_literal_int_arg_falls_through_to_undef() {
    let source =
        "structure def Bracket {\n    let body = box(10mm, 10mm, 10mm)\n    let watertight = is_watertight(42)\n}";
    let compiled = compile_no_errors(source);
    // Kernel is configured with Bool(true) — but the literal-arg guard in
    // `try_eval_conformance_query` must short-circuit to None *before* the
    // kernel is consulted, so this configuration is irrelevant.
    let mut engine = engine_with_mock_kernel(true);

    // Build must not panic.  The cell value should be Undef, NOT Bool(true)
    // (which would imply the post-process incorrectly resolved an unsupported
    // arg shape via the kernel) and NOT a panic in any layer.
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "watertight");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Undef),
        "Bracket.watertight with a literal-int arg must fall through to Undef, got {:?}",
        result.values.get(&cell),
    );
}

// ── Step-17: OCCT-backed end-to-end test ────────────────────────────────────

/// Step-17: OCCT-backed end-to-end smoke test for the conformance dispatch
/// surface. Gated by `reify_kernel_occt::OCCT_AVAILABLE` so the file always
/// compiles; the test is a runtime no-op when the OCCT shared lib is absent.
///
/// Mirrors the test the task's testStrategy explicitly names:
///
///   `cargo test -p reify-eval -- conformance_runtime` …
///   `box(10mm, 10mm, 10mm)` returns `true` for all three helpers.
///
/// Confirms `try_eval_conformance_query` composes correctly with the real
/// OCCT kernel — the dispatch resolves the geometry-arg ValueRef against the
/// realisation's named-step handle map, round-trips
/// `GeometryQuery::IsWatertight | IsManifold | IsOrientable` through OCCT,
/// and patches the resulting `Bool(true)` into each cell.
#[test]
fn box_is_watertight_manifold_orientable_via_occt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping box_is_watertight_manifold_orientable_via_occt: OCCT not available"
        );
        return;
    }
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let watertight = is_watertight(body)\n    \
        let manifold = is_manifold(body)\n    \
        let orientable = is_orientable(body)\n}";
    let compiled = compile_no_errors(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::DispatchPlanner::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.build(&compiled, ExportFormat::Step);

    for cell_name in ["watertight", "manifold", "orientable"] {
        let cell = ValueCellId::new("Bracket", cell_name);
        assert_eq!(
            result.values.get(&cell),
            Some(&Value::Bool(true)),
            "Bracket.{} for box(10mm,10mm,10mm) must resolve to Bool(true) via OCCT, got {:?}",
            cell_name,
            result.values.get(&cell),
        );
    }
}
