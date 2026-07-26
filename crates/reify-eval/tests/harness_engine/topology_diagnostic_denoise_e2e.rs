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
//! - (B) A TWO-SIDED control for the L2 gate, plus the L4 contract.
//!
//!   A model that binds a topology selector over a boolean must (i) RUN the
//!   gated tie-scan block, and (ii) still surface real correspondence-
//!   degradation signal at `Severity::Info`, with at most one aggregated
//!   silent-drop line per realization (L4). Its NEGATIVE TWIN — the same
//!   source with only the `let fs = faces(u)` line removed — must NOT run
//!   the gated block, and must still emit the aggregated silent-drop line.
//!
//!   (ii) on its own proves NOTHING about the gate, and originally it was the
//!   whole of case (B): `TopologyCorrespondenceDropped` comes from the
//!   UNGATED tally/flush path, so it fires identically whether the gate opens
//!   or closes. The gate-ran marker in (i) is the centroid pre-pass's failure
//!   summary, whose sole call site sits inside the gated block — see
//!   `has_gated_tie_scan_marker`. Only the PAIR pins the gate: stubbing the
//!   gate condition to a constant `false` fails the positive half, stubbing
//!   it to a constant `true` fails the twin. Both directions were run and
//!   observed to fail at step-14/step-15 time.
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
//!   real-OCCT engine-level positive control via a genuine DISTINCT-index
//!   tie is not constructible with the primitives/transforms available
//!   today. Routing through the mock keeps the assertion deterministic and
//!   CI-stable, and — because the mock answers no centroid query — it is
//!   what makes the gate-ran marker in (i) fire at all.
//!
//!   On the `ValueMap`: this module is compiled from real DSL source (never a
//!   hand-built `CompiledModule`), and the real evaluator does land a
//!   `Value::Selector` in `values` for an UNCONSUMED `let fs = faces(u)` —
//!   directly observed by `values_contain_selector_true_for_evaluated_
//!   unconsumed_selector_let` in `engine_build/tests.rs` (step-11c). But that
//!   is no longer what carries the gate, and the earlier version of this doc
//!   overstated it: post-step-12 the gate's PRIMARY term is the module-STATIC
//!   `module_binds_selector` walk over every `CompiledExpr` in the module,
//!   with `values_contain_selector` demoted to a secondary belt-and-braces
//!   term. The runtime probe does not generalize — a selector ctor passed
//!   inline as a call argument (`fillet(b, edges(b), 2mm)`, the dominant
//!   idiom) never reaches the `ValueMap` at all, and a selector cell that a
//!   realization CONSUMES is hydrated to `Value::List<Geometry>` rather than
//!   staying a `Value::Selector`. `faces(u)` is left deliberately unconsumed
//!   in these fixtures, so both terms are true here; the gate would still
//!   open on the static term alone.
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

// ─── L2 gate-ran marker (task #5196 step-14) ────────────────────────────────

/// True iff the task #5196 L2-gated block ACTUALLY RAN for at least one
/// realization of this build.
///
/// The marker is the centroid pre-pass's failure summary
/// (`collect_centroids_with_failure_summary`, `engine_build.rs`). That helper
/// has exactly ONE call site in the whole crate and it sits INSIDE the gated
/// block, so its diagnostics cannot be produced by any ungated path — unlike
/// `TopologyCorrespondenceDropped`, which the always-on tally/flush path emits
/// regardless of the gate and which therefore proves nothing about it.
///
/// Under a mock kernel with no registered query fixtures every centroid /
/// bounding-box query fails, so the summary fires deterministically whenever
/// the gate opens over a non-empty attribute set — no genuine geometric tie
/// (which real OCCT cannot produce on demand, see the module doc) is needed.
///
/// Matched on message text because these two diagnostics deliberately carry
/// `code == None`: they report an auxiliary-metadata query failure, not one of
/// the two user-facing topology-bookkeeping codes.
fn has_gated_tie_scan_marker(diagnostics: &[reify_core::Diagnostic]) -> bool {
    diagnostics.iter().any(|d| {
        d.message
            .starts_with("topology-attribute centroid query failed")
            || d.message
                .starts_with("topology-attribute centroid parse failed")
    })
}

/// Highest op-result handle id the fixtures below can allocate.
/// `MockGeometryKernel` hands out `GeometryHandleId(1)`, `(2)`, … in
/// `execute` order; the case-(B)/(C) fixture issues five ops (three spheres,
/// two unions). Fixtures are registered for a generous superset so a future
/// op-count change cannot silently un-seed the table and turn the positive
/// control vacuous.
const MAX_OP_HANDLE_ID: u64 = 16;

/// A `MockGeometryKernel` pre-loaded with topology-extraction fixtures for
/// every handle id an op in these fixtures can be assigned.
///
/// Without this, `seed_primitive_attributes_for_handle` fails at
/// `kernel.extract_faces(...)`, the `TopologyAttributeTable` stays empty for
/// the realization, and the gated block's inner `if !realization_attrs
/// .is_empty()` short-circuits — making the gate UNOBSERVABLE at engine level.
/// (That is exactly why the original case (B) could assert nothing about it.)
///
/// `sphere` is the deliberate primitive choice: its seeding arm reads only
/// `extract_faces` / `extract_edges` and never queries the kernel, whereas the
/// `Box` arm issues a `GeometryQuery::BoundingBox` per vertex — which a mock
/// with no query fixtures fails, aborting seeding before any entry lands.
///
/// Face handle ids are offset well past `MAX_OP_HANDLE_ID` so a face id can
/// never collide with an op-result id.
fn seeding_mock_kernel() -> MockGeometryKernel {
    let mut kernel = MockGeometryKernel::new();
    for parent in 1..=MAX_OP_HANDLE_ID {
        kernel = kernel
            .with_extracted_faces(
                GeometryHandleId(parent),
                vec![GeometryHandleId(1000 + parent)],
            )
            .with_extracted_edges(GeometryHandleId(parent), vec![]);
    }
    kernel
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
    /// Task #5196 step-14 delta from the #4545 original: the inner mock is
    /// supplied by the caller rather than default-constructed, so these
    /// fixtures can register the topology-extraction results that
    /// `seed_primitive_attributes_for_handle` needs (see
    /// [`seeding_mock_kernel`]).
    fn new(inner: MockGeometryKernel, boolean_history: BooleanOpHistoryRecords) -> Self {
        Self {
            inner,
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

    // Task #5196 step-14 delta from the #4545 original: forward topology
    // extraction to the inner mock. Without these three, the trait's default
    // impls answer "topology extraction not supported by this kernel", primitive
    // attribute seeding fails, and the gated tie-scan's attribute set is empty —
    // which makes the L2 gate unobservable at engine level.
    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_faces(handle)
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_edges(handle)
    }

    fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_vertices(handle)
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

/// Number of silent drops `DropInjectingKernel` injects per boolean op in the
/// case-(B) fixtures.
const DROP_COUNT_PER_OP: u32 = 4;

/// The case-(B) selector-BOUND source. `sphere` rather than `box` because the
/// sphere seeding arm never queries the kernel — see [`seeding_mock_kernel`].
const SELECTOR_BOUND_SOURCE: &str = r#"structure S {
    let u = union(union(sphere(10mm), sphere(5mm)), sphere(3mm))
    let fs = faces(u)
}"#;

/// The case-(B) NEGATIVE TWIN: byte-for-byte the same as
/// [`SELECTOR_BOUND_SOURCE`] with the single `let fs = faces(u)` line removed,
/// and nothing else changed. Any other difference would confound the
/// comparison.
const SELECTOR_FREE_TWIN_SOURCE: &str = r#"structure S {
    let u = union(union(sphere(10mm), sphere(5mm)), sphere(3mm))
}"#;

/// Builds `source` against a `DropInjectingKernel` whose inner mock is
/// pre-seeded with topology-extraction fixtures.
fn build_with_drop_injecting_kernel(source: &str) -> reify_eval::BuildResult {
    let compiled = compile_no_errors_for_engine(source);
    let kernel = DropInjectingKernel::new(
        seeding_mock_kernel(),
        BooleanOpHistoryRecords {
            silent_drop_count: DROP_COUNT_PER_OP,
            ..Default::default()
        },
    );
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), Some(Box::new(kernel)));
    engine.build(&compiled, ExportFormat::Step)
}

/// A model that binds a topology selector over a chain of booleans whose
/// fuse degrades correspondence (injected via `DropInjectingKernel`, see
/// module doc for why real OCCT cannot serve this role) must:
///
///   1. RUN the L2-gated tie-scan block — the POSITIVE half of the gate's
///      two-sided control (see [`has_gated_tie_scan_marker`] for why the
///      centroid pre-pass failure summary is a sound gate-ran marker and the
///      drop diagnostic is not). Its negative twin is
///      `selector_free_twin_does_not_run_the_gated_tie_scan` below.
///   2. still surface ≥1 `Severity::Info` diagnostic carrying one of the two
///      topology-bookkeeping codes, with at most one aggregated silent-drop
///      line for the single realization (L4 summarization) — the always-on
///      tally/flush path must stay independent of the gate.
///
/// Assertion (2) alone proves NOTHING about the gate: `TopologyCorrespondence
/// Dropped` is emitted by the UNGATED tally/flush path, so it fires whether
/// the gate opens or not. That was this test's original defect — it claimed to
/// prove the gate did not suppress genuine signal while asserting only a
/// gate-independent fact. Assertion (1) is what actually pins the gate, and
/// only as a PAIR with the negative twin: with both, stubbing the gate
/// condition to a constant `false` fails this test and stubbing it to a
/// constant `true` fails the twin.
#[test]
fn selector_bound_boolean_with_degraded_correspondence_emits_info_diagnostic() {
    let build_result = build_with_drop_injecting_kernel(SELECTOR_BOUND_SOURCE);

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

    // (1) POSITIVE CONTROL for the L2 gate's TRUE branch.
    assert!(
        has_gated_tie_scan_marker(&build_result.diagnostics),
        "the task #5196 L2-gated tie-scan block must RUN for a module that binds a selector \
         (`let fs = faces(u)`); its centroid pre-pass emits a failure summary under the mock \
         kernel, and no such diagnostic is reachable from any ungated path. Got:\n{:#?}",
        build_result.diagnostics
    );

    // (2) The ungated tally/flush path must still surface genuine signal.
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

/// NEGATIVE TWIN of the case-(B) positive control: the same fixture with the
/// `let fs = faces(u)` binding removed must NOT run the gated tie-scan block.
///
/// This is the half that makes the pair two-sided. Without it, a gate stubbed
/// to a constant `true` would leave the whole suite green — which is precisely
/// the defect this step repairs.
///
/// It also pins the L4 tally/flush path's INDEPENDENCE from the gate from the
/// other side: the silent-drop diagnostic must still fire here, with the gate
/// closed.
#[test]
fn selector_free_twin_does_not_run_the_gated_tie_scan() {
    let build_result = build_with_drop_injecting_kernel(SELECTOR_FREE_TWIN_SOURCE);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "selector-free twin build must not regress to Failed: {:#?}",
        errors
    );

    assert!(
        !has_gated_tie_scan_marker(&build_result.diagnostics),
        "the task #5196 L2-gated tie-scan block must be SKIPPED for a module that binds no \
         selector — the only textual difference from the positive control above is the removed \
         `let fs = faces(u)` line. Got:\n{:#?}",
        build_result.diagnostics
    );

    // Gate-independence of the L4 path, asserted from the closed-gate side:
    // the drop tally must still flush its one aggregated line.
    let drop_diags: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped))
        .collect();
    assert_eq!(
        drop_diags.len(),
        1,
        "the ungated silent-drop tally must still emit exactly one aggregated line with the \
         gate CLOSED; got:\n{:#?}",
        build_result.diagnostics
    );
}
