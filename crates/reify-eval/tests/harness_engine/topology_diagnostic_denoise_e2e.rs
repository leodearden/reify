//! Capstone acceptance test for task #5196 (de-noise persistent-naming
//! bookkeeping diagnostics: L1 equal-index guard + L3 Info severity + L4
//! silent-drop summarization + L2 selector-presence gate).
//!
//! Two observable-signal cases, per the task's design:
//!
//! - (A) A healthy, selector-free multi-boolean model (mirroring the
//!   litter-tray dogfooding example that motivated this task) must emit
//!   ZERO diagnostics carrying `DiagnosticCode::TopologyCorrespondenceDropped`
//!   or `DiagnosticCode::TopologyAttributeLocalIndexReassigned` **at any
//!   severity**, and no `Severity::Error`.
//!
//!   The severity-AGNOSTIC (code-only) count is the load-bearing assertion,
//!   mirroring the pattern
//!   `engine_build_emits_local_index_reassignment_for_coincident_box_union`
//!   (`tests/harness_topology_selector/topology_attribute_e2e.rs`) uses. A
//!   severity-qualified `== Severity::Warning` filter would be VACUOUS here:
//!   after L3 neither production emit site can produce `Warning` for either
//!   code, so such an assertion holds unconditionally — independent of L1,
//!   L2, and the fixture. That vacuity was this test's original defect; the
//!   zero-Warning check is retained below only as a secondary *global*
//!   severity contract (nothing anywhere in the build re-Warns these codes),
//!   never as the primary signal.
//!
//!   What the code-only assertion actually pins, jointly: L1's equal-index
//!   guard suppresses the coincident-union tie false-positives, and L2's
//!   selector gate skips the tie-scan entirely because this model binds no
//!   selector (`module_binds_selector` is `false` for it — pinned directly by
//!   `module_binds_selector_false_for_selector_free_multi_boolean_model` in
//!   `engine_build/tests.rs`). Because the two mechanisms are in series here,
//!   this case alone cannot separate them; the single-mechanism pins live at
//!   unit level (`detect_local_index_reassignment_*` in
//!   `topology_attribute_propagation.rs` for L1, the `module_binds_selector`
//!   group in `engine_build/tests.rs` for L2), and case (B) below supplies
//!   the two-sided engine-level control for the gate.
//!
//! - (B) A model that binds a topology selector over a boolean must still
//!   surface real correspondence-degradation signal at `Severity::Info`,
//!   with at most one aggregated silent-drop line per realization (L4).
//!   This proves the L2 gate — which only affects the tie-scan — does not
//!   accidentally suppress the always-on silent-drop tally/flush path.
//!
//!   Case (B) drives `Engine::build` with the `DropInjectingKernel` mock
//!   (same harness as `topology_correspondence_drop_diagnostic_e2e.rs`,
//!   task #4545) rather than real OCCT. This is a deliberate, empirically-
//!   justified choice: real, well-formed OCCT geometry cannot reliably
//!   trigger either diagnostic on demand, BY DESIGN —
//!   `reify-kernel-occt/tests/boolean_op_history_integration.rs` asserts
//!   `silent_drop_count == 0` for its real fixture, and
//!   `SweepOpHistoryRecords`'s field docs (`crates/reify-ir/src/geometry.rs`)
//!   state its counters are "always 0 for well-formed profiles" / "for
//!   vanilla sweep operations this should be zero". Independently,
//!   transform ops (translate/rotate/scale) do not forward topology
//!   attributes onto their result handle (confirmed empirically: the
//!   `TopologyAttributeTable` after `rotate(box(...), ...)` is byte-for-byte
//!   identical, same handle ids included, to the table after a bare
//!   `box(...)` — the wrapping rotate contributes nothing), and every
//!   origin-centred primitive combination is geometrically incapable of a
//!   genuine DISTINCT-index centroid coincidence (only equal-index
//!   coincidences are possible without a transform breaking the symmetry,
//!   and equal-index is exactly what L1's guard suppresses) — so a
//!   real-OCCT engine-level positive control for case (B) is not
//!   constructible with the primitives/transforms available today. Routing
//!   through the mock keeps the assertion deterministic and CI-stable
//!   while still proving the L2 gate's on/off boundary honestly: the module
//!   is compiled from real DSL source (not a hand-built `CompiledModule`),
//!   so `let fs = faces(u)` genuinely populates the module's `values`
//!   `ValueMap` with a `Value::Selector` entry — the exact signal
//!   `values_contain_selector` inspects.
//!
//! Self-skips (case A only; case B needs no OCCT) when
//! `reify_kernel_occt::OCCT_AVAILABLE` is false, mirroring the other
//! OCCT-gated e2e suites in this crate.

use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_eval::Engine;
use reify_ir::{
    AttributeHistory, BooleanOpHistoryRecords, ExportError, ExportFormat, GeometryError,
    GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh,
    QueryError, TessError, Value,
};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel};

// ─── shared engine-build helpers (mirrors topology_attribute_e2e.rs) ────────

fn compile_no_errors_for_engine(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test_topology_denoise_e2e"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

fn engine_with_occt_handle() -> Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())))
}

/// True iff `diagnostics` contains a `Severity::Warning` entry carrying
/// either of task #5196's two topology-bookkeeping codes.
fn has_warning_topology_diagnostic(diagnostics: &[reify_core::Diagnostic]) -> bool {
    diagnostics.iter().any(|d| {
        d.severity == Severity::Warning
            && matches!(
                d.code,
                Some(DiagnosticCode::TopologyCorrespondenceDropped)
                    | Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned)
            )
    })
}

// ─── DropInjectingKernel (copied from topology_correspondence_drop_diagnostic_e2e.rs, task #4545) ──
//
// Each `tests/*.rs` file compiles as an independent integration-test binary,
// so this small mock cannot be imported across files and is duplicated here
// verbatim rather than factored into `reify_test_support` (out of scope for
// this task — see reuse item in `.task/plan.json`).

/// Mock `GeometryKernel` that wraps `MockGeometryKernel` and overrides
/// `execute_with_history` to inject synthetic `AttributeHistory` records
/// carrying non-zero drop counters, for every `Union`/`Difference`/
/// `Intersection` op.
struct DropInjectingKernel {
    inner: MockGeometryKernel,
    boolean_history: BooleanOpHistoryRecords,
}

impl DropInjectingKernel {
    fn new(boolean_history: BooleanOpHistoryRecords) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            boolean_history,
        }
    }
}

impl GeometryKernel for DropInjectingKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let handle = self.inner.execute(op)?;
        let history = match op {
            GeometryOp::Union { .. }
            | GeometryOp::Difference { .. }
            | GeometryOp::Intersection { .. } => {
                AttributeHistory::Boolean(self.boolean_history.clone())
            }
            _ => AttributeHistory::None,
        };
        Ok((handle, history))
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
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

// ─── Case (A): healthy, selector-free multi-boolean model ──────────────────

/// A healthy, selector-free multi-boolean model — chaining several
/// `union(box, box)` ops, mirroring the litter-tray dogfooding example that
/// motivated task #5196 — must emit ZERO diagnostics carrying either
/// topology-bookkeeping code **at any severity**, and no `Severity::Error`.
///
/// Every union here is between coincident (fully-overlapping, equal-index)
/// boxes, exactly the shape the L1 guard targets: before task #5196 this
/// fixture emitted ~dozens of spurious `TopologyAttributeLocalIndexReassigned`
/// warnings (one per tied face/edge/vertex pair per union). Real OCCT,
/// self-skips without it.
///
/// The primary assertions filter by CODE ONLY, never by severity — see the
/// module doc for why a `severity == Severity::Warning` filter is vacuous
/// post-L3 and cannot fail. Both zero-counts below are EMPIRICALLY observed,
/// not assumed: this build emits an entirely empty diagnostics list under
/// real OCCT. The `TopologyCorrespondenceDropped` zero-count is additionally
/// backed by contract — `reify-kernel-occt/tests/boolean_op_history_integration.rs`
/// pins `silent_drop_count == 0` for well-formed OCCT boolean fixtures, so a
/// drop line here would signal genuine kernel-correspondence degradation on
/// healthy geometry, which is exactly what this case exists to catch.
#[test]
fn healthy_selector_free_multi_boolean_model_emits_no_topology_bookkeeping_diagnostics() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors_for_engine(
        r#"structure S {
    let a = union(box(20mm, 20mm, 20mm), box(20mm, 20mm, 20mm))
    let b = union(a, box(20mm, 20mm, 20mm))
    let c = union(b, box(20mm, 20mm, 20mm))
}"#,
    );
    let mut engine = engine_with_occt_handle();
    let build_result = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "healthy multi-boolean build must not regress to Failed: {:#?}",
        errors
    );

    // PRIMARY (severity-agnostic, and therefore able to fail): the L1 guard +
    // L2 gate must leave zero tie diagnostics on this healthy fixture.
    let tie_diags: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned))
        .collect();
    assert!(
        tie_diags.is_empty(),
        "expected ZERO TopologyAttributeLocalIndexReassigned diagnostics AT ANY SEVERITY on a \
         healthy, selector-free multi-boolean model (every tie this fixture can produce is \
         equal-index, and the model binds no selector so the tie-scan is gated off entirely); \
         got:\n{:#?}",
        tie_diags
    );

    // PRIMARY (severity-agnostic): empirically observed to be zero here, and
    // contract-backed by the OCCT boolean-history integration fixture.
    let drop_diags: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped))
        .collect();
    assert!(
        drop_diags.is_empty(),
        "expected ZERO TopologyCorrespondenceDropped diagnostics AT ANY SEVERITY on a healthy, \
         well-formed multi-boolean model (real OCCT reports silent_drop_count == 0 for such \
         fixtures); got:\n{:#?}",
        drop_diags
    );

    // SECONDARY, global severity contract only: nothing anywhere in the build
    // may re-emit either code at Warning severity after L3's downgrade. This
    // cannot fail while the two code-only assertions above hold — it is kept
    // as a cheap tripwire for a future third emit site, NOT as this case's
    // signal.
    assert!(
        !has_warning_topology_diagnostic(&build_result.diagnostics),
        "expected ZERO Warning-severity topology-bookkeeping diagnostics anywhere in the build \
         (task #5196 L3 downgraded both codes to Info); got:\n{:#?}",
        build_result.diagnostics
    );
}

// ─── Case (B): selector-bound boolean with degraded correspondence ─────────

/// A model that binds a topology selector over a chain of booleans whose
/// fuse degrades correspondence (injected via `DropInjectingKernel`, see
/// module doc for why real OCCT cannot serve this role) must still surface
/// ≥1 `Severity::Info` diagnostic carrying one of the two topology-
/// bookkeeping codes, with at most one aggregated silent-drop line for the
/// single realization (L4 summarization) — proving the L2 selector-presence
/// gate does not suppress genuine correspondence-degradation signal when a
/// selector is actually bound.
#[test]
fn selector_bound_boolean_with_degraded_correspondence_emits_info_diagnostic() {
    const DROP_COUNT_PER_OP: u32 = 4;

    let compiled = compile_no_errors_for_engine(
        r#"structure S {
    let u = union(union(box(10mm, 10mm, 10mm), box(5mm, 5mm, 5mm)), box(3mm, 3mm, 3mm))
    let fs = faces(u)
}"#,
    );

    let kernel = DropInjectingKernel::new(BooleanOpHistoryRecords {
        silent_drop_count: DROP_COUNT_PER_OP,
        ..Default::default()
    });
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), Some(Box::new(kernel)));
    let build_result = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "selector-bound boolean build must not regress to Failed: {:#?}",
        errors
    );

    let info_topology_diags: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Info
                && matches!(
                    d.code,
                    Some(DiagnosticCode::TopologyCorrespondenceDropped)
                        | Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned)
                )
        })
        .collect();
    assert!(
        !info_topology_diags.is_empty(),
        "expected >=1 Info-severity topology-bookkeeping diagnostic for a selector-bound \
         boolean with degraded correspondence; got:\n{:#?}",
        build_result.diagnostics
    );

    let drop_diags: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped))
        .collect();
    assert!(
        drop_diags.len() <= 1,
        "expected at most one aggregated silent-drop diagnostic for the single realization \
         (task #5196 L4 summarization), got {}: {:#?}",
        drop_diags.len(),
        drop_diags
    );
}
