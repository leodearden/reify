//! process-DFM δ end-to-end CI test (task #4275, PRD §7).
//!
//! **(a) ALWAYS-ON** (no kernel):
//!   - `std_process_dfm_compiles_error_clean` — the example parses and compiles
//!     with no Error-severity diagnostics.
//!   - `std_process_dfm_scalar_constraints_satisfied_violated` — under a no-kernel
//!     `make_simple_engine().check()`, the SCALAR DFM constraints (Manufacturable,
//!     BendManufacturable, DrawManufacturable, DraftManufacturable) report the
//!     expected Satisfied/Violated set. The geometry-backed FitsBuildVolume and
//!     FeatureManufacturable constraints are Indeterminate without a kernel and are
//!     intentionally NOT asserted here.
//!   - `std_process_dfm_build_volume_constraints_declared` — the two FitsBuildVolume
//!     constraint entries are present (as Indeterminate) in the no-kernel result.
//!
//! **(b) OCCT-GATED partial proof (degraded contract)** —
//!   `std_process_dfm_build_volume_flip_pinned`: the constraint Satisfied/Violated flip
//!   WORKS under the default UnifiedDag scheduler (Stage-4 cutover #4362 ι), and at
//!   least one diagnostic IS emitted. Does NOT assert DFM-specific codes — those are
//!   deferred to #4727. Self-skips without OCCT.
//!
//! **(c) FULL GATE (DFM diagnostic routing fixed, task #4734)** —
//!   `std_process_dfm_build_volume_flip_and_severity_diagnostics`: asserts that
//!   `W/E/I_DFM_BUILD_VOLUME` diagnostics ARE emitted under the default UnifiedDag
//!   scheduler, that NO generic `ConstraintViolated` referencing `FitsBuildVolume` is
//!   emitted (replaced by the W_DFM Warning), and that NO spurious
//!   `TopologyAttributeLocalIndexReassigned` warning is emitted on a plain-box build
//!   (regression from the unified-DAG edge-centroid path, fixed in #4734).
//!
//! The example is also covered for compile-cleanliness by
//! `crates/reify-compiler/tests/examples_smoke.rs` (directory walk).

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::{ConstraintCheckEntry, Engine};
use reify_ir::{ExportFormat, Satisfaction};
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

const STD_PROCESS_DFM_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/process/std_process_dfm.ri"
);

fn read_dfm_example() -> String {
    std::fs::read_to_string(STD_PROCESS_DFM_PATH)
        .expect("examples/process/std_process_dfm.ri should exist (task 4275 step-2)")
}

// ── (a) ALWAYS-ON: compile-cleanliness ───────────────────────────────────────

/// ALWAYS-ON: example file parses and compiles with no Error-severity diagnostics.
///
/// RED: the example file does not exist yet → `read_to_string` panics.
/// GREEN: `examples/process/std_process_dfm.ri` is authored (step-2).
#[test]
fn std_process_dfm_compiles_error_clean() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/process/std_process_dfm.ri should compile with no Error-severity \
         diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );
}

// ── (a) ALWAYS-ON: scalar DFM constraint satisfaction ────────────────────────

/// ALWAYS-ON: scalar DFM constraints report the expected Satisfied/Violated set
/// under `make_simple_engine().check()` (no geometry kernel required).
///
/// Asserts at minimum one Satisfied and one Violated entry for each of:
/// `Manufacturable`, `BendManufacturable`, `DrawManufacturable`, `DraftManufacturable`.
///
/// Does NOT assert global eval-Error-cleanliness: geometry-backed constraints
/// (FitsBuildVolume, FeatureManufacturable) are Indeterminate without a kernel;
/// direct `fits_build_volume(bounding_box(box(...)), ...)` let-bindings yield
/// `Value::Undef` (kernel absent → `bounding_box` unresolvable) and emit advisory
/// `E_DFM_BUILD_VOLUME` diagnostics that do NOT constitute a test failure here.
#[test]
fn std_process_dfm_scalar_constraints_satisfied_violated() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    // Belt-and-suspenders compile guard (canonical check is `std_process_dfm_compiles_error_clean`).
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    let mut engine = make_simple_engine();
    let result = engine.check(&compiled);

    // Helper: collect constraint results whose label starts with `def_name`.
    // Relies on the label format "DefName#inst[pred]" from the Reify compiler.
    // `starts_with("Manufacturable")` matches ONLY the pure Manufacturable def
    // (BendManufacturable starts with "Bend", DrawManufacturable with "Draw", etc.).
    let entries_for = |def_name: &str| -> Vec<_> {
        result
            .constraint_results
            .iter()
            .filter(|e| {
                e.label
                    .as_deref()
                    .map(|l| l.starts_with(def_name))
                    .unwrap_or(false)
            })
            .collect()
    };

    // ── Manufacturable ────────────────────────────────────────────────────────
    {
        let mfg = entries_for("Manufacturable");
        assert!(
            !mfg.is_empty(),
            "expected Manufacturable constraint results; got none (check example file)"
        );
        assert!(
            mfg.iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one Manufacturable Satisfied; got:\n{:#?}",
            mfg.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            mfg.iter().any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one Manufacturable Violated; got:\n{:#?}",
            mfg.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }

    // ── BendManufacturable ────────────────────────────────────────────────────
    {
        let bend = entries_for("BendManufacturable");
        assert!(
            !bend.is_empty(),
            "expected BendManufacturable constraint results; got none"
        );
        assert!(
            bend.iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one BendManufacturable Satisfied; got:\n{:#?}",
            bend.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            bend.iter()
                .any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one BendManufacturable Violated; got:\n{:#?}",
            bend.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }

    // ── DrawManufacturable ────────────────────────────────────────────────────
    {
        let draw = entries_for("DrawManufacturable");
        assert!(
            !draw.is_empty(),
            "expected DrawManufacturable constraint results; got none"
        );
        assert!(
            draw.iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one DrawManufacturable Satisfied; got:\n{:#?}",
            draw.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            draw.iter()
                .any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one DrawManufacturable Violated; got:\n{:#?}",
            draw.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }

    // ── DraftManufacturable ───────────────────────────────────────────────────
    {
        let draft = entries_for("DraftManufacturable");
        assert!(
            !draft.is_empty(),
            "expected DraftManufacturable constraint results; got none"
        );
        assert!(
            draft
                .iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one DraftManufacturable Satisfied; got:\n{:#?}",
            draft
                .iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            draft
                .iter()
                .any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one DraftManufacturable Violated; got:\n{:#?}",
            draft
                .iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }
}

// ── (b) OCCT-GATED: build-volume flip + severity bridge ──────────────────────

/// Helper: find the first constraint entry in `results` matching `entity` +
/// label starting with `label_prefix`.
fn find_fvb_entry(
    results: &[ConstraintCheckEntry],
    entity: &str,
    label_prefix: &str,
) -> Option<ConstraintCheckEntry> {
    results
        .iter()
        .find(|e| {
            e.id.entity == entity
                && e.label
                    .as_deref()
                    .map(|l| l.starts_with(label_prefix))
                    .unwrap_or(false)
        })
        .cloned()
}

/// ALWAYS-ON presence guard: the two `FitsBuildVolume` constraint entries
/// (`FittingPart`, `OversizedPart`) appear in the no-kernel `check()` result as
/// `Satisfaction::Indeterminate`. This proves the example declares the
/// build-volume constraints without requiring a geometry kernel on the CI host.
///
/// The geometry-backed OK→VIOLATED *flip* and the severity-tagged diagnostics
/// are exercised by `std_process_dfm_build_volume_flip_and_severity_diagnostics`
/// (OCCT-gated, fixed by #4734).
#[test]
fn std_process_dfm_build_volume_constraints_declared() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    let mut no_kernel_engine = make_simple_engine();
    let no_kernel_result = no_kernel_engine.check(&compiled);

    assert!(
        find_fvb_entry(
            &no_kernel_result.constraint_results,
            "FittingPart",
            "FitsBuildVolume"
        )
        .is_some(),
        "expected FittingPart FitsBuildVolume constraint entry in no-kernel result; \
         check the build-volume section of examples/process/std_process_dfm.ri; got:\n{:#?}",
        no_kernel_result.constraint_results
    );
    assert!(
        find_fvb_entry(
            &no_kernel_result.constraint_results,
            "OversizedPart",
            "FitsBuildVolume"
        )
        .is_some(),
        "expected OversizedPart FitsBuildVolume constraint entry in no-kernel result; \
         got:\n{:#?}",
        no_kernel_result.constraint_results
    );
}

/// OCCT-GATED partial proof: constraint flip + at least one diagnostic emitted
/// under the default `UnifiedDag` scheduler (Stage-4 cutover, task ι #4362).
///
/// Pins the contract that the Stage-4 cutover delivers:
/// - `FittingPart.FitsBuildVolume` → `Satisfaction::Satisfied`
/// - `OversizedPart.FitsBuildVolume` → `Satisfaction::Violated`
/// - At least one diagnostic is emitted.
///
/// Self-skips without OCCT. The full severity-bridge test (with DFM-code assertions
/// and no-spurious-topology assertions, fixed by #4734) is
/// `std_process_dfm_build_volume_flip_and_severity_diagnostics`.
#[test]
fn std_process_dfm_build_volume_flip_pinned() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping OCCT-gated build-volume flip assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Constraint flip — the primary deliverable of the Stage-4 default cutover.
    let fitting = find_fvb_entry(&result.constraint_results, "FittingPart", "FitsBuildVolume")
        .unwrap_or_else(|| {
            panic!(
                "FittingPart FitsBuildVolume absent; got:\n{:#?}",
                result.constraint_results
            )
        });
    assert_eq!(
        fitting.satisfaction,
        Satisfaction::Satisfied,
        "FittingPart (100×100×150 mm) should fit the 220×220×250 mm envelope; got {:?}",
        fitting.satisfaction
    );

    let oversized =
        find_fvb_entry(&result.constraint_results, "OversizedPart", "FitsBuildVolume")
            .unwrap_or_else(|| {
                panic!(
                    "OversizedPart FitsBuildVolume absent; got:\n{:#?}",
                    result.constraint_results
                )
            });
    assert_eq!(
        oversized.satisfaction,
        Satisfaction::Violated,
        "OversizedPart (250×100×100 mm) should exceed the 220 mm X-axis envelope; got {:?}",
        oversized.satisfaction
    );

    // Degraded-contract pin: the violated constraint emits at least one diagnostic
    // under the default UnifiedDag scheduler, even though DFM-specific codes are not
    // yet routed correctly (tracked by #4727).
    assert!(
        !result.diagnostics.is_empty(),
        "expected at least one diagnostic from the violated FitsBuildVolume constraint \
         under the default UnifiedDag scheduler; got none; \
         diagnostics:\n{:#?}",
        result.diagnostics
    );
}

/// FULL GATE (DFM diagnostic routing fixed, task #4734): the geometry-backed
/// `FitsBuildVolume` OK→VIOLATED flip and the severity-tagged DFM diagnostics under
/// the default UnifiedDag scheduler.
///
/// Fixed by task #4734 (DFM build-volume diagnostic routing + spurious
/// TopologyAttributeLocalIndexReassigned). This task:
/// - Routes `W/E/I_DFM_BUILD_VOLUME` diagnostics from `DFMSeverityBridge` let-cells
///   and from `OversizedPart`'s `FitsBuildVolume` constraint predicate via a
///   post-geometry harvest pass in `engine_build.rs` and `engine_constraints.rs`.
/// - Suppresses the generic `ConstraintViolated` for constraints whose predicate emits
///   a DFM diagnostic (so the violation surfaces as `W_DFM_BUILD_VOLUME` Warning, not
///   as a generic Error).
/// - Fixes spurious `TopologyAttributeLocalIndexReassigned` Warnings emitted per box
///   realization on the unified-DAG path (edge centroids were degenerate on that path).
///
/// Asserts (with OCCT):
/// - `FittingPart`'s `FitsBuildVolume` → `Satisfaction::Satisfied`
///   (100×100×150 mm part fits the 220×220×250 mm FDM build envelope).
/// - `OversizedPart`'s `FitsBuildVolume` → `Satisfaction::Violated`
///   (250 mm on X exceeds the 220 mm envelope).
/// - `result.diagnostics` contains a `Severity::Warning` with `"W_DFM_BUILD_VOLUME"`
///   (default 2-arg `FitsBuildVolume` violation from `OversizedPart`).
/// - `result.diagnostics` contains a `Severity::Error` with `"E_DFM_BUILD_VOLUME"`
///   (3-arg `DFMSeverity.Error` direct call in `DFMSeverityBridge`).
/// - `result.diagnostics` contains a `Severity::Info` with `"I_DFM_BUILD_VOLUME"`
///   (3-arg `DFMSeverity.Info` direct call in `DFMSeverityBridge`).
/// - NO `DiagnosticCode::ConstraintViolated` diagnostic whose message contains
///   `"FitsBuildVolume"` (the OversizedPart violation surfaces as W_DFM not generic Error).
/// - NO `DiagnosticCode::TopologyAttributeLocalIndexReassigned` diagnostic
///   (plain-box builds must not emit spurious tied-index warnings).
#[test]
fn std_process_dfm_build_volume_flip_and_severity_diagnostics() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    // OCCT-GATED: build-volume flip and severity diagnostics.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping OCCT-gated FitsBuildVolume assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // FittingPart: 100×100×150 mm fits the 220×220×250 mm FDM build envelope.
    let fitting = find_fvb_entry(&result.constraint_results, "FittingPart", "FitsBuildVolume")
        .unwrap_or_else(|| {
            panic!(
                "FittingPart FitsBuildVolume absent from build result; got:\n{:#?}",
                result.constraint_results
            )
        });
    assert_eq!(
        fitting.satisfaction,
        Satisfaction::Satisfied,
        "FittingPart (100×100×150 mm) should fit the 220×220×250 mm envelope; got {:?}",
        fitting.satisfaction
    );

    // OversizedPart: 250 mm on X exceeds the 220 mm envelope.
    let oversized = find_fvb_entry(
        &result.constraint_results,
        "OversizedPart",
        "FitsBuildVolume",
    )
    .unwrap_or_else(|| {
        panic!(
            "OversizedPart FitsBuildVolume absent from build result; got:\n{:#?}",
            result.constraint_results
        )
    });
    assert_eq!(
        oversized.satisfaction,
        Satisfaction::Violated,
        "OversizedPart (250×100×100 mm) should exceed the 220 mm X-axis envelope; got {:?}",
        oversized.satisfaction
    );

    // W_DFM_BUILD_VOLUME — from OversizedPart's 2-arg FitsBuildVolume (default Warning).
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message.contains("W_DFM_BUILD_VOLUME")),
        "expected a Warning diagnostic containing 'W_DFM_BUILD_VOLUME' \
         (from OversizedPart FitsBuildVolume); diagnostics:\n{:#?}",
        result.diagnostics
    );

    // E_DFM_BUILD_VOLUME — from DFMSeverityBridge DFMSeverity.Error direct call.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("E_DFM_BUILD_VOLUME")),
        "expected an Error diagnostic containing 'E_DFM_BUILD_VOLUME' \
         (from DFMSeverityBridge DFMSeverity.Error); diagnostics:\n{:#?}",
        result.diagnostics
    );

    // I_DFM_BUILD_VOLUME — from DFMSeverityBridge DFMSeverity.Info direct call.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Info && d.message.contains("I_DFM_BUILD_VOLUME")),
        "expected an Info diagnostic containing 'I_DFM_BUILD_VOLUME' \
         (from DFMSeverityBridge DFMSeverity.Info); diagnostics:\n{:#?}",
        result.diagnostics
    );

    // No generic ConstraintViolated for FitsBuildVolume — the OversizedPart violation
    // must surface as W_DFM_BUILD_VOLUME (Warning), not as a generic ConstraintViolated Error.
    // Scoped to FitsBuildVolume so the legitimate scalar ConstraintViolated entries are unaffected.
    let fvb_constraint_violated: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ConstraintViolated)
                && d.message.contains("FitsBuildVolume")
        })
        .collect();
    assert!(
        fvb_constraint_violated.is_empty(),
        "expected NO generic ConstraintViolated diagnostic referencing 'FitsBuildVolume' \
         (OversizedPart violation must surface as W_DFM_BUILD_VOLUME Warning, not generic Error); \
         got:\n{:#?}",
        fvb_constraint_violated
    );

    // No spurious TopologyAttributeLocalIndexReassigned — plain-box builds must not
    // emit tied-index warnings (empirically 0 under legacy; unified regression fixed by #4734).
    let topology_reassigned: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned))
        .collect();
    assert!(
        topology_reassigned.is_empty(),
        "expected NO TopologyAttributeLocalIndexReassigned diagnostics on plain-box build \
         (spurious unified-DAG edge-centroid regression, fixed by #4734); \
         got:\n{:#?}",
        topology_reassigned
    );
}

// ── (d) FOCUSED CONSTRAINT ROUTING TEST (A2/A3 isolation) ────────────────────

/// OCCT-GATED focused test (task #4734 step-3 / A2+A3): isolates the W_DFM routing
/// for the OversizedPart FitsBuildVolume constraint predicate.
///
/// Under the unfixed unified path, `OversizedPart`'s 2-arg
/// `constraint FitsBuildVolume(proc, part)` emits a generic
/// `ConstraintViolated` Error (not `W_DFM_BUILD_VOLUME` Warning) because
/// `check_constraints_post_geometry` runs the constraint checker with a
/// sink-less `EvalContext` — `emit_dfm_diagnostics` fires but there is no sink
/// to collect it.
///
/// Fixed by task #4734 step-4: harvest DFM diagnostics from the folded constraint
/// predicate and suppress the generic `ConstraintViolated` for constraints that
/// emitted a DFM diagnostic.
///
/// Asserts (with OCCT, under the default UnifiedDag scheduler):
/// - `result.diagnostics` contains a `Severity::Warning` with `"W_DFM_BUILD_VOLUME"`
///   (OversizedPart 2-arg FitsBuildVolume default-Warning violation).
/// - `result.diagnostics` contains NO `DiagnosticCode::ConstraintViolated` whose
///   message contains `"FitsBuildVolume"` (scoped: legitimate scalar ConstraintViolated
///   entries are unaffected).
///
/// Self-skips without OCCT.
#[test]
fn std_process_dfm_build_volume_constraint_routing_w_dfm() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping OCCT-gated W_DFM_BUILD_VOLUME constraint routing assertions: OCCT not available"
        );
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // W_DFM_BUILD_VOLUME — from OversizedPart 2-arg FitsBuildVolume (default Warning).
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message.contains("W_DFM_BUILD_VOLUME")),
        "expected a Warning containing 'W_DFM_BUILD_VOLUME' \
         (OversizedPart FitsBuildVolume 2-arg default-Warning); diagnostics:\n{:#?}",
        result.diagnostics
    );

    // No generic ConstraintViolated for FitsBuildVolume — must surface as W_DFM Warning.
    let fvb_cv: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ConstraintViolated)
                && d.message.contains("FitsBuildVolume")
        })
        .collect();
    assert!(
        fvb_cv.is_empty(),
        "expected NO generic ConstraintViolated for FitsBuildVolume (must surface as W_DFM); \
         got:\n{:#?}",
        fvb_cv
    );
}
