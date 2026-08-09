// SPDX-License-Identifier: AGPL-3.0-or-later

//! Regression tests for task #5951 — a premature per-template
//! `Engine::redispatch_geometry_consuming_compute_nodes` pass permanently
//! strands a geometry-consuming `@optimized` compute node.
//!
//! ## The defect these tests pin
//!
//! `redispatch_geometry_consuming_compute_nodes` is called once per template,
//! from inside `build()`'s `for (t_idx, template) in module.templates.iter()`
//! loop (`engine_build.rs:4619`, nested under `:4246`), and each call scans
//! **all** compute nodes in the graph — not just the current template's.
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
//! Both cases compile a module declaring one `@optimized` probe consuming a
//! `let body = box(..)`, and register a recording `ComputeFn` for it. The only
//! difference is whether a no-op `structure` is declared AHEAD of the consumer.
//! The assertion is identical in both: at least one probe invocation must see a
//! KERNEL-BACKED (`kernel_handle: Some(_)`) geometry arg — i.e. the
//! post-hydration redispatch actually ran.
//!
//! The control case isolates template ordering and nothing else: same body,
//! same probe, same kernel, same assertion.

use std::cell::RefCell;

use reify_ir::{ExportFormat, Value};

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

/// The geometry-consuming `@optimized` probe plus the consuming structure.
///
/// `order_probe` returns a NON-geometry type (`Int`) so `let probe =
/// order_probe(body)` becomes a *value cell* carrying a
/// `CompiledExprKind::UserFunctionCall` default-expr — which is exactly the
/// shape `redispatch_geometry_consuming_compute_nodes`'s Phase-1 scan matches
/// (`engine_build.rs:9652`). A Geometry-typed consumer would lower to a
/// separate realization and be invisible to that scan.
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

/// Build the module, register the probe, and run `build()` against a mock
/// kernel. Returns the build diagnostics for the caller to assert on.
///
/// `MockGeometryKernel::execute` accepts any op and mints a real
/// `GeometryHandleId`, so `let body = box(..)` realizes unconditionally and
/// `post_process_geometry_handle_cells` hydrates the cell with
/// `kernel_handle: Some(_)` — no OCCT required.
fn build_with_probe(source: &str) -> Vec<reify_core::Diagnostic> {
    let compiled = reify_test_support::parse_and_compile(source);
    let kernel = reify_test_support::MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(kernel)),
    );
    engine.register_compute_fn(
        "test::redispatch-order-probe",
        order_probe_fn as reify_eval::ComputeFn,
    );
    engine.build(&compiled, ExportFormat::Step).diagnostics
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
