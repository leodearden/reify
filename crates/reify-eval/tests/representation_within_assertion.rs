//! Engine-level tests for the `RepresentationWithin` assertion dispatch
//! interception (Determinacy γ, task-4199).
//!
//! # Non-OCCT tests (step-5 / step-6)
//!
//! Verify that `Engine::dispatch_constraints` correctly intercepts
//! `RepresentationWithin` constraint expressions, evaluates them against
//! `self.achieved_repr_tol` (injected via a test-instrumentation setter), and
//! weaves results back in caller (input) order.
//!
//! These tests use a non-kernel engine (no OCCT) so that the full pipeline
//! can be exercised in CI without a geometry backend.  The
//! `set_achieved_repr_tol_for_test` setter is the test-instrumentation seam
//! added alongside `set_capture_repr_tol` (engine_admin.rs).
//!
//! # OCCT-gated tests (step-7 / step-8)
//!
//! End-to-end tests that use a real OCCT kernel to tessellate curved geometry
//! and verify the full dispatch-interception + tessellation pipeline.
//! All OCCT-gated tests skip cleanly when OCCT is not available (stub mode).
//!
//! Pipeline under test: `set_capture_repr_tol(true)` →
//! `tessellate_realizations(&compiled)` → `check(&compiled)`.
//! The `tessellate_realizations` call populates `achieved_repr_tol`;
//! `check` calls `eval` (which does NOT clear the map) then
//! `dispatch_constraints`, which intercepts `RepresentationWithin` entries
//! and reads from the populated map.

use reify_core::ConstraintNodeId;
use reify_core::{ContentHash, DiagnosticCode, DimensionVector, Severity, Type};
use reify_eval::graph::ConstraintNodeData;
use reify_eval::tolerance_combine::extract_output_tolerance_bound;
use reify_ir::{CompiledExpr, PersistentMap, Satisfaction};
use reify_test_support::{make_simple_engine, parse_and_compile};
use std::collections::BTreeMap;

// ── Shared DSL fixture ────────────────────────────────────────────────────────

/// A module with two constraints in the **same** template (`Checker`):
///
/// - Constraint index 0: `RepresentationWithin(subject, 1mm)` — the assertion.
///   Bound is `1mm = 1e-3 m` (built-in unit, no stdlib required).
/// - Constraint index 1: `w > 0.0` — an ordinary always-`Satisfied` predicate.
///
/// `MyGeom` supplies the named structure type for `subject`; it has no
/// geometry (non-kernel engine) so `subject.self` is Undef at eval time.
/// The type-name scan fallback in `eval_representation_within` resolves
/// the achieved-tol key from the struct name `"MyGeom"` → key
/// `"MyGeom#realization[0]"` in the injected map.
///
/// Both constraints live in the **same** template so they pass through a
/// **single** `dispatch_constraints` call — this exercises within-batch order
/// preservation when the interception peels constraint 0 and leaves
/// constraint 1 for the language-level checker.
///
/// Note: `mm` is a built-in length unit available without stdlib; `um`
/// (micrometer) requires stdlib and is intentionally avoided here so that
/// `parse_and_compile` (no stdlib) can be used.
const INTERCEPTION_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

// Checker carries BOTH a RepresentationWithin assertion (constraint index 0)
// AND an ordinary always-satisfied constraint (index 1) in a single template.
// Placing both constraints here exercises the within-batch order-preservation
// invariant of dispatch_constraints: the engine-side result for index 0 must
// appear before the checker-side result for index 1 in the returned list.
structure Checker {
    param subject : MyGeom
    param w : Real = 5.0
    constraint RepresentationWithin(subject, 1mm)
    constraint w > 0.0
}
"#;

/// A module whose `RepresentationWithin` subject param carries a **default**
/// (`= MyGeom()`), so every operand is genuinely *defined* at check time.
///
/// That default is load-bearing for the non-measuring-surface tests below.
/// Without it (as in [`INTERCEPTION_SOURCE`]) the language-level checker's
/// reason for an Indeterminate is the correctly-attributed `undefined inputs:
/// Checker.subject`; only a DEFINED `StructureInstance` operand reaches the
/// `classify_undef` branch that emits the *misattributed* `operator undefined
/// for these operand kinds: StructureInstance` — which is exactly the message
/// task ζ (C-SURFACE 1) exists to eliminate.
///
/// A single constraint, so the whole batch is RepresentationWithin.
///
/// `mm` is a built-in length unit — no stdlib needed (matching
/// `INTERCEPTION_SOURCE`'s style; `um` would require stdlib).
const NON_MEASURING_SURFACE_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom = MyGeom()
    constraint RepresentationWithin(subject, 1mm)
}
"#;

// ── ζ / C-SURFACE 1: non-measuring surface ───────────────────────────────────

/// C-SURFACE (1): a `RepresentationWithin` shape must never fall through to the
/// language-level `ConstraintChecker` on a surface that never measured.
///
/// A fresh `make_simple_engine()` has an EMPTY `achieved_repr_tol` and
/// `capture_repr_tol: false` — i.e. it *is* the non-measuring `reify build`
/// surface, so no injection seam is needed here.
///
/// RED before ζ's guard reorder: `dispatch_constraints`' two-conjunct fast-path
/// guard fires (map empty, registry empty) and early-returns to
/// `SimpleConstraintChecker`, which emits the misattributed
/// `operator undefined for these operand kinds: StructureInstance`.
#[test]
fn non_measuring_surface_does_not_reach_language_checker() {
    let compiled = parse_and_compile(NON_MEASURING_SURFACE_SOURCE);
    // A fresh engine already has an empty `achieved_repr_tol` — deliberately no
    // `set_achieved_repr_tol_for_test` call here.
    let mut engine = make_simple_engine();

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "C1: a surface that never measured yields Indeterminate — never a false Violated"
    );

    let offenders: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message
                .contains("operator undefined for these operand kinds")
        })
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        offenders.is_empty(),
        "C-SURFACE 1: a RepresentationWithin assertion must not reach the \
         language-level ConstraintChecker on a non-measuring surface — the checker \
         has no access to achieved_repr_tol and misattributes the Indeterminate to \
         the operand kinds. Offending diagnostics: {offenders:#?}"
    );
}

/// INV-SF-4 + INV-SF-6: the Indeterminate must be ATTRIBUTABLE and CODED.
///
/// Removing the misattributed reason is only half the contract — a bare,
/// unexplained INDETERMINATE is an INV-SF-4 violation of its own flavour. The
/// surface that could not answer must say so, and point at the one that can.
///
/// The message assertions deliberately pin only the load-bearing tokens
/// ("does not measure", "reify check") rather than the full sentence, so a
/// later rewording pass does not break this gate.
///
/// RED after the guard reorder: nothing yet pushes a diagnostic onto the peel's
/// Indeterminate result.
#[test]
fn non_measuring_surface_yields_attributable_indeterminate() {
    let compiled = parse_and_compile(NON_MEASURING_SURFACE_SOURCE);
    let mut engine = make_simple_engine();

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "C1: a surface that never measured yields Indeterminate — never a false Violated"
    );

    // The two halves of the contract are pinned together: the misattributed
    // reason must be gone AND an attributable one must be present.
    let offenders: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message
                .contains("operator undefined for these operand kinds")
        })
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        offenders.is_empty(),
        "C-SURFACE 1: the language-level checker must not answer for this \
         constraint. Offending diagnostics: {offenders:#?}"
    );

    let attributions: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("Checker#constraint[0]"))
        .collect();
    assert_eq!(
        attributions.len(),
        1,
        "INV-SF-4: exactly one diagnostic must name Checker#constraint[0] and \
         explain the Indeterminate. Got: {:#?}",
        result.diagnostics
    );
    let attribution = attributions[0];

    assert_eq!(
        attribution.severity,
        Severity::Info,
        "C-LOG emission-table row 5: severity is Info — not Warning, and (per \
         INV-SF-2's severity-hygiene corollary) never Error on a path a healthy \
         design routinely hits. Got: {attribution:#?}"
    );
    assert_eq!(
        attribution.code,
        Some(DiagnosticCode::ConstraintIndeterminate),
        "INV-SF-6: the diagnostic must carry a machine-readable code. Got: {attribution:#?}"
    );
    assert!(
        attribution.message.contains("does not measure"),
        "INV-SF-4: the message must name the SURFACE as the reason, not the \
         operands. Got: {:?}",
        attribution.message
    );
    assert!(
        attribution.message.contains("reify check"),
        "INV-SF-4: the message must point at the remedy — the surface that does \
         measure. Got: {:?}",
        attribution.message
    );
}

/// INV-SF-4: when a measurement *was* requested and no geometry kernel exists
/// to make it, the remedy must be the kernel — NOT "run `reify check`".
///
/// An empty `achieved_repr_tol` has THREE causes, and the engine already
/// carries a discriminator for each:
/// - capture OFF — nobody asked (`reify build` / `reify eval`); remedy is
///   `reify check` (pinned by
///   [`non_measuring_surface_yields_attributable_indeterminate`]).
/// - capture ON, `default_kernel_name` is `None` — `cmd_check` asked (it sets
///   the flag whenever the module carries a `RepresentationWithin`) but this
///   binary registered no geometry kernel at all (stub-mode `reify check`).
///   Telling *that* user to run `reify check` is a dead end, so the remedy
///   names the kernel instead. **That is the arm this test isolates.**
/// - capture ON, a kernel IS registered, still nothing measured — the remedy
///   must not blame the kernel; pinned by the complementary
///   [`kernel_present_but_nothing_tessellated_does_not_blame_the_kernel`].
///
/// The two kernel-arm tests bracket the discriminator: same capture flag, same
/// empty map, opposite kernel presence, opposite remedy.
///
/// The source must genuinely REALIZE geometry so that the absent kernel is the
/// ONLY reason the map is empty. [`NON_MEASURING_SURFACE_SOURCE`] cannot serve
/// here: it declares no realization, so on `make_simple_engine()` (which is
/// `Engine::new(checker, None)` — no kernel) BOTH causes hold at once and the
/// test could not discriminate between them, which is precisely why an earlier
/// revision of this gate passed for the wrong reason and locked the kernel
/// wording in rather than gating it. [`OCCT_SOURCE_COARSE`] realizes a sphere,
/// so with no kernel behind it only the missing-kernel cause remains.
///
/// The kernel branch also keeps the pre-`check()` pass inside
/// `tessellate_realizations` — which runs before the map is populated, on a
/// surface that genuinely does measure — from minting a self-contradictory
/// "run `reify check`" diagnostic.
///
/// The emission *gate* is unchanged: still `Indeterminate && map.is_empty()`.
/// `capture_repr_tol` and `default_kernel_name` select only the remedy clause.
#[test]
fn measurement_requested_but_unmeasured_points_at_the_kernel_not_at_check() {
    // Geometry-bearing source: a realization exists to tessellate, so the
    // absent kernel is the only remaining reason the map stays empty.
    let compiled = compile_no_errors(OCCT_SOURCE_COARSE, "kernel_absent");
    // `Engine::new(checker, None)` ⇒ `default_kernel_name` is `None`.
    let mut engine = make_simple_engine();

    // Exactly what `cmd_check` does for a module carrying a
    // RepresentationWithin; with no kernel behind it, the map stays empty.
    engine.set_capture_repr_tol(true);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "C1: no measurement ⇒ Indeterminate, whoever asked for it"
    );

    let attributions: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("SphereCheck#constraint[0]"))
        .collect();
    assert_eq!(
        attributions.len(),
        1,
        "INV-SF-4: exactly one diagnostic must name the constraint. Got: {:#?}",
        result.diagnostics
    );
    let attribution = attributions[0];

    assert_eq!(
        attribution.severity,
        Severity::Info,
        "severity is unaffected by which remedy applies. Got: {attribution:#?}"
    );
    assert_eq!(
        attribution.code,
        Some(DiagnosticCode::ConstraintIndeterminate),
        "INV-SF-6: coded on this branch too. Got: {attribution:#?}"
    );
    assert!(
        attribution.message.contains("does not measure"),
        "the surface is still the reason — that token is stable across both \
         remedies. Got: {:?}",
        attribution.message
    );
    assert!(
        attribution.message.contains("geometry kernel"),
        "INV-SF-4: a run that ASKED for the measurement must be told what was \
         actually missing. Got: {:?}",
        attribution.message
    );
    assert!(
        !attribution.message.contains("reify check"),
        "pointing a `reify check` run back at `reify check` is a dead end — that \
         remedy belongs to the capture-OFF branch only. Got: {:?}",
        attribution.message
    );
}

/// Minimal stub `GeometryKernel` whose only job is to make
/// `default_kernel_name` be `Some(..)` — i.e. "a geometry kernel IS
/// registered" — without requiring OCCT.
///
/// None of these bodies is ever reached: the sources used with it declare no
/// realization, so no geometry op is ever dispatched.  Mirrors the equivalent
/// stub in `crates/reify-eval/tests/morph_producer_seam.rs`.
///
/// Using a stub rather than a real OCCT handle is deliberate: it makes
/// [`kernel_present_but_nothing_tessellated_does_not_blame_the_kernel`]
/// OCCT-INDEPENDENT, so it gates in stub-mode CI too — where the original
/// OCCT-binary reproduction of this defect cannot run at all.
struct NoRealizationStubKernel;

impl reify_ir::GeometryKernel for NoRealizationStubKernel {
    fn execute(
        &mut self,
        _op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        Err(reify_ir::GeometryError::OperationFailed("stub".into()))
    }
    fn query(&self, _q: &reify_ir::GeometryQuery) -> Result<reify_ir::Value, reify_ir::QueryError> {
        Err(reify_ir::QueryError::QueryFailed("stub".into()))
    }
    fn export(
        &self,
        _h: reify_ir::GeometryHandleId,
        _f: reify_ir::ExportFormat,
        _w: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        Err(reify_ir::ExportError::FormatError("stub".into()))
    }
    fn tessellate(
        &self,
        _h: reify_ir::GeometryHandleId,
        _t: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        Err(reify_ir::TessError::TessellationFailed("stub".into()))
    }
}

/// INV-SF-4: a run that asked for the measurement while a geometry kernel IS
/// present must not claim a kernel is missing.
///
/// `capture_repr_tol == true && achieved_repr_tol.is_empty()` has more than
/// one cause, and "no geometry kernel" is only one of them.  The other — a
/// kernel is present but the subject simply has no realization to tessellate —
/// is trivially reachable, and answering it with "build with OCCT" on a binary
/// where OCCT is demonstrably live is a false statement and a dead end: the
/// same INV-SF-4 misattribution class ζ exists to remove, merely relocated
/// from the operand kinds to the kernel.
///
/// The engine already carries the discriminator: `Engine::default_kernel_name`
/// is `Some(..)` iff a kernel is registered (`with_prelude` maps
/// `Some(kernel)` → `Some(DEFAULT_KERNEL_NAME)`, engine_admin.rs).
///
/// This test isolates the KERNEL-PRESENT arm;
/// [`measurement_requested_but_unmeasured_points_at_the_kernel_not_at_check`]
/// isolates the complementary kernel-absent one.  Together they bracket the
/// discriminator: same capture flag, same empty map, opposite kernel presence,
/// opposite remedy.
///
/// RED before the three-way remedy split: the remedy branches on
/// `capture_repr_tol` alone, so this emits the kernel wording.
#[test]
fn kernel_present_but_nothing_tessellated_does_not_blame_the_kernel() {
    let compiled = parse_and_compile(NON_MEASURING_SURFACE_SOURCE);
    // `Some(..)` ⇒ `default_kernel_name = Some(DEFAULT_KERNEL_NAME)`: a kernel
    // IS registered.  The subject declares no realization, so nothing is ever
    // tessellated and the map stays empty for a reason that is NOT the kernel.
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(NoRealizationStubKernel)),
    );

    // Exactly what `cmd_check` does for a module carrying a
    // RepresentationWithin.
    engine.set_capture_repr_tol(true);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "C1: nothing was measured for this subject ⇒ Indeterminate"
    );

    let attributions: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("Checker#constraint[0]"))
        .collect();
    assert_eq!(
        attributions.len(),
        1,
        "INV-SF-4: exactly one diagnostic must name the constraint. Got: {:#?}",
        result.diagnostics
    );
    let attribution = attributions[0];

    assert_eq!(
        attribution.severity,
        Severity::Info,
        "severity is unaffected by which remedy applies. Got: {attribution:#?}"
    );
    assert_eq!(
        attribution.code,
        Some(DiagnosticCode::ConstraintIndeterminate),
        "INV-SF-6: coded on this branch too. Got: {attribution:#?}"
    );
    assert!(
        attribution.message.contains("does not measure"),
        "the surface is still the reason — that token is stable across every \
         remedy. Got: {:?}",
        attribution.message
    );
    assert!(
        !attribution.message.contains("OCCT"),
        "INV-SF-4: a kernel is demonstrably registered on this engine, so any \
         sentence telling the user to build with OCCT is the false claim under \
         test. Got: {:?}",
        attribution.message
    );
    assert!(
        !attribution.message.contains("kernel"),
        "INV-SF-4: with a kernel present the engine has NOT established that a \
         kernel is the problem, so the remedy must not mention one at all. \
         Got: {:?}",
        attribution.message
    );
}

/// A module with NO `RepresentationWithin` at all — the universal
/// non-assertion case that must keep taking the C2 fast path.
const NO_ASSERTION_SOURCE: &str = r#"
structure Plain {
    param w : Real = 5.0
    constraint w > 0.0
}
"#;

/// Returns the messages of every diagnostic carrying the attribution's
/// load-bearing tokens — the population the new Info is allowed to occupy.
fn attribution_diagnostics(result: &reify_eval::CheckResult) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("does not measure") || d.message.contains("reify check"))
        .map(|d| d.message.as_str())
        .collect()
}

/// Regression guard: a MEASURING surface whose subject key is simply
/// unresolvable keeps today's silent Indeterminate (C1).
///
/// This is the direct guard on "`reify check` output must be unchanged". An
/// implementation that attributed every Indeterminate — forgetting the
/// `achieved_repr_tol.is_empty()` gate — would fail here, because tessellation
/// demonstrably DID run on this surface; the map just holds no entry for this
/// subject, which is a different fact needing a different (existing) answer.
#[test]
fn measuring_surface_indeterminate_carries_no_attribution_diagnostic() {
    let compiled = parse_and_compile(NON_MEASURING_SURFACE_SOURCE);
    let mut engine = make_simple_engine();

    // Non-empty map (a tessellation ran) that does NOT resolve "MyGeom".
    engine.set_achieved_repr_tol_for_test(BTreeMap::from([(
        "Unrelated#realization[0]".to_string(),
        1e-9_f64,
    )]));

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "C1 unchanged: an unresolvable subject key is still Indeterminate"
    );

    let attributions = attribution_diagnostics(&result);
    assert!(
        attributions.is_empty(),
        "the surface-attribution diagnostic must NOT reach a surface that DID \
         measure — this one tessellated, it simply has no entry for this subject. \
         Attaching it here would change `reify check` output. Got: {attributions:#?}"
    );
}

/// Regression guard: a MEASURING surface with a resolvable, under-bound subject
/// is `Satisfied` and gains no attribution diagnostic.
///
/// Mirrors `dispatch_interception_under_bound_yields_satisfied` for the
/// defaulted-subject source, so the happy `reify check` path is pinned against
/// the new code too.
#[test]
fn measuring_surface_satisfied_is_unchanged() {
    let compiled = parse_and_compile(NON_MEASURING_SURFACE_SOURCE);
    let mut engine = make_simple_engine();

    // 1e-9 m ≪ 1mm = 1e-3 m bound → Satisfied.
    engine.set_achieved_repr_tol_for_test(BTreeMap::from([(
        "MyGeom#realization[0]".to_string(),
        1e-9_f64,
    )]));

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "achieved 1e-9 m < bound 1mm (1e-3 m) → Satisfied"
    );

    let attributions = attribution_diagnostics(&result);
    assert!(
        attributions.is_empty(),
        "a Satisfied verdict needs no explanation — the attribution diagnostic is \
         scoped to Indeterminate-because-nothing-measured. Got: {attributions:#?}"
    );
}

/// Regression guard: the universal non-assertion module still takes the C2 fast
/// path and stays silent.
///
/// This pins that the third conjunct added to the fast-path guard did not
/// change the outcome for a module with no `RepresentationWithin`.
///
/// The *zero-allocation* property of that conjunct is argued structurally, not
/// asserted here: `entries.iter().any(..)` allocates nothing, and
/// `match_representation_within_shape` rejects at Gate 2 — on `expr.kind` or on
/// the `function_name != "RepresentationWithin"` compare — strictly before its
/// only allocation. Gate cost is not wall-clock asserted, so this test pins the
/// observable behaviour and leaves the cost argument to that function's gates.
#[test]
fn non_assertion_module_hot_path_is_unchanged() {
    let compiled = parse_and_compile(NO_ASSERTION_SOURCE);
    let mut engine = make_simple_engine();

    let result = engine.check(&compiled);

    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Plain" && e.id.index == 0)
        .expect("must have Plain#constraint[0] (w > 0.0)");
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "w=5.0 > 0.0 → Satisfied, exactly as before the guard gained its third conjunct"
    );
    assert!(
        result.diagnostics.is_empty(),
        "C2: a module with no RepresentationWithin must emit no diagnostics at \
         all — the fast path is byte-identical. Got: {:#?}",
        result.diagnostics
    );
}

/// The MIXED batch on a non-measuring surface: a `RepresentationWithin` peeled
/// engine-side alongside an ordinary predicate routed to the language checker.
///
/// Before ζ this combination took the fast path wholesale (empty map, no
/// registered impl) and never reached the slot-weaving branch at all, so the
/// interleaving is newly exercised here and worth pinning: the ordinary
/// constraint must keep its verdict, results must stay in input order, and the
/// batch must gain exactly ONE attribution — for the assertion that could not be
/// evaluated, not for its blameless neighbour.
///
/// [`INTERCEPTION_SOURCE`] is used precisely because its `subject` has no
/// default: the peel must claim the constraint on shape alone, without waiting
/// for the operands to be defined.
#[test]
fn non_measuring_mixed_batch_preserves_order_and_attributes_once() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    // Fresh engine: empty `achieved_repr_tol`, `capture_repr_tol: false` — the
    // `reify build` surface, and deliberately no injection.
    let mut engine = make_simple_engine();

    let result = engine.check(&compiled);

    assert_eq!(
        result.constraint_results.len(),
        2,
        "Checker has 2 constraints (RepresentationWithin + w>0) → 2 results; got {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, e.satisfaction))
            .collect::<Vec<_>>()
    );

    let rw_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    let ord_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 1)
        .expect("must have Checker#constraint[1] (w > 0.0)");

    assert_eq!(
        result.constraint_results[rw_pos].satisfaction,
        Satisfaction::Indeterminate,
        "C1: nothing measured this subject → Indeterminate, never a false Violated"
    );
    assert_eq!(
        result.constraint_results[ord_pos].satisfaction,
        Satisfaction::Satisfied,
        "w=5.0 > 0.0 → Satisfied: peeling the assertion must not disturb the \
         ordinary constraint that stayed on the language-checker path"
    );
    assert!(
        rw_pos < ord_pos,
        "input order must survive the peel on this surface too: \
         RepresentationWithin (pos {rw_pos}) before the ordinary constraint \
         (pos {ord_pos})"
    );

    let attributions = attribution_diagnostics(&result);
    assert_eq!(
        attributions.len(),
        1,
        "exactly ONE attribution for the batch — the assertion that could not be \
         evaluated, not its neighbour. Got: {attributions:#?}"
    );
    assert!(
        attributions[0].contains("Checker#constraint[0]"),
        "INV-SF-4: the attribution must name the RepresentationWithin, not the \
         ordinary constraint. Got: {:?}",
        attributions[0]
    );

    let offenders: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message
                .contains("operator undefined for these operand kinds")
        })
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        offenders.is_empty(),
        "C-SURFACE 1 holds for the mixed batch too. Offending diagnostics: {offenders:#?}"
    );
}

// ── BT1: over-bound → Violated ────────────────────────────────────────────────

/// BT1: achieved value ABOVE the bound (5e-3 m > 1 mm = 1e-3 m) → `Violated`.
///
/// Also verifies:
/// - The ordinary constraint (`w > 0.0` with `w = 5.0`) is `Satisfied`.
/// - **Input-order preservation**: RepresentationWithin (constraint index 0)
///   appears before the ordinary constraint (index 1) in the result list,
///   proving `dispatch_constraints` weaves interception results back in the
///   original entry order.
///
/// RED until step-6 adds `set_achieved_repr_tol_for_test` and wires the
/// interception into `dispatch_constraints`.
#[test]
fn dispatch_interception_over_bound_yields_violated() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // Inject achieved_repr_tol via the test-instrumentation setter.
    // "MyGeom#realization[0]" = 5e-3 m > 1mm = 1e-3 m bound → must yield Violated.
    //
    // RED: `set_achieved_repr_tol_for_test` does not exist until step-6.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 5e-3_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    // Checker has two constraints → exactly 2 constraint results.
    assert_eq!(
        result.constraint_results.len(),
        2,
        "Checker has 2 constraints (RepresentationWithin + w>0) → 2 results; \
         got {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, e.satisfaction))
            .collect::<Vec<_>>()
    );

    // ── RepresentationWithin (entity="Checker", index=0) ──────────────────────
    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "BT1: achieved 5e-3 m > bound 1mm (1e-3 m) → Violated"
    );

    // ── Ordinary constraint (entity="Checker", index=1) ───────────────────────
    let ord_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 1)
        .expect("must have Checker#constraint[1] (w > 0.0)");
    assert_eq!(
        ord_entry.satisfaction,
        Satisfaction::Satisfied,
        "w=5.0 > 0.0 → Satisfied (ordinary constraint unaffected by interception)"
    );

    // ── Input-order preservation ───────────────────────────────────────────────
    // The RepresentationWithin result (index 0) must appear at a LOWER position
    // in the output list than the ordinary result (index 1), matching the order
    // of entries in the dispatch batch.
    let rw_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 0)
        .unwrap();
    let ord_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 1)
        .unwrap();
    assert!(
        rw_pos < ord_pos,
        "BT1: RepresentationWithin (pos {rw_pos}) must precede the ordinary \
         constraint (pos {ord_pos}) — dispatch_constraints must preserve \
         within-batch input order even when interleaving engine-side and \
         checker-side results"
    );
}

// ── BT2: under-bound → Satisfied ─────────────────────────────────────────────

/// BT2: achieved value BELOW the bound (1e-9 m ≪ 1mm = 1e-3 m) → `Satisfied`.
///
/// RED until step-6.
#[test]
fn dispatch_interception_under_bound_yields_satisfied() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // 1e-9 m ≪ 1mm = 1e-3 m bound → Satisfied.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 1e-9_f64);
    // RED: setter does not exist until step-6.
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "BT2: achieved 1e-9 m < bound 1mm (1e-3 m) → Satisfied"
    );
}

// ── BT3: no entry → Indeterminate ────────────────────────────────────────────

/// BT3: no entry in `achieved_repr_tol` for the subject → `Indeterminate`.
///
/// C1 invariant: absent key ⇒ realization not run ⇒ never a false `Violated`.
///
/// The map is deliberately non-empty but non-matching: this test is about a
/// MEASURING surface that simply holds no entry for *this* subject. An empty map
/// is a different fact — "this surface never measured at all" — and since
/// task-6169 ζ it routes through the surface-attribution branch instead; that
/// case is covered by
/// [`non_measuring_mixed_batch_preserves_order_and_attributes_once`].
///
/// RED until step-6.
#[test]
fn dispatch_interception_no_entry_yields_indeterminate() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // Non-empty map (a tessellation ran) with no key matching
    // "MyGeom#realization[*]" → Indeterminate.
    // RED: setter does not exist until step-6.
    engine.set_achieved_repr_tol_for_test(BTreeMap::from([(
        "Unrelated#realization[0]".to_string(),
        1e-9_f64,
    )]));

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "BT3 / C1: no achieved entry → Indeterminate (never a false Violated)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// OCCT-gated end-to-end tests (step-7 / step-8)
// ═══════════════════════════════════════════════════════════════════════════════

// ── OCCT helpers ─────────────────────────────────────────────────────────────

/// Build a fresh `Engine` backed by a real OCCT kernel, mirroring the
/// `make_occt_engine` helper in `achieved_repr_tol.rs`.
///
/// Uses `OcctKernelHandle` directly (not `SingleKernelHolder`) so that
/// `measure_mesh_deviation` is reachable through the `&dyn GeometryKernel`
/// vtable — `SingleKernelHolder` defaults most optional methods to `None`.
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Compile `source` (no stdlib) and assert no error-severity diagnostics.
fn compile_no_errors(source: &str, name: &str) -> reify_compiler::CompiledModule {
    use reify_core::{ModulePath, Severity};
    let parsed = reify_syntax::parse(source, ModulePath::single(name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {name}: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors in {name}: {:#?}", errors);
    compiled
}

// ── Shared OCCT DSL fixture ───────────────────────────────────────────────────

/// DSL for the OCCT end-to-end tests.
///
/// - `Sphere`: a 1 m-radius sphere realized at `#precision(50mm)` (COARSE).
///   At 50 mm deflection the sampled chord deviation is on the order of
///   centimetres — far above `1mm` (1e-3 m) → **Violated** under tight bound.
/// - `SphereCheck`: carries `RepresentationWithin(subject, 1mm)` — bound = 1e-3 m.
///
/// BT6 uses this source verbatim (coarse → Violated).
/// BT7 replaces `#precision(50mm)` with `#precision(0.3mm)` so deviation < 1mm.
/// BT8 uses this source but skips `tessellate_realizations` → Indeterminate.
/// C4 uses a variant with `0mm` bound.
///
/// `mm` is a built-in length unit — no stdlib needed.
const OCCT_SOURCE_COARSE: &str = r#"
#precision(50mm)
structure Sphere {
    let r = sphere(1000mm)
}
structure SphereCheck {
    param subject : Sphere
    constraint RepresentationWithin(subject, 1mm)
}
"#;

/// Fine-precision variant: `#precision(0.3mm)` — sampled deviation 6.202e-4 m,
/// below the `1mm` (1e-3 m) bound (1.61x inside) → used by BT7 to verify
/// `Satisfied`.
///
/// **Single source of truth for these numbers.** The other sites that depend on
/// them — `crates/reify-cli/tests/fixtures/representation_within_satisfied.ri`
/// and its gate `crates/reify-cli/tests/harness_cli/cli_determinacy_gate.rs` —
/// point here rather than restating the digits, so retuning this constant cannot
/// leave a stale copy behind. BT7's pre-condition below is the only
/// machine-checked copy.
///
/// Measured achieved facet-chord deviation on the 1 m sphere, by requested
/// deflection (each value reproduced on a second independent run):
///
/// ```text
///   0.10mm → 2.078e-4    0.35mm → 7.271e-4    0.49mm → 1.013e-3  VIOLATES
///   0.25mm → 5.198e-4    0.48mm → 9.988e-4    0.50mm → 3.807e-4  (off-trend)
///   0.30mm → 6.202e-4    (chosen)             0.51mm → 1.056e-3  VIOLATES
/// ```
///
/// Every point except 0.50mm sits at 2.067x-2.081x the requested deflection — a
/// 0.65% spread, i.e. very close to LINEAR. (An earlier revision of this doc
/// called the curve a "sawtooth" that swings ~3x between adjacent values; no
/// measurement here supports that, and it is corrected rather than repeated.)
/// The linear trend puts the 1e-3 m bound crossing at ~0.4815mm, which is what
/// makes 0.49mm and 0.51mm violate. 0.50mm is the one measured exception at
/// 0.76x; OCCT meshes each edge into an INTEGER segment count, so isolated steps
/// like it are possible and linearity is not guaranteed outside the measured
/// range — re-measure rather than extrapolate far. BT7's pre-condition carries
/// the recipe.
///
/// 0.3mm was chosen over finer values purely for wall-clock: it is ~3x cheaper to
/// tessellate than 0.1mm (measured 3.0x-3.75x across runs; the ratio moves with
/// machine load) and still passes, as do both its measured neighbours, so there
/// is no cliff within ±17%. The faster 0.48mm/0.50mm were rejected: 0.48mm clears
/// the bound by only 0.12%, and 0.50mm passes only as an isolated off-trend point
/// with violations on both sides.
const OCCT_SOURCE_FINE: &str = r#"
#precision(0.3mm)
structure Sphere {
    let r = sphere(1000mm)
}
structure SphereCheck {
    param subject : Sphere
    constraint RepresentationWithin(subject, 1mm)
}
"#;

/// Zero-bound variant: `RepresentationWithin(subject, 0mm)` — bound = 0.0 m.
/// With C4's zero-bound floor (PLANAR_FLOOR = 1e-5 m), a coarse sphere
/// (deviation ≫ 1e-5 m) is still **Violated**.
const OCCT_SOURCE_ZERO_BOUND: &str = r#"
#precision(50mm)
structure Sphere {
    let r = sphere(1000mm)
}
structure SphereCheck {
    param subject : Sphere
    constraint RepresentationWithin(subject, 0mm)
}
"#;

// ── BT6/C3: coarse sphere + tight bound → Violated ───────────────────────────

/// BT6 / C3: a sphere tessellated at coarse precision (50 mm deflection) has
/// sampled deviation >> 1 mm (1e-3 m), so `RepresentationWithin(subject, 1mm)`
/// is **Violated** after the full `tessellate_realizations → check` pipeline.
///
/// This is the headline "the assertion fires" test: non-zero bound (C3),
/// coarse subject, OCCT kernel present.
///
/// Pipeline: `set_capture_repr_tol(true)` → `tessellate_realizations` →
/// `check` → `dispatch_constraints` intercepts `RepresentationWithin` →
/// type-name scan resolves "Sphere#realization[0]" → achieved > 1mm → Violated.
#[test]
fn bt6_coarse_sphere_tight_bound_yields_violated() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping bt6_coarse_sphere_tight_bound_yields_violated: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_COARSE, "bt6_coarse");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    // Verify the map was populated (BT6 pre-condition: OCCT measured something).
    let achieved = engine.achieved_repr_tol("Sphere#realization[0]").expect(
        "BT6: coarse sphere must have Some achieved_repr_tol after tessellate_realizations",
    );
    assert!(
        achieved > 1e-3,
        "BT6 pre-condition: coarse sphere deviation ({achieved:.3e} m) must exceed \
         the 1mm (1e-3 m) bound so the assertion fires"
    );

    // Run check: tessellate_realizations populated the map; check reads it.
    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "BT6 / C3: coarse sphere (deviation {achieved:.3e} m) > 1mm bound → Violated"
    );
}

// ── BT7/C3: fine sphere + tight bound → Satisfied ────────────────────────────

/// BT7 / C3: a sphere tessellated at fine precision (`OCCT_SOURCE_FINE`, 0.3 mm
/// deflection) has sampled deviation below the 1 mm (1e-3 m) bound, so
/// `RepresentationWithin(subject, 1mm)` is **Satisfied** after the full
/// pipeline.
///
/// The measured deviation, the full precision→deviation sweep and the
/// precision-choice rationale live on `OCCT_SOURCE_FINE`. The pre-condition
/// below is the only machine-checked copy of that number.
#[test]
fn bt7_fine_sphere_tight_bound_yields_satisfied() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping bt7_fine_sphere_tight_bound_yields_satisfied: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_FINE, "bt7_fine");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    let achieved = engine
        .achieved_repr_tol("Sphere#realization[0]")
        .expect("BT7: fine sphere must have Some achieved_repr_tol after tessellate_realizations");
    // Only the CEILING is asserted. It is the half that carries contract
    // information: 8e-4 is strictly below the 1e-3 assertion bound, so clearing it
    // IMPLIES the `Satisfied` verdict asserted below, and a future OCCT build
    // eroding the 1.61x margin fails here loudly — naming the measured value —
    // instead of silently flipping to a mysterious `Violated`.
    //
    // The low side is a DIAGNOSTIC, not an assertion: a build that meshes FINER
    // than when this was tuned still yields the correct verdict, so failing the
    // gate on it would turn a fully-correct environment red for no contractual
    // reason. (An earlier revision asserted a two-sided [4e-4, 8e-4] band and did
    // exactly that.)
    if achieved < 4e-4 {
        eprintln!(
            "BT7 note: fine sphere deviation ({achieved:.3e} m) is well below the \
             6.202e-4 m measured for #precision(0.3mm) on a 1 m sphere. NOT a failure \
             — the verdict is still Satisfied — but this OCCT build meshes finer than \
             when the value was tuned, so re-measure OCCT_SOURCE_FINE's sweep before \
             relying on its numbers."
        );
    }
    assert!(
        achieved < 8e-4,
        "BT7 pre-condition: fine sphere deviation ({achieved:.3e} m) must stay under \
         the 8e-4 m ceiling measured for #precision(0.3mm) on a 1 m sphere (measured \
         6.202e-4 m — 1.61x inside the 1e-3 m assertion bound). Exceeding it means \
         this OCCT build meshes coarser than when the value was tuned and the margin \
         is eroding toward a Violated verdict. Re-measure before widening: hold \
         #precision, tighten the assertion bound to 0.01mm to force a violation, then \
         read the achieved value off the `sampled facet deviation <X> m exceeds bound` \
         message. Never widen this ceiling to or past 1e-3 — it must stay strictly \
         below the assertion bound so that clearing it implies Satisfied."
    );

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "BT7 / C3: fine sphere (deviation {achieved:.3e} m) < 1mm bound → Satisfied"
    );
}

// ── BT8/C1: no tessellation → Indeterminate ──────────────────────────────────

/// BT8 / C1: when `tessellate_realizations` is NOT called (or
/// `set_capture_repr_tol` is NOT set to `true`), `achieved_repr_tol` stays
/// empty, and the assertion is **Indeterminate** (never a false Violated).
///
/// This verifies C1: absent key ⇒ realization not run ⇒ no assertion fire.
#[test]
fn bt8_no_tessellation_yields_indeterminate() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping bt8_no_tessellation_yields_indeterminate: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_COARSE, "bt8_no_tess");
    let mut engine = make_occt_engine();
    // Deliberately skip set_capture_repr_tol + tessellate_realizations
    // → achieved_repr_tol map stays empty.

    let result = engine.check(&compiled);

    // Map is empty → key absent → Indeterminate (C1: never a false Violated).
    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "BT8 / C1: no tessellation → empty map → Indeterminate (never a false Violated)"
    );
}

// ── C4: zero bound on curved subject → Violated ──────────────────────────────

/// C4: `RepresentationWithin(subject, 0mm)` with a coarse curved sphere.
///
/// The zero-bound floor (PLANAR_FLOOR = 1e-5 m) is applied: `eff = 1e-5 m`.
/// A coarse sphere has deviation ≫ 1e-5 m, so the assertion is **Violated**.
/// This distinguishes planar (B1-validated ≤ 1e-5 m → Satisfied) from
/// curved (B2-validated ≫ 1e-5 m → Violated) under a zero bound.
#[test]
fn c4_zero_bound_coarse_sphere_yields_violated() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping c4_zero_bound_coarse_sphere_yields_violated: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_ZERO_BOUND, "c4_zero_bound");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    let achieved = engine
        .achieved_repr_tol("Sphere#realization[0]")
        .expect("C4: coarse sphere must have Some achieved_repr_tol");
    // PLANAR_FLOOR = 1e-5 m; coarse sphere must be far above it.
    assert!(
        achieved > 1e-5,
        "C4 pre-condition: coarse sphere deviation ({achieved:.3e} m) must exceed \
         PLANAR_FLOOR (1e-5 m) so zero-bound → Violated"
    );

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin, zero bound)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "C4: zero bound + coarse sphere (achieved {achieved:.3e} m >> PLANAR_FLOOR 1e-5 m) → Violated"
    );
}

// ── C2 regression: extract_output_tolerance_bound returns the bound ───────────

/// C2 regression: `extract_output_tolerance_bound` still returns the declared
/// bound from a `RepresentationWithin` constraint expression, unchanged by
/// the addition of the assertion path.
///
/// The same constraint expression BOTH drives the tessellation budget (via
/// `extract_output_tolerance_bound`) AND asserts post-realization (via
/// `eval_representation_within`).  This test pins the extractor's return value
/// so that refactoring the assertion path cannot silently break the budget.
///
/// Uses a synthetic `PersistentMap<ConstraintNodeId, ConstraintNodeData>` to
/// call `extract_output_tolerance_bound` directly (no OCCT needed).
#[test]
fn c2_extract_output_tolerance_bound_still_returns_declared_bound() {
    // Build a synthetic ConstraintNodeData carrying the canonical shape:
    // RepresentationWithin(ValueRef(subject.self):StructureRef("Sphere"), 1mm)
    // where 1mm = 1e-3 m (SI).
    let bound_si = 1e-3_f64; // 1mm in SI metres
    let subject_arg = CompiledExpr::value_ref(
        reify_core::ValueCellId::new("subject", "self"),
        Type::StructureRef("Sphere".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        reify_ir::Value::Scalar {
            si_value: bound_si,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let expr = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );

    let entity = "SphereCheck";
    let index = 0u32;
    let id = ConstraintNodeId::new(entity, index);
    let data = ConstraintNodeData {
        id: id.clone(),
        label: None,
        expr,
        content_hash: ContentHash::of_str(&format!("{}#constraint[{}]", entity, index)),
        optimized_target: None,
    };

    let mut constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData> =
        PersistentMap::default();
    constraints.insert(id, data);

    // C2 regression: extract_output_tolerance_bound returns Some(1e-3) for
    // entity "SphereCheck" — the budget path is unaffected by the assertion path.
    let extracted = extract_output_tolerance_bound(&constraints, "SphereCheck");
    assert_eq!(
        extracted,
        Some(bound_si),
        "C2: extract_output_tolerance_bound must return the declared bound ({bound_si:.3e} m) \
         unchanged — the budget path must not be affected by the assertion interception"
    );

    // Also verify it returns None for an unrelated entity (gate 1 is still active).
    let not_found = extract_output_tolerance_bound(&constraints, "OtherEntity");
    assert_eq!(
        not_found, None,
        "C2: extract_output_tolerance_bound must return None for an unrelated entity"
    );
}
